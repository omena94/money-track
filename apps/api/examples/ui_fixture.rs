//! Local-only UI test fixture. Not an HTTP endpoint; requires local DB credentials.
use money_api::{
    auth::{hash, random_token},
    state::tenant,
};
use sqlx::PgPool;
use uuid::Uuid;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_url = std::env::var("DATABASE_URL")?;
    let auth_url = std::env::var("AUTH_DATABASE_URL")?;
    anyhow::ensure!(
        url::Url::parse(&db_url)?.host_str() == Some("127.0.0.1"),
        "Fixtures only run on localhost"
    );
    let db = PgPool::connect(&db_url).await?;
    let auth = PgPool::connect(&auth_url).await?;
    if let Some(id) = std::env::args().nth(1) {
        let id = Uuid::parse_str(&id)?;
        let email: Option<String> = sqlx::query_scalar(
            "SELECT email FROM users WHERE id=$1 AND google_sub LIKE 'ui-fixture-%'",
        )
        .bind(id)
        .fetch_optional(&auth)
        .await?;
        let email = email.ok_or_else(|| anyhow::anyhow!("Not a UI fixture"))?;
        let mut tx = tenant(&db, id, &email)
            .await
            .map_err(|_| anyhow::anyhow!("tenant failed"))?;
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
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        sqlx::query("DELETE FROM users WHERE id=$1")
            .bind(id)
            .execute(&auth)
            .await?;
        return Ok(());
    }
    let id = Uuid::new_v4();
    let email = format!("ui-{id}@example.invalid");
    let session = random_token();
    let csrf = random_token();
    sqlx::query(
        "INSERT INTO users(id,google_sub,email,name,sync_approved) VALUES($1,$2,$3,'介面測試（非真實資料）',true)",
    )
    .bind(id)
    .bind(format!("ui-fixture-{id}"))
    .bind(&email)
    .execute(&auth)
    .await?;
    sqlx::query("INSERT INTO sessions(token_hash,user_id,csrf_token,expires_at) VALUES($1,$2,$3,now()+interval '10 minutes')").bind(hash(&session)).bind(id).bind(&csrf).execute(&auth).await?;
    if std::env::var("MONEY_UI_SOURCES").as_deref() == Ok("1") {
        use serde_json::json;
        let mut tx = tenant(&db, id, &email)
            .await
            .map_err(|_| anyhow::anyhow!("tenant failed"))?;
        for (source, rows) in [
            (
                "esun",
                vec![
                    (
                        "account",
                        "a",
                        None,
                        json!({"name":"測試活存 ***0123","institution":"玉山銀行（測試）","balance":null,"currency":"TWD","as_of":"2026-09-24T00:00:00Z"}),
                    ),
                    (
                        "transaction",
                        "b",
                        Some("a"),
                        json!({"amount":"-120","currency":"TWD","date":"2026-09-23","description":"測試午餐","status":"posted"}),
                    ),
                    (
                        "bill",
                        "c",
                        Some("a"),
                        json!({"period":"2026-09","currency":"TWD","statement_amount":"1200","minimum_payment":"100","payment_due":"2026-10-05"}),
                    ),
                ],
            ),
            (
                "tdcc",
                vec![(
                    "holding",
                    "d",
                    None,
                    json!({"name":"測試股票","symbol":"2330","currency":"TWD","quantity":"100","market_value":"246000","cost_basis":null,"as_of":"2026-09-23"}),
                )],
            ),
            (
                "einvoice",
                vec![
                    (
                        "invoice",
                        "e",
                        None,
                        json!({"number":"AB12345678","seller":"測試商店","amount":"120","date":"2026-09-23","currency":"TWD"}),
                    ),
                    (
                        "invoice_item",
                        "f",
                        None,
                        json!({"invoice_key":"e".repeat(64),"line_number":1,"description":"測試品項","amount":"120","quantity":"1","currency":"TWD"}),
                    ),
                ],
            ),
        ] {
            let connection = Uuid::new_v4();
            sqlx::query("INSERT INTO connections(id,owner_id,connector) VALUES($1,$2,$3)")
                .bind(connection)
                .bind(id)
                .bind(source)
                .execute(&mut *tx)
                .await?;
            for (kind, key, account, mut data) in rows {
                if kind == "holding" && std::env::var("MONEY_UI_FUNDS").as_deref()==Ok("1") {
                    data["asset_type"]=json!("fund");data["name"]=json!("測試基金");
                    data["cost_basis"]=json!("200000");data["unit_price"]=json!("2460");
                    data["unrealized_gain"]=json!("46000");data["price_date"]=json!("2026-09-23");
                    data["accumulated_dividend"]=json!("1200");
                }
                sqlx::query("INSERT INTO source_records(id,owner_id,connection_id,kind,source_key,account_key,data) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(Uuid::new_v4()).bind(id).bind(connection).bind(kind).bind(key.repeat(64)).bind(account.map(|k|k.repeat(64))).bind(data).execute(&mut *tx).await?;
            }
            if source == "tdcc" {
                sqlx::query("INSERT INTO sync_jobs(id,owner_id,connection_id,status,message,lease_until,challenge) VALUES($1,$2,$3,'waiting_user','介面驗證（模擬工作，不連金融服務）',now()+interval '10 minutes',$4)")
                    .bind(Uuid::new_v4()).bind(id).bind(connection).bind(json!({"id":Uuid::new_v4(),"kind":"otp","message":"測試一次性驗證碼介面"})).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
    }
    // Caller captures stdout in memory; never persist or log this payload.
    println!("{}", serde_json::json!({"id":id,"session":session}));
    Ok(())
}
