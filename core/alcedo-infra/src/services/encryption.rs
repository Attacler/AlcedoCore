use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use once_cell::sync::OnceCell;
use rand::rngs::OsRng;
use rand::RngCore;

use crate::error::AppError;

static ENCRYPTION_KEY: OnceCell<[u8; 32]> = OnceCell::new();

fn get_key() -> Result<&'static [u8; 32], AppError> {
    if let Some(key) = ENCRYPTION_KEY.get() {
        return Ok(key);
    }
    let encoded = std::env::var("REGISTRY_ENCRYPTION_KEY").map_err(|_| {
        AppError::Internal("REGISTRY_ENCRYPTION_KEY environment variable not set".to_string())
    })?;
    let decoded = BASE64.decode(encoded.as_bytes()).map_err(|e| {
        AppError::Internal(format!("Invalid REGISTRY_ENCRYPTION_KEY: {}", e))
    })?;
    let key: [u8; 32] = decoded.try_into().map_err(|_| {
        AppError::Internal(
            "REGISTRY_ENCRYPTION_KEY must be 32 bytes (base64-encoded)".to_string(),
        )
    })?;
    let _ = ENCRYPTION_KEY.set(key);
    Ok(ENCRYPTION_KEY.get().unwrap())
}

pub fn encrypt(plaintext: &str) -> Result<String, AppError> {
    let key = get_key()?;
    let cipher = Aes256Gcm::new_from_slice(key.as_slice())
        .map_err(|e| AppError::Internal(format!("Cipher init failed: {}", e)))?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| AppError::Internal(format!("Encryption failed: {}", e)))?;

    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(&combined))
}

pub fn decrypt(encoded: &str) -> Result<String, AppError> {
    let key = get_key()?;
    let cipher = Aes256Gcm::new_from_slice(key.as_slice())
        .map_err(|e| AppError::Internal(format!("Cipher init failed: {}", e)))?;

    let combined = BASE64
        .decode(encoded.as_bytes())
        .map_err(|e| AppError::Internal(format!("Base64 decode failed: {}", e)))?;

    if combined.len() < 12 {
        return Err(AppError::Internal("Invalid ciphertext".to_string()));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| AppError::Internal(format!("Decryption failed: {}", e)))?;

    String::from_utf8(plaintext)
        .map_err(|e| AppError::Internal(format!("UTF-8 decode failed: {}", e)))
}
