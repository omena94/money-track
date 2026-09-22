use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::{AppState, tenant},
};
use axum::{Json, extract::State};
use axum::{
    extract::Path,
    http::HeaderMap,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use serde_json::{Value, json};
use std::{convert::Infallible, time::Duration};
use uuid::Uuid;

async fn owned_jobs(s: &AppState, id: Uuid, email: &str) -> Result<Vec<Value>> {
    let mut tx = tenant(&s.db, id, email).await?;
    Ok(sqlx::query_scalar(&format!("SELECT {} FROM sync_jobs j JOIN connections c ON c.id=j.connection_id WHERE j.owner_id=$1 ORDER BY j.created_at DESC LIMIT 100",crate::sync_log::JOB_JSON)).bind(id).fetch_all(&mut *tx).await?)
}
pub async fn cancel(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r=sqlx::query("UPDATE sync_jobs SET status='cancelled',message='使用者已取消',error_code='cancelled',lease_until=NULL,challenge=NULL,answer_cipher=NULL,updated_at=now() WHERE id=$1 AND owner_id=$2 AND status IN ('queued','running','waiting_user')").bind(id).bind(user.id).execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    crate::sync_log::record_in(&mut tx,user.id,id,"warn",None,"cancelled","使用者已取消同步",None).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn events(
    user: Identity,
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>> {
    let token = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .find_map(|p| p.trim().strip_prefix("money_session="))
        .unwrap_or("")
        .to_string();
    let token_hash = crate::auth::hash(&token);
    let stream = futures_util::stream::unfold(
        (s, user, token_hash, 0usize),
        |(s, user, token_hash, tick)| async move {
            if tick > 0 {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE token_hash=$1 AND user_id=$2 AND expires_at>now())").bind(&token_hash).bind(user.id).fetch_one(&s.auth_db).await.unwrap_or(false);
            if !valid || tick >= 900 {
                return None;
            }
            let event = match owned_jobs(&s, user.id, &user.email).await {
                Ok(rows) => Event::default().event("jobs").data(json!(rows).to_string()),
                Err(_) => Event::default().event("retry").data("{}"),
            };
            Some((Ok(event), (s, user, token_hash, tick + 1)))
        },
    );
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn run_one(s: &AppState) -> Result<()> {
    // Transaction lock is released even if the worker task is dropped.
    let mut guard = s.db.begin().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(72814493)")
        .fetch_one(&mut *guard)
        .await?;
    if !locked {
        return Ok(());
    }
    let result = run_locked(s).await;
    guard.commit().await?;
    result
}
async fn run_locked(s: &AppState) -> Result<()> {
    let users: Vec<(Uuid, String, bool)> =
        sqlx::query_as("SELECT id,email,is_admin OR sync_approved FROM users")
            .fetch_all(&s.auth_db)
            .await?;
    let prune = crate::sync_log::prune_due();
    for (owner, email, approved) in &users {
        let (owner, email, approved) = (*owner, email.as_str(), *approved);
        if prune && crate::sync_log::prune(s, owner, email).await.is_err() {
            tracing::warn!("expired sync events not pruned");
        }
        if approved {
            crate::sources::schedule_for(s, owner, email).await?;
        }
        // Resume durable normalized batches before expiring the worker lease.
        let mut pending = tenant(&s.db, owner, email).await?;
        let staged: Vec<Uuid> = sqlx::query_scalar(
            "SELECT job_id FROM source_batches WHERE owner_id=$1 AND cipher IS NOT NULL",
        )
        .bind(owner)
        .fetch_all(&mut *pending)
        .await?;
        pending.commit().await?;
        for id in staged {
            if approved {
                if crate::source_ingest::commit(s, owner, email, id)
                    .await
                    .is_err()
                {
                    tracing::warn!(job=%id,"staged financial batch recovery deferred");
                }
            } else {
                let mut tx = tenant(&s.db, owner, email).await?;
                sqlx::query("DELETE FROM source_batches WHERE job_id=$1 AND owner_id=$2")
                    .bind(id)
                    .bind(owner)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("UPDATE sync_jobs SET status='cancelled',message='本機同步資格已撤銷',error_code='not_approved',challenge=NULL,answer_cipher=NULL,lease_until=NULL,updated_at=now() WHERE id=$1 AND owner_id=$2 AND status IN ('running','waiting_user')").bind(id).bind(owner).execute(&mut *tx).await?;
                tx.commit().await?;
            }
        }
        let mut tx = tenant(&s.db, owner, email).await?;
        let expired:Vec<Uuid>=sqlx::query_scalar("UPDATE sync_jobs SET status='failed',message='工作租約到期，請重新執行',error_code='lease_expired',challenge=NULL,answer_cipher=NULL,lease_until=NULL,updated_at=now() WHERE owner_id=$1 AND status IN ('running','waiting_user') AND (lease_until IS NULL OR lease_until<now()) AND NOT EXISTS(SELECT 1 FROM source_batches b WHERE b.job_id=sync_jobs.id AND b.cipher IS NOT NULL) RETURNING id").bind(owner).fetch_all(&mut *tx).await?;
        for id in expired {
            crate::sync_log::record_in(&mut tx,owner,id,"error",None,"lease_expired","同步程序停止回應（可能是服務重新啟動），工作已中止",None).await?;
        }
        let job:Option<(Uuid,Uuid,String)>=sqlx::query_as("SELECT j.id,j.connection_id,c.connector FROM sync_jobs j JOIN connections c ON c.id=j.connection_id WHERE j.owner_id=$1 AND j.status='queued' ORDER BY j.created_at LIMIT 1 FOR UPDATE OF j SKIP LOCKED").bind(owner).fetch_optional(&mut *tx).await?;
        if let Some((id, connection, connector)) = job {
            if !approved {
                sqlx::query("UPDATE sync_jobs SET status='cancelled',message='本機同步資格已撤銷',error_code='not_approved',updated_at=now() WHERE id=$1 AND owner_id=$2").bind(id).bind(owner).execute(&mut *tx).await?;
                tx.commit().await?;
                continue;
            }
            if !crate::sources::IDS.contains(&connector.as_str()) {
                sqlx::query("UPDATE sync_jobs SET status='cancelled',message='此資料來源已移除',error_code='unsupported_source',challenge=NULL,answer_cipher=NULL,lease_until=NULL,updated_at=now() WHERE id=$1 AND owner_id=$2").bind(id).bind(owner).execute(&mut *tx).await?;
                crate::sync_log::record_in(&mut tx,owner,id,"warn",None,"unsupported_source","此資料來源已移除",None).await?;
                tx.commit().await?;
                continue;
            }
            sqlx::query("UPDATE sync_jobs SET status='running',message='正在啟動本機同步',attempts=attempts+1,lease_until=now()+interval '90 seconds',updated_at=now() WHERE id=$1 AND owner_id=$2").bind(id).bind(owner).execute(&mut *tx).await?;
            crate::sync_log::record_in(&mut tx,owner,id,"info",None,"started","開始執行同步",None).await?;
            tx.commit().await?;
            if prune {
                crate::sync_log::mark_pruned();
            }
            crate::source_worker::run(s, owner, email, id, connection).await?;
            return Ok(());
        }
        tx.commit().await?;
    }
    if prune {
        crate::sync_log::mark_pruned();
    }
    Ok(())
}
pub async fn catalog(_user: Identity) -> Json<Value> {
    Json(crate::sources::public_catalog())
}

pub async fn jobs(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    Ok(Json(owned_jobs(&s, user.id, &user.email).await?))
}
