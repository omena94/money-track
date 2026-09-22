use crate::{
    auth::Identity,
    error::{ApiError, Result},
    finance::decimal,
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::{NaiveDate, Timelike};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
const SOURCES: [(&str, &str); 2] = [
    (
        "TWSE",
        "https://openapi.twse.com.tw/v1/exchangeReport/STOCK_DAY_ALL",
    ),
    (
        "TPEX",
        "https://www.tpex.org.tw/openapi/v1/tpex_mainboard_quotes",
    ),
];
#[derive(Deserialize)]
pub struct Search {
    q: Option<String>,
}
pub async fn quotes(
    _user: Identity,
    State(s): State<AppState>,
    Query(q): Query<Search>,
) -> Result<Json<Value>> {
    let query = q.q.unwrap_or_default();
    if query.len() > 120 {
        return Err(ApiError::bad("查詢字數過長"));
    }
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('market',market,'symbol',symbol,'name',name,'price',price::text,'quoted_on',quoted_on,'fetched_at',fetched_at) FROM market_quotes WHERE symbol ILIKE $1||'%' OR name ILIKE '%'||$1||'%' ORDER BY symbol LIMIT 50").bind(query).fetch_all(&s.db).await?;
    let updates:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('source',source,'last_success',last_success,'last_attempt',last_attempt,'rows',rows,'message',message) FROM market_updates ORDER BY source").fetch_all(&s.db).await?;
    Ok(Json(json!({"quotes":rows,"sources":updates})))
}
pub async fn refresh(user: Identity, State(s): State<AppState>) -> Result<Json<Value>> {
    if !user.is_admin {
        return Err(ApiError::forbidden());
    }
    refresh_prices(&s, true).await?;
    Ok(Json(json!({"ok":true})))
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError(StatusCode::BAD_GATEWAY, "官方行情格式已變更"))
}
pub fn source_date(s: &str) -> Option<NaiveDate> {
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    if digits.len() == 7 {
        let year = digits[..3].parse::<i32>().ok()? + 1911;
        NaiveDate::from_ymd_opt(year, digits[3..5].parse().ok()?, digits[5..].parse().ok()?)
    } else if digits.len() == 8 {
        NaiveDate::parse_from_str(&digits, "%Y%m%d").ok()
    } else {
        None
    }
}
fn parse_row(source: &str, v: &Value) -> Result<Option<(String, String, Decimal, NaiveDate)>> {
    let (symbol_key, name_key, price_key) = if source == "TWSE" {
        ("Code", "Name", "ClosingPrice")
    } else {
        ("SecuritiesCompanyCode", "CompanyName", "Close")
    };
    let symbol = text(v, symbol_key)?;
    let name = text(v, name_key)?;
    let raw = text(v, price_key)?.replace(',', "");
    if raw.trim().is_empty() || matches!(raw.trim(), "--" | "---" | "----" | "N/A") {
        return Ok(None);
    }
    let price =
        decimal(&raw).map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "官方行情價格格式已變更"))?;
    if price <= Decimal::ZERO {
        return Ok(None);
    }
    let date = source_date(text(v, "Date")?)
        .ok_or_else(|| ApiError(StatusCode::BAD_GATEWAY, "官方行情日期格式已變更"))?;
    let today = (chrono::Utc::now() + chrono::Duration::hours(8)).date_naive();
    if date > today
        || date < NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()
        || symbol.is_empty()
        || symbol.len() > 30
    {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "官方行情資料日期或代碼不合理",
        ));
    }
    Ok(Some((symbol.to_string(), name.to_string(), price, date)))
}
pub async fn refresh_prices(s: &AppState, manual: bool) -> Result<()> {
    let local = chrono::Utc::now() + chrono::Duration::hours(8);
    for (source, url) in SOURCES {
        let mut lock = s.db.begin().await?;
        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtext($1))")
            .bind(format!("money-market:{source}"))
            .fetch_one(&mut *lock)
            .await?;
        if !acquired {
            continue;
        }
        let last: Option<(
            chrono::DateTime<chrono::Utc>,
            Option<chrono::DateTime<chrono::Utc>>,
        )> = sqlx::query_as("SELECT last_attempt,last_success FROM market_updates WHERE source=$1")
            .bind(source)
            .fetch_optional(&mut *lock)
            .await?;
        if let Some((attempt, success)) = last {
            if chrono::Utc::now() - attempt < chrono::Duration::minutes(15) {
                continue;
            }
            if !manual
                && success.is_some_and(|d| {
                    let t = d + chrono::Duration::hours(8);
                    t.date_naive() == local.date_naive() && t.hour() >= 17
                })
            {
                continue;
            }
            if !manual && success.is_some() && local.hour() < 17 {
                continue;
            }
        }
        sqlx::query("INSERT INTO market_updates(source,last_attempt,message) VALUES($1,now(),'更新中') ON CONFLICT(source) DO UPDATE SET last_attempt=now(),message='更新中'").bind(source).execute(&mut *lock).await?;
        let response = fetch_source(s, source, url).await;
        match response {
            Ok(rows) => {
                for (symbol, name, price, date) in &rows {
                    sqlx::query("INSERT INTO market_quotes(market,symbol,name,price,quoted_on) VALUES($1,$2,$3,$4,$5) ON CONFLICT(market,symbol) DO UPDATE SET name=EXCLUDED.name,price=EXCLUDED.price,quoted_on=EXCLUDED.quoted_on,fetched_at=now() WHERE market_quotes.quoted_on<=EXCLUDED.quoted_on").bind(source).bind(symbol).bind(name).bind(price).bind(date).execute(&mut *lock).await?;
                }
                sqlx::query("UPDATE market_updates SET last_success=now(),rows=$1,message='官方日收盤行情已更新' WHERE source=$2").bind(rows.len() as i32).bind(source).execute(&mut *lock).await?;
            }
            Err(_) => {
                sqlx::query("UPDATE market_updates SET message='官方來源暫時未取得有效行情，保留上次資料' WHERE source=$1").bind(source).execute(&mut *lock).await?;
            }
        }
        lock.commit().await?;
    }
    let users: Vec<(Uuid, String)> = sqlx::query_as("SELECT id,email FROM users")
        .fetch_all(&s.auth_db)
        .await?;
    for (owner, email) in users {
        let mut tx = tenant(&s.db, owner, &email).await?;
        sqlx::query("SELECT id FROM accounts WHERE owner_id=$1 AND kind='investment' ORDER BY id FOR UPDATE").bind(owner).fetch_all(&mut *tx).await?;
        sqlx::query("UPDATE investment_positions p SET last_price=m.price,price_date=m.quoted_on,price_source=m.market FROM market_quotes m WHERE p.owner_id=$1 AND p.market=m.market AND p.symbol=m.symbol AND (p.price_date IS NULL OR p.price_date<m.quoted_on OR (p.price_date=m.quoted_on AND p.price_source<>'manual'))").bind(owner).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    Ok(())
}
async fn fetch_source(
    s: &AppState,
    source: &str,
    url: &str,
) -> Result<Vec<(String, String, Decimal, NaiveDate)>> {
    let response = s
        .http
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await?
        .error_for_status()?;
    if response
        .content_length()
        .is_some_and(|n| n > 16 * 1024 * 1024)
    {
        return Err(ApiError(StatusCode::BAD_GATEWAY, "行情資料超出範圍"));
    }
    let body = response.bytes().await?;
    if body.len() > 16 * 1024 * 1024 {
        return Err(ApiError(StatusCode::BAD_GATEWAY, "行情資料超出範圍"));
    }
    let values: Vec<Value> = serde_json::from_slice(&body)
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "官方行情格式已變更"))?;
    let rows = values
        .iter()
        .filter_map(|v| parse_row(source, v).transpose())
        .collect::<Result<Vec<_>>>()?;
    if rows.len() < 300 {
        return Err(ApiError(StatusCode::BAD_GATEWAY, "官方行情資料不完整"));
    }
    Ok(rows)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_dates() {
        assert_eq!(source_date("1150923").unwrap().to_string(), "2026-09-23");
        assert_eq!(source_date("115/09/23").unwrap().to_string(), "2026-09-23");
        assert!(source_date("bad").is_none());
    }
    #[test]
    fn suspended_prices_stay_unknown() {
        assert!(
            parse_row(
                "TWSE",
                &json!({"Code":"2330","Name":"測試","ClosingPrice":"--","Date":"1150923"})
            )
            .unwrap()
            .is_none()
        );
    }
}
