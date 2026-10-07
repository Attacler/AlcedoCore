use std::sync::OnceLock;

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rand::RngCore;

use crate::services::errors::AlcedoError;

static ENCRYPTION_KEY: OnceLock<[u8; 32]> = OnceLock::new();

/// Key material for registry credentials, from `REGISTRY_ENCRYPTION_KEY`
/// (base64-encoded 32 bytes). Read lazily so the key is only required when a
/// registry with a password is written or read back — list/get never touch it.
fn get_key() -> Result<&'static [u8; 32], AlcedoError> {
    if let Some(key) = ENCRYPTION_KEY.get() {
        return Ok(key);
    }
    let encoded = std::env::var("REGISTRY_ENCRYPTION_KEY").map_err(|_| {
        AlcedoError::SystemError(
            "REGISTRY_ENCRYPTION_KEY environment variable not set".to_string(),
            0,
        )
    })?;
    let decoded = BASE64.decode(encoded.as_bytes()).map_err(|e| {
        AlcedoError::SystemError(format!("Invalid REGISTRY_ENCRYPTION_KEY: {}", e), 0)
    })?;
    let key: [u8; 32] = decoded.try_into().map_err(|_| {
        AlcedoError::SystemError(
            "REGISTRY_ENCRYPTION_KEY must be 32 bytes (base64-encoded)".to_string(),
            0,
        )
    })?;
    let _ = ENCRYPTION_KEY.set(key);
    Ok(ENCRYPTION_KEY.get().unwrap())
}

/// AES-256-GCM: 12-byte random nonce prepended to the ciphertext, base64-encoded.
/// Format-compatible with v1 so existing ciphertext/keys keep working.
pub fn encrypt(plaintext: &str) -> Result<String, AlcedoError> {
    let key = get_key()?;
    let cipher = Aes256Gcm::new_from_slice(key.as_slice())
        .map_err(|e| AlcedoError::SystemError(format!("Cipher init failed: {}", e), 0))?;

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| AlcedoError::SystemError(format!("Encryption failed: {}", e), 0))?;

    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(&combined))
}

pub fn decrypt(encoded: &str) -> Result<String, AlcedoError> {
    let key = get_key()?;
    let cipher = Aes256Gcm::new_from_slice(key.as_slice())
        .map_err(|e| AlcedoError::SystemError(format!("Cipher init failed: {}", e), 0))?;

    let combined = BASE64
        .decode(encoded.as_bytes())
        .map_err(|e| AlcedoError::SystemError(format!("Base64 decode failed: {}", e), 0))?;

    if combined.len() < 12 {
        return Err(AlcedoError::SystemError(
            "Invalid ciphertext".to_string(),
            0,
        ));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| AlcedoError::SystemError(format!("Decryption failed: {}", e), 0))?;

    String::from_utf8(plaintext)
        .map_err(|e| AlcedoError::SystemError(format!("UTF-8 decode failed: {}", e), 0))
}

#[cfg(test)]
mod tests {
    use super::{decrypt, encrypt};

    // The key is process-global, so tests must agree on it. Set before first use.
    fn set_key() {
        unsafe {
            std::env::set_var(
                "REGISTRY_ENCRYPTION_KEY",
                "UuG5d1HEJuBH0tElr6R4I3deq/BabTiHhfByFS9xom4=",
            );
        }
    }

    #[test]
    fn round_trips() {
        set_key();
        let ciphertext = encrypt("s3cret").unwrap();
        assert_ne!(ciphertext, "s3cret");
        assert_eq!(decrypt(&ciphertext).unwrap(), "s3cret");
    }

    #[test]
    fn tampered_ciphertext_fails() {
        set_key();
        let ciphertext = encrypt("s3cret").unwrap();
        let mut bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            ciphertext.as_bytes(),
        )
        .unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let tampered = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
        assert!(decrypt(&tampered).is_err());
    }
}
