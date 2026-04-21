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
    auth.set(session.clone(), master_key);
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
    auth.set(session.clone(), master_key);
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

/// Destroys the local identity file and the encrypted DB. Used when the user
/// has forgotten their password and has no other user to recover through —
/// there is no cryptographic way to recover the data (the password derives
/// the KEK that wraps the master key). Returns the app to onboarding.
///
/// Caller is expected to surface an explicit confirmation in the UI. The
/// backend requires the literal string `"i understand this wipes everything"`
/// to reduce the chance of accidental invocation via a mistyped command name
/// or a stray IPC payload.
#[tauri::command]
pub fn reset_identity(
    confirmation: String,
    paths: State<'_, AppPaths>,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<()> {
    const REQUIRED: &str = "i understand this wipes everything";
    if confirmation.trim() != REQUIRED {
        return Err(AppError::Message(
            "reset confirmation phrase does not match".into(),
        ));
    }

    auth.clear();
    db.close();

    let id_path = keyvault::path(&paths.app_data_dir);
    if id_path.exists() {
        fs::remove_file(&id_path)?;
    }
    if paths.db_path.exists() {
        fs::remove_file(&paths.db_path)?;
    }
    let _ = fs::remove_file(paths.db_path.with_extension("db-wal"));
    let _ = fs::remove_file(paths.db_path.with_extension("db-shm"));

    tracing::warn!(
        app_data_dir = %paths.app_data_dir.display(),
        "identity and encrypted DB wiped at user request"
    );
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CreateUserInput {
    pub email: String,
    pub display_name: String,
    pub password: String,
    pub role_id: String,
}

#[derive(Debug, Serialize)]
pub struct UserRecord {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub role_id: String,
    pub role_name: String,
    pub status: String,
    pub last_seen_at: Option<i64>,
    pub created_at: i64,
}

#[tauri::command]
pub fn list_users(
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Vec<UserRecord>> {
    let session = auth.require()?;
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT u.id, u.email, u.display_name, u.role_id, r.name,
                    u.status, u.last_seen_at, u.created_at
             FROM User u
             JOIN Role r ON r.id = u.role_id
             WHERE u.firm_id = ?1
             ORDER BY u.created_at ASC",
        )?;
        let rows = stmt
            .query_map(params![session.firm_id], |row| {
                Ok(UserRecord {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    display_name: row.get(2)?,
                    role_id: row.get(3)?,
                    role_name: row.get(4)?,
                    status: row.get(5)?,
                    last_seen_at: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[derive(Debug, Serialize)]
pub struct RoleRecord {
    pub id: String,
    pub name: String,
}

#[tauri::command]
pub fn list_roles(db: State<'_, DbState>, auth: State<'_, AuthState>) -> AppResult<Vec<RoleRecord>> {
    auth.require()?;
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name FROM Role WHERE is_builtin = 1
             ORDER BY CASE id
                WHEN 'role-partner' THEN 1
                WHEN 'role-manager' THEN 2
                WHEN 'role-senior' THEN 3
                WHEN 'role-associate' THEN 4
                WHEN 'role-readonly' THEN 5
                ELSE 99 END",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RoleRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// Creates an additional user inside the signed-in user's firm. The new user
/// gets their own `UserCredential` in `identity.json` wrapping the *same*
/// master key under their own password, so they can open the encrypted DB
/// without the original user's involvement.
///
/// Only partners may create users — the first person through onboarding is
/// always given `role-partner`, so a single-user firm stays functional.
#[tauri::command]
pub fn create_user(
    input: CreateUserInput,
    paths: State<'_, AppPaths>,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<UserRecord> {
    let (session, master_key) = auth.require_keyed()?;

    let email = input.email.trim().to_lowercase();
    let display_name = input.display_name.trim().to_string();
    let role_id = input.role_id.trim().to_string();

    if display_name.is_empty() {
        return Err(AppError::Message("display name is required".into()));
    }
    validate_email(&email)?;
    validate_password(&input.password)?;

    // Role-gate: only partners can add users. This uses the DB row rather
    // than the session so a demoted user can't still invite.
    let caller_role: String = db.with(|conn| {
        let r: String = conn.query_row(
            "SELECT role_id FROM User WHERE id = ?1",
            params![session.user_id],
            |r| r.get(0),
        )?;
        Ok(r)
    })?;
    if caller_role != "role-partner" {
        return Err(AppError::Unauthorised(
            "only partners can create users".into(),
        ));
    }

    // Block unknown role ids with a clearer error than the FK violation.
    let role_exists: i64 = db.with(|conn| {
        let n: i64 =
            conn.query_row("SELECT COUNT(*) FROM Role WHERE id = ?1", params![role_id], |r| r.get(0))?;
        Ok(n)
    })?;
    if role_exists == 0 {
        return Err(AppError::Message(format!("unknown role '{role_id}'")));
    }

    // Block duplicate email in identity.json *and* in the DB. Both surface as
    // the same user-facing message.
    let mut identity = keyvault::load(&paths.app_data_dir)?;
    if identity
        .users
        .iter()
        .any(|u| u.email.eq_ignore_ascii_case(&email))
    {
        return Err(AppError::Message(
            "a user with that email already exists".into(),
        ));
    }

    let email_in_db: i64 = db.with(|conn| {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM User WHERE email = ?1",
            params![email],
            |r| r.get(0),
        )?;
        Ok(n)
    })?;
    if email_in_db > 0 {
        return Err(AppError::Message(
            "a user with that email already exists".into(),
        ));
    }

    let new_user_id = Uuid::now_v7().to_string();

    // Hash + wrap outside the DB lock — Argon2 is intentionally slow.
    let credential = keyvault::wrap_master_key_for(
        &new_user_id,
        &session.firm_id,
        &email,
        &display_name,
        &input.password,
        &master_key,
    )?;

    // Placeholder bytes only exist to satisfy the NOT NULL constraint on the
    // legacy `master_key_wrapped` column. The authoritative wrap lives in
    // `identity.json`. Scheduled for removal in the schema cleanup milestone.
    let mut mk_placeholder = [0u8; 60];
    rand::thread_rng().fill_bytes(&mut mk_placeholder);

    let now = now_secs();
    let schema_hash = credential.argon2_hash.clone();

    // DB write + identity.json update must be kept in sync. If the DB insert
    // succeeds but identity persist fails, we roll back the inserted User row
    // manually so we don't leave a user that can't sign in.
    db.with_mut(|conn| {
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO User (
                id, firm_id, email, display_name, role_id,
                argon2_hash, master_key_wrapped, status, last_seen_at, created_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, 'active', NULL, ?8
             )",
            params![
                new_user_id,
                session.firm_id,
                email,
                display_name,
                role_id,
                schema_hash,
                mk_placeholder.to_vec(),
                now,
            ],
        )?;
        tx.commit()?;
        Ok(())
    })?;

    identity.users.push(credential);
    if let Err(e) = keyvault::save(&paths.app_data_dir, &identity) {
        // Best-effort rollback of the DB row so a retry can succeed.
        let _ = db.with(|conn| {
            conn.execute("DELETE FROM User WHERE id = ?1", params![new_user_id])?;
            Ok(())
        });
        return Err(e);
    }

    let record = db.with(|conn| {
        let r = conn.query_row(
            "SELECT u.id, u.email, u.display_name, u.role_id, r.name,
                    u.status, u.last_seen_at, u.created_at
             FROM User u JOIN Role r ON r.id = u.role_id
             WHERE u.id = ?1",
            params![new_user_id],
            |row| {
                Ok(UserRecord {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    display_name: row.get(2)?,
                    role_id: row.get(3)?,
                    role_name: row.get(4)?,
                    status: row.get(5)?,
                    last_seen_at: row.get(6)?,
                    created_at: row.get(7)?,
                })
            },
        )?;
        Ok(r)
    })?;

    tracing::info!(
        new_user_id = %record.id,
        firm_id = %session.firm_id,
        created_by = %session.user_id,
        "user created"
    );
    Ok(record)
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordInput {
    pub old_password: String,
    pub new_password: String,
}

/// Re-wraps the signed-in user's entry in `identity.json` under a new
/// password. Requires the current password — not just the active session —
/// so a walk-up attacker on an unlocked machine can't silently change it.
#[tauri::command]
pub fn change_password(
    input: ChangePasswordInput,
    paths: State<'_, AppPaths>,
    auth: State<'_, AuthState>,
) -> AppResult<()> {
    let (session, master_key) = auth.require_keyed()?;

    if input.old_password.is_empty() {
        return Err(AppError::Message("current password is required".into()));
    }
    validate_password(&input.new_password)?;
    if input.old_password == input.new_password {
        return Err(AppError::Message(
            "new password must differ from the current one".into(),
        ));
    }

    let mut identity = keyvault::load(&paths.app_data_dir)?;
    let idx = identity
        .users
        .iter()
        .position(|u| u.user_id == session.user_id)
        .ok_or_else(|| AppError::NotFound("identity entry for current user".into()))?;

    // Verify the old password actually unlocks the existing wrap. Don't
    // short-circuit via the argon2 hash alone: defense-in-depth against
    // a tampered-with identity.json.
    let unlocked = keyvault::unlock(&identity.users[idx], &input.old_password)
        .map_err(|_| AppError::Unauthorised("current password is incorrect".into()))?;
    if unlocked != master_key {
        return Err(AppError::Crypto(
            "unlocked master key does not match session — identity file corrupted".into(),
        ));
    }

    let new_cred = keyvault::wrap_master_key_for(
        &session.user_id,
        &session.firm_id,
        &session.email,
        &session.display_name,
        &input.new_password,
        &master_key,
    )?;
    identity.users[idx] = new_cred;
    keyvault::save(&paths.app_data_dir, &identity)?;

    tracing::info!(user_id = %session.user_id, "password changed");
    Ok(())
}
