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
#[ignore = "Requires local initialized PostgreSQL; run scripts/Test.ps1"]
async fn synced_transactions_are_auto_categorized() {
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
    // 新使用者都能查看真正參與分類的預設規則。
    let (s, defaults) = call(&app, &a, "GET", "/category-rules/defaults", Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        defaults
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["pattern"] == "全聯" && r["category"] == "購物" && r["kind"] == "expense")
    );
    let (_, other_defaults) = call(&app, &b, "GET", "/category-rules/defaults", Value::Null).await;
    assert_eq!(defaults, other_defaults);
    let key = |n: u8| format!("{:064x}", n);
    let conn = Uuid::new_v4();
    let mut ids = Vec::new();
    {
        let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
        sqlx::query("INSERT INTO connections(id,owner_id,connector) VALUES($1,$2,'cathaybk')")
            .bind(conn)
            .bind(a.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO source_records(id,owner_id,connection_id,kind,source_key,data) VALUES($1,$2,$3,'account',$4,$5)")
            .bind(Uuid::new_v4()).bind(a.id).bind(conn).bind(key(1))
            .bind(json!({"name":"活存","currency":"TWD","account_type":"savings","balance":"1000"}))
            .execute(&mut *tx).await.unwrap();
        for (n, desc, amount) in [
            (2u8, "eTag 臨時停車 _BXD-7179", "-30"),
            (3, "電支交易 街口儲值            P***3604", "-20"),
            (4, "ＡＴＭ存 0130VHGP", "5000"),
            (5, "三沅行 早午餐", "-120"),
        ] {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO source_records(id,owner_id,connection_id,kind,source_key,account_key,data) VALUES($1,$2,$3,'transaction',$4,$5,$6)")
                .bind(id).bind(a.id).bind(conn).bind(key(n)).bind(key(1))
                .bind(json!({"description":desc,"amount":amount,"currency":"TWD","date":"2026-09-20"}))
                .execute(&mut *tx).await.unwrap();
            ids.push(id);
        }
        tx.commit().await.unwrap();
    }
    let feed = |v: &Value, id: Uuid| {
        v.as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == json!(id))
            .cloned()
            .unwrap()
    };
    let (s, list) = call(&app, &a, "GET", "/activity-feed", Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{list}");
    assert_eq!(feed(&list, ids[0])["category"], "交通");
    assert_eq!(feed(&list, ids[0])["category_source"], "auto");
    assert_eq!(feed(&list, ids[1])["category"], "電子支付");
    assert_eq!(feed(&list, ids[2])["category"], "轉帳", "全形 ATM 也要認得");
    assert_eq!(feed(&list, ids[3])["category"], "餐飲");

    // 逐筆修改 + 建立規則：之後含「街口儲值」的都歸到「零用」
    let (s, v) = call(
        &app,
        &a,
        "PATCH",
        &format!("/source-transactions/{}", ids[1]),
        json!({"category":"零用","note":"備註","tags":["家用"],"rule":"街口 儲值"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["category"], "零用");
    let (_, list) = call(&app, &a, "GET", "/activity-feed", Value::Null).await;
    let e = feed(&list, ids[1]);
    assert_eq!(e["category"], "零用");
    assert_eq!(e["category_source"], "user");
    assert_eq!(e["tags"], json!(["家用"]));
    let (_, rules) = call(&app, &a, "GET", "/category-rules", Value::Null).await;
    assert_eq!(rules[0]["pattern"], "街口儲值");
    let rule_id = rules[0]["id"].as_str().unwrap().to_owned();
    // 規則優先於內建關鍵字
    let (s, v) = call(
        &app,
        &a,
        "POST",
        "/category-rules",
        json!({"pattern":"臨時停車","category":"汽車"}),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    // 儲存規則不再重算所有歷史交易，須明確指定期間。
    let (_, before) = call(&app, &a, "GET", "/activity-feed", Value::Null).await;
    assert_eq!(feed(&before, ids[0])["category"], "交通");
    let (s, v) = call(
        &app,
        &a,
        "POST",
        "/category-rules/reclassify",
        json!({"scope":"dates","start":"2026-09-20","end":"2026-09-20"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["processed"], 3);
    assert_eq!(v["protected"], 1);
    let (_, list) = call(&app, &a, "GET", "/activity-feed", Value::Null).await;
    assert_eq!(feed(&list, ids[0])["category"], "汽車");
    assert_eq!(feed(&list, ids[0])["category_source"], "rule");
    // 新同步進來的交易，寫入當下就套用規則
    {
        let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO source_records(id,owner_id,connection_id,kind,source_key,account_key,data) VALUES($1,$2,$3,'transaction',$4,$5,$6)")
            .bind(id).bind(a.id).bind(conn).bind(key(9)).bind(key(1))
            .bind(json!({"description":"eTag 臨時停車 _ABC-1234","amount":"-40","currency":"TWD","date":"2026-09-21"}))
            .execute(&mut *tx).await.unwrap();
        let c: Option<String> =
            sqlx::query_scalar("SELECT auto_category FROM source_records WHERE id=$1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(c.as_deref(), Some("汽車"));
        tx.commit().await.unwrap();
    }
    // 過短的關鍵字、別人的交易與規則都拒絕
    let (s, _) = call(
        &app,
        &a,
        "POST",
        "/category-rules",
        json!({"pattern":" 車 ","category":"x"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &app,
        &b,
        "PATCH",
        &format!("/source-transactions/{}", ids[0]),
        json!({"category":"x"}),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // 日期端點、月份範圍、非現行資料與跨擁有者隔離。
    let mut boundary_ids = Vec::new();
    {
        let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
        for (n, date, current) in [
            (10u8, "2026-08-31", true),
            (11, "2026-09-01", true),
            (12, "2026-09-30", true),
            (13, "2026-10-01", true),
            (14, "2026-09-20", false),
        ] {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO source_records(id,owner_id,connection_id,kind,source_key,account_key,data,current) VALUES($1,$2,$3,'transaction',$4,$5,$6,$7)")
                .bind(id).bind(a.id).bind(conn).bind(key(n)).bind(key(1))
                .bind(json!({"description":"全聯","amount":"-30","currency":"TWD","date":date})).bind(current)
                .execute(&mut *tx).await.unwrap();
            boundary_ids.push(id);
        }
        // 僅改快取分類以模擬過期分類；不觸發 auto_source/data 的重算。
        sqlx::query("UPDATE source_records SET auto_category='待重算' WHERE owner_id=$1 AND kind='transaction'")
            .bind(a.id).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let (s, v) = call(
        &app,
        &b,
        "POST",
        "/category-rules/reclassify",
        json!({"scope":"all"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["processed"], 0);
    {
        let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
        let unchanged: bool = sqlx::query_scalar("SELECT bool_and(auto_category='待重算') FROM source_records WHERE owner_id=$1 AND kind='transaction'")
            .bind(a.id).fetch_one(&mut *tx).await.unwrap();
        assert!(unchanged, "另一個使用者重算不得影響本人的資料");
    }
    let (s, _) = call(
        &app,
        &a,
        "POST",
        "/category-rules/reclassify",
        json!({"scope":"dates","start":"2026-09-21","end":"2026-09-20"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = call(
        &app,
        &a,
        "POST",
        "/category-rules/reclassify",
        json!({"scope":"months","start":"2026-09","end":"2026-09"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["processed"], 6);
    assert_eq!(v["protected"], 1);
    {
        let mut tx = tenant(&db, a.id, &a.email).await.unwrap();
        for (index, id) in boundary_ids.iter().enumerate() {
            let category: String =
                sqlx::query_scalar("SELECT auto_category FROM source_records WHERE id=$1")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
            assert_eq!(
                category,
                if index == 1 || index == 2 {
                    "購物"
                } else {
                    "待重算"
                }
            );
        }
    }
    let (_, list) = call(&app, &a, "GET", "/activity-feed", Value::Null).await;
    assert_eq!(feed(&list, ids[1])["category"], "零用");
    assert_eq!(feed(&list, ids[1])["note"], "備註");
    assert_eq!(feed(&list, ids[1])["tags"], json!(["家用"]));
    let (s, v) = call(
        &app,
        &a,
        "POST",
        "/category-rules/reclassify",
        json!({"scope":"dates","start":"2027-01-01","end":"2027-01-31"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["processed"], 0);
    let (s, _) = call(
        &app,
        &b,
        "DELETE",
        &format!("/category-rules/{rule_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // 還原逐筆修改、刪除規則
    let (s, _) = call(
        &app,
        &a,
        "DELETE",
        &format!("/source-transactions/{}/label", ids[1]),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = call(
        &app,
        &a,
        "DELETE",
        &format!("/category-rules/{rule_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, v) = call(
        &app,
        &a,
        "POST",
        "/category-rules/reclassify",
        json!({"scope":"all"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["processed"], 9);
    assert_eq!(v["protected"], 0);
    let (_, list) = call(&app, &a, "GET", "/activity-feed", Value::Null).await;
    assert_eq!(feed(&list, ids[1])["category"], "電子支付");

    for who in [&a, &b] {
        let mut tx = tenant(&db, who.id, &who.email).await.unwrap();
        for t in [
            "source_tx_labels",
            "category_rules",
            "source_records",
            "connections",
        ] {
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
