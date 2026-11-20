// Password hashing, opaque token generation and cookie helpers.
#![allow(dead_code)]

use argon2::Argon2;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use sha2::{Digest, Sha256};

const TOKEN_BYTES: usize = 32;

/// Hash a password with Argon2id. Returns a PHC string safe to store.
pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

/// Verify a password against a stored PHC hash.
pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    PasswordHash::new(stored_hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Random opaque token, hex encoded. Shared with the user exactly once.
pub fn generate_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Only the SHA-256 hash of a token is persisted, so a DB leak cannot mint sessions.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Extract the session token from the `Cookie` header.
pub fn session_token_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == crate::middleware::SESSION_COOKIE && !value.is_empty()).then(|| value.to_string())
    })
}

/// Build the `Set-Cookie` value for a session. `max_age_secs = 0` clears it.
pub fn session_cookie(token: &str, max_age_secs: i64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure_flag}",
        crate::middleware::SESSION_COOKIE
    )
}

/// Cookies require `Secure` everywhere except local development over plain HTTP.
pub fn cookie_secure() -> bool {
    let host = crate::config::parameter::get("APP_URL").unwrap_or_default();
    !(host.starts_with("127.0.0.1") || host.starts_with("localhost") || host == "0.0.0.0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn tokens_are_unique_and_hashed() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
        assert_eq!(hash_token(&a), hash_token(&a));
        assert_ne!(hash_token(&a), hash_token(&b));
        assert_eq!(hash_token(&a).len(), 64);
    }
}
