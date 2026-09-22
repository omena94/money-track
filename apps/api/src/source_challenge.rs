use serde_json::{Value, json};
use uuid::Uuid;

/// Only public presentation fields cross from the private worker into jobs API.
pub fn sanitize(c: &Value) -> Option<Value> {
    let kind = c["kind"].as_str()?;
    if !matches!(kind, "otp" | "captcha" | "choice") || c.to_string().len() > 512 * 1024 {
        return None;
    }
    let message = c["message"].as_str().unwrap_or("請完成本人驗證");
    if message.chars().count() > 1000 { return None; }
    if !c["channel"].is_null() && !matches!(c["channel"].as_str(), Some("sms" | "email" | "device")) { return None; }
    let mut result = json!({"id":Uuid::new_v4(),"kind":kind,"message":message,"channel":c["channel"]});
    if kind == "choice" {
        let choices = c["choices"].as_array()?;
        if choices.is_empty() || choices.len() > 4 { return None; }
        let mut seen = std::collections::HashSet::new();
        let mut cleaned = Vec::new();
        for choice in choices {
            let value = choice["value"].as_str()?;
            let label = choice["label"].as_str()?;
            if value.is_empty() || value.len() > 32 || !value.bytes().all(|b| b.is_ascii_lowercase() || b == b'_' || b == b'-')
                || label.is_empty() || label.chars().count() > 80 || !seen.insert(value) { return None; }
            cleaned.push(json!({"value":value,"label":label}));
        }
        result["choices"] = json!(cleaned);
    }
    if let Some(expiry) = c["expires_at"].as_str() {
        chrono::DateTime::parse_from_rfc3339(expiry).ok()?;
        result["expires_at"] = json!(expiry);
    }
    if let Some(img) = c["image"].as_str() {
        if !(img.starts_with("data:image/png;base64,") || img.starts_with("data:image/jpeg;base64,") || img.starts_with("data:image/gif;base64,")) { return None; }
        result["image"] = json!(img);
    }
    Some(result)
}

pub fn validate_answer(c: &Value, answer: &str) -> std::result::Result<(), &'static str> {
    if !matches!(c["kind"].as_str(), Some("otp" | "captcha" | "choice")) {
        return Err("此驗證方式已移除，請取消後重新同步");
    }
    if let Some(expiry) = c["expires_at"].as_str() {
        let deadline = chrono::DateTime::parse_from_rfc3339(expiry).map_err(|_| "驗證期限格式錯誤")?;
        if deadline <= chrono::Utc::now() { return Err("驗證碼已過期，請取消後重新同步"); }
    }
    if c["kind"] == "choice" {
        if !c["choices"].as_array().is_some_and(|options| options.iter().any(|option| option["value"].as_str() == Some(answer))) {
            return Err("請選擇本次提供的驗證選項");
        }
    } else if answer.trim().is_empty() {
        return Err("請輸入驗證碼");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choice_only_exposes_labels_and_rejects_unoffered_answers() {
        let c = sanitize(&json!({"kind":"choice","message":"選擇方式","secret":"private","choices":[{"value":"sms","label":"手機簡訊","token":"private"},{"value":"email","label":"Email"}]})).unwrap();
        assert!(!c.to_string().contains("private"));
        assert!(validate_answer(&c, "email").is_ok());
        assert!(validate_answer(&c, "trust").is_err());
        assert!(validate_answer(&c, "").is_err());
        assert!(sanitize(&json!({"kind":"choice","choices":[{"value":"sms","label":"簡訊"},{"value":"sms","label":"重複"}]})).is_none());
    }
    #[test]
    fn expired_otp_and_retired_browser_challenges_are_rejected() {
        assert!(validate_answer(&json!({"kind":"otp","expires_at":"2000-01-01T00:00:00Z"}), "123456").is_err());
        assert!(sanitize(&json!({"kind":"browser"})).is_none());
        assert!(validate_answer(&json!({"kind":"browser"}), "continue").is_err());
        assert!(validate_answer(&json!({"kind":"otp"}), "").is_err());
        assert!(sanitize(&json!({"kind":"otp","expires_at":"invalid"})).is_none());
    }
}
