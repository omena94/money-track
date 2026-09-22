use crate::{
    error::Result,
    source_ingest::{self, Batch},
    state::{AppState, tenant},
    vault,
};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use uuid::Uuid;

/// 同步階段白名單：collector 只能回報代碼，文字由後端決定。
pub fn progress_label(code: &str) -> Option<&'static str> {
    Some(match code {
        "connecting" => "啟動同步程序",
        "login" => "登入中",
        "captcha_refresh" => "正在更換圖形驗證碼並重新辨識",
        "replace_session" => "依銀行流程登出其他裝置並繼續登入",
        "logged_in" => "登入成功",
        "otp_send" => "傳送驗證碼",
        "otp_verify" => "確認驗證碼",
        "holdings" => "讀取持股",
        "bank_accounts" => "讀取銀行帳戶",
        "bank_transactions" => "讀取帳戶收支",
        "trend" => "讀取資產歷史",
        "trades" => "讀取投資交易",
        "deposits" => "讀取存款帳戶",
        "deposit_details" => "讀取存款明細",
        "cards" => "讀取信用卡",
        "bills" => "讀取信用卡帳單",
        "invoices" => "讀取電子發票清單",
        "invoice_items" => "讀取發票商品明細",
        "collecting" => "繼續讀取資料",
        "normalizing" => "整理資料",
        "waiting_user" => "等待本人驗證",
        "resumed" => "已收到驗證，繼續同步",
        "saving" => "寫入資料庫",
        _ => return None,
    })
}

/// 追加一個進度階段；同一階段連續回報時只更新計數。
async fn step(
    s: &AppState,
    owner: Uuid,
    email: &str,
    job: Uuid,
    code: &str,
    current: Option<u64>,
    total: Option<u64>,
) -> Result<()> {
    let Some(label) = progress_label(code) else {
        return Ok(());
    };
    let message = match (current, total) {
        (Some(c), Some(t)) => format!("{label}（{c}/{t}）"),
        _ => label.to_string(),
    };
    let entry = json!({"code": code, "label": label, "current": current, "total": total});
    let mut tx = tenant(&s.db, owner, email).await?;
    sqlx::query("UPDATE sync_jobs SET message=$1,progress=(CASE WHEN progress->-1->>'code'=$2 THEN progress-(jsonb_array_length(progress)-1) WHEN jsonb_array_length(progress)>=40 THEN progress-0 ELSE progress END)||jsonb_build_array($3::jsonb||jsonb_build_object('at',now())),updated_at=now() WHERE id=$4 AND owner_id=$5 AND status IN ('running','waiting_user')")
        .bind(&message)
        .bind(code)
        .bind(entry)
        .bind(job)
        .bind(owner)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn update(
    s: &AppState,
    owner: Uuid,
    email: &str,
    job: Uuid,
    status: &str,
    message: &str,
    challenge: Option<Value>,
) -> Result<()> {
    let mut tx = tenant(&s.db, owner, email).await?;
    sqlx::query("UPDATE sync_jobs SET status=$1,message=$2,challenge=$3,answer_cipher=NULL,updated_at=now(),lease_until=CASE WHEN $1 IN ('running','waiting_user') THEN now()+interval '90 seconds' ELSE NULL END WHERE id=$4 AND owner_id=$5 AND status IN ('running','waiting_user')").bind(status).bind(message).bind(challenge).bind(job).bind(owner).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
/// 標記失敗並記錄錯誤碼、失敗步驟與一筆 error 事件。
#[allow(clippy::too_many_arguments)]
async fn fail(
    s: &AppState,
    owner: Uuid,
    email: &str,
    job: Uuid,
    code: &str,
    step: Option<&str>,
    message: &str,
    detail: Option<&str>,
) -> Result<()> {
    let mut tx = tenant(&s.db, owner, email).await?;
    let r = sqlx::query("UPDATE sync_jobs SET status='failed',message=$1,error_code=$2,failed_step=$3,challenge=NULL,answer_cipher=NULL,lease_until=NULL,updated_at=now() WHERE id=$4 AND owner_id=$5 AND status IN ('running','waiting_user')").bind(message).bind(code).bind(step).bind(job).bind(owner).execute(&mut *tx).await?;
    if r.rows_affected() > 0 {
        crate::sync_log::record_in(&mut tx, owner, job, "error", step, code, message, detail).await?;
    }
    tx.commit().await?;
    Ok(())
}
/// 登入前先記一次失敗（登入成功時歸零）；連續失敗達使用者設定的上限就不再允許登入。
pub async fn reserve_login(s:&AppState,owner:Uuid,email:&str,connection:Uuid,revision:i32,job:Uuid)->Result<bool>{
    let mut tx=tenant(&s.db,owner,email).await?;
    let limit=crate::sync_settings::load(&mut tx,owner).await?.failure_limit;
    let reserved:Option<i16>=sqlx::query_scalar("UPDATE connections SET login_failures=login_failures+1 WHERE id=$1 AND owner_id=$2 AND revision=$3 AND enabled AND login_failures<$5 AND EXISTS(SELECT 1 FROM sync_jobs WHERE id=$4 AND owner_id=$2 AND connection_id=$1 AND source_revision=$3 AND status='running') RETURNING login_failures").bind(connection).bind(owner).bind(revision).bind(job).bind(limit).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    Ok(reserved.is_some())
}

/// Only the supervisor's outstanding reservation may be returned; historical failures remain.
pub async fn release_captcha_login(s:&AppState,owner:Uuid,email:&str,connection:Uuid,revision:i32,job:Uuid)->Result<()> {
    let mut tx=tenant(&s.db,owner,email).await?;
    sqlx::query("UPDATE connections SET login_failures=greatest(login_failures-1,0) WHERE id=$1 AND owner_id=$2 AND revision=$3 AND EXISTS(SELECT 1 FROM sync_jobs WHERE id=$4 AND owner_id=$2 AND connection_id=$1 AND source_revision=$3 AND status='running')").bind(connection).bind(owner).bind(revision).bind(job).execute(&mut *tx).await?;
    tx.commit().await?;Ok(())
}

pub async fn run(
    s: &AppState,
    owner: Uuid,
    email: &str,
    job: Uuid,
    connection: Uuid,
) -> Result<()> {
    let mut tx = tenant(&s.db, owner, email).await?;
    let (connector,credentials,state,revision):(String,String,Option<String>,i32)=sqlx::query_as("SELECT connector,credentials_cipher,state_cipher,revision FROM connections WHERE id=$1 AND owner_id=$2 AND enabled AND credentials_cipher IS NOT NULL").bind(connection).bind(owner).fetch_one(&mut *tx).await?;
    let guard = crate::sync_settings::load(&mut tx, owner).await?;
    tx.commit().await?;
    // 這次同步已嘗試登入幾次；達到單次上限就停止本次同步
    let mut attempts: i16 = 0;
    let mut pending_login = false;
    let mut captcha_returns: u8 = 0;
    let mut denied_by_attempts = false;
    let prior_state = if connector == "hsbc" { json!({}) } else {
        state.as_deref().map(|v|vault::open(owner,connection,"state",v)).transpose()?.unwrap_or(json!({}))
    };
    let gmail = if connector=="fubon" { crate::gmail::access(s,owner,email,connection).await? } else {None};
    let mut gmail_verified = false;
    let request = json!({"version":1,"command":"sync","owner_id":owner,"connection_id":connection,"connector":connector,"revision":revision,"credentials":vault::open(owner,connection,"credentials",&credentials)?,"state":prior_state,"gmail":gmail.as_ref().map(|g|json!({"accessToken":g.token,"email":g.email,"validate":g.needs_validation}))});
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let mut command = Command::new("node");
    command
        .arg(crate::sources::collector_worker_path())
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for key in [
        "DATABASE_URL",
        "AUTH_DATABASE_URL",
        "BACKUP_DATABASE_URL",
        "GOOGLE_CLIENT_SECRET",
        "CONFIG_ENCRYPTION_KEY",
        "CLOUDFLARE_API_TOKEN",
        "PGPASSWORD",
        "MONEY_COLLECTOR_TEST",
    ] {
        command.env_remove(key);
    }
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            let detail = e.kind().to_string();
            fail(
                s,
                owner,
                email,
                job,
                "worker_start_failed",
                None,
                "本機未安裝資料來源採集器（公開版不附採集器）",
                Some(&detail),
            )
            .await?;
            return Ok(());
        }
    };
    let mut input = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    if input
        .write_all((request.to_string() + "\n").as_bytes())
        .await
        .is_err()
    {
        fail(s, owner, email, job, "protocol_error", None, "同步程序通訊失敗", None).await?;
        return Ok(());
    }
    drop(request);
    let start = tokio::time::Instant::now();
    let mut completed = false;
    let mut failure_message = String::from("同步程序意外結束；請稍後重試，若持續發生請回報");
    let mut failure_code = String::from("worker_exited");
    let mut failure_detail: Option<String> = None;
    let mut current_step: Option<String> = None;
    macro_rules! log {
        ($level:expr, $code:expr, $message:expr) => {
            log!($level, $code, $message, None)
        };
        ($level:expr, $code:expr, $message:expr, $detail:expr) => {
            crate::sync_log::record(s, owner, email, job, $level, current_step.as_deref(), $code, $message, $detail).await
        };
    }
    log!("info", "worker_started", "同步程序已啟動");
    loop {
        tokio::select! {
            line=lines.next_line()=>{
                let Ok(Some(line))=line else{break};
                if line.len()>32*1024*1024{failure_code="protocol_error".into();failure_message="同步程序回傳的資料過大".into();break;}
                let Ok(value)=serde_json::from_str::<Value>(&line)else{failure_code="protocol_error".into();failure_message="同步程序回傳無法辨識的訊息".into();break};
                if value["version"]!=1{failure_code="protocol_error".into();failure_message="同步程序協定版本不符".into();break;}
                match value["type"].as_str(){
                    Some("progress")=>{
                        let code=value["code"].as_str().unwrap_or("");
                        if code=="logged_in" && matches!(connector.as_str(),"cathaybk"|"esun"|"sinopac"|"firstbank"|"fubon"|"richart"|"hncb"|"landbank"|"yuanta"|"einvoice"|"hsbc") {
                            pending_login=false;
                            let mut tx=tenant(&s.db,owner,email).await?;
                            sqlx::query("UPDATE connections SET login_failures=0 WHERE id=$1 AND owner_id=$2 AND revision=$3").bind(connection).bind(owner).bind(revision).execute(&mut *tx).await?;
                            tx.commit().await?;
                        }
                        let count=|k:&str|value[k].as_u64().filter(|n|*n<=100_000);
                        if let Some(label)=progress_label(code) && current_step.as_deref()!=Some(code) {
                            current_step=Some(code.to_string());
                            log!("info",code,label);
                        }
                        update(s,owner,email,job,"running",progress_label(code).unwrap_or("正在讀取來源資料"),None).await?;
                        step(s,owner,email,job,code,count("current"),count("total")).await?;
                    },
                    Some("login_attempt")=>{
                        if pending_login {break;}
                        if !matches!(connector.as_str(),"cathaybk"|"esun"|"sinopac"|"firstbank"|"fubon"|"richart"|"hncb"|"landbank"|"yuanta"|"einvoice"|"hsbc"){break;}
                        let allowed=if attempts>=guard.attempt_limit {
                            denied_by_attempts=true;
                            log!("warn","attempt_limit",&format!("本次同步已嘗試登入 {attempts} 次，達到單次上限"));
                            false
                        } else {
                            attempts+=1;
                            let ok=reserve_login(s,owner,email,connection,revision,job).await?;
                            if ok {log!("info","login_attempt",&format!("允許登入（本次第 {attempts} 次）"));}
                            else {log!("warn","login_blocked",&format!("連續登入失敗已達 {} 次，停止登入",guard.failure_limit));}
                            ok
                        };
                        input.write_all((json!({"command":"login_permit","allowed":allowed}).to_string()+"\n").as_bytes()).await.ok();
                        pending_login=allowed;
                    },
                    Some("captcha_rejected")=>{
                        if captcha_returns>=5 {failure_code="captcha_retry_exhausted".into();failure_message="圖形驗證碼連續未通過，已停止本次同步".into();break;}
                        captcha_returns+=1;
                        log!("warn","captcha_rejected",&format!("圖形驗證碼未通過，重新取圖（第 {captcha_returns} 次）"));
                        if pending_login {
                            release_captcha_login(s,owner,email,connection,revision,job).await?;
                            pending_login=false;
                            attempts=attempts.saturating_sub(1);
                        }
                    },
                    Some("verification_required")=>{
                        // The bank reached its validated second phase. A
                        // cancelled/timed-out OTP is not a password rejection.
                        if !matches!(connector.as_str(), "cathaybk" | "fubon") || !pending_login {failure_code="protocol_error".into();break;}
                        if connector == "fubon" && !gmail.as_ref().is_some_and(|g|g.automatic) {
                            crate::sources::require_manual_verification(s,owner,email,connection,revision,job).await?;
                            if gmail.is_some() {
                                log!("info","manual_only","此連線暫時維持手動，Gmail 驗收成功後依設定啟用排程");
                            } else {
                                log!("warn","manual_only","此富邦連線要求本人驗證，已關閉自動同步");
                            }
                        }
                        log!("info","verification_required","銀行要求兩步驟驗證");
                        release_captcha_login(s,owner,email,connection,revision,job).await?;
                        pending_login=false;
                    },
                    Some("session")=>{
                        if connector == "hsbc" { continue; }
                        if !matches!(connector.as_str(),"cathaybk"|"esun"|"sinopac"|"firstbank"|"fubon"|"richart"|"hncb"|"landbank"|"yuanta"|"einvoice") || value["state"].to_string().len()>256*1024 {break;}
                        let sealed=vault::seal(owner,connection,"state",&value["state"])?;
                        let mut tx=tenant(&s.db,owner,email).await?;
                        sqlx::query("UPDATE connections SET state_cipher=$1 WHERE id=$2 AND owner_id=$3 AND revision=$4 AND enabled AND EXISTS(SELECT 1 FROM sync_jobs WHERE id=$5 AND owner_id=$3 AND status='running')").bind(sealed).bind(connection).bind(owner).bind(revision).bind(job).execute(&mut *tx).await?;
                        tx.commit().await?;
                        if let Ok(mut tx)=tenant(&s.db,owner,email).await
                            && crate::sync_log::record_once_in(&mut tx,owner,job,"info","session_saved","已加密保存登入狀態，下次可沿用").await.is_ok() {tx.commit().await.ok();}
                    },
                    Some("waiting_user")=>{
                        let c=&value["challenge"];
                        let Some(challenge)=crate::source_challenge::sanitize(c) else {failure_code="protocol_error".into();failure_message="驗證要求格式錯誤".into();break;};
                        let kind=match challenge["kind"].as_str(){Some("otp")=>"驗證碼",Some("captcha")=>"圖形驗證碼",Some("choice")=>"驗證方式選擇",_=>"驗證"};
                        log!("warn","waiting_user",&format!("等待本人操作：{kind}（十分鐘內）"));
                        update(s,owner,email,job,"waiting_user","等待本人完成驗證（十分鐘內）",Some(challenge)).await?;
                        step(s,owner,email,job,"waiting_user",None,None).await?;
                    },
                    Some("gmail_fallback")=>{
                        if connector!="fubon" || gmail.is_none(){failure_code="protocol_error".into();break;}
                        crate::gmail::fallback(s,owner,email,connection,gmail.as_ref().map(|g|g.generation),"未取得本次 Gmail 驗證碼，暫停自動同步").await?;
                        log!("warn","gmail_fallback","Gmail 自動取碼未完成，已關閉排程並改回本人驗證");
                    },
                    Some("gmail_verified")=>{
                        if connector!="fubon" || gmail.is_none(){failure_code="protocol_error".into();break;}
                        gmail_verified=true;
                    },
                    Some("result")=>{
                        let rows=value["rows"].as_array().map_or(0,|r|r.len());
                        current_step=Some("saving".into());
                        log!("info","result_received",&format!("收到 {rows} 筆資料，開始檢查並寫入"));
                        if value["partial"]==true {log!("warn","partial","部分資料未完整讀取，會標示為部分完成");}
                        step(s,owner,email,job,"saving",None,None).await?;
                        match serde_json::from_value::<Batch>(value){
                            Ok(batch)=>match source_ingest::stage(s,owner,email,job,connection,&batch).await{
                                Ok(())=>{
                                    // The encrypted staged batch survives a crash before the atomic commit.
                                    if source_ingest::commit(s,owner,email,job).await.is_ok(){
                                        completed=true;
                                        log!("info","committed","資料已寫入");
                                        if gmail_verified && let Some(g)=gmail.as_ref() {
                                            match crate::gmail::verified(s,owner,email,connection,g.generation,revision,job).await {
                                                Ok(true)=>log!("info","gmail_auto_enabled","Gmail 自動驗證及資料寫入驗收成功，已依設定開啟排程"),
                                                Ok(false)=>log!("info","gmail_verified","Gmail 自動驗證已通過；依設定保留手動同步"),
                                                Err(_)=>log!("warn","gmail_setup_pending","資料已寫入；Gmail 排程設定尚待確認"),
                                            }
                                        }
                                    }
                                    else{
                                        failure_message="資料已暫存，等待下次恢復入庫".into();
                                        log!("warn","staged","資料已加密暫存，等待下次自動恢復入庫");
                                    }
                                },
                                Err(e)=>{
                                    let detail=source_ingest::explain(&batch).unwrap_or_else(||e.1.to_string());
                                    tracing::warn!(kind="source_validation",detail=%detail,"Source batch rejected");
                                    failure_message=format!("同步資料未通過檢查：{detail}");
                                    failure_code="validation_failed".into();
                                    failure_detail=Some(detail.to_string());
                                },
                            },
                            Err(_)=>{failure_message="同步結果格式錯誤".into();failure_code="protocol_error".into();},
                        }
                        break;
                    },
                    Some("failed")=>{
                        let code=value["code"].as_str().unwrap_or("");
                        failure_code=if matches!(code,"login_locked"|"captcha_uncertain"|"captcha_retry_exhausted"|"duplicate_login"|"verification_failed"|"login_failed"|"timeout"|"page_changed"|"empty"|"protocol_error") {code.into()} else {"source_failed".into()};
                        failure_detail=value["detail"].as_str().map(str::to_string);
                        if failure_code=="verification_failed" && let Some(g)=gmail.as_ref() {
                            crate::gmail::fallback(s,owner,email,connection,Some(g.generation),"銀行驗證未完成，已暫停自動同步").await?;
                        }
                        if code=="login_locked" {
                            if denied_by_attempts {failure_code="attempt_limit".into();}
                            failure_message=if denied_by_attempts {
                                format!("這次同步已嘗試登入 {attempts} 次仍未成功，已停止本次同步；下次排程會再試（連續失敗達 {} 次會暫停自動同步）",guard.failure_limit)
                            } else {
                                format!("已連續 {} 次登入未成功，已暫停自動同步；確認銀行帳號狀態後，更新登入設定或按「已修好，恢復同步」",guard.failure_limit)
                            };
                            break;
                        }
                        failure_message=match value["code"].as_str(){
                            Some("captcha_uncertain"|"captcha_retry_exhausted")=>"已自動更換多張圖形驗證碼，銀行驗證仍未完成；未計入帳密錯誤，下次同步會重新取圖",
                            Some("duplicate_login")=>"銀行提示已有登入狀態；請先登出銀行網站，稍後再同步",
                            Some("verification_failed")=>"登入或人工驗證未完成；請確認後重新同步",
                            Some("login_failed")=>"銀行登入失敗；請確認帳號密碼，或到銀行網站確認帳號狀態",
                            Some("timeout")=>"等待銀行網頁回應逾時；請稍後再同步",
                            Some("page_changed")=>"銀行網頁格式可能已變更，暫時無法讀取",
                            Some("empty")=>"已登入但沒有讀到任何帳戶資料",
                            _=>"來源連線或格式驗證失敗；請檢查登入狀態後重試",
                        }.into();
                        break;
                    },
                    Some("log")=>{
                        let code=value["code"].as_str().unwrap_or("");
                        let level=if value["level"]=="warn" {"warn"} else {"info"};
                        if let Some(message)=crate::sync_log::collector_message(code,value["count"].as_u64()) {
                            log!(level,code,&message,value["detail"].as_str());
                        }
                    },
                    _=>{failure_code="protocol_error".into();failure_message="同步程序回傳未知的訊息類型".into();break},
                }
            },
            _=tokio::time::sleep(Duration::from_secs(3))=>{
                let approved:bool=sqlx::query_scalar("SELECT is_admin OR sync_approved FROM users WHERE id=$1").bind(owner).fetch_one(&s.auth_db).await?;
                let mut tx=tenant(&s.db,owner,email).await?;
                let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sync_jobs j JOIN connections c ON c.id=j.connection_id WHERE j.id=$1 AND j.owner_id=$2 AND j.status IN ('running','waiting_user') AND c.enabled AND c.revision=$3)").bind(job).bind(owner).bind(revision).fetch_one(&mut *tx).await?;
                if !active||!approved||start.elapsed()>Duration::from_secs(1200){
                    tx.rollback().await?;
                    if !approved{update(s,owner,email,job,"cancelled","同步資格已撤銷",None).await?;}
                    if start.elapsed()>Duration::from_secs(1200) {
                        failure_code="timeout".into();
                        failure_message="同步超過 20 分鐘仍未完成，已中止並保留上次資料".into();
                    } else {
                        failure_code="cancelled".into();
                        failure_message="同步逾時或已取消，已保留上次資料".into();
                    }
                    break;
                }
                let answer:Option<String>=sqlx::query_scalar("SELECT answer_cipher FROM sync_jobs WHERE id=$1 AND owner_id=$2 FOR UPDATE").bind(job).bind(owner).fetch_one(&mut *tx).await?;
                sqlx::query("UPDATE sync_jobs SET lease_until=now()+interval '90 seconds',answer_cipher=NULL WHERE id=$1 AND owner_id=$2").bind(job).bind(owner).execute(&mut *tx).await?;
                tx.commit().await?;
                if let Some(answer)=answer{
                    let value=vault::open(owner,job,"answer",&answer)?;
                    input.write_all((value.to_string()+"\n").as_bytes()).await.ok();
                    update(s,owner,email,job,"running","已收到驗證操作，繼續同步",None).await?;
                    log!("info","resumed","已收到本人驗證，繼續同步");
                    step(s,owner,email,job,"resumed",None,None).await?;
                }
            }
        }
    }
    let _ = input.write_all(b"{\"command\":\"cancel\"}\n").await;
    drop(input);
    if tokio::time::timeout(Duration::from_secs(12), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
    if !completed {
        let mut tx = tenant(&s.db, owner, email).await?;
        let staged: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM source_batches WHERE job_id=$1 AND cipher IS NOT NULL)",
        )
        .bind(job)
        .fetch_one(&mut *tx)
        .await?;
        if !staged {
            fail(
                s,
                owner,
                email,
                job,
                &failure_code,
                current_step.as_deref(),
                &failure_message,
                failure_detail.as_deref(),
            )
            .await?;
        }
    }
    Ok(())
}
