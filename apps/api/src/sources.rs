use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::{AppState, tenant},
    vault,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};
use utoipa::ToSchema;
use uuid::Uuid;

/// 目前支援的 HTTP 資料來源；目錄、手動同步與排程共用此清單。
pub const IDS: [&str; 12] = ["tdcc", "cathaybk", "esun", "sinopac", "firstbank", "fubon", "richart", "einvoice", "hsbc", "hncb", "landbank", "yuanta"];
pub fn fields(id: &str) -> Option<&'static [&'static str]> {
    if !IDS.contains(&id) {
        return None;
    }
    match id {
        "hsbc" => Some(&["account"]),
        "tdcc" => Some(&["userId", "password"]),
        "einvoice" => Some(&["mobile", "password"]),
        "yuanta" | "landbank" | "richart" | "fubon" | "esun" | "cathaybk" | "sinopac" | "hncb" | "firstbank" => {
            Some(&["userId", "account", "password"])
        }
        _ => None,
    }
}
/// 公開版不附銀行／集保／電子發票採集器。採集器需另行實作並建置到此路徑，
/// 依 docs/financial-sync.md 的 stdin/stdout JSON 協定與 API 溝通。
pub fn collector_worker_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../collector/protocol/dist/worker.mjs")
}
/// 只有在本機安裝採集器後，介面才列出可連線的資料來源。
pub fn collector_installed() -> bool {
    collector_worker_path().is_file()
}
/// 對使用者公開的來源目錄：未安裝採集器時為空，避免顯示無法同步的來源。
pub fn public_catalog() -> Value {
    if collector_installed() { catalog_data() } else { json!([]) }
}
pub fn catalog_data() -> Value {
    let name = |id: &str| match id {
        "esun" => "玉山銀行",
        "cathaybk" => "國泰世華",
        "sinopac" => "永豐銀行",
        "hncb" => "華南銀行",
        "landbank" => "土地銀行",
        "yuanta" => "元大銀行",
        "firstbank" => "第一銀行",
        "fubon" => "台北富邦銀行",
        "hsbc" => "滙豐銀行",
        "richart" => "Richart",
        "tdcc" => "集保 e 存摺",
        "einvoice" => "電子發票",
        _ => "資料來源",
    };
    json!(IDS.iter().map(|id|{
        let name=name(id);
        let caps=match *id{"yuanta"=>vec!["臺幣存款","六個月交易","信用卡","六期帳單","繳款期限"],"hsbc"=>vec!["存款餘額","180 天交易","房貸","還款期數"],"richart"=>vec!["臺幣活存","12 個月交易","逐筆餘額","獨立帳戶"],"fubon"=>vec!["存款餘額","一年交易","信用卡","六期帳單","繳款期限"],"firstbank"=>vec!["臺幣活存","定存","兩年交易","信用卡","帳單","繳款期限"],"tdcc"=>vec!["持倉","投資交易","交割帳戶"],"einvoice"=>vec!["發票","品項"],"sinopac"=>vec!["臺外幣活存","一年交易"],"esun"=>vec!["信用卡","帳單","逐筆消費","繳款期限"],"hncb"|"landbank"=>vec!["臺幣存款","兩年交易","逐筆餘額"],_=>vec!["存款","交易","信用卡","帳單"]};
        json!({"id":id,"name":name,"mode":"api","manual_only":*id=="hsbc","capabilities":caps,"credential_fields":fields(id),"implementation":"verified_capabilities","parser_verified":true,"live_verified":true})
    }).collect::<Vec<_>>())
}
#[utoipa::path(get,path="/api/v1/connections",responses((status=200,body=Vec<Value>)),tag="sources")]
pub async fn list(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'connector',connector,'configured',credentials_cipher IS NOT NULL,'enabled',enabled,'daily',daily,'manual_only',manual_only OR connector='hsbc','login_failures',login_failures,'next_run_at',next_run_at,'last_success_at',last_success_at,'updated_at',updated_at,'record_count',(SELECT count(*) FROM source_records r WHERE r.connection_id=c.id AND r.owner_id=c.owner_id),'counts',(SELECT coalesce(jsonb_object_agg(kind,n),'{}') FROM (SELECT kind,count(*) n FROM source_records r WHERE r.connection_id=c.id AND r.owner_id=c.owner_id AND r.current GROUP BY kind) k)) FROM connections c WHERE owner_id=$1 AND connector=ANY($2) ORDER BY connector").bind(user.id).bind(&IDS[..]).fetch_all(&mut *tx).await?;
    Ok(Json(rows))
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub credentials: Value,
}
#[utoipa::path(post,path="/api/v1/connectors/{id}/credentials",params(("id"=String,Path)),request_body=Credentials,responses((status=200,body=Value),(status=403)),tag="sources")]
pub async fn save_credentials(
    user: Identity,
    State(s): State<AppState>,
    Path(source): Path<String>,
    Json(v): Json<Credentials>,
) -> Result<Json<Value>> {
    if !user.is_admin && !user.sync_approved {
        return Err(ApiError::forbidden());
    }
    let allowed = fields(&source).ok_or_else(ApiError::missing)?;
    let values = v
        .credentials
        .as_object()
        .ok_or_else(|| ApiError::bad("請填寫登入欄位"))?;
    if values.len() != allowed.len()
        || values.keys().any(|k| !allowed.contains(&k.as_str()))
        || allowed.iter().any(|k| {
            values
                .get(*k)
                .and_then(Value::as_str)
                .is_none_or(|v| v.is_empty() || v.len() > 256)
        })
    {
        return Err(ApiError::bad("登入欄位不完整或格式錯誤"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let next = crate::sync_settings::next_run(&mut tx, user.id).await?;
    let id:Uuid=sqlx::query_scalar("INSERT INTO connections(id,owner_id,connector,daily,next_run_at) VALUES($1,$2,$3,$3<>'hsbc',CASE WHEN $3<>'hsbc' THEN $4 ELSE NULL END) ON CONFLICT(owner_id,connector) DO UPDATE SET updated_at=now() RETURNING id").bind(Uuid::new_v4()).bind(user.id).bind(&source).bind(next).fetch_one(&mut *tx).await?;
    let sealed = vault::seal(user.id, id, "credentials", &v.credentials)?;
    sqlx::query("UPDATE connections SET credentials_cipher=$1,state_cipher=NULL,login_failures=0,revision=revision+1,enabled=true,daily=CASE WHEN connector='hsbc' OR manual_only THEN false ELSE daily END,next_run_at=CASE WHEN connector='hsbc' OR manual_only THEN NULL ELSE next_run_at END,last_success_at=NULL,updated_at=now() WHERE id=$2 AND owner_id=$3").bind(sealed).bind(id).bind(user.id).execute(&mut *tx).await?;
    sqlx::query("UPDATE sync_jobs SET status='cancelled',message='登入設定已更新，請重新同步',challenge=NULL,answer_cipher=NULL,lease_until=NULL,updated_at=now() WHERE connection_id=$1 AND owner_id=$2 AND status IN ('queued','running','waiting_user')").bind(id).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id,"configured":true})))
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub daily: bool,
}
#[utoipa::path(patch,path="/api/v1/connections/{id}",params(("id"=Uuid,Path)),request_body=Settings,responses((status=200,body=Value),(status=404)),tag="sources")]
pub async fn settings(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Settings>,
) -> Result<Json<Value>> {
    if (v.enabled || v.daily) && !user.is_admin && !user.sync_approved {
        return Err(ApiError::forbidden());
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let source: Option<(String, bool)> = sqlx::query_as("SELECT connector,manual_only FROM connections WHERE id=$1 AND owner_id=$2 AND connector=ANY($3) FOR UPDATE")
        .bind(id).bind(user.id).bind(&IDS[..]).fetch_optional(&mut *tx).await?;
    let (source, manual_only) = source.ok_or_else(ApiError::missing)?;
    if manual_only && v.daily {
        return Err(ApiError::bad("此帳號曾要求本人驗證，已永久改為手動同步"));
    }
    if source == "hsbc" && v.daily {
        return Err(ApiError::bad("滙豐僅支援手動同步，每次需輸入新的網銀認證碼"));
    }
    let next = crate::sync_settings::next_run(&mut tx, user.id).await?;
    let r=sqlx::query("UPDATE connections SET enabled=$1,daily=$2,next_run_at=CASE WHEN $1 AND $2 THEN $5 ELSE NULL END,updated_at=now() WHERE id=$3 AND owner_id=$4 ").bind(v.enabled).bind(v.daily).bind(id).bind(user.id).bind(next).execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    if source == "fubon" {
        sqlx::query("UPDATE gmail_links SET auto_requested=$1,updated_at=now() WHERE connection_id=$2 AND owner_id=$3")
            .bind(v.enabled && v.daily).bind(id).bind(user.id).execute(&mut *tx).await?;
    }
    if !v.enabled {
        sqlx::query("UPDATE sync_jobs SET status='cancelled',message='來源已停用',challenge=NULL,answer_cipher=NULL,lease_until=NULL WHERE connection_id=$1 AND owner_id=$2 AND status IN ('queued','running','waiting_user')").bind(id).bind(user.id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[utoipa::path(delete,path="/api/v1/connections/{id}",params(("id"=Uuid,Path)),responses((status=200,body=Value),(status=404)),tag="sources")]
pub async fn forget(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r=sqlx::query("UPDATE connections SET enabled=false,daily=false,next_run_at=NULL,credentials_cipher=NULL,state_cipher=NULL,login_failures=0,revision=revision+1,updated_at=now() WHERE id=$1 AND owner_id=$2").bind(id).bind(user.id).execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    // The connection update locks out ingestion; delete all source-owned data in
    // the same transaction so a staged or late worker result cannot restore it.
    // Record labels/account links and balance history use ON DELETE CASCADE.
    for table in ["source_records", "source_batches", "sync_jobs"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE connection_id=$1 AND owner_id=$2"))
            .bind(id).bind(user.id).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM connections WHERE id=$1 AND owner_id=$2")
        .bind(id).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[utoipa::path(post,path="/api/v1/connections/{id}/sync",params(("id"=Uuid,Path)),responses((status=200,body=Value),(status=404),(status=429)),tag="sources")]
pub async fn enqueue(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    if !user.is_admin && !user.sync_approved {
        return Err(ApiError::forbidden());
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    // 測試階段不限制同步間隔；只避免同一來源同時有兩個同步工作。
    let config:Option<(bool,bool,i32,i16)>=sqlx::query_as("SELECT enabled AND credentials_cipher IS NOT NULL,EXISTS(SELECT 1 FROM sync_jobs j WHERE j.connection_id=connections.id AND j.status IN ('queued','running','waiting_user')),revision,login_failures FROM connections WHERE id=$1 AND owner_id=$2 AND connector=ANY($3) FOR UPDATE").bind(id).bind(user.id).bind(&IDS[..]).fetch_optional(&mut *tx).await?;
    let (ready, busy, revision, failures) = config.ok_or_else(ApiError::missing)?;
    if !ready {
        return Err(ApiError::bad("請先儲存登入設定並啟用來源"));
    }
    let limit = crate::sync_settings::load(&mut tx, user.id).await?.failure_limit;
    if failures >= limit { return Err(ApiError::bad("連續登入未成功已達上限，同步已暫停；請確認銀行帳號狀態，更新登入設定或按「已修好，恢復同步」")); }
    if busy {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "這個來源正在同步中，請等目前的同步完成",
        ));
    }
    let job = Uuid::new_v4();
    sqlx::query("INSERT INTO sync_jobs(id,owner_id,connection_id,status,message,source_revision) VALUES($1,$2,$3,'queued','等待金融資料同步',$4)").bind(job).bind(user.id).bind(id).bind(revision).execute(&mut *tx).await?;
    crate::sync_log::record_in(&mut tx,user.id,job,"info",None,"queued","已加入同步佇列（手動）",None).await?;
    sqlx::query("UPDATE connections SET last_requested_at=now() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":job})))
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    challenge_id: Uuid,
    #[serde(default)]
    value: String,
}
#[utoipa::path(post,path="/api/v1/sync/jobs/{id}/continue",params(("id"=Uuid,Path)),request_body=Answer,responses((status=200,body=Value),(status=404)),tag="sources")]
pub async fn resume(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Answer>,
) -> Result<Json<Value>> {
    if !user.is_admin && !user.sync_approved {
        return Err(ApiError::forbidden());
    }
    if v.value.len() > 32 {
        return Err(ApiError::bad("驗證碼過長"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let challenge:Option<Value>=sqlx::query_scalar("SELECT challenge FROM sync_jobs WHERE id=$1 AND owner_id=$2 AND status='waiting_user' AND challenge IS NOT NULL AND answer_cipher IS NULL AND lease_until>now() FOR UPDATE").bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    let c = challenge.ok_or_else(ApiError::missing)?;
    if c["id"] != v.challenge_id.to_string() {
        return Err(ApiError::bad("驗證工作已更新，請重新整理"));
    }
    crate::source_challenge::validate_answer(&c, &v.value).map_err(ApiError::bad)?;
    let sealed = vault::seal(
        user.id,
        id,
        "answer",
        &json!({"command":"continue","value":v.value}),
    )?;
    sqlx::query(
        "UPDATE sync_jobs SET answer_cipher=$1,updated_at=now() WHERE id=$2 AND owner_id=$3",
    )
    .bind(sealed)
    .bind(id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
pub struct Filter {
    kind: Option<String>,
    offset: Option<i64>,
    q: Option<String>,
    invoice: Option<Uuid>,
}
#[utoipa::path(get,path="/api/v1/connections/{id}/records",params(("id"=Uuid,Path),("kind"=Option<String>,Query),("q"=Option<String>,Query),("offset"=Option<i64>,Query),("invoice"=Option<Uuid>,Query)),responses((status=200,body=Value),(status=404)),tag="sources")]
pub async fn records(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(f): Query<Filter>,
) -> Result<Json<Value>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM connections WHERE id=$1 AND owner_id=$2)")
            .bind(id)
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    if !exists {
        return Err(ApiError::missing());
    }
    let total:i64=sqlx::query_scalar("SELECT count(*) FROM source_records r WHERE connection_id=$1 AND owner_id=$2 AND ($3::text IS NULL OR kind=$3) AND ($4::text IS NULL OR data::text ILIKE '%'||$4||'%') AND (kind NOT IN ('invoice','invoice_item') OR current) AND ($5::uuid IS NULL OR (kind='invoice_item' AND data->>'invoice_key'=(SELECT i.source_key FROM source_records i WHERE i.id=$5 AND i.owner_id=$2 AND i.connection_id=$1 AND i.kind='invoice' AND i.current)))").bind(id).bind(user.id).bind(&f.kind).bind(&f.q).bind(f.invoice).fetch_one(&mut *tx).await?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'kind',kind,'data',data,'current',current,'observed_at',observed_at,'invoice_number',(SELECT i.data->>'number' FROM source_records i WHERE i.owner_id=$2 AND i.connection_id=r.connection_id AND i.kind='invoice' AND i.source_key=r.data->>'invoice_key'),'account_name',(SELECT a.data->>'name' FROM source_records a WHERE a.connection_id=r.connection_id AND a.kind='account' AND a.source_key=r.account_key)) FROM source_records r WHERE connection_id=$1 AND owner_id=$2 AND ($3::text IS NULL OR kind=$3) AND ($4::text IS NULL OR data::text ILIKE '%'||$4||'%') AND (kind NOT IN ('invoice','invoice_item') OR current) AND ($6::uuid IS NULL OR (kind='invoice_item' AND data->>'invoice_key'=(SELECT i.source_key FROM source_records i WHERE i.id=$6 AND i.owner_id=$2 AND i.connection_id=$1 AND i.kind='invoice' AND i.current))) ORDER BY current DESC,coalesce(data->>'date',data->>'as_of',data->>'period','') DESC,(data->>'line_number')::integer NULLS LAST,id LIMIT 100 OFFSET $5").bind(id).bind(user.id).bind(&f.kind).bind(&f.q).bind(f.offset.unwrap_or(0).max(0)).bind(f.invoice).fetch_all(&mut *tx).await?;
    let totals:Vec<Value>=if f.kind.as_deref()==Some("invoice") {
        sqlx::query_scalar("SELECT jsonb_build_object('currency',data->>'currency','amount',sum((data->>'amount')::numeric)::text,'count',count(*)) FROM source_records WHERE owner_id=$1 AND connection_id=$2 AND kind='invoice' AND current AND coalesce(data->>'status','active')<>'void' AND ($3::text IS NULL OR data::text ILIKE '%'||$3||'%') GROUP BY data->>'currency'")
            .bind(user.id).bind(id).bind(&f.q).fetch_all(&mut *tx).await?
    } else {vec![]};
    Ok(Json(json!({"total":total,"rows":rows,"invoice_totals":totals})))
}
/// This flag survives successful logins, credential updates and failure resets.
pub async fn require_manual_verification(s: &AppState, owner: Uuid, email: &str, connection: Uuid, revision: i32, job: Uuid) -> Result<()> {
    let mut tx = tenant(&s.db, owner, email).await?;
    sqlx::query("UPDATE connections SET manual_only=true,daily=false,next_run_at=NULL,updated_at=now() WHERE id=$1 AND owner_id=$2 AND connector='fubon' AND revision=$3 AND EXISTS(SELECT 1 FROM sync_jobs WHERE id=$4 AND connection_id=$1 AND owner_id=$2 AND source_revision=$3 AND status='running')")
        .bind(connection).bind(owner).bind(revision).bind(job).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn schedule_for(s: &AppState, owner: Uuid, email: &str) -> Result<()> {
    let mut tx = tenant(&s.db, owner, email).await?;
    // Upgrade legacy HSBC settings under the same owner/RLS boundary. Invalidating
    // old jobs also prevents recovery from resuming an automatic sync after rollout.
    sqlx::query("WITH changed AS (UPDATE connections SET daily=false,next_run_at=NULL,state_cipher=NULL,revision=revision+1,updated_at=now() WHERE owner_id=$1 AND connector='hsbc' AND (daily OR next_run_at IS NOT NULL OR state_cipher IS NOT NULL) RETURNING id) UPDATE sync_jobs SET status='cancelled',message='滙豐已改為手動同步，請重新同步並輸入新的認證碼',challenge=NULL,answer_cipher=NULL,lease_until=NULL,updated_at=now() WHERE owner_id=$1 AND connection_id IN (SELECT id FROM changed) AND status IN ('queued','running','waiting_user')")
        .bind(owner).execute(&mut *tx).await?;
    // 所有來源共用同步時間；連續登入失敗達上限的來源暫停自動同步，等使用者修好
    let settings = crate::sync_settings::load(&mut tx, owner).await?;
    let next = crate::sync_settings::next_slot(&settings.times, chrono::Utc::now());
    let due:Vec<(Uuid,i32)>=sqlx::query_as("SELECT id,revision FROM connections WHERE owner_id=$1 AND connector<>'hsbc' AND NOT manual_only AND connector=ANY($3) AND enabled AND daily AND login_failures<$2 AND credentials_cipher IS NOT NULL AND next_run_at<=now() AND NOT EXISTS(SELECT 1 FROM sync_jobs j WHERE j.connection_id=connections.id AND j.status IN ('queued','running','waiting_user')) ORDER BY next_run_at FOR UPDATE SKIP LOCKED").bind(owner).bind(settings.failure_limit).bind(&IDS[..]).fetch_all(&mut *tx).await?;
    for (id, revision) in due {
        let job = Uuid::new_v4();
        sqlx::query("INSERT INTO sync_jobs(id,owner_id,connection_id,status,message,source_revision) VALUES($1,$2,$3,'queued','每日金融資料同步',$4)").bind(job).bind(owner).bind(id).bind(revision).execute(&mut *tx).await?;
        crate::sync_log::record_in(&mut tx,owner,job,"info",None,"queued","已加入同步佇列（每日排程）",None).await?;
        sqlx::query("UPDATE connections SET next_run_at=$2,last_requested_at=now() WHERE id=$1").bind(id).bind(next).execute(&mut *tx).await?;
    }
    // 暫停中的來源也把下次時間往後推，恢復後才不會立刻補跑一堆
    sqlx::query("UPDATE connections SET next_run_at=$3 WHERE owner_id=$1 AND connector<>'hsbc' AND NOT manual_only AND connector=ANY($4) AND enabled AND daily AND login_failures>=$2 AND next_run_at<=now()").bind(owner).bind(settings.failure_limit).bind(next).bind(&IDS[..]).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
