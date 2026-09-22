//! 自動分類：同步交易的逐筆分類，以及使用者自訂的關鍵字規則。
//!
//! 同步交易寫入時由資料庫觸發器依「使用者規則 → 內建關鍵字」先分類（見 0015_auto_category.sql）；
//! 使用者逐筆修改會存在 `source_tx_labels`，優先於自動分類。
use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{Months, NaiveDate};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

fn check_category(c: &str) -> Result<String> {
    let c = c.trim();
    if c.is_empty() || c.chars().count() > 60 {
        return Err(ApiError::bad("分類名稱需為 1 到 60 個字"));
    }
    Ok(c.to_owned())
}

/// 新增或更新規則，回傳正規化後的關鍵字。
async fn upsert_rule(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    pattern: &str,
    category: &str,
) -> Result<String> {
    let normalized: String = sqlx::query_scalar("SELECT category_text($1)")
        .bind(pattern)
        .fetch_one(&mut **tx)
        .await?;
    let n = normalized.chars().count();
    if !(2..=80).contains(&n) {
        return Err(ApiError::bad("關鍵字需為 2 到 80 個字"));
    }
    sqlx::query("INSERT INTO category_rules(id,owner_id,pattern,category) VALUES($1,$2,$3,$4) ON CONFLICT(owner_id,pattern) DO UPDATE SET category=EXCLUDED.category,created_at=now()")
        .bind(Uuid::new_v4())
        .bind(owner)
        .bind(&normalized)
        .bind(category)
        .execute(&mut **tx)
        .await?;
    Ok(normalized)
}

#[derive(Deserialize)]
pub struct Label {
    category: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    tags: Vec<String>,
    /// 同時建立規則：之後含這個關鍵字的同步交易都用這個分類。
    rule: Option<String>,
}
/// PATCH /source-transactions/{id}：修改同步交易的分類、標籤與備註（金額與日期仍以來源為準）。
pub async fn label(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Label>,
) -> Result<Json<Value>> {
    let category = check_category(&v.category)?;
    if v.note.chars().count() > 2000
        || v.tags.len() > 20
        || v.tags
            .iter()
            .any(|t| t.trim().is_empty() || t.chars().count() > 80)
    {
        return Err(ApiError::bad("標籤或備註格式錯誤"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM source_records WHERE id=$1 AND owner_id=$2 AND kind='transaction')",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(ApiError::missing());
    }
    let tags: Vec<String> = v.tags.iter().map(|t| t.trim().to_owned()).collect();
    sqlx::query("INSERT INTO source_tx_labels(record_id,owner_id,category,note,tags) VALUES($1,$2,$3,$4,$5) ON CONFLICT(record_id) DO UPDATE SET category=EXCLUDED.category,note=EXCLUDED.note,tags=EXCLUDED.tags,updated_at=now()")
        .bind(id)
        .bind(user.id)
        .bind(&category)
        .bind(&v.note)
        .bind(&tags)
        .execute(&mut *tx)
        .await?;
    if let Some(p) = v.rule.as_deref().filter(|p| !p.trim().is_empty()) {
        upsert_rule(&mut tx, user.id, p, &category).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "category": category })))
}

/// DELETE /source-transactions/{id}/label：還原為自動分類。
pub async fn unlabel(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    sqlx::query("DELETE FROM source_tx_labels WHERE record_id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE source_records SET auto_source=NULL WHERE id=$1 AND owner_id=$2 AND kind='transaction'")
        .bind(id).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /category-rules：我的分類規則與目前符合的同步交易筆數。
pub async fn rules(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows = sqlx::query_scalar("SELECT jsonb_build_object('id',r.id,'pattern',r.pattern,'category',r.category,'created_at',r.created_at,'matches',(SELECT count(*) FROM source_records x WHERE x.owner_id=r.owner_id AND x.kind='transaction' AND x.current AND x.auto_source='rule' AND x.auto_category=r.category AND strpos(category_text(concat_ws(' ',x.data->>'description',x.data->>'memo')),r.pattern)>0)) FROM category_rules r WHERE r.owner_id=$1 ORDER BY r.created_at DESC")
        .bind(user.id)
        .fetch_all(&mut *tx)
        .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct NewRule {
    pattern: String,
    category: String,
}
/// POST /category-rules：儲存規則；既有交易由使用者指定範圍重新分類。
pub async fn create_rule(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<NewRule>,
) -> Result<(StatusCode, Json<Value>)> {
    let category = check_category(&v.category)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let pattern = upsert_rule(&mut tx, user.id, &v.pattern, &category).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "pattern": pattern }))))
}

/// DELETE /category-rules/{id}：刪除規則；既有交易等待指定範圍重新分類。
pub async fn delete_rule(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r = sqlx::query("DELETE FROM category_rules WHERE id=$1 AND owner_id=$2")
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

/// 每個使用者自動啟用的共用預設規則，直接讀取分類引擎的資料。
pub async fn defaults(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows = sqlx::query_scalar("SELECT jsonb_build_object('pattern',pattern,'category',category,'kind',kind,'priority',priority) FROM category_keywords ORDER BY category,priority DESC,length(pattern) DESC,pattern")
        .fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reclassify {
    All {},
    Dates { start: NaiveDate, end: NaiveDate },
    Months { start: String, end: String },
}

impl Reclassify {
    fn bounds(&self) -> Result<(Option<String>, Option<String>)> {
        let (start, end) = match self {
            Self::All {} => return Ok((None, None)),
            Self::Dates { start, end } => (*start, *end),
            Self::Months { start, end } => {
                let parse = |s: &str| -> Result<NaiveDate> {
                    if s.len() != 7 {
                        return Err(ApiError::bad("月份格式需為 YYYY-MM"));
                    }
                    NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d")
                        .map_err(|_| ApiError::bad("月份格式需為 YYYY-MM"))
                };
                let last = parse(end)?
                    .checked_add_months(Months::new(1))
                    .and_then(|d| d.pred_opt())
                    .ok_or_else(|| ApiError::bad("月份超出範圍"))?;
                (parse(start)?, last)
            }
        };
        if start > end {
            return Err(ApiError::bad("開始日期需早於或等於結束日期"));
        }
        if start.format("%Y").to_string().len() != 4 || end.format("%Y").to_string().len() != 4 {
            return Err(ApiError::bad("年份需為四位數"));
        }
        Ok((Some(start.to_string()), Some(end.to_string())))
    }
}

/// 只更新本人現行同步交易；單筆手動分類與標籤、備註保持原值。
pub async fn reclassify(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<Reclassify>,
) -> Result<Json<Value>> {
    let (start, end) = v.bounds()?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let result: Value = sqlx::query_scalar(r#"
        WITH candidates AS MATERIALIZED (
            SELECT r.id, EXISTS(SELECT 1 FROM source_tx_labels l WHERE l.record_id=r.id AND l.owner_id=$1) AS manual
            FROM source_records r
            WHERE r.owner_id=$1 AND r.kind='transaction' AND r.current
              AND ($2::text IS NULL OR left(r.data->>'date',10)>=$2)
              AND ($3::text IS NULL OR left(r.data->>'date',10)<=$3)
        ), updated AS (
            UPDATE source_records r SET auto_source=NULL
            FROM candidates c WHERE r.id=c.id AND r.owner_id=$1 AND NOT c.manual
            RETURNING r.id
        )
        SELECT jsonb_build_object('processed',(SELECT count(*) FROM updated),
            'protected',(SELECT count(*) FROM candidates WHERE manual))
    "#).bind(user.id).bind(start).bind(end).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reclassification_ranges() {
        let range = |v| serde_json::from_value::<Reclassify>(v).unwrap().bounds();
        assert_eq!(range(json!({"scope":"all"})).unwrap(), (None, None));
        assert_eq!(
            range(json!({"scope":"months","start":"2024-02","end":"2024-02"})).unwrap(),
            (Some("2024-02-01".into()), Some("2024-02-29".into()))
        );
        assert_eq!(
            range(json!({"scope":"months","start":"2025-12","end":"2026-01"})).unwrap(),
            (Some("2025-12-01".into()), Some("2026-01-31".into()))
        );
        assert!(range(json!({"scope":"months","start":"2026-09","end":"2026-08"})).is_err());
        assert!(range(json!({"scope":"months","start":"2026-13","end":"2026-13"})).is_err());
        assert!(range(json!({"scope":"dates","start":"2026-09-21","end":"2026-09-20"})).is_err());
        assert!(
            serde_json::from_value::<Reclassify>(json!({"scope":"all","start":"2026-09-20"}))
                .is_err()
        );
    }
}
