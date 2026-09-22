//! 個人資料：顯示名稱與頭像。
//!
//! - 顯示名稱留空時使用 Google 名稱；Google 登入只更新 `name`，不會覆蓋 `display_name`。
//! - 頭像 `avatar`：`{"type":"initial","color"}`、`{"type":"emoji","emoji","color"}`、
//!   `{"type":"photo","v"}`。照片由瀏覽器先裁切縮小，伺服器只接受 PNG／JPEG／WebP
//!   （檢查檔頭），上限 300KB；不接受 SVG，避免夾帶程式碼。
use crate::{
    auth::Identity,
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

const MAX_PHOTO: usize = 300_000;

fn check_color(c: &str) -> Result<()> {
    let ok = c
        .strip_prefix('c')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=12).contains(&n))
        && !c.starts_with("c0");
    if ok { Ok(()) } else { Err(ApiError::bad("顏色格式錯誤")) }
}
/// 表情符號：1～8 個 Unicode 字元（含組合字），不可含英數與控制字元。
fn check_emoji(e: &str) -> Result<()> {
    let n = e.chars().count();
    let ok = (1..=8).contains(&n)
        && e.len() <= 32
        && !e.chars().any(|c| c.is_ascii_alphanumeric() || c.is_control() || c.is_whitespace());
    if ok { Ok(()) } else { Err(ApiError::bad("請選擇一個表情符號")) }
}
/// 依檔頭判斷圖片格式；只接受 PNG、JPEG、WebP。
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

async fn load(s: &AppState, id: Uuid) -> Result<Value> {
    let v: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'email',email,'google_name',name,'display_name',display_name,'name',coalesce(display_name,name),'avatar',avatar) FROM users WHERE id=$1")
        .bind(id)
        .fetch_optional(&s.auth_db)
        .await?;
    v.ok_or_else(ApiError::missing)
}

/// GET /profile
pub async fn get(user: Identity, State(s): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(load(&s, user.id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Avatar {
    #[serde(rename = "type")]
    kind: String,
    emoji: Option<String>,
    color: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    /// 空字串代表改回 Google 名稱
    display_name: Option<String>,
    avatar: Option<Avatar>,
}
/// PATCH /profile：改顯示名稱，或改成表情／字母頭像（照片用 PUT /profile/photo）。
pub async fn update(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<Update>,
) -> Result<Json<Value>> {
    let mut tx = s.auth_db.begin().await?;
    if let Some(name) = v.display_name.as_deref() {
        let name = name.trim();
        if name.chars().count() > 40 || name.chars().any(char::is_control) {
            return Err(ApiError::bad("顯示名稱最多 40 個字"));
        }
        sqlx::query("UPDATE users SET display_name=nullif($2,'') WHERE id=$1")
            .bind(user.id)
            .bind(name)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(a) = v.avatar {
        let color = a.color.as_deref().unwrap_or("c7");
        check_color(color)?;
        let value = match a.kind.as_str() {
            "initial" => json!({"type": "initial", "color": color}),
            "emoji" => {
                let e = a.emoji.as_deref().unwrap_or("").trim();
                check_emoji(e)?;
                json!({"type": "emoji", "emoji": e, "color": color})
            }
            _ => return Err(ApiError::bad("頭像類型錯誤")),
        };
        sqlx::query("UPDATE users SET avatar=$2,avatar_photo=NULL,avatar_mime=NULL WHERE id=$1")
            .bind(user.id)
            .bind(value)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(load(&s, user.id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Photo {
    /// data:image/webp;base64,...
    data: String,
}
/// PUT /profile/photo：上傳頭像照片。
pub async fn photo(
    user: Identity,
    State(s): State<AppState>,
    Json(v): Json<Photo>,
) -> Result<Json<Value>> {
    let b64 = v
        .data
        .split_once(";base64,")
        .filter(|(head, _)| head.starts_with("data:image/"))
        .map(|(_, b)| b)
        .ok_or_else(|| ApiError::bad("圖片格式錯誤"))?;
    if b64.len() > MAX_PHOTO * 4 / 3 + 8 {
        return Err(ApiError::bad("圖片太大，請換一張較小的照片"));
    }
    let bytes = STANDARD
        .decode(b64)
        .map_err(|_| ApiError::bad("圖片格式錯誤"))?;
    if bytes.len() > MAX_PHOTO {
        return Err(ApiError::bad("圖片太大，請換一張較小的照片"));
    }
    let mime = sniff(&bytes).ok_or_else(|| ApiError::bad("只支援 PNG、JPEG、WebP 圖片"))?;
    let version = &Uuid::new_v4().simple().to_string()[..8];
    sqlx::query("UPDATE users SET avatar_photo=$2,avatar_mime=$3,avatar=jsonb_build_object('type','photo','v',$4::text) WHERE id=$1")
        .bind(user.id)
        .bind(&bytes)
        .bind(mime)
        .bind(version)
        .execute(&s.auth_db)
        .await?;
    Ok(Json(load(&s, user.id).await?))
}

/// GET /users/{id}/avatar：頭像照片。只需登入；其他人的照片也可讀，
/// 因為成員頁、家庭頁會顯示對方頭像（不含任何財務資料）。
pub async fn avatar(
    _user: Identity,
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    let row: Option<(Option<Vec<u8>>, Option<String>)> =
        sqlx::query_as("SELECT avatar_photo,avatar_mime FROM users WHERE id=$1")
            .bind(id)
            .fetch_optional(&s.auth_db)
            .await?;
    let Some((Some(bytes), Some(mime))) = row else {
        return Err(ApiError::missing());
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_avatar_inputs() {
        assert!(check_emoji("🐱").is_ok());
        assert!(check_emoji("👨‍👩‍👧").is_ok());
        assert!(check_emoji("a").is_err());
        assert!(check_emoji("").is_err());
        assert!(check_emoji("<script>").is_err());
        assert!(check_color("c12").is_ok());
        assert!(check_color("c13").is_err());
    }
    #[test]
    fn sniffs_only_raster_images() {
        assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]), Some("image/png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"<svg xmlns"), None);
    }
}
