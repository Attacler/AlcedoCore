use std::sync::OnceLock;

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rand::RngCore;

use crate::services::errors::AlcedoError;

static ENCRYPTION_KEY: OnceLock<[u8; 32]> = OnceLock::new();
static PLUGIN_ACCESS_KEY: OnceLock<[u8; 32]> = OnceLock::new();

/// Loads a base64-encoded 32-byte key from `var` into `cache`, lazily, so the
/// key is only required when a value under it is written or read back —
/// list/get never touch it.
fn load_key(
    var: &str,
    cache: &'static OnceLock<[u8; 32]>,
) -> Result<&'static [u8; 32], AlcedoError> {
    if let Some(key) = cache.get() {
        return Ok(key);
    }
    let encoded = std::env::var(var)
        .map_err(|_| AlcedoError::SystemError(format!("{var} environment variable not set"), 0))?;
    let decoded = BASE64
        .decode(encoded.as_bytes())
        .map_err(|e| AlcedoError::SystemError(format!("Invalid {var}: {e}"), 0))?;
    let key: [u8; 32] = decoded
        .try_into()
        .map_err(|_| AlcedoError::SystemError(format!("{var} must be 32 bytes (base64-encoded)"), 0))?;
    let _ = cache.set(key);
    Ok(cache.get().unwrap())
}

/// Key material for registry credentials, from `REGISTRY_ENCRYPTION_KEY`.
fn get_key() -> Result<&'static [u8; 32], AlcedoError> {
    load_key("REGISTRY_ENCRYPTION_KEY", &ENCRYPTION_KEY)
}

/// Key material for plugin DB-access passwords, from
/// `PLUGIN_ACCESS_ENCRYPTION_KEY`. Separate from the registry key so the two
/// credential classes rotate independently.
pub fn get_plugin_access_key() -> Result<&'static [u8; 32], AlcedoError> {
    load_key("PLUGIN_ACCESS_ENCRYPTION_KEY", &PLUGIN_ACCESS_KEY)
}

/// AES-256-GCM: 12-byte random nonce prepended to the ciphertext, base64-encoded.
/// Format-compatible with v1 so existing ciphertext/keys keep working.
pub fn encrypt_with(plaintext: &str, key: &[u8; 32]) -> Result<String, AlcedoError> {
    let cipher = Aes256Gcm::new_from_slice(key.as_slice())
        .map_err(|e| AlcedoError::SystemError(format!("Cipher init failed: {e}"), 0))?;

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| AlcedoError::SystemError(format!("Encryption failed: {e}"), 0))?;

    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(&combined))
}

pub fn decrypt_with(encoded: &str, key: &[u8; 32]) -> Result<String, AlcedoError> {
    let cipher = Aes256Gcm::new_from_slice(key.as_slice())
        .map_err(|e| AlcedoError::SystemError(format!("Cipher init failed: {e}"), 0))?;

    let combined = BASE64
        .decode(encoded.as_bytes())
        .map_err(|e| AlcedoError::SystemError(format!("Base64 decode failed: {e}"), 0))?;

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
        .map_err(|e| AlcedoError::SystemError(format!("Decryption failed: {e}"), 0))?;

    String::from_utf8(plaintext)
        .map_err(|e| AlcedoError::SystemError(format!("UTF-8 decode failed: {e}"), 0))
}

/// Registry-credential encryption (uses `REGISTRY_ENCRYPTION_KEY`).
pub fn encrypt(plaintext: &str) -> Result<String, AlcedoError> {
    encrypt_with(plaintext, get_key()?)
}

pub fn decrypt(encoded: &str) -> Result<String, AlcedoError> {
    decrypt_with(encoded, get_key()?)
}

#[cfg(test)]
mod tests {
    use super::{decrypt, decrypt_with, encrypt, encrypt_with, get_key, get_plugin_access_key};

    // The keys are process-global, so tests must agree on them. Set before first use.
    fn set_keys() {
        unsafe {
            std::env::set_var(
                "REGISTRY_ENCRYPTION_KEY",
                "UuG5d1HEJuBH0tElr6R4I3deq/BabTiHhfByFS9xom4=",
            );
            std::env::set_var(
                "PLUGIN_ACCESS_ENCRYPTION_KEY",
                "BuCUYpRBM9tByB6vwG/1uOYBCc4GzZ0OBL1BoOtkaWI=",
            );
        }
    }

    #[test]
    fn round_trips() {
        set_keys();
        let ciphertext = encrypt("s3cret").unwrap();
        assert_ne!(ciphertext, "s3cret");
        assert_eq!(decrypt(&ciphertext).unwrap(), "s3cret");
    }

    #[test]
    fn tampered_ciphertext_fails() {
        set_keys();
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

    #[test]
    fn plugin_key_round_trips_and_is_distinct_from_registry_key() {
        set_keys();
        let plugin_key = get_plugin_access_key().unwrap();
        let registry_key = get_key().unwrap();
        assert_ne!(plugin_key, registry_key);

        let ct = encrypt_with("db-password", plugin_key).unwrap();
        assert_eq!(decrypt_with(&ct, plugin_key).unwrap(), "db-password");
        // Ciphertext under one key does not decrypt under the other.
        assert!(decrypt_with(&ct, registry_key).is_err());

        // The wrappers keep using the registry key.
        let reg_ct = encrypt("reg-secret").unwrap();
        assert_eq!(decrypt(&reg_ct).unwrap(), "reg-secret");
        assert!(decrypt_with(&reg_ct, plugin_key).is_err());
    }
}
