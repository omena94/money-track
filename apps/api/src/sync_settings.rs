//! 同步設定：所有來源共用的每日同步時間，以及登入錯誤保護。
//!
//! - `times`：台北時間 HH:MM，一天 1～6 個；每個來源的 `next_run_at` 一律對齊下一個時間點。
//! - `attempt_limit`：單次同步最多嘗試登入幾次，超過就停止這次同步。
//! - `failure_limit`：連續登入未成功幾次就停止自動同步（也擋手動），直到使用者更新登入設定或按「已修好」。
//!   登入成功或同步完成時，連續失敗次數歸零。
use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub const MAX_ATTEMPTS: i16 = 3;
pub const MAX_FAILURES: i16 = 5;

#[derive(Clone, Debug)]
pub struct SyncSettings {
    pub times: Vec<String>,
    pub attempt_limit: i16,
    pub failure_limit: i16,
}
impl Default for SyncSettings {
    fn default() -> Self {
        Self { times: vec!["07:00".into()], attempt_limit: 1, failure_limit: 2 }
    }
}

fn taipei() -> chrono::FixedOffset {
    chrono::FixedOffset::east_opt(8 * 3600).unwrap()
}

/// 下一個同步時間點（嚴格晚於 `now`）。
pub fn next_slot(times: &[String], now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now.with_timezone(&taipei());
    let mut best: Option<DateTime<Utc>> = None;
    for t in times {
        let Ok(time) = NaiveTime::parse_from_str(t, "%H:%M") else { continue };
        for day in 0..2 {
            let date = local.date_naive() + Duration::days(day);
            let at = taipei()
                .from_local_datetime(&date.and_time(time))
                .single()
                .map(|d| d.with_timezone(&Utc));
            if let Some(at) = at.filter(|a| *a > now) {
                if best.is_none_or(|b| at < b) {
                    best = Some(at);
                }
                break;
            }
        }
    }
    best.unwrap_or(now + Duration::days(1))
}

/// 整理並驗證時間：去重、排序，1～6 個 HH:MM。
fn normalize_times(input: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for t in input {
        let parsed = NaiveTime::parse_from_str(t.trim(), "%H:%M")
            .map_err(|_| ApiError::bad("時間格式需為 HH:MM"))?;
        let s = parsed.format("%H:%M").to_string();
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out.sort();
    if out.is_empty() || out.len() > 6 {
        return Err(ApiError::bad("每天需設定 1 到 6 個同步時間"));
    }
    Ok(out)
}

pub async fn load(tx: &mut sqlx::PgConnection, owner: Uuid) -> Result<SyncSettings> {
    let row: Option<(Vec<String>, i16, i16)> = sqlx::query_as(
        "SELECT times,attempt_limit,failure_limit FROM sync_settings WHERE owner_id=$1",
    )
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await?;
    Ok(row
        .map(|(times, attempt_limit, failure_limit)| SyncSettings { times, attempt_limit, failure_limit })
        .unwrap_or_default())
}

/// 使用者下一個同步時間點。
pub async fn next_run(tx: &mut sqlx::PgConnection, owner: Uuid) -> Result<DateTime<Utc>> {
    let s = load(tx, owner).await?;
    Ok(next_slot(&s.times, Utc::now()))
}

fn to_json(s: &SyncSettings) -> Value {
    json!({
        "times": s.times,
        "attempt_limit": s.attempt_limit,
        "failure_limit": s.failure_limit,
        "next_run_at": next_slot(&s.times, Utc::now()),
        "max_attempts": MAX_ATTEMPTS,
        "max_failures": MAX_FAILURES,
    })
}

/// GET /sync-settings
pub async fn get(user: Identity, State(s): State<AppState>) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let v = load(&mut tx, user.id).await?;
    tx.commit().await?;
    Ok(Json(to_json(&v)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    times: Vec<String>,
    attempt_limit: i16,
    failure_limit: i16,
}
/// PUT /sync-settings：儲存後所有自動同步中的來源對齊到下一個時間點。
pub async fn put(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<Update>,
) -> Result<Json<Value>> {
    let times = normalize_times(&v.times)?;
    if !(1..=MAX_ATTEMPTS).contains(&v.attempt_limit) {
        return Err(ApiError::bad("單次同步登入次數需為 1 到 3 次"));
    }
    if !(1..=MAX_FAILURES).contains(&v.failure_limit) {
        return Err(ApiError::bad("連續失敗上限需為 1 到 5 次"));
    }
    let settings = SyncSettings { times, attempt_limit: v.attempt_limit, failure_limit: v.failure_limit };
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    sqlx::query("INSERT INTO sync_settings(owner_id,times,attempt_limit,failure_limit) VALUES($1,$2,$3,$4) ON CONFLICT(owner_id) DO UPDATE SET times=EXCLUDED.times,attempt_limit=EXCLUDED.attempt_limit,failure_limit=EXCLUDED.failure_limit,updated_at=now()")
        .bind(user.id).bind(&settings.times).bind(settings.attempt_limit).bind(settings.failure_limit)
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE connections SET next_run_at=$2,updated_at=now() WHERE owner_id=$1 AND enabled AND daily AND NOT manual_only AND connector<>'hsbc'")
        .bind(user.id).bind(next_slot(&settings.times, Utc::now()))
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(to_json(&settings)))
}

/// POST /connections/{id}/resume：使用者確認已修好（例如銀行端解鎖），連續失敗歸零並恢復排程。
pub async fn resume(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let next = next_run(&mut tx, user.id).await?;
    let r = sqlx::query("UPDATE connections SET login_failures=0,next_run_at=CASE WHEN enabled AND daily AND NOT manual_only AND connector<>'hsbc' THEN $3 ELSE next_run_at END,updated_at=now() WHERE id=$1 AND owner_id=$2 AND connector=ANY($4)")
        .bind(id).bind(user.id).bind(next).bind(&crate::sources::IDS[..])
        .execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(Json(json!({ "login_failures": 0 })))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }
    #[test]
    fn next_slot_picks_the_nearest_future_time_in_taipei() {
        let times = vec!["07:00".to_string(), "21:30".to_string()];
        // 台北 06:00 → 當天 07:00
        assert_eq!(next_slot(&times, at("2026-09-25T06:00:00+08:00")), at("2026-09-25T07:00:00+08:00"));
        // 台北 12:00 → 當天 21:30
        assert_eq!(next_slot(&times, at("2026-09-25T12:00:00+08:00")), at("2026-09-25T21:30:00+08:00"));
        // 台北 22:00 → 隔天 07:00（跨月）
        assert_eq!(next_slot(&times, at("2026-09-30T22:00:00+08:00")), at("2026-10-01T07:00:00+08:00"));
        // 剛好在時間點上 → 下一個
        assert_eq!(next_slot(&times, at("2026-09-25T07:00:00+08:00")), at("2026-09-25T21:30:00+08:00"));
    }
    #[test]
    fn times_are_validated_sorted_and_deduplicated() {
        let v = |x: &[&str]| normalize_times(&x.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(v(&["21:00", "7:05", "07:05"]).unwrap(), vec!["07:05", "21:00"]);
        assert!(v(&[]).is_err());
        assert!(v(&["25:00"]).is_err());
        assert!(v(&["01:00", "02:00", "03:00", "04:00", "05:00", "06:00", "07:00"]).is_err());
    }
}
