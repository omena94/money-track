pub mod account_links;
pub mod analysis;
pub mod admin;
pub mod auth;
pub mod calendar;
pub mod categories;
pub mod error;
pub mod finance;
pub mod fx;
pub mod gmail;
pub mod household;
pub mod investments;
pub mod ledger;
pub mod market;
pub mod networth;
pub mod profile;
pub mod recurring;
pub mod shares;
pub mod subscriptions;
pub mod source_ingest;
pub mod source_worker;
pub mod source_challenge;
pub mod sources;
pub mod state;
pub mod user_categories;
pub mod sync;
pub mod sync_log;
pub mod sync_settings;
pub mod vault;
use axum::{
    Json, Router,
    extract::State,
    routing::{delete, get, post},
};
use serde_json::{Value, json};
use state::AppState;
use utoipa::OpenApi;
#[derive(OpenApi)]
#[openapi(
    paths(
        account_links::link,
        account_links::unlink,
        finance::accounts,
        calendar::events,
        finance::create_account,
        finance::edit_opening_balance,
        finance::transactions,
        finance::activity_feed,
        finance::create_transaction,
        investments::create_trade,
        investments::quote,
        ledger::refund,
        sources::list,
        sources::save_credentials,
        sources::settings,
        sources::forget,
        sources::enqueue,
        sources::resume,
        sources::records
    ),
    components(schemas(
        account_links::LinkAccount,
        finance::Account,
        finance::NewAccount,
        finance::OpeningBalance,
        finance::Entry,
        finance::NewEntry,
        investments::NewTrade,
        investments::ManualQuote,
        ledger::Refund,
        sources::Credentials,
        sources::Settings,
        sources::Answer
    ))
)]
pub struct ApiDoc;
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route(
            "/api/v1/openapi.json",
            get(|| async { Json(ApiDoc::openapi()) }),
        )
        .route("/api/v1/auth/google", get(auth::start))
        .route("/api/v1/auth/google/callback", get(auth::callback))
        .route("/api/v1/auth/me", get(auth::me))
        .route("/api/v1/connections/{id}/gmail", get(gmail::status).delete(gmail::disconnect))
        .route("/api/v1/connections/{id}/gmail/authorize", post(gmail::start))
        .route("/api/v1/profile", get(profile::get).patch(profile::update))
        .route("/api/v1/profile/photo", axum::routing::put(profile::photo))
        .route("/api/v1/users/{id}/avatar", get(profile::avatar))
        .route("/api/v1/auth/logout", post(auth::logout))
        .route(
            "/api/v1/accounts",
            get(finance::accounts).post(finance::create_account),
        )
        .route("/api/v1/accounts/{id}", delete(finance::delete_account))
        .route("/api/v1/accounts/{id}/opening-balance", axum::routing::patch(finance::edit_opening_balance))
        .route(
            "/api/v1/transactions",
            get(finance::transactions).post(finance::create_transaction),
        )
        .route(
            "/api/v1/transactions/{id}",
            delete(finance::delete_transaction).patch(ledger::edit),
        )
        .route("/api/v1/transactions/{id}/split", post(ledger::split))
        .route("/api/v1/transactions/{id}/refund", post(ledger::refund))
        .route(
            "/api/v1/recurring",
            get(recurring::list).post(recurring::create),
        )
        .route(
            "/api/v1/recurring/{id}",
            axum::routing::patch(recurring::toggle),
        )
        .route("/api/v1/notifications", get(recurring::notifications))
        .route("/api/v1/transfers", post(finance::transfer))
        .route("/api/v1/dashboard", get(finance::dashboard))
        .route("/api/v1/analysis", get(analysis::analysis))
        .route("/api/v1/sync-settings", get(sync_settings::get).put(sync_settings::put))
        .route("/api/v1/connections/{id}/resume", post(sync_settings::resume))
        .route("/api/v1/categories", get(user_categories::list).post(user_categories::create))
        .route("/api/v1/categories/order", axum::routing::put(user_categories::reorder))
        .route("/api/v1/categories/{id}", axum::routing::patch(user_categories::edit).delete(user_categories::delete))
        .route("/api/v1/calendar", get(calendar::events))
        .route("/api/v1/subscriptions", get(subscriptions::list).post(subscriptions::create))
        .route("/api/v1/subscriptions/month", get(subscriptions::month))
        .route("/api/v1/subscriptions/matches", get(subscriptions::matches))
        .route("/api/v1/subscriptions/suggestions", get(subscriptions::suggestions))
        .route("/api/v1/subscriptions/suggestions/dismiss", post(subscriptions::dismiss))
        .route("/api/v1/subscriptions/suggestions/dismissed", get(subscriptions::dismissed))
        .route("/api/v1/subscriptions/suggestions/restore", post(subscriptions::restore))
        .route("/api/v1/subscriptions/{id}", axum::routing::put(subscriptions::update).delete(subscriptions::delete))
        .route("/api/v1/subscriptions/{id}/icon", axum::routing::put(subscriptions::set_icon).delete(subscriptions::clear_icon))
        .route("/api/v1/subscriptions/{id}/links", axum::routing::put(subscriptions::link))
        .route("/api/v1/subscriptions/{id}/links/{transaction_id}", delete(subscriptions::unlink))
        .route("/api/v1/activity-feed", get(finance::activity_feed))
        .route("/api/v1/net-worth", get(networth::net_worth))
        .route("/api/v1/source-assets", get(networth::source_assets))
        .route("/api/v1/source-assets/{id}/link", axum::routing::put(account_links::link).delete(account_links::unlink))
        .route(
            "/api/v1/asset-history/{kind}/{id}",
            get(networth::asset_history),
        )
        .route(
            "/api/v1/source-assets/{id}",
            axum::routing::patch(networth::set_included),
        )
        .route("/api/v1/export/transactions.csv", get(finance::export))
        .route("/api/v1/import/transactions", post(finance::import))
        .route(
            "/api/v1/budgets",
            get(finance::budgets).post(finance::save_budget),
        )
        .route(
            "/api/v1/goals",
            get(finance::goals).post(finance::save_goal),
        )
        .route(
            "/api/v1/source-transactions/{id}",
            axum::routing::patch(categories::label),
        )
        .route(
            "/api/v1/source-transactions/{id}/label",
            delete(categories::unlabel),
        )
        .route(
            "/api/v1/category-rules",
            get(categories::rules).post(categories::create_rule),
        )
        .route(
            "/api/v1/category-rules/defaults",
            get(categories::defaults),
        )
        .route(
            "/api/v1/category-rules/reclassify",
            post(categories::reclassify),
        )
        .route(
            "/api/v1/category-rules/{id}",
            delete(categories::delete_rule),
        )
        .route("/api/v1/household", get(household::overview))
        .route("/api/v1/household/viewers", post(household::add_viewer))
        .route("/api/v1/household/people", get(household::people_search))
        .route(
            "/api/v1/household/viewers/{id}",
            delete(household::remove_viewer),
        )
        .route("/api/v1/household/summary", get(household::summary))
        .route("/api/v1/household/upcoming", get(household::upcoming))
        .route("/api/v1/household/explore", get(household::explore))
        .route("/api/v1/shares", get(shares::list).post(shares::invite))
        .route("/api/v1/shares/{id}", delete(shares::revoke))
        .route("/api/v1/shares/{id}/accept", post(shares::accept))
        .route("/api/v1/shared/accounts", get(shares::shared_accounts))
        .route(
            "/api/v1/shared/accounts/{id}/transactions",
            get(shares::shared_transactions),
        )
        .route("/api/v1/connectors", get(sync::catalog))
        .route("/api/v1/connections", get(sources::list))
        .route(
            "/api/v1/connectors/{id}/credentials",
            post(sources::save_credentials),
        )
        .route(
            "/api/v1/connections/{id}",
            axum::routing::patch(sources::settings).delete(sources::forget),
        )
        .route("/api/v1/connections/{id}/sync", post(sources::enqueue))
        .route("/api/v1/connections/{id}/records", get(sources::records))
        .route("/api/v1/sync/jobs/{id}/continue", post(sources::resume))
        .route("/api/v1/investments/positions", get(investments::positions))
        .route(
            "/api/v1/investments/positions/{id}/quote",
            post(investments::quote),
        )
        .route(
            "/api/v1/investments/trades",
            get(investments::trades).post(investments::create_trade),
        )
        .route("/api/v1/investments/trades/{id}", delete(investments::undo))
        .route("/api/v1/market/quotes", get(market::quotes))
        .route("/api/v1/fx", get(fx::rates))
        .route("/api/v1/market/refresh", post(market::refresh))
        .route("/api/v1/sync/jobs", get(sync::jobs))
        .route("/api/v1/sync/jobs/{id}/cancel", post(sync::cancel))
        .route("/api/v1/sync/events", get(sync::events))
        .route("/api/v1/sync/jobs/{id}/log", get(sync_log::log))
        .route("/api/v1/sync/jobs/{id}/events", get(sync_log::events))
        .route("/api/v1/admin/users", get(admin::users))
        .route(
            "/api/v1/admin/users/{id}/sync-approval",
            post(admin::approve),
        )
        .route("/api/v1/admin/users/{id}/role", post(admin::set_role))
        .route("/api/v1/admin/audit", get(admin::audit))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(axum::middleware::from_fn(auth::no_store))
        .with_state(state)
}
async fn health(State(s): State<AppState>) -> Json<Value> {
    let db = sqlx::query("SELECT 1").execute(&s.db).await.is_ok();
    let redis = match s.redis.get_multiplexed_async_connection().await {
        Ok(mut c) => redis::cmd("PING")
            .query_async::<String>(&mut c)
            .await
            .is_ok(),
        Err(_) => false,
    };
    Json(
        json!({"status":if db&&redis{"ok"}else{"degraded"},"database":db,"redis":redis,"google_configured":s.config.google_ready(),"login_url":format!("{}/api/v1/auth/google",s.config.origin)}),
    )
}
