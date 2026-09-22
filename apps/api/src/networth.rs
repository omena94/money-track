//! 淨資產：手動帳戶與同步來源合併計算，並產生每日資產走勢。
//!
//! 類別：bank（銀行與現金）、investment（投資）、other（其他資產）、debt（負債，負數）。
//! 只加總 TWD；其他幣別列在 `foreign`，尚無匯率換算。
use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{Duration, NaiveDate};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

const MAX_DAYS: i64 = 3650;
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Cat {
    Bank,
    TimeDeposit,
    Investment,
    Other,
    Debt,
}
const CATS: [Cat; 5] = [
    Cat::Bank,
    Cat::TimeDeposit,
    Cat::Investment,
    Cat::Other,
    Cat::Debt,
];
impl Cat {
    fn key(self) -> &'static str {
        match self {
            Cat::Bank => "bank",
            Cat::TimeDeposit => "time_deposit",
            Cat::Investment => "investment",
            Cat::Other => "other",
            Cat::Debt => "debt",
        }
    }
}
fn manual_cat(kind: &str) -> Cat {
    match kind {
        "cash" | "bank" => Cat::Bank,
        "investment" => Cat::Investment,
        "credit" | "loan" => Cat::Debt,
        _ => Cat::Other,
    }
}
/// 同步帳戶的類別；信用卡與貸款一律視為負債（金額取絕對值後為負）。
fn source_cat(account_type: &str) -> Cat {
    match account_type {
        "credit" | "loan" => Cat::Debt,
        "settlement_cash" => Cat::Investment,
        "time_deposit" => Cat::TimeDeposit,
        _ => Cat::Bank,
    }
}
fn signed(cat: Cat, amount: Decimal) -> Decimal {
    if cat == Cat::Debt {
        -amount.abs()
    } else {
        amount
    }
}
// Signed source balances distinguish a credit overpayment from debt.
fn source_value(data:&Value,amount:Decimal)->(Cat,Decimal){
    let cat=source_cat(text(data,"account_type"));
    if cat==Cat::Debt && data["balance_convention"]=="signed" && amount>Decimal::ZERO {(Cat::Other,amount)} else {(cat,signed(cat,amount))}
}
fn day_of(v: Option<&Value>) -> Option<NaiveDate> {
    v.and_then(Value::as_str)
        .and_then(|s| NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok())
}

/// 集保資產歷史（TDCC 趨勢圖）的一天。
/// 實測 chartVal（存為 total）只含股票與 ETF，基金另列；
/// 因此投資合計 = total + fund。舊資料的 stock 是 total − fund，不採用。
#[derive(Default)]
struct TdccWorth {
    total: Option<Decimal>,
    stock: Decimal,
    fund: Decimal,
}
impl TdccWorth {
    fn value(&self) -> Decimal {
        match self.total {
            Some(total) => total + self.fund,
            None => self.stock + self.fund,
        }
    }
}

fn dec(v: Option<&Value>) -> Option<Decimal> {
    v.and_then(Value::as_str).and_then(|s| s.parse().ok())
}
fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// 依日期排序的點，查詢某日（含）以前最近一點；早於第一點時回傳 None。
fn at(points: &[(NaiveDate, Decimal)], day: NaiveDate) -> Option<Decimal> {
    match points.partition_point(|(d, _)| *d <= day) {
        0 => None,
        i => Some(points[i - 1].1),
    }
}

struct Series {
    start: NaiveDate,
    days: usize,
    values: HashMap<Cat, Vec<Decimal>>,
}
impl Series {
    fn new(start: NaiveDate, today: NaiveDate) -> Self {
        let days = (today - start).num_days().max(0) as usize + 1;
        Self {
            start,
            days,
            values: CATS
                .iter()
                .map(|c| (*c, vec![Decimal::ZERO; days]))
                .collect(),
        }
    }
    fn day(&self, i: usize) -> NaiveDate {
        self.start + Duration::days(i as i64)
    }
    fn add(&mut self, cat: Cat, i: usize, v: Decimal) {
        self.values.get_mut(&cat).unwrap()[i] += v;
    }
}

#[derive(sqlx::FromRow)]
struct ManualRow {
    id: Uuid,
    kind: String,
    currency: String,
    current_balance: Decimal,
    investment_value: Option<Decimal>,
    start: NaiveDate,
}
#[derive(sqlx::FromRow)]
struct SourceRow {
    id: Uuid,
    connection_id: Uuid,
    connector: String,
    kind: String,
    source_key: String,
    data: Value,
    included: bool,
    observed_at: chrono::DateTime<chrono::Utc>,
    #[sqlx(skip)]
    linked_account:Option<Value>,
}

async fn today(tx: &mut sqlx::PgConnection) -> Result<NaiveDate> {
    Ok(
        sqlx::query_scalar("SELECT (now() AT TIME ZONE 'Asia/Taipei')::date")
            .fetch_one(tx)
            .await?,
    )
}

async fn current_sources(tx: &mut sqlx::PgConnection, owner: Uuid) -> Result<Vec<SourceRow>> {
    let mut rows:Vec<SourceRow>=sqlx::query_as("SELECT r.id,r.connection_id,c.connector,r.kind,r.source_key,r.data,r.included,r.observed_at FROM source_records r JOIN connections c ON c.id=r.connection_id AND c.owner_id=r.owner_id WHERE r.owner_id=$1 AND r.current AND r.kind IN ('account','holding') AND c.connector=ANY($2) ORDER BY c.connector,r.kind,r.observed_at")
        .bind(owner)
        .bind(&crate::sources::IDS[..])
        .fetch_all(&mut *tx)
        .await?;
    let links:Vec<(Uuid,Value)>=sqlx::query_as("SELECT l.reference_id,jsonb_build_object('id',coalesce(l.primary_source_id,l.primary_manual_id),'kind',CASE WHEN l.primary_source_id IS NOT NULL THEN 'source' ELSE 'manual' END,'name',coalesce(p.data->>'name',m.name),'mask',p.data->>'mask','connector',c.connector,'available',coalesce(p.current,m.id IS NOT NULL),'included',coalesce(p.current AND p.included,m.id IS NOT NULL)) FROM source_account_links l LEFT JOIN source_records p ON p.id=l.primary_source_id AND p.owner_id=l.owner_id LEFT JOIN connections c ON c.id=p.connection_id AND c.owner_id=p.owner_id LEFT JOIN accounts m ON m.id=l.primary_manual_id AND m.owner_id=l.owner_id WHERE l.owner_id=$1")
        .bind(owner).fetch_all(&mut *tx).await?;
    for r in rows.iter_mut().filter(|r|r.connector=="tdcc" && r.kind=="account") {
        r.included=false;
        r.linked_account=links.iter().find(|(id,_)|*id==r.id).map(|(_,value)|value.clone());
    }
    Ok(rows)
}

/// 信用卡沒有目前欠款時，以最新一期未繳清帳單（應繳 − 已繳）作為負債，並標記來源。
async fn fill_card_balances(
    tx: &mut sqlx::PgConnection,
    owner: Uuid,
    rows: &mut [SourceRow],
) -> Result<()> {
    let bills: Vec<(Uuid, String, Value)> = sqlx::query_as("SELECT DISTINCT ON (connection_id,account_key) connection_id,account_key,data FROM source_records WHERE owner_id=$1 AND kind='bill' AND current AND account_key IS NOT NULL ORDER BY connection_id,account_key,data->>'period' DESC")
        .bind(owner)
        .fetch_all(tx)
        .await?;
    for r in rows.iter_mut().filter(|r| {
        r.kind == "account"
            && source_cat(text(&r.data, "account_type")) == Cat::Debt
            && r.data.get("balance").is_none_or(Value::is_null)
    }) {
        let Some((_, _, bill)) = bills
            .iter()
            .find(|(c, k, _)| *c == r.connection_id && *k == r.source_key)
        else {
            continue;
        };
        let owed = if bill.get("is_paid").and_then(Value::as_bool) == Some(true) {
            Decimal::ZERO
        } else {
            let (Some(statement), Some(paid)) = (dec(bill.get("statement_amount")), dec(bill.get("paid_amount"))) else { continue; };
            (statement - paid).max(Decimal::ZERO)
        };
        r.data["balance"] = json!(owed.to_string());
        r.data["balance_from"] = json!("bill");
    }
    Ok(())
}

/// GET /net-worth：目前淨資產組成與每日走勢。
pub async fn net_worth(user: Identity, State(s): State<AppState>) -> Result<Json<Value>> {
    let mut v = compute(&s.db, user.id, &user.email).await?;
    add_fx(&s, &mut v).await;
    Ok(Json(v))
}

/// 外幣依匯率附上等值台幣；台幣淨資產與走勢維持不含外幣。
async fn add_fx(s: &AppState, v: &mut Value) {
    let Some(list) = v["foreign"].as_array() else {
        return;
    };
    if list.is_empty() {
        v["fx"] = Value::Null;
        return;
    }
    let quotes = crate::fx::quotes(s).await;
    let mut total = Decimal::ZERO;
    let mut complete = true;
    let mut out = Vec::new();
    let mut codes = Vec::new();
    for f in list {
        let cur = f["currency"].as_str().unwrap_or_default().to_string();
        let net = dec(f.get("net")).unwrap_or_default();
        let twd = quotes.as_ref().and_then(|q| q.to_twd(&cur, net));
        match twd {
            Some(t) => total += t,
            None => complete = false,
        }
        let mut e = f.clone();
        e["twd"] = json!(twd.map(|t| t.to_string()));
        e["rate"] = json!(quotes.as_ref().and_then(|q| q.rates.get(&cur)).map(|r| r.buy.to_string()));
        out.push(e);
        codes.push(cur);
    }
    let net = dec(v.get("net")).unwrap_or_default();
    v["foreign"] = json!(out);
    v["fx"] = match &quotes {
        Some(q) => {
            let mut sum = crate::fx::summary(q, &codes.iter().map(String::as_str).collect::<Vec<_>>());
            sum["foreign_twd"] = json!(total.to_string());
            sum["complete"] = json!(complete);
            sum["net_with_foreign"] = json!((net + total).round_dp(2).normalize().to_string());
            sum
        }
        None => Value::Null,
    };
}

/// 計算某位使用者的淨資產；家庭合併檢視也使用。
pub async fn compute(db: &sqlx::PgPool, owner: Uuid, email: &str) -> Result<Value> {
    compute_for(db, owner, email, None).await
}

/// 只計算單一帳戶或持股時的對象。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Manual(Uuid),
    Source(Uuid),
}

/// GET /asset-history/{kind}/{id}：單一帳戶（manual）或同步項目（source）的每日餘額。
pub async fn asset_history(
    user: Identity,
    State(s): State<AppState>,
    Path((kind, id)): Path<(String, Uuid)>,
) -> Result<Json<Value>> {
    let focus = match kind.as_str() {
        "manual" => Focus::Manual(id),
        "source" => Focus::Source(id),
        _ => return Err(ApiError::missing()),
    };
    let v = compute_for(&s.db, user.id, &user.email, Some(focus)).await?;
    if v["found"] != json!(true) {
        return Err(ApiError::missing());
    }
    let points: Vec<Value> = v["history"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let total: i64 = CATS
                .iter()
                .map(|c| p[c.key()].as_i64().unwrap_or_default())
                .sum();
            json!({"date": p["date"], "value": total})
        })
        .collect();
    Ok(Json(json!({"as_of": v["as_of"], "history": points})))
}

async fn compute_for(
    db: &sqlx::PgPool,
    owner: Uuid,
    email: &str,
    focus: Option<Focus>,
) -> Result<Value> {
    let mut tx = tenant(db, owner, email).await?;
    let today = today(&mut tx).await?;
    // 手動投資市值無歷史報價，於查看時記下當日值。
    sqlx::query("INSERT INTO investment_value_snapshots(owner_id,account_id,date,value) SELECT owner_id,id,$2,investment_value FROM accounts WHERE owner_id=$1 AND kind='investment' AND investment_value IS NOT NULL ON CONFLICT(account_id,date) DO UPDATE SET value=EXCLUDED.value")
        .bind(owner).bind(today).execute(&mut *tx).await?;

    let mut manual: Vec<ManualRow> = sqlx::query_as("SELECT a.id,a.kind,a.currency,a.current_balance,a.investment_value,least((a.created_at AT TIME ZONE 'Asia/Taipei')::date,(SELECT min(occurred_on) FROM transactions t WHERE t.account_id=a.id AND t.owner_id=a.owner_id)) AS start FROM accounts a WHERE a.owner_id=$1")
        .bind(owner).fetch_all(&mut *tx).await?;
    let deltas: Vec<(Uuid, NaiveDate, Decimal)> = sqlx::query_as("SELECT account_id,occurred_on,sum(amount) FROM transactions WHERE owner_id=$1 AND NOT is_split_parent GROUP BY 1,2")
        .bind(owner).fetch_all(&mut *tx).await?;
    let snaps: Vec<(Uuid, NaiveDate, Decimal)> = sqlx::query_as("SELECT account_id,date,value FROM investment_value_snapshots WHERE owner_id=$1 ORDER BY date")
        .bind(owner).fetch_all(&mut *tx).await?;
    let mut sources = current_sources(&mut tx, owner).await?;
    fill_card_balances(&mut tx, owner, &mut sources).await?;
    let mut history: Vec<(Uuid, String, String, NaiveDate, Decimal, String)> = sqlx::query_as("SELECT connection_id,kind,source_key,date,amount,currency FROM source_balance_history WHERE owner_id=$1 ORDER BY date")
        .bind(owner).fetch_all(&mut *tx).await?;
    let mut worth: Vec<(Uuid, Value)> = sqlx::query_as(
        "SELECT connection_id,data FROM source_records WHERE owner_id=$1 AND kind='net_worth'",
    )
    .bind(owner)
    .fetch_all(&mut *tx)
    .await?;
    let source_tx: Vec<(Uuid, String, Value)> = sqlx::query_as("SELECT connection_id,account_key,data FROM source_records WHERE owner_id=$1 AND current AND kind='transaction' AND account_key IS NOT NULL AND coalesce(data->>'status','posted')<>'pending'")
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut found = true;
    if let Some(f) = focus {
        manual.retain(|a| f == Focus::Manual(a.id));
        sources.retain(|r| f == Focus::Source(r.id));
        // 單看一個項目時，不管是否計入淨資產都要畫出來
        for r in sources.iter_mut() {
            r.included = true;
        }
        let keep: Vec<(Uuid, String, String)> = sources
            .iter()
            .map(|r| (r.connection_id, r.kind.clone(), r.source_key.clone()))
            .collect();
        history.retain(|h| keep.iter().any(|k| k.0 == h.0 && k.1 == h.1 && k.2 == h.2));
        // 集保整體走勢只適用於全部持股加總
        worth.clear();
        found = !manual.is_empty() || !sources.is_empty();
    }

    // ---- 目前值 ----
    let mut current: HashMap<Cat, Decimal> = HashMap::new();
    let mut counts: HashMap<Cat, usize> = HashMap::new();
    let mut foreign: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut unknown = 0usize;
    for a in &manual {
        let cat = manual_cat(&a.kind);
        let value = a.current_balance + a.investment_value.unwrap_or_default();
        if a.investment_value.is_none() {
            unknown += 1;
        }
        if a.currency == "TWD" {
            *current.entry(cat).or_default() += value;
            *counts.entry(cat).or_default() += 1;
        } else {
            *foreign.entry(a.currency.clone()).or_default() += value;
        }
    }
    let mut source_starts = Vec::new();
    for r in sources.iter().filter(|r| r.included) {
        let currency = text(&r.data, "currency");
        let (cat, amount) = if r.kind == "account" {
            let cat = source_cat(text(&r.data, "account_type"));
            match dec(r.data.get("balance")) {Some(v)=>{let(c,n)=source_value(&r.data,v);(c,Some(n))},None=>(cat,None)}
        } else {
            (Cat::Investment, dec(r.data.get("market_value")))
        };
        let Some(amount) = amount else {
            unknown += 1;
            continue;
        };
        if currency == "TWD" {
            *current.entry(cat).or_default() += amount;
            *counts.entry(cat).or_default() += 1;
        } else {
            *foreign.entry(currency.to_string()).or_default() += amount;
        }
        source_starts.push(r.observed_at.date_naive());
    }

    // ---- 每日走勢 ----
    let hist_first = history.first().map(|h| h.3);
    let worth_first = worth
        .iter()
        .filter_map(|(_, d)| d.get("date").and_then(Value::as_str))
        .filter_map(|s| NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok())
        .min();
    let start = manual
        .iter()
        .map(|a| a.start)
        .chain(hist_first)
        .chain(worth_first)
        .chain(source_starts)
        .min()
        .unwrap_or(today)
        .max(today - Duration::days(MAX_DAYS))
        .min(today);
    let mut series = Series::new(start, today);

    let mut by_account: HashMap<Uuid, BTreeMap<NaiveDate, Decimal>> = HashMap::new();
    for (id, d, v) in deltas {
        *by_account.entry(id).or_default().entry(d).or_default() += v;
    }
    let mut snap_map: HashMap<Uuid, Vec<(NaiveDate, Decimal)>> = HashMap::new();
    for (id, d, v) in snaps {
        snap_map.entry(id).or_default().push((d, v));
    }
    for a in manual.iter().filter(|a| a.currency == "TWD") {
        let cat = manual_cat(&a.kind);
        let changes = by_account.remove(&a.id).unwrap_or_default();
        // 以目前餘額為準往回扣除，確保最後一天與帳戶餘額一致。
        let mut bal = a.current_balance
            - changes
                .range(today + Duration::days(1)..)
                .map(|(_, v)| *v)
                .sum::<Decimal>();
        let points = snap_map.remove(&a.id).unwrap_or_default();
        for i in (0..series.days).rev() {
            let day = series.day(i);
            if day < a.start {
                break;
            }
            let mut value = bal;
            if a.kind == "investment" {
                value += at(&points, day)
                    .or_else(|| points.first().map(|p| p.1))
                    .or(a.investment_value)
                    .unwrap_or_default();
            }
            series.add(cat, i, value);
            bal -= changes.get(&day).copied().unwrap_or_default();
        }
    }

    // 同步帳戶：第一次觀察之後以餘額歷史延續；之前依來源交易往回推算，
    // 最早交易之前維持該餘額（與手動帳戶同樣以已知餘額為錨點）。
    let mut hist_map: HashMap<(Uuid, &str, &str), Vec<(NaiveDate, Decimal)>> = HashMap::new();
    for (c, k, key, d, v, cur) in &history {
        if cur == "TWD" {
            hist_map
                .entry((*c, k.as_str(), key.as_str()))
                .or_default()
                .push((*d, *v));
        }
    }
    let mut tx_map: HashMap<(Uuid, &str), BTreeMap<NaiveDate, Decimal>> = HashMap::new();
    for (c, key, d) in &source_tx {
        let (Some(day), Some(amount)) = (day_of(d.get("date")), dec(d.get("amount"))) else {
            continue;
        };
        *tx_map
            .entry((*c, key.as_str()))
            .or_default()
            .entry(day)
            .or_default() += amount;
    }
    let empty = BTreeMap::new();
    for r in sources.iter().filter(|r| r.included && r.kind == "account") {
        let cat = source_cat(text(&r.data, "account_type"));
        let Some(points) = hist_map.get(&(r.connection_id, "account", r.source_key.as_str()))
        else {
            if text(&r.data, "balance_from") == "bill" {
                if let Some(v) = dec(r.data.get("balance")) {
                    let from = r.observed_at.date_naive();
                    for i in 0..series.days {
                        if series.day(i) >= from {
                            series.add(cat, i, signed(cat, v));
                        }
                    }
                }
            }
            continue;
        };
        let changes = tx_map
            .get(&(r.connection_id, r.source_key.as_str()))
            .unwrap_or(&empty);
        let (anchor, mut bal) = points[0];
        // 定存等有起存日的帳戶，起存日之前不存在
        let opened = day_of(r.data.get("opened_on"));
        for i in (0..series.days).rev() {
            let day = series.day(i);
            if opened.is_some_and(|o| day < o) {
                break;
            }
            let value = match at(points, day) {
                Some(v) => v,
                None => {
                    // 由錨點往回：扣掉這一天之後、錨點當天以前的交易
                    bal
                }
            };
            let (day_cat,day_value)=source_value(&r.data,value);
            series.add(day_cat, i, day_value);
            if day <= anchor {
                bal -= changes.get(&day).copied().unwrap_or_default();
            }
        }
    }
    // 同步持股：每一檔各自延續；所有目前持股都有快照之前，用集保提供的資產歷史。
    let mut connections: Vec<Uuid> = sources
        .iter()
        .filter(|r| r.included && r.kind == "holding")
        .map(|r| r.connection_id)
        .collect();
    connections.sort();
    connections.dedup();
    for c in connections {
        let mut parts: BTreeMap<NaiveDate, TdccWorth> = BTreeMap::new();
        for (conn, d) in &worth {
            if *conn != c {
                continue;
            }
            let (Some(day), Some(v)) = (day_of(d.get("date")), dec(d.get("amount"))) else {
                continue;
            };
            let e = parts.entry(day).or_default();
            match text(d, "asset_type") {
                "total" => e.total = Some(v),
                "fund" => e.fund += v,
                _ => e.stock += v,
            }
        }
        let worth_points: Vec<(NaiveDate, Decimal)> =
            parts.into_iter().map(|(d, w)| (d, w.value())).collect();
        let holdings: Vec<Vec<(NaiveDate, Decimal)>> = sources
            .iter()
            .filter(|r| r.included && r.kind == "holding" && r.connection_id == c)
            .filter_map(|r| {
                hist_map
                    .get(&(c, "holding", r.source_key.as_str()))
                    .cloned()
            })
            .collect();
        // 所有目前持股都至少有一筆快照的日期；之前各檔的資料日期可能不同
        let complete = holdings.iter().map(|p| p[0].0).max();
        for i in 0..series.days {
            let day = series.day(i);
            let value = match complete {
                Some(first) if day >= first => {
                    Some(holdings.iter().filter_map(|p| at(p, day)).sum())
                }
                _ => at(&worth_points, day).or_else(|| {
                    let partial: Decimal = holdings.iter().filter_map(|p| at(p, day)).sum();
                    (!partial.is_zero()).then_some(partial)
                }),
            };
            if let Some(v) = value {
                series.add(Cat::Investment, i, v);
            }
        }
    }

    let round = |v: Decimal| v.round().to_i64().unwrap_or_default();
    let points: Vec<Value> = (0..series.days)
        .map(|i| {
            let mut p = json!({ "date": series.day(i) });
            for c in CATS {
                p[c.key()] = json!(round(series.values[&c][i]));
            }
            p
        })
        .collect();
    let get = |c: Cat| current.get(&c).copied().unwrap_or_default();
    let assets = get(Cat::Bank) + get(Cat::TimeDeposit) + get(Cat::Investment) + get(Cat::Other);
    Ok(json!({
        "found": found,
        "as_of": today,
        "currency": "TWD",
        "net": (assets + get(Cat::Debt)).to_string(),
        "assets": assets.to_string(),
        "liabilities": get(Cat::Debt).abs().to_string(),
        "categories": CATS.iter().map(|c| json!({
            "key": c.key(),
            "amount": if *c == Cat::Debt { get(*c).abs() } else { get(*c) }.to_string(),
            "count": counts.get(c).copied().unwrap_or_default(),
        })).collect::<Vec<_>>(),
        "unknown": unknown,
        "foreign": foreign.iter().map(|(k, v)| json!({"currency": k, "net": v.to_string()})).collect::<Vec<_>>(),
        "history": points,
    }))
}

/// GET /source-assets：目前同步到的帳戶與持倉。
pub async fn source_assets(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let mut rows = current_sources(&mut tx, user.id).await?;
    fill_card_balances(&mut tx, user.id, &mut rows).await?;
    tx.commit().await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                let d = &r.data;
                let account = r.kind == "account";
                let cat = if account {
                    source_value(d,dec(d.get("balance")).unwrap_or_default()).0
                } else {
                    Cat::Investment
                };
                json!({
                    "id": r.id,
                    "connection_id": r.connection_id,
                    "connector": r.connector,
                    "kind": r.kind,
                    "category": cat.key(),
                    "name": text(d, "name"),
                    "institution": d.get("institution"),
                    "account_type": d.get("account_type"),
                    "mask": d.get("mask"),
                    "balance_from": d.get("balance_from"),
                    "symbol": d.get("symbol"),
                    "quantity": d.get("quantity"),
                    "asset_type": d.get("asset_type"),
                    "cost_basis": d.get("cost_basis"),
                    "unit_price": d.get("unit_price"),
                    "price_date": d.get("price_date"),
                    "unrealized_gain": d.get("unrealized_gain"),
                    "accumulated_dividend": d.get("accumulated_dividend"),
                    "recurring_amount": d.get("recurring_amount"),
                    "recurring_days": d.get("recurring_days"),
                    "source_status": d.get("source_status"),
                    "currency": text(d, "currency"),
                    "amount": if account { d.get("balance") } else { d.get("market_value") },
                    "available_balance": d.get("available_balance"),
                    "credit_limit": d.get("credit_limit"),
                    "payment_due": d.get("payment_due"),
                    "interest_rate": d.get("interest_rate"),
                    "next_payment_amount": d.get("next_payment_amount"),
                    "last_payment_amount": d.get("last_payment_amount"),
                    "last_payment_on": d.get("last_payment_on"),
                    "remaining_installments": d.get("remaining_installments"),
                    "term": d.get("term"),
                    "opened_on": d.get("opened_on"),
                    "maturity_on": d.get("maturity_on"),
                    "as_of": d.get("as_of"),
                    "observed_at": r.observed_at,
                    "included": r.included,
                    "reference_only":r.connector=="tdcc" && account,
                    "linked_account":r.linked_account,
                })
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct Include {
    included: bool,
}
/// PATCH /source-assets/{id}：是否計入淨資產。持倉以整個來源為單位切換。
pub async fn set_included(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Include>,
) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let reference:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM source_records r JOIN connections c ON c.id=r.connection_id AND c.owner_id=r.owner_id WHERE r.id=$1 AND r.owner_id=$2 AND r.kind='account' AND c.connector='tdcc')")
        .bind(id).bind(user.id).fetch_one(&mut *tx).await?;
    if reference && v.included {return Err(ApiError::bad("集保銀行帳戶僅供參考，請以銀行帳戶計入淨資產"));}
    let row: Option<(String, Uuid)> = sqlx::query_as("UPDATE source_records SET included=$1 WHERE id=$2 AND owner_id=$3 AND kind IN ('account','holding') RETURNING kind,connection_id")
        .bind(v.included).bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    let (kind, connection) = row.ok_or_else(ApiError::missing)?;
    if kind == "holding" {
        sqlx::query("UPDATE source_records SET included=$1 WHERE connection_id=$2 AND owner_id=$3 AND kind='holding'")
            .bind(v.included).bind(connection).bind(user.id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"included": v.included})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_credit_overpayment_is_an_asset() {
        let d=json!({"account_type":"credit","balance_convention":"signed"});
        let (cat,n)=source_value(&d,Decimal::from(160));
        assert!(cat==Cat::Other);assert_eq!(n,Decimal::from(160));
        let (cat,n)=source_value(&d,Decimal::from(-500));
        assert!(cat==Cat::Debt);assert_eq!(n,Decimal::from(-500));
    }
    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    #[test]
    fn forward_fills_and_ignores_before_first_point() {
        let p = vec![
            (d("2026-09-01"), Decimal::from(10)),
            (d("2026-09-05"), Decimal::from(20)),
        ];
        assert_eq!(at(&p, d("2026-08-31")), None);
        assert_eq!(at(&p, d("2026-09-01")), Some(Decimal::from(10)));
        assert_eq!(at(&p, d("2026-09-04")), Some(Decimal::from(10)));
        assert_eq!(at(&p, d("2026-09-30")), Some(Decimal::from(20)));
    }
    #[test]
    fn tdcc_trend_adds_funds_to_stock_only_total() {
        // 2026-09-23 實際資料：total 5,894,121（股票）＋ fund 1,902,425 ＝ 持股合計
        let w = TdccWorth {
            total: Some(Decimal::from(5_894_121)),
            stock: Decimal::from(3_991_696),
            fund: Decimal::from(1_902_425),
        };
        assert_eq!(w.value(), Decimal::from(7_796_546));
        let no_total = TdccWorth {
            total: None,
            stock: Decimal::from(10),
            fund: Decimal::from(5),
        };
        assert_eq!(no_total.value(), Decimal::from(15));
    }
    #[test]
    fn credit_cards_are_debt_regardless_of_sign() {
        assert!(source_cat("credit") == Cat::Debt);
        assert_eq!(signed(Cat::Debt, Decimal::from(500)), Decimal::from(-500));
        assert_eq!(signed(Cat::Debt, Decimal::from(-500)), Decimal::from(-500));
        assert!(source_cat("savings") == Cat::Bank);
        assert!(source_cat("settlement_cash") == Cat::Investment);
        assert!(source_cat("time_deposit") == Cat::TimeDeposit);
    }
}
