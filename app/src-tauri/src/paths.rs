//! App-managed paths resolved once at startup.
//!
//! Tauri's `PathResolver` is cheap to call but requires an `AppHandle`.
//! Baking the resolved paths into a managed struct keeps command handlers
//! from having to pull in the full handle just to locate files.

use std::path::PathBuf;

pub struct AppPaths {
    pub app_data_dir: PathBuf,
    pub db_path: PathBuf,
}

impl AppPaths {
    pub fn from_app_data_dir(app_data_dir: PathBuf) -> Self {
        let db_path = app_data_dir.join("audit.db");
        Self {
            app_data_dir,
            db_path,
        }
    }
}
