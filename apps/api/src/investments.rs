use crate::{
    auth::Identity,
    error::{ApiError, Result},
    finance::decimal,
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;
use uuid::Uuid;

fn zero() -> String {
    "0".into()
}
fn bounded(n: Option<Decimal>) -> Result<Decimal> {
    let n = n
        .ok_or_else(|| ApiError::bad("計算金額超出範圍"))?
        .round_dp(8);
    decimal(&n.to_string())
}
#[derive(Deserialize, Serialize, ToSchema)]
pub struct NewTrade {
    pub request_id: Uuid,
    pub account_id: Uuid,
    pub market: String,
    pub symbol: String,
    pub name: String,
    pub kind: String,
    pub quantity: String,
    pub unit_price: Option<String>,
    #[serde(default = "zero")]
    pub fee: String,
    #[serde(default = "zero")]
    pub tax: String,
    pub dividend_amount: Option<String>,
    pub occurred_on: NaiveDate,
}
#[derive(Debug, PartialEq)]
pub struct Change {
    pub quantity: Decimal,
    pub basis: Option<Decimal>,
    pub cash: Decimal,
    pub realized: Option<Decimal>,
}
pub fn calculate(
    old_q: Decimal,
    old_basis: Option<Decimal>,
    kind: &str,
    q: Decimal,
    price: Option<Decimal>,
    fee: Decimal,
    tax: Decimal,
    dividend: Option<Decimal>,
) -> Result<Change> {
    if q < Decimal::ZERO
        || fee < Decimal::ZERO
        || tax < Decimal::ZERO
        || price.is_some_and(|p| p < Decimal::ZERO)
    {
        return Err(ApiError::bad("數量、價格與費用須為非負數"));
    }
    let fees = bounded(fee.checked_add(tax))?;
    let mut c = Change {
        quantity: old_q,
        basis: old_basis,
        cash: Decimal::ZERO,
        realized: None,
    };
    match kind {
        "opening" => {
            if !old_q.is_zero() || q.is_zero() || !fees.is_zero() {
                return Err(ApiError::bad(
                    "期初持倉需在零持倉時建立，數量大於零且不含費用",
                ));
            }
            c.quantity = q;
            c.basis = price.map(|p| bounded(q.checked_mul(p))).transpose()?;
        }
        "buy" => {
            let p = price
                .filter(|p| *p > Decimal::ZERO)
                .ok_or_else(|| ApiError::bad("買入需填寫正數單價"))?;
            if q.is_zero() {
                return Err(ApiError::bad("數量需大於零"));
            }
            let cost = bounded(bounded(q.checked_mul(p))?.checked_add(fees))?;
            c.quantity = bounded(old_q.checked_add(q))?;
            c.basis = if old_q.is_zero() {
                Some(cost)
            } else {
                old_basis
                    .map(|b| bounded(b.checked_add(cost)))
                    .transpose()?
            };
            c.cash = -cost;
        }
        "sell" => {
            let p = price
                .filter(|p| *p > Decimal::ZERO)
                .ok_or_else(|| ApiError::bad("賣出需填寫正數單價"))?;
            if q.is_zero() || q > old_q {
                return Err(ApiError::bad("賣出數量需介於零與現有持倉之間"));
            }
            c.quantity = old_q - q;
            c.cash = bounded(bounded(q.checked_mul(p))?.checked_sub(fees))?;
            if let Some(b) = old_basis {
                let sold = if q == old_q {
                    b
                } else {
                    bounded(
                        b.checked_mul(
                            q.checked_div(old_q)
                                .ok_or_else(|| ApiError::bad("持倉成本錯誤"))?,
                        ),
                    )?
                };
                c.basis = Some(b - sold);
                c.realized = Some(c.cash - sold);
            } else if c.quantity.is_zero() {
                c.basis = Some(Decimal::ZERO);
            }
        }
        "dividend" => {
            if !q.is_zero() {
                return Err(ApiError::bad("現金股利的交易數量應為零"));
            }
            let gross = dividend
                .filter(|d| *d > fees)
                .ok_or_else(|| ApiError::bad("股利金額需大於稅費"))?;
            c.cash = gross - fees;
            c.realized = Some(c.cash);
        }
        "basis" => {
            if old_q.is_zero() || !q.is_zero() || !fees.is_zero() {
                return Err(ApiError::bad("補成本需有持倉，交易數量與稅費為零"));
            }
            let p = price.ok_or_else(|| ApiError::bad("請填每股平均成本"))?;
            c.basis = Some(bounded(old_q.checked_mul(p))?);
        }
        _ => return Err(ApiError::bad("投資交易類型不正確")),
    }
    Ok(c)
}
pub async fn positions(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',p.id,'account_id',p.account_id,'account_name',a.name,'currency',a.currency,'market',p.market,'symbol',p.symbol,'name',p.name,'quantity',p.quantity::text,'cost_basis',p.cost_basis::text,'average_cost',(p.cost_basis/nullif(p.quantity,0))::text,'last_price',p.last_price::text,'market_value',(p.quantity*p.last_price)::text,'unrealized_gain',(p.quantity*p.last_price-p.cost_basis)::text,'price_date',p.price_date,'price_source',p.price_source) FROM investment_positions p JOIN accounts a ON a.id=p.account_id WHERE p.owner_id=$1 ORDER BY a.name,p.symbol").bind(user.id).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
pub async fn trades(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',t.id,'position_id',p.id,'account_name',a.name,'currency',a.currency,'symbol',p.symbol,'name',p.name,'kind',t.kind,'quantity',t.quantity::text,'unit_price',t.unit_price::text,'fee',t.fee::text,'tax',t.tax::text,'cash_amount',t.cash_amount::text,'realized_gain',t.realized_gain::text,'occurred_on',t.occurred_on,'can_undo',NOT EXISTS(SELECT 1 FROM investment_trades n WHERE n.position_id=t.position_id AND (n.created_at,n.id)>(t.created_at,t.id))) FROM investment_trades t JOIN investment_positions p ON p.id=t.position_id JOIN accounts a ON a.id=p.account_id WHERE t.owner_id=$1 ORDER BY t.occurred_on DESC,t.created_at DESC,t.id DESC LIMIT 300").bind(user.id).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
#[utoipa::path(post,path="/api/v1/investments/trades",request_body=NewTrade,responses((status=200,description="Trade recorded atomically")))]
pub async fn create_trade(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<NewTrade>,
) -> Result<Json<Value>> {
    if !matches!(v.market.as_str(), "TWSE" | "TPEX" | "OTHER")
        || v.symbol.is_empty()
        || v.symbol.len() > 30
        || !v
            .symbol
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
        || v.name.trim().is_empty()
        || v.name.chars().count() > 120
    {
        return Err(ApiError::bad("市場、代號或名稱格式不正確"));
    }
    let today = (chrono::Utc::now() + chrono::Duration::hours(8)).date_naive();
    if v.occurred_on > today || v.occurred_on < NaiveDate::from_ymd_opt(1900, 1, 1).unwrap() {
        return Err(ApiError::bad("交易日期需介於 1900 年至今天"));
    }
    let fingerprint = crate::auth::hash(
        &serde_json::to_string(&v).map_err(|_| ApiError::bad("投資資料格式錯誤"))?,
    );
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let account: Option<(String, String)> =
        sqlx::query_as("SELECT currency,kind FROM accounts WHERE id=$1 AND owner_id=$2 FOR UPDATE")
            .bind(v.account_id)
            .bind(user.id)
            .fetch_optional(&mut *tx)
            .await?;
    let (currency, kind) = account.ok_or_else(ApiError::missing)?;
    if kind != "investment" || (v.market != "OTHER" && currency != "TWD") {
        return Err(ApiError::bad("請選擇投資帳戶；台股需使用 TWD 帳戶"));
    }
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT request_hash FROM investment_trades WHERE id=$1 AND owner_id=$2",
    )
    .bind(v.request_id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(hash) = existing {
        if hash != fingerprint {
            return Err(ApiError::bad("重試識別碼已使用，請重新開啟表單"));
        }
        return Ok(Json(json!({"id":v.request_id,"duplicate":true})));
    }
    let position:Uuid=sqlx::query_scalar("INSERT INTO investment_positions(id,owner_id,account_id,market,symbol,name,cost_basis) VALUES($1,$2,$3,$4,$5,$6,0) ON CONFLICT(account_id,market,symbol) DO UPDATE SET name=EXCLUDED.name RETURNING id").bind(Uuid::new_v4()).bind(user.id).bind(v.account_id).bind(&v.market).bind(v.symbol.to_uppercase()).bind(v.name.trim()).fetch_one(&mut *tx).await?;
    let(old_q,old_basis):(Decimal,Option<Decimal>)=sqlx::query_as("SELECT quantity,cost_basis FROM investment_positions WHERE id=$1 AND owner_id=$2 FOR UPDATE").bind(position).bind(user.id).fetch_one(&mut *tx).await?;
    let last: Option<NaiveDate> = sqlx::query_scalar(
        "SELECT max(occurred_on) FROM investment_trades WHERE position_id=$1 AND owner_id=$2",
    )
    .bind(position)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    if last.is_some_and(|d| d > v.occurred_on) {
        return Err(ApiError::bad(
            "請依日期順序記錄；較早的資料請先撤銷後續交易再補入",
        ));
    }
    let q = decimal(&v.quantity)?;
    let price = v.unit_price.as_deref().map(decimal).transpose()?;
    let fee = decimal(&v.fee)?;
    let tax = decimal(&v.tax)?;
    let dividend = v.dividend_amount.as_deref().map(decimal).transpose()?;
    let c = calculate(old_q, old_basis, &v.kind, q, price, fee, tax, dividend)?;
    sqlx::query("INSERT INTO investment_trades(id,owner_id,position_id,kind,quantity,unit_price,fee,tax,cash_amount,realized_gain,occurred_on,previous_quantity,previous_basis,request_hash) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)").bind(v.request_id).bind(user.id).bind(position).bind(&v.kind).bind(q).bind(price).bind(fee).bind(tax).bind(c.cash).bind(c.realized).bind(v.occurred_on).bind(old_q).bind(old_basis).bind(fingerprint).execute(&mut *tx).await?;
    if !c.cash.is_zero() {
        sqlx::query("INSERT INTO transactions(id,owner_id,account_id,amount,kind,occurred_on,description,category,investment_trade_id) VALUES($1,$2,$3,$4,$5,$6,$7,'投資收益',$8)").bind(Uuid::new_v4()).bind(user.id).bind(v.account_id).bind(c.cash).bind(if v.kind=="dividend"{"income"}else{"adjustment"}).bind(v.occurred_on).bind(format!("{} {}",v.symbol,match v.kind.as_str(){"buy"=>"買入","sell"=>"賣出",_=>"現金股利"})).bind(v.request_id).execute(&mut *tx).await?;
    }
    sqlx::query(
        "UPDATE investment_positions SET quantity=$1,cost_basis=$2 WHERE id=$3 AND owner_id=$4",
    )
    .bind(c.quantity)
    .bind(c.basis)
    .bind(position)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE investment_positions p SET last_price=m.price,price_date=m.quoted_on,price_source=m.market FROM market_quotes m WHERE p.id=$1 AND p.owner_id=$2 AND m.market=p.market AND m.symbol=p.symbol AND (p.price_date IS NULL OR m.quoted_on>=p.price_date)").bind(position).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":v.request_id,"position_id":position})))
}
#[derive(Deserialize, ToSchema)]
pub struct ManualQuote {
    pub price: String,
    pub date: NaiveDate,
}
#[utoipa::path(post,path="/api/v1/investments/positions/{id}/quote",request_body=ManualQuote,responses((status=204,description="Quote updated")))]
pub async fn quote(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<ManualQuote>,
) -> Result<StatusCode> {
    let price = decimal(&v.price)?;
    let today = (chrono::Utc::now() + chrono::Duration::hours(8)).date_naive();
    if price < Decimal::ZERO
        || v.date > today
        || v.date < NaiveDate::from_ymd_opt(1900, 1, 1).unwrap()
    {
        return Err(ApiError::bad("價格或日期格式錯誤"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let account:Option<Uuid>=sqlx::query_scalar("SELECT a.id FROM accounts a JOIN investment_positions p ON p.account_id=a.id WHERE p.id=$1 AND a.owner_id=$2 FOR UPDATE OF a").bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    if account.is_none() {
        return Err(ApiError::missing());
    }
    sqlx::query("UPDATE investment_positions SET last_price=$1,price_date=$2,price_source='manual' WHERE id=$3 AND owner_id=$4").bind(price).bind(v.date).bind(id).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn undo(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let account:Option<Uuid>=sqlx::query_scalar("SELECT a.id FROM accounts a JOIN investment_positions p ON p.account_id=a.id JOIN investment_trades t ON t.position_id=p.id WHERE t.id=$1 AND a.owner_id=$2 FOR UPDATE OF a").bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    if account.is_none() {
        return Err(ApiError::missing());
    }
    let(position,q,basis):(Uuid,Decimal,Option<Decimal>)=sqlx::query_as("SELECT position_id,previous_quantity,previous_basis FROM investment_trades WHERE id=$1 AND owner_id=$2").bind(id).bind(user.id).fetch_one(&mut *tx).await?;
    let latest:Uuid=sqlx::query_scalar("SELECT id FROM investment_trades WHERE position_id=$1 AND owner_id=$2 ORDER BY created_at DESC,id DESC LIMIT 1").bind(position).bind(user.id).fetch_one(&mut *tx).await?;
    if latest != id {
        return Err(ApiError::bad("請先撤銷這個持倉最新的一筆交易"));
    }
    sqlx::query("DELETE FROM investment_trades WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE investment_positions SET quantity=$1,cost_basis=$2 WHERE id=$3 AND owner_id=$4",
    )
    .bind(q)
    .bind(basis)
    .bind(position)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }
    #[test]
    fn average_cost_and_fees() {
        let c = calculate(
            d("100"),
            Some(d("1000")),
            "buy",
            d("100"),
            Some(d("12")),
            d("2"),
            d("0"),
            None,
        )
        .unwrap();
        assert_eq!(c.basis, Some(d("2202")));
        let sold = calculate(
            c.quantity,
            c.basis,
            "sell",
            d("50"),
            Some(d("15")),
            d("1"),
            d("2"),
            None,
        )
        .unwrap();
        assert_eq!(sold.basis, Some(d("1651.5")));
        assert_eq!(sold.realized, Some(d("196.5")));
    }
    #[test]
    fn unknown_cost_remains_unknown() {
        let c = calculate(
            d("100"),
            None,
            "sell",
            d("50"),
            Some(d("15")),
            d("0"),
            d("0"),
            None,
        )
        .unwrap();
        assert_eq!(c.basis, None);
        assert_eq!(c.realized, None);
    }
    #[test]
    fn prevent_oversell_and_overflow() {
        assert!(
            calculate(
                d("1"),
                None,
                "sell",
                d("2"),
                Some(d("10")),
                d("0"),
                d("0"),
                None
            )
            .is_err()
        );
        assert!(
            calculate(
                d("0"),
                None,
                "buy",
                d("9999999999999999"),
                Some(d("9999999999999999")),
                d("0"),
                d("0"),
                None
            )
            .is_err()
        );
    }
}
