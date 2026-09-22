//! 使用者分類：收入／支出／內部資金移動，各自的圖示、顏色與順序。
//!
//! 內建分類與自動分類關鍵字同名，只能換圖示、顏色或隱藏；自訂分類可改名與刪除，
//! 改名或刪除時一併更新既有交易、單筆標記、規則、預算與固定收支，避免留下孤兒分類。
use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::{AppState, tenant},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// 內建分類：名稱、類型、圖示、顏色（沿用舊版依名稱配色，避免既有畫面換色）。
const BUILTIN: [(&str, &str, &str, &str); 20] = [
    ("餐飲", "expense", "utensils", "c1"),
    ("交通", "expense", "bus", "c2"),
    ("購物", "expense", "shopping-bag", "c3"),
    ("居家", "expense", "house", "c4"),
    ("娛樂", "expense", "clapperboard", "c5"),
    ("醫療", "expense", "heart-pulse", "c6"),
    ("教育", "expense", "graduation-cap", "c7"),
    ("保險", "expense", "shield", "c2"),
    ("稅費", "expense", "landmark", "c3"),
    ("手續費", "expense", "receipt", "c1"),
    ("電子支付", "expense", "smartphone", "c5"),
    ("提款", "expense", "banknote", "c6"),
    ("貸款", "expense", "hand-coins", "c4"),
    ("薪資", "income", "briefcase", "c8"),
    ("獎金", "income", "gift", "c1"),
    ("投資收益", "income", "trending-up", "c2"),
    ("其他收入", "income", "coins", "c3"),
    ("轉帳", "flow", "arrow-left-right", "c4"),
    ("信用卡繳款", "flow", "credit-card", "c7"),
    ("投資", "flow", "trending-up", "c8"),
];
const UNCATEGORIZED: &str = "未分類";

fn check_name(v: &str) -> Result<String> {
    let v = v.trim();
    if v.is_empty() || v.chars().count() > 30 {
        return Err(ApiError::bad("分類名稱需為 1 到 30 個字"));
    }
    if v == UNCATEGORIZED {
        return Err(ApiError::bad("「未分類」是系統保留名稱"));
    }
    Ok(v.to_owned())
}
fn check_icon(v: &str) -> Result<()> {
    let ok = (1..=40).contains(&v.len())
        && v.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok { Ok(()) } else { Err(ApiError::bad("圖示格式錯誤")) }
}
fn check_color(v: &str) -> Result<()> {
    let ok = v
        .strip_prefix('c')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=12).contains(&n))
        && !v.starts_with("c0");
    if ok { Ok(()) } else { Err(ApiError::bad("顏色格式錯誤")) }
}
fn check_kind(v: &str) -> Result<()> {
    if matches!(v, "expense" | "income" | "flow") {
        Ok(())
    } else {
        Err(ApiError::bad("分類類型需為支出、收入或不算收支"))
    }
}

/// 補齊內建分類，並把過去自己輸入、但不在清單中的分類轉為自訂分類。
async fn ensure(tx: &mut sqlx::PgConnection, owner: Uuid) -> Result<()> {
    for (i, (name, kind, icon, color)) in BUILTIN.iter().enumerate() {
        sqlx::query("INSERT INTO user_categories(id,owner_id,name,kind,icon,color,position,builtin) VALUES($1,$2,$3,$4,$5,$6,$7,true) ON CONFLICT(owner_id,name) DO UPDATE SET builtin=true,kind=EXCLUDED.kind WHERE NOT user_categories.builtin")
            .bind(Uuid::new_v4()).bind(owner).bind(name).bind(kind).bind(icon).bind(color).bind(i as i32)
            .execute(&mut *tx).await?;
    }
    // 舊資料中的自訂名稱：收入交易用過就當收入，否則當支出
    sqlx::query(r#"
        INSERT INTO user_categories(id,owner_id,name,kind,icon,color,position)
        SELECT gen_random_uuid(),$1,u.name,CASE WHEN u.income THEN 'income' ELSE 'expense' END,'tag','c'||((hashtext(u.name)::bigint%8+8)%8+1),100
        FROM (
          SELECT btrim(category) AS name,bool_or(income) AS income FROM (
            SELECT category,kind='income' AS income FROM transactions WHERE owner_id=$1
            UNION ALL SELECT category,false FROM source_tx_labels WHERE owner_id=$1
            UNION ALL SELECT category,false FROM category_rules WHERE owner_id=$1
            UNION ALL SELECT category,false FROM budgets WHERE owner_id=$1
            UNION ALL SELECT category,kind='income' FROM recurring_entries WHERE owner_id=$1
            UNION ALL SELECT category,false FROM subscriptions WHERE owner_id=$1
          ) x WHERE length(btrim(category)) BETWEEN 1 AND 30 AND btrim(category)<>'未分類' GROUP BY 1
        ) u
        ON CONFLICT(owner_id,name) DO NOTHING
    "#).bind(owner).execute(&mut *tx).await?;
    Ok(())
}

/// GET /categories：我的分類，含規則數與內建關鍵字數。
pub async fn list(user: Identity, State(s): State<AppState>) -> Result<Json<Vec<Value>>> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    ensure(&mut tx, user.id).await?;
    let rows = sqlx::query_scalar("SELECT jsonb_build_object('id',c.id,'name',c.name,'kind',c.kind,'icon',c.icon,'color',c.color,'position',c.position,'hidden',c.hidden,'builtin',c.builtin,'rules',(SELECT count(*) FROM category_rules r WHERE r.owner_id=c.owner_id AND r.category=c.name),'keywords',(SELECT count(*) FROM category_keywords k WHERE k.category=c.name)) FROM user_categories c WHERE c.owner_id=$1 ORDER BY CASE c.kind WHEN 'expense' THEN 0 WHEN 'income' THEN 1 ELSE 2 END,c.position,c.created_at,c.name")
        .bind(user.id).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCategory {
    name: String,
    kind: String,
    icon: String,
    color: String,
}
/// POST /categories：新增自訂分類，排在同類型最後。
pub async fn create(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<NewCategory>,
) -> Result<(StatusCode, Json<Value>)> {
    let name = check_name(&v.name)?;
    check_kind(&v.kind)?;
    check_icon(&v.icon)?;
    check_color(&v.color)?;
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    ensure(&mut tx, user.id).await?;
    let id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO user_categories(id,owner_id,name,kind,icon,color,position) SELECT $1,$2,$3,$4,$5,$6,coalesce(max(position),0)+1 FROM user_categories WHERE owner_id=$2 AND kind=$4 ON CONFLICT(owner_id,name) DO NOTHING")
        .bind(id).bind(user.id).bind(&name).bind(&v.kind).bind(&v.icon).bind(&v.color)
        .execute(&mut *tx).await?;
    if inserted.rows_affected() == 0 {
        return Err(ApiError::bad("已經有同名的分類"));
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "name": name }))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditCategory {
    name: Option<String>,
    icon: Option<String>,
    color: Option<String>,
    hidden: Option<bool>,
}
/// 把某個分類名稱在所有用到的地方換成另一個。預算若撞名則保留目標分類的預算。
async fn rename_everywhere(tx: &mut sqlx::PgConnection, owner: Uuid, from: &str, to: &str) -> Result<()> {
    let removing = to == UNCATEGORIZED;
    let mut steps = vec![
        "UPDATE transactions SET category=$3 WHERE owner_id=$1 AND category=$2",
        "UPDATE source_tx_labels SET category=$3 WHERE owner_id=$1 AND category=$2",
        "UPDATE recurring_entries SET category=$3 WHERE owner_id=$1 AND category=$2",
        "UPDATE subscriptions SET category=$3 WHERE owner_id=$1 AND category=$2",
        "UPDATE source_records SET auto_category=$3 WHERE owner_id=$1 AND kind='transaction' AND auto_category=$2",
    ];
    if removing {
        // 移到未分類：規則與預算沒有意義，直接刪除
        steps.push("DELETE FROM category_rules WHERE owner_id=$1 AND category=$2 AND $3<>''");
        steps.push("DELETE FROM budgets WHERE owner_id=$1 AND category=$2 AND $3<>''");
    } else {
        steps.push("UPDATE category_rules SET category=$3 WHERE owner_id=$1 AND category=$2");
        steps.push("DELETE FROM budgets b WHERE owner_id=$1 AND category=$2 AND EXISTS(SELECT 1 FROM budgets o WHERE o.owner_id=b.owner_id AND o.month=b.month AND o.currency=b.currency AND o.category=$3)");
        steps.push("UPDATE budgets SET category=$3 WHERE owner_id=$1 AND category=$2");
    }
    for sql in steps {
        sqlx::query(sql).bind(owner).bind(from).bind(to).execute(&mut *tx).await?;
    }
    Ok(())
}

/// PATCH /categories/{id}：改圖示、顏色、隱藏；自訂分類可改名並同步更新既有資料。
pub async fn edit(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(v): Json<EditCategory>,
) -> Result<Json<Value>> {
    if let Some(i) = &v.icon {
        check_icon(i)?;
    }
    if let Some(c) = &v.color {
        check_color(c)?;
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let row: Option<(String, bool)> = sqlx::query_as("SELECT name,builtin FROM user_categories WHERE id=$1 AND owner_id=$2 FOR UPDATE")
        .bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    let (old, builtin) = row.ok_or_else(ApiError::missing)?;
    let name = match v.name.as_deref() {
        Some(n) => check_name(n)?,
        None => old.clone(),
    };
    if name != old {
        if builtin {
            return Err(ApiError::bad("內建分類供自動分類使用，不能改名；可以換圖示、顏色或隱藏"));
        }
        let taken: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_categories WHERE owner_id=$1 AND name=$2)")
            .bind(user.id).bind(&name).fetch_one(&mut *tx).await?;
        if taken {
            return Err(ApiError::bad("已經有同名的分類"));
        }
        rename_everywhere(&mut tx, user.id, &old, &name).await?;
    }
    sqlx::query("UPDATE user_categories SET name=$3,icon=coalesce($4,icon),color=coalesce($5,color),hidden=coalesce($6,hidden) WHERE id=$1 AND owner_id=$2")
        .bind(id).bind(user.id).bind(&name).bind(&v.icon).bind(&v.color).bind(v.hidden)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id, "name": name })))
}

#[derive(Deserialize)]
pub struct Reorder {
    ids: Vec<Uuid>,
}
/// PUT /categories/order：依傳入順序重排（只影響自己的分類）。
pub async fn reorder(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<Reorder>,
) -> Result<StatusCode> {
    if v.ids.len() > 200 {
        return Err(ApiError::bad("分類太多"));
    }
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    sqlx::query("UPDATE user_categories c SET position=o.i FROM unnest($2::uuid[]) WITH ORDINALITY AS o(id,i) WHERE c.id=o.id AND c.owner_id=$1")
        .bind(user.id).bind(&v.ids).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct DeleteQuery {
    /// 既有交易改歸到哪個分類；未給則歸到「未分類」。
    move_to: Option<String>,
}
/// DELETE /categories/{id}：刪除自訂分類，既有資料移到指定分類。
pub async fn delete(
    user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<DeleteQuery>,
) -> Result<StatusCode> {
    let mut tx = tenant(&s.db, user.id, &user.email).await?;
    let row: Option<(String, bool)> = sqlx::query_as("SELECT name,builtin FROM user_categories WHERE id=$1 AND owner_id=$2 FOR UPDATE")
        .bind(id).bind(user.id).fetch_optional(&mut *tx).await?;
    let (name, builtin) = row.ok_or_else(ApiError::missing)?;
    if builtin {
        return Err(ApiError::bad("內建分類不能刪除，可以改成隱藏"));
    }
    let target = q.move_to.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(UNCATEGORIZED).to_owned();
    if target == name {
        return Err(ApiError::bad("不能移到要刪除的分類"));
    }
    if target != UNCATEGORIZED {
        let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_categories WHERE owner_id=$1 AND name=$2)")
            .bind(user.id).bind(&target).fetch_one(&mut *tx).await?;
        if !ok {
            return Err(ApiError::bad("要移過去的分類不存在"));
        }
    }
    rename_everywhere(&mut tx, user.id, &name, &target).await?;
    sqlx::query("DELETE FROM user_categories WHERE id=$1 AND owner_id=$2")
        .bind(id).bind(user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_fields() {
        assert!(check_name(" 婚禮 ").is_ok());
        assert!(check_name("未分類").is_err());
        assert!(check_name("").is_err());
        assert!(check_icon("shopping-bag").is_ok());
        assert!(check_icon("Bad Icon").is_err());
        assert!(check_color("c1").is_ok());
        assert!(check_color("c12").is_ok());
        assert!(check_color("c0").is_err());
        assert!(check_color("c13").is_err());
        assert!(check_color("c01").is_err());
        assert!(check_kind("flow").is_ok());
        assert!(check_kind("other").is_err());
    }
    #[test]
    fn builtin_names_are_unique() {
        let mut names: Vec<_> = BUILTIN.iter().map(|b| b.0).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), BUILTIN.len());
    }
}
