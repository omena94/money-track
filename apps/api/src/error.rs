use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub &'static str);
impl ApiError {
    pub fn bad(message: &'static str) -> Self {
        Self(StatusCode::BAD_REQUEST, message)
    }
    pub fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "請先使用 Google 登入")
    }
    pub fn forbidden() -> Self {
        Self(StatusCode::FORBIDDEN, "此操作未取得授權")
    }
    pub fn missing() -> Self {
        Self(StatusCode::NOT_FOUND, "找不到資料")
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        if let Some(db) = e.as_database_error() {
            if db.is_unique_violation() {
                return Self(StatusCode::CONFLICT, "資料已存在，請重新整理");
            }
        }
        tracing::error!(kind="database",error_code=?e.as_database_error().and_then(|d|d.code()),"Database operation failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "資料處理失敗，請稍後再試",
        )
    }
}
impl From<reqwest::Error> for ApiError {
    fn from(_: reqwest::Error) -> Self {
        Self(StatusCode::BAD_GATEWAY, "外部服務暫時未回應")
    }
}
pub type Result<T> = std::result::Result<T, ApiError>;
