//! In-memory session state for the signed-in auditor.
//!
//! Held in a `parking_lot::Mutex<Option<Session>>` behind a Tauri-managed state.
//! Every mutation path in command handlers goes through `require_session()` to
//! ensure actions are attributable — no anonymous writes.

pub mod keyvault;

use parking_lot::Mutex;
use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub user_id: String,
    pub firm_id: String,
    pub display_name: String,
    pub email: String,
}

/// Signed-in state. `master_key` is held alongside the `Session` so mutation
/// paths that need to wrap new key material (create additional user, change
/// password, per-engagement encryption keys) can re-wrap without asking the
/// user for the password again. The key is the same 32-byte blob SQLCipher
/// already holds internally — keeping a Rust-side copy here doesn't change the
/// attack surface in a meaningful way, and it lets multi-user and rekey flows
/// work without touching the password.
struct AuthInner {
    session: Session,
    master_key: [u8; 32],
}

pub struct AuthState {
    inner: Mutex<Option<AuthInner>>,
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    pub fn set(&self, session: Session, master_key: [u8; 32]) {
        *self.inner.lock() = Some(AuthInner {
            session,
            master_key,
        });
    }

    pub fn clear(&self) {
        *self.inner.lock() = None;
    }

    pub fn current(&self) -> Option<Session> {
        self.inner.lock().as_ref().map(|i| i.session.clone())
    }

    /// Used by command handlers that require an authenticated session.
    pub fn require(&self) -> AppResult<Session> {
        self.inner
            .lock()
            .as_ref()
            .map(|i| i.session.clone())
            .ok_or_else(|| AppError::Unauthorised("no active session".into()))
    }

    /// Returns the session plus the in-memory master key. For handlers that
    /// need to wrap new material (create user, change password, per-engagement
    /// keys). Fails with `Unauthorised` if no one is signed in.
    pub fn require_keyed(&self) -> AppResult<(Session, [u8; 32])> {
        self.inner
            .lock()
            .as_ref()
            .map(|i| (i.session.clone(), i.master_key))
            .ok_or_else(|| AppError::Unauthorised("no active session".into()))
    }
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}
