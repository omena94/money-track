use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
pub async fn users(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    if !user.is_admin {
        return Err(ApiError::forbidden());
    }
    // 測試帳號（@example.invalid）另外標記，前端預設隱藏
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',u.id,'name',coalesce(u.display_name,u.name),'avatar',u.avatar,'email',u.email,'sync_approved',u.sync_approved,'is_admin',u.is_admin,'created_at',u.created_at,'last_active_at',u.last_active_at,'last_login',(SELECT max(created_at) FROM sessions x WHERE x.user_id=u.id),'active_sessions',(SELECT count(*) FROM sessions x WHERE x.user_id=u.id AND x.expires_at>now()),'test',lower(u.email) LIKE '%@example.invalid') FROM users u ORDER BY u.is_admin DESC,u.created_at DESC LIMIT 1000").fetch_all(&s.auth_db).await?;
    Ok(Json(rows))
}
#[derive(Deserialize)]
pub struct Approval {
    approved: bool,
}
pub async fn approve(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Approval>,
) -> Result<Json<Value>> {
    if !user.is_admin {
        return Err(ApiError::forbidden());
    }
    let mut tx = s.auth_db.begin().await?;
    let r = sqlx::query("UPDATE users SET sync_approved=$1 WHERE id=$2 AND NOT is_admin")
        .bind(v.approved)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    sqlx::query("INSERT INTO audit_events(id,actor_id,target_id,action) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4())
        .bind(user.id)
        .bind(id)
        .bind(if v.approved {
            "sync_approved"
        } else {
            "sync_revoked"
        })
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
pub struct Role {
    admin: bool,
}
/// POST /admin/users/{id}/role：指定或移除管理員。不能移除自己，也至少保留一位管理員。
/// 指定為管理員時一併給同步資格，之後被移除管理員仍保留同步資格，需要再另外關閉。
pub async fn set_role(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Role>,
) -> Result<Json<Value>> {
    if !user.is_admin {
        return Err(ApiError::forbidden());
    }
    if id == user.id && !v.admin {
        return Err(ApiError::bad("不能移除自己的管理員權限；請由另一位管理員操作"));
    }
    let mut tx = s.auth_db.begin().await?;
    // 鎖住所有管理員列，避免兩位管理員同時互相移除
    let admins: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE is_admin FOR UPDATE")
        .fetch_all(&mut *tx)
        .await?;
    if !v.admin && admins.len() <= 1 && admins.contains(&id) {
        return Err(ApiError::bad("至少要保留一位管理員"));
    }
    let r = sqlx::query("UPDATE users SET is_admin=$1,sync_approved=sync_approved OR $1 WHERE id=$2 AND is_admin<>$1")
        .bind(v.admin)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        if !exists {
            return Err(ApiError::missing());
        }
        tx.commit().await?;
        return Ok(Json(json!({"is_admin": v.admin})));
    }
    sqlx::query("INSERT INTO audit_events(id,actor_id,target_id,action) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4())
        .bind(user.id)
        .bind(id)
        .bind(if v.admin { "admin_granted" } else { "admin_revoked" })
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"is_admin": v.admin})))
}

/// GET /admin/audit：最近 50 筆成員權限變更。
pub async fn audit(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    if !user.is_admin {
        return Err(ApiError::forbidden());
    }
    let rows = sqlx::query_scalar("SELECT jsonb_build_object('id',e.id,'action',e.action,'created_at',e.created_at,'actor',coalesce(a.display_name,a.name),'actor_email',a.email,'target',coalesce(t.display_name,t.name),'target_email',t.email) FROM audit_events e JOIN users a ON a.id=e.actor_id LEFT JOIN users t ON t.id=e.target_id WHERE e.action IN ('sync_approved','sync_revoked','admin_granted','admin_revoked') ORDER BY e.created_at DESC LIMIT 50")
        .fetch_all(&s.auth_db)
        .await?;
    Ok(Json(rows))
}
