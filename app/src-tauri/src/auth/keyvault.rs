//! On-disk password-protected identity vault.
//!
//! Stored as a plaintext JSON file at `app_data_dir/identity.json`. Must be
//! plaintext because the login flow has to read it *before* the SQLCipher DB
//! is unlocked. Contains only login material:
//!   - Argon2id verifier for the user's password (detects wrong password
//!     before any decryption work).
//!   - `kek_salt` + `mk_nonce` + `mk_wrapped`: the random 256-bit master key
//!     encrypted with a key derived from the password via Argon2id.
//!
//! The master key itself never touches disk in plaintext. It lives inside the
//! opened SQLCipher connection for the lifetime of the signed-in session and
//! is dropped on logout.
//!
//! Multiple users on one local DB is a future feature — the file format
//! already carries a `users` list where each entry wraps the *same* master
//! key under a different user's password.

use std::{
    fs,
    path::{Path, PathBuf},
};

use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::{
    crypto::{cipher, kdf, password},
    error::{AppError, AppResult},
};

pub const CURRENT_VERSION: u32 = 1;
pub const MASTER_KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;

#[derive(Debug, Serialize, Deserialize)]
pub struct Identity {
    pub version: u32,
    pub users: Vec<UserCredential>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserCredential {
    pub user_id: String,
    pub firm_id: String,
    pub email: String,
    pub display_name: String,
    pub argon2_hash: String,
    pub kek_salt: String,
    pub mk_nonce: String,
    pub mk_wrapped: String,
}

pub fn path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("identity.json")
}

pub fn exists(app_data_dir: &Path) -> bool {
    path(app_data_dir).is_file()
}

pub fn load(app_data_dir: &Path) -> AppResult<Identity> {
    let p = path(app_data_dir);
    let bytes = fs::read(&p)?;
    let identity: Identity = serde_json::from_slice(&bytes)?;
    if identity.version != CURRENT_VERSION {
        return Err(AppError::Message(format!(
            "unsupported identity file version {}",
            identity.version
        )));
    }
    Ok(identity)
}

pub fn save(app_data_dir: &Path, identity: &Identity) -> AppResult<()> {
    let p = path(app_data_dir);
    let serialised = serde_json::to_vec_pretty(identity)?;
    fs::write(&p, serialised)?;
    Ok(())
}

pub fn find_by_email<'a>(
    identity: &'a Identity,
    email: &str,
) -> Option<&'a UserCredential> {
    identity
        .users
        .iter()
        .find(|u| u.email.eq_ignore_ascii_case(email))
}

/// Generates a fresh identity with one user. Returns the random master key so
/// the caller can key the SQLCipher connection. Persisting the identity file
/// is the caller's responsibility (so onboard can do DB setup and file write
/// in the order it prefers — in practice we save after the DB is successfully
/// keyed and migrated, to avoid an orphan identity file).
pub fn create_first_user(
    user_id: &str,
    firm_id: &str,
    email: &str,
    display_name: &str,
    password_input: &str,
) -> AppResult<(Identity, [u8; MASTER_KEY_LEN])> {
    let argon2_hash = password::hash(password_input)?;

    let mut master_key = [0u8; MASTER_KEY_LEN];
    rand::thread_rng().fill_bytes(&mut master_key);

    let kek_salt = kdf::random_salt();
    let mut kek = [0u8; MASTER_KEY_LEN];
    kdf::derive_key(password_input.as_bytes(), &kek_salt, &mut kek)?;

    let mk_nonce = cipher::generate_nonce();
    let mk_wrapped = cipher::encrypt(&kek, &mk_nonce, &master_key)?;

    let identity = Identity {
        version: CURRENT_VERSION,
        users: vec![UserCredential {
            user_id: user_id.to_string(),
            firm_id: firm_id.to_string(),
            email: email.to_string(),
            display_name: display_name.to_string(),
            argon2_hash,
            kek_salt: hex::encode(kek_salt),
            mk_nonce: hex::encode(mk_nonce),
            mk_wrapped: hex::encode(&mk_wrapped),
        }],
    };

    Ok((identity, master_key))
}

/// Verifies the password against the stored Argon2 hash, then re-derives the
/// KEK and unwraps the master key. Verify-first gives a clean "wrong
/// password" error before we touch the AEAD; a tag failure after a passing
/// verify therefore signals file corruption, not a bad password.
pub fn unlock(
    credential: &UserCredential,
    password_input: &str,
) -> AppResult<[u8; MASTER_KEY_LEN]> {
    if !password::verify(password_input, &credential.argon2_hash)? {
        return Err(AppError::Unauthorised("invalid email or password".into()));
    }

    let kek_salt_bytes = hex::decode(&credential.kek_salt)
        .map_err(|e| AppError::Crypto(format!("kek_salt hex: {e}")))?;
    let kek_salt: [u8; SALT_LEN] = kek_salt_bytes
        .try_into()
        .map_err(|_| AppError::Crypto("kek_salt length".into()))?;

    let mk_nonce_bytes = hex::decode(&credential.mk_nonce)
        .map_err(|e| AppError::Crypto(format!("mk_nonce hex: {e}")))?;
    let mk_nonce: [u8; cipher::NONCE_LEN] = mk_nonce_bytes
        .try_into()
        .map_err(|_| AppError::Crypto("mk_nonce length".into()))?;

    let mk_wrapped = hex::decode(&credential.mk_wrapped)
        .map_err(|e| AppError::Crypto(format!("mk_wrapped hex: {e}")))?;

    let mut kek = [0u8; MASTER_KEY_LEN];
    kdf::derive_key(password_input.as_bytes(), &kek_salt, &mut kek)?;

    let mk_bytes = cipher::decrypt(&kek, &mk_nonce, &mk_wrapped).map_err(|_| {
        AppError::Crypto("identity file integrity check failed".into())
    })?;
    let master_key: [u8; MASTER_KEY_LEN] = mk_bytes
        .try_into()
        .map_err(|_| AppError::Crypto("master key length".into()))?;

    Ok(master_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_unlock_returns_same_master_key() {
        let (identity, mk) = create_first_user(
            "user-1",
            "firm-1",
            "simba@example.com",
            "Simba G.",
            "correct horse battery staple",
        )
        .unwrap();

        let cred = &identity.users[0];
        let recovered = unlock(cred, "correct horse battery staple").unwrap();
        assert_eq!(mk, recovered);
    }

    #[test]
    fn unlock_rejects_wrong_password() {
        let (identity, _) = create_first_user(
            "user-1",
            "firm-1",
            "simba@example.com",
            "Simba G.",
            "correct horse battery staple",
        )
        .unwrap();

        let cred = &identity.users[0];
        let err = unlock(cred, "stapler horse battery").unwrap_err();
        assert!(matches!(err, AppError::Unauthorised(_)));
    }

    #[test]
    fn two_identities_have_different_master_keys() {
        let (_, mk1) = create_first_user(
            "u1",
            "f1",
            "a@x.com",
            "A",
            "correct horse battery staple",
        )
        .unwrap();
        let (_, mk2) = create_first_user(
            "u2",
            "f2",
            "b@x.com",
            "B",
            "correct horse battery staple",
        )
        .unwrap();
        assert_ne!(mk1, mk2, "each new identity gets a unique random master key");
    }
}
