use crate::{
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Json,
    extract::{FromRequestParts, Query, Request, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(Clone, Serialize, FromRow)]
pub struct Identity {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub is_admin: bool,
    pub sync_approved: bool,
    pub csrf_token: String,
    /// 頭像設定（小型 JSON）；照片本體另由 /users/{id}/avatar 取得
    pub avatar: Option<serde_json::Value>,
}
pub fn random_token() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
pub fn hash(v: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(v.as_bytes()))
}
pub(crate) fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|s| {
            let (k, v) = s.trim().split_once('=')?;
            (k == name).then(|| v.to_owned())
        })
}
pub(crate) fn set_cookie(name: &str, value: &str, seconds: i64, secure: bool) -> String {
    format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={seconds}{}",
        if secure { "; Secure" } else { "" }
    )
}
fn equals(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}
impl FromRequestParts<AppState> for Identity {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self> {
        let token = cookie(&parts.headers, "money_session").ok_or_else(ApiError::unauthorized)?;
        if token.len() != 43 {
            return Err(ApiError::unauthorized());
        }
        let user=sqlx::query_as::<_,Identity>("SELECT u.id,u.email,coalesce(u.display_name,u.name) AS name,u.is_admin,u.sync_approved,s.csrf_token,u.avatar FROM users u JOIN sessions s ON s.user_id=u.id WHERE s.token_hash=$1 AND s.expires_at>now()")
   .bind(hash(&token)).fetch_optional(&s.auth_db).await?.ok_or_else(ApiError::unauthorized)?;
        if !matches!(
            parts.method,
            axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
        ) {
            let origin = parts
                .headers
                .get(header::ORIGIN)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let csrf = parts
                .headers
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if origin != s.config.origin || !equals(csrf, &user.csrf_token) {
                return Err(ApiError::forbidden());
            }
        }
        // 家庭檢視：GET 帶 x-view-owner 時改以被授權的成員身分唯讀讀取。
        if let Some(raw) = parts.headers.get("x-view-owner") {
            if !matches!(
                parts.method,
                axum::http::Method::GET | axum::http::Method::HEAD
            ) {
                return Err(ApiError::forbidden());
            }
            let owner: Uuid = raw
                .to_str()
                .ok()
                .and_then(|v| v.parse().ok())
                .ok_or_else(ApiError::forbidden)?;
            if owner != user.id {
                if !crate::household::can_view(s, &user, owner).await? {
                    return Err(ApiError::forbidden());
                }
                let (email, name, avatar): (String, String, Option<serde_json::Value>) =
                    sqlx::query_as("SELECT email,coalesce(display_name,name),avatar FROM users WHERE id=$1")
                        .bind(owner)
                        .fetch_optional(&s.auth_db)
                        .await?
                        .ok_or_else(ApiError::forbidden)?;
                record_activity(s, user.id).await?;
                return Ok(Identity {
                    id: owner,
                    email,
                    name,
                    is_admin: false,
                    sync_approved: false,
                    csrf_token: user.csrf_token,
                    avatar,
                });
            }
        }
        record_activity(s, user.id).await?;
        Ok(user)
    }
}
// 永遠記在實際使用者，而不是家庭檢視的帳本擁有者；最多每 30 秒寫一次。
async fn record_activity(s: &AppState, id: Uuid) -> Result<()> {
    sqlx::query("UPDATE users SET last_active_at=clock_timestamp() WHERE id=$1 AND (last_active_at IS NULL OR last_active_at < clock_timestamp()-interval '30 seconds')")
        .bind(id)
        .execute(&s.auth_db)
        .await?;
    Ok(())
}
pub async fn no_store(req: Request, next: Next) -> Response {
    let mut r = next.run(req).await;
    let streaming = r
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    r.headers_mut().insert(
        header::CACHE_CONTROL,
        if streaming {
            "no-store, no-transform"
        } else {
            "no-store"
        }
        .parse()
        .unwrap(),
    );
    r
}
pub async fn me(user: Identity) -> Json<Identity> {
    Json(user)
}
pub async fn logout(
    user: Identity,
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Response> {
    if let Some(token) = cookie(&headers, "money_session") {
        sqlx::query("DELETE FROM sessions WHERE token_hash=$1 AND user_id=$2")
            .bind(hash(&token))
            .bind(user.id)
            .execute(&s.auth_db)
            .await?;
    }
    Ok((
        [(
            header::SET_COOKIE,
            set_cookie("money_session", "", 0, s.config.secure()),
        )],
        Json(serde_json::json!({"ok":true})),
    )
        .into_response())
}

fn google_authorization_url(
    client_id: &str,
    origin: &str,
    state: &str,
    nonce: &str,
    verifier: &str,
) -> url::Url {
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id),
        (
            "redirect_uri",
            &format!("{origin}/api/v1/auth/google/callback"),
        ),
        ("response_type", "code"),
        ("scope", "openid email profile"),
        ("prompt", "select_account"),
        ("state", state),
        ("nonce", nonce),
        ("code_challenge", &hash(verifier)),
        ("code_challenge_method", "S256"),
    ]);
    url
}

pub async fn start(State(s): State<AppState>) -> Result<Response> {
    if !s.config.google_ready() {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Google 登入尚待設定，請查看本機操作文件",
        ));
    }
    // Bound the global OAuth start rate. Failure to contact Redis fails closed for this entrypoint.
    let mut redis = s
        .redis
        .get_multiplexed_async_connection()
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "登入服務暫時忙碌"))?;
    let count:i64=redis::Script::new("local n=redis.call('INCR',KEYS[1]); if n==1 then redis.call('EXPIRE',KEYS[1],60) end; return n").key("money:oauth:start").invoke_async(&mut redis).await.map_err(|_|ApiError(StatusCode::SERVICE_UNAVAILABLE,"登入服務暫時忙碌"))?;
    if count > 60 {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "請稍後再嘗試登入"));
    }
    let state = random_token();
    let verifier = random_token();
    let browser = random_token();
    let nonce = random_token();
    sqlx::query("DELETE FROM oauth_attempts WHERE expires_at<now()")
        .execute(&s.auth_db)
        .await?;
    sqlx::query("INSERT INTO oauth_attempts(state_hash,browser_hash,verifier,nonce,expires_at) VALUES($1,$2,$3,$4,now()+interval '10 minutes')")
  .bind(hash(&state)).bind(hash(&browser)).bind(&verifier).bind(&nonce).execute(&s.auth_db).await?;
    let url = google_authorization_url(
        &s.config.google_client,
        &s.config.origin,
        &state,
        &nonce,
        &verifier,
    );
    Ok((
        [(
            header::SET_COOKIE,
            set_cookie("money_oauth", &browser, 600, s.config.secure()),
        )],
        Redirect::temporary(url.as_str()),
    )
        .into_response())
}
#[derive(Deserialize)]
pub struct Callback {
    pub(crate) state: Option<String>,
    pub(crate) code: Option<String>,
    pub(crate) error: Option<String>,
}
#[derive(Deserialize)]
struct Tokens {
    id_token: String,
}
#[derive(Deserialize)]
struct Claims {
    sub: String,
    email: String,
    email_verified: bool,
    name: Option<String>,
    nonce: String,
}
pub async fn callback(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<Callback>,
) -> Result<Response> {
    if q.state.as_deref().is_some_and(|v|v.starts_with("gmail.")) {
        return crate::gmail::callback(s,headers,q).await;
    }
    let browser =
        cookie(&headers, "money_oauth").ok_or_else(|| ApiError::bad("登入工作階段已過期"))?;
    let state = q.state.ok_or_else(|| ApiError::bad("登入狀態缺失"))?;
    let attempt:Option<(String,String)>=sqlx::query_as("DELETE FROM oauth_attempts WHERE state_hash=$1 AND browser_hash=$2 AND expires_at>now() RETURNING verifier,nonce")
  .bind(hash(&state)).bind(hash(&browser)).fetch_optional(&s.auth_db).await?;
    let (verifier, nonce) = attempt.ok_or_else(|| ApiError::bad("登入狀態已過期，請重新登入"))?;
    if q.error.is_some() {
        return Ok(Redirect::to("/?login=cancelled").into_response());
    }
    let code = q.code.ok_or_else(|| ApiError::bad("缺少登入授權碼"))?;
    let token_response = s
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
            ("code_verifier", &verifier),
        ])
        .send()
        .await?;
    if !token_response.status().is_success() {
        return Err(ApiError::bad("Google 登入驗證未成功，請重新登入"));
    }
    let tokens: Tokens = token_response.json().await?;
    let header = decode_header(&tokens.id_token).map_err(|_| ApiError::unauthorized())?;
    if header.alg != Algorithm::RS256 {
        return Err(ApiError::unauthorized());
    }
    let keys: JwkSet = s
        .http
        .get("https://www.googleapis.com/oauth2/v3/certs")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let jwk = keys
        .find(header.kid.as_deref().ok_or_else(ApiError::unauthorized)?)
        .ok_or_else(ApiError::unauthorized)?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[&s.config.google_client]);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);
    let claims = decode::<Claims>(
        &tokens.id_token,
        &DecodingKey::from_jwk(jwk).map_err(|_| ApiError::unauthorized())?,
        &validation,
    )
    .map_err(|_| ApiError::unauthorized())?
    .claims;
    if !equals(&nonce, &claims.nonce) || !claims.email_verified || claims.sub.is_empty() {
        return Err(ApiError::unauthorized());
    }
    let email = claims.email.to_lowercase();
    let id = Uuid::new_v4();
    let admin = email == s.config.admin_email && !s.config.admin_email.is_empty();
    let mut tx = s.auth_db.begin().await?;
    let user_id:Uuid=sqlx::query_scalar("INSERT INTO users(id,google_sub,email,name,is_admin,sync_approved) VALUES($1,$2,$3,$4,$5,$5) ON CONFLICT(google_sub) DO UPDATE SET email=EXCLUDED.email,name=EXCLUDED.name RETURNING id")
  .bind(id).bind(&claims.sub).bind(&email).bind(claims.name.unwrap_or_else(||email.clone())).bind(admin).fetch_one(&mut *tx).await?;
    let session = random_token();
    sqlx::query("INSERT INTO sessions(token_hash,user_id,csrf_token,expires_at) VALUES($1,$2,$3,now()+interval '7 days')").bind(hash(&session)).bind(user_id).bind(random_token()).execute(&mut *tx).await?;
    if let Some(previous) = cookie(&headers, "money_session") {
        sqlx::query("DELETE FROM sessions WHERE token_hash=$1")
            .bind(hash(&previous))
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM sessions WHERE expires_at<now()")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut response = Redirect::to("/").into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie("money_session", &session, 7 * 86400, s.config.secure())
            .parse()
            .unwrap(),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie("money_oauth", "", 0, s.config.secure())
            .parse()
            .unwrap(),
    );
    Ok(response)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_tokens_are_random_and_fixed_length() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
        assert_eq!(hash(&a).len(), 43);
    }
    #[test]
    fn cookies_are_exact_not_substring_matches() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            "not_money_session=x; money_session=y".parse().unwrap(),
        );
        assert_eq!(cookie(&h, "money_session"), Some("y".into()));
    }
    #[test]
    fn google_login_always_prompts_for_account_selection() {
        let url = google_authorization_url(
            "test-client",
            "https://money.example",
            "test-state",
            "test-nonce",
            "test-verifier",
        );
        let prompt = url
            .query_pairs()
            .find_map(|(key, value)| (key == "prompt").then(|| value.into_owned()));
        assert_eq!(prompt.as_deref(), Some("select_account"));
    }
}
