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

pub struct AuthState {
    session: Mutex<Option<Session>>,
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            session: Mutex::new(None),
        }
    }

    pub fn set(&self, session: Session) {
        *self.session.lock() = Some(session);
    }

    pub fn clear(&self) {
        *self.session.lock() = None;
    }

    pub fn current(&self) -> Option<Session> {
        self.session.lock().clone()
    }

    /// Used by command handlers that require an authenticated session.
    pub fn require(&self) -> AppResult<Session> {
        self.session
            .lock()
            .clone()
            .ok_or_else(|| AppError::Unauthorised("no active session".into()))
    }
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}
