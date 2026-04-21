// Scaffold phase: many modules have types and helpers that aren't used yet
// because their commands haven't been wired in. Suppress dead-code warnings at
// the crate level until the first engagement flow lands.
#![allow(dead_code)]

mod commands;
mod crypto;
mod db;
mod error;
mod models;

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
            db::initialise(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::identity::ping,
            commands::identity::current_user,
            commands::clients::list_clients,
            commands::engagements::list_engagements,
            commands::library::library_version,
        ])
        .run(tauri::generate_context!())
        .expect("failed to launch audit application");
}
