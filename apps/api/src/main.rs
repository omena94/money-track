use money_api::{
    router,
    state::{AppState, Config},
};
use sqlx::postgres::PgPoolOptions;
use std::{sync::Arc, time::Duration};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("money_api=info")
        .init();
    let config = Arc::new(Config::env()?);
    let db = PgPoolOptions::new()
        .max_connections(12)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    let auth_db = PgPoolOptions::new()
        .max_connections(4)
        .connect(&std::env::var("AUTH_DATABASE_URL")?)
        .await?;
    let dangerous: bool = sqlx::query_scalar(
        "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_one(&db)
    .await?;
    anyhow::ensure!(!dangerous, "Runtime role must not bypass RLS");
    let state = AppState {
        db,
        auth_db,
        config,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        redis: redis::Client::open(
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".into()),
        )?,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await?;
    let scheduler_state = state.clone();
    let market_state = state.clone();
    tokio::spawn(async move {
        loop {
            if money_api::market::refresh_prices(&market_state, false)
                .await
                .is_err()
            {
                tracing::error!("Market refresh failed; previous quotes retained");
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
    let worker_state = state.clone();
    tokio::spawn(async move {
        loop {
            if money_api::sync::run_one(&worker_state).await.is_err() {
                tracing::error!("Sync worker failed; durable jobs will be checked again");
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if money_api::recurring::run_due(&scheduler_state)
                .await
                .is_err()
            {
                tracing::error!("Recurring scheduler failed; will retry next minute");
            }
        }
    });
    tracing::info!("Money Track API listening on 127.0.0.1:3001");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
