use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;
use uuid::Uuid;
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub auth_db: PgPool,
    pub http: reqwest::Client,
    pub config: Arc<Config>,
    pub redis: redis::Client,
}
pub struct Config {
    pub origin: String,
    pub google_client: String,
    pub google_secret: String,
    pub admin_email: String,
}
impl Config {
    pub fn env() -> anyhow::Result<Self> {
        let origin = std::env::var("APP_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into());
        let u = url::Url::parse(&origin)?;
        anyhow::ensure!(
            u.scheme() == "https"
                || (u.scheme() == "http"
                    && matches!(u.host_str(), Some("localhost" | "127.0.0.1"))),
            "APP_ORIGIN requires HTTPS or localhost"
        );
        anyhow::ensure!(
            u.path() == "/" && u.query().is_none() && u.fragment().is_none(),
            "APP_ORIGIN must be an origin"
        );
        Ok(Self {
            origin: origin.trim_end_matches('/').into(),
            google_client: std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default(),
            google_secret: std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default(),
            admin_email: std::env::var("ADMIN_EMAIL")
                .unwrap_or_default()
                .to_lowercase(),
        })
    }
    pub fn secure(&self) -> bool {
        self.origin.starts_with("https://")
    }
    pub fn google_ready(&self) -> bool {
        !self.google_client.is_empty() && !self.google_secret.is_empty()
    }
}
pub async fn tenant(
    db: &PgPool,
    id: Uuid,
    email: &str,
) -> crate::error::Result<Transaction<'static, Postgres>> {
    let mut tx = db.begin().await?;
    sqlx::query("SELECT set_config('app.user_id',$1,true),set_config('app.email',$2,true)")
        .bind(id.to_string())
        .bind(email)
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
