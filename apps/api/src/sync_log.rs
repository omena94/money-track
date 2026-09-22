//! 同步事件紀錄：每個工作的逐行 log，供網頁即時顯示與事後診斷。
//! 顯示文字一律由後端產生；collector 提供的摘要只在遮罩、截斷後存入 detail。
use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::{
    convert::Infallible,
    sync::atomic::{AtomicI64, Ordering},
    time::Duration,
};
use uuid::Uuid;

/// 每個工作最多保存的事件數，避免異常迴圈塞滿資料表。
const MAX_EVENTS: i64 = 500;
const RETENTION_DAYS: i32 = 30;
const TERMINAL: [&str; 4] = ["succeeded", "partial", "failed", "cancelled"];

pub const JOB_JSON: &str = "jsonb_build_object('id',j.id,'status',j.status,'message',j.message,'created_at',j.created_at,'updated_at',j.updated_at,'connector',c.connector,'connection_id',j.connection_id,'challenge',j.challenge,'answer_pending',j.answer_cipher IS NOT NULL,'summary',j.summary,'progress',j.progress,'error_code',j.error_code,'failed_step',j.failed_step,'attempts',j.attempts)";

/// 遮罩 Email、身分證、網址路徑與查詢、長 token 及 4 位以上數字，並截斷為 200 字。
/// collector 已做過同樣處理；後端再做一次，避免單點疏漏。
pub fn redact(input: &str) -> String {
    let words: Vec<String> = input.split_whitespace().map(redact_word).collect();
    let joined = words.join(" ");
    let masked = mask_digits(&joined);
    masked.chars().take(200).collect()
}
fn redact_word(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    for scheme in ["https://", "http://"] {
        if let Some(pos) = lower.find(scheme) {
            let rest = &word[pos + scheme.len()..];
            let host: String = rest
                .chars()
                .take_while(|c| !matches!(c, '/' | '?' | '#'))
                .collect();
            return format!("{}{}", &word[..pos], host);
        }
    }
    if let Some(at) = word.find('@') {
        let (user, domain) = (&word[..at], &word[at + 1..]);
        if !user.is_empty() && domain.contains('.') {
            return "[email]".into();
        }
    }
    // 長 token：連續 24 個以上的 base64／URL 安全字元
    let mut out = String::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.chars().count() >= 24 {
            out.push_str("[token]");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in word.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-') {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}
fn mask_digits(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // 身分證：英文字母 + 1/2 + 8 位數字
        if chars[i].is_ascii_alphabetic()
            && i + 10 <= chars.len()
            && matches!(chars[i + 1], '1' | '2')
            && chars[i + 2..i + 10].iter().all(|c| c.is_ascii_digit())
            && (i == 0 || !chars[i - 1].is_ascii_alphanumeric())
        {
            out.push_str("[ID]");
            i += 10;
            continue;
        }
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i - start >= 4 {
                out.push_str("****");
            } else {
                out.extend(&chars[start..i]);
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// collector `log` 訊息白名單：代碼 → 顯示文字。
pub fn collector_message(code: &str, count: Option<u64>) -> Option<String> {
    let n = count.unwrap_or(0);
    Some(match code {
        "collector_warning" => "採集程式回報警告".into(),
        "gmail_waiting" => "等待 Gmail 中符合本次交易代碼的驗證信".into(),
        "gmail_validation" => "開始驗收 Gmail 自動取碼，重新建立銀行登入".into(),
        "gmail_received" => "已取得本次 Gmail 驗證碼，接續銀行驗證".into(),
        "detail_errors" => format!("有 {n} 項明細讀取失敗，結果可能不完整"),
        "warnings_truncated" => format!("共 {n} 則警告，只保留前 20 則"),
        _ => return None,
    })
}

/// 在既有交易中寫入一筆事件（需已設定租戶）。超過上限時靜默略過。
#[allow(clippy::too_many_arguments)]
pub async fn record_in(
    conn: &mut PgConnection,
    owner: Uuid,
    job: Uuid,
    level: &str,
    step: Option<&str>,
    code: &str,
    message: &str,
    detail: Option<&str>,
) -> Result<()> {
    let message: String = message.chars().take(300).collect();
    let detail = detail.map(redact).filter(|d| !d.is_empty());
    sqlx::query("INSERT INTO sync_job_events(job_id,owner_id,level,step,code,message,detail) SELECT $1,$2,$3,$4,$5,$6,$7 WHERE (SELECT count(*) FROM sync_job_events WHERE job_id=$1)<$8")
        .bind(job)
        .bind(owner)
        .bind(level)
        .bind(step)
        .bind(code)
        .bind(message)
        .bind(detail)
        .bind(MAX_EVENTS)
        .execute(conn)
        .await?;
    Ok(())
}

/// 自行開交易寫入事件；log 失敗不影響同步本身。
#[allow(clippy::too_many_arguments)]
pub async fn record(
    s: &AppState,
    owner: Uuid,
    email: &str,
    job: Uuid,
    level: &str,
    step: Option<&str>,
    code: &str,
    message: &str,
    detail: Option<&str>,
) {
    let result = async {
        let mut tx = tenant(&s.db, owner, email).await?;
        record_in(&mut tx, owner, job, level, step, code, message, detail).await?;
        tx.commit().await?;
        Ok::<_, ApiError>(())
    }
    .await;
    if result.is_err() {
        tracing::warn!(job=%job, code, "sync event not recorded");
    }
}

/// 同一工作同一代碼只記一次（例如反覆保存登入狀態）。
pub async fn record_once_in(
    conn: &mut PgConnection,
    owner: Uuid,
    job: Uuid,
    level: &str,
    code: &str,
    message: &str,
) -> Result<()> {
    let seen: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sync_job_events WHERE job_id=$1 AND code=$2)",
    )
    .bind(job)
    .bind(code)
    .fetch_one(&mut *conn)
    .await?;
    if !seen {
        record_in(conn, owner, job, level, None, code, message, None).await?;
    }
    Ok(())
}

static LAST_PRUNE: AtomicI64 = AtomicI64::new(0);
/// 清除過期事件；呼叫端以 `prune_due`／`mark_pruned` 控制每小時最多一次。
pub async fn prune(s: &AppState, owner: Uuid, email: &str) -> Result<()> {
    let mut tx = tenant(&s.db, owner, email).await?;
    sqlx::query("DELETE FROM sync_job_events WHERE owner_id=$1 AND created_at<now()-make_interval(days=>$2)")
        .bind(owner)
        .bind(RETENTION_DAYS)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
pub fn mark_pruned() {
    LAST_PRUNE.store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
}
pub fn prune_due() -> bool {
    chrono::Utc::now().timestamp() - LAST_PRUNE.load(Ordering::Relaxed) >= 3600
}

async fn load(s: &AppState, user: &Identity, job: Uuid, after: i64) -> Result<(Value, Vec<Value>)> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let snapshot: Option<Value> = sqlx::query_scalar(&format!(
        "SELECT {JOB_JSON} FROM sync_jobs j JOIN connections c ON c.id=j.connection_id WHERE j.id=$1 AND j.owner_id=$2"
    ))
    .bind(job)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let snapshot = snapshot.ok_or_else(ApiError::missing)?;
    let events: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'level',level,'step',step,'code',code,'message',message,'detail',detail,'at',created_at) FROM sync_job_events WHERE job_id=$1 AND owner_id=$2 AND id>$3 ORDER BY id LIMIT 200")
        .bind(job)
        .bind(user.id)
        .bind(after)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((snapshot, events))
}

#[derive(Deserialize)]
pub struct After {
    after: Option<i64>,
}
/// 輪詢備援及歷史重播：`GET /api/v1/sync/jobs/{id}/log?after=N`
pub async fn log(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<After>,
) -> Result<Json<Value>> {
    let (job, events) = load(&s, &user, id, q.after.unwrap_or(0).max(0)).await?;
    Ok(Json(json!({"job":job,"events":events})))
}

struct Tail {
    s: AppState,
    user: Identity,
    token_hash: String,
    job: Uuid,
    after: i64,
    last: Option<Value>,
    tick: usize,
    done: bool,
}
/// 單一工作的即時事件：先重播歷史，之後每秒推送新事件；工作結束送出 `done` 後關閉。
/// 事件 id 即資料表 id，瀏覽器重連時以 Last-Event-ID 接續。
pub async fn events(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<After>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>> {
    // 先確認工作屬於本人，否則直接 404，不開串流
    load(&s, &user, id, i64::MAX).await?;
    let after = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
        .max(q.after.unwrap_or(0))
        .max(0);
    let token = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .find_map(|p| p.trim().strip_prefix("money_session="))
        .unwrap_or("")
        .to_string();
    let state = Tail {
        s,
        user,
        token_hash: crate::auth::hash(&token),
        job: id,
        after,
        last: None,
        tick: 0,
        done: false,
    };
    let stream = futures_util::stream::unfold(state, |mut t| async move {
        if t.done || t.tick >= 1800 {
            return None;
        }
        if t.tick > 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        t.tick += 1;
        if t.tick % 5 == 1 {
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE token_hash=$1 AND user_id=$2 AND expires_at>now())").bind(&t.token_hash).bind(t.user.id).fetch_one(&t.s.auth_db).await.unwrap_or(false);
            if !valid {
                return None;
            }
        }
        let mut out: Vec<std::result::Result<Event, Infallible>> = vec![];
        match load(&t.s, &t.user, t.job, t.after).await {
            Ok((job, events)) => {
                let more = events.len() == 200;
                for e in events {
                    let id = e["id"].as_i64().unwrap_or(t.after);
                    t.after = t.after.max(id);
                    out.push(Ok(Event::default().event("log").id(id.to_string()).data(e.to_string())));
                }
                if t.last.as_ref() != Some(&job) {
                    out.push(Ok(Event::default().event("job").data(job.to_string())));
                }
                let finished = TERMINAL.contains(&job["status"].as_str().unwrap_or(""));
                t.last = Some(job);
                if finished && !more {
                    out.push(Ok(Event::default().event("done").data("{}")));
                    t.done = true;
                }
            }
            Err(_) => out.push(Ok(Event::default().event("retry").data("{}"))),
        }
        Some((futures_util::stream::iter(out), t))
    });
    use futures_util::StreamExt;
    Ok(Sse::new(stream.flatten()).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_identifiers_and_secrets() {
        let out = redact("Login a.b@example.com A123456789 acct 0123456789012 at https://bank.example.com/login?token=abc sid=abcdefghijklmnopqrstuvwxyz0123");
        for leaked in ["a.b@", "A123456789", "0123456789012", "/login", "token=abc", "abcdefghijklmnop"] {
            assert!(!out.contains(leaked), "{leaked} leaked in {out}");
        }
        assert!(out.contains("bank.example.com"));
        assert!(out.contains("[email]") && out.contains("[ID]") && out.contains("****"));
    }
    #[test]
    fn keeps_short_numbers_and_truncates() {
        assert_eq!(redact("HTTP 503  retry 2/3"), "HTTP 503 retry 2/3");
        assert_eq!(redact(&"ab ".repeat(200)).chars().count(), 200);
        assert_eq!(redact("卡號 4111111111111111"), "卡號 ****");
    }
    #[test]
    fn collector_codes_are_whitelisted() {
        assert!(collector_message("detail_errors", Some(3)).unwrap().contains('3'));
        assert!(collector_message("anything_else", None).is_none());
    }
}
