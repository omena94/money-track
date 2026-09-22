//! 訂閱：預期扣款清單與實際交易配對。
//!
//! 訂閱本身不產生帳務，金額一律以同步或手動交易為準，因此不會與刷卡明細重複計算。
//! 讀取時依「比對字串 → 最近的扣款日 → 金額」把交易歸到某個訂閱的某一期；
//! 使用者的確認或排除存在 `subscription_links`，優先於自動配對。
use crate::{
    auth::Identity,
    error::{ApiError, Result},
    finance::{FEED_SQL, NOT_FLOW, decimal},
    fx,
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{Datelike, Duration, Months, NaiveDate};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::str::FromStr;
use uuid::Uuid;

#[derive(sqlx::FromRow, Clone)]
pub struct Sub {
    pub id: Uuid,
    pub name: String,
    pub category: String,
    pub amount: Decimal,
    pub currency: String,
    pub cycle: String,
    pub anchor_day: i32,
    pub anchor_month: Option<i32>,
    pub started_on: NaiveDate,
    pub ended_on: Option<NaiveDate>,
    pub paused: bool,
    pub patterns: Vec<String>,
    pub tolerance: Decimal,
    pub window_days: i32,
    pub require_amount: bool,
    pub note: String,
    /// 自訂圖示（data URL）；沒有上傳為 None
    pub icon: Option<String>,
}

/// 收支清單中的一筆交易（手動或同步），`text` 為 `category_text(description)`。
#[derive(sqlx::FromRow, Clone)]
pub struct Tx {
    pub id: Uuid,
    pub date: NaiveDate,
    pub amount: Decimal,
    pub currency: Option<String>,
    pub original_amount: Option<String>,
    pub original_currency: Option<String>,
    pub description: Option<String>,
    pub account_name: Option<String>,
    pub status: String,
    pub manual: bool,
    pub text: String,
}

#[derive(sqlx::FromRow, Clone)]
pub struct Link {
    pub subscription_id: Uuid,
    pub tx_id: Uuid,
    pub period: String,
    pub state: String,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    Charge,
    Fee,
    Refund,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Match {
    Auto,
    Confirmed,
    Pending,
}

pub struct Item<'a> {
    pub tx: &'a Tx,
    /// 服務費所屬的扣款
    pub parent: Option<Uuid>,
    pub role: Role,
    pub state: Match,
    pub amount_ok: Option<bool>,
}

pub struct Period<'a> {
    pub sub: &'a Sub,
    pub month: String,
    pub due: Option<NaiveDate>,
    pub status: &'static str,
    pub items: Vec<Item<'a>>,
    pub flags: Vec<&'static str>,
}

fn last_day(y: i32, m: u32) -> u32 {
    let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    (first.checked_add_months(Months::new(1)).unwrap() - Duration::days(1)).day()
}
fn on(y: i32, m: u32, day: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, (day as u32).min(last_day(y, m))).unwrap()
}
fn month_key(d: NaiveDate) -> String {
    d.format("%Y-%m").to_string()
}
pub fn parse_month(s: &str) -> Result<NaiveDate> {
    let d = NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d")
        .map_err(|_| ApiError::bad("月份格式須為 YYYY-MM"))?;
    if s.len() != 7 || month_key(d) != s {
        return Err(ApiError::bad("月份格式須為 YYYY-MM"));
    }
    Ok(d)
}

/// 這個月的預期扣款日；年繳只在扣款月份有。
pub fn due_in(s: &Sub, month: NaiveDate) -> Option<NaiveDate> {
    match s.anchor_month {
        Some(am) if s.cycle == "yearly" => {
            (month.month() == am as u32).then(|| on(month.year(), am as u32, s.anchor_day))
        }
        _ => Some(on(month.year(), month.month(), s.anchor_day)),
    }
}

/// 交易歸屬的一期：離交易日最近的扣款日（同距離取較早的一期）。
pub fn nearest_due(s: &Sub, date: NaiveDate) -> NaiveDate {
    let candidates: Vec<NaiveDate> = match s.anchor_month {
        Some(am) if s.cycle == "yearly" => (date.year() - 1..=date.year() + 1)
            .map(|y| on(y, am as u32, s.anchor_day))
            .collect(),
        _ => {
            let first = date.with_day(1).unwrap();
            [
                first.checked_sub_months(Months::new(1)).unwrap(),
                first,
                first.checked_add_months(Months::new(1)).unwrap(),
            ]
            .iter()
            .map(|m| on(m.year(), m.month(), s.anchor_day))
            .collect()
        }
    };
    let mut best = candidates[0];
    for c in candidates {
        if (c - date).num_days().abs() < (best - date).num_days().abs() {
            best = c;
        }
    }
    best
}

fn expected(s: &Sub, due: NaiveDate) -> bool {
    !s.paused && due >= s.started_on && s.ended_on.is_none_or(|e| due <= e)
}

fn is_fee(text: &str) -> bool {
    text.contains("手續費") || text.contains("服務費")
}

fn role(t: &Tx) -> Role {
    if is_fee(&t.text) {
        Role::Fee
    } else if t.amount > Decimal::ZERO {
        Role::Refund
    } else {
        Role::Charge
    }
}

/// 以訂閱幣別比較金額：外幣刷卡優先用原幣金額。無法比較時回傳 None。
pub fn amount_ok(s: &Sub, t: &Tx) -> Option<bool> {
    let original = t
        .original_amount
        .as_deref()
        .and_then(|v| Decimal::from_str(v).ok())
        .map(|v| v.abs())
        .filter(|v| !v.is_zero());
    let value = if t.original_currency.as_deref() == Some(&s.currency) && original.is_some() {
        original?
    } else if t.currency.as_deref() == Some(&s.currency) {
        t.amount.abs()
    } else {
        return None;
    };
    let allowed = s.amount * s.tolerance / Decimal::from(100);
    Some((value - s.amount).abs() <= allowed)
}

/// 外幣服務費另列一行時指向哪筆扣款：富邦寫「簽帳 6,336」，國泰寫「-CLAUDE」。
pub fn fee_ref(text: &str) -> Option<(Option<Decimal>, Option<&str>)> {
    if !is_fee(text) {
        return None;
    }
    let amount = text.split_once("簽帳").and_then(|(_, rest)| {
        let digits: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == ',')
            .filter(|c| *c != ',')
            .collect();
        Decimal::from_str(&digits).ok()
    });
    let merchant = text
        .rsplit_once('-')
        .map(|(_, m)| m)
        .filter(|m| m.chars().filter(|c| c.is_alphanumeric()).count() >= 3);
    (amount.is_some() || merchant.is_some()).then_some((amount, merchant))
}

/// (交易, 期別, 使用者確認, 服務費所屬的扣款)
type Assigned<'a> = (&'a Tx, String, bool, Option<Uuid>);

/// 每筆候選交易屬於哪個訂閱：確認過的優先；否則由最長的比對字串決定，排除過的不算。
/// 比對不到的外幣服務費，跟著它所指的扣款歸到同一個訂閱、同一期。
fn assign<'a>(subs: &'a [Sub], txs: &'a [Tx], links: &[Link]) -> HashMap<Uuid, Vec<Assigned<'a>>> {
    let mut out: HashMap<Uuid, Vec<Assigned>> = HashMap::new();
    let rejected = |t: &Tx, s: &Sub| {
        links
            .iter()
            .any(|l| l.tx_id == t.id && l.subscription_id == s.id && l.state == "rejected")
    };
    let mut fees = Vec::new();
    for t in txs {
        if let Some(l) = links
            .iter()
            .find(|l| l.tx_id == t.id && l.state == "confirmed")
        {
            if subs.iter().any(|s| s.id == l.subscription_id) {
                out.entry(l.subscription_id)
                    .or_default()
                    .push((t, l.period.clone(), true, None));
            }
            continue;
        }
        let found: Vec<(usize, &Sub)> = subs
            .iter()
            .filter(|s| !rejected(t, s))
            .filter_map(|s| {
                let len = s
                    .patterns
                    .iter()
                    .filter(|p| t.text.contains(p.as_str()))
                    .map(|p| p.chars().count())
                    .max()?;
                Some((len, s))
            })
            .collect();
        // 比對字串一樣長時，金額相符的優先（同一代收商底下的多個訂閱依金額分流），
        // 金額也相同就看扣款日哪個最近（例如每月 3 日、12 日各一筆 100 元）
        let longest = found.iter().map(|(len, _)| *len).max();
        let tied: Vec<&Sub> = found
            .iter()
            .filter(|(len, _)| Some(*len) == longest)
            .map(|(_, s)| *s)
            .collect();
        let best = tied
            .iter()
            .copied()
            .filter(|s| amount_ok(s, t) == Some(true))
            .min_by_key(|s| (nearest_due(s, t.date) - t.date).num_days().abs())
            .or_else(|| match tied.as_slice() {
                [only] => Some(*only),
                // 多個訂閱共用比對字串、且金額都不符：歸給不限金額的訂閱，都限金額就不屬於任何一個
                _ => tied.iter().copied().find(|s| !s.require_amount),
            })
            .filter(|_| role(t) != Role::Fee || tied.len() == 1);
        if let Some(s) = best {
            let period = month_key(nearest_due(s, t.date));
            out.entry(s.id).or_default().push((t, period, false, None));
        } else if let Some(r) = fee_ref(&t.text) {
            fees.push((t, r));
        }
    }
    for (f, (amount, merchant)) in fees {
        let parent = out
            .iter()
            .flat_map(|(sub, list)| list.iter().map(move |e| (*sub, e)))
            .filter(|(_, (c, _, _, parent))| parent.is_none() && !is_fee(&c.text))
            .filter(|(_, (c, ..))| (-3..=10).contains(&(f.date - c.date).num_days()))
            .filter(|(_, (c, ..))| {
                amount.is_some_and(|a| c.amount.abs() == a)
                    || merchant.is_some_and(|m| c.text.contains(m))
            })
            .filter(|(sub, _)| subs.iter().any(|s| s.id == *sub && !rejected(f, s)))
            .min_by_key(|(_, (c, ..))| (f.date - c.date).num_days().abs())
            .map(|(sub, (c, period, ..))| (sub, period.clone(), c.id));
        if let Some((sub, period, id)) = parent {
            out.entry(sub)
                .or_default()
                .push((f, period, false, Some(id)));
        }
    }
    out
}

/// 逐訂閱、逐月份評估：已扣款、待確認、即將扣款、未扣款。
pub fn evaluate<'a>(
    subs: &'a [Sub],
    txs: &'a [Tx],
    links: &[Link],
    months: &[NaiveDate],
    today: NaiveDate,
) -> Vec<Period<'a>> {
    let assigned = assign(subs, txs, links);
    let empty = Vec::new();
    let mut out = Vec::new();
    for s in subs {
        let mine = assigned.get(&s.id).unwrap_or(&empty);
        for &m in months {
            let key = month_key(m);
            let due = due_in(s, m);
            let rows: Vec<&Assigned> = mine.iter().filter(|(_, p, ..)| *p == key).collect();
            let mut items: Vec<Item> = rows
                .iter()
                .map(|(t, _, confirmed, parent)| Item {
                    parent: *parent,
                    tx: t,
                    role: role(t),
                    state: if *confirmed {
                        Match::Confirmed
                    } else {
                        Match::Auto
                    },
                    amount_ok: amount_ok(s, t),
                })
                .collect();
            // 扣款：同一期只自動採用一筆；有確認過的就不再自動採用其他筆。
            let has_confirmed = items
                .iter()
                .any(|i| i.role == Role::Charge && i.state == Match::Confirmed);
            let auto: Vec<usize> = items
                .iter()
                .enumerate()
                .filter(|(_, i)| i.role == Role::Charge && i.state == Match::Auto)
                .filter(|(_, i)| !s.require_amount || i.amount_ok == Some(true))
                .map(|(n, _)| n)
                .collect();
            let keep: Option<usize> = if has_confirmed {
                None
            } else if auto.len() == 1 {
                Some(auto[0])
            } else {
                let ok: Vec<usize> = auto
                    .iter()
                    .copied()
                    .filter(|&n| items[n].amount_ok == Some(true))
                    .collect();
                (ok.len() == 1).then(|| ok[0])
            };
            for (n, i) in items.iter_mut().enumerate() {
                if i.state != Match::Auto {
                    continue;
                }
                let pending = match i.role {
                    Role::Charge => keep != Some(n),
                    Role::Refund => s.require_amount && i.amount_ok != Some(true),
                    Role::Fee => false,
                };
                if pending {
                    i.state = Match::Pending;
                }
            }
            // 服務費跟著它的扣款：扣款待確認時服務費也待確認
            let states: HashMap<Uuid, Match> = items
                .iter()
                .filter(|i| i.parent.is_none())
                .map(|i| (i.tx.id, i.state))
                .collect();
            for i in items.iter_mut() {
                if let Some(p) = i.parent {
                    if i.state == Match::Auto && states.get(&p).is_none_or(|s| *s == Match::Pending)
                    {
                        i.state = Match::Pending;
                    }
                }
            }
            items.sort_by_key(|i| (i.tx.date, i.tx.id));
            let accepted: Vec<&Item> = items.iter().filter(|i| i.state != Match::Pending).collect();
            let charges: Vec<&&Item> = accepted.iter().filter(|i| i.role == Role::Charge).collect();
            let status = if !charges.is_empty() {
                "paid"
            } else if items.iter().any(|i| i.role == Role::Charge) {
                "review"
            } else {
                match due {
                    None => "none",
                    Some(d) if !expected(s, d) => "inactive",
                    Some(d) if today > d + Duration::days(s.window_days as i64) => "missed",
                    Some(_) => "upcoming",
                }
            };
            if matches!(status, "none" | "inactive") && items.is_empty() {
                continue;
            }
            let mut flags = Vec::new();
            if charges.iter().any(|i| i.amount_ok == Some(false)) {
                flags.push("amount_changed");
            }
            if charges.len() > 1 {
                flags.push("multiple");
            }
            if let Some(end) = s.ended_on {
                if charges.iter().any(|i| i.tx.date > end) {
                    flags.push("after_end");
                }
            }
            if accepted.iter().any(|i| i.role == Role::Refund) {
                flags.push("refunded");
            }
            if charges.iter().any(|i| i.tx.status == "pending") {
                flags.push("posting");
            }
            out.push(Period {
                sub: s,
                month: key,
                due,
                status,
                items,
                flags,
            });
        }
    }
    out
}

/// 一期實際支出（以交易帳戶幣別，正數為支出）。
fn spent(p: &Period) -> BTreeMap<String, Decimal> {
    let mut m = BTreeMap::new();
    for i in p.items.iter().filter(|i| i.state != Match::Pending) {
        *m.entry(i.tx.currency.clone().unwrap_or_else(|| "TWD".into()))
            .or_insert(Decimal::ZERO) -= i.tx.amount;
    }
    m
}

fn item_json(i: &Item) -> Value {
    let t = i.tx;
    json!({
        "id": t.id, "date": t.date, "description": t.description, "account_name": t.account_name,
        "amount": t.amount.normalize().to_string(), "currency": t.currency,
        "original_amount": t.original_amount, "original_currency": t.original_currency,
        "status": t.status, "manual": t.manual,
        "role": match i.role { Role::Charge => "charge", Role::Fee => "fee", Role::Refund => "refund" },
        "match": match i.state { Match::Auto => "auto", Match::Confirmed => "confirmed", Match::Pending => "pending" },
        "amount_ok": i.amount_ok,
        "fee_for": i.parent,
    })
}

/// 這一期被使用者按「不是」排除的交易，讓畫面可以還原。
fn excluded(p: &Period, txs: &[Tx], links: &[Link]) -> Vec<Value> {
    links
        .iter()
        .filter(|l| l.subscription_id == p.sub.id && l.state == "rejected" && l.period == p.month)
        .filter_map(|l| txs.iter().find(|t| t.id == l.tx_id))
        .map(|t| {
            json!({
                "id": t.id, "date": t.date, "description": t.description, "account_name": t.account_name,
                "amount": t.amount.normalize().to_string(), "currency": t.currency,
            })
        })
        .collect()
}

fn sub_json(s: &Sub) -> Value {
    json!({
        "id": s.id, "name": s.name, "category": s.category,
        "amount": s.amount.normalize().to_string(), "currency": s.currency,
        "cycle": s.cycle, "anchor_day": s.anchor_day, "anchor_month": s.anchor_month,
        "started_on": s.started_on, "ended_on": s.ended_on, "paused": s.paused,
        "patterns": s.patterns, "tolerance": s.tolerance.normalize().to_string(),
        "window_days": s.window_days, "require_amount": s.require_amount, "note": s.note,
        "icon": s.icon,
    })
}

const SUB_SQL: &str = "SELECT id,name,category,amount,currency,cycle,anchor_day,anchor_month,started_on,ended_on,paused,patterns,tolerance,window_days,require_amount,note,CASE WHEN icon_photo IS NOT NULL THEN 'data:'||icon_mime||';base64,'||translate(encode(icon_photo,'base64'),E'\n','') END AS icon FROM subscriptions WHERE owner_id=$1";

type Tenant = sqlx::Transaction<'static, sqlx::Postgres>;

async fn load(
    tx: &mut Tenant,
    owner: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<(Vec<Sub>, Vec<Tx>, Vec<Link>)> {
    let subs: Vec<Sub> = sqlx::query_as(&format!("{SUB_SQL} ORDER BY created_at,id"))
        .bind(owner)
        .fetch_all(&mut **tx)
        .await?;
    if subs.is_empty() {
        return Ok((subs, Vec::new(), Vec::new()));
    }
    let txs: Vec<Tx> = sqlx::query_as(&format!(
        "{FEED_SQL}, pats AS (SELECT DISTINCT unnest(patterns) p FROM subscriptions WHERE owner_id=$1),
         linked AS (SELECT coalesce(source_id,manual_id) id FROM subscription_links WHERE owner_id=$1)
         SELECT f.id,f.occurred_on AS date,f.amount,f.currency,f.source_details->>'original_amount' AS original_amount,
          f.source_details->>'original_currency' AS original_currency,f.description,f.account_name,f.status,
          NOT f.read_only AS manual,category_text(f.description) AS text
         FROM feed f WHERE f.occurred_on BETWEEN $2 AND $3 AND f.kind<>'transfer'
          AND (EXISTS(SELECT 1 FROM pats WHERE strpos(category_text(f.description),pats.p)>0) OR f.id IN (SELECT id FROM linked)
           OR category_text(f.description) ~ '(手續|服務)費')"
    ))
    .bind(owner)
    .bind(from)
    .bind(to)
    .fetch_all(&mut **tx)
    .await?;
    let links: Vec<Link> = sqlx::query_as("SELECT subscription_id,coalesce(source_id,manual_id) AS tx_id,period,state FROM subscription_links WHERE owner_id=$1")
        .bind(owner)
        .fetch_all(&mut **tx)
        .await?;
    Ok((subs, txs, links))
}

fn today() -> NaiveDate {
    (chrono::Utc::now() + Duration::hours(8)).date_naive()
}
fn months_between(from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut m = from;
    while m <= to {
        out.push(m);
        m = m.checked_add_months(Months::new(1)).unwrap();
    }
    out
}

/// 估計每期台幣金額：最近一期實際台幣支出，否則以匯率換算預期金額。
fn estimate(s: &Sub, history: &[&Period], quotes: Option<&fx::Quotes>) -> Option<Decimal> {
    history
        .iter()
        .filter(|p| p.sub.id == s.id && p.status == "paid")
        .max_by(|a, b| a.month.cmp(&b.month))
        .and_then(|p| {
            let m = spent(p);
            (m.len() == 1).then(|| m.get("TWD").copied()).flatten()
        })
        .or_else(|| {
            if s.currency == "TWD" {
                Some(s.amount)
            } else {
                quotes?.to_twd(&s.currency, s.amount)
            }
        })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonthQuery {
    month: String,
}

/// GET /subscriptions/month?month=YYYY-MM：這個月每個訂閱的扣款狀態與實際交易。
pub async fn month(
    user: Identity,
    State(s): State<AppState>,
    Query(q): Query<MonthQuery>,
) -> Result<Json<Value>> {
    let m = parse_month(&q.month)?;
    let first = m.checked_sub_months(Months::new(12)).unwrap();
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let (subs, txs, links) = load(
        &mut tx,
        user.id,
        first - Duration::days(200),
        m.checked_add_months(Months::new(2)).unwrap() + Duration::days(200),
    )
    .await?;
    drop(tx);
    let now = today();
    let periods = evaluate(&subs, &txs, &links, &months_between(first, m), now);
    let quotes = if subs.iter().any(|x| x.currency != "TWD") {
        fx::quotes(&s).await
    } else {
        None
    };
    let history: Vec<&Period> = periods.iter().collect();
    let mut actual: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut expected_twd = Decimal::ZERO;
    let mut monthly_twd = Decimal::ZERO;
    let mut unknown_estimate = 0;
    let mut rows = Vec::new();
    for p in periods.iter().filter(|p| p.month == q.month) {
        for (c, v) in spent(p) {
            *actual.entry(c).or_insert(Decimal::ZERO) += v;
        }
        let est = estimate(p.sub, &history, quotes.as_ref());
        if p.due.is_some_and(|d| expected(p.sub, d)) {
            match est {
                Some(v) => expected_twd += v,
                None => unknown_estimate += 1,
            }
        }
        rows.push(json!({
            "subscription": sub_json(p.sub), "month": p.month, "due": p.due, "status": p.status,
            "flags": p.flags, "estimate_twd": est.map(|v| v.round_dp(0).to_string()),
            "spent": spent(p).into_iter().map(|(c, v)| json!({"currency": c, "amount": v.normalize().to_string()})).collect::<Vec<_>>(),
            "items": p.items.iter().map(item_json).collect::<Vec<_>>(),
            "excluded": excluded(p, &txs, &links),
        }));
    }
    let month_end = m.checked_add_months(Months::new(1)).unwrap() - Duration::days(1);
    for x in subs
        .iter()
        .filter(|x| !x.paused && x.started_on <= month_end && x.ended_on.is_none_or(|e| e >= m))
    {
        match estimate(x, &history, quotes.as_ref()) {
            Some(v) => {
                monthly_twd += if x.cycle == "yearly" {
                    v / Decimal::from(12)
                } else {
                    v
                }
            }
            None => unknown_estimate += 1,
        }
    }
    rows.sort_by(|a, b| {
        let d = |v: &Value| {
            v["items"]
                .as_array()
                .and_then(|i| i.first())
                .and_then(|i| i["date"].as_str().map(str::to_owned))
                .or_else(|| v["due"].as_str().map(str::to_owned))
                .unwrap_or_default()
        };
        d(a).cmp(&d(b))
    });
    let count = |st: &str| rows.iter().filter(|r| r["status"] == st).count();
    Ok(Json(json!({
        "month": q.month, "today": now,
        "summary": {
            "actual": actual.into_iter().map(|(c, v)| json!({"currency": c, "amount": v.normalize().to_string()})).collect::<Vec<_>>(),
            "expected_twd": expected_twd.round_dp(0).to_string(),
            "monthly_equivalent_twd": monthly_twd.round_dp(0).to_string(),
            "unknown_estimate": unknown_estimate,
            "paid": count("paid"), "upcoming": count("upcoming"), "missed": count("missed"), "review": count("review"),
        },
        "periods": rows,
    })))
}

/// GET /subscriptions：訂閱清單與最近一次扣款。
/// GET /subscriptions/matches：近 13 個月已歸到訂閱的交易（不含待確認），讓收支清單標示是哪個訂閱。
pub async fn matches(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let now = today();
    let this = now.with_day(1).unwrap();
    let first = this.checked_sub_months(Months::new(12)).unwrap();
    let last = this.checked_add_months(Months::new(1)).unwrap();
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let (subs, txs, links) = load(
        &mut tx,
        user.id,
        first - Duration::days(200),
        last + Duration::days(200),
    )
    .await?;
    drop(tx);
    let periods = evaluate(&subs, &txs, &links, &months_between(first, last), now);
    Ok(Json(
        periods
            .iter()
            .flat_map(|p| {
                p.items
                    .iter()
                    .filter(|i| i.state != Match::Pending)
                    .map(move |i| {
                        json!({
                            "transaction_id": i.tx.id, "subscription_id": p.sub.id,
                            "name": p.sub.name, "month": p.month,
                            "role": match i.role { Role::Charge => "charge", Role::Fee => "fee", Role::Refund => "refund" },
                        })
                    })
            })
            .collect(),
    ))
}

pub async fn list(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let now = today();
    let this = now.with_day(1).unwrap();
    let first = this.checked_sub_months(Months::new(12)).unwrap();
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let (subs, txs, links) = load(
        &mut tx,
        user.id,
        first - Duration::days(200),
        now + Duration::days(200),
    )
    .await?;
    drop(tx);
    let periods = evaluate(&subs, &txs, &links, &months_between(first, this), now);
    let history: Vec<&Period> = periods.iter().collect();
    let quotes = if subs.iter().any(|x| x.currency != "TWD") {
        fx::quotes(&s).await
    } else {
        None
    };
    Ok(Json(
        subs.iter()
            .map(|x| {
                let mine: Vec<&&Period> = history.iter().filter(|p| p.sub.id == x.id).collect();
                let last = mine
                    .iter()
                    .flat_map(|p| p.items.iter())
                    .filter(|i| i.role == Role::Charge && i.state != Match::Pending)
                    .max_by_key(|i| i.tx.date);
                let mut v = sub_json(x);
                v["estimate_twd"] = json!(estimate(x, &history, quotes.as_ref()).map(|v| v.round_dp(0).to_string()));
                v["last_charge"] = json!(last.map(|i| json!({"date": i.tx.date, "amount": i.tx.amount.normalize().to_string(), "currency": i.tx.currency})));
                v["review"] = json!(mine.iter().filter(|p| p.status == "review" || p.items.iter().any(|i| i.state == Match::Pending)).count());
                v["history"] = json!(mine.iter().map(|p| json!({"month": p.month, "status": p.status})).collect::<Vec<_>>());
                v
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    name: String,
    category: String,
    amount: String,
    currency: String,
    cycle: String,
    anchor_day: i32,
    anchor_month: Option<i32>,
    started_on: NaiveDate,
    ended_on: Option<NaiveDate>,
    #[serde(default)]
    paused: bool,
    patterns: Vec<String>,
    tolerance: Option<String>,
    window_days: Option<i32>,
    #[serde(default)]
    require_amount: bool,
    #[serde(default)]
    note: String,
}

struct Valid {
    amount: Decimal,
    tolerance: Decimal,
    anchor_month: Option<i32>,
}

fn validate(v: &Input) -> Result<Valid> {
    let amount = decimal(&v.amount)?;
    let tolerance = match &v.tolerance {
        Some(t) => decimal(t)?,
        None => Decimal::from(15),
    };
    let name = v.name.trim().chars().count();
    let category = v.category.trim().chars().count();
    if !(1..=80).contains(&name) || !(1..=60).contains(&category) || v.note.chars().count() > 500 {
        return Err(ApiError::bad("名稱、分類或備註長度不正確"));
    }
    if amount <= Decimal::ZERO {
        return Err(ApiError::bad("預期金額需大於 0"));
    }
    if v.currency.len() != 3 || !v.currency.bytes().all(|c| c.is_ascii_uppercase()) {
        return Err(ApiError::bad("幣別需為三碼大寫代碼"));
    }
    if !(1..=31).contains(&v.anchor_day) {
        return Err(ApiError::bad("扣款日需為 1 到 31"));
    }
    let anchor_month = match v.cycle.as_str() {
        "monthly" => None,
        "yearly" => Some(
            v.anchor_month
                .filter(|m| (1..=12).contains(m))
                .ok_or_else(|| ApiError::bad("年繳需指定扣款月份"))?,
        ),
        _ => return Err(ApiError::bad("週期需為每月或每年")),
    };
    if tolerance < Decimal::ZERO || tolerance > Decimal::from(100) {
        return Err(ApiError::bad("金額誤差需為 0 到 100%"));
    }
    if !(0..=15).contains(&v.window_days.unwrap_or(5)) {
        return Err(ApiError::bad("扣款日誤差需為 0 到 15 天"));
    }
    if v.ended_on.is_some_and(|e| e < v.started_on) {
        return Err(ApiError::bad("結束日期需晚於開始日期"));
    }
    if v.patterns.is_empty() || v.patterns.len() > 10 {
        return Err(ApiError::bad("比對字串需為 1 到 10 個"));
    }
    Ok(Valid {
        amount,
        tolerance,
        anchor_month,
    })
}

/// 比對字串與同步交易使用相同正規化（小寫、去空白）。
async fn normalize(tx: &mut Tenant, patterns: &[String]) -> Result<Vec<String>> {
    let out: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT category_text(p) FROM unnest($1::text[]) p")
            .bind(patterns)
            .fetch_all(&mut **tx)
            .await?;
    if out.iter().any(|p| !(2..=80).contains(&p.chars().count())) {
        return Err(ApiError::bad("比對字串需為 2 到 80 個字"));
    }
    Ok(out)
}

/// POST /subscriptions
pub async fn create(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<Input>,
) -> Result<(StatusCode, Json<Value>)> {
    let ok = validate(&v)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let patterns = normalize(&mut tx, &v.patterns).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM subscriptions WHERE owner_id=$1")
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= 200 {
        return Err(ApiError::bad("訂閱數量已達上限"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO subscriptions(id,owner_id,name,category,amount,currency,cycle,anchor_day,anchor_month,started_on,ended_on,paused,patterns,tolerance,window_days,require_amount,note) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)")
        .bind(id).bind(user.id).bind(v.name.trim()).bind(v.category.trim()).bind(ok.amount).bind(&v.currency)
        .bind(&v.cycle).bind(v.anchor_day).bind(ok.anchor_month).bind(v.started_on).bind(v.ended_on).bind(v.paused)
        .bind(&patterns).bind(ok.tolerance).bind(v.window_days.unwrap_or(5)).bind(v.require_amount).bind(v.note.trim())
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// PUT /subscriptions/{id}：整筆更新。
pub async fn update(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<Input>,
) -> Result<Json<Value>> {
    let ok = validate(&v)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let patterns = normalize(&mut tx, &v.patterns).await?;
    let r = sqlx::query("UPDATE subscriptions SET name=$3,category=$4,amount=$5,currency=$6,cycle=$7,anchor_day=$8,anchor_month=$9,started_on=$10,ended_on=$11,paused=$12,patterns=$13,tolerance=$14,window_days=$15,require_amount=$16,note=$17,updated_at=now() WHERE id=$1 AND owner_id=$2")
        .bind(id).bind(user.id).bind(v.name.trim()).bind(v.category.trim()).bind(ok.amount).bind(&v.currency)
        .bind(&v.cycle).bind(v.anchor_day).bind(ok.anchor_month).bind(v.started_on).bind(v.ended_on).bind(v.paused)
        .bind(&patterns).bind(ok.tolerance).bind(v.window_days.unwrap_or(5)).bind(v.require_amount).bind(v.note.trim())
        .execute(&mut *tx).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

/// DELETE /subscriptions/{id}：只刪除訂閱與配對決定，交易不受影響。
pub async fn delete(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r = sqlx::query("DELETE FROM subscriptions WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 圖示解碼後的上限；前端會先縮成 128px 以內，正常只有數 KB。
const MAX_ICON: usize = 65_536;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IconInput {
    /// data:image/(png|jpeg|webp);base64,...
    data: String,
}

/// PUT /subscriptions/{id}/icon：上傳自訂圖示。只接受依檔頭確認的 PNG／JPEG／WebP，不接受 SVG。
pub async fn set_icon(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<IconInput>,
) -> Result<Json<Value>> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let b64 = v
        .data
        .split_once(";base64,")
        .filter(|(head, _)| head.starts_with("data:image/"))
        .map(|(_, b)| b)
        .ok_or_else(|| ApiError::bad("圖片格式錯誤"))?;
    if b64.len() > MAX_ICON * 4 / 3 + 8 {
        return Err(ApiError::bad("圖示太大，請換一張較小的圖片"));
    }
    let bytes = STANDARD
        .decode(b64)
        .map_err(|_| ApiError::bad("圖片格式錯誤"))?;
    if bytes.len() > MAX_ICON {
        return Err(ApiError::bad("圖示太大，請換一張較小的圖片"));
    }
    let mime = crate::profile::sniff(&bytes)
        .ok_or_else(|| ApiError::bad("只支援 PNG、JPEG、WebP 圖片"))?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r = sqlx::query("UPDATE subscriptions SET icon_photo=$3,icon_mime=$4,updated_at=now() WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .bind(&bytes)
        .bind(mime)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

/// DELETE /subscriptions/{id}/icon：移除自訂圖示，改回分類圖示。
pub async fn clear_icon(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r = sqlx::query("UPDATE subscriptions SET icon_photo=NULL,icon_mime=NULL,updated_at=now() WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkInput {
    transaction_id: Uuid,
    period: String,
    state: String,
}

/// PUT /subscriptions/{id}/links：確認某筆交易屬於這一期，或排除它。
pub async fn link(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<LinkInput>,
) -> Result<Json<Value>> {
    parse_month(&v.period)?;
    if !matches!(v.state.as_str(), "confirmed" | "rejected") {
        return Err(ApiError::bad("狀態需為確認或排除"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM subscriptions WHERE id=$1 AND owner_id=$2)",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    let source: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM source_records WHERE id=$1 AND owner_id=$2 AND kind='transaction')")
        .bind(v.transaction_id)
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await?;
    let manual: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM transactions WHERE id=$1 AND owner_id=$2 AND kind<>'transfer')")
        .bind(v.transaction_id)
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await?;
    if !exists || !(source || manual) {
        return Err(ApiError::missing());
    }
    let (sid, mid) = if source {
        (Some(v.transaction_id), None)
    } else {
        (None, Some(v.transaction_id))
    };
    // 同一筆交易只能確認給一個訂閱：改確認到這裡時移除其他訂閱的確認。
    sqlx::query("DELETE FROM subscription_links WHERE owner_id=$1 AND coalesce(source_id,manual_id)=$2 AND (subscription_id=$3 OR ($4='confirmed' AND state='confirmed'))")
        .bind(user.id)
        .bind(v.transaction_id)
        .bind(id)
        .bind(&v.state)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO subscription_links(id,owner_id,subscription_id,source_id,manual_id,period,state) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(Uuid::new_v4()).bind(user.id).bind(id).bind(sid).bind(mid).bind(&v.period).bind(&v.state)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

/// DELETE /subscriptions/{id}/links/{transaction_id}：還原為自動配對。
pub async fn unlink(
    user: Identity,
    State(s): State<AppState>,
    Path((id, tx_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    sqlx::query("DELETE FROM subscription_links WHERE owner_id=$1 AND subscription_id=$2 AND coalesce(source_id,manual_id)=$3")
        .bind(user.id)
        .bind(id)
        .bind(tx_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 疑似訂閱的商家代表字串：去掉交易序號等尾端數字與符號。
pub fn merchant_key(text: &str) -> String {
    text.trim_end_matches(|c: char| c.is_ascii_digit() || "*#-/._".contains(c))
        .to_owned()
}

#[derive(sqlx::FromRow)]
struct Candidate {
    date: NaiveDate,
    amount: Decimal,
    currency: Option<String>,
    description: Option<String>,
    category: String,
    original_amount: Option<String>,
    original_currency: Option<String>,
    text: String,
}

/// 同一商家連續多個月、平均每月約一筆，且扣款日或金額大致固定，就視為疑似訂閱；
/// 代收商（每月多筆不同金額）改以「商家＋金額」分組，並建議只配對相同金額。
/// 至少四分之三的交易落在同一個扣款日前後 3 天內（跨月底也算）。
fn steady_day(list: &[(NaiveDate, Decimal)]) -> bool {
    let days: Vec<i32> = list.iter().map(|(d, _)| d.day() as i32).collect();
    let near = |a: i32, b: i32| {
        let x = (a - b).abs();
        x.min(31 - x) <= 3
    };
    let best = days
        .iter()
        .map(|&c| days.iter().filter(|&&d| near(c, d)).count())
        .max()
        .unwrap_or(0);
    best * 4 >= days.len() * 3
}

/// 至少四分之三的交易金額與同一筆相差 15% 以內。
fn steady_amount(list: &[(NaiveDate, Decimal)]) -> bool {
    let amounts: Vec<Decimal> = list.iter().map(|(_, a)| a.abs()).collect();
    let best = amounts
        .iter()
        .map(|c| {
            amounts
                .iter()
                .filter(|a| (**a - *c).abs() <= *c * Decimal::new(15, 2))
                .count()
        })
        .max()
        .unwrap_or(0);
    best * 4 >= amounts.len() * 3
}

pub fn suggest(
    rows: &[(String, NaiveDate, Decimal)],
    min_months: usize,
) -> Vec<(String, Option<Decimal>)> {
    let mut by_key: BTreeMap<&str, Vec<(NaiveDate, Decimal)>> = BTreeMap::new();
    for (k, d, a) in rows {
        by_key.entry(k).or_default().push((*d, *a));
    }
    let regular = |list: &[(NaiveDate, Decimal)]| {
        let months: BTreeSet<String> = list.iter().map(|(d, _)| month_key(*d)).collect();
        months.len() >= min_months && list.len() * 2 <= months.len() * 3
    };
    let mut out = Vec::new();
    for (k, list) in by_key {
        // 訂閱的扣款日或金額大致固定；日常消費兩者都不固定
        if regular(&list) && (steady_day(&list) || steady_amount(&list)) {
            out.push((k.to_owned(), None));
            continue;
        }
        let mut by_amount: BTreeMap<Decimal, Vec<(NaiveDate, Decimal)>> = BTreeMap::new();
        for (d, a) in list {
            by_amount
                .entry(a.abs().normalize())
                .or_default()
                .push((d, a));
        }
        // 同金額分組時另外要求間隔約一個月，避免把固定價格的日常消費當成訂閱
        for (a, mut l) in by_amount {
            l.sort();
            let monthly = l
                .windows(2)
                .all(|w| (25..=37).contains(&(w[1].0 - w[0].0).num_days()));
            if regular(&l) && monthly {
                out.push((k.to_owned(), Some(a)));
            }
        }
    }
    out
}

/// GET /subscriptions/suggestions：近 7 個月內像訂閱、但還沒有建立的扣款。
pub async fn suggestions(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let now = today();
    let from = now
        .with_day(1)
        .unwrap()
        .checked_sub_months(Months::new(6))
        .unwrap();
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows: Vec<Candidate> = sqlx::query_as(&format!(
        "{FEED_SQL} SELECT feed.occurred_on AS date,feed.amount,feed.currency,feed.description,feed.category,
          feed.source_details->>'original_amount' AS original_amount,feed.source_details->>'original_currency' AS original_currency,
          category_text(feed.description) AS text
         FROM feed WHERE feed.occurred_on>=$2 AND feed.amount<0 AND feed.kind='expense' AND {NOT_FLOW}
          AND category_text(feed.description) !~ '(手續|服務)費'
          AND NOT EXISTS(SELECT 1 FROM subscriptions s,unnest(s.patterns) p WHERE s.owner_id=$1 AND strpos(category_text(feed.description),p)>0
            AND (NOT s.require_amount OR s.currency<>coalesce(feed.currency,'TWD') OR abs(abs(feed.amount)-s.amount)<=s.amount*s.tolerance/100))
          AND NOT EXISTS(SELECT 1 FROM subscription_links l WHERE l.owner_id=$1 AND coalesce(l.source_id,l.manual_id)=feed.id)
         ORDER BY feed.occurred_on"
    ))
    .bind(user.id)
    .bind(from)
    .fetch_all(&mut *tx)
    .await?;
    let dismissed: BTreeSet<String> =
        sqlx::query_scalar("SELECT key FROM subscription_dismissals WHERE owner_id=$1")
            .bind(user.id)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();
    drop(tx);
    let keyed: Vec<(String, NaiveDate, Decimal)> = rows
        .iter()
        .map(|r| (merchant_key(&r.text), r.date, r.amount))
        .collect();
    let mut out = Vec::new();
    for (key, amount) in suggest(&keyed, 3) {
        if key.chars().count() < 3 {
            continue;
        }
        let id = match amount {
            Some(a) => format!("{key}|{a}"),
            None => key.clone(),
        };
        if dismissed.contains(&id) || id.chars().count() > 120 {
            continue;
        }
        let list: Vec<&Candidate> = rows
            .iter()
            .filter(|r| {
                merchant_key(&r.text) == key
                    && amount.is_none_or(|a| r.amount.abs().normalize() == a)
            })
            .collect();
        let last = list.last().unwrap();
        let months: BTreeSet<String> = list.iter().map(|r| month_key(r.date)).collect();
        let foreign = last
            .original_currency
            .as_deref()
            .filter(|c| !c.is_empty() && Some(*c) != last.currency.as_deref());
        out.push(json!({
            "key": id, "pattern": key,
            "name": last.description.as_deref().map(|d| d.trim_end_matches(|c: char| c.is_ascii_digit() || c.is_whitespace()).to_owned()),
            "category": last.category,
            "months": months.len(), "count": list.len(),
            "last_date": last.date, "anchor_day": last.date.day(),
            "amount": last.amount.abs().normalize().to_string(),
            "currency": last.currency,
            "original_amount": foreign.and(last.original_amount.clone()),
            "original_currency": foreign,
            "require_amount": amount.is_some(),
            "recent": list.iter().rev().take(6).map(|r| json!({"date": r.date, "amount": r.amount.normalize().to_string()})).collect::<Vec<_>>(),
        }));
    }
    out.sort_by(|a, b| {
        b["months"]
            .as_u64()
            .cmp(&a["months"].as_u64())
            .then_with(|| b["last_date"].as_str().cmp(&a["last_date"].as_str()))
    });
    out.truncate(30);
    Ok(Json(out))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dismiss {
    key: String,
}
/// POST /subscriptions/suggestions/dismiss：這個商家不是訂閱，不再建議。
pub async fn dismiss(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<Dismiss>,
) -> Result<StatusCode> {
    let n = v.key.chars().count();
    if !(1..=120).contains(&n) {
        return Err(ApiError::bad("商家代碼格式不正確"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    sqlx::query(
        "INSERT INTO subscription_dismissals(owner_id,key) VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(user.id)
    .bind(&v.key)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /subscriptions/suggestions/dismissed：按過「不是訂閱」的商家，新的在前。
pub async fn dismissed(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let rows: Vec<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT key,created_at FROM subscription_dismissals WHERE owner_id=$1 ORDER BY created_at DESC,key",
    )
    .bind(user.id)
    .fetch_all(&mut *tx)
    .await?;
    drop(tx);
    Ok(Json(
        rows.into_iter()
            .map(|(key, at)| {
                // key 為「商家」或「商家|金額」（代收商依金額區分）
                let (name, amount) = match key.split_once('|') {
                    Some((n, a)) => (n.to_owned(), Some(a.to_owned())),
                    None => (key.clone(), None),
                };
                json!({"key": key, "name": name, "amount": amount, "created_at": at})
            })
            .collect(),
    ))
}

/// POST /subscriptions/suggestions/restore：取消「不是訂閱」，之後重新列入建議。
pub async fn restore(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<Dismiss>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let r = sqlx::query("DELETE FROM subscription_dismissals WHERE owner_id=$1 AND key=$2")
        .bind(user.id)
        .bind(&v.key)
        .execute(&mut *tx)
        .await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    fn sub(patterns: &[&str], amount: i64, currency: &str) -> Sub {
        Sub {
            id: Uuid::new_v4(),
            name: "t".into(),
            category: "AI 工具".into(),
            amount: Decimal::from(amount),
            currency: currency.into(),
            cycle: "monthly".into(),
            anchor_day: 9,
            anchor_month: None,
            started_on: d("2026-01-01"),
            ended_on: None,
            paused: false,
            patterns: patterns.iter().map(|p| p.to_string()).collect(),
            tolerance: Decimal::from(15),
            window_days: 5,
            require_amount: false,
            note: String::new(),
            icon: None,
        }
    }
    fn tx(date: &str, text: &str, amount: i64, orig: Option<(&str, &str)>) -> Tx {
        Tx {
            id: Uuid::new_v4(),
            date: d(date),
            amount: Decimal::from(amount),
            currency: Some("TWD".into()),
            original_amount: orig.map(|o| o.0.into()),
            original_currency: orig.map(|o| o.1.into()),
            description: Some(text.into()),
            account_name: None,
            status: "posted".into(),
            manual: false,
            text: text.into(),
        }
    }

    #[test]
    fn due_dates_clamp_to_month_end() {
        let mut s = sub(&["x"], 1, "TWD");
        s.anchor_day = 31;
        assert_eq!(due_in(&s, d("2026-02-01")), Some(d("2026-02-28")));
        assert_eq!(nearest_due(&s, d("2026-03-02")), d("2026-02-28"));
        s.cycle = "yearly".into();
        s.anchor_month = Some(3);
        assert_eq!(due_in(&s, d("2026-02-01")), None);
        assert_eq!(due_in(&s, d("2026-03-01")), Some(d("2026-03-31")));
        assert_eq!(nearest_due(&s, d("2026-09-01")), d("2026-03-31"));
    }

    #[test]
    fn matches_foreign_amount_and_flags_plan_change() {
        let s = sub(&["anthropic*claude", "claude.ai"], 200, "USD");
        let subs = [s];
        let txs = [
            tx(
                "2026-08-09",
                "anthropic*claudes125001",
                -6451,
                Some(("200", "USD")),
            ),
            tx(
                "2026-09-09",
                "anthropic*claudes125001",
                -3325,
                Some(("105", "USD")),
            ),
        ];
        let ps = evaluate(
            &subs,
            &txs,
            &[],
            &[d("2026-08-01"), d("2026-09-01")],
            d("2026-09-25"),
        );
        assert_eq!(ps[0].status, "paid");
        assert!(ps[0].flags.is_empty());
        assert_eq!(ps[1].status, "paid");
        assert_eq!(ps[1].flags, vec!["amount_changed"]);
    }

    #[test]
    fn ambiguous_charges_need_review_and_missing_is_reported() {
        let subs = [sub(&["claude"], 200, "USD")];
        let txs = [
            tx(
                "2026-08-09",
                "anthropic*claudes125001",
                -6451,
                Some(("200", "USD")),
            ),
            tx("2026-08-11", "anthropic*claudes125044", -6000, None),
        ];
        let ps = evaluate(
            &subs,
            &txs,
            &[],
            &[d("2026-08-01"), d("2026-09-01")],
            d("2026-09-20"),
        );
        assert_eq!(ps[0].status, "paid");
        assert_eq!(ps[0].items[0].state, Match::Auto);
        assert_eq!(ps[0].items[1].state, Match::Pending);
        assert_eq!(ps[1].status, "missed");
        let early = evaluate(&subs, &[], &[], &[d("2026-09-01")], d("2026-09-14"));
        assert_eq!(early[0].status, "upcoming");
    }

    #[test]
    fn aggregator_requires_amount_and_links_override() {
        let mut s = sub(&["googlepaymentcorp"], 90, "TWD");
        s.require_amount = true;
        s.anchor_day = 16;
        let subs = [s];
        let txs = [
            tx("2026-09-15", "googlepaymentcorp156132", -330, None),
            tx("2026-09-16", "googlepaymentcorp156132", -90, None),
        ];
        let ps = evaluate(&subs, &txs, &[], &[d("2026-09-01")], d("2026-09-25"));
        assert_eq!(ps[0].status, "paid");
        let states: Vec<Match> = ps[0].items.iter().map(|i| i.state).collect();
        assert_eq!(states, vec![Match::Pending, Match::Auto]);
        // 使用者排除 90 元那筆、確認 330 元那筆
        let links = [
            Link {
                subscription_id: subs[0].id,
                tx_id: txs[1].id,
                period: "2026-09".into(),
                state: "rejected".into(),
            },
            Link {
                subscription_id: subs[0].id,
                tx_id: txs[0].id,
                period: "2026-09".into(),
                state: "confirmed".into(),
            },
        ];
        let ps = evaluate(&subs, &txs, &links, &[d("2026-09-01")], d("2026-09-25"));
        assert_eq!(ps[0].items.len(), 1);
        assert_eq!(ps[0].items[0].state, Match::Confirmed);
        assert_eq!(spent(&ps[0])["TWD"], Decimal::from(330));
    }

    #[test]
    fn fees_refunds_and_charges_after_cancellation() {
        let mut s = sub(&["claude"], 200, "USD");
        s.ended_on = Some(d("2026-08-31"));
        let subs = [s];
        let txs = [
            tx(
                "2026-09-09",
                "claude.aisubscription",
                -6400,
                Some(("200", "USD")),
            ),
            tx("2026-09-09", "國外交易手續費-claude", -96, None),
            tx(
                "2026-09-10",
                "claude.aisubscription",
                6400,
                Some(("200", "USD")),
            ),
        ];
        let ps = evaluate(&subs, &txs, &[], &[d("2026-09-01")], d("2026-09-25"));
        assert_eq!(ps[0].status, "paid");
        assert!(ps[0].flags.contains(&"after_end"));
        assert!(ps[0].flags.contains(&"refunded"));
        assert_eq!(spent(&ps[0])["TWD"], Decimal::from(96));
        // 取消後的月份不再預期扣款
        let later = evaluate(&subs, &[], &[], &[d("2026-10-01")], d("2026-10-25"));
        assert!(later.is_empty());
    }

    #[test]
    fn longest_pattern_wins_between_subscriptions() {
        let a = sub(&["google"], 90, "TWD");
        let b = sub(&["googleyoutube"], 199, "TWD");
        let subs = [a, b];
        let txs = [
            tx("2026-09-09", "google*youtubepremium", -199, None),
            tx("2026-09-09", "googleyoutubepremium", -199, None),
        ];
        let ps = evaluate(&subs, &txs, &[], &[d("2026-09-01")], d("2026-09-25"));
        let a_items = ps
            .iter()
            .find(|p| p.sub.id == subs[0].id)
            .unwrap()
            .items
            .len();
        let b_items = ps
            .iter()
            .find(|p| p.sub.id == subs[1].id)
            .unwrap()
            .items
            .len();
        assert_eq!((a_items, b_items), (1, 1));
    }

    #[test]
    fn shared_aggregator_pattern_splits_by_amount() {
        let mk = |amount, day| {
            let mut s = sub(&["googlepaymentcorp"], amount, "TWD");
            s.require_amount = true;
            s.anchor_day = day;
            s
        };
        let subs = [mk(479, 10), mk(100, 12), mk(75, 15)];
        let txs = [
            tx("2026-09-03", "googlepaymentcorp156132", -100, None),
            tx("2026-09-10", "googlepaymentcorp156132", -479, None),
            tx("2026-09-10", "googlepaymentcorp156132", -16, None),
            tx("2026-09-15", "googlepaymentcorp156132", -75, None),
            tx("2026-09-15", "國外交易服務費(簽帳75)", -1, None),
        ];
        let ps = evaluate(&subs, &txs, &[], &[d("2026-09-01")], d("2026-09-25"));
        let of = |n: usize| ps.iter().find(|p| p.sub.id == subs[n].id).unwrap();
        let amounts = |n: usize| -> Vec<i64> {
            of(n).items.iter().map(|i| i64::try_from(i.tx.amount).unwrap()).collect()
        };
        assert_eq!(amounts(0), vec![-479]);
        assert_eq!(amounts(1), vec![-100]);
        assert_eq!(amounts(2).iter().sum::<i64>(), -76);
        assert!((0..3).all(|n| of(n).status == "paid"));
        assert!(ps.iter().all(|p| p.items.iter().all(|i| i.state != Match::Pending)));
    }

    #[test]
    fn same_price_subscriptions_split_by_due_day() {
        let mk = |day| {
            let mut s = sub(&["googlepaymentcorp"], 100, "TWD");
            s.require_amount = true;
            s.anchor_day = day;
            s
        };
        let subs = [mk(3), mk(12)];
        let txs = [
            tx("2026-09-03", "googlepaymentcorp156132", -100, None),
            tx("2026-09-12", "googlepaymentcorp156132", -100, None),
        ];
        let ps = evaluate(&subs, &txs, &[], &[d("2026-09-01")], d("2026-09-25"));
        for (n, date) in [(0, "2026-09-03"), (1, "2026-09-12")] {
            let p = ps.iter().find(|p| p.sub.id == subs[n].id).unwrap();
            assert_eq!(p.status, "paid");
            assert_eq!(p.items.len(), 1);
            assert_eq!(p.items[0].tx.date, d(date));
            assert!(p.flags.is_empty());
        }
    }

    #[test]
    fn suggestions_find_monthly_merchants() {
        assert_eq!(merchant_key("anthropic*claudes125001"), "anthropic*claudes");
        assert_eq!(merchant_key("apple.com/bill004619"), "apple.com/bill");
        let mut rows = Vec::new();
        for m in 4..=9 {
            let date = on(2026, m, 16);
            rows.push(("apple.com/bill".to_owned(), date, Decimal::from(-90)));
            // 代收商：每月多筆，其中 100 元固定每月一次
            rows.push(("googlepaymentcorp".to_owned(), date, Decimal::from(-100)));
            rows.push((
                "googlepaymentcorp".to_owned(),
                on(2026, m, 3),
                Decimal::from(-30 - m as i64),
            ));
            rows.push((
                "googlepaymentcorp".to_owned(),
                on(2026, m, 5),
                Decimal::from(-60 - m as i64),
            ));
            // 超市：每月很多筆、金額不固定
            for day in 1..6 {
                rows.push((
                    "全聯".to_owned(),
                    on(2026, m, day * 3),
                    Decimal::from(-100 - (m * 7) as i64 - day as i64),
                ));
            }
        }
        rows.push(("once".to_owned(), d("2026-09-01"), Decimal::from(-5)));
        // 同金額但間隔不規律（例如同一週內三次）不算
        for day in [2, 4, 6] {
            rows.push(("cafe".to_owned(), on(2026, 5, day), Decimal::from(-65)));
        }
        rows.push(("cafe".to_owned(), on(2026, 7, 2), Decimal::from(-65)));
        rows.push(("cafe".to_owned(), on(2026, 9, 2), Decimal::from(-65)));
        // 健身房：月費固定在 14 日，加一筆入會費仍算
        rows.push(("健身工廠".to_owned(), d("2026-04-02"), Decimal::from(-7200)));
        for m in 4..=9 {
            rows.push(("健身工廠".to_owned(), on(2026, m, 14), Decimal::from(-1288)));
        }
        // 藥局：每月一次左右，但日期與金額都不固定
        for (date, amount) in [
            ("2026-06-18", -171),
            ("2026-07-04", -145),
            ("2026-07-25", -310),
            ("2026-08-06", -176),
        ] {
            rows.push(("詠晴藥局".to_owned(), d(date), Decimal::from(amount)));
        }
        let found = suggest(&rows, 3);
        assert!(found.contains(&("apple.com/bill".into(), None)));
        assert!(found.contains(&("健身工廠".into(), None)));
        assert!(found.contains(&("googlepaymentcorp".into(), Some(Decimal::from(100)))));
        assert!(
            !found
                .iter()
                .any(|(k, _)| k == "全聯" || k == "once" || k == "cafe" || k == "詠晴藥局")
        );
    }

    #[test]
    fn foreign_fees_follow_their_charge() {
        assert_eq!(
            fee_ref("國外交易服務費(簽帳6,336)"),
            Some((Some(Decimal::from(6336)), None))
        );
        assert_eq!(
            fee_ref("國外交易手續費-claude"),
            Some((None, Some("claude")))
        );
        assert_eq!(fee_ref("國外交易服務費"), None);
        assert_eq!(fee_ref("anthropic*claudes125001"), None);
        let subs = [sub(&["anthropic*claude"], 200, "USD")];
        let txs = [
            tx(
                "2026-08-09",
                "anthropic*claudes125001",
                -6451,
                Some(("200", "USD")),
            ),
            tx("2026-08-11", "anthropic*claudes125044", -6000, None),
            tx("2026-08-09", "國外交易服務費(簽帳6,451)", -97, None),
            tx("2026-08-11", "國外交易服務費(簽帳6,000)", -90, None),
            // 別的商家的服務費不算
            tx("2026-08-10", "國外交易服務費(簽帳479)", -7, None),
            tx("2026-08-10", "國外交易手續費-claude", -47, None),
        ];
        let ps = evaluate(&subs, &txs, &[], &[d("2026-08-01")], d("2026-09-25"));
        let p = &ps[0];
        let state = |n: usize| {
            p.items
                .iter()
                .find(|i| i.tx.id == txs[n].id)
                .map(|i| i.state)
        };
        assert_eq!(state(2), Some(Match::Auto));
        assert_eq!(state(3), Some(Match::Pending), "扣款待確認時服務費也待確認");
        assert_eq!(state(4), None);
        assert_eq!(state(5), Some(Match::Auto), "國泰以商家名稱對應");
        assert_eq!(spent(p)["TWD"], Decimal::from(6451 + 97 + 47));
        // 確認 6000 那筆後，它的服務費一起算進來
        let links = [Link {
            subscription_id: subs[0].id,
            tx_id: txs[1].id,
            period: "2026-08".into(),
            state: "confirmed".into(),
        }];
        let ps = evaluate(&subs, &txs, &links, &[d("2026-08-01")], d("2026-09-25"));
        assert_eq!(
            spent(&ps[0])["TWD"],
            Decimal::from(6000 + 90),
            "國泰手續費仍跟著 8/9 那筆（已改為待確認）"
        );
    }
}
