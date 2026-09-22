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
#[ignore = "Requires migrated PostgreSQL and stopped workers"]
async fn richart_stays_separate_from_taishin() {
    use money_api::source_ingest;
    let db=PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth=PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let state=AppState{db:db.clone(),auth_db:auth.clone(),http:reqwest::Client::new(),redis:redis::Client::open("redis://127.0.0.1/").unwrap(),config:Arc::new(Config{origin:"http://localhost:3000".into(),google_client:String::new(),google_secret:String::new(),admin_email:String::new()})};
    let app=router(state.clone());let a=actor(&auth).await;
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1").bind(a.id).execute(&auth).await.unwrap();
    let (status,saved)=call(&app,&a,"POST","/connectors/richart/credentials",json!({"credentials":{"userId":"fixture","account":"richart-code","password":"richart-fixture"}})).await;
    assert_eq!(status,StatusCode::OK,"{saved}");let richart=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();let taishin=Uuid::new_v4();
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();sqlx::query("INSERT INTO connections(id,owner_id,connector,enabled) VALUES($1,$2,'taishin',true)").bind(taishin).bind(a.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    let mut bank=account("a",json!("100"));bank["data"]["account_type"]=json!("checking");
    let record=json!({"kind":"transaction","source_id":"b".repeat(64),"account_key":"a".repeat(64),"data":{"amount":"-10","currency":"TWD","date":"2026-09-01","description":"independent-source","status":"posted","entry_type":"expense"}});
    for connection in [richart,taishin]{let j=running(&state,&a,connection).await;source_ingest::stage(&state,a.id,&a.email,j,connection,&batch(json!([bank,record]),false)).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();}
    let mut empty=batch(json!([bank]),false);empty.state=json!({"cursor":json!({"version":1,"transport":"http","coverage":{"depositStart":"2026-09-01","end":"2026-09-24"}}).to_string()});
    let j=running(&state,&a,richart).await;source_ingest::stage(&state,a.id,&a.email,j,richart,&empty).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();let counts:Vec<(String,i64)>=sqlx::query_as("SELECT c.connector,count(r.id) FROM connections c LEFT JOIN source_records r ON r.connection_id=c.id AND r.current AND r.kind='transaction' WHERE c.owner_id=$1 GROUP BY c.connector ORDER BY c.connector").bind(a.id).fetch_all(&mut *tx).await.unwrap();assert_eq!(counts,vec![("richart".into(),0),("taishin".into(),1)]);let ciphers:Vec<(String,bool)>=sqlx::query_as("SELECT connector,credentials_cipher IS NOT NULL FROM connections WHERE owner_id=$1 ORDER BY connector").bind(a.id).fetch_all(&mut *tx).await.unwrap();assert_eq!(ciphers,vec![("richart".into(),true),("taishin".into(),false)]);tx.commit().await.unwrap();
    clean(&db,&auth,&a).await;
}

#[tokio::test]
#[ignore = "Requires migrated PostgreSQL and stopped workers"]
async fn hncb_stays_separate_from_richart() {
    use money_api::source_ingest;
    let db=PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth=PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let state=AppState{db:db.clone(),auth_db:auth.clone(),http:reqwest::Client::new(),redis:redis::Client::open("redis://127.0.0.1/").unwrap(),config:Arc::new(Config{origin:"http://localhost:3000".into(),google_client:String::new(),google_secret:String::new(),admin_email:String::new()})};
    let app=router(state.clone());let a=actor(&auth).await;
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1").bind(a.id).execute(&auth).await.unwrap();
    let (status,saved)=call(&app,&a,"POST","/connectors/hncb/credentials",json!({"credentials":{"userId":"fixture","account":"hncb-code","password":"hncb-fixture"}})).await;
    assert_eq!(status,StatusCode::OK,"{saved}");let hncb=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();let richart=Uuid::new_v4();
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();sqlx::query("INSERT INTO connections(id,owner_id,connector,enabled) VALUES($1,$2,'richart',true)").bind(richart).bind(a.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    let mut bank=account("a",json!("100"));bank["data"]["account_type"]=json!("checking");
    let record=json!({"kind":"transaction","source_id":"b".repeat(64),"account_key":"a".repeat(64),"data":{"amount":"-10","currency":"TWD","date":"2026-09-01","description":"independent-source","status":"posted","entry_type":"expense"}});
    for connection in [hncb,richart]{let j=running(&state,&a,connection).await;source_ingest::stage(&state,a.id,&a.email,j,connection,&batch(json!([bank,record]),false)).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();}
    let mut empty=batch(json!([bank]),false);empty.state=json!({"cursor":json!({"version":1,"transport":"http","coverage":{"depositStart":"2026-09-01","end":"2026-09-24"}}).to_string()});
    let j=running(&state,&a,hncb).await;source_ingest::stage(&state,a.id,&a.email,j,hncb,&empty).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();let counts:Vec<(String,i64)>=sqlx::query_as("SELECT c.connector,count(r.id) FROM connections c LEFT JOIN source_records r ON r.connection_id=c.id AND r.current AND r.kind='transaction' WHERE c.owner_id=$1 GROUP BY c.connector ORDER BY c.connector").bind(a.id).fetch_all(&mut *tx).await.unwrap();assert_eq!(counts,vec![("hncb".into(),0),("richart".into(),1)]);let ciphers:Vec<(String,bool)>=sqlx::query_as("SELECT connector,credentials_cipher IS NOT NULL FROM connections WHERE owner_id=$1 ORDER BY connector").bind(a.id).fetch_all(&mut *tx).await.unwrap();assert_eq!(ciphers,vec![("hncb".into(),true),("richart".into(),false)]);tx.commit().await.unwrap();
    clean(&db,&auth,&a).await;
}

#[tokio::test]
#[ignore = "Requires migrated PostgreSQL and stopped workers"]
async fn native_bank_complete_windows_and_unknown_card_balances() {
    use money_api::source_ingest;
    let db=PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth=PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let state=AppState{db:db.clone(),auth_db:auth.clone(),http:reqwest::Client::new(),redis:redis::Client::open("redis://127.0.0.1/").unwrap(),config:Arc::new(Config{origin:"http://localhost:3000".into(),google_client:String::new(),google_secret:String::new(),admin_email:String::new()})};
    let app=router(state.clone());let a=actor(&auth).await;let other=actor(&auth).await;
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1 OR id=$2").bind(a.id).bind(other.id).execute(&auth).await.unwrap();
    for source in ["firstbank","fubon","cathaybk","yuanta"] {
        let (status,saved)=call(&app,&a,"POST",&format!("/connectors/{source}/credentials"),json!({"credentials":{"userId":"fixture","account":"fixture","password":"fixture"}})).await;
        assert_eq!(status,StatusCode::OK,"{source}: {saved}");let connection=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();
        assert_eq!(call(&app,&other,"POST",&format!("/connections/{connection}/sync"),json!({})).await.0,StatusCode::NOT_FOUND);
        let mut bank=account("a",json!("100"));bank["data"]["account_type"]=json!("checking");
        let mut card=account("b",Value::Null);card["data"]["account_type"]=json!("credit");
        let bill=json!({"kind":"bill","source_id":"c".repeat(64),"account_key":"b".repeat(64),"data":{"period":"2026-09","currency":"TWD","statement_amount":"500","paid_amount":null,"is_paid":null,"payment_due":"2026-10-01"}});
        let txrow=|id:&str,key:&str,date:&str,status:&str,period:Value|json!({"kind":"transaction","source_id":id.repeat(64),"account_key":key.repeat(64),"data":{"amount":"-10","currency":"TWD","description":"window-fixture","date":date,"entry_type":"expense","status":status,"statement_period":period}});
        let rows=json!([bank,card,bill,txrow("d","a","2026-09-01","posted",Value::Null),txrow("e","a","2026-09-24","posted",Value::Null),txrow("f","a","2026-08-31","posted",Value::Null),txrow("1","b","2026-09-22","pending",Value::Null),txrow("2","b","2026-09-02","posted",json!("2026-09"))]);
        let j=running(&state,&a,connection).await;source_ingest::stage(&state,a.id,&a.email,j,connection,&batch(rows,false)).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();
        let (_,assets)=call(&app,&a,"GET","/source-assets",Value::Null).await;
        let unknown=assets.as_array().unwrap().iter().find(|r|r["connector"]==source&&r["account_type"]=="credit").unwrap();assert!(unknown["amount"].is_null(),"unknown paid amount stays unknown: {unknown}");
        let cursor=if source=="cathaybk"{json!({"version":2,"transport":"http","coverage":{"twdStart":"2026-09-01","fxStart":"2026-09-01","end":"2026-09-24","billPeriods":["2026-09"]}})}else{json!({"version":1,"transport":"http","coverage":{"depositStart":"2026-09-01","end":"2026-09-24","billPeriods":["202609"],"unbilledComplete":true}})};
        let mut empty=batch(json!([bank,card]),true);empty.state=json!({"cursor":cursor.to_string()});
        let j=running(&state,&a,connection).await;source_ingest::stage(&state,a.id,&a.email,j,connection,&empty).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();
        let mut tx=tenant(&db,a.id,&a.email).await.unwrap();let n:i64=sqlx::query_scalar("SELECT count(*) FROM source_records WHERE connection_id=$1 AND current AND kind='transaction'").bind(connection).fetch_one(&mut *tx).await.unwrap();assert_eq!(n,5,"partial retains history");tx.commit().await.unwrap();
        empty.partial=false;let j=running(&state,&a,connection).await;source_ingest::stage(&state,a.id,&a.email,j,connection,&empty).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();
        let mut tx=tenant(&db,a.id,&a.email).await.unwrap();let remaining:Vec<String>=sqlx::query_scalar("SELECT source_key FROM source_records WHERE connection_id=$1 AND current AND kind='transaction'").bind(connection).fetch_all(&mut *tx).await.unwrap();assert_eq!(remaining,vec!["f".repeat(64)],"{source}: only outside-window history survives");tx.commit().await.unwrap();
        let (_,assets)=call(&app,&a,"GET","/source-assets",Value::Null).await;let unknown=assets.as_array().unwrap().iter().find(|r|r["connector"]==source&&r["account_type"]=="credit").unwrap();assert!(unknown["amount"].is_null(),"retired bills do not fill balance");
    }
    clean(&db,&auth,&a).await;clean(&db,&auth,&other).await;
}

#[tokio::test]
#[ignore = "Requires migrated PostgreSQL and stopped workers"]
async fn calendar_isolation_and_login_budget_survive_jobs() {
    use money_api::{source_ingest,source_worker};
    let db=PgPoolOptions::new().max_connections(3).connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth=PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let state=AppState{db:db.clone(),auth_db:auth.clone(),http:reqwest::Client::new(),redis:redis::Client::open("redis://127.0.0.1/").unwrap(),config:Arc::new(Config{origin:"http://localhost:3000".into(),google_client:String::new(),google_secret:String::new(),admin_email:String::new()})};
    let app=router(state.clone());let a=actor(&auth).await;let b=actor(&auth).await;
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1").bind(a.id).execute(&auth).await.unwrap();
    let credentials=json!({"credentials":{"userId":"fixture","account":"fixture","password":"fixture"}});
    let (status,saved)=call(&app,&a,"POST","/connectors/esun/credentials",credentials.clone()).await;assert_eq!(status,StatusCode::OK,"{saved}");
    let connection=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();let job=running(&state,&a,connection).await;
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();let revision:i32=sqlx::query_scalar("SELECT revision FROM connections WHERE id=$1").bind(connection).fetch_one(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    assert!(!source_worker::reserve_login(&state,b.id,&b.email,connection,revision,job).await.unwrap());
    assert!(!source_worker::reserve_login(&state,a.id,&a.email,connection,revision+1,job).await.unwrap());
    assert!(source_worker::reserve_login(&state,a.id,&a.email,connection,revision,job).await.unwrap());
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();sqlx::query("UPDATE sync_jobs SET status='failed' WHERE id=$1").bind(job).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    let next=running(&state,&a,connection).await;
    assert!(source_worker::reserve_login(&state,a.id,&a.email,connection,revision,next).await.unwrap());
    source_worker::release_captcha_login(&state,b.id,&b.email,connection,revision,next).await.unwrap();
    source_worker::release_captcha_login(&state,a.id,&a.email,connection,revision+1,next).await.unwrap();
    assert!(!source_worker::reserve_login(&state,a.id,&a.email,connection,revision,next).await.unwrap());
    source_worker::release_captcha_login(&state,a.id,&a.email,connection,revision,next).await.unwrap();
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();let remaining:i16=sqlx::query_scalar("SELECT login_failures FROM connections WHERE id=$1").bind(connection).fetch_one(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    assert_eq!(remaining,1,"captcha returns only this attempt, preserving the earlier password failure");
    assert!(source_worker::reserve_login(&state,a.id,&a.email,connection,revision,next).await.unwrap());
    assert_eq!(call(&app,&a,"POST",&format!("/connections/{connection}/sync"),json!({})).await.0,StatusCode::BAD_REQUEST);
    assert_eq!(call(&app,&a,"POST","/connectors/esun/credentials",credentials).await.0,StatusCode::OK);
    let job=running(&state,&a,connection).await;
    let rows=json!([account("a",json!("-500")),{"kind":"bill","source_id":"b".repeat(64),"account_key":"a".repeat(64),"data":{"period":"2026-08","currency":"TWD","statement_amount":"500.01","minimum_payment":"50","payment_due":"2026-10-01","is_paid":null}},{"kind":"bill","source_id":"c".repeat(64),"account_key":"a".repeat(64),"data":{"period":"2026-07","currency":"TWD","statement_amount":"10","payment_due":null}}]);
    source_ingest::stage(&state,a.id,&a.email,job,connection,&batch(rows,false)).await.unwrap();source_ingest::commit(&state,a.id,&a.email,job).await.unwrap();
    let (status,calendar)=call(&app,&a,"GET","/calendar?month=2026-10",Value::Null).await;assert_eq!(status,StatusCode::OK,"{calendar}");assert_eq!(calendar["events"].as_array().unwrap().len(),1);assert_eq!(calendar["events"][0]["statement_amount"],"500.01");assert_eq!(calendar["events"][0]["is_paid"],Value::Null);assert_eq!(calendar["unknown_due_count"],1);
    let (_,other)=call(&app,&b,"GET","/calendar?month=2026-10",Value::Null).await;assert_eq!(other["events"],json!([]));assert_eq!(other["unknown_due_count"],0);
    let (_,outside)=call(&app,&a,"GET","/calendar?month=2026-09",Value::Null).await;assert_eq!(outside["events"],json!([]));
    assert_eq!(call(&app,&a,"GET","/calendar?month=2026-13",Value::Null).await.0,StatusCode::BAD_REQUEST);
    let (_,saved)=call(&app,&a,"POST","/connectors/tdcc/credentials",json!({"credentials":{"userId":"fixture","password":"fixture"}})).await;
    let tdcc=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();let j=running(&state,&a,tdcc).await;
    let txrow=|id:&str,key:&str,date:&str|json!({"kind":"transaction","source_id":id.repeat(64),"account_key":key.repeat(64),"data":{"amount":"10","currency":"TWD","description":"dedupe-fixture","date":date,"entry_type":"income","status":"posted"}});
    let rows=json!([account("d",json!("100")),txrow("e","d","2026-09-01"),txrow("f","d","2025-08-01")]);
    source_ingest::stage(&state,a.id,&a.email,j,tdcc,&batch(rows,false)).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();
    let (_,saved)=call(&app,&a,"POST","/connectors/sinopac/credentials",json!({"credentials":{"userId":"fixture","account":"fixture","password":"fixture"}})).await;
    let native=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();let j=running(&state,&a,native).await;
    let mut bank=account("1",json!("100"));bank["data"]["tdcc_account_key"]=json!("d".repeat(64));bank["data"]["transactions_from"]=json!("2025-09-25");bank["data"]["transactions_to"]=json!("2026-09-24");
    source_ingest::stage(&state,a.id,&a.email,j,native,&batch(json!([bank,txrow("2","1","2026-09-01")]),false)).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();
    let (_,feed)=call(&app,&a,"GET","/activity-feed?q=dedupe-fixture",Value::Null).await;assert_eq!(feed.as_array().unwrap().len(),1,"{feed}");
    let (_,assets)=call(&app,&a,"GET","/source-assets",Value::Null).await;let duplicate=assets.as_array().unwrap().iter().find(|x|x["connector"]=="tdcc").unwrap();assert_eq!(duplicate["included"],false);assert_eq!(duplicate["reference_only"],true);assert!(duplicate["linked_account"].is_null());
    let reference=duplicate["id"].as_str().unwrap();let primary=assets.as_array().unwrap().iter().find(|x|x["connector"]=="sinopac").unwrap()["id"].as_str().unwrap();
    let link_path=format!("/source-assets/{reference}/link");
    let mapping=json!({"primary_kind":"source","primary_id":primary});
    assert_eq!(call(&app,&a,"PUT",&link_path,mapping.clone()).await.0,StatusCode::OK);
    assert_eq!(call(&app,&a,"PUT",&link_path,mapping.clone()).await.0,StatusCode::OK); // idempotent
    assert_eq!(call(&app,&b,"PUT",&link_path,mapping.clone()).await.0,StatusCode::NOT_FOUND);
    assert_eq!(call(&app,&b,"DELETE",&link_path,Value::Null).await.0,StatusCode::NOT_FOUND);
    assert_eq!(call(&app,&a,"PATCH",&format!("/source-assets/{reference}"),json!({"included":true})).await.0,StatusCode::BAD_REQUEST);
    assert_eq!(call(&app,&a,"PUT",&link_path,json!({"primary_kind":"source","primary_id":reference})).await.0,StatusCode::NOT_FOUND);
    let (_,assets)=call(&app,&a,"GET","/source-assets",Value::Null).await;let linked=assets.as_array().unwrap().iter().find(|x|x["id"]==reference).unwrap();assert_eq!(linked["linked_account"]["id"],primary);
    let mut tx=tenant(&db,b.id,&b.email).await.unwrap();let count:i64=sqlx::query_scalar("SELECT count(*) FROM source_account_links").fetch_one(&mut *tx).await.unwrap();assert_eq!(count,0);tx.commit().await.unwrap();
    let (_,worth)=call(&app,&a,"GET","/net-worth",Value::Null).await;assert_eq!(worth["net"],"-400","{worth}");
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();sqlx::query("UPDATE source_records SET included=false WHERE connection_id=$1 AND kind='account'").bind(native).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    let (_,assets)=call(&app,&a,"GET","/source-assets",Value::Null).await;let recovered=assets.as_array().unwrap().iter().find(|x|x["connector"]=="tdcc").unwrap();assert_eq!(recovered["included"],false);assert_eq!(recovered["linked_account"]["included"],false);
    assert_eq!(call(&app,&a,"DELETE",&link_path,Value::Null).await.0,StatusCode::NO_CONTENT);
    let (_,assets)=call(&app,&a,"GET","/source-assets",Value::Null).await;let unlinked=assets.as_array().unwrap().iter().find(|x|x["connector"]=="tdcc").unwrap();assert_eq!(unlinked["included"],false);assert!(unlinked["linked_account"].is_null());
    let (_,manual)=call(&app,&a,"POST","/accounts",json!({"name":"手動銀行","kind":"bank","currency":"TWD","opening_balance":"40","institution":"測試"})).await;
    assert_eq!(call(&app,&a,"PUT",&link_path,json!({"primary_kind":"manual","primary_id":manual["id"]})).await.0,StatusCode::OK);
    let (_,other)=call(&app,&b,"POST","/accounts",json!({"name":"他人銀行","kind":"bank","currency":"TWD","opening_balance":"0"})).await;
    assert_eq!(call(&app,&a,"PUT",&link_path,json!({"primary_kind":"manual","primary_id":other["id"]})).await.0,StatusCode::NOT_FOUND);
    let (_,foreign)=call(&app,&a,"POST","/accounts",json!({"name":"美元銀行","kind":"bank","currency":"USD","opening_balance":"0"})).await;
    assert_eq!(call(&app,&a,"PUT",&link_path,json!({"primary_kind":"manual","primary_id":foreign["id"]})).await.0,StatusCode::BAD_REQUEST);
    let (_,worth)=call(&app,&a,"GET","/net-worth",Value::Null).await;assert_eq!(worth["net"].as_str().unwrap().parse::<rust_decimal::Decimal>().unwrap(),rust_decimal::Decimal::from(-460)); // no TDCC fallback
    let j=running(&state,&a,tdcc).await;
    let mut changed=account("d",json!("9999"));changed["data"]["name"]=json!("集保更新後帳戶");
    let holding=json!({"kind":"holding","source_id":"3".repeat(64),"account_key":null,"data":{"name":"測試股票","symbol":"FIXTURE","currency":"TWD","quantity":"1","market_value":"250","as_of":"2026-09-24T00:00:00Z"}});
    source_ingest::stage(&state,a.id,&a.email,j,tdcc,&batch(json!([changed,holding]),false)).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();
    let (_,assets)=call(&app,&a,"GET","/source-assets",Value::Null).await;let refreshed=assets.as_array().unwrap().iter().find(|x|x["id"]==reference).unwrap();assert_eq!(refreshed["included"],false);assert_eq!(refreshed["linked_account"]["id"],manual["id"]);
    let stock=assets.as_array().unwrap().iter().find(|x|x["kind"]=="holding").unwrap();assert_eq!(stock["included"],true);assert_eq!(stock["reference_only"],false);
    let (_,worth)=call(&app,&a,"GET","/net-worth",Value::Null).await;assert_eq!(worth["net"].as_str().unwrap().parse::<rust_decimal::Decimal>().unwrap(),rust_decimal::Decimal::from(-210));
    clean(&db,&auth,&a).await;clean(&db,&auth,&b).await;
}


#[tokio::test]
#[ignore = "Requires migrated PostgreSQL and stopped workers"]
async fn landbank_stays_separate_from_firstbank() {
    use money_api::source_ingest;
    let db=PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth=PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let state=AppState{db:db.clone(),auth_db:auth.clone(),http:reqwest::Client::new(),redis:redis::Client::open("redis://127.0.0.1/").unwrap(),config:Arc::new(Config{origin:"http://localhost:3000".into(),google_client:String::new(),google_secret:String::new(),admin_email:String::new()})};
    let app=router(state.clone());let a=actor(&auth).await;
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1").bind(a.id).execute(&auth).await.unwrap();
    let (status,saved)=call(&app,&a,"POST","/connectors/landbank/credentials",json!({"credentials":{"userId":"fixture","account":"landbank-code","password":"landbank-fixture"}})).await;
    assert_eq!(status,StatusCode::OK,"{saved}");let landbank=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();let firstbank=Uuid::new_v4();
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();sqlx::query("INSERT INTO connections(id,owner_id,connector,enabled) VALUES($1,$2,'firstbank',true)").bind(firstbank).bind(a.id).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    let mut bank=account("a",json!("100"));bank["data"]["account_type"]=json!("checking");
    let record=json!({"kind":"transaction","source_id":"b".repeat(64),"account_key":"a".repeat(64),"data":{"amount":"-10","currency":"TWD","date":"2026-09-01","description":"independent-source","status":"posted","entry_type":"expense"}});
    for connection in [landbank,firstbank]{let j=running(&state,&a,connection).await;source_ingest::stage(&state,a.id,&a.email,j,connection,&batch(json!([bank,record]),false)).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();}
    let mut empty=batch(json!([bank]),false);empty.state=json!({"cursor":json!({"version":1,"transport":"http","coverage":{"depositStart":"2026-09-01","end":"2026-09-24"}}).to_string()});
    let j=running(&state,&a,landbank).await;source_ingest::stage(&state,a.id,&a.email,j,landbank,&empty).await.unwrap();source_ingest::commit(&state,a.id,&a.email,j).await.unwrap();
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();let counts:Vec<(String,i64)>=sqlx::query_as("SELECT c.connector,count(r.id) FROM connections c LEFT JOIN source_records r ON r.connection_id=c.id AND r.current AND r.kind='transaction' WHERE c.owner_id=$1 GROUP BY c.connector ORDER BY c.connector").bind(a.id).fetch_all(&mut *tx).await.unwrap();let ciphers:Vec<(String,bool)>=sqlx::query_as("SELECT connector,credentials_cipher IS NOT NULL FROM connections WHERE owner_id=$1 ORDER BY connector").bind(a.id).fetch_all(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    clean(&db,&auth,&a).await;
    assert_eq!(counts,vec![("firstbank".into(),1),("landbank".into(),0)]);
    assert_eq!(ciphers,vec![("firstbank".into(),false),("landbank".into(),true)]);
}

#[tokio::test]
#[ignore = "Requires migrated PostgreSQL and stopped workers"]
async fn fubon_verification_permanently_disables_automatic_sync() {
    use money_api::sources;
    let db=PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth=PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let state=AppState{db:db.clone(),auth_db:auth.clone(),http:reqwest::Client::new(),redis:redis::Client::open("redis://127.0.0.1/").unwrap(),config:Arc::new(Config{origin:"http://localhost:3000".into(),google_client:String::new(),google_secret:String::new(),admin_email:String::new()})};
    let app=router(state.clone()); let a=actor(&auth).await; let b=actor(&auth).await;
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1 OR id=$2").bind(a.id).bind(b.id).execute(&auth).await.unwrap();
    let creds=json!({"credentials":{"userId":"fixture","account":"fixture","password":"fixture"}});
    let (status,saved)=call(&app,&a,"POST","/connectors/fubon/credentials",creds.clone()).await;
    assert_eq!(status,StatusCode::OK);
    let id=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();
    let job=running(&state,&a,id).await;
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();
    let rev:i32=sqlx::query_scalar("SELECT revision FROM connections WHERE id=$1").bind(id).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    sources::require_manual_verification(&state,b.id,&b.email,id,rev,job).await.unwrap();
    sources::require_manual_verification(&state,a.id,&a.email,id,rev+1,job).await.unwrap();
    let (_,rows)=call(&app,&a,"GET","/connections",Value::Null).await;
    assert_eq!(rows[0]["manual_only"],false);
    sources::require_manual_verification(&state,a.id,&a.email,id,rev,job).await.unwrap();
    let (_,rows)=call(&app,&a,"GET","/connections",Value::Null).await;
    assert_eq!(rows[0]["manual_only"],true); assert_eq!(rows[0]["daily"],false); assert!(rows[0]["next_run_at"].is_null());
    // Success/reset/reconfiguration do not erase the requirement.
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();
    sqlx::query("UPDATE sync_jobs SET status='succeeded',lease_until=NULL WHERE id=$1").bind(job).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(call(&app,&a,"POST",&format!("/connections/{id}/resume"),json!({})).await.0,StatusCode::OK);
    assert_eq!(call(&app,&a,"POST","/connectors/fubon/credentials",creds).await.0,StatusCode::OK);
    assert_eq!(call(&app,&a,"PATCH",&format!("/connections/{id}"),json!({"enabled":true,"daily":true})).await.0,StatusCode::BAD_REQUEST);
    assert_eq!(call(&app,&b,"PATCH",&format!("/connections/{id}"),json!({"enabled":true,"daily":true})).await.0,StatusCode::NOT_FOUND);
    assert_eq!(call(&app,&a,"PUT","/sync-settings",json!({"times":["07:00"],"attempt_limit":1,"failure_limit":2})).await.0,StatusCode::OK);
    sources::schedule_for(&state,a.id,&a.email).await.unwrap();
    let (_,rows)=call(&app,&a,"GET","/connections",Value::Null).await;
    assert_eq!(rows[0]["manual_only"],true); assert_eq!(rows[0]["daily"],false); assert!(rows[0]["next_run_at"].is_null());
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();
    let queued:i64=sqlx::query_scalar("SELECT count(*) FROM sync_jobs WHERE connection_id=$1 AND status='queued'").bind(id).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap(); assert_eq!(queued,0);
    assert_eq!(call(&app,&a,"POST",&format!("/connections/{id}/sync"),json!({})).await.0,StatusCode::OK);
    clean(&db,&auth,&a).await; clean(&db,&auth,&b).await;
}

#[tokio::test]
#[ignore = "Requires migrated PostgreSQL and stopped workers"]
async fn cathay_choice_challenge_is_owned_one_time_and_encrypted() {
    let db = PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth = PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let state = AppState { db: db.clone(), auth_db: auth.clone(), http: reqwest::Client::new(), redis: redis::Client::open("redis://127.0.0.1/").unwrap(), config: Arc::new(Config { origin: "http://localhost:3000".into(), google_client: String::new(), google_secret: String::new(), admin_email: String::new() }) };
    let app = router(state.clone());
    let a = actor(&auth).await;
    let b = actor(&auth).await;
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1 OR id=$2").bind(a.id).bind(b.id).execute(&auth).await.unwrap();
    let (status, saved) = call(&app, &a, "POST", "/connectors/cathaybk/credentials", json!({"credentials":{"userId":"fixture","account":"fixture","password":"fixture"}})).await;
    assert_eq!(status, StatusCode::OK);
    let connection = Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();
    let job = running(&state, &a, connection).await;
    let challenge = money_api::source_challenge::sanitize(&json!({"kind":"choice","message":"驗證方式","choices":[{"value":"sms","label":"手機簡訊"},{"value":"email","label":"Email"}]})).unwrap();
    let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
    sqlx::query("UPDATE sync_jobs SET status='waiting_user',challenge=$1,lease_until=now()+interval '5 minutes' WHERE id=$2 AND owner_id=$3").bind(&challenge).bind(job).bind(a.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let path = format!("/sync/jobs/{job}/continue");
    let answer = json!({"challenge_id":challenge["id"],"value":"email"});
    assert_eq!(call(&app,&b,"POST",&path,answer.clone()).await.0,StatusCode::NOT_FOUND);
    assert_eq!(call(&app,&a,"POST",&path,json!({"challenge_id":Uuid::new_v4(),"value":"email"})).await.0,StatusCode::BAD_REQUEST);
    assert_eq!(call(&app,&a,"POST",&path,json!({"challenge_id":challenge["id"],"value":"trust"})).await.0,StatusCode::BAD_REQUEST);
    assert_eq!(call(&app,&a,"POST",&path,answer.clone()).await.0,StatusCode::OK);
    assert_eq!(call(&app,&a,"POST",&path,answer).await.0,StatusCode::NOT_FOUND);
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();
    let cipher:String=sqlx::query_scalar("SELECT answer_cipher FROM sync_jobs WHERE id=$1 AND owner_id=$2").bind(job).bind(a.id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(money_api::vault::open(a.id,job,"answer",&cipher).unwrap()["value"],"email");
    assert!(!cipher.contains("email"));
    let otp = money_api::source_challenge::sanitize(&json!({"kind":"otp","expires_at":"2000-01-01T00:00:00Z"})).unwrap();
    sqlx::query("UPDATE sync_jobs SET answer_cipher=NULL,challenge=$1 WHERE id=$2 AND owner_id=$3").bind(&otp).bind(job).bind(a.id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(call(&app,&a,"POST",&path,json!({"challenge_id":otp["id"],"value":"123456"})).await.0,StatusCode::BAD_REQUEST);
    assert_eq!(call(&app,&a,"POST",&format!("/sync/jobs/{job}/cancel"),json!({})).await.0,StatusCode::OK);
    assert_eq!(call(&app,&a,"POST",&path,json!({"challenge_id":otp["id"],"value":"123456"})).await.0,StatusCode::NOT_FOUND);
    clean(&db,&auth,&a).await;
    clean(&db,&auth,&b).await;
}

#[tokio::test]
#[ignore = "Requires migrated PostgreSQL and stopped workers"]
async fn gmail_link_is_owned_and_schedule_requires_verified_committed_job() {
    use money_api::{gmail,vault};
    let db=PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth=PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let state=AppState{db:db.clone(),auth_db:auth.clone(),http:reqwest::Client::new(),redis:redis::Client::open("redis://127.0.0.1/").unwrap(),config:Arc::new(Config{origin:"http://localhost:3000".into(),google_client:"fixture-client".into(),google_secret:"fixture-secret".into(),admin_email:String::new()})};
    let app=router(state.clone());let a=actor(&auth).await;let b=actor(&auth).await;
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1 OR id=$2").bind(a.id).bind(b.id).execute(&auth).await.unwrap();
    let (_,saved)=call(&app,&a,"POST","/connectors/fubon/credentials",json!({"credentials":{"userId":"fixture","account":"fixture","password":"fixture"}})).await;
    let id=Uuid::parse_str(saved["id"].as_str().unwrap()).unwrap();let path=format!("/connections/{id}/gmail");
    assert_eq!(call(&app,&b,"GET",&path,Value::Null).await.0,StatusCode::NOT_FOUND);
    assert_eq!(call(&app,&b,"POST",&format!("{path}/authorize"),json!({})).await.0,StatusCode::NOT_FOUND);
    let req=Request::builder().method("POST").uri(format!("/api/v1{path}/authorize")).header("cookie",format!("money_session={}",a.session)).header("origin","http://localhost:3000").header("x-csrf-token",&a.csrf).header("content-type","application/json").body(Body::from(json!({"auto_requested":true}).to_string())).unwrap();
    let response=app.clone().oneshot(req).await.unwrap();assert_eq!(response.status(),StatusCode::OK);
    let cookie=response.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_owned();
    let data:Value=serde_json::from_slice(&to_bytes(response.into_body(),100_000).await.unwrap()).unwrap();
    let url=url::Url::parse(data["url"].as_str().unwrap()).unwrap();let params:std::collections::HashMap<_,_>=url.query_pairs().collect();
    assert_eq!(params["scope"],"https://www.googleapis.com/auth/gmail.readonly");assert_eq!(params["access_type"],"offline");assert_eq!(params["redirect_uri"],"http://localhost:3000/api/v1/auth/google/callback");
    let callback=format!("/api/v1/auth/google/callback?state={}&error=access_denied",params["state"]);
    for (actor,status) in [(&b,StatusCode::BAD_REQUEST),(&a,StatusCode::SEE_OTHER),(&a,StatusCode::BAD_REQUEST)] {
        let req=Request::builder().uri(&callback).header("cookie",format!("money_session={}; {cookie}",actor.session)).body(Body::empty()).unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(),status);
    }
    let generation=Uuid::new_v4();let sealed=vault::seal(a.id,id,"gmail-tokens",&json!({"refresh_token":"fixture-refresh"})).unwrap();
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();
    let revision:i32=sqlx::query_scalar("UPDATE connections SET manual_only=true,daily=false,next_run_at=NULL WHERE id=$1 RETURNING revision").bind(id).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO gmail_links(connection_id,owner_id,generation,email,tokens_cipher,auto_requested) VALUES($1,$2,$3,'mail@example.invalid',$4,true)").bind(id).bind(a.id).bind(generation).bind(sealed).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    let (_,status)=call(&app,&a,"GET",&path,Value::Null).await;assert_eq!(status["connected"],true);assert!(!status.to_string().contains("fixture-refresh"));
    let job=running(&state,&a,id).await;
    assert!(!gmail::verified(&state,a.id,&a.email,id,generation,revision,job).await.unwrap());
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();sqlx::query("UPDATE sync_jobs SET status='succeeded' WHERE id=$1").bind(job).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
    assert!(!gmail::verified(&state,a.id,&a.email,id,Uuid::new_v4(),revision,job).await.unwrap());
    assert!(!gmail::verified(&state,b.id,&b.email,id,generation,revision,job).await.unwrap());
    assert!(!gmail::verified(&state,a.id,&a.email,id,generation,revision+1,job).await.unwrap());
    assert!(gmail::verified(&state,a.id,&a.email,id,generation,revision,job).await.unwrap());
    let (_,rows)=call(&app,&a,"GET","/connections",Value::Null).await;assert_eq!(rows[0]["manual_only"],false);assert_eq!(rows[0]["daily"],true);assert!(!rows[0]["next_run_at"].is_null());
    assert_eq!(call(&app,&a,"PATCH",&format!("/connections/{id}"),json!({"enabled":true,"daily":false})).await.0,StatusCode::OK);
    assert!(!gmail::verified(&state,a.id,&a.email,id,generation,revision,job).await.unwrap());
    let (_,rows)=call(&app,&a,"GET","/connections",Value::Null).await;assert_eq!(rows[0]["daily"],false);
    gmail::fallback(&state,a.id,&a.email,id,Some(generation),"fixture fallback").await.unwrap();
    let (_,rows)=call(&app,&a,"GET","/connections",Value::Null).await;assert_eq!(rows[0]["manual_only"],true);assert_eq!(rows[0]["daily"],false);assert!(rows[0]["next_run_at"].is_null());
    assert_eq!(call(&app,&b,"DELETE",&path,Value::Null).await.0,StatusCode::NOT_FOUND);
    let active=running(&state,&a,id).await;
    assert_eq!(call(&app,&a,"DELETE",&path,Value::Null).await.0,StatusCode::OK);
    assert!(!gmail::verified(&state,a.id,&a.email,id,generation,revision,job).await.unwrap());
    let mut tx=tenant(&db,a.id,&a.email).await.unwrap();let cancelled:String=sqlx::query_scalar("SELECT status FROM sync_jobs WHERE id=$1").bind(active).fetch_one(&mut *tx).await.unwrap();tx.commit().await.unwrap();assert_eq!(cancelled,"cancelled");
    clean(&db,&auth,&a).await;clean(&db,&auth,&b).await;
}
