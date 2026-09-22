use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Query, State},
};
use chrono::{Months, NaiveDate};
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarQuery {
    pub month: String,
}
fn bounds(month: &str) -> Result<(String, String)> {
    if month.len() != 7 {
        return Err(ApiError::bad("月份格式須為 YYYY-MM"));
    }
    let start = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| ApiError::bad("月份格式須為 YYYY-MM"))?;
    let end = start
        .checked_add_months(Months::new(1))
        .ok_or_else(|| ApiError::bad("月份超出範圍"))?;
    if start.format("%Y-%m").to_string() != month {
        return Err(ApiError::bad("月份格式須為 YYYY-MM"));
    }
    Ok((start.to_string(), end.to_string()))
}
#[utoipa::path(get,path="/api/v1/calendar",params(("month"=String,Query,description="YYYY-MM，Asia/Taipei")),responses((status=200,body=Value)),tag="calendar")]
pub async fn events(
    user: Identity,
    State(s): State<AppState>,
    Query(q): Query<CalendarQuery>,
) -> Result<Json<Value>> {
    let (start, end) = bounds(&q.month)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',r.id,'connection_id',r.connection_id,'connector',c.connector,'account_name',coalesce(a.data->>'name',c.connector),'period',r.data->>'period','date',left(r.data->>'payment_due',10),'currency',r.data->>'currency','statement_amount',r.data->>'statement_amount','minimum_payment',r.data->>'minimum_payment','paid_amount',r.data->>'paid_amount','is_paid',r.data->'is_paid','last_synced_at',c.last_success_at) FROM source_records r JOIN connections c ON c.id=r.connection_id AND c.owner_id=r.owner_id LEFT JOIN source_records a ON a.connection_id=r.connection_id AND a.owner_id=r.owner_id AND a.kind='account' AND a.source_key=r.account_key WHERE r.owner_id=$1 AND r.kind='bill' AND r.current AND (a.id IS NULL OR a.current) AND left(r.data->>'payment_due',10)>=$2 AND left(r.data->>'payment_due',10)<$3 ORDER BY r.data->>'payment_due',c.connector,r.source_key")
        .bind(user.id).bind(start).bind(end).fetch_all(&mut *tx).await?;
    let unknown:i64=sqlx::query_scalar("SELECT count(*) FROM source_records r WHERE r.owner_id=$1 AND r.kind='bill' AND r.current AND (r.data->>'payment_due' IS NULL OR r.data->>'payment_due'='') AND NOT EXISTS(SELECT 1 FROM source_records a WHERE a.owner_id=r.owner_id AND a.connection_id=r.connection_id AND a.kind='account' AND a.source_key=r.account_key AND NOT a.current)").bind(user.id).fetch_one(&mut *tx).await?;
    Ok(Json(
        json!({"month":q.month,"timezone":"Asia/Taipei","events":rows,"unknown_due_count":unknown}),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn month_boundaries() {
        assert_eq!(
            bounds("2026-12").unwrap(),
            ("2026-12-01".into(), "2027-01-01".into())
        );
        assert_eq!(bounds("2024-02").unwrap().1, "2024-03-01");
        for s in ["2026-1", "2026-00", "2026-13", "2026-09-24", ""] {
            assert!(bounds(s).is_err());
        }
    }
}
