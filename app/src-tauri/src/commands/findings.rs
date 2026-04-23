//! Findings commands (Module 8).
//!
//! The first useful operation here is **elevation**: turn a `TestResult` with
//! exceptions into a draft `Finding`. The auditor then refines the condition
//! and recommendation text before the finding is communicated to the client.
//! We intentionally keep the initial condition/recommendation generic — the
//! matcher can produce machine text, the auditor adds context and judgment.
//!
//! Uniqueness: `Finding.code` is scoped to the engagement (`UNIQUE
//! (engagement_id, code)`). Codes are generated as `F-001`, `F-002`, ...
//! against a count of existing findings. Simultaneous elevations could race
//! on the same number; in a single-user desktop app this is vanishingly
//! unlikely, and if it ever does happen the SQLite constraint error bubbles
//! up cleanly — the auditor can retry.
//!
//! Linking: every elevation inserts a `FindingTestResultLink`. Multiple
//! `TestResult`s can cite the same finding (e.g. two rule runs of the same
//! test both feeding one finding) and one `TestResult` can only point at
//! a finding via an explicit elevation.

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

#[derive(Debug, Deserialize)]
pub struct ElevateFindingInput {
    pub test_result_id: String,
    /// Optional — defaults to the test's name.
    pub title: Option<String>,
    /// Optional — defaults to `sev-medium`.
    pub severity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFindingInput {
    pub finding_id: String,
    pub title: String,
    /// Empty string clears the column to NULL.
    pub condition_text: Option<String>,
    pub recommendation_text: Option<String>,
    pub severity_id: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct FindingSummary {
    pub id: String,
    pub engagement_id: String,
    pub code: String,
    pub title: String,
    pub condition_text: Option<String>,
    pub recommendation_text: Option<String>,
    pub severity_id: Option<String>,
    pub severity_name: Option<String>,
    pub status: String,
    pub test_id: Option<String>,
    pub test_code: Option<String>,
    pub engagement_control_id: Option<String>,
    pub control_code: Option<String>,
    pub identified_at: i64,
    pub identified_by_name: Option<String>,
    pub linked_test_result_ids: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SeveritySummary {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
    pub description: Option<String>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub fn engagement_elevate_finding(
    input: ElevateFindingInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<FindingSummary> {
    elevate_finding(db.inner(), auth.inner(), input)
}

#[tauri::command]
pub fn engagement_list_findings(
    engagement_id: String,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Vec<FindingSummary>> {
    list_findings(db.inner(), auth.inner(), engagement_id)
}

#[tauri::command]
pub fn list_finding_severities(
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Vec<SeveritySummary>> {
    list_severities(db.inner(), auth.inner())
}

#[tauri::command]
pub fn engagement_update_finding(
    input: UpdateFindingInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<FindingSummary> {
    update_finding(db.inner(), auth.inner(), input)
}

pub(crate) fn elevate_finding(
    db: &DbState,
    auth: &AuthState,
    input: ElevateFindingInput,
) -> AppResult<FindingSummary> {
    let session = auth.require()?;
    let test_result_id = input.test_result_id.trim().to_string();
    if test_result_id.is_empty() {
        return Err(AppError::Message("test result is required".into()));
    }
    let supplied_title = input
        .title
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let supplied_severity = input
        .severity_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let now = now_secs();

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        let info = tx
            .query_row(
                "SELECT tr.outcome, tr.exception_summary, tr.evidence_count,
                        tr.population_ref_label,
                        t.id, t.engagement_id, t.engagement_control_id,
                        t.code, t.name,
                        c.firm_id
                 FROM TestResult tr
                 JOIN Test t ON t.id = tr.test_id
                 JOIN Engagement e ON e.id = t.engagement_id
                 JOIN Client c ON c.id = e.client_id
                 WHERE tr.id = ?1",
                params![test_result_id],
                |r| {
                    Ok(ResultRow {
                        outcome: r.get(0)?,
                        exception_summary: r.get(1)?,
                        evidence_count: r.get(2)?,
                        population_ref_label: r.get(3)?,
                        test_id: r.get(4)?,
                        engagement_id: r.get(5)?,
                        engagement_control_id: r.get(6)?,
                        test_code: r.get(7)?,
                        test_name: r.get(8)?,
                        firm_id: r.get(9)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("test result not found".into()))?;

        if info.firm_id != session.firm_id {
            return Err(AppError::NotFound("test result not found".into()));
        }

        // Elevation only makes sense when there's something to remediate.
        if info.outcome == "pass" || info.evidence_count <= 0 {
            return Err(AppError::Message(
                "cannot elevate a test result that had no exceptions".into(),
            ));
        }

        let severity_id = match supplied_severity {
            Some(id) => {
                let exists: bool = tx
                    .query_row(
                        "SELECT 1 FROM FindingSeverity WHERE id = ?1",
                        params![id],
                        |_| Ok(true),
                    )
                    .optional()?
                    .unwrap_or(false);
                if !exists {
                    return Err(AppError::NotFound("severity not found".into()));
                }
                id
            }
            None => "sev-medium".to_string(),
        };

        let existing_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM Finding WHERE engagement_id = ?1",
            params![info.engagement_id],
            |r| r.get(0),
        )?;
        let code = format!("F-{:03}", existing_count + 1);

        let title = supplied_title.unwrap_or_else(|| info.test_name.clone());
        let population_line = info
            .population_ref_label
            .as_deref()
            .map(|p| format!(" Population reviewed: {p}."))
            .unwrap_or_default();
        let summary_line = info
            .exception_summary
            .as_deref()
            .unwrap_or("exceptions identified by the automated matcher");
        let condition = format!(
            "Testing performed under {} identified {}: {}.{}",
            info.test_code, info.evidence_count, summary_line, population_line
        );
        let recommendation = "Confirm with management the reason each exception remains \
             outstanding, agree a remediation plan with target dates, and re-test once \
             the plan is complete."
            .to_string();

        let finding_id = Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO Finding (
                id, engagement_id, test_id, engagement_control_id,
                code, title, condition_text, recommendation_text,
                severity_id, status, identified_by, identified_at,
                first_communicated_at, closed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'draft', ?10, ?11, NULL, NULL)",
            params![
                finding_id,
                info.engagement_id,
                info.test_id,
                info.engagement_control_id,
                code,
                title,
                condition,
                recommendation,
                severity_id,
                session.user_id,
                now,
            ],
        )?;

        tx.execute(
            "INSERT INTO FindingTestResultLink (id, finding_id, test_result_id)
             VALUES (?1, ?2, ?3)",
            params![Uuid::now_v7().to_string(), finding_id, test_result_id],
        )?;

        let sync_id = Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO SyncRecord (
                id, entity_type, entity_id, last_modified_at,
                last_modified_by, version, deleted, sync_state
             ) VALUES (?1, 'Finding', ?2, ?3, ?4, 1, 0, 'local_only')",
            params![sync_id, finding_id, now, session.user_id],
        )?;
        tx.execute(
            "INSERT INTO ChangeLog (
                id, sync_record_id, occurred_at, user_id,
                field_name, old_value_json, new_value_json
             ) VALUES (?1, ?2, ?3, ?4, '.', NULL, ?5)",
            params![
                Uuid::now_v7().to_string(),
                sync_id,
                now,
                session.user_id,
                json!({
                    "engagement_id": info.engagement_id,
                    "test_id": info.test_id,
                    "engagement_control_id": info.engagement_control_id,
                    "code": code,
                    "title": title,
                    "severity_id": severity_id,
                    "status": "draft",
                    "linked_test_result_ids": [test_result_id],
                })
                .to_string(),
            ],
        )?;

        tx.execute(
            "INSERT INTO ActivityLog (
                id, engagement_id, entity_type, entity_id,
                action, performed_by, performed_at, summary
             ) VALUES (?1, ?2, 'Finding', ?3, 'elevated_from_result', ?4, ?5, ?6)",
            params![
                Uuid::now_v7().to_string(),
                info.engagement_id,
                finding_id,
                session.user_id,
                now,
                format!(
                    "Elevated {} exception(s) from {} into finding {}",
                    info.evidence_count, info.test_code, code
                ),
            ],
        )?;

        let severity_name: Option<String> = tx
            .query_row(
                "SELECT name FROM FindingSeverity WHERE id = ?1",
                params![severity_id],
                |r| r.get(0),
            )
            .optional()?;

        let control_code: Option<String> = match &info.engagement_control_id {
            Some(id) => tx
                .query_row(
                    "SELECT code FROM EngagementControl WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };

        tx.commit()?;

        tracing::info!(
            engagement_id = %info.engagement_id,
            finding_id = %finding_id,
            code = %code,
            evidence_count = info.evidence_count,
            "finding elevated from test result"
        );

        Ok(FindingSummary {
            id: finding_id,
            engagement_id: info.engagement_id,
            code,
            title,
            condition_text: Some(condition),
            recommendation_text: Some(recommendation),
            severity_id: Some(severity_id),
            severity_name,
            status: "draft".into(),
            test_id: Some(info.test_id),
            test_code: Some(info.test_code),
            engagement_control_id: info.engagement_control_id,
            control_code,
            identified_at: now,
            identified_by_name: Some(session.display_name.clone()),
            linked_test_result_ids: vec![test_result_id],
        })
    })
}

pub(crate) fn list_findings(
    db: &DbState,
    auth: &AuthState,
    engagement_id: String,
) -> AppResult<Vec<FindingSummary>> {
    let session = auth.require()?;
    let engagement_id = engagement_id.trim().to_string();
    if engagement_id.is_empty() {
        return Err(AppError::Message("engagement is required".into()));
    }

    db.with(|conn| {
        let firm: Option<String> = conn
            .query_row(
                "SELECT c.firm_id
                 FROM Engagement e
                 JOIN Client c ON c.id = e.client_id
                 WHERE e.id = ?1",
                params![engagement_id],
                |r| r.get(0),
            )
            .optional()?;
        match firm {
            Some(f) if f == session.firm_id => {}
            _ => return Err(AppError::NotFound("engagement not found".into())),
        }

        let mut stmt = conn.prepare(
            "SELECT f.id, f.engagement_id, f.code, f.title,
                    f.condition_text, f.recommendation_text,
                    f.severity_id, fs.name, f.status,
                    f.test_id, t.code,
                    f.engagement_control_id, ec.code,
                    f.identified_at, u.display_name
             FROM Finding f
             LEFT JOIN FindingSeverity fs ON fs.id = f.severity_id
             LEFT JOIN Test t ON t.id = f.test_id
             LEFT JOIN EngagementControl ec ON ec.id = f.engagement_control_id
             LEFT JOIN User u ON u.id = f.identified_by
             WHERE f.engagement_id = ?1
             ORDER BY f.identified_at DESC",
        )?;
        let findings: Vec<FindingSummary> = stmt
            .query_map(params![engagement_id], |row| {
                Ok(FindingSummary {
                    id: row.get(0)?,
                    engagement_id: row.get(1)?,
                    code: row.get(2)?,
                    title: row.get(3)?,
                    condition_text: row.get(4)?,
                    recommendation_text: row.get(5)?,
                    severity_id: row.get(6)?,
                    severity_name: row.get(7)?,
                    status: row.get(8)?,
                    test_id: row.get(9)?,
                    test_code: row.get(10)?,
                    engagement_control_id: row.get(11)?,
                    control_code: row.get(12)?,
                    identified_at: row.get(13)?,
                    identified_by_name: row.get(14)?,
                    linked_test_result_ids: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Second pass: fetch linked test_result_ids per finding. A single query
        // would mean splitting the row reader into two stages; separate queries
        // are clearer and the N here is the number of findings (small).
        let mut enriched = Vec::with_capacity(findings.len());
        let mut link_stmt = conn.prepare(
            "SELECT test_result_id FROM FindingTestResultLink WHERE finding_id = ?1",
        )?;
        for mut f in findings {
            let links: Vec<String> = link_stmt
                .query_map(params![f.id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            f.linked_test_result_ids = links;
            enriched.push(f);
        }
        Ok(enriched)
    })
}

pub(crate) fn list_severities(
    db: &DbState,
    auth: &AuthState,
) -> AppResult<Vec<SeveritySummary>> {
    let _ = auth.require()?;
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, sort_order, description
             FROM FindingSeverity
             ORDER BY sort_order",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SeveritySummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    sort_order: row.get(2)?,
                    description: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub(crate) fn update_finding(
    db: &DbState,
    auth: &AuthState,
    input: UpdateFindingInput,
) -> AppResult<FindingSummary> {
    let session = auth.require()?;
    let finding_id = input.finding_id.trim().to_string();
    if finding_id.is_empty() {
        return Err(AppError::Message("finding is required".into()));
    }
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::Message("title is required".into()));
    }
    let severity_id = input.severity_id.trim().to_string();
    if severity_id.is_empty() {
        return Err(AppError::Message("severity is required".into()));
    }
    let condition = input
        .condition_text
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let recommendation = input
        .recommendation_text
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let now = now_secs();

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        let existing = tx
            .query_row(
                "SELECT f.engagement_id, f.code, f.title, f.condition_text,
                        f.recommendation_text, f.severity_id, f.status,
                        f.test_id, t.code, f.engagement_control_id, ec.code,
                        f.identified_at, f.identified_by, c.firm_id
                 FROM Finding f
                 JOIN Engagement e ON e.id = f.engagement_id
                 JOIN Client c ON c.id = e.client_id
                 LEFT JOIN Test t ON t.id = f.test_id
                 LEFT JOIN EngagementControl ec ON ec.id = f.engagement_control_id
                 WHERE f.id = ?1",
                params![finding_id],
                |r| {
                    Ok(ExistingFinding {
                        engagement_id: r.get(0)?,
                        code: r.get(1)?,
                        title: r.get(2)?,
                        condition: r.get(3)?,
                        recommendation: r.get(4)?,
                        severity_id: r.get(5)?,
                        status: r.get(6)?,
                        test_id: r.get(7)?,
                        test_code: r.get(8)?,
                        engagement_control_id: r.get(9)?,
                        control_code: r.get(10)?,
                        identified_at: r.get(11)?,
                        identified_by: r.get(12)?,
                        firm_id: r.get(13)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("finding not found".into()))?;

        if existing.firm_id != session.firm_id {
            return Err(AppError::NotFound("finding not found".into()));
        }

        let severity_name: String = tx
            .query_row(
                "SELECT name FROM FindingSeverity WHERE id = ?1",
                params![severity_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("severity not found".into()))?;

        // Build list of fields that actually changed. If none, return the current
        // projection without writing — keeps the audit trail free of noop saves.
        let changes: Vec<(&str, Option<String>, Option<String>)> = [
            (
                "title",
                Some(existing.title.clone()),
                Some(title.clone()),
            ),
            (
                "condition_text",
                existing.condition.clone(),
                condition.clone(),
            ),
            (
                "recommendation_text",
                existing.recommendation.clone(),
                recommendation.clone(),
            ),
            (
                "severity_id",
                existing.severity_id.clone(),
                Some(severity_id.clone()),
            ),
        ]
        .into_iter()
        .filter(|(_, old, new)| old != new)
        .collect();

        let linked: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT test_result_id FROM FindingTestResultLink WHERE finding_id = ?1",
            )?;
            let rows: Vec<String> = stmt
                .query_map(params![finding_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };

        let identified_by_name: Option<String> = match &existing.identified_by {
            Some(uid) => tx
                .query_row(
                    "SELECT display_name FROM User WHERE id = ?1",
                    params![uid],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };

        if changes.is_empty() {
            tx.commit()?;
            return Ok(FindingSummary {
                id: finding_id,
                engagement_id: existing.engagement_id,
                code: existing.code,
                title: existing.title,
                condition_text: existing.condition,
                recommendation_text: existing.recommendation,
                severity_id: existing.severity_id,
                severity_name: Some(severity_name),
                status: existing.status,
                test_id: existing.test_id,
                test_code: existing.test_code,
                engagement_control_id: existing.engagement_control_id,
                control_code: existing.control_code,
                identified_at: existing.identified_at,
                identified_by_name,
                linked_test_result_ids: linked,
            });
        }

        tx.execute(
            "UPDATE Finding
             SET title = ?1, condition_text = ?2, recommendation_text = ?3, severity_id = ?4
             WHERE id = ?5",
            params![title, condition, recommendation, severity_id, finding_id],
        )?;

        let sync_id: String = match tx
            .query_row(
                "SELECT id FROM SyncRecord
                 WHERE entity_type = 'Finding' AND entity_id = ?1",
                params![finding_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            Some(id) => {
                tx.execute(
                    "UPDATE SyncRecord
                     SET last_modified_at = ?1, last_modified_by = ?2, version = version + 1
                     WHERE id = ?3",
                    params![now, session.user_id, id],
                )?;
                id
            }
            None => {
                let new_id = Uuid::now_v7().to_string();
                tx.execute(
                    "INSERT INTO SyncRecord (
                        id, entity_type, entity_id, last_modified_at,
                        last_modified_by, version, deleted, sync_state
                     ) VALUES (?1, 'Finding', ?2, ?3, ?4, 1, 0, 'local_only')",
                    params![new_id, finding_id, now, session.user_id],
                )?;
                new_id
            }
        };

        for (field, old, new) in &changes {
            tx.execute(
                "INSERT INTO ChangeLog (
                    id, sync_record_id, occurred_at, user_id,
                    field_name, old_value_json, new_value_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    Uuid::now_v7().to_string(),
                    sync_id,
                    now,
                    session.user_id,
                    field,
                    old.as_ref().map(|s| json!(s).to_string()),
                    new.as_ref().map(|s| json!(s).to_string()),
                ],
            )?;
        }

        tx.execute(
            "INSERT INTO ActivityLog (
                id, engagement_id, entity_type, entity_id,
                action, performed_by, performed_at, summary
             ) VALUES (?1, ?2, 'Finding', ?3, 'edited', ?4, ?5, ?6)",
            params![
                Uuid::now_v7().to_string(),
                existing.engagement_id,
                finding_id,
                session.user_id,
                now,
                format!(
                    "Edited finding {} ({} field{} changed)",
                    existing.code,
                    changes.len(),
                    if changes.len() == 1 { "" } else { "s" }
                ),
            ],
        )?;

        tx.commit()?;

        tracing::info!(
            engagement_id = %existing.engagement_id,
            finding_id = %finding_id,
            code = %existing.code,
            fields_changed = changes.len(),
            "finding edited"
        );

        Ok(FindingSummary {
            id: finding_id,
            engagement_id: existing.engagement_id,
            code: existing.code,
            title,
            condition_text: condition,
            recommendation_text: recommendation,
            severity_id: Some(severity_id),
            severity_name: Some(severity_name),
            status: existing.status,
            test_id: existing.test_id,
            test_code: existing.test_code,
            engagement_control_id: existing.engagement_control_id,
            control_code: existing.control_code,
            identified_at: existing.identified_at,
            identified_by_name,
            linked_test_result_ids: linked,
        })
    })
}

struct ExistingFinding {
    engagement_id: String,
    code: String,
    title: String,
    condition: Option<String>,
    recommendation: Option<String>,
    severity_id: Option<String>,
    status: String,
    test_id: Option<String>,
    test_code: Option<String>,
    engagement_control_id: Option<String>,
    control_code: Option<String>,
    identified_at: i64,
    identified_by: Option<String>,
    firm_id: String,
}

struct ResultRow {
    outcome: String,
    exception_summary: Option<String>,
    evidence_count: i64,
    population_ref_label: Option<String>,
    test_id: String,
    engagement_id: String,
    engagement_control_id: Option<String>,
    test_code: String,
    test_name: String,
    firm_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Session;
    use crate::commands::testing::{
        clone_library_control, run_access_review, upload_data_import,
        AddLibraryControlInput, RunAccessReviewInput, UploadDataImportInput,
    };
    use crate::paths::AppPaths;

    fn tmp_path(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.join(format!("audit-findings-test-{stamp}-{suffix}.db"))
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

    fn seeded_db(
        firm_id: &str,
        user_id: &str,
        client_id: &str,
        engagement_id: &str,
    ) -> (DbState, std::path::PathBuf) {
        let path = tmp_path("seeded");
        let key = [7u8; 32];
        let db = DbState::new();
        db.open_with_key(&path, &key).unwrap();

        db.with_mut(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO Firm (id, name, country, default_locale, library_version, created_at)
                 VALUES (?1, 'Test Firm', 'ZW', 'en-GB', '0.1.0', 0)",
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
            let kc_id = Uuid::now_v7().to_string();
            tx.execute(
                "INSERT INTO KeychainEntry (
                    id, purpose, scope_entity_type, scope_entity_id,
                    os_keychain_ref, wrapped_key, algorithm, created_at
                 ) VALUES (?1, 'engagement-key', 'Engagement', ?2,
                    ?3, NULL, 'AES-256-GCM', 0)",
                params![
                    kc_id,
                    engagement_id,
                    format!("engagement/{}", engagement_id)
                ],
            )?;
            tx.execute(
                "INSERT INTO Engagement (
                    id, client_id, name, period_id, status_id,
                    library_version_at_start, encryption_key_id,
                    lead_partner_id, created_at
                 ) VALUES (?1, ?2, 'Test Engagement', NULL, 'status-planning',
                    '0.1.0', ?3, ?4, 0)",
                params![engagement_id, client_id, kc_id, user_id],
            )?;
            tx.commit()?;
            Ok(())
        })
        .unwrap();
        (db, path)
    }

    fn library_control_id(db: &DbState, code: &str) -> String {
        db.with(|conn| {
            Ok(conn.query_row(
                "SELECT id FROM LibraryControl WHERE code = ?1 AND superseded_by IS NULL",
                params![code],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    }

    fn paths_for(dir: &std::path::Path) -> AppPaths {
        AppPaths::from_app_data_dir(dir.to_path_buf())
    }

    // End-to-end rig: clone UAM-C-001 → upload AD + leavers → run matcher →
    // return the TestResult id ready for elevation.
    fn seed_with_exception(
        db: &DbState,
        auth: &AuthState,
        paths: &AppPaths,
        engagement_id: &str,
    ) -> String {
        let clone = clone_library_control(
            db,
            auth,
            AddLibraryControlInput {
                engagement_id: engagement_id.into(),
                library_control_id: library_control_id(db, "UAM-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let test_id = clone.test_ids[0].clone();

        upload_data_import(
            db,
            auth,
            paths,
            UploadDataImportInput {
                engagement_id: engagement_id.into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "ad.csv".into(),
                mime_type: None,
                content: b"email,enabled\nalice@a.com,TRUE\nbob@a.com,TRUE\n".to_vec(),
            },
        )
        .unwrap();
        upload_data_import(
            db,
            auth,
            paths,
            UploadDataImportInput {
                engagement_id: engagement_id.into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_leavers".into(),
                filename: "leavers.csv".into(),
                mime_type: None,
                content: b"email\nalice@a.com\n".to_vec(),
            },
        )
        .unwrap();
        let run = run_access_review(
            db,
            auth,
            paths,
            RunAccessReviewInput {
                test_id,
                ad_import_id: None,
                leavers_import_id: None,
            },
        )
        .unwrap();
        assert_eq!(run.outcome, "exception");
        run.test_result_id
    }

    #[test]
    fn elevate_creates_finding_with_link_and_defaults() {
        let (db, db_path) = seeded_db("firm-f1", "user-f1", "client-f1", "eng-f1");
        let auth = session_for("firm-f1", "user-f1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());
        let result_id = seed_with_exception(&db, &auth, &paths, "eng-f1");

        let finding = elevate_finding(
            &db,
            &auth,
            ElevateFindingInput {
                test_result_id: result_id.clone(),
                title: None,
                severity_id: None,
            },
        )
        .unwrap();

        assert_eq!(finding.code, "F-001");
        assert_eq!(finding.status, "draft");
        assert_eq!(finding.severity_id.as_deref(), Some("sev-medium"));
        assert!(finding.condition_text.as_deref().unwrap().contains("UAM-T-001"));
        assert_eq!(finding.linked_test_result_ids, vec![result_id.clone()]);
        assert_eq!(finding.control_code.as_deref(), Some("UAM-C-001"));

        db.with(|conn| {
            let link_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM FindingTestResultLink
                 WHERE finding_id = ?1 AND test_result_id = ?2",
                params![finding.id, result_id],
                |r| r.get(0),
            )?;
            assert_eq!(link_count, 1);
            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE engagement_id = 'eng-f1' AND action = 'elevated_from_result'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(activity, 1);
            let sync: i64 = conn.query_row(
                "SELECT COUNT(*) FROM SyncRecord
                 WHERE entity_type = 'Finding' AND entity_id = ?1",
                params![finding.id],
                |r| r.get(0),
            )?;
            assert_eq!(sync, 1);
            Ok(())
        })
        .unwrap();
        cleanup(&db_path);
    }

    #[test]
    fn elevate_rejects_pass_test_result() {
        let (db, db_path) = seeded_db("firm-f2", "user-f2", "client-f2", "eng-f2");
        let auth = session_for("firm-f2", "user-f2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-f2".into(),
                library_control_id: library_control_id(&db, "UAM-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let test_id = clone.test_ids[0].clone();
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-f2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "ad.csv".into(),
                mime_type: None,
                content: b"email,enabled\nfine@a.com,TRUE\n".to_vec(),
            },
        )
        .unwrap();
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-f2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_leavers".into(),
                filename: "leavers.csv".into(),
                mime_type: None,
                content: b"email\ngone@a.com\n".to_vec(),
            },
        )
        .unwrap();
        let run = run_access_review(
            &db,
            &auth,
            &paths,
            RunAccessReviewInput {
                test_id,
                ad_import_id: None,
                leavers_import_id: None,
            },
        )
        .unwrap();
        assert_eq!(run.outcome, "pass");

        let err = elevate_finding(
            &db,
            &auth,
            ElevateFindingInput {
                test_result_id: run.test_result_id,
                title: None,
                severity_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Message(_)));
        cleanup(&db_path);
    }

    #[test]
    fn elevate_rejects_cross_firm() {
        let (db, db_path) = seeded_db("firm-f3", "user-f3", "client-f3", "eng-f3");
        let owner = session_for("firm-f3", "user-f3");
        let other = session_for("firm-other", "user-f3");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());
        let result_id = seed_with_exception(&db, &owner, &paths, "eng-f3");

        let err = elevate_finding(
            &db,
            &other,
            ElevateFindingInput {
                test_result_id: result_id,
                title: None,
                severity_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&db_path);
    }

    #[test]
    fn list_findings_returns_newest_first_with_links() {
        let (db, db_path) = seeded_db("firm-f4", "user-f4", "client-f4", "eng-f4");
        let auth = session_for("firm-f4", "user-f4");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());
        let result_id = seed_with_exception(&db, &auth, &paths, "eng-f4");

        let first = elevate_finding(
            &db,
            &auth,
            ElevateFindingInput {
                test_result_id: result_id.clone(),
                title: Some("First".into()),
                severity_id: Some("sev-high".into()),
            },
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = elevate_finding(
            &db,
            &auth,
            ElevateFindingInput {
                test_result_id: result_id.clone(),
                title: Some("Second".into()),
                severity_id: Some("sev-low".into()),
            },
        )
        .unwrap();

        let findings = list_findings(&db, &auth, "eng-f4".into()).unwrap();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].id, second.id);
        assert_eq!(findings[0].code, "F-002");
        assert_eq!(findings[1].id, first.id);
        assert_eq!(findings[1].code, "F-001");
        assert_eq!(findings[0].linked_test_result_ids, vec![result_id.clone()]);
        assert_eq!(findings[1].severity_id.as_deref(), Some("sev-high"));
        cleanup(&db_path);
    }

    #[test]
    fn list_severities_returns_builtin_rows_sorted() {
        let (db, db_path) = seeded_db("firm-f5", "user-f5", "client-f5", "eng-f5");
        let auth = session_for("firm-f5", "user-f5");
        let sevs = list_severities(&db, &auth).unwrap();
        assert_eq!(sevs.len(), 5);
        assert_eq!(sevs[0].id, "sev-critical");
        assert_eq!(sevs[4].id, "sev-observation");
        cleanup(&db_path);
    }

    #[test]
    fn update_finding_edits_fields_and_records_change_log() {
        let (db, db_path) = seeded_db("firm-f6", "user-f6", "client-f6", "eng-f6");
        let auth = session_for("firm-f6", "user-f6");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());
        let result_id = seed_with_exception(&db, &auth, &paths, "eng-f6");
        let finding = elevate_finding(
            &db,
            &auth,
            ElevateFindingInput {
                test_result_id: result_id,
                title: None,
                severity_id: None,
            },
        )
        .unwrap();

        let updated = update_finding(
            &db,
            &auth,
            UpdateFindingInput {
                finding_id: finding.id.clone(),
                title: "Terminated employees retained access".into(),
                condition_text: Some(
                    "Two terminated employees still had enabled AD accounts 30 days after departure.".into(),
                ),
                recommendation_text: Some(
                    "Disable the identified accounts within 24 hours and re-run the matcher.".into(),
                ),
                severity_id: "sev-high".into(),
            },
        )
        .unwrap();

        assert_eq!(updated.title, "Terminated employees retained access");
        assert_eq!(updated.severity_id.as_deref(), Some("sev-high"));
        assert_eq!(updated.severity_name.as_deref(), Some("High"));
        assert!(updated.recommendation_text.as_deref().unwrap().contains("24 hours"));

        db.with(|conn| {
            let sync_id: String = conn.query_row(
                "SELECT id FROM SyncRecord
                 WHERE entity_type = 'Finding' AND entity_id = ?1",
                params![finding.id],
                |r| r.get(0),
            )?;
            let version: i64 = conn.query_row(
                "SELECT version FROM SyncRecord WHERE id = ?1",
                params![sync_id],
                |r| r.get(0),
            )?;
            assert_eq!(version, 2);

            let changes: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ChangeLog WHERE sync_record_id = ?1",
                params![sync_id],
                |r| r.get(0),
            )?;
            // One whole-row entry from elevation (field_name='.') + four
            // field-level entries from this update.
            assert_eq!(changes, 5);

            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE entity_type = 'Finding'
                   AND entity_id = ?1
                   AND action = 'edited'",
                params![finding.id],
                |r| r.get(0),
            )?;
            assert_eq!(activity, 1);
            Ok(())
        })
        .unwrap();
        cleanup(&db_path);
    }

    #[test]
    fn update_finding_is_noop_when_nothing_changes() {
        let (db, db_path) = seeded_db("firm-f7", "user-f7", "client-f7", "eng-f7");
        let auth = session_for("firm-f7", "user-f7");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());
        let result_id = seed_with_exception(&db, &auth, &paths, "eng-f7");
        let finding = elevate_finding(
            &db,
            &auth,
            ElevateFindingInput {
                test_result_id: result_id,
                title: None,
                severity_id: None,
            },
        )
        .unwrap();

        let _ = update_finding(
            &db,
            &auth,
            UpdateFindingInput {
                finding_id: finding.id.clone(),
                title: finding.title.clone(),
                condition_text: finding.condition_text.clone(),
                recommendation_text: finding.recommendation_text.clone(),
                severity_id: finding.severity_id.clone().unwrap(),
            },
        )
        .unwrap();

        db.with(|conn| {
            let version: i64 = conn.query_row(
                "SELECT version FROM SyncRecord
                 WHERE entity_type = 'Finding' AND entity_id = ?1",
                params![finding.id],
                |r| r.get(0),
            )?;
            assert_eq!(version, 1);
            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE entity_type = 'Finding'
                   AND entity_id = ?1
                   AND action = 'edited'",
                params![finding.id],
                |r| r.get(0),
            )?;
            assert_eq!(activity, 0);
            Ok(())
        })
        .unwrap();
        cleanup(&db_path);
    }

    #[test]
    fn update_finding_rejects_cross_firm() {
        let (db, db_path) = seeded_db("firm-f8", "user-f8", "client-f8", "eng-f8");
        let owner = session_for("firm-f8", "user-f8");
        let other = session_for("firm-other", "user-f8");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());
        let result_id = seed_with_exception(&db, &owner, &paths, "eng-f8");
        let finding = elevate_finding(
            &db,
            &owner,
            ElevateFindingInput {
                test_result_id: result_id,
                title: None,
                severity_id: None,
            },
        )
        .unwrap();

        let err = update_finding(
            &db,
            &other,
            UpdateFindingInput {
                finding_id: finding.id,
                title: "hijack".into(),
                condition_text: None,
                recommendation_text: None,
                severity_id: "sev-high".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&db_path);
    }

    #[test]
    fn update_finding_rejects_unknown_severity() {
        let (db, db_path) = seeded_db("firm-f9", "user-f9", "client-f9", "eng-f9");
        let auth = session_for("firm-f9", "user-f9");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());
        let result_id = seed_with_exception(&db, &auth, &paths, "eng-f9");
        let finding = elevate_finding(
            &db,
            &auth,
            ElevateFindingInput {
                test_result_id: result_id,
                title: None,
                severity_id: None,
            },
        )
        .unwrap();

        let err = update_finding(
            &db,
            &auth,
            UpdateFindingInput {
                finding_id: finding.id,
                title: finding.title,
                condition_text: finding.condition_text,
                recommendation_text: finding.recommendation_text,
                severity_id: "sev-bogus".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&db_path);
    }

    #[test]
    fn update_finding_rejects_missing_id() {
        let (db, db_path) = seeded_db("firm-f10", "user-f10", "client-f10", "eng-f10");
        let auth = session_for("firm-f10", "user-f10");
        let err = update_finding(
            &db,
            &auth,
            UpdateFindingInput {
                finding_id: "does-not-exist".into(),
                title: "ignored".into(),
                condition_text: None,
                recommendation_text: None,
                severity_id: "sev-medium".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&db_path);
    }
}
