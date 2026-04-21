//! Authentication commands — onboarding, sign-in, sign-out, status.
//!
//! The DB file is encrypted at rest with a random master key held only in
//! the live `SqlCipher` connection. Onboard generates that key, wraps it under
//! an Argon2id-derived KEK, and persists the wrap in `identity.json`. Login
//! unwraps the key and opens the DB; logout drops the connection.
//!
//! The `User.argon2_hash` and `User.master_key_wrapped` columns in the
//! encrypted schema are still populated for now — the authoritative copy is
//! in `identity.json`, and tidying the schema to drop these is a follow-up.

use std::{fs, time::{SystemTime, UNIX_EPOCH}};

use rand::RngCore;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    auth::{keyvault, AuthState, Session},
    db::DbState,
    error::{AppError, AppResult},
    paths::AppPaths,
};

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthStatus {
    OnboardingRequired,
    SignInRequired,
    SignedIn { user: Session },
}

#[derive(Debug, Deserialize)]
pub struct OnboardInput {
    pub firm_name: String,
    pub firm_country: String,
    pub display_name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn validate_email(email: &str) -> AppResult<()> {
    if email.contains('@') && email.len() >= 3 {
        Ok(())
    } else {
        Err(AppError::Message(
            "please enter a valid email address".into(),
        ))
    }
}

fn validate_password(password: &str) -> AppResult<()> {
    if password.len() >= 8 {
        Ok(())
    } else {
        Err(AppError::Message(
            "password must be at least 8 characters".into(),
        ))
    }
}

#[tauri::command]
pub fn auth_status(
    paths: State<'_, AppPaths>,
    auth: State<'_, AuthState>,
) -> AppResult<AuthStatus> {
    if let Some(session) = auth.current() {
        return Ok(AuthStatus::SignedIn { user: session });
    }
    if keyvault::exists(&paths.app_data_dir) {
        Ok(AuthStatus::SignInRequired)
    } else {
        Ok(AuthStatus::OnboardingRequired)
    }
}

#[tauri::command]
pub fn onboard(
    input: OnboardInput,
    paths: State<'_, AppPaths>,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Session> {
    let firm_name = input.firm_name.trim().to_string();
    let firm_country = input.firm_country.trim().to_string();
    let display_name = input.display_name.trim().to_string();
    let email = input.email.trim().to_lowercase();

    if firm_name.is_empty() {
        return Err(AppError::Message("firm name is required".into()));
    }
    if firm_country.is_empty() {
        return Err(AppError::Message("firm country is required".into()));
    }
    if display_name.is_empty() {
        return Err(AppError::Message("display name is required".into()));
    }
    validate_email(&email)?;
    validate_password(&input.password)?;

    if keyvault::exists(&paths.app_data_dir) {
        return Err(AppError::Message(
            "identity already exists; sign in instead".into(),
        ));
    }

    // A leftover DB with no identity cannot be opened (wrong/no key) and
    // would fail confusingly further down. Purge it so the fresh keyed DB
    // can replace it. Safe because there is no identity to own the data.
    if paths.db_path.exists() {
        tracing::warn!(
            path = %paths.db_path.display(),
            "removing stale un-keyed database before onboard"
        );
        fs::remove_file(&paths.db_path)?;
        // Also remove WAL/SHM sidecars left by the old unencrypted DB.
        let wal = paths.db_path.with_extension("db-wal");
        let shm = paths.db_path.with_extension("db-shm");
        let _ = fs::remove_file(&wal);
        let _ = fs::remove_file(&shm);
    }

    let user_id = Uuid::now_v7().to_string();
    let firm_id = Uuid::now_v7().to_string();

    // Generate identity + master key *before* opening the DB.
    let (identity, master_key) = keyvault::create_first_user(
        &user_id,
        &firm_id,
        &email,
        &display_name,
        &input.password,
    )?;

    // Open the encrypted DB and run migrations inside the keyed connection.
    db.open_with_key(&paths.db_path, &master_key)?;

    // These columns are still required by the schema. The `identity.json`
    // copies are authoritative; these are populated to satisfy NOT NULL.
    let schema_hash = identity.users[0].argon2_hash.clone();
    let mut mk_placeholder = [0u8; 60];
    rand::thread_rng().fill_bytes(&mut mk_placeholder);

    let now = now_secs();
    db.with_mut(|conn| {
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO Firm (id, name, country, default_locale, library_version, created_at)
             VALUES (?1, ?2, ?3, 'en-GB', 'v1', ?4)",
            params![firm_id, firm_name, firm_country, now],
        )?;
        tx.execute(
            "INSERT INTO User (
                id, firm_id, email, display_name, role_id,
                argon2_hash, master_key_wrapped, status, last_seen_at, created_at
             ) VALUES (
                ?1, ?2, ?3, ?4, 'role-partner',
                ?5, ?6, 'active', ?7, ?7
             )",
            params![
                user_id,
                firm_id,
                email,
                display_name,
                schema_hash,
                mk_placeholder.to_vec(),
                now,
            ],
        )?;
        tx.commit()?;
        Ok(())
    })?;

    // Persist identity.json only after the DB is successfully seeded — avoid
    // an orphan identity pointing at a half-initialised DB.
    keyvault::save(&paths.app_data_dir, &identity)?;

    let session = Session {
        user_id: user_id.clone(),
        firm_id: firm_id.clone(),
        display_name,
        email,
    };
    auth.set(session.clone());
    tracing::info!(user_id = %user_id, firm_id = %firm_id, "onboarded new firm");
    Ok(session)
}

#[tauri::command]
pub fn login(
    input: LoginInput,
    paths: State<'_, AppPaths>,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Session> {
    let email = input.email.trim().to_lowercase();
    validate_email(&email)?;
    if input.password.is_empty() {
        return Err(AppError::Message("password is required".into()));
    }

    let identity = keyvault::load(&paths.app_data_dir)?;
    let credential = keyvault::find_by_email(&identity, &email)
        .ok_or_else(|| AppError::Unauthorised("invalid email or password".into()))?
        .clone();

    let master_key = keyvault::unlock(&credential, &input.password)?;

    db.open_with_key(&paths.db_path, &master_key)?;

    // Re-fetch display name from the encrypted DB in case it was updated
    // there (the identity.json copy is written at onboard and not kept in
    // lock-step with user edits yet).
    let display_name = db.with(|conn| {
        let name: Option<String> = conn
            .query_row(
                "SELECT display_name FROM User WHERE id = ?1",
                params![credential.user_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(name.unwrap_or_else(|| credential.display_name.clone()))
    })?;

    let now = now_secs();
    db.with(|conn| {
        conn.execute(
            "UPDATE User SET last_seen_at = ?1 WHERE id = ?2",
            params![now, credential.user_id],
        )?;
        Ok(())
    })?;

    let session = Session {
        user_id: credential.user_id.clone(),
        firm_id: credential.firm_id.clone(),
        display_name,
        email: credential.email.clone(),
    };
    auth.set(session.clone());
    tracing::info!(user_id = %credential.user_id, "user signed in");
    Ok(session)
}

#[tauri::command]
pub fn logout(db: State<'_, DbState>, auth: State<'_, AuthState>) -> AppResult<()> {
    auth.clear();
    db.close();
    tracing::info!("user signed out");
    Ok(())
}
