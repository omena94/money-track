use axum::{body::Body, extract::FromRequestParts, http::Request};
use money_api::{
    admin,
    auth::{Identity, hash, random_token},
    state::{AppState, Config},
};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use uuid::Uuid;

// Temporary tables on one connection: never changes real users or sessions.
#[tokio::test]
#[ignore = "Requires local PostgreSQL and AUTH_DATABASE_URL"]
async fn existing_session_updates_member_activity_and_logout_preserves_it() {
    let db = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AUTH_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TEMP TABLE users (LIKE public.users INCLUDING DEFAULTS); ALTER TABLE users DROP COLUMN IF EXISTS last_active_at; CREATE TEMP TABLE sessions (LIKE public.sessions INCLUDING DEFAULTS);")
        .execute(&db).await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/0023_member_activity.sql"))
        .execute(&db)
        .await
        .unwrap();
    let id = Uuid::new_v4();
    let idle = Uuid::new_v4();
    for uid in [id, idle] {
        sqlx::query("INSERT INTO users(id,google_sub,email,name,is_admin) VALUES($1,$2,$3,'Activity fixture',true)")
            .bind(uid).bind(uid.to_string()).bind(format!("{uid}@example.invalid"))
            .execute(&db).await.unwrap();
    }
    let token = random_token();
    sqlx::query("INSERT INTO sessions(token_hash,user_id,csrf_token,created_at,expires_at) VALUES($1,$2,'fixture',now()-interval '4 hours',now()+interval '1 day')")
        .bind(hash(&token)).bind(id).execute(&db).await.unwrap();
    let state = AppState {
        db: db.clone(),
        auth_db: db.clone(),
        http: reqwest::Client::new(),
        config: Arc::new(Config {
            origin: "http://localhost:3000".into(),
            google_client: String::new(),
            google_secret: String::new(),
            admin_email: String::new(),
        }),
        redis: redis::Client::open("redis://127.0.0.1:6379/").unwrap(),
    };
    let identify = |token: String| {
        let state = state.clone();
        async move {
            let (mut parts, _) = Request::builder()
                .uri("/api/v1/auth/me")
                .header("cookie", format!("money_session={token}"))
                .body(Body::empty())
                .unwrap()
                .into_parts();
            Identity::from_request_parts(&mut parts, &state).await
        }
    };
    assert!(identify(random_token()).await.is_err());
    sqlx::query("UPDATE sessions SET expires_at=now()-interval '1 second'")
        .execute(&db)
        .await
        .unwrap();
    assert!(identify(token.clone()).await.is_err());
    sqlx::query("UPDATE sessions SET expires_at=now()+interval '1 day'")
        .execute(&db)
        .await
        .unwrap();
    let (mut rejected, _) = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .header("cookie", format!("money_session={token}"))
        .body(Body::empty())
        .unwrap()
        .into_parts();
    assert!(
        Identity::from_request_parts(&mut rejected, &state)
            .await
            .is_err()
    );
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT last_active_at IS NULL FROM users WHERE id=$1")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap()
    );
    let user = identify(token.clone()).await.unwrap();
    let recent: bool = sqlx::query_scalar(
        "SELECT coalesce(last_active_at > now()-interval '5 seconds',false) FROM users WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&db)
    .await
    .unwrap();
    assert!(recent, "使用既有 session 後，最近活動仍未更新");
    let before: String = sqlx::query_scalar("SELECT last_active_at::text FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    identify(token.clone()).await.unwrap();
    let after: String = sqlx::query_scalar("SELECT last_active_at::text FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(before, after, "短時間內不重複寫入活動時間");
    sqlx::query("UPDATE users SET last_active_at=now()-interval '2 minutes' WHERE id=$1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    identify(token.clone()).await.unwrap();
    let rows = admin::users(user.clone(), axum::extract::State(state.clone()))
        .await
        .unwrap()
        .0;
    assert!(rows.iter().find(|r| r["id"] == id.to_string()).unwrap()["last_active_at"].is_string());
    assert!(rows.iter().find(|r| r["id"] == idle.to_string()).unwrap()["last_active_at"].is_null());
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT last_active_at > now()-interval '5 seconds' FROM users WHERE id=$1"
        )
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap()
    );
    // 查看家庭帳本只更新操作者；被檢視者仍沒有活動紀錄。
    sqlx::raw_sql("CREATE TEMP TABLE household_viewers (owner_id uuid, viewer_email text)")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO household_viewers VALUES($1,$2)")
        .bind(idle)
        .bind(&user.email)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET last_active_at=NULL WHERE id=$1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    let (mut parts, _) = Request::builder()
        .uri("/api/v1/accounts")
        .header("cookie", format!("money_session={token}"))
        .header("x-view-owner", idle.to_string())
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let viewed = Identity::from_request_parts(&mut parts, &state)
        .await
        .unwrap();
    assert_eq!(viewed.id, idle);
    assert!(
        admin::users(viewed, axum::extract::State(state.clone()))
            .await
            .is_err()
    );
    let rows = admin::users(user.clone(), axum::extract::State(state.clone()))
        .await
        .unwrap()
        .0;
    assert!(rows.iter().find(|r| r["id"] == idle.to_string()).unwrap()["last_active_at"].is_null());
    let saved = rows.iter().find(|r| r["id"] == id.to_string()).unwrap()["last_active_at"].clone();
    assert!(saved.is_string());
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("cookie", format!("money_session={token}").parse().unwrap());
    money_api::auth::logout(user.clone(), axum::extract::State(state.clone()), headers)
        .await
        .unwrap();
    assert!(identify(token).await.is_err());
    let rows = admin::users(user, axum::extract::State(state))
        .await
        .unwrap()
        .0;
    assert_eq!(
        rows.iter().find(|r| r["id"] == id.to_string()).unwrap()["last_active_at"],
        saved
    );
    db.close().await;
}
