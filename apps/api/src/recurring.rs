use crate::{
    auth::Identity,
    error::{ApiError, Result},
    finance::decimal,
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
#[derive(Deserialize)]
pub struct NewRecurring {
    account_id: Uuid,
    amount: String,
    kind: String,
    description: String,
    category: String,
    frequency: String,
    next_on: NaiveDate,
}
pub async fn list(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',r.id,'account_id',r.account_id,'account_name',a.name,'currency',a.currency,'amount',r.amount::text,'kind',r.kind,'description',r.description,'category',r.category,'frequency',r.frequency,'next_on',r.next_on,'enabled',r.enabled) FROM recurring_entries r JOIN accounts a ON a.id=r.account_id WHERE r.owner_id=$1 ORDER BY r.next_on,r.id").bind(user.id).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
pub async fn create(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<NewRecurring>,
) -> Result<Json<Value>> {
    let n = decimal(&v.amount)?;
    if n.is_zero()
        || !matches!(v.kind.as_str(), "income" | "expense")
        || (v.kind == "expense") != n.is_sign_negative()
        || !matches!(
            v.frequency.as_str(),
            "daily" | "weekly" | "monthly" | "yearly"
        )
        || v.description.trim().is_empty()
        || v.description.chars().count() > 500
        || v.category.trim().is_empty()
        || v.category.len() > 240
    {
        return Err(ApiError::bad("週期帳務內容不正確"));
    }
    let today = (chrono::Utc::now() + Duration::hours(8)).date_naive();
    if v.next_on < today || v.next_on > today + Duration::days(3660) {
        return Err(ApiError::bad("首次日期需在今天至未來十年內"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let owner: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=$1 AND owner_id=$2)")
            .bind(v.account_id)
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    if !owner {
        return Err(ApiError::missing());
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO recurring_entries(id,owner_id,account_id,amount,kind,description,category,frequency,next_on,anchor_day) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)").bind(id).bind(user.id).bind(v.account_id).bind(n).bind(v.kind).bind(v.description).bind(v.category).bind(v.frequency).bind(v.next_on).bind(v.next_on.day()as i32).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
#[derive(Deserialize)]
pub struct Toggle {
    enabled: bool,
}
pub async fn toggle(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(t): Json<Toggle>,
) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r = sqlx::query("UPDATE recurring_entries SET enabled=$1 WHERE id=$2 AND owner_id=$3")
        .bind(t.enabled)
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub fn next_date(date: NaiveDate, frequency: &str, anchor: u32) -> NaiveDate {
    match frequency {
        "daily" => date + Duration::days(1),
        "weekly" => date + Duration::days(7),
        _ => {
            let month = date
                .with_day(1)
                .unwrap()
                .checked_add_months(Months::new(if frequency == "yearly" { 12 } else { 1 }))
                .unwrap();
            let end = month.checked_add_months(Months::new(1)).unwrap() - Duration::days(1);
            month.with_day(anchor.min(end.day())).unwrap()
        }
    }
}
pub async fn run_due(s: &AppState) -> Result<()> {
    let users: Vec<(Uuid, String)> = sqlx::query_as("SELECT id,email FROM users")
        .fetch_all(&s.auth_db)
        .await?;
    for (id, email) in users {
        run_due_for(s, id, &email).await?;
    }
    Ok(())
}
pub async fn run_due_for(s: &AppState, id: Uuid, email: &str) -> Result<()> {
    let today = (chrono::Utc::now() + Duration::hours(8)).date_naive();
    let mut tx = tenant(&s.db, id, email).await?;
    let due:Vec<(Uuid,NaiveDate,String,i32)>=sqlx::query_as("SELECT id,next_on,frequency,anchor_day FROM recurring_entries WHERE owner_id=$1 AND enabled AND next_on<=$2 ORDER BY next_on LIMIT 100 FOR UPDATE SKIP LOCKED").bind(id).bind(today).fetch_all(&mut *tx).await?;
    for (rule, date, frequency, anchor) in due {
        let key = format!("recurring:{rule}:{date}");
        let entry = Uuid::new_v4();
        let inserted=sqlx::query("INSERT INTO transactions(id,owner_id,account_id,amount,kind,occurred_on,description,category,import_key) SELECT $1,owner_id,account_id,amount,kind,$2,description,category,$3 FROM recurring_entries WHERE id=$4 AND owner_id=$5 ON CONFLICT(owner_id,import_key) DO NOTHING").bind(entry).bind(date).bind(key).bind(rule).bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE recurring_entries SET next_on=$1 WHERE id=$2 AND owner_id=$3")
            .bind(next_date(date, &frequency, anchor as u32))
            .bind(rule)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if inserted.rows_affected() > 0 {
            sqlx::query("INSERT INTO notifications(id,owner_id,title,body) SELECT $1,owner_id,'週期帳務已記錄',description FROM recurring_entries WHERE id=$2 AND owner_id=$3").bind(Uuid::new_v4()).bind(rule).bind(id).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}
pub async fn notifications(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'title',title,'body',body,'created_at',created_at,'read',read_at IS NOT NULL) FROM notifications WHERE owner_id=$1 ORDER BY created_at DESC LIMIT 100").bind(user.id).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn month_end_does_not_drift() {
        let jan = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let feb = next_date(jan, "monthly", 31);
        assert_eq!(feb.to_string(), "2026-02-28");
        assert_eq!(next_date(feb, "monthly", 31).to_string(), "2026-03-31");
    }
    #[test]
    fn leap_year_anchor_is_preserved() {
        let leap = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        assert_eq!(next_date(leap, "yearly", 29).to_string(), "2025-02-28");
    }
}
