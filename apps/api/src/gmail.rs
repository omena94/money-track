//! Gmail access is separate from sign-in and bound to the source owner.
use crate::{
    auth::{self, Identity},
    error::{ApiError, Result},
    state::{AppState, tenant},
    vault,
};
use axum::{
    Json,
    extract::{FromRequestParts, Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub auto_requested: bool,
}

fn permitted(user: &Identity) -> Result<()> {
    if user.is_admin || user.sync_approved {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}
fn auth_url(s: &AppState, state: &str, verifier: &str) -> url::Url {
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", s.config.google_client.as_str()),
        (
            "redirect_uri",
            &format!("{}/api/v1/auth/google/callback", s.config.origin),
        ),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("access_type", "offline"),
        ("prompt", "consent select_account"),
        ("state", state),
        ("code_challenge", &auth::hash(verifier)),
        ("code_challenge_method", "S256"),
    ]);
    url
}
pub async fn status(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    permitted(&user)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM connections WHERE id=$1 AND owner_id=$2 AND connector='fubon')").bind(id).bind(user.id).fetch_one(&mut *tx).await?;
    if !exists {
        return Err(ApiError::missing());
    }
    let result:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('connected',true,'email',email,'auto_requested',auto_requested,'verified_at',verified_at,'last_error',last_error) FROM gmail_links WHERE connection_id=$1 AND owner_id=$2").bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(result.unwrap_or(json!({"connected":false}))))
}
pub async fn start(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Settings>,
) -> Result<Response> {
    permitted(&user)?;
    if !s.config.google_ready() {
        return Err(ApiError::bad("請先設定 Google OAuth"));
    }
    let state = format!("gmail.{}", auth::random_token());
    let browser = auth::random_token();
    let verifier = auth::random_token();
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let revision:i32=sqlx::query_scalar("SELECT revision FROM connections WHERE id=$1 AND owner_id=$2 AND connector='fubon' AND enabled AND credentials_cipher IS NOT NULL").bind(id).bind(user.id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::missing)?;
    sqlx::query("DELETE FROM gmail_oauth_attempts WHERE owner_id=$1")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    let sealed = vault::seal(user.id, id, "gmail-oauth", &json!({"verifier":verifier}))?;
    sqlx::query("INSERT INTO gmail_oauth_attempts(state_hash,owner_id,connection_id,source_revision,browser_hash,verifier_cipher,auto_requested,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,now()+interval '10 minutes')")
        .bind(auth::hash(&state)).bind(user.id).bind(id).bind(revision).bind(auth::hash(&browser)).bind(sealed).bind(v.auto_requested).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((
        [(
            header::SET_COOKIE,
            auth::set_cookie("money_gmail_oauth", &browser, 600, s.config.secure()),
        )],
        Json(json!({"url":auth_url(&s,&state,&verifier).as_str()})),
    )
        .into_response())
}
#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    scope: Option<String>,
}
pub async fn callback(s: AppState, headers: HeaderMap, q: auth::Callback) -> Result<Response> {
    if headers.contains_key("x-view-owner") {
        return Err(ApiError::forbidden());
    }
    let mut request = axum::http::Request::new(());
    *request.headers_mut() = headers.clone();
    let (mut parts, _) = request.into_parts();
    let user = Identity::from_request_parts(&mut parts, &s).await?;
    permitted(&user)?;
    let browser = auth::cookie(&headers, "money_gmail_oauth")
        .ok_or_else(|| ApiError::bad("Gmail 授權工作階段已過期"))?;
    let state = q
        .state
        .ok_or_else(|| ApiError::bad("缺少 Gmail 授權狀態"))?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let attempt:Option<(Uuid,i32,String,bool)>=sqlx::query_as("DELETE FROM gmail_oauth_attempts WHERE state_hash=$1 AND owner_id=$2 AND browser_hash=$3 AND expires_at>now() RETURNING connection_id,source_revision,verifier_cipher,auto_requested")
        .bind(auth::hash(&state)).bind(user.id).bind(auth::hash(&browser)).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    let (id, revision, sealed, auto) =
        attempt.ok_or_else(|| ApiError::bad("Gmail 授權狀態已過期，請重新連結"))?;
    let done = |result: &str| {
        (
            [(
                header::SET_COOKIE,
                auth::set_cookie("money_gmail_oauth", "", 0, s.config.secure()),
            )],
            Redirect::to(&format!("/?gmail={result}#/sources")),
        )
            .into_response()
    };
    if q.error.is_some() {
        return Ok(done("cancelled"));
    }
    let secret = vault::open(user.id, id, "gmail-oauth", &sealed)?;
    let code = q.code.ok_or_else(|| ApiError::bad("缺少 Gmail 授權碼"))?;
    let response = s
        .http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code.as_str()),
            ("client_id", s.config.google_client.as_str()),
            ("client_secret", s.config.google_secret.as_str()),
            (
                "redirect_uri",
                &format!("{}/api/v1/auth/google/callback", s.config.origin),
            ),
            ("grant_type", "authorization_code"),
            (
                "code_verifier",
                secret["verifier"]
                    .as_str()
                    .ok_or_else(|| ApiError::bad("Gmail 授權格式錯誤"))?,
            ),
        ])
        .send()
        .await
        .map_err(|_| ApiError::bad("Google 授權連線未成功"))?;
    if !response.status().is_success() {
        return Err(ApiError::bad("Google Gmail 授權未成功，請重新連結"));
    }
    let tokens: Tokens = response
        .json()
        .await
        .map_err(|_| ApiError::bad("Google 授權回應格式錯誤"))?;
    if !tokens
        .scope
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .any(|v| v == SCOPE)
    {
        return Err(ApiError::bad("請授予 Gmail 唯讀權限"));
    }
    let refresh = tokens
        .refresh_token
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::bad("未取得背景讀信授權，請重新連結 Gmail"))?;
    let response = s
        .http
        .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .map_err(|_| ApiError::bad("Gmail API 連線未成功"))?;
    if !response.status().is_success() {
        return Err(ApiError::bad(
            "請在 Google Cloud 啟用 Gmail API，再重新連結 Gmail",
        ));
    }
    let profile: Value = response
        .json()
        .await
        .map_err(|_| ApiError::bad("Gmail 帳號回應格式錯誤"))?;
    let email = profile["emailAddress"]
        .as_str()
        .filter(|e| e.len() < 255 && e.contains('@'))
        .ok_or_else(|| ApiError::bad("Gmail 帳號格式錯誤"))?
        .to_lowercase();
    let tokens = vault::seal(
        user.id,
        id,
        "gmail-tokens",
        &json!({"refresh_token":refresh}),
    )?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM connections WHERE id=$1 AND owner_id=$2 AND revision=$3 AND connector='fubon' AND enabled)").bind(id).bind(user.id).bind(revision).fetch_one(&mut *tx).await?;
    if !valid {
        return Err(ApiError::bad("來源已變更，請重新連結 Gmail"));
    }
    sqlx::query("INSERT INTO gmail_links(connection_id,owner_id,generation,email,tokens_cipher,auto_requested) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(connection_id) DO UPDATE SET generation=EXCLUDED.generation,email=EXCLUDED.email,tokens_cipher=EXCLUDED.tokens_cipher,auto_requested=EXCLUDED.auto_requested,verified_at=NULL,last_error=NULL,updated_at=now() WHERE gmail_links.owner_id=$2")
        .bind(id).bind(user.id).bind(Uuid::new_v4()).bind(email).bind(tokens).bind(auto).execute(&mut *tx).await?;
    sqlx::query("UPDATE connections SET manual_only=true,daily=false,next_run_at=NULL WHERE id=$1 AND owner_id=$2").bind(id).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(done("connected"))
}
pub struct Access {
    pub token: String,
    pub email: String,
    pub generation: Uuid,
    pub automatic: bool,
    pub needs_validation: bool,
}
pub async fn access(s: &AppState, owner: Uuid, email: &str, id: Uuid) -> Result<Option<Access>> {
    let mut tx = tenant(&s.db, owner, email).await?;
    let row:Option<(String,String,Uuid,bool,bool)>=sqlx::query_as("SELECT tokens_cipher,email,generation,auto_requested AND verified_at IS NOT NULL AND last_error IS NULL,verified_at IS NULL FROM gmail_links WHERE connection_id=$1 AND owner_id=$2").bind(id).bind(owner).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    let Some((sealed, mail, generation, automatic, needs_validation)) = row else {
        return Ok(None);
    };
    let secret = vault::open(owner, id, "gmail-tokens", &sealed)?;
    let response = s
        .http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("grant_type", "refresh_token"),
            (
                "refresh_token",
                secret["refresh_token"].as_str().unwrap_or(""),
            ),
            ("client_id", s.config.google_client.as_str()),
            ("client_secret", s.config.google_secret.as_str()),
        ])
        .send()
        .await;
    if let Ok(response) = response {
        if response.status().is_success() {
            if let Ok(tokens) = response.json::<Tokens>().await {
                if !tokens.access_token.is_empty() {
                    return Ok(Some(Access {
                        token: tokens.access_token,
                        email: mail,
                        generation,
                        automatic,
                        needs_validation,
                    }));
                }
            }
        }
    }
    fallback(
        s,
        owner,
        email,
        id,
        Some(generation),
        "Gmail 授權失效或連線未成功，請重新連結",
    )
    .await?;
    Ok(None)
}
pub async fn fallback(
    s: &AppState,
    owner: Uuid,
    email: &str,
    id: Uuid,
    generation: Option<Uuid>,
    reason: &str,
) -> Result<()> {
    let mut tx = tenant(&s.db, owner, email).await?;
    let changed=sqlx::query("UPDATE gmail_links SET verified_at=NULL,last_error=$1,updated_at=now() WHERE connection_id=$2 AND owner_id=$3 AND ($4::uuid IS NULL OR generation=$4)").bind(reason).bind(id).bind(owner).bind(generation).execute(&mut *tx).await?.rows_affected();
    if changed > 0 {
        sqlx::query("UPDATE connections SET manual_only=true,daily=false,next_run_at=NULL WHERE id=$1 AND owner_id=$2").bind(id).bind(owner).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn verified(
    s: &AppState,
    owner: Uuid,
    email: &str,
    id: Uuid,
    generation: Uuid,
    revision: i32,
    job: Uuid,
) -> Result<bool> {
    let mut tx = tenant(&s.db, owner, email).await?;
    let automatic:Option<bool>=sqlx::query_scalar("UPDATE gmail_links g SET verified_at=now(),last_error=NULL,updated_at=now() WHERE connection_id=$1 AND owner_id=$2 AND generation=$3 AND EXISTS(SELECT 1 FROM connections c JOIN sync_jobs j ON j.connection_id=c.id AND j.owner_id=c.owner_id WHERE c.id=g.connection_id AND c.owner_id=$2 AND c.revision=$4 AND c.enabled AND j.id=$5 AND j.status='succeeded' AND j.source_revision=$4) RETURNING auto_requested").bind(id).bind(owner).bind(generation).bind(revision).bind(job).fetch_optional(&mut *tx).await?;
    if automatic == Some(true) {
        let settings = crate::sync_settings::load(&mut tx, owner).await?;
        let next = crate::sync_settings::next_slot(&settings.times, chrono::Utc::now());
        sqlx::query("UPDATE connections SET manual_only=false,daily=true,next_run_at=$1 WHERE id=$2 AND owner_id=$3 AND revision=$4 AND enabled").bind(next).bind(id).bind(owner).bind(revision).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(automatic == Some(true))
}
pub async fn disconnect(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    permitted(&user)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let deleted = sqlx::query("DELETE FROM gmail_links WHERE connection_id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::missing());
    }
    sqlx::query("DELETE FROM gmail_oauth_attempts WHERE connection_id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE connections SET manual_only=true,daily=false,next_run_at=NULL WHERE id=$1 AND owner_id=$2").bind(id).bind(user.id).execute(&mut *tx).await?;
    sqlx::query("UPDATE sync_jobs SET status='cancelled',message='Gmail 已解除連結，請重新手動同步',challenge=NULL,answer_cipher=NULL,updated_at=now() WHERE connection_id=$1 AND owner_id=$2 AND status IN ('queued','running','waiting_user')").bind(id).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
