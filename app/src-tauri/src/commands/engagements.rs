//! Engagement management commands.
//!
//! `create_engagement` is the second mutation path and the first one that
//! writes an `ActivityLog` row — `ActivityLog.engagement_id` is `NOT NULL`, so
//! it only applies to engagement-scoped operations. Pre-engagement mutations
//! (like client creation) live only in the row-level `ChangeLog`.
//!
//! The transaction writes:
//!   1. `KeychainEntry` — placeholder for the per-engagement encryption key.
//!      `wrapped_key` is NULL for now; the real wrap-under-master-key step
//!      lands when `EncryptedBlob` content actually needs encrypting.
//!   2. `Engagement` — FKs to the keychain entry above. Status defaults to
//!      `status-planning`; `library_version_at_start` snapshots the firm's
//!      current library version so the engagement is reproducible even after
//!      the library advances.
//!   3. `EngagementPeriod` — optional; only if all three of start/end/label
//!      are provided by the form.
//!   4. `SyncRecord` + `ChangeLog` — one SyncRecord per mutated entity
//!      (Engagement and, if created, EngagementPeriod). Each gets per-field
//!      ChangeLog rows for the non-null fields.
//!   5. `ActivityLog` — reviewer-facing "engagement created" entry.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
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
pub struct EngagementSummary {
    pub id: String,
    pub name: String,
    pub client_name: String,
    pub status: String,
    pub fiscal_year: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct NewEngagementInput {
    pub client_id: String,
    pub name: String,
    pub fiscal_year_label: Option<String>,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn is_iso_date(s: &str) -> bool {
    // YYYY-MM-DD — good enough for a form guard; the DB doesn't constrain it
    // beyond TEXT. Full calendar validation comes later.
    s.len() == 10
        && s.as_bytes()[4] == b'-'
        && s.as_bytes()[7] == b'-'
        && s[..4].chars().all(|c| c.is_ascii_digit())
        && s[5..7].chars().all(|c| c.is_ascii_digit())
        && s[8..10].chars().all(|c| c.is_ascii_digit())
}

#[tauri::command]
pub fn list_engagements(db: State<'_, DbState>) -> AppResult<Vec<EngagementSummary>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT e.id, e.name, c.name, s.name, p.fiscal_year_label, e.created_at
             FROM Engagement e
             JOIN Client c ON c.id = e.client_id
             JOIN EngagementStatus s ON s.id = e.status_id
             LEFT JOIN EngagementPeriod p ON p.engagement_id = e.id
             ORDER BY e.created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(EngagementSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    client_name: row.get(2)?,
                    status: row.get(3)?,
                    fiscal_year: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[tauri::command]
pub fn create_engagement(
    input: NewEngagementInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<EngagementSummary> {
    let session = auth.require()?;

    let client_id = input.client_id.trim().to_string();
    let name = input.name.trim().to_string();
    let fiscal_year_label = input
        .fiscal_year_label
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let period_start = input
        .period_start
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let period_end = input
        .period_end
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if client_id.is_empty() {
        return Err(AppError::Message("client is required".into()));
    }
    if name.is_empty() {
        return Err(AppError::Message("engagement name is required".into()));
    }

    // All-or-nothing on the period fields: if any one is set, all three must
    // be set. Keeps EngagementPeriod rows internally consistent.
    let make_period = match (&period_start, &period_end, &fiscal_year_label) {
        (Some(_), Some(_), Some(_)) => true,
        (None, None, None) => false,
        _ => {
            return Err(AppError::Message(
                "period start, end, and fiscal year label must be provided together".into(),
            ));
        }
    };
    if let Some(d) = &period_start {
        if !is_iso_date(d) {
            return Err(AppError::Message("period start must be YYYY-MM-DD".into()));
        }
    }
    if let Some(d) = &period_end {
        if !is_iso_date(d) {
            return Err(AppError::Message("period end must be YYYY-MM-DD".into()));
        }
    }

    let engagement_id = Uuid::now_v7().to_string();
    let keychain_id = Uuid::now_v7().to_string();
    let period_id = if make_period {
        Some(Uuid::now_v7().to_string())
    } else {
        None
    };
    let now = now_secs();

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        // Guard: the client must belong to the signed-in user's firm. Without
        // this check, a crafted IPC payload could create an engagement under a
        // different firm's client.
        let client_firm: Option<String> = tx
            .query_row(
                "SELECT firm_id FROM Client WHERE id = ?1",
                params![client_id],
                |r| r.get(0),
            )
            .optional()?;
        match client_firm {
            Some(f) if f == session.firm_id => {}
            _ => return Err(AppError::NotFound("client not found".into())),
        }

        // Snapshot the firm's current library version at engagement start so
        // the engagement's risk/control set stays stable across library
        // updates. If the firm row has no library_version, fall back to "v1"
        // (the onboarding default).
        let library_version: String = tx
            .query_row(
                "SELECT COALESCE(library_version, 'v1') FROM Firm WHERE id = ?1",
                params![session.firm_id],
                |r| r.get(0),
            )?;

        // Per-engagement encryption key, step 1: the KeychainEntry row the
        // Engagement FKs into. `wrapped_key` is NULL and `os_keychain_ref`
        // is a placeholder — the real wrap-under-master-key step lands with
        // the first EncryptedBlob write. Documented TODO in the decision log.
        tx.execute(
            "INSERT INTO KeychainEntry (
                id, purpose, scope_entity_type, scope_entity_id,
                os_keychain_ref, wrapped_key, algorithm, created_at
             ) VALUES (
                ?1, 'engagement-key', 'Engagement', ?2,
                ?3, NULL, 'AES-256-GCM', ?4
             )",
            params![
                keychain_id,
                engagement_id,
                format!("engagement/{}", engagement_id),
                now,
            ],
        )?;

        tx.execute(
            "INSERT INTO Engagement (
                id, client_id, name, period_id, status_id,
                library_version_at_start, encryption_key_id,
                lead_partner_id, created_at
             ) VALUES (
                ?1, ?2, ?3, ?4, 'status-planning',
                ?5, ?6, ?7, ?8
             )",
            params![
                engagement_id,
                client_id,
                name,
                period_id,
                library_version,
                keychain_id,
                session.user_id,
                now,
            ],
        )?;

        if make_period {
            tx.execute(
                "INSERT INTO EngagementPeriod (
                    id, engagement_id, start_date, end_date, fiscal_year_label
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    period_id.as_ref().unwrap(),
                    engagement_id,
                    period_start.as_ref().unwrap(),
                    period_end.as_ref().unwrap(),
                    fiscal_year_label.as_ref().unwrap(),
                ],
            )?;

            let period_sync_id = Uuid::now_v7().to_string();
            tx.execute(
                "INSERT INTO SyncRecord (
                    id, entity_type, entity_id, last_modified_at, last_modified_by,
                    version, deleted, sync_state
                 ) VALUES (?1, 'EngagementPeriod', ?2, ?3, ?4, 1, 0, 'local_only')",
                params![
                    period_sync_id,
                    period_id.as_ref().unwrap(),
                    now,
                    session.user_id,
                ],
            )?;
            let period_fields: Vec<(&str, serde_json::Value)> = vec![
                ("engagement_id", json!(engagement_id.clone())),
                ("start_date", json!(period_start.as_ref().unwrap().clone())),
                ("end_date", json!(period_end.as_ref().unwrap().clone())),
                (
                    "fiscal_year_label",
                    json!(fiscal_year_label.as_ref().unwrap().clone()),
                ),
            ];
            for (field_name, new_value) in period_fields {
                tx.execute(
                    "INSERT INTO ChangeLog (
                        id, sync_record_id, occurred_at, user_id,
                        field_name, old_value_json, new_value_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                    params![
                        Uuid::now_v7().to_string(),
                        period_sync_id,
                        now,
                        session.user_id,
                        field_name,
                        new_value.to_string(),
                    ],
                )?;
            }
        }

        let sync_id = Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO SyncRecord (
                id, entity_type, entity_id, last_modified_at, last_modified_by,
                version, deleted, sync_state
             ) VALUES (?1, 'Engagement', ?2, ?3, ?4, 1, 0, 'local_only')",
            params![sync_id, engagement_id, now, session.user_id],
        )?;
        let mut fields: Vec<(&str, serde_json::Value)> = vec![
            ("client_id", json!(client_id.clone())),
            ("name", json!(name.clone())),
            ("status_id", json!("status-planning")),
            ("library_version_at_start", json!(library_version)),
            ("encryption_key_id", json!(keychain_id.clone())),
            ("lead_partner_id", json!(session.user_id.clone())),
        ];
        if let Some(pid) = &period_id {
            fields.push(("period_id", json!(pid.clone())));
        }
        for (field_name, new_value) in fields {
            tx.execute(
                "INSERT INTO ChangeLog (
                    id, sync_record_id, occurred_at, user_id,
                    field_name, old_value_json, new_value_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                params![
                    Uuid::now_v7().to_string(),
                    sync_id,
                    now,
                    session.user_id,
                    field_name,
                    new_value.to_string(),
                ],
            )?;
        }

        // Reviewer-facing activity. `entity_id` points back at the Engagement
        // itself so the reviewer UI can surface "engagement X was created on
        // date Y by user Z" alongside later updates.
        tx.execute(
            "INSERT INTO ActivityLog (
                id, engagement_id, entity_type, entity_id,
                action, performed_by, performed_at, summary
             ) VALUES (?1, ?2, 'Engagement', ?2, 'created', ?3, ?4, ?5)",
            params![
                Uuid::now_v7().to_string(),
                engagement_id,
                session.user_id,
                now,
                format!("Engagement '{}' created", name),
            ],
        )?;

        tx.commit()?;
        Ok(())
    })?;

    let summary = db.with(|conn| {
        let s = conn.query_row(
            "SELECT e.id, e.name, c.name, s.name, p.fiscal_year_label, e.created_at
             FROM Engagement e
             JOIN Client c ON c.id = e.client_id
             JOIN EngagementStatus s ON s.id = e.status_id
             LEFT JOIN EngagementPeriod p ON p.engagement_id = e.id
             WHERE e.id = ?1",
            params![engagement_id],
            |row| {
                Ok(EngagementSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    client_name: row.get(2)?,
                    status: row.get(3)?,
                    fiscal_year: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )?;
        Ok(s)
    })?;

    tracing::info!(
        engagement_id = %summary.id,
        client_id = %client_id,
        firm_id = %session.firm_id,
        user_id = %session.user_id,
        "engagement created"
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
        dir.join(format!("audit-engagement-test-{stamp}-{suffix}.db"))
    }

    fn seeded_db(firm_id: &str, user_id: &str, client_id: &str) -> (DbState, std::path::PathBuf) {
        let path = tmp_path("seeded");
        let key = [3u8; 32];
        let db = DbState::new();
        db.open_with_key(&path, &key).unwrap();
        db.with_mut(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO Firm (id, name, country, default_locale, library_version, created_at)
                 VALUES (?1, 'Test Firm', 'ZW', 'en-GB', 'v1', 0)",
                params![firm_id],
            )?;
            tx.execute(
                "INSERT INTO User (
                    id, firm_id, email, display_name, role_id,
                    argon2_hash, master_key_wrapped, status, created_at
                 ) VALUES (?1, ?2, 'u@x.com', 'Tester', 'role-partner',
                    'x', zeroblob(32), 'active', 0)",
                params![user_id, firm_id],
            )?;
            tx.execute(
                "INSERT INTO Client (id, firm_id, name, country, status, created_at)
                 VALUES (?1, ?2, 'Test Client', 'ZW', 'active', 0)",
                params![client_id, firm_id],
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

    fn session_for(firm_id: &str, user_id: &str) -> AuthState {
        let a = AuthState::new();
        a.set(
            Session {
                user_id: user_id.into(),
                firm_id: firm_id.into(),
                display_name: "Tester".into(),
                email: "u@x.com".into(),
            },
            [0u8; 32],
        );
        a
    }

    #[test]
    fn create_engagement_writes_all_related_rows() {
        let firm_id = "firm-e1";
        let user_id = "user-e1";
        let client_id = "client-e1";
        let (db, path) = seeded_db(firm_id, user_id, client_id);
        let auth = session_for(firm_id, user_id);

        let summary = create_engagement_for_test(
            &db,
            &auth,
            NewEngagementInput {
                client_id: client_id.into(),
                name: "FY2026 audit".into(),
                fiscal_year_label: Some("FY2026".into()),
                period_start: Some("2026-01-01".into()),
                period_end: Some("2026-12-31".into()),
            },
        )
        .unwrap();

        assert_eq!(summary.name, "FY2026 audit");
        assert_eq!(summary.client_name, "Test Client");
        assert_eq!(summary.status, "Planning");
        assert_eq!(summary.fiscal_year.as_deref(), Some("FY2026"));

        db.with(|conn| {
            let kc_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM KeychainEntry
                 WHERE scope_entity_type = 'Engagement' AND scope_entity_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            assert_eq!(kc_count, 1);

            let eng_sync_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM SyncRecord
                 WHERE entity_type = 'Engagement' AND entity_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            assert_eq!(eng_sync_count, 1);

            let period_sync_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM SyncRecord WHERE entity_type = 'EngagementPeriod'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(period_sync_count, 1);

            // 7 Engagement fields: client_id, name, status_id,
            // library_version_at_start, encryption_key_id, lead_partner_id,
            // period_id.
            let eng_changes: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ChangeLog cl
                 JOIN SyncRecord sr ON sr.id = cl.sync_record_id
                 WHERE sr.entity_type = 'Engagement' AND sr.entity_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            assert_eq!(eng_changes, 7);

            let activity_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog WHERE engagement_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            assert_eq!(activity_count, 1, "one created-activity row");

            let action: String = conn.query_row(
                "SELECT action FROM ActivityLog WHERE engagement_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            assert_eq!(action, "created");
            Ok(())
        })
        .unwrap();

        cleanup(&path);
    }

    #[test]
    fn create_engagement_without_period_skips_engagement_period_row() {
        let firm_id = "firm-e2";
        let user_id = "user-e2";
        let client_id = "client-e2";
        let (db, path) = seeded_db(firm_id, user_id, client_id);
        let auth = session_for(firm_id, user_id);

        let summary = create_engagement_for_test(
            &db,
            &auth,
            NewEngagementInput {
                client_id: client_id.into(),
                name: "Ad-hoc review".into(),
                fiscal_year_label: None,
                period_start: None,
                period_end: None,
            },
        )
        .unwrap();
        assert!(summary.fiscal_year.is_none());

        db.with(|conn| {
            let period_rows: i64 = conn.query_row(
                "SELECT COUNT(*) FROM EngagementPeriod WHERE engagement_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            assert_eq!(period_rows, 0);
            let eng_changes: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ChangeLog cl
                 JOIN SyncRecord sr ON sr.id = cl.sync_record_id
                 WHERE sr.entity_type = 'Engagement' AND sr.entity_id = ?1",
                params![summary.id],
                |r| r.get(0),
            )?;
            // No period_id when skipped → 6 fields, not 7.
            assert_eq!(eng_changes, 6);
            Ok(())
        })
        .unwrap();

        cleanup(&path);
    }

    #[test]
    fn create_engagement_rejects_client_from_other_firm() {
        let firm_id = "firm-e3";
        let user_id = "user-e3";
        let client_id = "client-e3";
        let (db, path) = seeded_db(firm_id, user_id, client_id);
        // Session is a different firm from the one owning the client.
        let auth = session_for("firm-other", user_id);

        let err = create_engagement_for_test(
            &db,
            &auth,
            NewEngagementInput {
                client_id: client_id.into(),
                name: "Cross-firm".into(),
                fiscal_year_label: None,
                period_start: None,
                period_end: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&path);
    }

    #[test]
    fn create_engagement_rejects_partial_period() {
        let firm_id = "firm-e4";
        let user_id = "user-e4";
        let client_id = "client-e4";
        let (db, path) = seeded_db(firm_id, user_id, client_id);
        let auth = session_for(firm_id, user_id);

        let err = create_engagement_for_test(
            &db,
            &auth,
            NewEngagementInput {
                client_id: client_id.into(),
                name: "Partial period".into(),
                fiscal_year_label: Some("FY2026".into()),
                period_start: Some("2026-01-01".into()),
                period_end: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Message(_)));
        cleanup(&path);
    }

    // Test-only helper mirroring the command body. Tests don't have a Tauri
    // `State`, so we call `DbState` / `AuthState` directly.
    fn create_engagement_for_test(
        db: &DbState,
        auth: &AuthState,
        input: NewEngagementInput,
    ) -> AppResult<EngagementSummary> {
        let session = auth.require()?;

        let client_id = input.client_id.trim().to_string();
        let name = input.name.trim().to_string();
        let fiscal_year_label = input
            .fiscal_year_label
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let period_start = input
            .period_start
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let period_end = input
            .period_end
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if client_id.is_empty() {
            return Err(AppError::Message("client is required".into()));
        }
        if name.is_empty() {
            return Err(AppError::Message("engagement name is required".into()));
        }
        let make_period = match (&period_start, &period_end, &fiscal_year_label) {
            (Some(_), Some(_), Some(_)) => true,
            (None, None, None) => false,
            _ => {
                return Err(AppError::Message(
                    "period start, end, and fiscal year label must be provided together".into(),
                ));
            }
        };
        if let Some(d) = &period_start {
            if !is_iso_date(d) {
                return Err(AppError::Message("period start must be YYYY-MM-DD".into()));
            }
        }
        if let Some(d) = &period_end {
            if !is_iso_date(d) {
                return Err(AppError::Message("period end must be YYYY-MM-DD".into()));
            }
        }

        let engagement_id = Uuid::now_v7().to_string();
        let keychain_id = Uuid::now_v7().to_string();
        let period_id = if make_period {
            Some(Uuid::now_v7().to_string())
        } else {
            None
        };
        let now = now_secs();

        db.with_mut(|conn| {
            let tx = conn.transaction()?;

            let client_firm: Option<String> = tx
                .query_row(
                    "SELECT firm_id FROM Client WHERE id = ?1",
                    params![client_id],
                    |r| r.get(0),
                )
                .optional()?;
            match client_firm {
                Some(f) if f == session.firm_id => {}
                _ => return Err(AppError::NotFound("client not found".into())),
            }

            let library_version: String = tx.query_row(
                "SELECT COALESCE(library_version, 'v1') FROM Firm WHERE id = ?1",
                params![session.firm_id],
                |r| r.get(0),
            )?;

            tx.execute(
                "INSERT INTO KeychainEntry (
                    id, purpose, scope_entity_type, scope_entity_id,
                    os_keychain_ref, wrapped_key, algorithm, created_at
                 ) VALUES (
                    ?1, 'engagement-key', 'Engagement', ?2,
                    ?3, NULL, 'AES-256-GCM', ?4
                 )",
                params![
                    keychain_id,
                    engagement_id,
                    format!("engagement/{}", engagement_id),
                    now
                ],
            )?;

            tx.execute(
                "INSERT INTO Engagement (
                    id, client_id, name, period_id, status_id,
                    library_version_at_start, encryption_key_id,
                    lead_partner_id, created_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, 'status-planning',
                    ?5, ?6, ?7, ?8
                 )",
                params![
                    engagement_id,
                    client_id,
                    name,
                    period_id,
                    library_version,
                    keychain_id,
                    session.user_id,
                    now,
                ],
            )?;

            if make_period {
                tx.execute(
                    "INSERT INTO EngagementPeriod (
                        id, engagement_id, start_date, end_date, fiscal_year_label
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        period_id.as_ref().unwrap(),
                        engagement_id,
                        period_start.as_ref().unwrap(),
                        period_end.as_ref().unwrap(),
                        fiscal_year_label.as_ref().unwrap(),
                    ],
                )?;
                let period_sync_id = Uuid::now_v7().to_string();
                tx.execute(
                    "INSERT INTO SyncRecord (
                        id, entity_type, entity_id, last_modified_at, last_modified_by,
                        version, deleted, sync_state
                     ) VALUES (?1, 'EngagementPeriod', ?2, ?3, ?4, 1, 0, 'local_only')",
                    params![
                        period_sync_id,
                        period_id.as_ref().unwrap(),
                        now,
                        session.user_id,
                    ],
                )?;
                let period_fields: Vec<(&str, serde_json::Value)> = vec![
                    ("engagement_id", json!(engagement_id.clone())),
                    ("start_date", json!(period_start.as_ref().unwrap().clone())),
                    ("end_date", json!(period_end.as_ref().unwrap().clone())),
                    (
                        "fiscal_year_label",
                        json!(fiscal_year_label.as_ref().unwrap().clone()),
                    ),
                ];
                for (field_name, new_value) in period_fields {
                    tx.execute(
                        "INSERT INTO ChangeLog (
                            id, sync_record_id, occurred_at, user_id,
                            field_name, old_value_json, new_value_json
                         ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                        params![
                            Uuid::now_v7().to_string(),
                            period_sync_id,
                            now,
                            session.user_id,
                            field_name,
                            new_value.to_string(),
                        ],
                    )?;
                }
            }

            let sync_id = Uuid::now_v7().to_string();
            tx.execute(
                "INSERT INTO SyncRecord (
                    id, entity_type, entity_id, last_modified_at, last_modified_by,
                    version, deleted, sync_state
                 ) VALUES (?1, 'Engagement', ?2, ?3, ?4, 1, 0, 'local_only')",
                params![sync_id, engagement_id, now, session.user_id],
            )?;
            let mut fields: Vec<(&str, serde_json::Value)> = vec![
                ("client_id", json!(client_id.clone())),
                ("name", json!(name.clone())),
                ("status_id", json!("status-planning")),
                ("library_version_at_start", json!(library_version)),
                ("encryption_key_id", json!(keychain_id.clone())),
                ("lead_partner_id", json!(session.user_id.clone())),
            ];
            if let Some(pid) = &period_id {
                fields.push(("period_id", json!(pid.clone())));
            }
            for (field_name, new_value) in fields {
                tx.execute(
                    "INSERT INTO ChangeLog (
                        id, sync_record_id, occurred_at, user_id,
                        field_name, old_value_json, new_value_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                    params![
                        Uuid::now_v7().to_string(),
                        sync_id,
                        now,
                        session.user_id,
                        field_name,
                        new_value.to_string(),
                    ],
                )?;
            }

            tx.execute(
                "INSERT INTO ActivityLog (
                    id, engagement_id, entity_type, entity_id,
                    action, performed_by, performed_at, summary
                 ) VALUES (?1, ?2, 'Engagement', ?2, 'created', ?3, ?4, ?5)",
                params![
                    Uuid::now_v7().to_string(),
                    engagement_id,
                    session.user_id,
                    now,
                    format!("Engagement '{}' created", name),
                ],
            )?;

            tx.commit()?;
            Ok(())
        })?;

        db.with(|conn| {
            let s = conn.query_row(
                "SELECT e.id, e.name, c.name, s.name, p.fiscal_year_label, e.created_at
                 FROM Engagement e
                 JOIN Client c ON c.id = e.client_id
                 JOIN EngagementStatus s ON s.id = e.status_id
                 LEFT JOIN EngagementPeriod p ON p.engagement_id = e.id
                 WHERE e.id = ?1",
                params![engagement_id],
                |row| {
                    Ok(EngagementSummary {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        client_name: row.get(2)?,
                        status: row.get(3)?,
                        fiscal_year: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )?;
            Ok(s)
        })
    }
}
