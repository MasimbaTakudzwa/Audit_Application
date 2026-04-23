use rusqlite::Connection;

use crate::error::AppResult;

const MIGRATIONS: &[(u32, &str, &str)] = &[
    (1, "foundations", include_str!("0001_foundations.sql")),
    (2, "identity", include_str!("0002_identity.sql")),
    (3, "clients", include_str!("0003_clients.sql")),
    (4, "engagements", include_str!("0004_engagements.sql")),
    (5, "systems", include_str!("0005_systems.sql")),
    (6, "library", include_str!("0006_library.sql")),
    (7, "activity_log", include_str!("0007_activity_log.sql")),
    (8, "testing_findings", include_str!("0008_testing_findings.sql")),
];

pub fn run(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS SchemaMigration (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at INTEGER NOT NULL
        );",
    )?;

    let current: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM SchemaMigration",
        [],
        |r| r.get(0),
    )?;

    for (version, name, sql) in MIGRATIONS {
        if *version > current {
            tracing::info!(version, name, "applying migration");
            conn.execute_batch(sql)?;
            conn.execute(
                "INSERT INTO SchemaMigration (version, name, applied_at) VALUES (?1, ?2, unixepoch())",
                rusqlite::params![version, name],
            )?;
        }
    }

    Ok(())
}
