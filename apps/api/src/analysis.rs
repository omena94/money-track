//! 收支分析：近 12 個月收支、分類×月份、當月明細、預算與電子發票彙總。
//!
//! 口徑與收支頁一致：不含待入帳、內部資金移動（轉帳、信用卡繳款、投資），只計 TWD。
//! 電子發票與刷卡是同一筆錢的不同視角，只另列商店與品項，不加進支出合計。
use crate::{
    auth::Identity,
    error::{ApiError, Result},
    finance::{FEED_SQL, NOT_FLOW},
    household::shift_months,
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Query, State},
};
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use serde_json::{Value, json};

const MONTHS: i32 = 12;
/// 單筆支出達此金額視為大額，總檢討中與日常支出分開看。
const LARGE: i64 = 50_000;

#[derive(Deserialize)]
pub struct AnalysisQuery {
    month: Option<String>,
}

/// GET /analysis?month=YYYY-MM
pub async fn analysis(
    user: Identity,
    State(s): State<AppState>,
    Query(q): Query<AnalysisQuery>,
) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let today: NaiveDate = sqlx::query_scalar("SELECT (now() AT TIME ZONE 'Asia/Taipei')::date")
        .fetch_one(&mut *tx)
        .await?;
    let month = match q.month.as_deref() {
        Some(m) => NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d")
            .map_err(|_| ApiError::bad("月份格式錯誤"))?,
        None => NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap(),
    };
    let start = shift_months(month, -(MONTHS - 1));
    let end = shift_months(month, 1);
    let key = month.format("%Y-%m").to_string();
    let base = format!("{FEED_SQL} SELECT * FROM feed WHERE status<>'pending' AND {NOT_FLOW} AND currency='TWD'");

    let months: Vec<Value> = sqlx::query_scalar(&format!("WITH f AS ({base} AND occurred_on>=$2 AND occurred_on<$3) SELECT jsonb_build_object('month',to_char(occurred_on,'YYYY-MM'),'income',coalesce(sum(amount) FILTER(WHERE kind='income'),0)::text,'expense',coalesce(-sum(amount) FILTER(WHERE kind IN ('expense','refund')),0)::text,'large',coalesce(-sum(amount) FILTER(WHERE kind='expense' AND amount<=-($4::numeric)),0)::text,'count',count(*) FILTER(WHERE kind IN ('expense','refund'))) FROM f GROUP BY to_char(occurred_on,'YYYY-MM') ORDER BY 1"))
        .bind(user.id).bind(start).bind(end).bind(LARGE).fetch_all(&mut *tx).await?;
    let matrix: Vec<Value> = sqlx::query_scalar(&format!("WITH f AS ({base} AND kind IN ('expense','refund') AND occurred_on>=$2 AND occurred_on<$3) SELECT jsonb_build_object('month',to_char(occurred_on,'YYYY-MM'),'category',category,'amount',(-sum(amount))::text,'count',count(*)) FROM f GROUP BY to_char(occurred_on,'YYYY-MM'),category"))
        .bind(user.id).bind(start).bind(end).fetch_all(&mut *tx).await?;
    let entries: Vec<Value> = sqlx::query_scalar(&format!("WITH f AS ({base} AND occurred_on>=$2 AND occurred_on<$3) SELECT jsonb_build_object('id',id,'date',occurred_on,'kind',kind,'amount',amount::text,'description',description,'category',category,'account_name',account_name) FROM f ORDER BY occurred_on,amount"))
        .bind(user.id).bind(month).bind(end).fetch_all(&mut *tx).await?;
    let budgets: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('category',category,'amount',amount::text) FROM budgets WHERE owner_id=$1 AND month=$2 AND currency='TWD'")
        .bind(user.id).bind(&key).fetch_all(&mut *tx).await?;

    // 電子發票：當月有效發票（作廢不計）的商店與品項
    let invoice_sql = "SELECT source_key,data FROM source_records WHERE owner_id=$1 AND kind='invoice' AND current AND coalesce(data->>'status','active')<>'void' AND data->>'currency'='TWD' AND left(data->>'date',10)>=$2 AND left(data->>'date',10)<$3";
    let (from, to) = (month.to_string(), end.to_string());
    let invoice_total: Value = sqlx::query_scalar(&format!("WITH i AS ({invoice_sql}) SELECT jsonb_build_object('count',count(*),'amount',coalesce(sum((data->>'amount')::numeric),0)::text) FROM i"))
        .bind(user.id).bind(&from).bind(&to).fetch_one(&mut *tx).await?;
    let sellers: Vec<Value> = sqlx::query_scalar(&format!("WITH i AS ({invoice_sql}) SELECT jsonb_build_object('name',name,'amount',amount::text,'count',n) FROM (SELECT coalesce(nullif(data->>'seller',''),'未知商店') name,sum((data->>'amount')::numeric) amount,count(*) n FROM i GROUP BY 1 ORDER BY 2 DESC LIMIT 10) x ORDER BY amount DESC"))
        .bind(user.id).bind(&from).bind(&to).fetch_all(&mut *tx).await?;
    let items: Vec<Value> = sqlx::query_scalar(&format!("WITH i AS ({invoice_sql}) SELECT jsonb_build_object('name',t.data->>'description','amount',sum((t.data->>'amount')::numeric)::text,'quantity',coalesce(sum((t.data->>'quantity')::numeric),0)::text,'count',count(*)) FROM source_records t JOIN i ON i.source_key=t.data->>'invoice_key' WHERE t.owner_id=$1 AND t.kind='invoice_item' AND t.current AND coalesce(t.data->>'description','')<>'' GROUP BY t.data->>'description' ORDER BY sum((t.data->>'amount')::numeric) DESC LIMIT 12"))
        .bind(user.id).bind(&from).bind(&to).fetch_all(&mut *tx).await?;
    tx.commit().await?;

    Ok(Json(json!({
        "month": key,
        "today": today,
        "large_threshold": LARGE,
        "months": months,
        "matrix": matrix,
        "entries": entries,
        "budgets": budgets,
        "invoices": {
            "count": invoice_total["count"],
            "amount": invoice_total["amount"],
            "sellers": sellers,
            "items": items,
        },
    })))
}
