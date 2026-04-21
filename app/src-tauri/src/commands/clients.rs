//! Client management commands.
//!
//! `create_client` is the first mutation path in the app. It exercises:
//!   - `AuthState::require()` — every write is attributable to a signed-in user.
//!   - A single transaction over `Client` + `SyncRecord` + `ChangeLog`, so a
//!     failure anywhere rolls the whole creation back. No orphan sync rows.
//!   - One `ChangeLog` row per non-null input field, with `old_value_json`
//!     left NULL (creation = all fields newly set). The `field_name NOT NULL`
//!     schema constraint means per-field rows, not a single "*"-style row.
//!   - `ActivityLog` is NOT written here. Per the `DATA_MODEL.md` decision,
//!     `ActivityLog` is engagement-scoped (has a `NOT NULL engagement_id`).
//!     Pre-engagement mutations like client creation live only in the
//!     row-level `ChangeLog`.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;
use uuid::Uuid;

use crate::{
    auth::AuthState,
    db::DbState,
    error::{AppError, AppResult},
};

#[derive(Debug, Serialize)]
pub struct ClientSummary {
    pub id: String,
    pub name: String,
    pub country: String,
    pub industry: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct IndustrySummary {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct NewClientInput {
    pub name: String,
    pub country: String,
    pub industry_id: Option<String>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub fn list_clients(db: State<'_, DbState>) -> AppResult<Vec<ClientSummary>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT c.id, c.name, c.country, i.name, c.status
             FROM Client c
             LEFT JOIN Industry i ON i.id = c.industry_id
             WHERE c.status = 'active'
             ORDER BY c.name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ClientSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    country: row.get(2)?,
                    industry: row.get(3)?,
                    status: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[tauri::command]
pub fn list_industries(db: State<'_, DbState>) -> AppResult<Vec<IndustrySummary>> {
    db.with(|conn| {
        let mut stmt = conn.prepare("SELECT id, name FROM Industry ORDER BY name")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(IndustrySummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[tauri::command]
pub fn create_client(
    input: NewClientInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<ClientSummary> {
    let session = auth.require()?;

    let name = input.name.trim().to_string();
    let country = input.country.trim().to_string();
    let industry_id = input
        .industry_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if name.is_empty() {
        return Err(AppError::Message("client name is required".into()));
    }
    if country.is_empty() {
        return Err(AppError::Message("country is required".into()));
    }

    let client_id = Uuid::now_v7().to_string();
    let now = now_secs();

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        tx.execute(
            "INSERT INTO Client (id, firm_id, name, industry_id, country, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6)",
            params![client_id, session.firm_id, name, industry_id, country, now],
        )?;

        let sync_record_id = Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO SyncRecord (
                id, entity_type, entity_id, last_modified_at, last_modified_by,
                version, deleted, sync_state
             ) VALUES (?1, 'Client', ?2, ?3, ?4, 1, 0, 'local_only')",
            params![sync_record_id, client_id, now, session.user_id],
        )?;

        // One ChangeLog row per non-null input field. UPDATE paths will write
        // only the columns that actually changed.
        let mut fields: Vec<(&str, serde_json::Value)> = vec![
            ("name", json!(name.clone())),
            ("country", json!(country.clone())),
            ("status", json!("active")),
            ("firm_id", json!(session.firm_id)),
        ];
        if let Some(ind) = &industry_id {
            fields.push(("industry_id", json!(ind.clone())));
        }
        for (field_name, new_value) in fields {
            tx.execute(
                "INSERT INTO ChangeLog (
                    id, sync_record_id, occurred_at, user_id,
                    field_name, old_value_json, new_value_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                params![
                    Uuid::now_v7().to_string(),
                    sync_record_id,
                    now,
                    session.user_id,
                    field_name,
                    new_value.to_string(),
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    })?;

    // Read the summary back with the joined industry name, so the frontend
    // can append the row to the list without a refetch.
    let summary = db.with(|conn| {
        let s = conn.query_row(
            "SELECT c.id, c.name, c.country, i.name, c.status
             FROM Client c
             LEFT JOIN Industry i ON i.id = c.industry_id
             WHERE c.id = ?1",
            params![client_id],
            |row| {
                Ok(ClientSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    country: row.get(2)?,
                    industry: row.get(3)?,
                    status: row.get(4)?,
                })
            },
        )?;
        Ok(s)
    })?;

    tracing::info!(
        client_id = %summary.id,
        firm_id = %session.firm_id,
        user_id = %session.user_id,
        "client created"
    );
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Session;

    fn tmp_path(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.join(format!("audit-client-test-{stamp}-{suffix}.db"))
    }

    fn seeded_db(firm_id: &str) -> (DbState, std::path::PathBuf) {
        let path = tmp_path("seeded");
        let key = [9u8; 32];
        let db = DbState::new();
        db.open_with_key(&path, &key).unwrap();
        db.with_mut(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO Firm (id, name, country, default_locale, created_at)
                 VALUES (?1, 'Acme Advisory', 'ZW', 'en-GB', 0)",
                params![firm_id],
            )?;
            tx.commit()?;
            Ok(())
        })
        .unwrap();
        (db, path)
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    fn session_for(firm_id: &str) -> AuthState {
        let a = AuthState::new();
        a.set(
            Session {
                user_id: "user-test".into(),
                firm_id: firm_id.into(),
                display_name: "Tester".into(),
                email: "tester@example.com".into(),
            },
            [0u8; 32],
        );
        a
    }

    #[test]
    fn create_client_writes_client_sync_record_and_change_log() {
        let firm_id = "firm-test-1";
        let (db, path) = seeded_db(firm_id);
        let auth = session_for(firm_id);

        let summary = create_client_for_test(
            &db,
            &auth,
            NewClientInput {
                name: "First Client".into(),
                country: "ZW".into(),
                industry_id: Some("ind-banking".into()),
            },
        )
        .unwrap();

        assert_eq!(summary.name, "First Client");
        assert_eq!(summary.country, "ZW");
        assert_eq!(summary.industry.as_deref(), Some("Banking"));
        assert_eq!(summary.status, "active");

        db.with(|conn| {
            let sync_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM SyncRecord WHERE entity_type = 'Client' AND entity_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            assert_eq!(sync_count, 1, "one SyncRecord row per entity");

            let version: i64 = conn.query_row(
                "SELECT version FROM SyncRecord WHERE entity_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            assert_eq!(version, 1, "version starts at 1");

            let sync_state: String = conn.query_row(
                "SELECT sync_state FROM SyncRecord WHERE entity_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            assert_eq!(sync_state, "local_only");

            let change_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ChangeLog cl
                 JOIN SyncRecord sr ON sr.id = cl.sync_record_id
                 WHERE sr.entity_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            // name, country, status, firm_id, industry_id
            assert_eq!(change_count, 5, "one ChangeLog row per non-null field");
            Ok(())
        })
        .unwrap();

        cleanup(&path);
    }

    #[test]
    fn create_client_rejects_when_not_signed_in() {
        let firm_id = "firm-test-2";
        let (db, path) = seeded_db(firm_id);
        let auth = AuthState::new(); // empty — no session

        let err = create_client_for_test(
            &db,
            &auth,
            NewClientInput {
                name: "Unauthed".into(),
                country: "ZW".into(),
                industry_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Unauthorised(_)));
        cleanup(&path);
    }

    #[test]
    fn create_client_requires_name() {
        let firm_id = "firm-test-3";
        let (db, path) = seeded_db(firm_id);
        let auth = session_for(firm_id);

        let err = create_client_for_test(
            &db,
            &auth,
            NewClientInput {
                name: "   ".into(),
                country: "ZW".into(),
                industry_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Message(_)));
        cleanup(&path);
    }

    // Test-only helper that mirrors the command body without pulling a Tauri
    // `State` — unit tests don't have a running Tauri runtime.
    fn create_client_for_test(
        db: &DbState,
        auth: &AuthState,
        input: NewClientInput,
    ) -> AppResult<ClientSummary> {
        let session = auth.require()?;

        let name = input.name.trim().to_string();
        let country = input.country.trim().to_string();
        let industry_id = input
            .industry_id
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if name.is_empty() {
            return Err(AppError::Message("client name is required".into()));
        }
        if country.is_empty() {
            return Err(AppError::Message("country is required".into()));
        }

        let client_id = Uuid::now_v7().to_string();
        let now = now_secs();

        db.with_mut(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO Client (id, firm_id, name, industry_id, country, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6)",
                params![client_id, session.firm_id, name, industry_id, country, now],
            )?;
            let sync_record_id = Uuid::now_v7().to_string();
            tx.execute(
                "INSERT INTO SyncRecord (
                    id, entity_type, entity_id, last_modified_at, last_modified_by,
                    version, deleted, sync_state
                 ) VALUES (?1, 'Client', ?2, ?3, ?4, 1, 0, 'local_only')",
                params![sync_record_id, client_id, now, session.user_id],
            )?;
            let mut fields: Vec<(&str, serde_json::Value)> = vec![
                ("name", json!(name.clone())),
                ("country", json!(country.clone())),
                ("status", json!("active")),
                ("firm_id", json!(session.firm_id.clone())),
            ];
            if let Some(ind) = &industry_id {
                fields.push(("industry_id", json!(ind.clone())));
            }
            for (field_name, new_value) in fields {
                tx.execute(
                    "INSERT INTO ChangeLog (
                        id, sync_record_id, occurred_at, user_id,
                        field_name, old_value_json, new_value_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                    params![
                        Uuid::now_v7().to_string(),
                        sync_record_id,
                        now,
                        session.user_id,
                        field_name,
                        new_value.to_string(),
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })?;

        db.with(|conn| {
            let s = conn.query_row(
                "SELECT c.id, c.name, c.country, i.name, c.status
                 FROM Client c
                 LEFT JOIN Industry i ON i.id = c.industry_id
                 WHERE c.id = ?1",
                params![client_id],
                |row| {
                    Ok(ClientSummary {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        country: row.get(2)?,
                        industry: row.get(3)?,
                        status: row.get(4)?,
                    })
                },
            )?;
            Ok(s)
        })
    }
}
