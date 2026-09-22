//! 外幣兌台幣匯率：玉山銀行牌告為主、open.er-api 為備援，Redis 快取。
use crate::{auth::Identity, error::Result, state::AppState};
use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};

const FRESH_KEY: &str = "money:fx:v1";
const LAST_KEY: &str = "money:fx:v1:last";
const FRESH_SECS: u64 = 600;
/// 來源失敗時，舊值暫放回快取的時間，避免每次請求都打外部服務。
const RETRY_SECS: u64 = 60;
const ESUN: &str = "https://www.esunbank.com/api/client/ExchangeRate/LastRateInfo";
const ER_API: &str = "https://open.er-api.com/v6/latest/USD";

#[derive(Clone, Serialize, Deserialize)]
pub struct Rate {
    /// 銀行買入（你賣外幣換台幣）的每單位台幣；估值用這個。
    pub buy: Decimal,
    /// 銀行賣出（你用台幣買外幣）的每單位台幣。
    pub sell: Decimal,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Quotes {
    pub source: String,
    pub quoted_at: DateTime<Utc>,
    pub fetched_at: DateTime<Utc>,
    pub rates: BTreeMap<String, Rate>,
}

impl Quotes {
    pub fn to_twd(&self, currency: &str, amount: Decimal) -> Option<Decimal> {
        if currency == "TWD" {
            return Some(amount);
        }
        self.rates.get(currency).map(|r| (amount * r.buy).round_dp(0))
    }
}

/// 取得匯率；Redis 有新鮮值直接用，否則向來源更新，全部失敗時回傳最後一次成功值。
pub async fn quotes(s: &AppState) -> Option<Quotes> {
    let mut redis = tokio::time::timeout(
        Duration::from_secs(2),
        s.redis.get_multiplexed_async_connection(),
    )
    .await
    .ok()
    .and_then(|c| c.ok());
    if let Some(c) = redis.as_mut() {
        if let Some(q) = read(c, FRESH_KEY).await {
            return Some(q);
        }
    }
    match fetch(s).await {
        Some(q) => {
            if let Some(c) = redis.as_mut() {
                if let Ok(raw) = serde_json::to_string(&q) {
                    let _: redis::RedisResult<()> = c.set_ex(FRESH_KEY, &raw, FRESH_SECS).await;
                    let _: redis::RedisResult<()> = c.set(LAST_KEY, &raw).await;
                }
            }
            Some(q)
        }
        None => {
            tracing::warn!(kind = "fx", "All FX sources failed");
            let c = redis.as_mut()?;
            let last = read(c, LAST_KEY).await?;
            if let Ok(raw) = serde_json::to_string(&last) {
                let _: redis::RedisResult<()> = c.set_ex(FRESH_KEY, &raw, RETRY_SECS).await;
            }
            Some(last)
        }
    }
}

async fn read(c: &mut redis::aio::MultiplexedConnection, key: &str) -> Option<Quotes> {
    let raw: Option<String> = c.get(key).await.ok()?;
    serde_json::from_str(&raw?).ok()
}

async fn fetch(s: &AppState) -> Option<Quotes> {
    match fetch_esun(s).await {
        Some(q) => Some(q),
        None => fetch_er_api(s).await,
    }
}

async fn get_json(req: reqwest::RequestBuilder) -> Option<Value> {
    let body = req
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .await
        .ok()?;
    if body.len() > 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(&body).ok()
}

async fn fetch_esun(s: &AppState) -> Option<Quotes> {
    let v = get_json(
        s.http
            .post(ESUN)
            .header("Accept", "application/json")
            .json(&json!({"day":"","time":""})),
    )
    .await?;
    parse_esun(&v)
}

fn num(v: Option<&Value>) -> Option<Decimal> {
    let d = match v? {
        Value::Number(n) => n.to_string().parse().ok()?,
        Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    (d > Decimal::ZERO).then_some(d)
}

pub fn parse_esun(v: &Value) -> Option<Quotes> {
    let mut rates = BTreeMap::new();
    let mut quoted: Option<DateTime<Utc>> = None;
    for r in v.get("Rates")?.as_array()? {
        let Some(ccy) = r.get("CCY").and_then(Value::as_str) else {
            continue;
        };
        let Some(code) = ccy.strip_suffix("/TWD") else {
            continue;
        };
        if code.len() != 3 || !code.chars().all(|c| c.is_ascii_uppercase()) {
            continue;
        }
        let (Some(buy), Some(sell)) = (num(r.get("BBoardRate")), num(r.get("SBoardRate"))) else {
            continue;
        };
        if buy > sell {
            continue;
        }
        // "/Date(1790278180000)/"
        let ts = r
            .get("UpdateTime")
            .and_then(Value::as_str)
            .and_then(|t| t.trim_start_matches("/Date(").split(')').next())
            .and_then(|ms| ms.parse::<i64>().ok())
            .and_then(DateTime::from_timestamp_millis);
        if let Some(t) = ts {
            quoted = Some(quoted.map_or(t, |q| q.max(t)));
        }
        rates.insert(code.to_string(), Rate { buy, sell });
    }
    rates.contains_key("USD").then(|| Quotes {
        source: "玉山銀行牌告即期".into(),
        quoted_at: quoted.unwrap_or_else(Utc::now),
        fetched_at: Utc::now(),
        rates,
    })
}

async fn fetch_er_api(s: &AppState) -> Option<Quotes> {
    let v = get_json(s.http.get(ER_API).header("Accept", "application/json")).await?;
    parse_er_api(&v)
}

pub fn parse_er_api(v: &Value) -> Option<Quotes> {
    if v.get("result").and_then(Value::as_str) != Some("success") {
        return None;
    }
    let all = v.get("rates")?.as_object()?;
    let twd = num(all.get("TWD"))?;
    let mut rates = BTreeMap::new();
    for code in ["USD", "JPY", "HKD", "EUR", "CNY", "GBP", "AUD", "SGD"] {
        if let Some(per_usd) = num(all.get(code)) {
            let mid = (twd / per_usd).round_dp(6).normalize();
            rates.insert(code.to_string(), Rate { buy: mid, sell: mid });
        }
    }
    let quoted = v
        .get("time_last_update_unix")
        .and_then(Value::as_i64)
        .and_then(|t| DateTime::from_timestamp(t, 0))
        .unwrap_or_else(Utc::now);
    Some(Quotes {
        source: "ExchangeRate-API 每日中價".into(),
        quoted_at: quoted,
        fetched_at: Utc::now(),
        rates,
    })
}

/// 回應用的匯率摘要。
pub fn summary(q: &Quotes, currencies: &[&str]) -> Value {
    json!({
        "source": q.source,
        "quoted_at": q.quoted_at,
        "fetched_at": q.fetched_at,
        "rates": currencies.iter().filter_map(|c| q.rates.get(*c).map(|r| json!({
            "currency": c, "buy": r.buy.to_string(), "sell": r.sell.to_string(),
        }))).collect::<Vec<_>>(),
    })
}

/// GET /fx：目前外幣兌台幣匯率。
pub async fn rates(_user: Identity, State(s): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(match quotes(&s).await {
        Some(q) => {
            let all: Vec<&str> = q.rates.keys().map(String::as_str).collect();
            summary(&q, &all)
        }
        None => Value::Null,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn esun_board_rates() {
        let q = parse_esun(&json!({"Rates":[
            {"CCY":"USD/TWD","BBoardRate":31.75,"SBoardRate":31.85,"UpdateTime":"/Date(1790278180000)/"},
            {"CCY":"JPY/TWD","BBoardRate":0.1982,"SBoardRate":0.2022,"UpdateTime":"/Date(1790278180000)/"},
            {"CCY":"BAD","BBoardRate":1,"SBoardRate":1},
        ]}))
        .unwrap();
        assert_eq!(q.rates.len(), 2);
        assert_eq!(q.to_twd("USD", "497.37".parse().unwrap()).unwrap().to_string(), "15791");
        assert_eq!(q.quoted_at.timestamp(), 1790278180);
        assert!(q.to_twd("HKD", Decimal::ONE).is_none());
    }
    #[test]
    fn esun_without_usd_is_rejected() {
        assert!(parse_esun(&json!({"Rates":[]})).is_none());
    }
    #[test]
    fn er_api_mid_rates() {
        let q = parse_er_api(&json!({"result":"success","time_last_update_unix":1790208152,
            "rates":{"USD":1,"TWD":32,"JPY":160}}))
        .unwrap();
        assert_eq!(q.rates["USD"].buy.to_string(), "32");
        assert_eq!(q.rates["JPY"].buy.to_string(), "0.2");
    }
}
