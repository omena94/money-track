use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use money_api::{
    auth::{hash, random_token},
    router,
    state::{AppState, Config, tenant},
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct Actor {
    id: Uuid,
    email: String,
    session: String,
    csrf: String,
}
async fn actor(pool: &PgPool) -> Actor {
    let id = Uuid::new_v4();
    let email = format!("test-{id}@example.invalid");
    let session = random_token();
    let csrf = random_token();
    sqlx::query(
        "INSERT INTO users(id,google_sub,email,name) VALUES($1,$2,$3,'Integration fixture')",
    )
    .bind(id)
    .bind(format!("fixture-{id}"))
    .bind(&email)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO sessions(token_hash,user_id,csrf_token,expires_at) VALUES($1,$2,$3,now()+interval '10 minutes')").bind(hash(&session)).bind(id).bind(&csrf).execute(pool).await.unwrap();
    Actor {
        id,
        email,
        session,
        csrf,
    }
}
async fn call(
    app: &Router,
    a: &Actor,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header("cookie", format!("money_session={}", a.session))
        .header("origin", "http://localhost:3000")
        .header("x-csrf-token", &a.csrf)
        .header("content-type", "application/json")
        .body(if body.is_null() {
            Body::empty()
        } else {
            Body::from(body.to_string())
        })
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 4 * 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn clean(db: &PgPool, auth: &PgPool, a: &Actor) {
    let mut tx = tenant(db, a.id, &a.email).await.unwrap();
    for table in [
        "source_records",
        "source_batches",
        "sync_jobs",
        "connections",
        "shares",
        "recurring_entries",
        "notifications",
        "transactions",
        "investment_trades",
        "investment_positions",
        "budgets",
        "goals",
        "accounts",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE owner_id=$1"))
            .bind(a.id)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(a.id)
        .execute(auth)
        .await
        .unwrap();
}

async fn running(s: &AppState, a: &Actor, connection: Uuid) -> Uuid {
    let mut tx = tenant(&s.db, a.id, &a.email).await.unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO sync_jobs(id,owner_id,connection_id,status,source_revision,lease_until) SELECT $1,$2,id,'running',revision,now()+interval '90 seconds' FROM connections WHERE id=$3 AND owner_id=$2")
        .bind(id).bind(a.id).bind(connection).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    id
}
fn batch(rows: Value, partial: bool) -> money_api::source_ingest::Batch {
    serde_json::from_value(
        json!({"rows":rows,"partial":partial,"state":{"cursor":"private-fixture-cursor"}}),
    )
    .unwrap()
}
fn account(id: &str, balance: Value) -> Value {
    json!({"kind":"account","source_id":id.repeat(64),"account_key":null,"data":{"name":"測試銀行 ***0123","currency":"TWD","balance":balance,"as_of":"2026-09-24T00:00:00Z"}})
}

#[tokio::test]
#[ignore = "Requires local initialized PostgreSQL and stopped workers"]
async fn removed_bank_leaves_no_assets_or_history() {
    use money_api::source_ingest;
    let db = PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth = PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let state = AppState {
        db: db.clone(), auth_db: auth.clone(), http: reqwest::Client::new(),
        config: Arc::new(Config { origin: "http://localhost:3000".into(), google_client: String::new(), google_secret: String::new(), admin_email: String::new() }),
        redis: redis::Client::open("redis://127.0.0.1:6379/").unwrap(),
    };
    let app = router(state.clone());
    let a = actor(&auth).await;
    let b = actor(&auth).await;
    sqlx::query("UPDATE users SET sync_approved=true WHERE id IN ($1,$2)").bind(a.id).bind(b.id).execute(&auth).await.unwrap();
    let mut ids = Vec::new();
    for (user, bank, balance) in [(&a, "hncb", "120423"), (&a, "richart", "7151"), (&b, "hncb", "999")] {
        let (status, value) = call(&app, user, "POST", &format!("/connectors/{bank}/credentials"), json!({"credentials":{"userId":"fixture","account":"fixture","password":"fixture"}})).await;
        assert_eq!(status, StatusCode::OK);
        let id = Uuid::parse_str(value["id"].as_str().unwrap()).unwrap();
        let job = running(&state, user, id).await;
        source_ingest::stage(&state, user.id, &user.email, job, id, &batch(json!([account("a", json!(balance))]), false)).await.unwrap();
        source_ingest::commit(&state, user.id, &user.email, job).await.unwrap();
        ids.push(id);
    }
    let before = call(&app, &a, "GET", "/source-assets", Value::Null).await.1;
    let worth_before = money_api::networth::compute(&db, a.id, &a.email).await.unwrap();
    let forbidden = call(&app, &b, "DELETE", &format!("/connections/{}", ids[0]), Value::Null).await.0;
    // A result already staged before removal must never restore deleted records.
    let pending = running(&state, &a, ids[0]).await;
    let payload = batch(json!([account("a", json!("120423"))]), false);
    source_ingest::stage(&state, a.id, &a.email, pending, ids[0], &payload).await.unwrap();
    let removed = call(&app, &a, "DELETE", &format!("/connections/{}", ids[0]), Value::Null).await.0;
    let late_commit = source_ingest::commit(&state, a.id, &a.email, pending).await;
    let late_stage = source_ingest::stage(&state, a.id, &a.email, pending, ids[0], &payload).await;
    let after = call(&app, &a, "GET", "/source-assets", Value::Null).await.1;
    let worth_after = money_api::networth::compute(&db, a.id, &a.email).await.unwrap();
    let other_owner = call(&app, &b, "GET", "/source-assets", Value::Null).await.1;
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    let mut remains = Vec::new();
    for table in ["source_records", "source_batches", "source_balance_history", "sync_jobs", "connections"] {
        let column = if table == "connections" { "id" } else { "connection_id" };
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE {column}=$1 AND owner_id=$2")).bind(ids[0]).bind(a.id).fetch_one(&mut *tx).await.unwrap();
        remains.push((table, count));
    }
    tx.commit().await.unwrap();
    clean(&db, &auth, &a).await;
    clean(&db, &auth, &b).await;
    assert_eq!(before.as_array().unwrap().len(), 2);
    assert_eq!(forbidden, StatusCode::NOT_FOUND);
    assert_eq!(removed, StatusCode::OK);
    assert_eq!(after.as_array().unwrap().len(), 1, "Removed bank still appears in assets");
    assert_eq!(after[0]["connector"], "richart");
    assert_eq!(worth_before["net"], "127574");
    assert_eq!(worth_after["net"], "7151");
    assert!(worth_after["history"].as_array().unwrap().iter().all(|point| point["bank"].as_i64().unwrap() <= 7151));
    assert_eq!(other_owner.as_array().unwrap().len(), 1);
    assert!(late_commit.is_err());
    assert!(late_stage.is_err());
    assert!(remains.iter().all(|(_, count)| *count == 0), "Residual rows: {remains:?}");
}

#[tokio::test]
#[ignore = "Requires local initialized PostgreSQL and stopped workers; run scripts/Test.ps1"]
async fn financial_sources_isolation_and_recovery() {
    use money_api::{source_ingest, sources, vault};
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let auth = PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("AUTH_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let state = AppState {
        db: db.clone(),
        auth_db: auth.clone(),
        http: reqwest::Client::new(),
        config: Arc::new(Config {
            origin: "http://localhost:3000".into(),
            google_client: String::new(),
            google_secret: String::new(),
            admin_email: String::new(),
        }),
        redis: redis::Client::open("redis://127.0.0.1:6379/").unwrap(),
    };
    let app = router(state.clone());
    let a = actor(&auth).await;
    let b = actor(&auth).await;
    let catalog = sources::catalog_data();
    assert!(catalog.as_array().unwrap().iter().any(|source| source["id"] == "einvoice" && source["credential_fields"] == json!(["mobile", "password"])));
    assert_eq!(catalog[0]["id"], "tdcc");
    let credentials = json!({"userId":"fixture-id","password":"fixture-private-password"});
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            "/connectors/tdcc/credentials",
            json!({"credentials":credentials})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE users SET sync_approved=true WHERE id IN ($1,$2)")
        .bind(a.id)
        .bind(b.id)
        .execute(&auth)
        .await
        .unwrap();
    let (status, saved) = call(
        &app,
        &a,
        "POST",
        "/connectors/tdcc/credentials",
        json!({"credentials":credentials}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let connection = Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            "/connectors/tdcc/credentials",
            json!({"credentials":{"userId":"x","password":"z","url":"https://example.invalid"}})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // 未開放的來源不能設定登入
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            "/connectors/taishin/credentials",
            json!({"credentials":{"userId":"x","account":"y","password":"z"}})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, connections) = call(&app, &a, "GET", "/connections", Value::Null).await;
    assert!(!connections.to_string().contains("fixture-private"));
    assert!(!connections.to_string().contains("cipher"));
    assert_eq!(
        call(&app, &b, "GET", "/connections", Value::Null).await.1,
        json!([])
    );
    for (method, path, body) in [
        (
            "GET",
            format!("/connections/{connection}/records"),
            Value::Null,
        ),
        (
            "POST",
            format!("/connections/{connection}/sync"),
            Value::Null,
        ),
        ("DELETE", format!("/connections/{connection}"), Value::Null),
        (
            "PATCH",
            format!("/connections/{connection}"),
            json!({"enabled":true,"daily":true}),
        ),
    ] {
        assert_eq!(
            call(&app, &b, method, &path, body).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    let cipher: String =
        sqlx::query_scalar("SELECT credentials_cipher FROM connections WHERE id=$1")
            .bind(connection)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert!(!cipher.contains("fixture-private"));
    assert_eq!(
        vault::open(a.id, connection, "credentials", &cipher).unwrap(),
        credentials
    );
    assert!(vault::open(b.id, connection, "credentials", &cipher).is_err());
    assert!(vault::open(a.id, Uuid::new_v4(), "credentials", &cipher).is_err());
    assert!(vault::open(a.id, connection, "state", &cipher).is_err());
    let mut tampered = cipher.clone().into_bytes();
    tampered[10] = if tampered[10] == b'A' { b'B' } else { b'A' };
    assert!(
        vault::open(
            a.id,
            connection,
            "credentials",
            std::str::from_utf8(&tampered).unwrap()
        )
        .is_err()
    );

    let (status, queued) = call(
        &app,
        &a,
        "POST",
        &format!("/connections/{connection}/sync"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    let job = Uuid::parse_str(queued["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            &format!("/connections/{connection}/sync"),
            Value::Null
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let challenge = Uuid::new_v4();
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    sqlx::query("UPDATE sync_jobs SET status='waiting_user',challenge=$1,lease_until=now()+interval '90 seconds' WHERE id=$2").bind(json!({"id":challenge,"kind":"otp","message":"測試驗證"})).bind(job).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let path = format!("/sync/jobs/{job}/continue");
    assert_eq!(
        call(
            &app,
            &b,
            "POST",
            &path,
            json!({"challenge_id":challenge,"value":"123456"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            &path,
            json!({"challenge_id":Uuid::new_v4(),"value":"123456"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            &path,
            json!({"challenge_id":challenge,"value":"123456"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            &path,
            json!({"challenge_id":challenge,"value":"123456"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, &b, "GET", "/sync/jobs", Value::Null).await.1,
        json!([])
    );
    let stream_request = Request::builder()
        .uri("/api/v1/sync/events")
        .header("cookie", format!("money_session={}", b.session))
        .body(Body::empty())
        .unwrap();
    let mut stream = app
        .clone()
        .oneshot(stream_request)
        .await
        .unwrap()
        .into_body()
        .into_data_stream();
    use futures_util::StreamExt;
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&chunk).contains("data: []"));
    drop(stream);
    assert!(
        !call(&app, &a, "GET", "/sync/jobs", Value::Null)
            .await
            .1
            .to_string()
            .contains("123456")
    );
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    let encrypted_answer: String =
        sqlx::query_scalar("SELECT answer_cipher FROM sync_jobs WHERE id=$1")
            .bind(job)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        vault::open(a.id, job, "answer", &encrypted_answer).unwrap()["value"],
        "123456"
    );
    sqlx::query(
        "UPDATE sync_jobs SET status='running',lease_until=now()-interval '1 second' WHERE id=$1",
    )
    .bind(job)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut bad = account("a", json!("0.3"));
    bad["data"]["raw"] = json!({"secret":"private"});
    assert!(
        source_ingest::stage(
            &state,
            a.id,
            &a.email,
            job,
            connection,
            &batch(json!([bad]), false)
        )
        .await
        .is_err()
    );
    let rows = json!([account("a",Value::Null),{"kind":"transaction","source_id":"b".repeat(64),"account_key":"a".repeat(64),"data":{"amount":"-0.1","currency":"TWD","description":"測試交易","date":"2026-09-24"}}]);
    source_ingest::stage(
        &state,
        a.id,
        &a.email,
        job,
        connection,
        &batch(rows.clone(), false),
    )
    .await
    .unwrap();
    // Simulate process interruption after staging: recover from a new pool with an expired lease.
    let restarted = AppState {
        db: PgPoolOptions::new()
            .max_connections(3)
            .connect(&std::env::var("DATABASE_URL").unwrap())
            .await
            .unwrap(),
        ..state.clone()
    };
    source_ingest::commit(&restarted, a.id, &a.email, job)
        .await
        .unwrap();
    source_ingest::commit(&restarted, a.id, &a.email, job)
        .await
        .unwrap();
    let (status, data) = call(
        &app,
        &a,
        "GET",
        &format!("/connections/{connection}/records"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{data}");
    assert_eq!(data["total"], 2);
    let (feed_status, feed) = call(&app, &a, "GET", "/activity-feed?q=測試交易", Value::Null).await;
    assert_eq!(feed_status, StatusCode::OK, "{feed}");
    assert!(feed.as_array().unwrap().is_empty()); // TDCC bank movements are reference-only.
    let source_transaction=data["rows"].as_array().unwrap().iter().find(|r|r["kind"]=="transaction").unwrap();
    assert_eq!(source_transaction["data"]["amount"],"-0.1"); // retained for source inspection
    let (_, other_feed) = call(&app, &b, "GET", "/activity-feed?q=測試交易", Value::Null).await;
    assert!(other_feed.as_array().unwrap().is_empty());
    let source_entry = source_transaction["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "DELETE",
            &format!("/transactions/{source_entry}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        data["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["kind"] == "account" && r["data"]["balance"].is_null())
    );
    let mut tx = tenant(&db, b.id, &b.email).await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM source_records WHERE connection_id=$1")
            .bind(connection)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(count, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM source_batches WHERE job_id=$1")
        .bind(job)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 0);
    tx.commit().await.unwrap();
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM notifications WHERE owner_id=$1")
        .bind(a.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let cleared: bool = sqlx::query_scalar(
        "SELECT cipher IS NULL AND committed_at IS NOT NULL FROM source_batches WHERE job_id=$1",
    )
    .bind(job)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(cleared);
    let cleared:bool=sqlx::query_scalar("SELECT challenge IS NULL AND answer_cipher IS NULL AND status='succeeded' FROM sync_jobs WHERE id=$1").bind(job).fetch_one(&mut *tx).await.unwrap();
    assert!(cleared);
    tx.commit().await.unwrap();
    let repeat = running(&state, &a, connection).await;
    source_ingest::stage(
        &state,
        a.id,
        &a.email,
        repeat,
        connection,
        &batch(rows, false),
    )
    .await
    .unwrap();
    source_ingest::commit(&state, a.id, &a.email, repeat)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "GET",
            &format!("/connections/{connection}/records"),
            Value::Null
        )
        .await
        .1["total"],
        2
    );

    let partial = running(&state, &a, connection).await;
    source_ingest::stage(
        &state,
        a.id,
        &a.email,
        partial,
        connection,
        &batch(json!([account("c", json!("123.45"))]), true),
    )
    .await
    .unwrap();
    source_ingest::commit(&state, a.id, &a.email, partial)
        .await
        .unwrap();
    let (_, data) = call(
        &app,
        &a,
        "GET",
        &format!("/connections/{connection}/records?kind=account"),
        Value::Null,
    )
    .await;
    assert!(
        data["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["current"] == true)
    );
    let full = running(&state, &a, connection).await;
    source_ingest::stage(
        &state,
        a.id,
        &a.email,
        full,
        connection,
        &batch(json!([account("c", json!("123.45"))]), false),
    )
    .await
    .unwrap();
    source_ingest::commit(&state, a.id, &a.email, full)
        .await
        .unwrap();
    let (_, data) = call(
        &app,
        &a,
        "GET",
        &format!("/connections/{connection}/records?kind=account"),
        Value::Null,
    )
    .await;
    assert_eq!(
        data["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["current"] == true)
            .count(),
        1
    );
    let cancelled = running(&state, &a, connection).await;
    source_ingest::stage(
        &state,
        a.id,
        &a.email,
        cancelled,
        connection,
        &batch(json!([account("d", json!("999"))]), false),
    )
    .await
    .unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            &format!("/sync/jobs/{cancelled}/cancel"),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    source_ingest::commit(&state, a.id, &a.email, cancelled)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "GET",
            &format!("/connections/{connection}/records"),
            Value::Null
        )
        .await
        .1["total"],
        3
    );

    let revoked = running(&state, &a, connection).await;
    source_ingest::stage(
        &state,
        a.id,
        &a.email,
        revoked,
        connection,
        &batch(json!([account("e", json!("888"))]), false),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE users SET sync_approved=false WHERE id=$1")
        .bind(a.id)
        .execute(&auth)
        .await
        .unwrap();
    source_ingest::commit(&state, a.id, &a.email, revoked)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "GET",
            &format!("/connections/{connection}/records"),
            Value::Null
        )
        .await
        .1["total"],
        3
    );
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1")
        .bind(a.id)
        .execute(&auth)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "PATCH",
            &format!("/connections/{connection}"),
            json!({"enabled":true,"daily":true})
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    sqlx::query("UPDATE connections SET next_run_at=now()-interval '1 second' WHERE id=$1")
        .bind(connection)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sources::schedule_for(&state, a.id, &a.email).await.unwrap();
    sources::schedule_for(&state, a.id, &a.email).await.unwrap();
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sync_jobs WHERE connection_id=$1 AND status='queued'",
    )
    .bind(connection)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count, 1);
    tx.commit().await.unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "DELETE",
            &format!("/connections/{connection}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    let cleared:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM connections WHERE id=$1)").bind(connection).fetch_one(&mut *tx).await.unwrap();
    assert!(cleared);
    tx.commit().await.unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "GET",
            &format!("/connections/{connection}/records"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // Electronic invoice ingestion is idempotent, owner-scoped, and separate from bank spending.
    let (status,saved)=call(&app,&a,"POST","/connectors/einvoice/credentials",json!({"credentials":{"mobile":"0900000000","password":"fixture-invoice"}})).await;
    assert_eq!(status,StatusCode::OK);
    let invoice_connection=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();
    let invoice=|key:&str,date:&str,status:&str|json!({"kind":"invoice","source_id":key.repeat(64),"data":{"number":"AB12345678","date":date,"seller":"測試商店","amount":"99","currency":"TWD","status":status,"carrier":"手機條碼","detail_status":"complete","donated":false,"has_allowance":false}});
    let item=|key:&str|json!({"kind":"invoice_item","source_id":key.repeat(64),"data":{"invoice_key":"d".repeat(64),"line_number":1,"description":"測試品項","amount":"99","currency":"TWD"}});
    let invoice_batch=|rows:Value,partial:bool| {
        let mut b=batch(rows,partial);
        b.state=json!({"cursor":json!({"transport":"einvoice-web","version":1,"coverage":{"start":"2026-07-01","end":"2026-09-24"}}).to_string()});b
    };
    for _ in 0..2 {
        let job=running(&state,&a,invoice_connection).await;
        let data=invoice_batch(json!([invoice("d","2026-09-24","active"),invoice("e","2026-06-01","active"),item("f")]),false);
        source_ingest::stage(&state,a.id,&a.email,job,invoice_connection,&data).await.unwrap();
        source_ingest::commit(&state,a.id,&a.email,job).await.unwrap();
    }
    let (_,invoices)=call(&app,&a,"GET",&format!("/connections/{invoice_connection}/records?kind=invoice"),Value::Null).await;
    assert_eq!(invoices["total"],2);assert_eq!(invoices["invoice_totals"][0]["amount"],"198");
    let selected=invoices["rows"].as_array().unwrap().iter().find(|r|r["data"]["date"]=="2026-09-24").unwrap()["id"].as_str().unwrap();
    assert_eq!(call(&app,&a,"GET",&format!("/connections/{invoice_connection}/records?kind=invoice_item&invoice={selected}"),Value::Null).await.1["total"],1);
    assert_eq!(call(&app,&b,"GET",&format!("/connections/{invoice_connection}/records?kind=invoice_item&invoice={selected}"),Value::Null).await.0,StatusCode::NOT_FOUND);
    assert!(call(&app,&a,"GET","/activity-feed?q=測試商店",Value::Null).await.1.as_array().unwrap().is_empty());
    // Partial reads preserve prior items. A complete void update retires them, but not older invoices.
    let job=running(&state,&a,invoice_connection).await;
    source_ingest::stage(&state,a.id,&a.email,job,invoice_connection,&invoice_batch(json!([invoice("d","2026-09-24","active")]),true)).await.unwrap();
    source_ingest::commit(&state,a.id,&a.email,job).await.unwrap();
    assert_eq!(call(&app,&a,"GET",&format!("/connections/{invoice_connection}/records?kind=invoice_item"),Value::Null).await.1["total"],1);
    let job=running(&state,&a,invoice_connection).await;
    source_ingest::stage(&state,a.id,&a.email,job,invoice_connection,&invoice_batch(json!([invoice("d","2026-09-24","void")]),false)).await.unwrap();
    source_ingest::commit(&state,a.id,&a.email,job).await.unwrap();
    assert_eq!(call(&app,&a,"GET",&format!("/connections/{invoice_connection}/records?kind=invoice_item"),Value::Null).await.1["total"],0);
    let result=call(&app,&a,"GET",&format!("/connections/{invoice_connection}/records?kind=invoice"),Value::Null).await.1;
    assert_eq!(result["total"],2);assert_eq!(result["invoice_totals"][0]["amount"],"99");
    // HSBC is manual-only at the API, scheduler and persistence boundaries.
    assert!(catalog.as_array().unwrap().iter().any(|source| source["id"] == "hsbc" && source["manual_only"] == true));
    let (status, saved) = call(&app, &a, "POST", "/connectors/hsbc/credentials", json!({"credentials":{"account":"fixture-hsbc-user"}})).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let hsbc = Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();
    // Retired connections keep their data, but cannot be re-enabled or scheduled.
    for source in ["taishin", "ctbc", "skbank", "obank"] {
        let retired = Uuid::new_v4();
        let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
        // 原 connection 已在前面的移除驗證刪除；使用仍存在的 HSBC fixture。
        let inserted = sqlx::query("INSERT INTO connections(id,owner_id,connector,enabled,daily,next_run_at,credentials_cipher) SELECT $1,$2,$3,true,true,now()-interval '1 minute',credentials_cipher FROM connections WHERE id=$4 AND owner_id=$2")
            .bind(retired).bind(a.id).bind(source).bind(hsbc).execute(&mut *tx).await.unwrap();
        assert_eq!(inserted.rows_affected(), 1, "退役來源 fixture 必須實際建立");
        tx.commit().await.unwrap();
        for (method, suffix, body) in [
            ("POST", "/sync", Value::Null),
            ("POST", "/resume", Value::Null),
            ("PATCH", "", json!({"enabled":true,"daily":true})),
        ] {
            assert_eq!(call(&app, &a, method, &format!("/connections/{retired}{suffix}"), body).await.0, StatusCode::NOT_FOUND);
        }
        sources::schedule_for(&state, a.id, &a.email).await.unwrap();
        let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
        let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_jobs WHERE connection_id=$1 AND owner_id=$2")
            .bind(retired).bind(a.id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(jobs, 0);
        tx.commit().await.unwrap();
        assert_eq!(call(&app, &a, "DELETE", &format!("/connections/{retired}"), Value::Null).await.0, StatusCode::OK);
    }
    let (_, connections) = call(&app, &a, "GET", "/connections", Value::Null).await;
    let hsbc_setting = connections.as_array().unwrap().iter().find(|c| c["id"] == hsbc.to_string()).unwrap();
    assert_eq!(hsbc_setting["daily"], false);
    assert!(hsbc_setting["next_run_at"].is_null());
    assert_eq!(call(&app, &a, "PATCH", &format!("/connections/{hsbc}"), json!({"enabled":true,"daily":true})).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(call(&app, &b, "PATCH", &format!("/connections/{hsbc}"), json!({"enabled":true,"daily":false})).await.0, StatusCode::NOT_FOUND);
    for enabled in [false, true] {
        assert_eq!(call(&app, &a, "PATCH", &format!("/connections/{hsbc}"), json!({"enabled":enabled,"daily":false})).await.0, StatusCode::OK);
    }
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    sqlx::query("UPDATE connections SET daily=true,next_run_at=now()-interval '1 minute',state_cipher='legacy-fixture' WHERE id=$1").bind(hsbc).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO sync_jobs(id,owner_id,connection_id,status,source_revision) SELECT $1,owner_id,id,'queued',revision FROM connections WHERE id=$2").bind(Uuid::new_v4()).bind(hsbc).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    sources::schedule_for(&state, a.id, &a.email).await.unwrap();
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_jobs WHERE connection_id=$1 AND status IN ('queued','running','waiting_user')").bind(hsbc).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(count, 0);
    let manual: bool = sqlx::query_scalar("SELECT state_cipher IS NULL AND NOT daily AND next_run_at IS NULL FROM connections WHERE id=$1").bind(hsbc).fetch_one(&mut *tx).await.unwrap();
    assert!(manual);
    tx.commit().await.unwrap();
    for _ in 0..2 {
        let (status, queued) = call(&app, &a, "POST", &format!("/connections/{hsbc}/sync"), Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let job = queued["id"].as_str().unwrap();
        assert_eq!(call(&app, &a, "POST", &format!("/sync/jobs/{job}/cancel"), Value::Null).await.0, StatusCode::OK);
    }
    let job = running(&state, &a, hsbc).await;
    // Even an older staged result containing auth state must not retain a reusable session.
    source_ingest::stage(&state, a.id, &a.email, job, hsbc, &batch(json!([account("f", json!("1000"))]), false)).await.unwrap();
    source_ingest::commit(&state, a.id, &a.email, job).await.unwrap();
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    let cleared: bool = sqlx::query_scalar("SELECT state_cipher IS NULL AND NOT daily AND next_run_at IS NULL FROM connections WHERE id=$1").bind(hsbc).fetch_one(&mut *tx).await.unwrap();
    assert!(cleared);
    tx.commit().await.unwrap();
    clean(&db, &auth, &a).await;
    clean(&db, &auth, &b).await;
}
