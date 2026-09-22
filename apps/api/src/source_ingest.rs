use crate::{
    error::{ApiError, Result},
    finance::decimal,
    state::{AppState, tenant},
    vault,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub kind: String,
    pub source_id: String,
    pub account_key: Option<String>,
    pub data: Value,
}
#[derive(Serialize, Deserialize)]
pub struct Batch {
    pub rows: Vec<Record>,
    pub partial: bool,
    pub state: Value,
}
fn hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub fn validate(batch: &Batch) -> Result<()> {
    if batch.rows.len() > 50000
        || batch.state.to_string().len() > 512 * 1024
        || serde_json::to_vec(batch).map_or(true, |v| v.len() > 32 * 1024 * 1024)
    {
        return Err(ApiError::bad("來源資料超過單次上限"));
    }
    let mut keys = HashSet::new();
    let mut accounts = HashSet::new();
    for r in &batch.rows {
        if r.kind == "account" {
            accounts.insert(r.source_id.as_str());
        }
    }
    for r in &batch.rows {
        if !hash(&r.source_id)
            || r.account_key.as_ref().is_some_and(|s| !hash(s))
            || !keys.insert((&r.kind, &r.source_id))
        {
            return Err(ApiError::bad("來源識別碼重複或格式錯誤"));
        }
        let allowed: &[&str] = match r.kind.as_str() {
            "account" => &[
                "name",
                "institution",
                "account_type",
                "balance_convention",
                "tdcc_account_key",
                "tdcc_formatted_key",
                "transactions_from",
                "transactions_to",
                "currency",
                "balance",
                "available_balance",
                "credit_limit",
                "as_of",
                "payment_due",
                "statement_date",
                "opened_on",
                "maturity_on",
                "mask",
                "interest_rate",
                "term",
                "next_payment_amount",
                "last_payment_amount",
                "last_payment_on",
                "remaining_installments",
            ],
            "transaction" => &[
                "amount",
                "currency",
                "date",
                "description",
                "status",
                "transfer_peer",
                "entry_type",
                "original_currency",
                "original_amount",
                "fee",
                "balance",
                "card_mask",
                "statement_period",
                "posted_date",
                "source_date",
                "memo",
                "source_status",
            ],
            "card" => &["name", "mask", "currency", "card_type", "status"],
            "bill" => &[
                "period",
                "currency",
                "statement_amount",
                "minimum_payment",
                "paid_amount",
                "is_paid",
                "payment_due",
                "statement_date",
            ],
            "holding" => &[
                "unit_price", "price_date", "unrealized_gain", "accumulated_dividend",
                "adjusted_dividend", "adjusted_dividend_with_conversion", "recurring_amount",
                "investment_started_on", "recurring_days", "successful_debits", "source_status", "fund_company",
                "name",
                "symbol",
                "asset_type",
                "currency",
                "quantity",
                "market_value",
                "cost_basis",
                "as_of",
            ],
            "investment_transaction" => &[
                "name",
                "symbol",
                "date",
                "quantity",
                "price",
                "amount",
                "description",
                "currency",
            ],
            "invoice" => &["number", "date", "seller", "amount", "currency", "status", "carrier", "detail_status", "donated", "has_allowance"],
            "invoice_item" => &[
                "invoice_key",
                "line_number",
                "description",
                "quantity",
                "unit_price",
                "amount",
                "currency",
            ],
            "net_worth" => &["date", "amount", "asset_type", "currency"],
            _ => return Err(ApiError::bad("來源紀錄類型錯誤")),
        };
        let data = r
            .data
            .as_object()
            .ok_or_else(|| ApiError::bad("來源內容格式錯誤"))?;
        if data.keys().any(|k| !allowed.contains(&k.as_str())) || r.data.to_string().len() > 8192 {
            return Err(ApiError::bad("來源包含未接受的欄位"));
        }
        let code = data.get("currency").and_then(Value::as_str).unwrap_or("");
        if code.len() != 3 || !code.bytes().all(|c| c.is_ascii_uppercase()) {
            return Err(ApiError::bad("來源幣別格式錯誤"));
        }
        for (k, v) in data {
            if v.is_null() {
                continue;
            }
            if k == "entry_type"
                && !v.as_str().is_some_and(|x| {
                    matches!(
                        x,
                        "expense" | "income" | "transfer" | "refund" | "adjustment"
                    )
                })
            {
                return Err(ApiError::bad("來源收支類型錯誤"));
            }
            if k == "original_currency"
                && !v
                    .as_str()
                    .is_some_and(|x| x.len() == 3 && x.bytes().all(|b| b.is_ascii_uppercase()))
            {
                return Err(ApiError::bad("來源原幣格式錯誤"));
            }
            if matches!(k.as_str(), "is_paid" | "donated" | "has_allowance") {
                if !v.is_boolean() {
                    return Err(ApiError::bad("來源狀態格式錯誤"));
                }
            } else if k == "line_number" {
                if !v.as_u64().is_some_and(|n| n > 0 && n <= 100000) {
                    return Err(ApiError::bad("來源品項序號格式錯誤"));
                }
            } else if !v.is_string() {
                return Err(ApiError::bad("來源欄位必須是純文字或明確型別"));
            }
            // 帳號遮罩只允許末四碼，避免完整帳號進入資料庫。
            if matches!(k.as_str(), "mask" | "card_mask")
                && !v.as_str().is_some_and(|m| {
                    m.len() == 8
                        && m.starts_with("****")
                        && m[4..].bytes().all(|c| c.is_ascii_digit())
                })
            {
                return Err(ApiError::bad("來源帳號遮罩格式錯誤"));
            }
            if matches!(
                k.as_str(),
                "balance"
                    | "available_balance"
                    | "credit_limit"
                    | "amount"
                    | "statement_amount"
                    | "minimum_payment"
                    | "paid_amount"
                    | "quantity"
                    | "price"
                    | "market_value"
                    | "cost_basis"
                    | "unrealized_gain" | "accumulated_dividend" | "adjusted_dividend" | "adjusted_dividend_with_conversion" | "recurring_amount"
                    | "unit_price"
                    | "interest_rate"
                    | "original_amount"
                    | "fee"
                    | "next_payment_amount"
                    | "last_payment_amount"
            ) {
                decimal(
                    v.as_str()
                        .ok_or_else(|| ApiError::bad("來源金額須為十進位字串"))?,
                )?;
            } else if matches!(
                k.as_str(),
                "date"
                    | "as_of"
                    | "payment_due"
                    | "statement_date"
                    | "opened_on"
                    | "price_date" | "investment_started_on"
                    | "maturity_on"
                    | "posted_date"
                    | "transactions_from"
                    | "transactions_to"
                    | "last_payment_on"
            ) {
                let date = v
                    .as_str()
                    .ok_or_else(|| ApiError::bad("來源日期格式錯誤"))?;
                let day = date
                    .get(..10)
                    .ok_or_else(|| ApiError::bad("來源日期格式錯誤"))?;
                chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
                    .map_err(|_| ApiError::bad("來源日期格式錯誤"))?;
                if date.len() > 10 {
                    chrono::DateTime::parse_from_rfc3339(date)
                        .map_err(|_| ApiError::bad("來源時間缺少時區"))?;
                }
            } else if k == "remaining_installments" {
                if v.as_str().and_then(|s| s.parse::<u32>().ok()).is_none_or(|n| n > 10000) {
                    return Err(ApiError::bad("來源剩餘期數格式錯誤"));
                }
            } else if v.as_str().is_some_and(|s| s.chars().count() > 500) {
                return Err(ApiError::bad("來源文字過長"));
            }
        }
        if matches!(r.kind.as_str(), "transaction" | "bill")
            && !r
                .account_key
                .as_ref()
                .is_some_and(|id| accounts.contains(id.as_str()))
        {
            return Err(ApiError::bad("來源交易缺少對應帳戶"));
        }
        if matches!(r.kind.as_str(), "transaction" | "invoice" | "invoice_item")
            && r.data["amount"].is_null()
        {
            return Err(ApiError::bad("來源交易金額缺漏"));
        }
    }
    Ok(())
}
/// 找出第一筆未通過檢查的資料：只回報類型、原因與欄位名稱，不含任何值。
pub fn explain(batch: &Batch) -> Option<String> {
    let accounts: Vec<Record> = batch
        .rows
        .iter()
        .filter(|r| r.kind == "account")
        .cloned()
        .collect();
    for r in &batch.rows {
        let mut rows = accounts.clone();
        if r.kind != "account" {
            rows.push(r.clone());
        } else {
            rows.retain(|a| a.source_id != r.source_id);
            rows.push(r.clone());
        }
        let single = Batch {
            rows,
            state: Value::Null,
            partial: batch.partial,
        };
        if let Err(e) = validate(&single) {
            let keys: Vec<String> = r
                .data
                .as_object()
                .map(|o| {
                    o.iter()
                        .map(|(k, v)| {
                            let t = match v {
                                Value::Null => "null",
                                Value::String(_) => "text",
                                Value::Number(_) => "number",
                                Value::Bool(_) => "bool",
                                _ => "object",
                            };
                            format!("{k}:{t}")
                        })
                        .collect()
                })
                .unwrap_or_default();
            return Some(format!("{}（{}）欄位 {}", e.1, r.kind, keys.join(", ")));
        }
    }
    validate(batch).err().map(|e| e.1.to_string())
}

pub async fn stage(
    s: &AppState,
    owner: Uuid,
    email: &str,
    job: Uuid,
    connection: Uuid,
    batch: &Batch,
) -> Result<()> {
    validate(batch)?;
    let sealed = vault::seal(
        owner,
        job,
        "batch",
        &serde_json::to_value(batch).map_err(|_| ApiError::bad("來源資料格式錯誤"))?,
    )?;
    let mut tx = tenant(&s.db, owner, email).await?;
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sync_jobs WHERE id=$1 AND owner_id=$2 AND connection_id=$3 AND status IN ('running','waiting_user'))").bind(job).bind(owner).bind(connection).fetch_one(&mut *tx).await?;
    if !active {
        return Err(ApiError::bad("同步工作已結束"));
    }
    sqlx::query("INSERT INTO source_batches(job_id,owner_id,connection_id,cipher) VALUES($1,$2,$3,$4) ON CONFLICT(job_id) DO NOTHING").bind(job).bind(owner).bind(connection).bind(sealed).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn commit(s: &AppState, owner: Uuid, email: &str, job: Uuid) -> Result<()> {
    // A completed revocation cannot race with the final write of a financial batch.
    let mut permission = s.auth_db.begin().await?;
    let approved: bool =
        sqlx::query_scalar("SELECT is_admin OR sync_approved FROM users WHERE id=$1 FOR SHARE")
            .bind(owner)
            .fetch_one(&mut *permission)
            .await?;
    let mut tx = tenant(&s.db, owner, email).await?;
    let record:Option<(Uuid,String,i32,String,Option<String>)>=sqlx::query_as("SELECT j.connection_id,j.status,j.source_revision,c.connector,b.cipher FROM sync_jobs j JOIN source_batches b ON b.job_id=j.id JOIN connections c ON c.id=j.connection_id WHERE j.id=$1 AND j.owner_id=$2 AND b.owner_id=$2 FOR UPDATE OF j,c,b").bind(job).bind(owner).fetch_optional(&mut *tx).await?;
    let (connection, status, revision, _source, payload) = record.ok_or_else(ApiError::missing)?;
    if payload.is_none() {
        return Ok(());
    }
    let valid: bool = sqlx::query_scalar(
        "SELECT enabled AND revision=$1 FROM connections WHERE id=$2 AND owner_id=$3",
    )
    .bind(revision)
    .bind(connection)
    .bind(owner)
    .fetch_one(&mut *tx)
    .await?;
    if !approved || !valid || !matches!(status.as_str(), "running" | "waiting_user") {
        sqlx::query("DELETE FROM source_batches WHERE job_id=$1 AND owner_id=$2")
            .bind(job)
            .bind(owner)
            .execute(&mut *tx)
            .await?;
        if !approved {
            sqlx::query("UPDATE sync_jobs SET status='cancelled',message='本機同步資格已撤銷',challenge=NULL,answer_cipher=NULL,lease_until=NULL,updated_at=now() WHERE id=$1 AND owner_id=$2 AND status IN ('running','waiting_user')").bind(job).bind(owner).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        return Ok(());
    }
    let batch: Batch = serde_json::from_value(vault::open(owner, job, "batch", &payload.unwrap())?)
        .map_err(|_| ApiError::bad("來源資料格式錯誤"))?;
    validate(&batch)?;
    if !batch.partial {
        sqlx::query("UPDATE source_records SET current=false WHERE connection_id=$1 AND owner_id=$2 AND kind IN ('account','holding')").bind(connection).bind(owner).execute(&mut *tx).await?;
        // Reconcile complete HTTP snapshots within each returned account's date range.
        // Old rows remain available for audit; they stop contributing to the live feed.
        let http = batch
            .state
            .get("cursor")
            .and_then(Value::as_str)
            .and_then(|v| serde_json::from_str::<Value>(v).ok())
            .is_some_and(|v| v["transport"] == "http" && v["version"] == 2);
        let native = batch.state["cursor"].as_str().and_then(|v|serde_json::from_str::<Value>(v).ok()).unwrap_or(Value::Null);
        if _source == "hsbc" && native["transport"] == "hsbc-http" && native["version"] == 1 {
            if let (Some(start), Some(end)) = (native["coverage"]["start"].as_str(), native["coverage"]["end"].as_str()) {
                if chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").is_ok() && chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d").is_ok() && start <= end {
                    let keys: Vec<String> = batch.rows.iter().filter(|r| r.kind == "account").map(|r| r.source_id.clone()).collect();
                    sqlx::query("UPDATE source_records SET current=false WHERE owner_id=$1 AND connection_id=$2 AND account_key=ANY($3) AND kind='transaction' AND left(data->>'date',10) BETWEEN $4 AND $5")
                        .bind(owner).bind(connection).bind(keys).bind(start).bind(end).execute(&mut *tx).await?;
                }
            }
        }
        if _source == "einvoice" && native["transport"] == "einvoice-web" && native["version"] == 1 {
            if let (Some(start), Some(end)) = (native["coverage"]["start"].as_str(), native["coverage"]["end"].as_str()) {
                if chrono::NaiveDate::parse_from_str(start,"%Y-%m-%d").is_ok() && chrono::NaiveDate::parse_from_str(end,"%Y-%m-%d").is_ok() && start <= end {
                    // Invalidate only the fully queried window; retain older history and all partial results.
                    sqlx::query("UPDATE source_records r SET current=false WHERE r.owner_id=$1 AND r.connection_id=$2 AND ((r.kind='invoice' AND left(r.data->>'date',10) BETWEEN $3 AND $4) OR (r.kind='invoice_item' AND EXISTS(SELECT 1 FROM source_records i WHERE i.owner_id=$1 AND i.connection_id=$2 AND i.kind='invoice' AND i.source_key=r.data->>'invoice_key' AND left(i.data->>'date',10) BETWEEN $3 AND $4)))")
                        .bind(owner).bind(connection).bind(start).bind(end).execute(&mut *tx).await?;
                }
            }
        }
        if native["transport"] == "http" && native["version"] == 1 {
            let keys:Vec<String>=batch.rows.iter().filter(|r|r.kind=="account").map(|r|r.source_id.clone()).collect();
            if matches!(_source.as_str(),"esun"|"firstbank"|"fubon"|"yuanta") && native["coverage"]["unbilledComplete"]==true {
                let periods:Vec<String>=native["coverage"]["billPeriods"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|s|s.len()==6&&s.bytes().all(|b|b.is_ascii_digit())).map(|s|format!("{}-{}",&s[..4],&s[4..])).collect();
                sqlx::query("UPDATE source_records SET current=false WHERE owner_id=$1 AND connection_id=$2 AND account_key=ANY($3) AND (kind='card' OR (kind='bill' AND data->>'period'=ANY($4)) OR (kind='transaction' AND (data->>'status'='pending' OR data->>'statement_period'=ANY($4))))").bind(owner).bind(connection).bind(&keys).bind(&periods).execute(&mut *tx).await?;
            }
            if matches!(_source.as_str(),"sinopac"|"firstbank"|"fubon"|"richart"|"hncb"|"landbank"|"yuanta") {
                let keys:Vec<String>=batch.rows.iter().filter(|r|r.kind=="account" && r.data["account_type"]=="checking").map(|r|r.source_id.clone()).collect();
                if let (Some(start),Some(end))=(native["coverage"]["depositStart"].as_str(),native["coverage"]["end"].as_str()) {
                    if chrono::NaiveDate::parse_from_str(start,"%Y-%m-%d").is_ok() && chrono::NaiveDate::parse_from_str(end,"%Y-%m-%d").is_ok() && start<=end {
                        sqlx::query("UPDATE source_records SET current=false WHERE owner_id=$1 AND connection_id=$2 AND account_key=ANY($3) AND kind='transaction' AND data->>'date'>=$4 AND data->>'date'<=$5").bind(owner).bind(connection).bind(&keys).bind(start).bind(end).execute(&mut *tx).await?;
                    }
                }
            }
        }
        if _source == "cathaybk" && http {
            sqlx::query("UPDATE source_records SET current=false WHERE connection_id=$1 AND owner_id=$2 AND (kind='card' OR (kind='transaction' AND data->>'status'='pending'))")
                .bind(connection).bind(owner).execute(&mut *tx).await?;
            // A successful empty window must retire stale rows too. Returned min/max dates
            // cannot establish coverage at either edge of the queried interval.
            for row in batch.rows.iter().filter(|r|r.kind=="account" && r.data["account_type"]!="credit") {
                let field=if row.data["currency"]=="TWD" {"twdStart"}else{"fxStart"};
                if let (Some(start),Some(end))=(native["coverage"][field].as_str(),native["coverage"]["end"].as_str()) {
                    if chrono::NaiveDate::parse_from_str(start,"%Y-%m-%d").is_ok() && chrono::NaiveDate::parse_from_str(end,"%Y-%m-%d").is_ok() && start<=end {
                        sqlx::query("UPDATE source_records SET current=false WHERE connection_id=$1 AND owner_id=$2 AND kind='transaction' AND account_key=$3 AND left(data->>'date',10) BETWEEN $4 AND $5")
                            .bind(connection).bind(owner).bind(&row.source_id).bind(start).bind(end).execute(&mut *tx).await?;
                    }
                }
            }
            let periods:Vec<String>=native["coverage"]["billPeriods"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|p|chrono::NaiveDate::parse_from_str(&format!("{p}-01"),"%Y-%m-%d").is_ok()).map(str::to_owned).collect();
            sqlx::query("UPDATE source_records SET current=false WHERE connection_id=$1 AND owner_id=$2 AND ((kind='bill' AND data->>'period'=ANY($3)) OR (kind='transaction' AND data->>'statement_period'=ANY($3)))")
                .bind(connection).bind(owner).bind(&periods).execute(&mut *tx).await?;
        }
    }
    let mut summary = BTreeMap::<String, usize>::new();
    for r in &batch.rows {
        *summary.entry(r.kind.clone()).or_default() += 1;
        sqlx::query("INSERT INTO source_records(id,owner_id,connection_id,kind,source_key,account_key,data,last_job_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(connection_id,kind,source_key) DO UPDATE SET data=EXCLUDED.data,account_key=EXCLUDED.account_key,observed_at=now(),last_job_id=EXCLUDED.last_job_id,current=true WHERE source_records.owner_id=$2").bind(Uuid::new_v4()).bind(owner).bind(connection).bind(&r.kind).bind(&r.source_id).bind(&r.account_key).bind(&r.data).bind(job).execute(&mut *tx).await?;
        // 每次同步留下當日餘額／市值，供資產走勢使用；未知金額不寫入。
        let field = match r.kind.as_str() {
            "account" => "balance",
            "holding" => "market_value",
            _ => continue,
        };
        if let Some(amount) = r.data.get(field).and_then(Value::as_str) {
            let as_of = r
                .data
                .get("as_of")
                .and_then(Value::as_str)
                .and_then(|s| s.get(..10))
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            sqlx::query("INSERT INTO source_balance_history(owner_id,connection_id,kind,source_key,date,amount,currency) VALUES($1,$2,$3,$4,coalesce($5,(now() AT TIME ZONE 'Asia/Taipei')::date),$6::numeric,$7) ON CONFLICT(connection_id,kind,source_key,date) DO UPDATE SET amount=EXCLUDED.amount,currency=EXCLUDED.currency,updated_at=now()")
                .bind(owner).bind(connection).bind(&r.kind).bind(&r.source_id).bind(as_of).bind(amount)
                .bind(r.data.get("currency").and_then(Value::as_str).unwrap_or("TWD"))
                .execute(&mut *tx).await?;
        }
    }
    if !batch.partial
        && _source == "cathaybk"
        && batch.state["cursor"]
            .as_str()
            .and_then(|v| serde_json::from_str::<Value>(v).ok())
            .is_some_and(|v| v["transport"] == "http" && v["version"] == 2)
    {
        // Legacy DOM account IDs may use a shorter account representation. Preserve
        // those records for audit without counting an obsolete account twice.
        sqlx::query("UPDATE source_records r SET current=false WHERE r.owner_id=$1 AND r.connection_id=$2 AND r.kind IN ('transaction','bill','card') AND NOT EXISTS(SELECT 1 FROM source_records a WHERE a.owner_id=r.owner_id AND a.connection_id=r.connection_id AND a.kind='account' AND a.source_key=r.account_key AND a.current)")
            .bind(owner).bind(connection).execute(&mut *tx).await?;
    }
    let sealed = if _source == "hsbc" { None } else {
        Some(vault::seal(owner, connection, "state", &batch.state)?)
    };
    sqlx::query("UPDATE connections SET state_cipher=$1,last_success_at=CASE WHEN $2 THEN last_success_at ELSE now() END,updated_at=now() WHERE id=$3 AND owner_id=$4").bind(sealed).bind(batch.partial).bind(connection).bind(owner).execute(&mut *tx).await?;
    let state = if batch.partial {
        "partial"
    } else {
        "succeeded"
    };
    let message = if batch.partial {
        "部分資料已儲存；請查看來源與資料日期"
    } else {
        "金融資料同步完成"
    };
    sqlx::query("UPDATE sync_jobs SET status=$1,message=$2,summary=$3,challenge=NULL,answer_cipher=NULL,lease_until=NULL,updated_at=now() WHERE id=$4 AND owner_id=$5").bind(state).bind(message).bind(json!(summary)).bind(job).bind(owner).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE source_batches SET cipher=NULL,committed_at=now() WHERE job_id=$1 AND owner_id=$2",
    )
    .bind(job)
    .bind(owner)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO notifications(id,owner_id,title,body) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4())
        .bind(owner)
        .bind(message)
        .bind(format!(
            "本次處理 {} 筆來源資料，可在資料來源查看。",
            batch.rows.len()
        ))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    permission.commit().await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loan_repayment_fields_validate_without_accepting_malformed_values() {
        let mut b = Batch { rows: vec![Record { kind: "account".into(), source_id: "a".repeat(64), account_key: None,
            data: json!({"currency":"TWD","balance":"-1000","next_payment_amount":"50.25","last_payment_amount":"45","last_payment_on":"2026-09-01","remaining_installments":"24"}) }], partial: false, state: json!({}) };
        assert!(validate(&b).is_ok());
        b.rows[0].data["remaining_installments"] = json!("-1"); assert!(validate(&b).is_err());
        b.rows[0].data["remaining_installments"] = json!("24");
        b.rows[0].data["last_payment_on"] = json!("2026-02-30"); assert!(validate(&b).is_err());
        b.rows[0].data["last_payment_on"] = Value::Null;
        b.rows[0].data["next_payment_amount"] = json!(1.2); assert!(validate(&b).is_err());
    }
    #[test]
    fn validation_preserves_unknown_and_rejects_extra_secret() {
        let mut b = Batch {
            rows: vec![Record {
                kind: "account".into(),
                source_id: "a".repeat(64),
                account_key: None,
                data: json!({"currency":"TWD","balance":null,"name":"測試"}),
            }],
            partial: false,
            state: json!({}),
        };
        assert!(validate(&b).is_ok());
        b.rows[0].data["password"] = json!("fixture");
        assert!(validate(&b).is_err());
    }
    #[test]
    fn rejects_float_and_orphan() {
        let mut b = Batch {
            rows: vec![Record {
                kind: "transaction".into(),
                source_id: "b".repeat(64),
                account_key: Some("a".repeat(64)),
                data: json!({"currency":"TWD","amount":"-0.1"}),
            }],
            partial: false,
            state: json!({}),
        };
        assert!(validate(&b).is_err());
        b.rows[0].kind = "invoice".into();
        b.rows[0].data["amount"] = json!(-0.1);
        assert!(validate(&b).is_err());
    }
}

