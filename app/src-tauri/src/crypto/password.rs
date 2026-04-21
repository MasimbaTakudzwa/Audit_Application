//! Password hashing and verification using Argon2id in the PHC string format.
//!
//! Separate from `crypto::kdf`: that module derives raw key material from
//! passwords for file/DB encryption. This one produces verifier strings suitable
//! for storing in the `User.argon2_hash` column.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand::rngs::OsRng;

use crate::error::{AppError, AppResult};

pub fn hash(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Crypto(format!("argon2 hash: {e}")))
}

pub fn verify(password: &str, phc: &str) -> AppResult<bool> {
    let parsed =
        PasswordHash::new(phc).map_err(|e| AppError::Crypto(format!("argon2 parse: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_correct_password() {
        let h = hash("correct horse battery staple").unwrap();
        assert!(verify("correct horse battery staple", &h).unwrap());
    }

    #[test]
    fn rejects_wrong_password() {
        let h = hash("correct horse battery staple").unwrap();
        assert!(!verify("stapler horse battery", &h).unwrap());
    }

    #[test]
    fn two_hashes_of_same_password_differ() {
        let a = hash("same").unwrap();
        let b = hash("same").unwrap();
        assert_ne!(a, b, "random salt should produce different PHC strings");
    }
}
