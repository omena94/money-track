//! 家庭檢視：擁有者授權某個 Email 唯讀查看自己的全部資料；檢視者可把多人合併加總，
//! 並查看成員近期要付的固定支出與信用卡帳單。
//!
//! 檢視單一成員時，前端在 GET 請求帶 `x-view-owner`，由 `Identity` 萃取器驗證授權後改以
//! 該成員身分讀取；非 GET 請求帶此標頭一律拒絕，因此檢視他人時不可能寫入。
use crate::{
    auth::Identity,
    error::{ApiError, Result},
    finance::{FEED_SQL, NOT_FLOW},
    networth, recurring,
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

const MONTHS: i32 = 12;

#[derive(sqlx::FromRow)]
struct Person {
    id: Uuid,
    email: String,
    name: String,
}

/// 授權檢視者可查看 owner 的資料嗎？以檢視者身分查詢，受 RLS 保護。
pub async fn can_view(s: &AppState, viewer: &Identity, owner: Uuid) -> Result<bool> {
    let mut tx = tenant(&s.db, viewer.id, &viewer.email).await?;
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM household_viewers WHERE owner_id=$1 AND viewer_email=$2)",
    )
    .bind(owner)
    .bind(&viewer.email)
    .fetch_one(&mut *tx)
    .await?)
}

async fn people(s: &AppState, ids: &[Uuid]) -> Result<Vec<(Uuid, String, String)>> {
    let rows: Vec<Person> = sqlx::query_as(
        "SELECT id,email,coalesce(display_name,name) AS name FROM users WHERE id=ANY($1)",
    )
    .bind(ids)
    .fetch_all(&s.auth_db)
    .await?;
    Ok(rows.into_iter().map(|p| (p.id, p.email, p.name)).collect())
}

/// 我能檢視的成員（含自己，自己排第一）。
async fn members(s: &AppState, user: &Identity) -> Result<Vec<(Uuid, String, String)>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let owners: Vec<Uuid> = sqlx::query_scalar(
        "SELECT owner_id FROM household_viewers WHERE viewer_email=$1 AND owner_id<>$2 ORDER BY created_at",
    )
    .bind(&user.email)
    .bind(user.id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let found = people(s, &owners).await?;
    let mut out = vec![(user.id, user.email.clone(), user.name.clone())];
    for id in owners {
        if let Some(p) = found.iter().find(|p| p.0 == id) {
            out.push(p.clone());
        }
    }
    Ok(out)
}

/// GET /household：我授權的檢視者，以及授權給我的人。
pub async fn overview(user: Identity, State(s): State<AppState>) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let viewers: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'email',viewer_email,'created_at',created_at) FROM household_viewers WHERE owner_id=$1 ORDER BY created_at")
        .bind(user.id)
        .fetch_all(&mut *tx)
        .await?;
    let incoming: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT id,owner_id FROM household_viewers WHERE viewer_email=$1 AND owner_id<>$2 ORDER BY created_at")
        .bind(&user.email)
        .bind(user.id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let ids: Vec<Uuid> = incoming.iter().map(|r| r.1).collect();
    let found = people(&s, &ids).await?;
    let avatars: Vec<(Uuid, Option<Value>)> =
        sqlx::query_as("SELECT id,avatar FROM users WHERE id=ANY($1)")
            .bind(&ids)
            .fetch_all(&s.auth_db)
            .await?;
    let avatar_of = |id: &Uuid| {
        avatars
            .iter()
            .find(|a| a.0 == *id)
            .and_then(|a| a.1.clone())
    };
    let shared: Vec<Value> = incoming
        .iter()
        .filter_map(|(grant, owner)| {
            let p = found.iter().find(|p| p.0 == *owner)?;
            Some(json!({"grant_id": grant, "id": owner, "email": p.1, "name": p.2, "avatar": avatar_of(owner)}))
        })
        .collect();
    Ok(Json(json!({
        "me": {"id": user.id, "email": user.email, "name": user.name, "avatar": user.avatar},
        "viewers": viewers,
        "shared_with_me": shared,
    })))
}

#[derive(Deserialize)]
pub struct NewViewer {
    /// 直接輸入 Email（對方可以還沒註冊）
    email: Option<String>,
    /// 從搜尋結果選擇的使用者；Email 由伺服器查出，前端只看到遮蔽後的 Email
    user_id: Option<Uuid>,
}

/// Email 遮蔽：de***7@example.com
fn mask_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return "***".into();
    };
    let chars: Vec<char> = local.chars().collect();
    let head: String = chars
        .iter()
        .take(2.min(chars.len().saturating_sub(1)))
        .collect();
    let tail = if chars.len() > 3 {
        chars.last().map(|c| c.to_string()).unwrap_or_default()
    } else {
        String::new()
    };
    format!("{head}***{tail}@{domain}")
}

#[derive(Deserialize)]
pub struct PeopleQuery {
    q: Option<String>,
}
/// GET /household/people?q=：用姓名或 Email 模糊搜尋已註冊的使用者，給分享設定選人用。
/// 至少 2 個字、最多 8 筆，不含自己與測試帳號，Email 遮蔽後回傳。
pub async fn people_search(
    user: Identity,
    State(s): State<AppState>,
    Query(q): Query<PeopleQuery>,
) -> Result<Json<Vec<Value>>> {
    let needle = q.q.unwrap_or_default().trim().to_lowercase();
    if needle.chars().count() < 2 || needle.chars().count() > 80 {
        return Ok(Json(vec![]));
    }
    // LIKE 萬用字元當一般文字比對
    let pattern = format!(
        "%{}%",
        needle
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows: Vec<(Uuid, String, String, Option<Value>)> = sqlx::query_as("SELECT id,coalesce(display_name,name),lower(email),avatar FROM users WHERE id<>$1 AND lower(email)<>$3 AND lower(email) NOT LIKE '%@example.invalid' AND (lower(coalesce(display_name,name)) LIKE $2 OR lower(name) LIKE $2 OR lower(email) LIKE $2) ORDER BY (lower(coalesce(display_name,name))=$4 OR lower(email)=$4) DESC,2 LIMIT 8")
        .bind(user.id)
        .bind(&pattern)
        .bind(user.email.to_lowercase())
        .bind(&needle)
        .fetch_all(&s.auth_db)
        .await?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let existing: Vec<String> =
        sqlx::query_scalar("SELECT viewer_email FROM household_viewers WHERE owner_id=$1")
            .bind(user.id)
            .fetch_all(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, email, avatar)| {
                json!({
                    "id": id,
                    "name": name,
                    "avatar": avatar,
                    "email": mask_email(&email),
                    "added": existing.contains(&email),
                })
            })
            .collect(),
    ))
}
/// POST /household/viewers：授權某個 Email 查看我的全部資料。
pub async fn add_viewer(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<NewViewer>,
) -> Result<(StatusCode, Json<Value>)> {
    let email = match (v.user_id, v.email.as_deref()) {
        (Some(id), _) => {
            sqlx::query_scalar::<_, String>("SELECT lower(email) FROM users WHERE id=$1")
                .bind(id)
                .fetch_optional(&s.auth_db)
                .await?
                .ok_or_else(ApiError::missing)?
        }
        (None, Some(e)) => e.trim().to_lowercase(),
        (None, None) => return Err(ApiError::bad("請輸入 Email 或選擇成員")),
    };
    let valid = email.len() <= 254
        && email.split_once('@').is_some_and(|(a, d)| {
            !a.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
        })
        && !email.chars().any(|c| c.is_whitespace() || c.is_control());
    if !valid {
        return Err(ApiError::bad("請輸入正確的 Email"));
    }
    if email == user.email {
        return Err(ApiError::bad("不需要分享給自己"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let id = Uuid::new_v4();
    let r = sqlx::query("INSERT INTO household_viewers(id,owner_id,viewer_email) VALUES($1,$2,$3) ON CONFLICT(owner_id,viewer_email) DO NOTHING")
        .bind(id)
        .bind(user.id)
        .bind(&email)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError(StatusCode::CONFLICT, "這個 Email 已經可以查看"));
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"id": id}))))
}

/// DELETE /household/viewers/{id}：擁有者撤銷，或檢視者自行退出。
pub async fn remove_viewer(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r = sqlx::query(
        "DELETE FROM household_viewers WHERE id=$1 AND (owner_id=$2 OR viewer_email=$3)",
    )
    .bind(id)
    .bind(user.id)
    .bind(&user.email)
    .execute(&mut *tx)
    .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct SummaryQuery {
    /// 逗號分隔的成員 id；省略時為全部可檢視成員。
    members: Option<String>,
    /// YYYY-MM；省略時為本月。
    month: Option<String>,
}

/// 依查詢參數挑出成員；只能挑選自己可檢視的人，其餘一律視為不存在。
fn choose(
    all: Vec<(Uuid, String, String)>,
    list: Option<&str>,
) -> Result<Vec<(Uuid, String, String)>> {
    let Some(list) = list.filter(|m| !m.is_empty()) else {
        return Ok(all);
    };
    let mut ids = Vec::new();
    for part in list.split(',').take(20) {
        let id: Uuid = part
            .trim()
            .parse()
            .map_err(|_| ApiError::bad("成員格式錯誤"))?;
        if !all.iter().any(|m| m.0 == id) {
            return Err(ApiError::missing());
        }
        ids.push(id);
    }
    Ok(all.into_iter().filter(|m| ids.contains(&m.0)).collect())
}

fn dec(v: &Value, k: &str) -> Decimal {
    v.get(k)
        .and_then(|x| {
            x.as_str()
                .map(str::to_owned)
                .or_else(|| x.as_i64().map(|n| n.to_string()))
        })
        .and_then(|x| x.parse().ok())
        .unwrap_or_default()
}

fn taipei_today() -> NaiveDate {
    chrono::Utc::now()
        .with_timezone(&chrono_tz_taipei())
        .date_naive()
}

/// 家庭合併用的一筆收支（只含 TWD、非待入帳）。
#[derive(sqlx::FromRow, Clone)]
struct Flow {
    id: Uuid,
    account_name: Option<String>,
    occurred_on: NaiveDate,
    amount: Decimal,
    kind: String,
    category: String,
    description: String,
    /// 是否計入收支（非轉帳、非信用卡繳款、非投資等資金移動分類）
    counted: bool,
    /// 同步帳戶的識別（來源＋末四碼）；手動帳戶為 None
    account_key: Option<String>,
    /// 看起來像轉帳：資金移動分類、來源標示轉帳對象或摘要含轉帳字樣
    transferish: bool,
}

/// 共同帳戶去重：同一個同步帳戶出現在多位成員帳本時，相同日期、金額、摘要的交易只算一次。
/// 回傳每筆是否為重複。同一位成員自己的相同交易（例如同天兩杯咖啡）不受影響。
fn mark_duplicates(members: &[Vec<Flow>]) -> Vec<Vec<bool>> {
    let mut kept: std::collections::HashMap<(String, NaiveDate, Decimal, String), usize> =
        Default::default();
    let mut out = Vec::with_capacity(members.len());
    for rows in members {
        let mut mine: std::collections::HashMap<(String, NaiveDate, Decimal, String), usize> =
            Default::default();
        let flags = rows
            .iter()
            .map(|r| {
                let Some(key) = &r.account_key else {
                    return false;
                };
                let k = (key.clone(), r.occurred_on, r.amount, r.description.clone());
                let n = mine.entry(k.clone()).or_default();
                *n += 1;
                *n <= kept.get(&k).copied().unwrap_or(0)
            })
            .collect();
        for (k, n) in mine {
            let e = kept.entry(k).or_default();
            *e = (*e).max(n);
        }
        out.push(flags);
    }
    out
}

/// 家庭內互轉：一位成員轉出、另一位成員在 3 天內轉入相同金額，且至少一邊看起來像轉帳。
/// 以日期最接近者優先配對，每筆最多配對一次。回傳 (轉出成員, 轉出列, 轉入成員, 轉入列)。
fn match_transfers(members: &[Vec<Flow>], skip: &[Vec<bool>]) -> Vec<(usize, usize, usize, usize)> {
    let mut outs = Vec::new();
    let mut ins = Vec::new();
    for (m, rows) in members.iter().enumerate() {
        for (i, r) in rows.iter().enumerate() {
            if skip[m][i] || r.amount.is_zero() {
                continue;
            }
            if r.amount.is_sign_negative() {
                outs.push((m, i));
            } else {
                ins.push((m, i));
            }
        }
    }
    let mut candidates = Vec::new();
    for &(om, oi) in &outs {
        let o = &members[om][oi];
        for &(im, ii) in &ins {
            let n = &members[im][ii];
            if om == im || -o.amount != n.amount || !(o.transferish || n.transferish) {
                continue;
            }
            let gap = (n.occurred_on - o.occurred_on).num_days().abs();
            if gap <= 3 {
                candidates.push((gap, o.occurred_on, (om, oi, im, ii)));
            }
        }
    }
    candidates.sort_by_key(|c| (c.0, c.1));
    let mut used = std::collections::HashSet::new();
    let mut pairs = Vec::new();
    for (_, _, (om, oi, im, ii)) in candidates {
        if used.contains(&(om, oi)) || used.contains(&(im, ii)) {
            continue;
        }
        used.insert((om, oi));
        used.insert((im, ii));
        pairs.push((om, oi, im, ii));
    }
    pairs
}

/// 一位成員 [from, to) 期間的 TWD 收支明細（非待入帳）；`tx` 必須是該成員的 tenant。
async fn member_flows(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<Flow>> {
    Ok(sqlx::query_as(&format!("{FEED_SQL} SELECT feed.id,feed.account_name,feed.occurred_on,feed.amount,feed.kind,feed.category,coalesce(feed.description,'') AS description,
 ({NOT_FLOW} AND feed.kind IN ('income','expense','refund')) AS counted,
 CASE WHEN sa.data->>'mask' IS NOT NULL THEN c.connector||':'||(sa.data->>'mask') END AS account_key,
 (NOT ({NOT_FLOW}) OR feed.source_details->>'transfer_peer' IS NOT NULL OR coalesce(feed.description,'') ~ '轉帳|轉入|轉出|匯款|匯入|跨行|轉存') AS transferish
 FROM feed LEFT JOIN source_records sa ON sa.id=feed.account_id AND sa.owner_id=$1 AND sa.kind='account'
 LEFT JOIN connections c ON c.id=sa.connection_id AND c.owner_id=$1
 WHERE feed.status<>'pending' AND feed.currency='TWD' AND feed.occurred_on>=$2 AND feed.occurred_on<$3
 ORDER BY feed.occurred_on,feed.sort_time,feed.id"))
        .bind(id).bind(from).bind(to).fetch_all(&mut **tx).await?)
}

/// 家庭合併時不計的列：共同帳戶重複（dup）與家庭內互轉（pairs）；skip 為兩者聯集。
fn exclusions(flows: &[Vec<Flow>]) -> (Vec<Vec<bool>>, Vec<(usize, usize, usize, usize)>, Vec<Vec<bool>>) {
    let dup = mark_duplicates(flows);
    let pairs = if flows.len() > 1 {
        match_transfers(flows, &dup)
    } else {
        vec![]
    };
    let mut skip = dup.clone();
    for &(om, oi, im, ii) in &pairs {
        skip[om][oi] = true;
        skip[im][ii] = true;
    }
    (dup, pairs, skip)
}

fn is_expense(kind: &str) -> bool {
    matches!(kind, "expense" | "refund")
}

/// 帳本每月收入與支出（支出為正數）。
fn add_flow(t: &mut (Decimal, Decimal), r: &Flow) {
    if r.kind == "income" {
        t.0 += r.amount;
    } else if is_expense(&r.kind) {
        t.1 -= r.amount;
    }
}

fn days_in_month(first: NaiveDate) -> u32 {
    (shift_months(first, 1) - first).num_days() as u32
}

/// GET /household/summary：合併多位成員的淨資產、每月收支、本月分類支出、上月同期比較與預算（只計 TWD）。
/// 家庭內互轉會從合併收支排除；同一同步帳戶出現在多人帳本時，交易只算一次。
pub async fn summary(
    user: Identity,
    State(s): State<AppState>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<Value>> {
    let chosen = choose(members(&s, &user).await?, q.members.as_deref())?;
    let today = taipei_today();
    let month = match q.month.as_deref() {
        Some(m) => NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d")
            .map_err(|_| ApiError::bad("月份格式錯誤"))?,
        None => NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap(),
    };
    let start = shift_months(month, -(MONTHS - 1));
    let end = shift_months(month, 1);
    let this = month.format("%Y-%m").to_string();
    let prev = shift_months(month, -1);
    let month_days = days_in_month(month);
    // 本月尚未結束時只和上月的同一段日期比較，避免月初與上月整月比而誤判。
    let through = if today >= month && today < end {
        today.day()
    } else {
        month_days
    };
    let prev_through = through.min(days_in_month(prev));

    let mut people_out = Vec::new();
    let mut history: BTreeMap<String, [i64; 6]> = BTreeMap::new();
    let mut net = Decimal::ZERO;
    let mut assets = Decimal::ZERO;
    let mut liabilities = Decimal::ZERO;
    let keys = ["bank", "time_deposit", "investment", "other", "debt"];
    let mut flows: Vec<Vec<Flow>> = Vec::new();
    let mut budgets: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut foreign = 0i64;
    let mut account_owners: BTreeMap<String, (String, Vec<usize>)> = BTreeMap::new();

    for (idx, (id, email, name)) in chosen.iter().enumerate() {
        let worth = networth::compute(&s.db, *id, email).await?;
        let m_net = dec(&worth, "net");
        net += m_net;
        assets += dec(&worth, "assets");
        liabilities += dec(&worth, "liabilities");
        for p in worth["history"].as_array().into_iter().flatten() {
            let Some(d) = p["date"].as_str() else {
                continue;
            };
            let e = history.entry(d.to_owned()).or_default();
            for (i, k) in keys.iter().enumerate() {
                e[i] += p[k].as_i64().unwrap_or_default();
            }
            e[5] += 1;
        }

        let mut tx = tenant(&s.db, *id, email).await?;
        let rows = member_flows(&mut tx, *id, start, end).await?;
        foreign += sqlx::query_scalar::<_, i64>(&format!("{FEED_SQL} SELECT count(*) FROM feed WHERE status<>'pending' AND {NOT_FLOW} AND currency<>'TWD' AND occurred_on>=$2 AND occurred_on<$3"))
            .bind(id).bind(month).bind(end).fetch_one(&mut *tx).await?;
        let owned: Vec<(String, Decimal)> = sqlx::query_as(
            "SELECT category,amount FROM budgets WHERE owner_id=$1 AND month=$2 AND currency='TWD'",
        )
        .bind(id)
        .bind(&this)
        .fetch_all(&mut *tx)
        .await?;
        let accounts: Vec<(String, String)> = sqlx::query_as("SELECT c.connector||':'||(a.data->>'mask'),coalesce(a.data->>'name',c.connector) FROM source_records a JOIN connections c ON c.id=a.connection_id AND c.owner_id=a.owner_id WHERE a.owner_id=$1 AND a.kind='account' AND a.current AND a.data->>'mask' IS NOT NULL")
            .bind(id).fetch_all(&mut *tx).await?;
        let sync: (Option<chrono::DateTime<chrono::Utc>>, i64, i64, i64) = sqlx::query_as("SELECT max(last_success_at),count(*) FILTER(WHERE enabled),count(*) FILTER(WHERE enabled AND (last_success_at IS NULL OR last_success_at<now()-interval '48 hours')),count(*) FILTER(WHERE enabled AND login_failures>0) FROM connections WHERE owner_id=$1 AND credentials_cipher IS NOT NULL AND connector<>'browser-diagnostic'")
            .bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;

        for (c, v) in owned {
            *budgets.entry(c).or_default() += v;
        }
        for (key, label) in accounts {
            let e = account_owners.entry(key).or_insert_with(|| (label, vec![]));
            if !e.1.contains(&idx) {
                e.1.push(idx);
            }
        }
        // 成員自己的收支維持本人帳本的數字，不套用家庭層級的排除。
        let mut own = (Decimal::ZERO, Decimal::ZERO);
        for r in rows.iter().filter(|r| r.counted && r.occurred_on >= month) {
            add_flow(&mut own, r);
        }
        let (inc, exp) = own;
        people_out.push(json!({
            "id": id, "email": email, "name": name, "me": *id == user.id,
            "net": m_net.to_string(), "income": inc.to_string(), "expense": exp.to_string(),
            "last_synced_at": sync.0, "connections": sync.1, "stale": sync.2, "failing": sync.3,
        }));
        flows.push(rows);
    }

    let (dup, pairs, skip) = exclusions(&flows);

    let mut monthly: BTreeMap<String, (Decimal, Decimal)> = BTreeMap::new();
    let mut cats: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut now_cats: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut prev_cats: BTreeMap<String, Decimal> = BTreeMap::new();
    let (mut now_t, mut prev_t) = (
        (Decimal::ZERO, Decimal::ZERO),
        (Decimal::ZERO, Decimal::ZERO),
    );
    let mut dup_count = 0;
    let mut dup_amount = Decimal::ZERO;
    for (m, rows) in flows.iter().enumerate() {
        for (i, r) in rows.iter().enumerate() {
            if !r.counted {
                continue;
            }
            if dup[m][i] {
                if r.occurred_on >= month {
                    dup_count += 1;
                    dup_amount += r.amount.abs();
                }
                continue;
            }
            if skip[m][i] {
                continue;
            }
            add_flow(
                monthly
                    .entry(r.occurred_on.format("%Y-%m").to_string())
                    .or_default(),
                r,
            );
            let spent = -r.amount;
            if r.occurred_on >= month {
                if is_expense(&r.kind) {
                    *cats.entry(r.category.clone()).or_default() += spent;
                }
                if r.occurred_on.day() <= through {
                    add_flow(&mut now_t, r);
                    if is_expense(&r.kind) {
                        *now_cats.entry(r.category.clone()).or_default() += spent;
                    }
                }
            } else if r.occurred_on >= prev
                && r.occurred_on < month
                && r.occurred_on.day() <= prev_through
            {
                add_flow(&mut prev_t, r);
                if is_expense(&r.kind) {
                    *prev_cats.entry(r.category.clone()).or_default() += spent;
                }
            }
        }
    }

    // 只保留所有成員都有資料的日期，避免某人起始日較晚造成曲線跳動。
    let n = chosen.len() as i64;
    let points: Vec<Value> = history
        .into_iter()
        .filter(|(_, v)| v[5] == n)
        .map(|(d, v)| {
            let mut p = json!({"date": d});
            for (i, k) in keys.iter().enumerate() {
                p[k] = json!(v[i]);
            }
            p
        })
        .collect();
    let months: Vec<Value> = (0..MONTHS)
        .map(|i| {
            let m = shift_months(start, i).format("%Y-%m").to_string();
            let (inc, exp) = monthly.get(&m).copied().unwrap_or_default();
            json!({"month": m, "income": inc.to_string(), "expense": exp.to_string()})
        })
        .collect();
    let mut categories: Vec<(String, Decimal)> = cats
        .iter()
        .filter(|c| !c.1.is_zero())
        .map(|(c, v)| (c.clone(), *v))
        .collect();
    categories.sort_by(|a, b| b.1.cmp(&a.1));

    let mut deltas: Vec<(String, Decimal, Decimal)> = now_cats
        .keys()
        .chain(prev_cats.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|c| {
            (
                c.clone(),
                now_cats.get(c).copied().unwrap_or_default(),
                prev_cats.get(c).copied().unwrap_or_default(),
            )
        })
        .filter(|d| d.1 != d.2)
        .collect();
    deltas.sort_by_key(|d| std::cmp::Reverse((d.1 - d.2).abs()));

    let budget = (!budgets.is_empty()).then(|| {
        let rows: Vec<Value> = budgets
            .iter()
            .map(|(c, amount)| {
                json!({"category": c, "amount": amount.to_string(), "spent": cats.get(c).copied().unwrap_or_default().to_string()})
            })
            .collect();
        let total: Decimal = budgets.values().copied().sum();
        let spent: Decimal = budgets.keys().map(|c| cats.get(c).copied().unwrap_or_default()).sum();
        json!({"amount": total.to_string(), "spent": spent.to_string(), "categories": rows})
    });

    let who = |m: usize| chosen[m].2.clone();
    let transfers: Vec<Value> = pairs
        .iter()
        .filter(|&&(om, oi, im, ii)| flows[om][oi].occurred_on >= month || flows[im][ii].occurred_on >= month)
        .map(|&(om, oi, im, ii)| {
            let (o, i) = (&flows[om][oi], &flows[im][ii]);
            json!({"date": o.occurred_on, "amount": i.amount.to_string(), "from": who(om), "to": who(im),
                "from_description": o.description, "to_description": i.description})
        })
        .collect();
    let transfer_amount: Decimal = transfers.iter().map(|t| dec(t, "amount")).sum();
    let shared_accounts: Vec<Value> = account_owners
        .into_values()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(name, owners)| json!({"name": name, "members": owners.into_iter().map(who).collect::<Vec<_>>()}))
        .collect();

    Ok(Json(json!({
        "month": this,
        "currency": "TWD",
        "generated_at": chrono::Utc::now(),
        "net": net.to_string(),
        "assets": assets.to_string(),
        "liabilities": liabilities.to_string(),
        "members": people_out,
        "history": points,
        "months": months,
        "categories": categories.into_iter().map(|(c, v)| json!({"category": c, "amount": v.to_string()})).collect::<Vec<_>>(),
        "compare": {
            "through_day": through,
            "days_in_month": month_days,
            "prev_month": prev.format("%Y-%m").to_string(),
            "prev_through_day": prev_through,
            "income": now_t.0.to_string(),
            "expense": now_t.1.to_string(),
            "prev_income": prev_t.0.to_string(),
            "prev_expense": prev_t.1.to_string(),
            "categories": deltas.into_iter().take(8).map(|(c, a, b)| json!({"category": c, "amount": a.to_string(), "prev": b.to_string()})).collect::<Vec<_>>(),
        },
        "budget": budget,
        "internal_transfers": {"count": transfers.len(), "amount": transfer_amount.to_string(), "items": transfers},
        "duplicates": {"count": dup_count, "amount": dup_amount.to_string()},
        "shared_accounts": shared_accounts,
        "foreign_count": foreign,
    })))
}

#[derive(Deserialize)]
pub struct ExploreQuery {
    /// 要載入的成員（A、B 兩邊的聯集）；省略時為全部可檢視成員。
    members: Option<String>,
    /// A、B 各自的成員；省略時為 `members` 全部。成員比較時兩邊不同。
    a_members: Option<String>,
    b_members: Option<String>,
    /// A、B 期間，YYYY-MM-DD，含首尾。
    a_from: String,
    a_to: String,
    b_from: String,
    b_to: String,
}

fn parse_day(v: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| ApiError::bad("日期格式錯誤"))
}

/// 單段期間最多一年多一點，整體載入範圍最多三年，避免一次讀太多。
const EXPLORE_SPAN_DAYS: i64 = 400;
const EXPLORE_WINDOW_DAYS: i64 = 1100;
const EXPLORE_MAX_ENTRIES: usize = 20_000;

/// GET /household/explore：支出追查。回傳 A、B 兩段期間合併後的每筆支出（已去除共同帳戶重複與
/// 家庭內互轉，只計 TWD），以及 A 期間結束月往前 12 個月的分類×成員月支出。
/// 分組、圓餅、累積曲線與下鑽都在前端依明細計算，點選時不需再次請求。
pub async fn explore(
    user: Identity,
    State(s): State<AppState>,
    Query(q): Query<ExploreQuery>,
) -> Result<Json<Value>> {
    let chosen = choose(members(&s, &user).await?, q.members.as_deref())?;
    let side = |list: Option<&str>| -> Result<Vec<Uuid>> {
        Ok(choose(chosen.clone(), list)?.into_iter().map(|m| m.0).collect())
    };
    let (a_ids, b_ids) = (side(q.a_members.as_deref())?, side(q.b_members.as_deref())?);
    let (a_from, a_to, b_from, b_to) = (
        parse_day(&q.a_from)?,
        parse_day(&q.a_to)?,
        parse_day(&q.b_from)?,
        parse_day(&q.b_to)?,
    );
    for (f, t) in [(a_from, a_to), (b_from, b_to)] {
        if t < f {
            return Err(ApiError::bad("結束日不能早於開始日"));
        }
        if (t - f).num_days() >= EXPLORE_SPAN_DAYS {
            return Err(ApiError::bad("每段期間最多 400 天"));
        }
    }
    let trend_start = shift_months(NaiveDate::from_ymd_opt(a_to.year(), a_to.month(), 1).unwrap(), -(MONTHS - 1));
    let lo = a_from.min(b_from).min(trend_start);
    let hi = a_to.max(b_to) + chrono::Duration::days(1);
    if (hi - lo).num_days() > EXPLORE_WINDOW_DAYS {
        return Err(ApiError::bad("兩段期間相距太遠，最多涵蓋約三年"));
    }

    let mut flows = Vec::with_capacity(chosen.len());
    for (id, email, _) in &chosen {
        let mut tx = tenant(&s.db, *id, email).await?;
        flows.push(member_flows(&mut tx, *id, lo, hi).await?);
        tx.commit().await?;
    }
    let (dup, pairs, skip) = exclusions(&flows);
    let within = |d: NaiveDate| (d >= a_from && d <= a_to) || (d >= b_from && d <= b_to);

    let mut entries = Vec::new();
    let mut truncated = false;
    let mut trend: BTreeMap<(String, String, usize), Decimal> = BTreeMap::new();
    for (m, rows) in flows.iter().enumerate() {
        for (i, r) in rows.iter().enumerate() {
            if !r.counted || skip[m][i] || !is_expense(&r.kind) {
                continue;
            }
            if r.occurred_on >= trend_start && r.occurred_on <= a_to && a_ids.contains(&chosen[m].0) {
                *trend
                    .entry((r.occurred_on.format("%Y-%m").to_string(), r.category.clone(), m))
                    .or_default() -= r.amount;
            }
            if !within(r.occurred_on) {
                continue;
            }
            if entries.len() >= EXPLORE_MAX_ENTRIES {
                truncated = true;
                continue;
            }
            entries.push(json!({
                "id": r.id, "member_id": chosen[m].0, "date": r.occurred_on,
                "category": r.category, "description": r.description,
                "account": r.account_name, "kind": r.kind,
                // 支出為正、退款為負
                "amount": (-r.amount).to_string(),
            }));
        }
    }
    let in_scope = |m: usize, i: usize| within(flows[m][i].occurred_on) && flows[m][i].counted;
    let transfer_count = pairs
        .iter()
        .filter(|&&(om, oi, im, ii)| in_scope(om, oi) || in_scope(im, ii))
        .count();
    let dup_count = dup
        .iter()
        .enumerate()
        .flat_map(|(m, v)| v.iter().enumerate().map(move |(i, d)| (m, i, *d)))
        .filter(|&(m, i, d)| d && in_scope(m, i))
        .count();
    let span = |f: NaiveDate, t: NaiveDate| json!({"from": f, "to": t, "days": (t - f).num_days() + 1});
    Ok(Json(json!({
        "currency": "TWD",
        "generated_at": chrono::Utc::now(),
        "today": taipei_today(),
        "members": chosen.iter().map(|(id, _, name)| json!({"id": id, "name": name, "me": *id == user.id})).collect::<Vec<_>>(),
        "a": {"members": a_ids, "period": span(a_from, a_to)},
        "b": {"members": b_ids, "period": span(b_from, b_to)},
        "entries": entries,
        "truncated": truncated,
        "trend": trend.into_iter().filter(|(_, v)| !v.is_zero()).map(|((month, category, m), v)| json!({
            "month": month, "category": category, "member_id": chosen[m].0, "amount": v.to_string(),
        })).collect::<Vec<_>>(),
        "excluded": {"transfers": transfer_count, "duplicates": dup_count},
    })))
}

#[derive(Deserialize)]
pub struct UpcomingQuery {
    members: Option<String>,
    /// 往後看幾天：7 或 30（預設 30）。
    days: Option<i64>,
}

fn bill_paid(data: &Value) -> Option<bool> {
    match data.get("is_paid") {
        Some(Value::Bool(b)) => Some(*b),
        Some(Value::String(s)) if s == "true" => Some(true),
        Some(Value::String(s)) if s == "false" => Some(false),
        _ => None,
    }
}

/// 帳單狀態只依來源資料判斷：過了繳款日但來源沒標示已繳，只能說「未見繳款紀錄」。
fn bill_status(data: &Value, due: NaiveDate, today: NaiveDate) -> Option<(&'static str, Decimal)> {
    let statement = dec(data, "statement_amount");
    let paid = dec(data, "paid_amount");
    if statement <= Decimal::ZERO {
        return None;
    }
    let remaining = (statement - paid).max(Decimal::ZERO);
    if bill_paid(data) == Some(true) || remaining.is_zero() {
        return Some(("paid", statement));
    }
    Some((if due < today { "overdue" } else { "unpaid" }, remaining))
}

#[derive(sqlx::FromRow)]
struct Rule {
    id: Uuid,
    next_on: NaiveDate,
    frequency: String,
    anchor_day: i32,
    amount: Decimal,
    currency: String,
    description: String,
    category: String,
    account: String,
}

#[derive(sqlx::FromRow)]
struct Bill {
    id: Uuid,
    connector: String,
    data: Value,
    account: Option<String>,
    synced: Option<chrono::DateTime<chrono::Utc>>,
}

/// GET /household/upcoming：成員未來的固定支出與信用卡帳單。
/// 固定支出是預計會自動記帳的項目，不代表已付款；帳單狀態只採用來源同步的資料。
pub async fn upcoming(
    user: Identity,
    State(s): State<AppState>,
    Query(q): Query<UpcomingQuery>,
) -> Result<Json<Value>> {
    let chosen = choose(members(&s, &user).await?, q.members.as_deref())?;
    let days = match q.days.unwrap_or(30) {
        7 => 7,
        30 => 30,
        _ => return Err(ApiError::bad("天數只能是 7 或 30")),
    };
    let today = taipei_today();
    let until = today + chrono::Duration::days(days);
    let week = today + chrono::Duration::days(7);
    let mut items: Vec<(NaiveDate, Value)> = Vec::new();
    for (id, email, name) in &chosen {
        let mut tx = tenant(&s.db, *id, email).await?;
        let rules: Vec<Rule> = sqlx::query_as("SELECT r.id,r.next_on,r.frequency,r.anchor_day,-r.amount AS amount,a.currency,r.description,r.category,a.name AS account FROM recurring_entries r JOIN accounts a ON a.id=r.account_id AND a.owner_id=r.owner_id WHERE r.owner_id=$1 AND r.enabled AND r.kind='expense' AND r.next_on<=$2")
            .bind(id).bind(until).fetch_all(&mut *tx).await?;
        let bills: Vec<Bill> = sqlx::query_as("SELECT r.id,c.connector,r.data,a.data->>'name' AS account,c.last_success_at AS synced FROM source_records r JOIN connections c ON c.id=r.connection_id AND c.owner_id=r.owner_id LEFT JOIN source_records a ON a.connection_id=r.connection_id AND a.owner_id=r.owner_id AND a.kind='account' AND a.source_key=r.account_key WHERE r.owner_id=$1 AND r.kind='bill' AND r.current AND (a.id IS NULL OR a.current) AND left(r.data->>'payment_due',10)>=$2 AND left(r.data->>'payment_due',10)<=$3")
            .bind(id).bind((today - chrono::Duration::days(14)).to_string()).bind(until.to_string()).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        for r in rules {
            let mut date = r.next_on;
            for _ in 0..40 {
                if date > until {
                    break;
                }
                if date >= today {
                    items.push((date, json!({
                        "id": format!("recurring:{}:{date}", r.id), "source": "recurring", "date": date,
                        "title": r.description, "category": r.category, "account_name": r.account,
                        "amount": r.amount.to_string(), "currency": r.currency, "status": "scheduled",
                        "member_id": id, "member": name,
                    })));
                }
                date = recurring::next_date(date, &r.frequency, r.anchor_day as u32);
            }
        }
        for b in bills {
            let data = &b.data;
            let Some(due) = data["payment_due"]
                .as_str()
                .and_then(|d| NaiveDate::parse_from_str(d.get(..10)?, "%Y-%m-%d").ok())
            else {
                continue;
            };
            let Some((status, amount)) = bill_status(data, due, today) else {
                continue;
            };
            // 已繳的過去帳單不需要再出現
            if due < today && status == "paid" {
                continue;
            }
            items.push((due, json!({
                "id": format!("bill:{}", b.id), "source": "bill", "date": due,
                "title": format!("{} 信用卡帳單", b.account.as_deref().unwrap_or(&b.connector)),
                "category": "信用卡繳款", "period": data.get("period"),
                "amount": amount.to_string(), "currency": data["currency"].as_str().unwrap_or("TWD"),
                "status": status, "last_synced_at": b.synced, "member_id": id, "member": name,
            })));
        }
    }
    items.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1["member"].as_str().cmp(&b.1["member"].as_str()))
    });
    let open = |v: &Value| v["status"] != "paid" && v["currency"] == "TWD";
    let sum = |f: &dyn Fn(&NaiveDate) -> bool| -> Decimal {
        items
            .iter()
            .filter(|(d, v)| f(d) && open(v))
            .map(|(_, v)| dec(v, "amount"))
            .sum()
    };
    let week_due = sum(&|d| *d >= today && *d < week);
    let period_due = sum(&|d| *d >= today);
    let overdue = items
        .iter()
        .filter(|(_, v)| v["status"] == "overdue")
        .count();
    Ok(Json(json!({
        "today": today, "days": days, "currency": "TWD",
        "week_due": week_due.to_string(), "period_due": period_due.to_string(), "overdue_count": overdue,
        "items": items.into_iter().map(|i| i.1).collect::<Vec<_>>(),
    })))
}

fn chrono_tz_taipei() -> chrono::FixedOffset {
    chrono::FixedOffset::east_opt(8 * 3600).unwrap()
}
pub fn shift_months(d: NaiveDate, n: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month0() as i32 + n;
    NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn months_shift_across_years() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(
            shift_months(d, -1),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
        assert_eq!(
            shift_months(d, -13),
            NaiveDate::from_ymd_opt(2024, 12, 1).unwrap()
        );
        assert_eq!(
            shift_months(d, 12),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }
    #[test]
    fn masks_email() {
        assert_eq!(mask_email("demo940807@example.com"), "de***7@example.com");
        assert_eq!(mask_email("ab@example.com"), "a***@example.com");
        assert_eq!(mask_email("a@example.com"), "***@example.com");
    }

    fn flow(date: &str, amount: i64, desc: &str, key: Option<&str>) -> Flow {
        Flow {
            id: Uuid::new_v4(),
            account_name: None,
            occurred_on: date.parse().unwrap(),
            amount: Decimal::from(amount),
            kind: if amount < 0 { "expense" } else { "income" }.into(),
            category: "其他".into(),
            description: desc.into(),
            counted: true,
            account_key: key.map(str::to_owned),
            transferish: desc.contains("轉帳"),
        }
    }
    #[test]
    fn shared_account_rows_count_once() {
        let a = vec![
            flow("2026-09-01", -120, "咖啡", Some("bank:****1234")),
            flow("2026-09-01", -120, "咖啡", Some("bank:****1234")),
            flow("2026-09-02", -50, "早餐", None),
        ];
        let b = vec![
            flow("2026-09-01", -120, "咖啡", Some("bank:****1234")),
            flow("2026-09-01", -120, "咖啡", Some("bank:****1234")),
            flow("2026-09-01", -120, "咖啡", Some("bank:****1234")),
            flow("2026-09-02", -50, "早餐", None),
        ];
        let d = mark_duplicates(&[a, b]);
        assert_eq!(d[0], vec![false, false, false]);
        // 第三筆是 B 多出來的一筆，手動帳戶不去重
        assert_eq!(d[1], vec![true, true, false, false]);
    }
    #[test]
    fn transfers_between_members_are_paired() {
        let a = vec![
            flow("2026-09-05", -3000, "轉帳給家人", None),
            flow("2026-09-05", -500, "餐廳", None),
        ];
        let b = vec![
            flow("2026-09-06", 3000, "跨行入帳", None),
            flow("2026-09-06", 500, "薪資", None),
        ];
        let skip = vec![vec![false; 2], vec![false; 2]];
        // 餐廳與薪資金額相同，但兩邊都不像轉帳，不配對
        assert_eq!(match_transfers(&[a, b], &skip), vec![(0, 0, 1, 0)]);
    }
    #[test]
    fn transfer_pairs_need_close_dates_and_distinct_members() {
        let a = vec![
            flow("2026-09-01", -1000, "轉帳", None),
            flow("2026-09-01", 1000, "轉帳", None),
        ];
        let b = vec![flow("2026-09-10", 1000, "轉帳", None)];
        let skip = vec![vec![false; 2], vec![false]];
        assert!(match_transfers(&[a, b], &skip).is_empty());
    }
    #[test]
    fn bill_status_follows_source_data() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
        let past = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
        let unpaid = json!({"statement_amount": "5000", "paid_amount": "1000"});
        assert_eq!(
            bill_status(&unpaid, today, today),
            Some(("unpaid", Decimal::from(4000)))
        );
        assert_eq!(
            bill_status(&unpaid, past, today),
            Some(("overdue", Decimal::from(4000)))
        );
        let paid = json!({"statement_amount": "5000", "is_paid": true});
        assert_eq!(bill_status(&paid, past, today).unwrap().0, "paid");
        assert_eq!(
            bill_status(&json!({"statement_amount": "0"}), today, today),
            None
        );
    }
}
