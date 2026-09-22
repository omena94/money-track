use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::FromRow;
use std::str::FromStr;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, FromRow, ToSchema)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub currency: String,
    pub opening_balance: String,
    pub balance: Option<String>,
    pub cash_balance: String,
    pub valuation_date: Option<NaiveDate>,
    pub institution: Option<String>,
}
#[derive(Deserialize, ToSchema)]
pub struct NewAccount {
    pub name: String,
    pub kind: String,
    pub currency: String,
    pub opening_balance: String,
    pub institution: Option<String>,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OpeningBalance {
    pub opening_balance: String,
    pub expected_opening_balance: String,
}
#[derive(Serialize, FromRow, ToSchema)]
pub struct Entry {
    pub id: Uuid,
    pub account_id: Uuid,
    pub account_name: String,
    pub currency: String,
    pub amount: String,
    pub kind: String,
    pub occurred_on: NaiveDate,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub note: String,
    pub transfer_id: Option<Uuid>,
    pub split_parent_id: Option<Uuid>,
    pub investment_trade_id: Option<Uuid>,
}
#[derive(Deserialize, ToSchema)]
pub struct NewEntry {
    pub account_id: Uuid,
    pub amount: String,
    pub kind: String,
    pub occurred_on: NaiveDate,
    pub description: String,
    #[serde(default = "uncategorized")]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub note: String,
}
fn uncategorized() -> String {
    "未分類".into()
}
pub fn decimal(s: &str) -> Result<Decimal> {
    let n = Decimal::from_str(s).map_err(|_| ApiError::bad("金額格式不正確"))?;
    if n.scale() > 8 || n.abs() >= Decimal::from(10_000_000_000_000_000i64) {
        return Err(ApiError::bad("金額超出範圍或小數位過多"));
    }
    Ok(n)
}
fn currency(s: &str) -> Result<()> {
    if s.len() != 3 || !s.bytes().all(|c| c.is_ascii_uppercase()) {
        return Err(ApiError::bad("幣別需為三碼大寫代碼"));
    }
    Ok(())
}
fn text(s: &str, max: usize) -> Result<()> {
    if s.trim().is_empty() || s.chars().count() > max {
        return Err(ApiError::bad("欄位為空或超出長度限制"));
    }
    Ok(())
}
fn validate_entry(e: &NewEntry) -> Result<Decimal> {
    text(&e.description, 500)?;
    text(&e.category, 80)?;
    if e.note.chars().count() > 2000 || e.tags.len() > 20 || e.tags.iter().any(|v| v.len() > 80) {
        return Err(ApiError::bad("備註或標籤過長"));
    }
    let n = decimal(&e.amount)?;
    if !matches!(e.kind.as_str(), "income" | "expense" | "adjustment")
        || n.is_zero()
        || (e.kind == "income" && n.is_sign_negative())
        || (e.kind == "expense" && n.is_sign_positive())
    {
        return Err(ApiError::bad("收入需為正數、支出需為負數"));
    }
    Ok(n)
}
const ACCOUNT_SQL: &str = "SELECT id,name,kind,currency,opening_balance::text,(current_balance+investment_value)::text AS balance,current_balance::text AS cash_balance,valuation_date,institution FROM accounts WHERE owner_id=$1 ORDER BY created_at";
const ENTRY_SQL: &str = "SELECT t.id,t.account_id,a.name AS account_name,a.currency,t.amount::text,t.kind,t.occurred_on,t.description,t.category,t.tags,t.note,t.transfer_id,t.split_parent_id,t.investment_trade_id FROM transactions t JOIN accounts a ON a.id=t.account_id WHERE NOT t.is_split_parent AND t.owner_id=$1";

#[utoipa::path(get,path="/api/v1/accounts",responses((status=200,body=Vec<Account>)))]
pub async fn accounts(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Account>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows = sqlx::query_as(ACCOUNT_SQL)
        .bind(user.id)
        .fetch_all(&mut *tx)
        .await?;
    Ok(Json(rows))
}
#[utoipa::path(post,path="/api/v1/accounts",request_body=NewAccount,responses((status=201,body=Account)))]
pub async fn create_account(
    user: Identity,
    State(s): State<AppState>,
    Json(a): Json<NewAccount>,
) -> Result<(StatusCode, Json<Account>)> {
    text(&a.name, 120)?;
    currency(&a.currency)?;
    if ![
        "cash",
        "bank",
        "credit",
        "investment",
        "property",
        "insurance",
        "loan",
        "other",
    ]
    .contains(&a.kind.as_str())
    {
        return Err(ApiError::bad("帳戶類型不正確"));
    }
    if let Some(i) = &a.institution {
        text(i, 120)?;
    }
    let balance = decimal(&a.opening_balance)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let row=sqlx::query_as("INSERT INTO accounts(id,owner_id,name,kind,currency,opening_balance,current_balance,institution) VALUES($1,$2,$3,$4,$5,$6,$6,$7) RETURNING id,name,kind,currency,opening_balance::text,current_balance::text AS balance,current_balance::text AS cash_balance,valuation_date,institution")
 .bind(Uuid::new_v4()).bind(user.id).bind(a.name.trim()).bind(a.kind).bind(a.currency).bind(balance).bind(a.institution).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}
#[utoipa::path(patch,path="/api/v1/accounts/{id}/opening-balance",params(("id"=Uuid,Path)),request_body=OpeningBalance,responses((status=200,body=Account),(status=400),(status=404),(status=409)))]
pub async fn edit_opening_balance(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(a): Json<OpeningBalance>,
) -> Result<Json<Account>> {
    let balance = decimal(&a.opening_balance)?;
    let expected = decimal(&a.expected_opening_balance)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    // Serialize with transaction balance triggers; never replace the current
    // balance from a stale browser snapshot or insert an income adjustment.
    let previous: Decimal = sqlx::query_scalar("SELECT opening_balance FROM accounts WHERE id=$1 AND owner_id=$2 FOR UPDATE")
        .bind(id).bind(user.id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::missing)?;
    if previous != expected {
        return Err(ApiError(StatusCode::CONFLICT, "期初餘額已變更，請重新開啟編輯後再試"));
    }
    let row = sqlx::query_as("UPDATE accounts SET current_balance=current_balance+$3-opening_balance,opening_balance=$3 WHERE id=$1 AND owner_id=$2 RETURNING id,name,kind,currency,opening_balance::text,(current_balance+investment_value)::text AS balance,current_balance::text AS cash_balance,valuation_date,institution")
        .bind(id).bind(user.id).bind(balance).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(row))
}

pub async fn delete_account(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM transactions WHERE account_id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if count > 0 {
        return Err(ApiError::bad("請先移除帳戶內的交易"));
    }
    sqlx::query("DELETE FROM shares WHERE account_id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    let r = sqlx::query("DELETE FROM accounts WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize, Default)]
pub struct EntryFilter {
    pub account_id: Option<Uuid>,
    pub q: Option<String>,
    pub offset: Option<i64>,
}
#[utoipa::path(get,path="/api/v1/transactions",responses((status=200,body=Vec<Entry>)))]
pub async fn transactions(
    user: Identity,
    State(s): State<AppState>,
    Query(f): Query<EntryFilter>,
) -> Result<Json<Vec<Entry>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_as(&format!("{ENTRY_SQL} AND ($2::uuid IS NULL OR t.account_id=$2) AND ($3::text IS NULL OR t.description ILIKE '%'||$3||'%' OR t.category ILIKE '%'||$3||'%' OR a.name ILIKE '%'||$3||'%') ORDER BY t.occurred_on DESC,t.created_at DESC,t.id LIMIT 200 OFFSET $4"))
 .bind(user.id).bind(f.account_id).bind(f.q).bind(f.offset.unwrap_or(0).max(0)).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}

// Both branches explicitly scope owner and preserve RLS. Source rows remain read-only.
/// 內部資金移動（自己帳戶間轉帳、繳卡費、買賣投資）不算收入或支出。
/// 內建三個之外，使用者自訂的「不算收支」分類（`user_categories.kind='flow'`）也排除。
/// 使用處的 `$1` 必須是帳本擁有者，並從 `feed` 查詢。
pub const NOT_FLOW: &str = "feed.category NOT IN ('轉帳','信用卡繳款','投資') AND NOT EXISTS(SELECT 1 FROM user_categories uc WHERE uc.owner_id=$1 AND uc.kind='flow' AND uc.name=feed.category)";
/// 手動與同步交易合併的收支清單。同步交易的分類依序採用：使用者逐筆設定 → 寫入時依規則／關鍵字推測的分類。
pub const FEED_SQL: &str = r#"WITH feed AS (
 SELECT t.id,t.account_id,a.name AS account_name,a.currency,t.amount,t.kind,t.occurred_on,t.description,t.category,t.tags,t.note,t.transfer_id,t.split_parent_id,t.investment_trade_id,false AS read_only,'posted'::text AS status,NULL::jsonb AS source_details,'manual'::text AS category_source,t.created_at AS sort_time
 FROM transactions t JOIN accounts a ON a.id=t.account_id AND a.owner_id=t.owner_id WHERE t.owner_id=$1 AND NOT t.is_split_parent
 UNION ALL
 SELECT r.id,a.id,a.data->>'name',r.data->>'currency',(r.data->>'amount')::numeric,
 coalesce(r.data->>'entry_type',CASE WHEN (r.data->>'amount')::numeric<0 THEN 'expense' ELSE 'income' END),left(r.data->>'date',10)::date,r.data->>'description',
 coalesce(l.category,r.auto_category,'未分類'),coalesce(l.tags,ARRAY['自動同步']::text[]),coalesce(l.note,r.data->>'memo',''),NULL::uuid,NULL::uuid,NULL::uuid,true,coalesce(r.data->>'status','posted'),r.data,
 CASE WHEN l.category IS NOT NULL THEN 'user' WHEN r.auto_category IS NOT NULL THEN r.auto_source END,r.observed_at
 FROM source_records r JOIN source_records a ON a.connection_id=r.connection_id AND a.owner_id=r.owner_id AND a.kind='account' AND a.source_key=r.account_key AND a.current
 LEFT JOIN source_tx_labels l ON l.record_id=r.id AND l.owner_id=r.owner_id
 WHERE r.owner_id=$1 AND r.kind='transaction' AND r.current AND r.data->>'date' IS NOT NULL
 AND NOT EXISTS(SELECT 1 FROM connections tc WHERE tc.id=r.connection_id AND tc.owner_id=r.owner_id AND tc.connector='tdcc')
) "#;

#[utoipa::path(get,path="/api/v1/activity-feed",responses((status=200,body=Vec<Value>)),tag="sources")]
pub async fn activity_feed(
    user: Identity,
    State(s): State<AppState>,
    Query(f): Query<EntryFilter>,
) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar(&format!("{FEED_SQL} SELECT to_jsonb(f)-'sort_time'-'amount'||jsonb_build_object('amount',amount::text) FROM feed f WHERE ($2::uuid IS NULL OR account_id=$2) AND ($3::text IS NULL OR description ILIKE '%'||$3||'%' OR category ILIKE '%'||$3||'%' OR account_name ILIKE '%'||$3||'%') ORDER BY occurred_on DESC,sort_time DESC,id LIMIT 200 OFFSET $4"))
        .bind(user.id).bind(f.account_id).bind(f.q).bind(f.offset.unwrap_or(0).max(0)).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
#[utoipa::path(post,path="/api/v1/transactions",request_body=NewEntry,responses((status=201,description="Created")))]
pub async fn create_transaction(
    user: Identity,
    State(s): State<AppState>,
    Json(e): Json<NewEntry>,
) -> Result<(StatusCode, Json<Value>)> {
    let amount = validate_entry(&e)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    own_account(&mut tx, user.id, e.account_id).await?;
    let id = Uuid::new_v4();
    insert_entry(&mut tx, user.id, id, &e, amount, None).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn own_account(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: Uuid,
    id: Uuid,
) -> Result<String> {
    sqlx::query_scalar("SELECT currency FROM accounts WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(ApiError::missing)
}
async fn insert_entry(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    id: Uuid,
    e: &NewEntry,
    n: Decimal,
    key: Option<String>,
) -> Result<u64> {
    Ok(sqlx::query("INSERT INTO transactions(id,owner_id,account_id,amount,kind,occurred_on,description,category,tags,note,import_key) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT(owner_id,import_key) DO NOTHING")
 .bind(id).bind(owner).bind(e.account_id).bind(n).bind(&e.kind).bind(e.occurred_on).bind(e.description.trim()).bind(&e.category).bind(&e.tags).bind(&e.note).bind(key).execute(&mut **tx).await?.rows_affected())
}
pub async fn delete_transaction(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let transfer: Option<(Option<Uuid>, Option<Uuid>,Option<Uuid>)> = sqlx::query_as(
        "SELECT transfer_id,split_parent_id,investment_trade_id FROM transactions WHERE id=$1 AND owner_id=$2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((transfer_id, parent_id, investment)) = transfer else {
        return Err(ApiError::missing());
    };
    if investment.is_some() {
        return Err(ApiError::bad("投資現金紀錄請至投資頁撤銷對應交易"));
    }
    sqlx::query("DELETE FROM transactions WHERE owner_id=$1 AND (id=$2 OR ($3::uuid IS NOT NULL AND transfer_id=$3))").bind(user.id).bind(parent_id.unwrap_or(id)).bind(transfer_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
pub struct Transfer {
    from: Uuid,
    to: Uuid,
    amount: String,
    received_amount: Option<String>,
    occurred_on: NaiveDate,
    description: String,
}
pub async fn transfer(
    user: Identity,
    State(s): State<AppState>,
    Json(e): Json<Transfer>,
) -> Result<Json<Value>> {
    if e.from == e.to {
        return Err(ApiError::bad("轉出與轉入帳戶需不同"));
    }
    text(&e.description, 500)?;
    let n = decimal(&e.amount)?;
    let received = e
        .received_amount
        .as_deref()
        .map(decimal)
        .transpose()?
        .unwrap_or(n);
    if n <= Decimal::ZERO || received <= Decimal::ZERO {
        return Err(ApiError::bad("轉帳金額需大於零"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let a = own_account(&mut tx, user.id, e.from).await?;
    let b = own_account(&mut tx, user.id, e.to).await?;
    if a != b && e.received_amount.is_none() {
        return Err(ApiError::bad("跨幣別轉帳請填寫實際入帳金額"));
    }
    if a == b && n != received {
        return Err(ApiError::bad("同幣別轉帳金額需一致，手續費請另記支出"));
    }
    let group = Uuid::new_v4();
    for (account, amount) in [(e.from, -n), (e.to, received)] {
        sqlx::query("INSERT INTO transactions(id,owner_id,account_id,amount,kind,occurred_on,description,category,transfer_id) VALUES($1,$2,$3,$4,'transfer',$5,$6,'轉帳',$7)").bind(Uuid::new_v4()).bind(user.id).bind(account).bind(amount).bind(e.occurred_on).bind(&e.description).bind(group).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"id":group})))
}
pub async fn dashboard(user: Identity, State(s): State<AppState>) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let totals:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('currency',currency,'assets',CASE WHEN count(*) FILTER(WHERE investment_value IS NULL)>0 THEN NULL ELSE sum(greatest(current_balance+investment_value,0))::text END,'liabilities',CASE WHEN count(*) FILTER(WHERE investment_value IS NULL)>0 THEN NULL ELSE sum(least(current_balance+investment_value,0))::text END,'net',CASE WHEN count(*) FILTER(WHERE investment_value IS NULL)>0 THEN NULL ELSE sum(current_balance+investment_value)::text END,'unknown_accounts',count(*) FILTER(WHERE investment_value IS NULL)) FROM accounts WHERE owner_id=$1 GROUP BY currency ORDER BY currency").bind(user.id).fetch_all(&mut *tx).await?;
    let monthly:Vec<Value>=sqlx::query_scalar(&format!("{FEED_SQL} SELECT jsonb_build_object('currency',currency,'income',coalesce(sum(amount) FILTER(WHERE kind='income'),0)::text,'expense',coalesce(-sum(amount) FILTER(WHERE kind IN ('expense','refund')),0)::text) FROM feed WHERE status<>'pending' AND {NOT_FLOW} AND occurred_on>=date_trunc('month',now() AT TIME ZONE 'Asia/Taipei')::date AND occurred_on<(date_trunc('month',now() AT TIME ZONE 'Asia/Taipei')+interval '1 month')::date GROUP BY currency")).bind(user.id).fetch_all(&mut *tx).await?;
    let categories:Vec<Value>=sqlx::query_scalar(&format!("{FEED_SQL} SELECT jsonb_build_object('category',category,'currency',currency,'amount',(-sum(amount))::text) FROM feed WHERE status<>'pending' AND {NOT_FLOW} AND kind IN ('expense','refund') AND occurred_on>=date_trunc('month',now() AT TIME ZONE 'Asia/Taipei')::date AND occurred_on<(date_trunc('month',now() AT TIME ZONE 'Asia/Taipei')+interval '1 month')::date GROUP BY category,currency ORDER BY sum(amount)")).bind(user.id).fetch_all(&mut *tx).await?;
    Ok(Json(
        json!({"totals":totals,"monthly":monthly,"categories":categories}),
    ))
}
fn csv_safe(s: &str) -> String {
    if matches!(s.chars().next(), Some('=' | '+' | '-' | '@' | '\t' | '\r')) {
        format!("'{s}")
    } else {
        s.into()
    }
}
pub async fn export(user: Identity, State(s): State<AppState>) -> Result<Response> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows: Vec<Entry> = sqlx::query_as(&format!("{FEED_SQL} SELECT id,account_id,account_name,currency,amount::text,kind,occurred_on,description,category,tags,note,transfer_id,split_parent_id,investment_trade_id FROM feed ORDER BY occurred_on,id"))
        .bind(user.id)
        .fetch_all(&mut *tx)
        .await?;
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record([
        "id",
        "account",
        "currency",
        "date",
        "amount",
        "kind",
        "description",
        "category",
        "note",
    ])
    .map_err(|_| ApiError::bad("匯出失敗"))?;
    for e in rows {
        w.write_record([
            e.id.to_string(),
            csv_safe(&e.account_name),
            e.currency,
            e.occurred_on.to_string(),
            e.amount,
            e.kind,
            csv_safe(&e.description),
            csv_safe(&e.category),
            csv_safe(&e.note),
        ])
        .map_err(|_| ApiError::bad("匯出失敗"))?;
    }
    let bytes = w.into_inner().map_err(|_| ApiError::bad("匯出失敗"))?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=transactions.csv",
            ),
        ],
        bytes,
    )
        .into_response())
}
#[derive(Deserialize)]
pub struct Import {
    account_id: Uuid,
    csv: String,
    #[serde(default)]
    commit: bool,
}
#[derive(Deserialize)]
struct CsvRow {
    date: NaiveDate,
    amount: String,
    kind: String,
    description: String,
    #[serde(default = "uncategorized")]
    category: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    id: String,
}
pub async fn import(
    user: Identity,
    State(s): State<AppState>,
    Json(input): Json<Import>,
) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    own_account(&mut tx, user.id, input.account_id).await?;
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(input.csv.trim_start_matches('\u{feff}').as_bytes());
    let mut entries = Vec::new();
    let file_hash = crate::auth::hash(&input.csv);
    for (index, row) in reader.deserialize::<CsvRow>().enumerate() {
        if index >= 5000 {
            return Err(ApiError::bad("每次最多匯入 5000 筆"));
        }
        let r = row.map_err(|_| ApiError::bad("CSV 欄位或日期格式錯誤，請使用範本"))?;
        let key = crate::auth::hash(&format!(
            "{}:{}",
            input.account_id,
            if r.id.is_empty() {
                format!("{file_hash}:{index}")
            } else {
                r.id
            }
        ));
        let e = NewEntry {
            account_id: input.account_id,
            amount: r.amount,
            kind: r.kind,
            occurred_on: r.date,
            description: r.description,
            category: r.category,
            tags: vec![],
            note: r.note,
        };
        let n = validate_entry(&e)?;
        entries.push((e, n, key));
    }
    let mut inserted = 0;
    let total = entries.len();
    if input.commit {
        for (e, n, key) in entries {
            inserted += insert_entry(&mut tx, user.id, Uuid::new_v4(), &e, n, Some(key)).await?;
        }
        tx.commit().await?;
    }
    Ok(Json(
        json!({"rows":total,"inserted":inserted,"preview":!input.commit,"deduplication":"依來源 id；無 id 時依原檔與列號，同檔重複匯入會略過"}),
    ))
}
#[derive(Deserialize)]
pub struct BudgetInput {
    month: String,
    category: String,
    amount: String,
    currency: String,
}
pub async fn budgets(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',b.id,'month',b.month,'category',b.category,'amount',b.amount::text,'currency',b.currency,'spent',(SELECT coalesce(-sum(t.amount),0)::text FROM transactions t JOIN accounts a ON a.id=t.account_id WHERE NOT t.is_split_parent AND t.owner_id=b.owner_id AND t.kind IN ('expense','refund') AND t.category=b.category AND a.currency=b.currency AND to_char(t.occurred_on,'YYYY-MM')=b.month)) FROM budgets b WHERE owner_id=$1 ORDER BY month DESC,category").bind(user.id).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
pub async fn save_budget(
    user: Identity,
    State(s): State<AppState>,
    Json(b): Json<BudgetInput>,
) -> Result<Json<Value>> {
    NaiveDate::parse_from_str(&format!("{}-01", b.month), "%Y-%m-%d")
        .map_err(|_| ApiError::bad("月份格式需為 YYYY-MM"))?;
    text(&b.category, 80)?;
    currency(&b.currency)?;
    let n = decimal(&b.amount)?;
    if n <= Decimal::ZERO {
        return Err(ApiError::bad("預算需大於零"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let id:Uuid=sqlx::query_scalar("INSERT INTO budgets(id,owner_id,month,category,amount,currency) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(owner_id,month,category,currency) DO UPDATE SET amount=EXCLUDED.amount RETURNING id").bind(Uuid::new_v4()).bind(user.id).bind(b.month).bind(b.category).bind(n).bind(b.currency).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
#[derive(Deserialize)]
pub struct GoalInput {
    #[serde(default)]
    id: Option<Uuid>,
    name: String,
    target: String,
    saved: String,
    currency: String,
    due_on: Option<NaiveDate>,
}
pub async fn goals(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'target',target::text,'saved',saved::text,'currency',currency,'due_on',due_on) FROM goals WHERE owner_id=$1 ORDER BY due_on NULLS LAST,name").bind(user.id).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
pub async fn save_goal(
    user: Identity,
    State(s): State<AppState>,
    Json(g): Json<GoalInput>,
) -> Result<Json<Value>> {
    text(&g.name, 120)?;
    currency(&g.currency)?;
    let target = decimal(&g.target)?;
    let saved = decimal(&g.saved)?;
    if target <= Decimal::ZERO || saved < Decimal::ZERO {
        return Err(ApiError::bad("目標金額需為正數，已存金額需為非負数"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let id = g.id.unwrap_or_else(Uuid::new_v4);
    sqlx::query("INSERT INTO goals(id,owner_id,name,target,saved,currency,due_on) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(id) DO UPDATE SET name=EXCLUDED.name,target=EXCLUDED.target,saved=EXCLUDED.saved,currency=EXCLUDED.currency,due_on=EXCLUDED.due_on WHERE goals.owner_id=$2").bind(id).bind(user.id).bind(g.name).bind(target).bind(saved).bind(g.currency).bind(g.due_on).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_lossy_or_extreme_amounts() {
        assert!(decimal("0.000000001").is_err());
        assert!(decimal("10000000000000000").is_err());
        assert_eq!(
            decimal("0.1").unwrap() + decimal("0.2").unwrap(),
            decimal("0.3").unwrap()
        );
    }
    #[test]
    fn prevents_spreadsheet_formulas() {
        assert_eq!(csv_safe("=HYPERLINK(\"bad\")"), "'=HYPERLINK(\"bad\")");
        assert_eq!(csv_safe("午餐"), "午餐");
    }
}
