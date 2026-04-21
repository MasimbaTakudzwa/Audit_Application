// Scaffold phase: many modules have types and helpers that aren't used yet
// because their commands haven't been wired in. Suppress dead-code warnings at
// the crate level until the first engagement flow lands.
#![allow(dead_code)]

mod auth;
mod commands;
mod crypto;
mod db;
mod error;
mod models;
mod paths;

use std::fs;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("audit_app_lib=info,warn")),
        )
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            tracing::info!("audit application starting");

            let app_data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&app_data_dir)?;
            tracing::info!(path = %app_data_dir.display(), "app data dir ready");

            app.manage(paths::AppPaths::from_app_data_dir(app_data_dir));
            app.manage(db::DbState::new());
            app.manage(auth::AuthState::new());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::identity::ping,
            commands::identity::current_user,
            commands::auth::auth_status,
            commands::auth::onboard,
            commands::auth::login,
            commands::auth::logout,
            commands::clients::list_clients,
            commands::engagements::list_engagements,
            commands::library::library_version,
        ])
        .run(tauri::generate_context!())
        .expect("failed to launch audit application");
}
