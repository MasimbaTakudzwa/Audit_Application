//! Encrypted blob storage on disk.
//!
//! Each engagement has its own 32-byte AES-256-GCM *content key*. The key is
//! generated on the engagement's first blob write, wrapped under the
//! signed-in user's master key, and stashed in the engagement's
//! `KeychainEntry.wrapped_key` column. Subsequent writes rehydrate the key
//! from that row.
//!
//! The encrypted bytes themselves live under
//! `{app_data_dir}/blobs/{engagement_id}/{blob_id_prefix}/{blob_id}.bin`
//! with the GCM auth tag stored separately in `EncryptedBlob.auth_tag`. A
//! disk-side tamper is caught by the tag check even before the AEAD runs.
//!
//! Bytes-on-disk format per file: ciphertext only (no tag, no nonce).
//! Nonce + tag go into the DB — this means restoring a stolen bare `.bin`
//! without the DB yields nothing useful.

use std::{
    fs,
    path::{Path, PathBuf},
};

use rand::RngCore;
use rusqlite::{params, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    crypto::cipher,
    error::{AppError, AppResult},
};

const ENG_KEY_LEN: usize = 32;
const WRAP_NONCE_LEN: usize = cipher::NONCE_LEN;
const GCM_TAG_LEN: usize = cipher::TAG_LEN;

pub struct WrittenBlob {
    pub id: String,
    pub plaintext_size: i64,
    pub sha256_plaintext: String,
}

/// Encrypt `plaintext` under the engagement's content key, write the
/// ciphertext to disk, and insert an `EncryptedBlob` row. Returns the newly
/// assigned blob id plus the plaintext size and sha256 so the caller can
/// populate related tables (`DataImport`, `Evidence`, ...) without rehashing.
///
/// `owning_entity_*` is purely informational — it lets the UI find "blobs
/// belonging to this finding" without a wildcard scan.
pub fn write_engagement_blob(
    tx: &Transaction<'_>,
    app_data_dir: &Path,
    engagement_id: &str,
    owning_entity_type: Option<&str>,
    owning_entity_id: Option<&str>,
    filename: Option<&str>,
    mime_type: Option<&str>,
    plaintext: &[u8],
    master_key: &[u8; ENG_KEY_LEN],
    now: i64,
) -> AppResult<WrittenBlob> {
    let (keychain_id, engagement_key) =
        ensure_engagement_key(tx, engagement_id, master_key)?;

    let mut sha = Sha256::new();
    sha.update(plaintext);
    let sha_hex = hex::encode(sha.finalize());

    let file_nonce = cipher::generate_nonce();
    let mut combined = cipher::encrypt(&engagement_key, &file_nonce, plaintext)?;
    if combined.len() < GCM_TAG_LEN {
        return Err(AppError::Crypto("ciphertext shorter than GCM tag".into()));
    }
    let tag_start = combined.len() - GCM_TAG_LEN;
    let auth_tag: Vec<u8> = combined.split_off(tag_start);
    let ciphertext = combined;

    let blob_id = Uuid::now_v7().to_string();
    let rel_path = relative_blob_path(engagement_id, &blob_id);
    let abs_path = app_data_dir.join(&rel_path);
    if let Some(parent) = abs_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&abs_path, &ciphertext)?;

    let rel_path_str = rel_path.to_string_lossy().to_string();
    tx.execute(
        "INSERT INTO EncryptedBlob (
            id, owning_entity_type, owning_entity_id, filename, mime_type,
            nonce, ciphertext_path, auth_tag, plaintext_size, key_id,
            sha256_plaintext, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            blob_id,
            owning_entity_type,
            owning_entity_id,
            filename,
            mime_type,
            &file_nonce[..],
            rel_path_str,
            &auth_tag[..],
            plaintext.len() as i64,
            keychain_id,
            sha_hex,
            now,
        ],
    )?;

    Ok(WrittenBlob {
        id: blob_id,
        plaintext_size: plaintext.len() as i64,
        sha256_plaintext: sha_hex,
    })
}

/// Decrypt `blob_id` under the session master key and return plaintext.
///
/// Reads the blob row (nonce, ciphertext path, auth tag, key id), reads the
/// ciphertext from disk, rejoins it with the tag, unwraps the engagement
/// content key via `KeychainEntry`, and runs GCM. The on-disk file without
/// the tag is unverifiable on its own — tamper-resistant by construction.
pub fn read_blob(
    tx: &Transaction<'_>,
    app_data_dir: &Path,
    blob_id: &str,
    master_key: &[u8; ENG_KEY_LEN],
) -> AppResult<Vec<u8>> {
    let (nonce, rel_path, auth_tag, keychain_id): (Vec<u8>, String, Vec<u8>, String) = tx
        .query_row(
            "SELECT nonce, ciphertext_path, auth_tag, key_id
             FROM EncryptedBlob WHERE id = ?1",
            params![blob_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("blob {blob_id}")))?;

    if nonce.len() != cipher::NONCE_LEN {
        return Err(AppError::Crypto("blob nonce length".into()));
    }
    if auth_tag.len() != GCM_TAG_LEN {
        return Err(AppError::Crypto("blob auth tag length".into()));
    }
    let nonce_arr: [u8; cipher::NONCE_LEN] = nonce
        .as_slice()
        .try_into()
        .map_err(|_| AppError::Crypto("blob nonce length".into()))?;

    let engagement_key = unwrap_engagement_key(tx, &keychain_id, master_key)?;

    let on_disk = fs::read(app_data_dir.join(&rel_path))?;
    let mut combined = Vec::with_capacity(on_disk.len() + auth_tag.len());
    combined.extend_from_slice(&on_disk);
    combined.extend_from_slice(&auth_tag);

    cipher::decrypt(&engagement_key, &nonce_arr, &combined)
}

fn unwrap_engagement_key(
    tx: &Transaction<'_>,
    keychain_id: &str,
    master_key: &[u8; ENG_KEY_LEN],
) -> AppResult<[u8; ENG_KEY_LEN]> {
    let wrapped: Option<Vec<u8>> = tx
        .query_row(
            "SELECT wrapped_key FROM KeychainEntry WHERE id = ?1",
            params![keychain_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("keychain entry {keychain_id}")))?;

    let blob = wrapped
        .ok_or_else(|| AppError::Crypto("keychain entry missing wrapped key".into()))?;
    if blob.len() < WRAP_NONCE_LEN + ENG_KEY_LEN + GCM_TAG_LEN {
        return Err(AppError::Crypto("engagement key blob too short".into()));
    }
    let nonce_bytes: [u8; WRAP_NONCE_LEN] = blob[..WRAP_NONCE_LEN]
        .try_into()
        .map_err(|_| AppError::Crypto("wrap nonce length".into()))?;
    let ct = &blob[WRAP_NONCE_LEN..];
    let unwrapped = cipher::decrypt(master_key, &nonce_bytes, ct)?;
    unwrapped
        .try_into()
        .map_err(|_| AppError::Crypto("engagement key length".into()))
}

fn ensure_engagement_key(
    tx: &Transaction<'_>,
    engagement_id: &str,
    master_key: &[u8; ENG_KEY_LEN],
) -> AppResult<(String, [u8; ENG_KEY_LEN])> {
    let row: Option<(String, Option<Vec<u8>>)> = tx
        .query_row(
            "SELECT id, wrapped_key
             FROM KeychainEntry
             WHERE scope_entity_type = 'Engagement' AND scope_entity_id = ?1",
            params![engagement_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    let (keychain_id, wrapped) = row.ok_or_else(|| {
        AppError::NotFound(format!("keychain entry for engagement {engagement_id}"))
    })?;

    if wrapped.is_some() {
        let key = unwrap_engagement_key(tx, &keychain_id, master_key)?;
        return Ok((keychain_id, key));
    }

    // First blob write for this engagement — generate, wrap, persist.
    let mut engagement_key = [0u8; ENG_KEY_LEN];
    rand::thread_rng().fill_bytes(&mut engagement_key);

    let wrap_nonce = cipher::generate_nonce();
    let wrapped = cipher::encrypt(master_key, &wrap_nonce, &engagement_key)?;
    let mut combined = Vec::with_capacity(WRAP_NONCE_LEN + wrapped.len());
    combined.extend_from_slice(&wrap_nonce);
    combined.extend_from_slice(&wrapped);

    tx.execute(
        "UPDATE KeychainEntry
         SET wrapped_key = ?1, kdf = 'direct',
             kdf_params_json = '{\"wrap\":\"AES-256-GCM under session master key\"}'
         WHERE id = ?2",
        params![&combined[..], keychain_id],
    )?;

    Ok((keychain_id, engagement_key))
}

fn relative_blob_path(engagement_id: &str, blob_id: &str) -> PathBuf {
    let shard = blob_id.get(..2).unwrap_or("00");
    PathBuf::from("blobs")
        .join(engagement_id)
        .join(shard)
        .join(format!("{blob_id}.bin"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbState;

    fn tmp_path(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.join(format!("audit-blob-test-{stamp}-{suffix}.db"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    fn seeded(engagement_id: &str) -> (DbState, std::path::PathBuf, tempfile::TempDir) {
        let path = tmp_path("seeded");
        let key = [11u8; 32];
        let db = DbState::new();
        db.open_with_key(&path, &key).unwrap();
        db.with_mut(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO Firm (id, name, country, default_locale, library_version, created_at)
                 VALUES ('firm-b', 'Test', 'ZW', 'en-GB', '0.1.0', 0)",
                [],
            )?;
            tx.execute(
                "INSERT INTO User (
                    id, firm_id, email, display_name, role_id,
                    argon2_hash, master_key_wrapped, status, created_at
                 ) VALUES ('user-b', 'firm-b', 'b@x.com', 'B', 'role-partner',
                    'x', zeroblob(32), 'active', 0)",
                [],
            )?;
            tx.execute(
                "INSERT INTO Client (id, firm_id, name, country, status, created_at)
                 VALUES ('client-b', 'firm-b', 'Client', 'ZW', 'active', 0)",
                [],
            )?;
            tx.execute(
                "INSERT INTO KeychainEntry (
                    id, purpose, scope_entity_type, scope_entity_id,
                    os_keychain_ref, wrapped_key, algorithm, created_at
                 ) VALUES ('kc-b', 'engagement-key', 'Engagement', ?1,
                    ?2, NULL, 'AES-256-GCM', 0)",
                params![engagement_id, format!("engagement/{engagement_id}")],
            )?;
            tx.execute(
                "INSERT INTO Engagement (
                    id, client_id, name, period_id, status_id,
                    library_version_at_start, encryption_key_id,
                    lead_partner_id, created_at
                 ) VALUES (?1, 'client-b', 'Test Eng', NULL, 'status-planning',
                    '0.1.0', 'kc-b', 'user-b', 0)",
                params![engagement_id],
            )?;
            tx.commit()?;
            Ok(())
        })
        .unwrap();

        let blob_dir = tempfile::tempdir().unwrap();
        (db, path, blob_dir)
    }

    #[test]
    fn write_blob_populates_keychain_and_persists_ciphertext_on_disk() {
        let engagement_id = "eng-b1";
        let (db, db_path, tmpdir) = seeded(engagement_id);
        let master_key = [7u8; 32];
        let plaintext = b"employee_id,email\n42,simba@example.com\n";

        let written = db
            .with_mut(|conn| {
                let tx = conn.transaction()?;
                let w = write_engagement_blob(
                    &tx,
                    tmpdir.path(),
                    engagement_id,
                    Some("DataImport"),
                    None,
                    Some("users.csv"),
                    Some("text/csv"),
                    plaintext,
                    &master_key,
                    100,
                )?;
                tx.commit()?;
                Ok(w)
            })
            .unwrap();

        assert_eq!(written.plaintext_size, plaintext.len() as i64);
        assert_eq!(written.sha256_plaintext.len(), 64);

        // KeychainEntry got its wrapped_key populated on first write.
        db.with(|conn| {
            let wrapped: Vec<u8> = conn.query_row(
                "SELECT wrapped_key FROM KeychainEntry WHERE id = 'kc-b'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(wrapped.len(), WRAP_NONCE_LEN + ENG_KEY_LEN + GCM_TAG_LEN);

            let (rel_path, nonce, tag): (String, Vec<u8>, Vec<u8>) = conn.query_row(
                "SELECT ciphertext_path, nonce, auth_tag
                 FROM EncryptedBlob WHERE id = ?1",
                params![written.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let on_disk = std::fs::read(tmpdir.path().join(&rel_path)).unwrap();
            assert_eq!(nonce.len(), WRAP_NONCE_LEN);
            assert_eq!(tag.len(), GCM_TAG_LEN);
            // Plaintext must not appear in the on-disk bytes.
            assert!(!on_disk.windows(5).any(|w| w == b"simba"));
            Ok(())
        })
        .unwrap();

        cleanup(&db_path);
    }

    #[test]
    fn two_blobs_for_same_engagement_reuse_the_keychain_entry() {
        let engagement_id = "eng-b2";
        let (db, db_path, tmpdir) = seeded(engagement_id);
        let master_key = [9u8; 32];

        db.with_mut(|conn| {
            let tx = conn.transaction()?;
            write_engagement_blob(
                &tx,
                tmpdir.path(),
                engagement_id,
                None, None, None, None,
                b"first",
                &master_key,
                0,
            )?;
            write_engagement_blob(
                &tx,
                tmpdir.path(),
                engagement_id,
                None, None, None, None,
                b"second",
                &master_key,
                0,
            )?;
            tx.commit()?;
            Ok(())
        })
        .unwrap();

        db.with(|conn| {
            // No second KeychainEntry — the first write's key is reused.
            let kc_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM KeychainEntry
                 WHERE scope_entity_type = 'Engagement' AND scope_entity_id = ?1",
                params![engagement_id],
                |r| r.get(0),
            )?;
            assert_eq!(kc_count, 1);
            let blob_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM EncryptedBlob",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(blob_count, 2);
            Ok(())
        })
        .unwrap();

        cleanup(&db_path);
    }
}
