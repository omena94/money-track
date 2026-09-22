use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::{Datelike, Duration, Months, NaiveDate};
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
        "subscription_links",
        "subscription_dismissals",
        "subscriptions",
        "source_records",
        "connections",
        "transactions",
        "accounts",
        "user_categories",
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
async fn subscriptions_match_synced_charges_without_double_counting() {
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
    let key = |n: u8| format!("{:064x}", n);
    let conn = Uuid::new_v4();
    // 近 4 個月每月 16 日的固定扣款，用來測疑似訂閱
    let today = (chrono::Utc::now() + Duration::hours(8)).date_naive();
    let this = today.with_day(1).unwrap();
    let recent: Vec<NaiveDate> = (1..=4)
        .map(|n| {
            this.checked_sub_months(Months::new(n))
                .unwrap()
                .with_day(16)
                .unwrap()
        })
        .collect();
    let mut ids = Vec::new();
    {
        let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
        sqlx::query("INSERT INTO connections(id,owner_id,connector) VALUES($1,$2,'fubon')")
            .bind(conn)
            .bind(a.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO source_records(id,owner_id,connection_id,kind,source_key,data) VALUES($1,$2,$3,'account',$4,$5)")
            .bind(Uuid::new_v4()).bind(a.id).bind(conn).bind(key(1))
            .bind(json!({"name":"信用卡","currency":"TWD","account_type":"credit_card"}))
            .execute(&mut *tx).await.unwrap();
        let mut rows = vec![
            (
                2u8,
                "CLAUDE.AI SUBSCRIPT125001",
                "-6412",
                "2026-07-09",
                Some("200"),
            ),
            (
                3,
                "ANTHROPIC* CLAUDE S125001",
                "-6451",
                "2026-08-09",
                Some("200"),
            ),
            (4, "ANTHROPIC* CLAUDE S125044", "-6000", "2026-08-11", None),
            (
                5,
                "ANTHROPIC* CLAUDE S125001",
                "6451",
                "2026-08-12",
                Some("200"),
            ),
        ];
        let apple: Vec<String> = recent.iter().map(|d| d.to_string()).collect();
        for (n, date) in apple.iter().enumerate() {
            rows.push((
                20 + n as u8,
                "APPLE.COM/BILL 004619",
                "-90",
                date.as_str(),
                None,
            ));
        }
        for (n, desc, amount, date, usd) in rows {
            let id = Uuid::new_v4();
            let mut data = json!({"description":desc,"amount":amount,"currency":"TWD","date":date,"entry_type":if amount.starts_with('-') {"expense"} else {"refund"}});
            if let Some(u) = usd {
                data["original_amount"] = json!(u);
                data["original_currency"] = json!("USD");
            }
            sqlx::query("INSERT INTO source_records(id,owner_id,connection_id,kind,source_key,account_key,data) VALUES($1,$2,$3,'transaction',$4,$5,$6)")
                .bind(id).bind(a.id).bind(conn).bind(key(n)).bind(key(1)).bind(data)
                .execute(&mut *tx).await.unwrap();
            ids.push(id);
        }
        tx.commit().await.unwrap();
    }

    // 疑似訂閱：Apple 每月一筆會被找出來
    let (s, found) = call(&app, &a, "GET", "/subscriptions/suggestions", Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{found}");
    let apple = found
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["pattern"] == "apple.com/bill")
        .cloned()
        .expect("apple suggested");
    assert_eq!(apple["months"], 4);
    assert_eq!(apple["amount"], "90");

    let body = json!({"name":"Claude Max","category":"AI 工具","amount":"200","currency":"USD","cycle":"monthly",
        "anchor_day":9,"started_on":"2026-01-01","patterns":["ANTHROPIC* CLAUDE","claude.ai"]});
    let (s, v) = call(&app, &a, "POST", "/subscriptions", body.clone()).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let sub = v["id"].as_str().unwrap().to_owned();
    let (_, list) = call(&app, &a, "GET", "/subscriptions", Value::Null).await;
    assert_eq!(
        list[0]["patterns"],
        json!(["anthropic*claude", "claude.ai"]),
        "比對字串要正規化"
    );

    // 8 月：200 美元那筆自動配對，6000 元待確認，退款抵銷
    let (s, m) = call(
        &app,
        &a,
        "GET",
        "/subscriptions/month?month=2026-08",
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{m}");
    let p = &m["periods"][0];
    assert_eq!(p["status"], "paid");
    let states: Vec<&str> = p["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["match"].as_str().unwrap())
        .collect();
    assert_eq!(states, vec!["auto", "pending", "auto"]);
    assert!(p["flags"].as_array().unwrap().contains(&json!("refunded")));
    assert_eq!(p["spent"], json!([{"currency":"TWD","amount":"0"}]));
    // 訂閱不會新增交易：收支清單筆數不變
    let (_, feed) = call(&app, &a, "GET", "/activity-feed", Value::Null).await;
    assert_eq!(feed.as_array().unwrap().len(), ids.len());

    // 確認 6000 元屬於這期、排除退款
    let (s, v) = call(
        &app,
        &a,
        "PUT",
        &format!("/subscriptions/{sub}/links"),
        json!({"transaction_id":ids[2],"period":"2026-08","state":"confirmed"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    call(
        &app,
        &a,
        "PUT",
        &format!("/subscriptions/{sub}/links"),
        json!({"transaction_id":ids[3],"period":"2026-08","state":"rejected"}),
    )
    .await;
    let (_, m) = call(
        &app,
        &a,
        "GET",
        "/subscriptions/month?month=2026-08",
        Value::Null,
    )
    .await;
    let p = &m["periods"][0];
    assert_eq!(p["items"].as_array().unwrap().len(), 2, "{p}");
    assert_eq!(p["spent"], json!([{"currency":"TWD","amount":"6000"}]));
    // 排除的交易另外列出，才能在畫面上還原
    assert_eq!(p["excluded"].as_array().unwrap().len(), 1, "{p}");
    assert_eq!(p["excluded"][0]["id"], json!(ids[3]));
    // 收支清單用：列出已歸到訂閱的交易，排除的不算，別人看不到
    let (s, found) = call(&app, &a, "GET", "/subscriptions/matches", Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{found}");
    let found = found.as_array().unwrap();
    let hit = found
        .iter()
        .find(|f| f["transaction_id"] == json!(ids[2]))
        .expect("confirmed charge listed");
    assert_eq!(hit["name"], "Claude Max");
    assert_eq!(hit["month"], "2026-08");
    assert!(!found.iter().any(|f| f["transaction_id"] == json!(ids[3])));
    let (_, others) = call(&app, &b, "GET", "/subscriptions/matches", Value::Null).await;
    assert_eq!(others, json!([]));
    // 還原為自動
    let (s, _) = call(
        &app,
        &a,
        "DELETE",
        &format!("/subscriptions/{sub}/links/{}", ids[3]),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    // 取消後 9 月不再預期扣款；10 月之前的扣款日過了沒扣會顯示未扣款
    let mut ended = body.clone();
    ended["ended_on"] = json!("2026-08-31");
    let (s, v) = call(&app, &a, "PUT", &format!("/subscriptions/{sub}"), ended).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, m) = call(
        &app,
        &a,
        "GET",
        "/subscriptions/month?month=2026-09",
        Value::Null,
    )
    .await;
    assert_eq!(m["periods"], json!([]));

    // 驗證錯誤
    let mut bad = body.clone();
    bad["cycle"] = json!("yearly");
    assert_eq!(
        call(&app, &a, "POST", "/subscriptions", bad).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut bad = body.clone();
    bad["patterns"] = json!([" x "]);
    assert_eq!(
        call(&app, &a, "POST", "/subscriptions", bad).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            &a,
            "GET",
            "/subscriptions/month?month=2026-13",
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    // 自訂圖示：只收依檔頭確認的點陣圖，列表內嵌回傳，可移除
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let (s, v) = call(&app, &a, "PUT", &format!("/subscriptions/{sub}/icon"), json!({"data": png})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, list) = call(&app, &a, "GET", "/subscriptions", Value::Null).await;
    assert_eq!(list[0]["icon"], png);
    let svg = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciLz4=";
    let (s, _) = call(&app, &a, "PUT", &format!("/subscriptions/{sub}/icon"), json!({"data": svg})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(&app, &b, "PUT", &format!("/subscriptions/{sub}/icon"), json!({"data": png})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(&app, &b, "DELETE", &format!("/subscriptions/{sub}/icon"), Value::Null).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(&app, &a, "DELETE", &format!("/subscriptions/{sub}/icon"), Value::Null).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, &a, "GET", "/subscriptions", Value::Null).await;
    assert_eq!(list[0]["icon"], Value::Null);

    // 擁有者隔離：別人看不到、改不到，也不能把我的交易掛到他的訂閱
    let (_, other) = call(&app, &b, "GET", "/subscriptions", Value::Null).await;
    assert_eq!(other, json!([]));
    assert_eq!(
        call(
            &app,
            &b,
            "PUT",
            &format!("/subscriptions/{sub}"),
            body.clone()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &b,
            "DELETE",
            &format!("/subscriptions/{sub}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &b,
            "PUT",
            &format!("/subscriptions/{sub}/links"),
            json!({"transaction_id":ids[0],"period":"2026-07","state":"confirmed"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, bs) = call(&app, &b, "POST", "/subscriptions", body.clone()).await;
    let bsub = bs["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            &b,
            "PUT",
            &format!("/subscriptions/{bsub}/links"),
            json!({"transaction_id":ids[0],"period":"2026-07","state":"confirmed"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, bm) = call(
        &app,
        &b,
        "GET",
        "/subscriptions/month?month=2026-08",
        Value::Null,
    )
    .await;
    assert_eq!(bm["periods"][0]["status"], "missed", "{bm}");
    assert_eq!(bm["periods"][0]["items"], json!([]));

    // 不是訂閱就不再建議
    let (s, _) = call(
        &app,
        &a,
        "POST",
        "/subscriptions/suggestions/dismiss",
        json!({"key":"apple.com/bill"}),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, found) = call(&app, &a, "GET", "/subscriptions/suggestions", Value::Null).await;
    assert!(
        !found
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["pattern"] == "apple.com/bill")
    );
    // 按錯可以找回：列出已隱藏的商家，取消隱藏後重新建議；別人動不到
    let (_, hidden) = call(&app, &a, "GET", "/subscriptions/suggestions/dismissed", Value::Null).await;
    assert_eq!(hidden[0]["key"], "apple.com/bill", "{hidden}");
    let (_, others) = call(&app, &b, "GET", "/subscriptions/suggestions/dismissed", Value::Null).await;
    assert_eq!(others, json!([]));
    let (s, _) = call(&app, &b, "POST", "/subscriptions/suggestions/restore", json!({"key":"apple.com/bill"})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(&app, &a, "POST", "/subscriptions/suggestions/restore", json!({"key":"apple.com/bill"})).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, hidden) = call(&app, &a, "GET", "/subscriptions/suggestions/dismissed", Value::Null).await;
    assert_eq!(hidden, json!([]));

    // 刪除訂閱只刪配對決定，不動交易
    assert_eq!(
        call(
            &app,
            &a,
            "DELETE",
            &format!("/subscriptions/{sub}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, feed) = call(&app, &a, "GET", "/activity-feed", Value::Null).await;
    assert_eq!(feed.as_array().unwrap().len(), ids.len());

    clean(&db, &auth, &a).await;
    clean(&db, &auth, &b).await;
}
