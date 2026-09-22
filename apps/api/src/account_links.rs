//! Explicit owner-confirmed mappings. TDCC bank accounts remain reference-only even without a link.
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
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkAccount {
    pub primary_id: Uuid,
    pub primary_kind: String,
}

#[utoipa::path(put,path="/api/v1/source-assets/{id}/link",params(("id"=Uuid,Path)),request_body=LinkAccount,responses((status=200,body=Value)),tag="sources")]
pub async fn link(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<LinkAccount>,
) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let currency:Option<String>=sqlx::query_scalar("SELECT r.data->>'currency' FROM source_records r JOIN connections c ON c.id=r.connection_id AND c.owner_id=r.owner_id WHERE r.id=$1 AND r.owner_id=$2 AND r.kind='account' AND r.current AND c.connector='tdcc' FOR UPDATE OF r")
        .bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    let currency = currency.ok_or_else(ApiError::missing)?;
    let (source, manual) = match input.primary_kind.as_str() {
        "source" => {
            let target:Option<String>=sqlx::query_scalar("SELECT r.data->>'currency' FROM source_records r JOIN connections c ON c.id=r.connection_id AND c.owner_id=r.owner_id WHERE r.id=$1 AND r.owner_id=$2 AND r.current AND r.kind='account' AND c.connector=ANY($3) AND c.connector<>'tdcc' AND coalesce(r.data->>'account_type','savings') IN ('savings','checking','settlement_cash','time_deposit') FOR SHARE OF r")
                .bind(input.primary_id).bind(user.id).bind(&crate::sources::IDS[..]).fetch_optional(&mut *tx).await?;
            if target.ok_or_else(ApiError::missing)? != currency {
                return Err(ApiError::bad("請選擇相同幣別的銀行帳戶"));
            }
            (Some(input.primary_id), None)
        }
        "manual" => {
            let target:Option<String>=sqlx::query_scalar("SELECT currency FROM accounts WHERE id=$1 AND owner_id=$2 AND kind='bank' FOR SHARE")
                .bind(input.primary_id).bind(user.id).fetch_optional(&mut *tx).await?;
            if target.ok_or_else(ApiError::missing)? != currency {
                return Err(ApiError::bad("請選擇相同幣別的銀行帳戶"));
            }
            (None, Some(input.primary_id))
        }
        _ => return Err(ApiError::bad("帳戶連結類型不正確")),
    };
    sqlx::query("INSERT INTO source_account_links(owner_id,reference_id,primary_source_id,primary_manual_id) VALUES($1,$2,$3,$4) ON CONFLICT(reference_id) DO UPDATE SET primary_source_id=EXCLUDED.primary_source_id,primary_manual_id=EXCLUDED.primary_manual_id,created_at=now() WHERE source_account_links.owner_id=EXCLUDED.owner_id")
        .bind(user.id).bind(id).bind(source).bind(manual).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"linked":true})))
}

#[utoipa::path(delete,path="/api/v1/source-assets/{id}/link",params(("id"=Uuid,Path)),responses((status=204)),tag="sources")]
pub async fn unlink(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM source_records r JOIN connections c ON c.id=r.connection_id AND c.owner_id=r.owner_id WHERE r.id=$1 AND r.owner_id=$2 AND r.kind='account' AND c.connector='tdcc')")
        .bind(id).bind(user.id).fetch_one(&mut *tx).await?;
    if !exists {
        return Err(ApiError::missing());
    }
    sqlx::query("DELETE FROM source_account_links WHERE reference_id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
