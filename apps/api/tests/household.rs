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
async fn call_as(
    app: &Router,
    a: &Actor,
    view: Option<Uuid>,
    method: &str,
    path: &str,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(format!("/api/v1{path}"))
        .header("cookie", format!("money_session={}", a.session))
        .header("origin", "http://localhost:3000")
        .header("x-csrf-token", &a.csrf)
        .header("content-type", "application/json");
    if let Some(v) = view {
        req = req.header("x-view-owner", v.to_string());
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 4 * 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
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
#[ignore = "Requires local initialized PostgreSQL; run scripts/Test.ps1"]
async fn household_viewers_read_everything_and_merge() {
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
    let app = router(state);
    let a = actor(&auth).await;
    let b = actor(&auth).await;
    let c = actor(&auth).await;
    let today = chrono::Utc::now()
        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
        .date_naive()
        .to_string();
    let mut accts = Vec::new();
    for (who, opening, spend) in [(&a, "1000", "-100"), (&b, "500", "-40")] {
        let (s, acct) = call(
            &app,
            who,
            "POST",
            "/accounts",
            json!({"name":"家用","kind":"bank","currency":"TWD","opening_balance":opening}),
        )
        .await;
        assert_eq!(s, StatusCode::CREATED, "{acct}");
        let (s, v) = call(&app, who, "POST", "/transactions", json!({"account_id":acct["id"],"amount":spend,"kind":"expense","occurred_on":today,"description":"晚餐","category":"餐飲"})).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        accts.push(acct["id"].clone());
    }
    // a 轉 300 給 b：各自帳本照記，家庭合併時視為家庭內互轉
    for (who, acct, amount, kind, desc) in [
        (&a, &accts[0], "-300", "expense", "轉帳給家人"),
        (&b, &accts[1], "300", "income", "跨行轉入"),
    ] {
        let (s, v) = call(&app, who, "POST", "/transactions", json!({"account_id":acct,"amount":amount,"kind":kind,"occurred_on":today,"description":desc,"category":"其他"})).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
    }
    let (s, v) = call(
        &app,
        &a,
        "POST",
        "/budgets",
        json!({"month":&today[..7],"category":"餐飲","amount":"500","currency":"TWD"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let soon = (chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").unwrap()
        + chrono::Duration::days(2))
    .to_string();
    let (s, v) = call(&app, &a, "POST", "/recurring", json!({"account_id":accts[0],"amount":"-1200","kind":"expense","description":"房租","category":"居住","frequency":"monthly","next_on":soon})).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // 單一帳戶走勢：最後一天等於目前餘額（1000 − 100 − 轉出 300）；別人的帳戶查不到
    let (_, mine) = call(&app, &a, "GET", "/accounts", Value::Null).await;
    let acct_a = mine[0]["id"].as_str().unwrap().to_owned();
    let (s, h) = call(
        &app,
        &a,
        "GET",
        &format!("/asset-history/manual/{acct_a}"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{h}");
    assert_eq!(
        h["history"].as_array().unwrap().last().unwrap()["value"],
        600
    );
    let (s, _) = call(
        &app,
        &b,
        "GET",
        &format!("/asset-history/manual/{acct_a}"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &app,
        &a,
        "GET",
        &format!("/asset-history/other/{acct_a}"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // 未授權：不能以他人身分讀取
    let (s, _) = call_as(&app, &b, Some(a.id), "GET", "/accounts").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = call(
        &app,
        &a,
        "POST",
        "/household/viewers",
        json!({"email": a.email}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, grant) = call(
        &app,
        &a,
        "POST",
        "/household/viewers",
        json!({"email": b.email.to_uppercase()}),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{grant}");
    let (s, _) = call(
        &app,
        &a,
        "POST",
        "/household/viewers",
        json!({"email": b.email}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // 授權後：b 可以唯讀看 a 的全部資料
    let (s, accts) = call_as(&app, &b, Some(a.id), "GET", "/accounts").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(accts[0]["balance"], "600.00000000");
    let (s, worth) = call_as(&app, &b, Some(a.id), "GET", "/net-worth").await;
    assert_eq!(s, StatusCode::OK, "{worth}");
    assert_eq!(worth["net"], "600.00000000");
    // 帶檢視標頭的寫入一律拒絕
    let (s, _) = call_as(&app, &b, Some(a.id), "POST", "/accounts").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // c 沒被授權
    let (s, _) = call_as(&app, &c, Some(a.id), "GET", "/net-worth").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // a 沒被 b 授權，不能反過來看 b
    let (s, _) = call_as(&app, &a, Some(b.id), "GET", "/accounts").await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (_, home) = call(&app, &b, "GET", "/household", Value::Null).await;
    assert_eq!(home["shared_with_me"][0]["id"], json!(a.id));
    let (s, sum) = call(&app, &b, "GET", "/household/summary", Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{sum}");
    assert_eq!(sum["members"].as_array().unwrap().len(), 2);
    assert_eq!(sum["net"], "1360.00000000");
    let this_month = &today[..7];
    let m = sum["months"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["month"] == this_month)
        .unwrap();
    // 互轉 300 不算進家庭支出與收入
    assert_eq!(m["expense"], "140.00000000");
    assert_eq!(m["income"], "0");
    assert_eq!(sum["categories"][0]["category"], "餐飲");
    assert_eq!(sum["internal_transfers"]["count"], 1);
    assert_eq!(sum["internal_transfers"]["amount"], "300.00000000");
    assert_eq!(sum["budget"]["amount"], "500.00000000");
    assert_eq!(sum["budget"]["spent"], "140.00000000");
    // 成員本人帳本仍保留自己的收支
    let me_b = sum["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == json!(b.id))
        .unwrap();
    assert_eq!(me_b["income"], "300.00000000");
    // 近期付款：看得到 a 的固定支出，標為預計而非已付
    let (s, up) = call(&app, &b, "GET", "/household/upcoming?days=7", Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{up}");
    assert_eq!(up["items"][0]["title"], "房租");
    assert_eq!(up["items"][0]["status"], "scheduled");
    assert_eq!(up["week_due"], "1200.00000000");
    let (s, _) = call(&app, &b, "GET", "/household/upcoming?days=5", Value::Null).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &app,
        &b,
        "GET",
        &format!("/household/upcoming?members={}", c.id),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // 只選自己
    let (_, mine) = call(
        &app,
        &b,
        "GET",
        &format!("/household/summary?members={}", b.id),
        Value::Null,
    )
    .await;
    assert_eq!(mine["net"], "760.00000000");
    // 選沒授權的人
    let (s, _) = call(
        &app,
        &b,
        "GET",
        &format!("/household/summary?members={}", c.id),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // 支出追查：只剩兩筆晚餐（互轉已排除），並標示成員；趨勢只含 A 邊成員
    let first = format!("{this_month}-01");
    let (s, ex) = call(
        &app,
        &b,
        "GET",
        &format!(
            "/household/explore?a_from={first}&a_to={today}&b_from={first}&b_to={today}&a_members={}&b_members={}",
            b.id, a.id
        ),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{ex}");
    let entries = ex["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{ex}");
    assert!(entries.iter().all(|e| e["category"] == "餐飲"));
    let of_a = entries.iter().find(|e| e["member_id"] == json!(a.id)).unwrap();
    assert_eq!(of_a["amount"], "100.00000000");
    assert_eq!(ex["excluded"]["transfers"], 1);
    assert_eq!(ex["a"]["members"], json!([b.id]));
    let trend = ex["trend"].as_array().unwrap();
    assert_eq!(trend.len(), 1, "{ex}");
    assert_eq!(trend[0]["member_id"], json!(b.id));
    assert_eq!(trend[0]["amount"], "40.00000000");
    let (s, _) = call(
        &app,
        &b,
        "GET",
        &format!("/household/explore?a_from={first}&a_to={today}&b_from={first}&b_to={today}&b_members={}", c.id),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &app,
        &b,
        "GET",
        &format!("/household/explore?a_from={today}&a_to={first}0&b_from={first}&b_to={today}"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &app,
        &b,
        "GET",
        "/household/explore?a_from=2024-01-01&a_to=2025-06-01&b_from=2024-01-01&b_to=2024-01-31",
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (_, sum_a) = call(&app, &a, "GET", "/household/summary", Value::Null).await;
    assert_eq!(sum_a["members"].as_array().unwrap().len(), 1);

    // 檢視者可以自行退出；之後就不能再看
    let gid = home["shared_with_me"][0]["grant_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (s, _) = call(
        &app,
        &b,
        "DELETE",
        &format!("/household/viewers/{gid}"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = call_as(&app, &b, Some(a.id), "GET", "/accounts").await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    for who in [&a, &b, &c] {
        let mut tx = tenant(&db, who.id, &who.email).await.unwrap();
        for t in ["household_viewers", "investment_value_snapshots"] {
            sqlx::query(&format!("DELETE FROM {t} WHERE owner_id=$1"))
                .bind(who.id)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        clean(&db, &auth, who).await;
    }
}
