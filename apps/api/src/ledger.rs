use crate::{
    auth::Identity,
    error::{ApiError, Result},
    finance::decimal,
    state::{AppState, tenant},
};
#[derive(Deserialize, utoipa::ToSchema)]
pub struct Refund {
    request_id: Uuid,
    amount: String,
    occurred_on: chrono::NaiveDate,
}
#[utoipa::path(post,path="/api/v1/transactions/{id}/refund",request_body=Refund,responses((status=200,description="Refund recorded")))]
pub async fn refund(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Refund>,
) -> Result<Json<Value>> {
    let amount = decimal(&v.amount)?;
    if amount <= Decimal::ZERO {
        return Err(ApiError::bad("退款金額需大於零"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let source:Option<(Decimal,chrono::NaiveDate)>=sqlx::query_as("SELECT amount,occurred_on FROM transactions WHERE id=$1 AND owner_id=$2 AND kind='expense' AND NOT is_split_parent AND investment_trade_id IS NULL FOR UPDATE").bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    let (original, date) = source.ok_or_else(ApiError::missing)?;
    let duplicate:Option<(Uuid,Decimal,chrono::NaiveDate)>=sqlx::query_as("SELECT refund_of,amount,occurred_on FROM transactions WHERE id=$1 AND owner_id=$2 AND kind='refund'").bind(v.request_id).bind(user.id).fetch_optional(&mut *tx).await?;
    if let Some((parent, n, d)) = duplicate {
        if parent == id && n == amount && d == v.occurred_on {
            return Ok(Json(json!({"id":v.request_id,"duplicate":true})));
        }
        return Err(ApiError::bad("退款識別碼已使用，請重新開啟表單"));
    }
    let today = (chrono::Utc::now() + chrono::Duration::hours(8)).date_naive();
    if v.occurred_on < date || v.occurred_on > today {
        return Err(ApiError::bad("退款日期需介於原交易與今天之間"));
    }
    let refunded: Decimal = sqlx::query_scalar(
        "SELECT coalesce(sum(amount),0) FROM transactions WHERE refund_of=$1 AND owner_id=$2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    if amount > -original - refunded {
        return Err(ApiError::bad("退款合計超出原始支出"));
    }
    sqlx::query("INSERT INTO transactions(id,owner_id,account_id,amount,kind,occurred_on,description,category,tags,refund_of) SELECT $1,owner_id,account_id,$2,'refund',$3,left('退款 · '||description,500),category,tags,id FROM transactions WHERE id=$4 AND owner_id=$5").bind(v.request_id).bind(amount).bind(v.occurred_on).bind(id).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":v.request_id})))
}
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
#[derive(Deserialize)]
pub struct Edit {
    category: String,
    note: String,
    tags: Vec<String>,
}
pub async fn edit(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Edit>,
) -> Result<StatusCode> {
    if v.category.trim().is_empty()
        || v.category.len() > 240
        || v.note.chars().count() > 2000
        || v.tags.len() > 20
        || v.tags.iter().any(|t| t.len() > 80)
    {
        return Err(ApiError::bad("分類、標籤或備註格式錯誤"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r=sqlx::query("UPDATE transactions SET category=$1,note=$2,tags=$3 WHERE id=$4 AND owner_id=$5 AND NOT is_split_parent").bind(v.category).bind(v.note).bind(v.tags).bind(id).bind(user.id).execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
pub struct Split {
    parts: Vec<Part>,
}
#[derive(Deserialize)]
pub struct Part {
    amount: String,
    category: String,
    description: String,
}
pub async fn split(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Split>,
) -> Result<Json<Value>> {
    if !(2..=30).contains(&v.parts.len()) {
        return Err(ApiError::bad("拆帳需為 2 到 30 筆"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let original:Option<(Decimal,String)>=sqlx::query_as("SELECT amount,kind FROM transactions WHERE id=$1 AND owner_id=$2 AND NOT is_split_parent AND split_parent_id IS NULL AND investment_trade_id IS NULL FOR UPDATE").bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    let (amount, kind) = original.ok_or_else(ApiError::missing)?;
    if !matches!(kind.as_str(), "income" | "expense") {
        return Err(ApiError::bad("只有一般收支支援拆帳"));
    }
    let refunded: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM transactions WHERE refund_of=$1 AND owner_id=$2)",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    if refunded {
        return Err(ApiError::bad("這筆支出已有退款，請先移除退款再調整拆帳"));
    }
    let mut parts = Vec::new();
    let mut total = Decimal::ZERO;
    for p in v.parts {
        let n = decimal(&p.amount)?;
        if n.is_zero()
            || n.is_sign_negative() != amount.is_sign_negative()
            || p.category.trim().is_empty()
            || p.category.len() > 240
            || p.description.trim().is_empty()
            || p.description.chars().count() > 500
        {
            return Err(ApiError::bad("每筆金額方向須相同，分類與說明必填"));
        }
        total += n;
        parts.push((p, n));
    }
    if total != amount {
        return Err(ApiError::bad("拆帳合計需等於原始金額"));
    }
    sqlx::query("UPDATE transactions SET is_split_parent=true WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    for (p, n) in parts {
        sqlx::query("INSERT INTO transactions(id,owner_id,account_id,amount,kind,occurred_on,description,category,tags,note,split_parent_id) SELECT $1,owner_id,account_id,$2,kind,occurred_on,$3,$4,tags,note,id FROM transactions WHERE id=$5 AND owner_id=$6").bind(Uuid::new_v4()).bind(n).bind(p.description).bind(p.category).bind(id).bind(user.id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
