//! Fieldwork & testing commands (Module 6 plus its downstream testing hooks).
//!
//! `engagement_add_library_control` is the first real mutation in this module:
//! it clones a `LibraryControl` (with its related `LibraryRisk`s and
//! `TestProcedure`s) into the engagement as `EngagementRisk`,
//! `EngagementControl`, and `Test` rows. This is the entry point for the
//! access-review vertical slice — the UI picks a control like `UAM-C-001`,
//! scopes it to a System (AD, Entra, a core banking app), and the clone
//! produces the fieldwork artefacts the auditor then executes against.
//!
//! Design notes:
//!   - **Risks are idempotent within an engagement**. If two library controls
//!     reference the same risk code, cloning both produces one
//!     `EngagementRisk` row, not two. The lookup is by
//!     `UNIQUE(engagement_id, code)`.
//!   - **Controls are strict-insert**. `UNIQUE(engagement_id, code)` on
//!     `EngagementControl` means calling this twice for the same library
//!     control bubbles up a SQLite constraint error. The UI should prevent
//!     that by hiding already-cloned controls from the picker.
//!   - **Tests are always created fresh**. One per `TestProcedure`, scoped to
//!     the provided `system_id`.
//!   - **Audit trail**: every new row gets a `SyncRecord` and a whole-row
//!     `ChangeLog` entry (`field_name = "."` per `DATA_MODEL.md`). One
//!     `ActivityLog` entry summarises the clone.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;
use uuid::Uuid;

use crate::{
    auth::AuthState,
    blobs,
    db::DbState,
    error::{AppError, AppResult},
    matcher::{access_review, csv as csv_parser},
    paths::AppPaths,
};

/// Cap uploaded file size. AD/Entra and HR exports in practice are well
/// under this; the cap stops a misbehaving client from exhausting memory on
/// the encrypt path. If a legitimate import ever hits the limit we can
/// stream — but CSVs that large are already a workflow smell.
const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct AddLibraryControlInput {
    pub engagement_id: String,
    pub library_control_id: String,
    pub system_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AddLibraryControlResult {
    pub engagement_control_id: String,
    pub engagement_risk_ids: Vec<String>,
    pub test_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UploadDataImportInput {
    pub engagement_id: String,
    pub system_id: Option<String>,
    pub source_kind: String,
    pub purpose_tag: String,
    pub filename: String,
    pub mime_type: Option<String>,
    pub content: Vec<u8>,
}

#[derive(Debug, Serialize, Clone)]
pub struct DataImportSummary {
    pub id: String,
    pub filename: Option<String>,
    pub source_kind: String,
    pub purpose_tag: Option<String>,
    pub row_count: Option<i64>,
    pub plaintext_size: Option<i64>,
    pub imported_at: i64,
    pub imported_by: Option<String>,
    pub imported_by_name: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TestSummary {
    pub id: String,
    pub engagement_control_id: String,
    pub control_code: String,
    pub control_title: String,
    pub code: String,
    pub name: String,
    pub objective: String,
    pub automation_tier: String,
    pub status: String,
    pub latest_result_outcome: Option<String>,
    pub latest_result_at: Option<i64>,
    pub latest_result_evidence_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RunAccessReviewInput {
    pub test_id: String,
    pub ad_import_id: Option<String>,
    pub leavers_import_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AccessReviewRunResult {
    pub test_result_id: String,
    pub outcome: String,
    pub exception_count: i64,
    pub ad_import_id: String,
    pub ad_import_filename: Option<String>,
    pub leavers_import_id: String,
    pub leavers_import_filename: Option<String>,
    pub ad_rows_considered: i64,
    pub leaver_rows_considered: i64,
    pub ad_rows_skipped_disabled: i64,
    pub ad_rows_skipped_unmatchable: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct TestResultSummary {
    pub id: String,
    pub test_id: String,
    pub test_code: String,
    pub test_name: String,
    pub outcome: String,
    pub exception_summary: Option<String>,
    pub evidence_count: i64,
    pub performed_at: i64,
    pub performed_by_name: Option<String>,
    pub population_ref_label: Option<String>,
    pub detail_json: Option<String>,
    pub notes_blob_id: Option<String>,
    pub has_linked_finding: bool,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// Library `automation_hint` uses hyphenated tokens (`rule-based`). The Test
// `automation_tier` ladder uses underscored tokens (`rule_based`,
// `classical_ml`, ...). Downstream matchers filter on the engagement-side
// value, so we normalise on the way in.
fn normalise_automation_tier(hint: &str) -> String {
    hint.replace('-', "_")
}

#[tauri::command]
pub fn engagement_add_library_control(
    input: AddLibraryControlInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<AddLibraryControlResult> {
    clone_library_control(db.inner(), auth.inner(), input)
}

pub(crate) fn clone_library_control(
    db: &DbState,
    auth: &AuthState,
    input: AddLibraryControlInput,
) -> AppResult<AddLibraryControlResult> {
    let session = auth.require()?;

    let engagement_id = input.engagement_id.trim().to_string();
    let library_control_id = input.library_control_id.trim().to_string();
    let system_id = input
        .system_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if engagement_id.is_empty() {
        return Err(AppError::Message("engagement is required".into()));
    }
    if library_control_id.is_empty() {
        return Err(AppError::Message("library control is required".into()));
    }

    let now = now_secs();

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        // Cross-firm guard: engagement -> client -> firm must match the
        // signed-in user's firm. Without this an IPC payload could clone into
        // another firm's engagement.
        let eng_firm: Option<String> = tx
            .query_row(
                "SELECT c.firm_id
                 FROM Engagement e
                 JOIN Client c ON c.id = e.client_id
                 WHERE e.id = ?1",
                params![engagement_id],
                |r| r.get(0),
            )
            .optional()?;
        match eng_firm {
            Some(f) if f == session.firm_id => {}
            _ => return Err(AppError::NotFound("engagement not found".into())),
        }

        // If the caller provided a system_id, it must belong to this
        // engagement. (Systems are engagement-scoped, so a cross-engagement
        // system id would be nonsensical even within the same firm.)
        if let Some(sys_id) = &system_id {
            let sys_eng: Option<String> = tx
                .query_row(
                    "SELECT engagement_id FROM System WHERE id = ?1",
                    params![sys_id],
                    |r| r.get(0),
                )
                .optional()?;
            match sys_eng {
                Some(e) if e == engagement_id => {}
                _ => return Err(AppError::NotFound("system not found".into())),
            }
        }

        let lib_control = tx
            .query_row(
                "SELECT id, code, title, description, objective,
                        control_type, frequency, related_risk_ids_json, library_version
                 FROM LibraryControl
                 WHERE id = ?1 AND superseded_by IS NULL",
                params![library_control_id],
                |row| {
                    Ok(LibraryControlRow {
                        id: row.get(0)?,
                        code: row.get(1)?,
                        title: row.get(2)?,
                        description: row.get(3)?,
                        objective: row.get(4)?,
                        control_type: row.get(5)?,
                        frequency: row.get(6)?,
                        related_risk_ids_json: row.get(7)?,
                        library_version: row.get(8)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("library control not found".into()))?;

        let related_library_risk_ids: Vec<String> = lib_control
            .related_risk_ids_json
            .as_deref()
            .map(|s| serde_json::from_str::<Vec<String>>(s).unwrap_or_default())
            .unwrap_or_default();

        let applicable_systems_json = system_id.as_ref().map(|s| json!([s]).to_string());

        let mut engagement_risk_ids: Vec<String> = Vec::with_capacity(
            related_library_risk_ids.len(),
        );
        for lib_risk_id in &related_library_risk_ids {
            let lib_risk = tx
                .query_row(
                    "SELECT id, code, title, description,
                            default_inherent_rating, library_version
                     FROM LibraryRisk
                     WHERE id = ?1",
                    params![lib_risk_id],
                    |row| {
                        Ok(LibraryRiskRow {
                            id: row.get(0)?,
                            code: row.get(1)?,
                            title: row.get(2)?,
                            description: row.get(3)?,
                            default_inherent_rating: row.get(4)?,
                            library_version: row.get(5)?,
                        })
                    },
                )
                .optional()?
                .ok_or_else(|| {
                    AppError::Message(format!(
                        "library control {} references missing risk {}",
                        lib_control.code, lib_risk_id
                    ))
                })?;

            // Idempotency: the risk may already exist on this engagement from
            // an earlier clone of a sibling control.
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM EngagementRisk
                     WHERE engagement_id = ?1 AND code = ?2",
                    params![engagement_id, lib_risk.code],
                    |r| r.get(0),
                )
                .optional()?;

            let risk_id = match existing {
                Some(id) => id,
                None => {
                    let new_id = Uuid::now_v7().to_string();
                    let inherent_rating = lib_risk
                        .default_inherent_rating
                        .clone()
                        .unwrap_or_else(|| "medium".into());

                    tx.execute(
                        "INSERT INTO EngagementRisk (
                            id, engagement_id, derived_from, source_library_version,
                            prior_engagement_risk_id, code, title, description,
                            inherent_rating, residual_rating,
                            applicable_system_ids_json, notes_blob_id,
                            created_by, created_at
                         ) VALUES (
                            ?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, NULL, ?9, NULL, ?10, ?11
                         )",
                        params![
                            new_id,
                            engagement_id,
                            lib_risk.id,
                            lib_risk.library_version,
                            lib_risk.code,
                            lib_risk.title,
                            lib_risk.description,
                            inherent_rating,
                            applicable_systems_json,
                            session.user_id,
                            now,
                        ],
                    )?;

                    let sync_id = Uuid::now_v7().to_string();
                    tx.execute(
                        "INSERT INTO SyncRecord (
                            id, entity_type, entity_id, last_modified_at,
                            last_modified_by, version, deleted, sync_state
                         ) VALUES (?1, 'EngagementRisk', ?2, ?3, ?4, 1, 0, 'local_only')",
                        params![sync_id, new_id, now, session.user_id],
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
                                "engagement_id": engagement_id,
                                "derived_from": lib_risk.id,
                                "source_library_version": lib_risk.library_version,
                                "code": lib_risk.code,
                                "title": lib_risk.title,
                                "inherent_rating": inherent_rating,
                            })
                            .to_string(),
                        ],
                    )?;

                    new_id
                }
            };

            engagement_risk_ids.push(risk_id);
        }

        let engagement_control_id = Uuid::now_v7().to_string();
        let related_eng_risk_ids_json = if engagement_risk_ids.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&engagement_risk_ids)?)
        };

        tx.execute(
            "INSERT INTO EngagementControl (
                id, engagement_id, derived_from, source_library_version,
                prior_engagement_control_id, code, title, description,
                objective, control_type, frequency,
                design_assessment, operating_assessment,
                related_engagement_risk_ids_json,
                applicable_system_ids_json, notes_blob_id,
                created_by, created_at
             ) VALUES (
                ?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10,
                NULL, NULL, ?11, ?12, NULL, ?13, ?14
             )",
            params![
                engagement_control_id,
                engagement_id,
                lib_control.id,
                lib_control.library_version,
                lib_control.code,
                lib_control.title,
                lib_control.description,
                lib_control.objective,
                lib_control.control_type,
                lib_control.frequency,
                related_eng_risk_ids_json,
                applicable_systems_json,
                session.user_id,
                now,
            ],
        )?;

        let control_sync_id = Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO SyncRecord (
                id, entity_type, entity_id, last_modified_at,
                last_modified_by, version, deleted, sync_state
             ) VALUES (?1, 'EngagementControl', ?2, ?3, ?4, 1, 0, 'local_only')",
            params![control_sync_id, engagement_control_id, now, session.user_id],
        )?;
        tx.execute(
            "INSERT INTO ChangeLog (
                id, sync_record_id, occurred_at, user_id,
                field_name, old_value_json, new_value_json
             ) VALUES (?1, ?2, ?3, ?4, '.', NULL, ?5)",
            params![
                Uuid::now_v7().to_string(),
                control_sync_id,
                now,
                session.user_id,
                json!({
                    "engagement_id": engagement_id,
                    "derived_from": lib_control.id,
                    "source_library_version": lib_control.library_version,
                    "code": lib_control.code,
                    "title": lib_control.title,
                    "control_type": lib_control.control_type,
                    "related_engagement_risk_ids": engagement_risk_ids,
                })
                .to_string(),
            ],
        )?;

        // Collect TestProcedures first, then iterate without holding the
        // prepared statement across tx.execute borrows.
        let tp_rows: Vec<TestProcedureRow> = {
            let mut stmt = tx.prepare(
                "SELECT id, code, name, objective, steps_json,
                        automation_hint, library_version
                 FROM TestProcedure
                 WHERE control_id = ?1 AND library_version = ?2
                 ORDER BY code",
            )?;
            let rows = stmt
                .query_map(
                    params![lib_control.id, lib_control.library_version],
                    |row| {
                        Ok(TestProcedureRow {
                            id: row.get(0)?,
                            code: row.get(1)?,
                            name: row.get(2)?,
                            objective: row.get(3)?,
                            steps_json: row.get(4)?,
                            automation_hint: row.get(5)?,
                            library_version: row.get(6)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };

        let mut test_ids: Vec<String> = Vec::with_capacity(tp_rows.len());
        for tp in tp_rows {
            let test_id = Uuid::now_v7().to_string();
            let automation_tier = normalise_automation_tier(&tp.automation_hint);

            tx.execute(
                "INSERT INTO Test (
                    id, engagement_id, engagement_control_id, system_id,
                    derived_from, source_library_version, prior_test_id,
                    code, name, objective, steps_json, automation_tier,
                    assigned_to, status,
                    planned_start_date, planned_end_date,
                    actual_started_at, actual_completed_at,
                    notes_blob_id, created_by, created_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, NULL,
                    ?7, ?8, ?9, ?10, ?11,
                    NULL, 'not_started',
                    NULL, NULL, NULL, NULL, NULL, ?12, ?13
                 )",
                params![
                    test_id,
                    engagement_id,
                    engagement_control_id,
                    system_id,
                    tp.id,
                    tp.library_version,
                    tp.code,
                    tp.name,
                    tp.objective,
                    tp.steps_json,
                    automation_tier,
                    session.user_id,
                    now,
                ],
            )?;

            let test_sync_id = Uuid::now_v7().to_string();
            tx.execute(
                "INSERT INTO SyncRecord (
                    id, entity_type, entity_id, last_modified_at,
                    last_modified_by, version, deleted, sync_state
                 ) VALUES (?1, 'Test', ?2, ?3, ?4, 1, 0, 'local_only')",
                params![test_sync_id, test_id, now, session.user_id],
            )?;
            tx.execute(
                "INSERT INTO ChangeLog (
                    id, sync_record_id, occurred_at, user_id,
                    field_name, old_value_json, new_value_json
                 ) VALUES (?1, ?2, ?3, ?4, '.', NULL, ?5)",
                params![
                    Uuid::now_v7().to_string(),
                    test_sync_id,
                    now,
                    session.user_id,
                    json!({
                        "engagement_id": engagement_id,
                        "engagement_control_id": engagement_control_id,
                        "system_id": system_id,
                        "derived_from": tp.id,
                        "source_library_version": tp.library_version,
                        "code": tp.code,
                        "name": tp.name,
                        "automation_tier": automation_tier,
                        "status": "not_started",
                    })
                    .to_string(),
                ],
            )?;

            test_ids.push(test_id);
        }

        tx.execute(
            "INSERT INTO ActivityLog (
                id, engagement_id, entity_type, entity_id,
                action, performed_by, performed_at, summary
             ) VALUES (?1, ?2, 'EngagementControl', ?3, 'cloned_from_library', ?4, ?5, ?6)",
            params![
                Uuid::now_v7().to_string(),
                engagement_id,
                engagement_control_id,
                session.user_id,
                now,
                format!(
                    "Added library control {} ({}) with {} test(s)",
                    lib_control.code,
                    lib_control.title,
                    test_ids.len(),
                ),
            ],
        )?;

        tx.commit()?;

        tracing::info!(
            engagement_id = %engagement_id,
            library_control_code = %lib_control.code,
            engagement_control_id = %engagement_control_id,
            risks = engagement_risk_ids.len(),
            tests = test_ids.len(),
            "library control cloned into engagement"
        );

        Ok(AddLibraryControlResult {
            engagement_control_id,
            engagement_risk_ids,
            test_ids,
        })
    })
}

#[tauri::command]
pub fn engagement_upload_data_import(
    input: UploadDataImportInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
    paths: State<'_, AppPaths>,
) -> AppResult<DataImportSummary> {
    upload_data_import(db.inner(), auth.inner(), paths.inner(), input)
}

#[tauri::command]
pub fn engagement_list_data_imports(
    engagement_id: String,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Vec<DataImportSummary>> {
    list_data_imports(db.inner(), auth.inner(), engagement_id)
}

#[tauri::command]
pub fn engagement_list_tests(
    engagement_id: String,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Vec<TestSummary>> {
    list_tests(db.inner(), auth.inner(), engagement_id)
}

#[tauri::command]
pub fn engagement_run_access_review(
    input: RunAccessReviewInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
    paths: State<'_, AppPaths>,
) -> AppResult<AccessReviewRunResult> {
    run_access_review(db.inner(), auth.inner(), paths.inner(), input)
}

#[tauri::command]
pub fn engagement_list_test_results(
    engagement_id: String,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Vec<TestResultSummary>> {
    list_test_results(db.inner(), auth.inner(), engagement_id)
}

pub(crate) fn upload_data_import(
    db: &DbState,
    auth: &AuthState,
    paths: &AppPaths,
    input: UploadDataImportInput,
) -> AppResult<DataImportSummary> {
    let (session, master_key) = auth.require_keyed()?;

    let engagement_id = input.engagement_id.trim().to_string();
    let system_id = input
        .system_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let source_kind = input.source_kind.trim().to_string();
    let purpose_tag = input.purpose_tag.trim().to_string();
    let filename = input.filename.trim().to_string();
    let mime_type = input
        .mime_type
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if engagement_id.is_empty() {
        return Err(AppError::Message("engagement is required".into()));
    }
    if source_kind.is_empty() {
        return Err(AppError::Message("source kind is required".into()));
    }
    if purpose_tag.is_empty() {
        return Err(AppError::Message("purpose tag is required".into()));
    }
    if filename.is_empty() {
        return Err(AppError::Message("filename is required".into()));
    }
    if input.content.is_empty() {
        return Err(AppError::Message("file is empty".into()));
    }
    if input.content.len() > MAX_UPLOAD_BYTES {
        return Err(AppError::Message(format!(
            "file exceeds {} MB limit",
            MAX_UPLOAD_BYTES / (1024 * 1024)
        )));
    }

    let (row_count, schema_json) = if source_kind.eq_ignore_ascii_case("csv") {
        parse_csv_metadata(&input.content)?
    } else {
        (None, None)
    };

    let now = now_secs();
    let plaintext_size = input.content.len() as i64;

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        let eng_firm: Option<String> = tx
            .query_row(
                "SELECT c.firm_id
                 FROM Engagement e
                 JOIN Client c ON c.id = e.client_id
                 WHERE e.id = ?1",
                params![engagement_id],
                |r| r.get(0),
            )
            .optional()?;
        match eng_firm {
            Some(f) if f == session.firm_id => {}
            _ => return Err(AppError::NotFound("engagement not found".into())),
        }

        if let Some(sys_id) = &system_id {
            let sys_eng: Option<String> = tx
                .query_row(
                    "SELECT engagement_id FROM System WHERE id = ?1",
                    params![sys_id],
                    |r| r.get(0),
                )
                .optional()?;
            match sys_eng {
                Some(e) if e == engagement_id => {}
                _ => return Err(AppError::NotFound("system not found".into())),
            }
        }

        let data_import_id = Uuid::now_v7().to_string();
        let written = blobs::write_engagement_blob(
            &tx,
            &paths.app_data_dir,
            &engagement_id,
            Some("DataImport"),
            Some(&data_import_id),
            Some(&filename),
            mime_type.as_deref(),
            &input.content,
            &master_key,
            now,
        )?;

        tx.execute(
            "INSERT INTO DataImport (
                id, engagement_id, system_id, connector_id,
                source_kind, filename, blob_id, row_count,
                sha256_plaintext, schema_json, purpose_tag,
                imported_by, imported_at
             ) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                data_import_id,
                engagement_id,
                system_id,
                source_kind,
                filename,
                written.id,
                row_count,
                written.sha256_plaintext,
                schema_json,
                purpose_tag,
                session.user_id,
                now,
            ],
        )?;

        let sync_id = Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO SyncRecord (
                id, entity_type, entity_id, last_modified_at,
                last_modified_by, version, deleted, sync_state
             ) VALUES (?1, 'DataImport', ?2, ?3, ?4, 1, 0, 'local_only')",
            params![sync_id, data_import_id, now, session.user_id],
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
                    "engagement_id": engagement_id,
                    "system_id": system_id,
                    "source_kind": source_kind,
                    "filename": filename,
                    "blob_id": written.id,
                    "row_count": row_count,
                    "sha256_plaintext": written.sha256_plaintext,
                    "purpose_tag": purpose_tag,
                })
                .to_string(),
            ],
        )?;
        tx.execute(
            "INSERT INTO ActivityLog (
                id, engagement_id, entity_type, entity_id,
                action, performed_by, performed_at, summary
             ) VALUES (?1, ?2, 'DataImport', ?3, 'uploaded', ?4, ?5, ?6)",
            params![
                Uuid::now_v7().to_string(),
                engagement_id,
                data_import_id,
                session.user_id,
                now,
                format!(
                    "Uploaded {} ({}) tagged '{}'",
                    filename,
                    row_count
                        .map(|n| format!("{n} rows"))
                        .unwrap_or_else(|| format!("{plaintext_size} bytes")),
                    purpose_tag,
                ),
            ],
        )?;

        tx.commit()?;

        tracing::info!(
            engagement_id = %engagement_id,
            data_import_id = %data_import_id,
            filename = %filename,
            purpose_tag = %purpose_tag,
            row_count = row_count.unwrap_or(-1),
            "data import uploaded"
        );

        Ok(DataImportSummary {
            id: data_import_id,
            filename: Some(filename),
            source_kind,
            purpose_tag: Some(purpose_tag),
            row_count,
            plaintext_size: Some(plaintext_size),
            imported_at: now,
            imported_by: Some(session.user_id.clone()),
            imported_by_name: Some(session.display_name.clone()),
        })
    })
}

pub(crate) fn list_data_imports(
    db: &DbState,
    auth: &AuthState,
    engagement_id: String,
) -> AppResult<Vec<DataImportSummary>> {
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
            "SELECT di.id, di.filename, di.source_kind, di.purpose_tag,
                    di.row_count,
                    (SELECT eb.plaintext_size FROM EncryptedBlob eb WHERE eb.id = di.blob_id),
                    di.imported_at, di.imported_by, u.display_name
             FROM DataImport di
             LEFT JOIN User u ON u.id = di.imported_by
             WHERE di.engagement_id = ?1
             ORDER BY di.imported_at DESC",
        )?;
        let rows = stmt
            .query_map(params![engagement_id], |row| {
                Ok(DataImportSummary {
                    id: row.get(0)?,
                    filename: row.get(1)?,
                    source_kind: row.get(2)?,
                    purpose_tag: row.get(3)?,
                    row_count: row.get(4)?,
                    plaintext_size: row.get(5)?,
                    imported_at: row.get(6)?,
                    imported_by: row.get(7)?,
                    imported_by_name: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub(crate) fn list_tests(
    db: &DbState,
    auth: &AuthState,
    engagement_id: String,
) -> AppResult<Vec<TestSummary>> {
    let session = auth.require()?;
    let engagement_id = engagement_id.trim().to_string();
    if engagement_id.is_empty() {
        return Err(AppError::Message("engagement is required".into()));
    }

    db.with(|conn| {
        assert_engagement_in_firm(conn, &engagement_id, &session.firm_id)?;

        let mut stmt = conn.prepare(
            "SELECT t.id, t.engagement_control_id, ec.code, ec.title,
                    t.code, t.name, t.objective, t.automation_tier, t.status,
                    (SELECT tr.outcome
                       FROM TestResult tr
                       WHERE tr.test_id = t.id
                       ORDER BY tr.performed_at DESC LIMIT 1),
                    (SELECT tr.performed_at
                       FROM TestResult tr
                       WHERE tr.test_id = t.id
                       ORDER BY tr.performed_at DESC LIMIT 1),
                    (SELECT tr.evidence_count
                       FROM TestResult tr
                       WHERE tr.test_id = t.id
                       ORDER BY tr.performed_at DESC LIMIT 1)
             FROM Test t
             JOIN EngagementControl ec ON ec.id = t.engagement_control_id
             WHERE t.engagement_id = ?1
             ORDER BY ec.code, t.code",
        )?;
        let rows = stmt
            .query_map(params![engagement_id], |row| {
                Ok(TestSummary {
                    id: row.get(0)?,
                    engagement_control_id: row.get(1)?,
                    control_code: row.get(2)?,
                    control_title: row.get(3)?,
                    code: row.get(4)?,
                    name: row.get(5)?,
                    objective: row.get(6)?,
                    automation_tier: row.get(7)?,
                    status: row.get(8)?,
                    latest_result_outcome: row.get(9)?,
                    latest_result_at: row.get(10)?,
                    latest_result_evidence_count: row.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub(crate) fn run_access_review(
    db: &DbState,
    auth: &AuthState,
    paths: &AppPaths,
    input: RunAccessReviewInput,
) -> AppResult<AccessReviewRunResult> {
    let (session, master_key) = auth.require_keyed()?;
    let test_id = input.test_id.trim().to_string();
    if test_id.is_empty() {
        return Err(AppError::Message("test is required".into()));
    }
    let ad_import_override = input
        .ad_import_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let leavers_import_override = input
        .leavers_import_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let now = now_secs();

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        let test = tx
            .query_row(
                "SELECT id, engagement_id, code, name, status
                 FROM Test WHERE id = ?1",
                params![test_id],
                |r| {
                    Ok(TestRow {
                        id: r.get(0)?,
                        engagement_id: r.get(1)?,
                        code: r.get(2)?,
                        name: r.get(3)?,
                        status: r.get(4)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("test not found".into()))?;

        let engagement_id = test.engagement_id.clone();

        let firm_ok: bool = tx
            .query_row(
                "SELECT 1
                 FROM Engagement e
                 JOIN Client c ON c.id = e.client_id
                 WHERE e.id = ?1 AND c.firm_id = ?2",
                params![engagement_id, session.firm_id],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        if !firm_ok {
            return Err(AppError::NotFound("test not found".into()));
        }

        let ad_import = resolve_import(
            &tx,
            &engagement_id,
            ad_import_override.as_deref(),
            &["ad_export", "entra_export"],
            "AD or Entra export",
        )?;
        let leavers_import = resolve_import(
            &tx,
            &engagement_id,
            leavers_import_override.as_deref(),
            &["hr_leavers"],
            "HR leavers list",
        )?;

        let ad_blob_id = ad_import
            .blob_id
            .clone()
            .ok_or_else(|| AppError::Message("AD export has no attached file".into()))?;
        let leavers_blob_id = leavers_import.blob_id.clone().ok_or_else(|| {
            AppError::Message("HR leavers list has no attached file".into())
        })?;

        let ad_bytes = blobs::read_blob(&tx, &paths.app_data_dir, &ad_blob_id, &master_key)?;
        let leavers_bytes =
            blobs::read_blob(&tx, &paths.app_data_dir, &leavers_blob_id, &master_key)?;

        let ad_text = std::str::from_utf8(&ad_bytes)
            .map_err(|e| AppError::Message(format!("AD export is not valid UTF-8: {e}")))?;
        let leavers_text = std::str::from_utf8(&leavers_bytes).map_err(|e| {
            AppError::Message(format!("HR leavers list is not valid UTF-8: {e}"))
        })?;

        let ad_table = csv_parser::parse(ad_text)?;
        let leavers_table = csv_parser::parse(leavers_text)?;
        let report = access_review::run_terminated_but_active(&ad_table, &leavers_table);

        let report_json = serde_json::to_vec_pretty(&report)?;
        let report_filename = format!("access-review-{}.json", &test.code);
        let written_report = blobs::write_engagement_blob(
            &tx,
            &paths.app_data_dir,
            &engagement_id,
            Some("TestResult"),
            None,
            Some(&report_filename),
            Some("application/json"),
            &report_json,
            &master_key,
            now,
        )?;

        let exception_count = report.exceptions.len() as i64;
        let outcome = if exception_count == 0 { "pass" } else { "exception" };
        let exception_summary = if exception_count == 0 {
            format!(
                "No terminated users still enabled across {} AD rows and {} leaver rows",
                report.ad_rows_considered, report.leaver_rows_considered
            )
        } else {
            format!(
                "{} terminated user{} still enabled in AD",
                exception_count,
                if exception_count == 1 { "" } else { "s" }
            )
        };

        let detail_json = json!({
            "rule": report.rule,
            "ad_import_id": ad_import.id,
            "leavers_import_id": leavers_import.id,
            "ad_rows_considered": report.ad_rows_considered,
            "leaver_rows_considered": report.leaver_rows_considered,
            "ad_rows_skipped_disabled": report.ad_rows_skipped_disabled,
            "ad_rows_skipped_unmatchable": report.ad_rows_skipped_unmatchable,
        })
        .to_string();
        let population_label = format!(
            "AD export: {}",
            ad_import.filename.as_deref().unwrap_or("(unnamed)")
        );

        let test_result_id = Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO TestResult (
                id, test_id, sample_id, outcome, exception_summary,
                evidence_count, performed_by, performed_at,
                notes_blob_id, population_ref, population_ref_label, detail_json
             ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                test_result_id,
                test.id,
                outcome,
                exception_summary,
                exception_count,
                session.user_id,
                now,
                written_report.id,
                ad_blob_id,
                population_label,
                detail_json,
            ],
        )?;

        let tr_sync_id = Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO SyncRecord (
                id, entity_type, entity_id, last_modified_at,
                last_modified_by, version, deleted, sync_state
             ) VALUES (?1, 'TestResult', ?2, ?3, ?4, 1, 0, 'local_only')",
            params![tr_sync_id, test_result_id, now, session.user_id],
        )?;
        tx.execute(
            "INSERT INTO ChangeLog (
                id, sync_record_id, occurred_at, user_id,
                field_name, old_value_json, new_value_json
             ) VALUES (?1, ?2, ?3, ?4, '.', NULL, ?5)",
            params![
                Uuid::now_v7().to_string(),
                tr_sync_id,
                now,
                session.user_id,
                json!({
                    "test_id": test.id,
                    "outcome": outcome,
                    "exception_summary": exception_summary,
                    "evidence_count": exception_count,
                    "notes_blob_id": written_report.id,
                    "population_ref": ad_blob_id,
                })
                .to_string(),
            ],
        )?;

        let new_test_status = "in_review";
        if test.status != new_test_status {
            tx.execute(
                "UPDATE Test SET status = ?1 WHERE id = ?2",
                params![new_test_status, test.id],
            )?;
            let test_sync_id: Option<String> = tx
                .query_row(
                    "SELECT id FROM SyncRecord
                     WHERE entity_type = 'Test' AND entity_id = ?1",
                    params![test.id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(sid) = test_sync_id {
                tx.execute(
                    "UPDATE SyncRecord
                     SET last_modified_at = ?1, last_modified_by = ?2, version = version + 1
                     WHERE id = ?3",
                    params![now, session.user_id, sid],
                )?;
                tx.execute(
                    "INSERT INTO ChangeLog (
                        id, sync_record_id, occurred_at, user_id,
                        field_name, old_value_json, new_value_json
                     ) VALUES (?1, ?2, ?3, ?4, 'status', ?5, ?6)",
                    params![
                        Uuid::now_v7().to_string(),
                        sid,
                        now,
                        session.user_id,
                        json!(test.status).to_string(),
                        json!(new_test_status).to_string(),
                    ],
                )?;
            }
        }

        tx.execute(
            "INSERT INTO ActivityLog (
                id, engagement_id, entity_type, entity_id,
                action, performed_by, performed_at, summary
             ) VALUES (?1, ?2, 'TestResult', ?3, 'matcher_run', ?4, ?5, ?6)",
            params![
                Uuid::now_v7().to_string(),
                engagement_id,
                test_result_id,
                session.user_id,
                now,
                format!(
                    "Access review matcher on {}: {}",
                    test.code, exception_summary
                ),
            ],
        )?;

        tx.commit()?;

        tracing::info!(
            engagement_id = %engagement_id,
            test_id = %test.id,
            test_result_id = %test_result_id,
            outcome,
            exception_count,
            "access review matcher ran"
        );

        Ok(AccessReviewRunResult {
            test_result_id,
            outcome: outcome.to_string(),
            exception_count,
            ad_import_id: ad_import.id,
            ad_import_filename: ad_import.filename,
            leavers_import_id: leavers_import.id,
            leavers_import_filename: leavers_import.filename,
            ad_rows_considered: report.ad_rows_considered as i64,
            leaver_rows_considered: report.leaver_rows_considered as i64,
            ad_rows_skipped_disabled: report.ad_rows_skipped_disabled as i64,
            ad_rows_skipped_unmatchable: report.ad_rows_skipped_unmatchable as i64,
        })
    })
}

pub(crate) fn list_test_results(
    db: &DbState,
    auth: &AuthState,
    engagement_id: String,
) -> AppResult<Vec<TestResultSummary>> {
    let session = auth.require()?;
    let engagement_id = engagement_id.trim().to_string();
    if engagement_id.is_empty() {
        return Err(AppError::Message("engagement is required".into()));
    }

    db.with(|conn| {
        assert_engagement_in_firm(conn, &engagement_id, &session.firm_id)?;

        let mut stmt = conn.prepare(
            "SELECT tr.id, tr.test_id, t.code, t.name, tr.outcome,
                    tr.exception_summary, tr.evidence_count,
                    tr.performed_at, u.display_name,
                    tr.population_ref_label, tr.detail_json, tr.notes_blob_id,
                    EXISTS (
                        SELECT 1 FROM FindingTestResultLink ftl
                        WHERE ftl.test_result_id = tr.id
                    ) AS has_finding
             FROM TestResult tr
             JOIN Test t ON t.id = tr.test_id
             LEFT JOIN User u ON u.id = tr.performed_by
             WHERE t.engagement_id = ?1
             ORDER BY tr.performed_at DESC",
        )?;
        let rows = stmt
            .query_map(params![engagement_id], |row| {
                let has_finding: i64 = row.get(12)?;
                Ok(TestResultSummary {
                    id: row.get(0)?,
                    test_id: row.get(1)?,
                    test_code: row.get(2)?,
                    test_name: row.get(3)?,
                    outcome: row.get(4)?,
                    exception_summary: row.get(5)?,
                    evidence_count: row.get(6)?,
                    performed_at: row.get(7)?,
                    performed_by_name: row.get(8)?,
                    population_ref_label: row.get(9)?,
                    detail_json: row.get(10)?,
                    notes_blob_id: row.get(11)?,
                    has_linked_finding: has_finding != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

fn assert_engagement_in_firm(
    conn: &rusqlite::Connection,
    engagement_id: &str,
    firm_id: &str,
) -> AppResult<()> {
    let row: Option<String> = conn
        .query_row(
            "SELECT c.firm_id
             FROM Engagement e
             JOIN Client c ON c.id = e.client_id
             WHERE e.id = ?1",
            params![engagement_id],
            |r| r.get(0),
        )
        .optional()?;
    match row {
        Some(f) if f == firm_id => Ok(()),
        _ => Err(AppError::NotFound("engagement not found".into())),
    }
}

struct ImportRef {
    id: String,
    filename: Option<String>,
    blob_id: Option<String>,
}

fn resolve_import(
    tx: &rusqlite::Transaction<'_>,
    engagement_id: &str,
    override_id: Option<&str>,
    purpose_tags: &[&str],
    label: &str,
) -> AppResult<ImportRef> {
    if let Some(id) = override_id {
        let row: Option<(String, Option<String>, Option<String>, Option<String>, String)> = tx
            .query_row(
                "SELECT id, filename, blob_id, purpose_tag, engagement_id
                 FROM DataImport WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let (import_id, filename, blob_id, purpose, eng) = row
            .ok_or_else(|| AppError::NotFound(format!("{label} import not found")))?;
        if eng != engagement_id {
            return Err(AppError::NotFound(format!("{label} import not found")));
        }
        match purpose.as_deref() {
            Some(p) if purpose_tags.iter().any(|t| *t == p) => Ok(ImportRef {
                id: import_id,
                filename,
                blob_id,
            }),
            _ => Err(AppError::Message(format!(
                "selected import is not an {label}"
            ))),
        }
    } else {
        // Pick newest matching import for the engagement.
        let placeholders: Vec<String> = (0..purpose_tags.len()).map(|i| format!("?{}", i + 2)).collect();
        let sql = format!(
            "SELECT id, filename, blob_id
             FROM DataImport
             WHERE engagement_id = ?1
               AND purpose_tag IN ({})
             ORDER BY imported_at DESC LIMIT 1",
            placeholders.join(",")
        );
        let mut stmt = tx.prepare(&sql)?;
        let mut bound: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(purpose_tags.len() + 1);
        bound.push(&engagement_id);
        for tag in purpose_tags {
            bound.push(tag);
        }
        let row: Option<(String, Option<String>, Option<String>)> = stmt
            .query_row(bound.as_slice(), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        let (id, filename, blob_id) = row.ok_or_else(|| {
            AppError::Message(format!(
                "no {label} has been uploaded for this engagement"
            ))
        })?;
        Ok(ImportRef {
            id,
            filename,
            blob_id,
        })
    }
}

struct TestRow {
    id: String,
    engagement_id: String,
    code: String,
    #[allow(dead_code)]
    name: String,
    status: String,
}

fn parse_csv_metadata(bytes: &[u8]) -> AppResult<(Option<i64>, Option<String>)> {
    // Best-effort CSV metadata. Strips a UTF-8 BOM, takes the first non-empty
    // line as the header, counts remaining non-empty lines. Quoted commas and
    // escape sequences are not handled — sufficient for the AD / HR / payroll
    // exports the access review expects; Excel-authored CSVs with embedded
    // commas would need a real parser, which we can reach for later if it
    // becomes a problem in practice.
    let text = std::str::from_utf8(bytes)
        .map_err(|e| AppError::Message(format!("CSV is not valid UTF-8: {e}")))?;
    let stripped = text.trim_start_matches('\u{feff}');

    let mut lines = stripped.lines().filter(|l| !l.trim().is_empty());
    let header = match lines.next() {
        Some(h) => h,
        None => return Ok((None, None)),
    };
    let columns: Vec<String> = header
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect();
    let row_count = lines.count() as i64;
    let schema_json = serde_json::to_string(&columns)?;
    Ok((Some(row_count), Some(schema_json)))
}

struct LibraryControlRow {
    id: String,
    code: String,
    title: String,
    description: String,
    objective: String,
    control_type: String,
    frequency: Option<String>,
    related_risk_ids_json: Option<String>,
    library_version: String,
}

struct LibraryRiskRow {
    id: String,
    code: String,
    title: String,
    description: String,
    default_inherent_rating: Option<String>,
    library_version: String,
}

struct TestProcedureRow {
    id: String,
    code: String,
    name: String,
    objective: String,
    steps_json: String,
    automation_hint: String,
    library_version: String,
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
        dir.join(format!("audit-testing-test-{stamp}-{suffix}.db"))
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

    // Seeds Firm + User + Client + Engagement and returns the DB. The library
    // bundle is installed automatically by `open_with_key`, so `LibraryRisk`,
    // `LibraryControl`, and `TestProcedure` rows for version 0.1.0 already
    // exist by the time this returns.
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

    #[test]
    fn clone_library_control_writes_all_related_rows() {
        let (db, path) = seeded_db("firm-t1", "user-t1", "client-t1", "eng-t1");
        let auth = session_for("firm-t1", "user-t1");

        let result = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-t1".into(),
                library_control_id: library_control_id(&db, "UAM-C-001"),
                system_id: None,
            },
        )
        .unwrap();

        assert_eq!(result.engagement_risk_ids.len(), 1, "UAM-C-001 has one related risk");
        assert_eq!(result.test_ids.len(), 1, "UAM-C-001 has one test procedure");

        db.with(|conn| {
            let (ec_code, ec_title, derived_from, ec_rel_json): (
                String,
                String,
                String,
                Option<String>,
            ) = conn.query_row(
                "SELECT code, title, derived_from, related_engagement_risk_ids_json
                 FROM EngagementControl WHERE id = ?1",
                params![result.engagement_control_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
            assert_eq!(ec_code, "UAM-C-001");
            assert!(!ec_title.is_empty());
            assert!(!derived_from.is_empty());
            let linked_risks: Vec<String> = ec_rel_json
                .as_deref()
                .map(|s| serde_json::from_str(s).unwrap_or_default())
                .unwrap_or_default();
            assert_eq!(linked_risks, result.engagement_risk_ids);

            let (risk_code, risk_derived): (String, String) = conn.query_row(
                "SELECT code, derived_from FROM EngagementRisk WHERE id = ?1",
                params![result.engagement_risk_ids[0]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(risk_code, "UAM-R-001");
            assert!(!risk_derived.is_empty());

            let (test_ctrl, test_status, test_tier): (String, String, String) = conn.query_row(
                "SELECT engagement_control_id, status, automation_tier
                 FROM Test WHERE id = ?1",
                params![result.test_ids[0]],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            assert_eq!(test_ctrl, result.engagement_control_id);
            assert_eq!(test_status, "not_started");
            assert_eq!(test_tier, "rule_based");

            let sync_control: i64 = conn.query_row(
                "SELECT COUNT(*) FROM SyncRecord
                 WHERE entity_type = 'EngagementControl' AND entity_id = ?1",
                params![result.engagement_control_id],
                |r| r.get(0),
            )?;
            assert_eq!(sync_control, 1);
            let sync_risks: i64 = conn.query_row(
                "SELECT COUNT(*) FROM SyncRecord WHERE entity_type = 'EngagementRisk'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(sync_risks, 1);
            let sync_tests: i64 = conn.query_row(
                "SELECT COUNT(*) FROM SyncRecord WHERE entity_type = 'Test'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(sync_tests, 1);

            let changes: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ChangeLog WHERE field_name = '.'",
                [],
                |r| r.get(0),
            )?;
            // 1 control + 1 risk + 1 test = 3 whole-row ChangeLog rows.
            assert_eq!(changes, 3);

            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE engagement_id = ?1 AND action = 'cloned_from_library'",
                params!["eng-t1"],
                |r| r.get(0),
            )?;
            assert_eq!(activity, 1);
            Ok(())
        })
        .unwrap();

        cleanup(&path);
    }

    #[test]
    fn clone_library_control_shares_risks_between_sibling_controls() {
        // UAM-C-001 and UAM-C-002 both reference UAM-R-001. Cloning both
        // should yield one EngagementRisk, two EngagementControls, two Tests.
        let (db, path) = seeded_db("firm-t2", "user-t2", "client-t2", "eng-t2");
        let auth = session_for("firm-t2", "user-t2");

        let first = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-t2".into(),
                library_control_id: library_control_id(&db, "UAM-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let second = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-t2".into(),
                library_control_id: library_control_id(&db, "UAM-C-002"),
                system_id: None,
            },
        )
        .unwrap();

        // Both controls land on the same EngagementRisk id.
        assert_eq!(first.engagement_risk_ids, second.engagement_risk_ids);

        db.with(|conn| {
            let risk_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM EngagementRisk WHERE engagement_id = 'eng-t2'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risk_count, 1, "shared risk must not duplicate");
            let control_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM EngagementControl WHERE engagement_id = 'eng-t2'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(control_count, 2);
            let test_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM Test WHERE engagement_id = 'eng-t2'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(test_count, 2);
            Ok(())
        })
        .unwrap();
        cleanup(&path);
    }

    #[test]
    fn clone_library_control_rejects_engagement_from_other_firm() {
        let (db, path) = seeded_db("firm-t3", "user-t3", "client-t3", "eng-t3");
        let auth = session_for("firm-other", "user-t3");

        let err = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-t3".into(),
                library_control_id: library_control_id(&db, "UAM-C-001"),
                system_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&path);
    }

    fn paths_for(dir: &std::path::Path) -> AppPaths {
        AppPaths::from_app_data_dir(dir.to_path_buf())
    }

    #[test]
    fn upload_data_import_writes_row_blob_and_audit_trail() {
        let (db, db_path) = seeded_db("firm-u1", "user-u1", "client-u1", "eng-u1");
        let auth = session_for("firm-u1", "user-u1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let csv = b"employee_id,email,status\n1,a@x.com,active\n2,b@x.com,terminated\n";
        let summary = upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-u1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "users.csv".into(),
                mime_type: Some("text/csv".into()),
                content: csv.to_vec(),
            },
        )
        .unwrap();

        assert_eq!(summary.row_count, Some(2));
        assert_eq!(summary.filename.as_deref(), Some("users.csv"));
        assert_eq!(summary.imported_by_name.as_deref(), Some("Tester"));

        db.with(|conn| {
            let (blob_id, sha_hex): (String, String) = conn.query_row(
                "SELECT blob_id, sha256_plaintext FROM DataImport WHERE id = ?1",
                params![summary.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(sha_hex.len(), 64);

            let (rel, size): (String, i64) = conn.query_row(
                "SELECT ciphertext_path, plaintext_size FROM EncryptedBlob WHERE id = ?1",
                params![blob_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(size, csv.len() as i64);
            let on_disk = std::fs::read(blob_dir.path().join(&rel)).unwrap();
            assert!(!on_disk.windows(7).any(|w| w == b"a@x.com"));

            let sync: i64 = conn.query_row(
                "SELECT COUNT(*) FROM SyncRecord WHERE entity_type = 'DataImport'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(sync, 1);
            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE engagement_id = 'eng-u1' AND action = 'uploaded'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(activity, 1);
            Ok(())
        })
        .unwrap();
        cleanup(&db_path);
    }

    #[test]
    fn list_data_imports_returns_newest_first_and_enforces_firm() {
        let (db, db_path) = seeded_db("firm-u2", "user-u2", "client-u2", "eng-u2");
        let auth = session_for("firm-u2", "user-u2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let first = upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-u2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_leavers".into(),
                filename: "leavers.csv".into(),
                mime_type: None,
                content: b"id,name\n1,Alpha\n".to_vec(),
            },
        )
        .unwrap();
        // Ensure the second row lands a second later so ORDER BY imported_at
        // DESC is deterministic. The command uses SystemTime::now() internally.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-u2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "ad.csv".into(),
                mime_type: None,
                content: b"sam,enabled\nx,1\ny,0\nz,1\n".to_vec(),
            },
        )
        .unwrap();

        let rows = list_data_imports(&db, &auth, "eng-u2".into()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, second.id);
        assert_eq!(rows[1].id, first.id);
        assert_eq!(rows[0].row_count, Some(3));

        let other = session_for("firm-other", "user-u2");
        let err = list_data_imports(&db, &other, "eng-u2".into()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&db_path);
    }

    #[test]
    fn upload_data_import_rejects_cross_firm_engagement() {
        let (db, db_path) = seeded_db("firm-u3", "user-u3", "client-u3", "eng-u3");
        let auth = session_for("firm-other", "user-u3");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let err = upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-u3".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "x.csv".into(),
                mime_type: None,
                content: b"a,b\n1,2\n".to_vec(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&db_path);
    }

    #[test]
    fn run_access_review_produces_exception_test_result_and_updates_test_status() {
        let (db, db_path) = seeded_db("firm-m1", "user-m1", "client-m1", "eng-m1");
        let auth = session_for("firm-m1", "user-m1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-m1".into(),
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
                engagement_id: "eng-m1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "ad.csv".into(),
                mime_type: Some("text/csv".into()),
                content: b"sAMAccountName,email,enabled\n\
                           alice,alice@acme.com,TRUE\n\
                           bob,bob@acme.com,TRUE\n\
                           carol,carol@acme.com,FALSE\n"
                    .to_vec(),
            },
        )
        .unwrap();
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-m1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_leavers".into(),
                filename: "leavers.csv".into(),
                mime_type: Some("text/csv".into()),
                content: b"employee_id,email\n1,alice@acme.com\n2,carol@acme.com\n".to_vec(),
            },
        )
        .unwrap();

        let result = run_access_review(
            &db,
            &auth,
            &paths,
            RunAccessReviewInput {
                test_id: test_id.clone(),
                ad_import_id: None,
                leavers_import_id: None,
            },
        )
        .unwrap();

        assert_eq!(result.outcome, "exception");
        assert_eq!(result.exception_count, 1);
        assert_eq!(result.ad_rows_skipped_disabled, 1);

        db.with(|conn| {
            let (outcome, evidence, blob_id, pop_label, detail): (
                String,
                i64,
                String,
                String,
                String,
            ) = conn.query_row(
                "SELECT outcome, evidence_count, notes_blob_id, population_ref_label, detail_json
                 FROM TestResult WHERE id = ?1",
                params![result.test_result_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
            assert_eq!(outcome, "exception");
            assert_eq!(evidence, 1);
            assert!(pop_label.contains("ad.csv"));
            assert!(detail.contains("terminated_but_active"));
            let blob_exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM EncryptedBlob WHERE id = ?1",
                params![blob_id],
                |r| r.get(0),
            )?;
            assert_eq!(blob_exists, 1);

            let test_status: String = conn.query_row(
                "SELECT status FROM Test WHERE id = ?1",
                params![test_id],
                |r| r.get(0),
            )?;
            assert_eq!(test_status, "in_review");

            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE engagement_id = 'eng-m1' AND action = 'matcher_run'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(activity, 1);
            Ok(())
        })
        .unwrap();
        cleanup(&db_path);
    }

    #[test]
    fn run_access_review_passes_when_no_leaver_is_enabled() {
        let (db, db_path) = seeded_db("firm-m2", "user-m2", "client-m2", "eng-m2");
        let auth = session_for("firm-m2", "user-m2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-m2".into(),
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
                engagement_id: "eng-m2".into(),
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
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-m2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_leavers".into(),
                filename: "leavers.csv".into(),
                mime_type: None,
                content: b"email\nzoe@a.com\n".to_vec(),
            },
        )
        .unwrap();

        let result = run_access_review(
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
        assert_eq!(result.outcome, "pass");
        assert_eq!(result.exception_count, 0);
        cleanup(&db_path);
    }

    #[test]
    fn run_access_review_errors_when_no_ad_import_uploaded() {
        let (db, db_path) = seeded_db("firm-m3", "user-m3", "client-m3", "eng-m3");
        let auth = session_for("firm-m3", "user-m3");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-m3".into(),
                library_control_id: library_control_id(&db, "UAM-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let test_id = clone.test_ids[0].clone();

        let err = run_access_review(
            &db,
            &auth,
            &paths,
            RunAccessReviewInput {
                test_id,
                ad_import_id: None,
                leavers_import_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Message(_)));
        cleanup(&db_path);
    }

    #[test]
    fn run_access_review_rejects_test_from_other_firm() {
        let (db, db_path) = seeded_db("firm-m4", "user-m4", "client-m4", "eng-m4");
        let mine = session_for("firm-m4", "user-m4");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let clone = clone_library_control(
            &db,
            &mine,
            AddLibraryControlInput {
                engagement_id: "eng-m4".into(),
                library_control_id: library_control_id(&db, "UAM-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let test_id = clone.test_ids[0].clone();

        let other = session_for("firm-other", "user-m4");
        let err = run_access_review(
            &db,
            &other,
            &paths,
            RunAccessReviewInput {
                test_id,
                ad_import_id: None,
                leavers_import_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&db_path);
    }

    #[test]
    fn list_test_results_returns_newest_first() {
        let (db, db_path) = seeded_db("firm-m5", "user-m5", "client-m5", "eng-m5");
        let auth = session_for("firm-m5", "user-m5");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-m5".into(),
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
                engagement_id: "eng-m5".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "ad.csv".into(),
                mime_type: None,
                content: b"email,enabled\nalice@a.com,TRUE\n".to_vec(),
            },
        )
        .unwrap();
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-m5".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_leavers".into(),
                filename: "leavers.csv".into(),
                mime_type: None,
                content: b"email\nalice@a.com\n".to_vec(),
            },
        )
        .unwrap();

        run_access_review(
            &db,
            &auth,
            &paths,
            RunAccessReviewInput {
                test_id: test_id.clone(),
                ad_import_id: None,
                leavers_import_id: None,
            },
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        run_access_review(
            &db,
            &auth,
            &paths,
            RunAccessReviewInput {
                test_id: test_id.clone(),
                ad_import_id: None,
                leavers_import_id: None,
            },
        )
        .unwrap();

        let results = list_test_results(&db, &auth, "eng-m5".into()).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].performed_at >= results[1].performed_at);
        assert_eq!(results[0].test_code, "UAM-T-001");
        cleanup(&db_path);
    }

    #[test]
    fn upload_data_import_rejects_oversize_payload() {
        let (db, db_path) = seeded_db("firm-u4", "user-u4", "client-u4", "eng-u4");
        let auth = session_for("firm-u4", "user-u4");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let oversize = vec![b'a'; MAX_UPLOAD_BYTES + 1];
        let err = upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-u4".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "big.csv".into(),
                mime_type: None,
                content: oversize,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Message(_)));
        cleanup(&db_path);
    }

    #[test]
    fn clone_library_control_rejects_duplicate_of_same_control() {
        // EngagementControl has UNIQUE(engagement_id, code). The second call
        // must fail — the UI is expected to hide already-cloned controls.
        let (db, path) = seeded_db("firm-t4", "user-t4", "client-t4", "eng-t4");
        let auth = session_for("firm-t4", "user-t4");

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-t4".into(),
                library_control_id: library_control_id(&db, "UAM-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let err = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-t4".into(),
                library_control_id: library_control_id(&db, "UAM-C-001"),
                system_id: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Database(_)));
        cleanup(&path);
    }
}
