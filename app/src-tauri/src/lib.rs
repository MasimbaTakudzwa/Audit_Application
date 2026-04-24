// Scaffold phase: many modules have types and helpers that aren't used yet
// because their commands haven't been wired in. Suppress dead-code warnings at
// the crate level until the first engagement flow lands.
#![allow(dead_code)]

mod auth;
mod blobs;
mod commands;
mod crypto;
mod db;
mod error;
mod library;
mod matcher;
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
            commands::auth::reset_identity,
            commands::auth::list_users,
            commands::auth::list_roles,
            commands::auth::create_user,
            commands::auth::change_password,
            commands::clients::list_clients,
            commands::clients::list_industries,
            commands::clients::create_client,
            commands::engagements::list_engagements,
            commands::engagements::create_engagement,
            commands::library::library_version,
            commands::library::library_list_risks,
            commands::library::library_list_controls,
            commands::library::library_get_control,
            commands::testing::engagement_add_library_control,
            commands::testing::engagement_upload_data_import,
            commands::testing::engagement_list_data_imports,
            commands::testing::engagement_list_tests,
            commands::testing::engagement_run_matcher,
            commands::testing::engagement_list_test_results,
            commands::findings::engagement_elevate_finding,
            commands::findings::engagement_update_finding,
            commands::findings::engagement_list_findings,
            commands::findings::list_finding_severities,
            commands::evidence::engagement_list_evidence,
            commands::evidence::engagement_upload_evidence,
            commands::evidence::engagement_download_evidence,
            commands::evidence::finding_attach_evidence,
            commands::evidence::finding_detach_evidence,
            commands::evidence::finding_list_evidence,
        ])
        .run(tauri::generate_context!())
        .expect("failed to launch audit application");
}
