use std::{fs, sync::Arc};

use parking_lot::Mutex;
use rusqlite::Connection;
use tauri::{App, Manager};

pub mod migrations;

pub struct DbState {
    pub conn: Arc<Mutex<Connection>>,
}

pub fn initialise(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let app_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&app_dir)?;
    let db_path = app_dir.join("audit.db");

    let conn = Connection::open(&db_path)?;

    // SQLCipher key will be supplied from the OS keychain once auth lands.
    // For the scaffold the DB is unencrypted so development is unblocked.
    // Uncomment once key management is wired:
    //     conn.pragma_update(None, "key", &master_key_hex)?;

    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    migrations::run(&conn)?;

    app.manage(DbState {
        conn: Arc::new(Mutex::new(conn)),
    });

    tracing::info!(path = %db_path.display(), "database ready");
    Ok(())
}
