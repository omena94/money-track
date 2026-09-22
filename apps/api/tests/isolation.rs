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

#[tokio::test]
#[ignore = "Requires local initialized PostgreSQL"]
async fn opening_balance_edit_preserves_transactions() {
    let db = PgPoolOptions::new().max_connections(4).connect(&std::env::var("DATABASE_URL").unwrap()).await.unwrap();
    let auth = PgPoolOptions::new().max_connections(2).connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
    let app = router(AppState {
        db: db.clone(), auth_db: auth.clone(), http: reqwest::Client::new(),
        config: Arc::new(Config { origin: "http://localhost:3000".into(), google_client: String::new(), google_secret: String::new(), admin_email: String::new() }),
        redis: redis::Client::open("redis://127.0.0.1:6379/").unwrap(),
    });
    let a = actor(&auth).await;
    let b = actor(&auth).await;
    let (_, account) = call(&app, &a, "POST", "/accounts", json!({"name":"期初編輯測試","kind":"cash","currency":"TWD","opening_balance":"1000"})).await;
    let id = account["id"].as_str().unwrap();
    let date: String = sqlx::query_scalar("SELECT to_char(now() AT TIME ZONE 'Asia/Taipei','YYYY-MM-DD')").fetch_one(&db).await.unwrap();
    for (amount, kind) in [("200", "income"), ("-50", "expense")] {
        let (s, _) = call(&app, &a, "POST", "/transactions", json!({"account_id":id,"amount":amount,"kind":kind,"occurred_on":date,"description":"既有收支"})).await;
        assert_eq!(s, StatusCode::CREATED);
    }
    let path = format!("/accounts/{id}/opening-balance");
    let (_, before) = call(&app, &a, "GET", "/dashboard", Value::Null).await;
    let (_, transactions) = call(&app, &a, "GET", "/transactions", Value::Null).await;
    let (status, edited) = call(&app, &a, "PATCH", &path, json!({"opening_balance":"2000.25","expected_opening_balance":"1000"})).await;
    if status != StatusCode::OK {
        clean(&db, &auth, &a).await;
        clean(&db, &auth, &b).await;
        assert_eq!(status, StatusCode::OK, "期初餘額必須可修改：{edited}");
        return;
    }
    let (again, repeated) = call(&app, &a, "PATCH", &path, json!({"opening_balance":"2000.25","expected_opening_balance":"2000.25"})).await;
    let (cross, _) = call(&app, &b, "PATCH", &path, json!({"opening_balance":"0","expected_opening_balance":"2000.25"})).await;
    let (stale, _) = call(&app, &a, "PATCH", &path, json!({"opening_balance":"0","expected_opening_balance":"1000"})).await;
    let (invalid, _) = call(&app, &a, "PATCH", &path, json!({"opening_balance":"NaN","expected_opening_balance":"2000.25"})).await;
    let (_, after) = call(&app, &a, "GET", "/dashboard", Value::Null).await;
    let (_, after_tx) = call(&app, &a, "GET", "/transactions", Value::Null).await;
    let (_, net) = call(&app, &a, "GET", "/net-worth", Value::Null).await;
    let (_, history) = call(&app, &a, "GET", &format!("/asset-history/manual/{id}"), Value::Null).await;
    let no_csrf = Request::builder().method("PATCH").uri(format!("/api/v1{path}"))
        .header("cookie", format!("money_session={}", a.session)).header("origin", "http://localhost:3000")
        .header("content-type", "application/json").body(Body::from(json!({"opening_balance":"0","expected_opening_balance":"2000.25"}).to_string())).unwrap();
    let csrf_status = app.clone().oneshot(no_csrf).await.unwrap().status();
    let (zero, zero_account) = call(&app, &a, "PATCH", &path, json!({"opening_balance":"0","expected_opening_balance":"2000.25"})).await;
    clean(&db, &auth, &a).await;
    clean(&db, &auth, &b).await;
    assert_eq!(edited["opening_balance"], "2000.25000000");
    assert_eq!(edited["cash_balance"], "2150.25000000");
    assert_eq!(again, StatusCode::OK);
    assert_eq!(repeated["balance"], edited["balance"]);
    assert_eq!(cross, StatusCode::NOT_FOUND);
    assert_eq!(stale, StatusCode::CONFLICT);
    assert_eq!(invalid, StatusCode::BAD_REQUEST);
    assert_eq!(csrf_status, StatusCode::FORBIDDEN);
    assert_eq!(net["net"], "2150.25000000");
    assert_eq!(before["monthly"][0]["income"], "200.00000000");
    assert_eq!(before["monthly"][0]["expense"], "50.00000000");
    assert_eq!(before["monthly"], after["monthly"]);
    assert_eq!(transactions, after_tx);
    assert_eq!(history["history"].as_array().unwrap().last().unwrap()["value"], 2150);
    assert_eq!(zero, StatusCode::OK);
    assert_eq!(zero_account["cash_balance"], "150.00000000");
}

#[tokio::test]
#[ignore = "Requires local initialized PostgreSQL; run scripts/Test.ps1"]
async fn money_isolation_and_accounting() {
    let db = PgPoolOptions::new()
        .max_connections(4)
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
    let (status, account) = call(
        &app,
        &a,
        "POST",
        "/accounts",
        json!({"name":"生活帳戶","kind":"bank","currency":"TWD","opening_balance":"1000"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{account}");
    let id = account["id"].as_str().unwrap();
    let (_, other) = call(&app, &b, "GET", "/accounts", Value::Null).await;
    assert_eq!(other, json!([]));
    let(status,_)=call(&app,&b,"POST","/transactions",json!({"account_id":id,"amount":"-1","kind":"expense","occurred_on":"2026-09-23","description":"cross user"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let no_csrf = Request::builder()
        .method("POST")
        .uri("/api/v1/accounts")
        .header("cookie", format!("money_session={}", a.session))
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(no_csrf).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let wrong_origin = Request::builder()
        .method("POST")
        .uri("/api/v1/accounts")
        .header("cookie", format!("money_session={}", a.session))
        .header("x-csrf-token", &a.csrf)
        .header("origin", "https://evil.invalid")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(wrong_origin).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    for amount in ["-0.1", "-0.2"] {
        let(status,v)=call(&app,&a,"POST","/transactions",json!({"account_id":id,"amount":amount,"kind":"expense","occurred_on":"2026-09-23","description":"精度驗證"})).await;
        assert_eq!(status, StatusCode::CREATED, "{v}");
    }
    let (_, balances) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    assert_eq!(balances[0]["balance"], "999.70000000");
    let (_, share) = call(
        &app,
        &a,
        "POST",
        "/shares",
        json!({"account_id":id,"email":b.email,"permission":"balance"}),
    )
    .await;
    let share_id = share["id"].as_str().unwrap();
    let (_, pending) = call(&app, &b, "GET", "/shared/accounts", Value::Null).await;
    assert_eq!(pending, json!([]));
    let (status, v) = call(
        &app,
        &b,
        "POST",
        &format!("/shares/{share_id}/accept"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{v}");
    let (_, shared) = call(&app, &b, "GET", "/shared/accounts", Value::Null).await;
    assert_eq!(shared[0]["balance"], "999.70000000");
    assert!(shared[0].get("opening_balance").is_none());
    assert!(shared[0].get("cash_balance").is_none());
    let (status, _) = call(
        &app,
        &b,
        "GET",
        &format!("/shared/accounts/{id}/transactions"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let mut tx = tenant(&db, b.id, &b.email).await.unwrap();
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM transactions WHERE account_id=$1")
        .bind(Uuid::parse_str(id).unwrap())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(visible, 0);
    let account_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE id=$1")
        .bind(Uuid::parse_str(id).unwrap())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        account_rows, 0,
        "Shared recipients use a restricted projection, never the original account row"
    );
    let summaries: i64 = sqlx::query_scalar("SELECT count(*) FROM account_summaries WHERE id=$1")
        .bind(Uuid::parse_str(id).unwrap())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(summaries, 1);
    tx.rollback().await.unwrap();
    let (_, dash) = call(&app, &b, "GET", "/dashboard", Value::Null).await;
    assert_eq!(dash["totals"], json!([]));
    assert_eq!(
        call(
            &app,
            &a,
            "DELETE",
            &format!("/shares/{share_id}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, &b, "GET", "/shared/accounts", Value::Null)
            .await
            .1,
        json!([])
    );
    let (_, share) = call(
        &app,
        &a,
        "POST",
        "/shares",
        json!({"account_id":id,"email":b.email,"permission":"details"}),
    )
    .await;
    let share_id = share["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            &b,
            "POST",
            &format!("/shares/{share_id}/accept"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, rows) = call(
        &app,
        &b,
        "GET",
        &format!("/shared/accounts/{id}/transactions"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let (_, target) = call(
        &app,
        &a,
        "POST",
        "/accounts",
        json!({"name":"信用卡","kind":"credit","currency":"TWD","opening_balance":"-200"}),
    )
    .await;
    let target_id = target["id"].as_str().unwrap();
    let(status,v)=call(&app,&a,"POST","/transfers",json!({"from":id,"to":target_id,"amount":"200","occurred_on":"2026-09-23","description":"繳信用卡"})).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (_, balances) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    assert_eq!(balances[0]["balance"], "799.70000000");
    assert_eq!(balances[1]["balance"], "0.00000000");
    let csv = "date,amount,kind,description,id\n2026-09-23,100,income,薪資,salary-1\n";
    for expected in [1, 0] {
        let (status, v) = call(
            &app,
            &a,
            "POST",
            "/import/transactions",
            json!({"account_id":id,"csv":csv,"commit":true}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        assert_eq!(v["inserted"], expected);
    }
    let (status, _) = call(
        &app,
        &b,
        "DELETE",
        &format!("/accounts/{target_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, expense) = call(&app, &a, "POST", "/transactions", json!({
        "account_id":id,"amount":"-100","kind":"expense","occurred_on":"2026-09-23","description":"拆帳驗證"
    })).await;
    let entry_id = expense["id"].as_str().unwrap();
    let split_path = format!("/transactions/{entry_id}/split");
    let parts = json!({"parts":[{"amount":"-40","category":"餐飲","description":"午餐"},{"amount":"-60","category":"購物","description":"用品"}]});
    assert_eq!(
        call(&app, &b, "POST", &split_path, parts.clone()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(call(&app,&a,"POST",&split_path,json!({"parts":[{"amount":"-40","category":"餐飲","description":"午餐"},{"amount":"-50","category":"購物","description":"用品"}]})).await.0,StatusCode::BAD_REQUEST);
    let (_, before_split) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    assert_eq!(
        call(&app, &a, "POST", &split_path, parts.clone()).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, &a, "POST", &split_path, parts).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, &a, "GET", "/accounts", Value::Null).await.1,
        before_split
    );
    let (_, entries) = call(&app, &a, "GET", "/transactions", Value::Null).await;
    let entries = entries.as_array().unwrap();
    assert!(!entries.iter().any(|v| v["id"] == entry_id));
    let child = entries.iter().find(|v| v["description"] == "午餐").unwrap()["id"]
        .as_str()
        .unwrap();
    let edit = json!({"category":"日常","note":"驗證備註","tags":["家庭"]});
    assert_eq!(
        call(
            &app,
            &b,
            "PATCH",
            &format!("/transactions/{child}"),
            edit.clone()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, &a, "PATCH", &format!("/transactions/{child}"), edit)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            &a,
            "DELETE",
            &format!("/transactions/{child}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, balances) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    assert_eq!(balances[0]["balance"], "899.70000000");
    let today = (chrono::Utc::now() + chrono::Duration::hours(8))
        .date_naive()
        .to_string();
    let rule = json!({"account_id":id,"amount":"-10","kind":"expense","description":"週期驗證","category":"訂閱","frequency":"monthly","next_on":today});
    assert_eq!(
        call(&app, &b, "POST", "/recurring", rule.clone()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, &a, "POST", "/recurring", rule).await.0,
        StatusCode::OK
    );
    money_api::recurring::run_due_for(&state, a.id, &a.email)
        .await
        .unwrap();
    money_api::recurring::run_due_for(&state, a.id, &a.email)
        .await
        .unwrap();
    let (_, balances) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    assert_eq!(balances[0]["balance"], "889.70000000");
    assert_eq!(
        call(&app, &b, "GET", "/recurring", Value::Null).await.1,
        json!([])
    );
    assert_eq!(
        call(&app, &b, "GET", "/notifications", Value::Null).await.1,
        json!([])
    );
    assert_eq!(
        call(&app, &a, "GET", "/notifications", Value::Null)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(&app, &a, "GET", "/admin/users", Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE users SET sync_approved=true WHERE id=$1")
        .bind(a.id)
        .execute(&auth)
        .await
        .unwrap();
    // 以資料庫直接建立一個排隊中的工作，驗證工作清單、取消與 SSE 的使用者隔離
    let job_id = Uuid::new_v4();
    {
        let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
        let connection: Uuid = sqlx::query_scalar(
            "INSERT INTO connections(id,owner_id,connector) VALUES($1,$2,'tdcc') RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(a.id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        sqlx::query("INSERT INTO sync_jobs(id,owner_id,connection_id,status,message) VALUES($1,$2,$3,'queued','isolation fixture')")
            .bind(job_id)
            .bind(a.id)
            .bind(connection)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(
        call(&app, &b, "GET", "/sync/jobs", Value::Null).await.1,
        json!([])
    );
    assert_eq!(
        call(
            &app,
            &b,
            "POST",
            &format!("/sync/jobs/{job_id}/cancel"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let request = Request::builder()
        .uri("/api/v1/sync/events")
        .header("cookie", format!("money_session={}", b.session))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut data = response.into_body().into_data_stream();
    use futures_util::StreamExt;
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), data.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = String::from_utf8_lossy(&chunk);
    assert!(text.contains("data: []"));
    drop(data);
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            &format!("/sync/jobs/{job_id}/cancel"),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    // 單一工作的 log 與事件串流：只有擁有者看得到，取消會留下錯誤碼與事件
    assert_eq!(
        call(&app, &b, "GET", &format!("/sync/jobs/{job_id}/log"), Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    let request = Request::builder()
        .uri(format!("/api/v1/sync/jobs/{job_id}/events"))
        .header("cookie", format!("money_session={}", b.session))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::NOT_FOUND);
    let (status, log) = call(&app, &a, "GET", &format!("/sync/jobs/{job_id}/log"), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(log["job"]["error_code"], "cancelled");
    assert!(log["events"].as_array().unwrap().iter().any(|e| e["code"] == "cancelled"));
    let (_,investment)=call(&app,&a,"POST","/accounts",json!({"name":"測試投資現金","kind":"investment","currency":"TWD","opening_balance":"1000"})).await;
    let (_,expense)=call(&app,&a,"POST","/transactions",json!({"account_id":id,"amount":"-100","kind":"expense","occurred_on":today,"description":"退款測試","category":"購物"})).await;
    let expense_id = expense["id"].as_str().unwrap();
    let refund_path = format!("/transactions/{expense_id}/refund");
    let refund = json!({"request_id":Uuid::new_v4(),"amount":"20","occurred_on":today});
    assert_eq!(
        call(&app, &b, "POST", &refund_path, refund.clone()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, &a, "POST", &refund_path, refund.clone()).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, &a, "POST", &refund_path, refund).await.1["duplicate"],
        true
    );
    assert_eq!(
        call(
            &app,
            &a,
            "POST",
            &refund_path,
            json!({"request_id":Uuid::new_v4(),"amount":"90","occurred_on":today})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, dashboard) = call(&app, &a, "GET", "/dashboard", Value::Null).await;
    assert_eq!(dashboard["monthly"][0]["expense"], "90.30000000");
    assert_eq!(dashboard["monthly"][0]["income"], "100.00000000");
    assert_eq!(
        call(
            &app,
            &a,
            "DELETE",
            &format!("/transactions/{expense_id}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, balances) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    assert_eq!(balances[0]["balance"], "889.70000000");
    let inv = investment["id"].as_str().unwrap();
    let opening_id = Uuid::new_v4();
    let mut order = json!({"request_id":opening_id,"account_id":inv,"market":"OTHER","symbol":"TEST","name":"測試持倉","kind":"opening","quantity":"200","unit_price":"10","fee":"0","tax":"0","occurred_on":today});
    assert_eq!(
        call(&app, &b, "POST", "/investments/trades", order.clone())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, opened) = call(&app, &a, "POST", "/investments/trades", order.clone()).await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    assert_eq!(
        call(&app, &a, "POST", "/investments/trades", order.clone())
            .await
            .1["duplicate"],
        true
    );
    let position_id = opened["position_id"].as_str().unwrap();
    let (_, all) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    assert!(
        all.as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == inv)
            .unwrap()["balance"]
            .is_null()
    );
    let quote = json!({"price":"12","date":today});
    assert_eq!(
        call(
            &app,
            &b,
            "POST",
            &format!("/investments/positions/{position_id}/quote"),
            quote.clone()
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
            &format!("/investments/positions/{position_id}/quote"),
            quote
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let sell_id = Uuid::new_v4();
    order["request_id"] = json!(sell_id);
    order["kind"] = json!("sell");
    order["quantity"] = json!("10");
    order["unit_price"] = json!("15");
    order["fee"] = json!("1");
    assert_eq!(
        call(&app, &a, "POST", "/investments/trades", order.clone())
            .await
            .0,
        StatusCode::OK
    );
    let (_, inv_trades) = call(&app, &a, "GET", "/investments/trades", Value::Null).await;
    assert_eq!(inv_trades[0]["realized_gain"], "49.00000000");
    order["request_id"] = json!(Uuid::new_v4());
    order["kind"] = json!("buy");
    order["unit_price"] = json!("12");
    assert_eq!(
        call(&app, &a, "POST", "/investments/trades", order.clone())
            .await
            .0,
        StatusCode::OK
    );
    let (_, all) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    let account = all
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == inv)
        .unwrap();
    assert_eq!(account["cash_balance"], "1028.00000000");
    assert_eq!(account["balance"], "3428.00000000");
    let dividend_id = Uuid::new_v4();
    order["request_id"] = json!(dividend_id);
    order["kind"] = json!("dividend");
    order["quantity"] = json!("0");
    order["fee"] = json!("0");
    order["tax"] = json!("10");
    order["dividend_amount"] = json!("100");
    assert_eq!(
        call(&app, &a, "POST", "/investments/trades", order.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            &a,
            "DELETE",
            &format!("/investments/trades/{sell_id}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, ledger) = call(&app, &a, "GET", "/transactions", Value::Null).await;
    let dividend_entry = ledger
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["investment_trade_id"] == dividend_id.to_string())
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_eq!(
        call(
            &app,
            &a,
            "DELETE",
            &format!("/transactions/{dividend_entry}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            &b,
            "DELETE",
            &format!("/investments/trades/{dividend_id}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &a,
            "DELETE",
            &format!("/investments/trades/{dividend_id}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, all) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    assert_eq!(
        all.as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == inv)
            .unwrap()["balance"],
        "3428.00000000"
    );
    assert_eq!(
        call(&app, &b, "GET", "/investments/positions", Value::Null)
            .await
            .1,
        json!([])
    );
    assert_eq!(
        call(&app, &b, "GET", "/investments/trades", Value::Null)
            .await
            .1,
        json!([])
    );
    clean(&db, &auth, &a).await;
    clean(&db, &auth, &b).await;
}
