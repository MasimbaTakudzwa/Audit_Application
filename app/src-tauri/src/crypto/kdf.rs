use argon2::{Algorithm, Argon2, Params, Version};

use crate::error::{AppError, AppResult};

/// Derive a 32-byte key from a password and salt using Argon2id with
/// reference defaults (m=19456 KiB, t=2, p=1). Parameters can be tuned
/// upwards as target hardware permits; this is the floor.
pub fn derive_key(password: &[u8], salt: &[u8], out: &mut [u8; 32]) -> AppResult<()> {
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default());
    argon
        .hash_password_into(password, salt, out)
        .map_err(|e| AppError::Crypto(format!("argon2id: {e}")))
}

pub fn random_salt() -> [u8; 16] {
    use rand::RngCore;
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_for_same_input() {
        let password = b"correct horse battery staple";
        let salt = [0u8; 16];
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        derive_key(password, &salt, &mut a).unwrap();
        derive_key(password, &salt, &mut b).unwrap();
        assert_eq!(a, b);
    }
}
