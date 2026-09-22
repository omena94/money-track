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
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
pub async fn list(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'account_id',account_id,'recipient_email',recipient_email,'permission',permission,'accepted',accepted_at IS NOT NULL,'direction',CASE WHEN owner_id=$1 THEN 'outgoing' ELSE 'incoming' END) FROM shares ORDER BY created_at DESC").bind(user.id).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
#[derive(Deserialize)]
pub struct Invitation {
    account_id: Uuid,
    email: String,
    permission: String,
}
pub async fn invite(
    user: Identity,
    State(s): State<AppState>,
    Json(i): Json<Invitation>,
) -> Result<Json<Value>> {
    let email = i.email.trim().to_lowercase();
    if email == user.email
        || !email.contains('@')
        || email.len() > 254
        || !matches!(i.permission.as_str(), "balance" | "details")
    {
        return Err(ApiError::bad("請確認收件人與分享權限"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let owned: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM accounts WHERE id=$1 AND owner_id=$2)")
            .bind(i.account_id)
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    if !owned {
        return Err(ApiError::missing());
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO shares(id,owner_id,account_id,recipient_email,permission) VALUES($1,$2,$3,$4,$5)").bind(id).bind(user.id).bind(i.account_id).bind(email).bind(i.permission).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
pub async fn accept(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    // auth role has column-only UPDATE privilege and a recipient-only RLS policy.
    let mut tx = tenant(&s.auth_db, user.id, &user.email).await?;
    let r=sqlx::query("UPDATE shares SET accepted_by=$1,accepted_at=now() WHERE id=$2 AND recipient_email=$3 AND accepted_at IS NULL").bind(user.id).bind(id).bind(&user.email).execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn revoke(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r = sqlx::query("DELETE FROM shares WHERE id=$1 AND owner_id=$2")
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
pub async fn shared_accounts(
    user: Identity,
    State(s): State<AppState>,
) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'kind',kind,'currency',currency,'balance',balance::text,'institution',institution) FROM account_summaries WHERE owner_id<>$1 ORDER BY name").bind(user.id).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
pub async fn shared_transactions(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::finance::Entry>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM shares WHERE account_id=$1 AND accepted_by=$2 AND recipient_email=$3 AND permission='details')").bind(id).bind(user.id).bind(&user.email).fetch_one(&mut *tx).await?;
    if !allowed {
        return Err(ApiError::missing());
    }
    let rows=sqlx::query_as("SELECT t.id,t.account_id,a.name AS account_name,a.currency,t.amount::text,t.kind,t.occurred_on,t.description,t.category,t.tags,t.note,t.transfer_id,t.split_parent_id,t.investment_trade_id FROM transactions t JOIN account_summaries a ON a.id=t.account_id WHERE NOT t.is_split_parent AND t.account_id=$1 ORDER BY t.occurred_on DESC,t.id LIMIT 200").bind(id).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
