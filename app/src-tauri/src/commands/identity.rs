use serde::Serialize;

use crate::error::AppResult;

#[derive(Debug, Serialize)]
pub struct HealthStatus {
    pub app: &'static str,
    pub version: &'static str,
}

#[tauri::command]
pub fn ping() -> HealthStatus {
    HealthStatus {
        app: "audit-app",
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[derive(Debug, Serialize)]
pub struct CurrentUser {
    pub signed_in: bool,
    pub display_name: Option<String>,
    pub firm_name: Option<String>,
    pub role: Option<String>,
}

/// Placeholder until authentication is wired. Returns "not signed in".
#[tauri::command]
pub fn current_user() -> AppResult<CurrentUser> {
    Ok(CurrentUser {
        signed_in: false,
        display_name: None,
        firm_name: None,
        role: None,
    })
}
