use crate::error::{ApiError, Result};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::RngCore;
use serde_json::Value;
use uuid::Uuid;

fn cipher() -> Result<Aes256Gcm> {
    let key = std::env::var("CONFIG_ENCRYPTION_KEY")
        .map_err(|_| ApiError::bad("本機加密金鑰尚未設定"))?;
    if key.len() != 64 || !key.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::bad("本機加密金鑰格式錯誤"));
    }
    let bytes: Vec<u8> = (0..64)
        .step_by(2)
        .map(|i| u8::from_str_radix(&key[i..i + 2], 16))
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| ApiError::bad("本機加密金鑰格式錯誤"))?;
    Aes256Gcm::new_from_slice(&bytes).map_err(|_| ApiError::bad("本機加密金鑰格式錯誤"))
}
fn aad(owner: Uuid, id: Uuid, purpose: &str) -> String {
    format!("money:v1:{owner}:{id}:{purpose}")
}
pub fn seal(owner: Uuid, id: Uuid, purpose: &str, value: &Value) -> Result<String> {
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let plain = serde_json::to_vec(value).map_err(|_| ApiError::bad("資料加密失敗"))?;
    let payload = cipher()?
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &plain,
                aad: aad(owner, id, purpose).as_bytes(),
            },
        )
        .map_err(|_| ApiError::bad("資料加密失敗"))?;
    let mut output = nonce.to_vec();
    output.extend(payload);
    Ok(format!("v1:{}", STANDARD.encode(output)))
}
pub fn open(owner: Uuid, id: Uuid, purpose: &str, value: &str) -> Result<Value> {
    let bytes = STANDARD
        .decode(
            value
                .strip_prefix("v1:")
                .ok_or_else(|| ApiError::bad("加密資料版本不符"))?,
        )
        .map_err(|_| ApiError::bad("加密資料損毀"))?;
    if bytes.len() < 28 {
        return Err(ApiError::bad("加密資料損毀"));
    }
    let nonce: [u8; 12] = bytes[..12]
        .try_into()
        .map_err(|_| ApiError::bad("加密資料損毀"))?;
    let plain = cipher()?
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &bytes[12..],
                aad: aad(owner, id, purpose).as_bytes(),
            },
        )
        .map_err(|_| ApiError::bad("資料解密驗證失敗"))?;
    serde_json::from_slice(&plain).map_err(|_| ApiError::bad("解密資料格式錯誤"))
}
