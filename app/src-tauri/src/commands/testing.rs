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

use std::collections::HashMap;
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
    matcher::{
        access_review, backup, change_management, csv as csv_parser, itac_benford,
        itac_duplicates,
    },
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

/// Input for the generic matcher dispatcher. One endpoint for every rule
/// family — the rule to run is derived from the test code.
///
/// `overrides` is a free-form `purpose_tag -> DataImport.id` map. The UI
/// normally passes `None`, letting the matcher pick the newest import for
/// each required purpose tag. Advanced callers can pin a specific import —
/// useful for reproducing a prior run, or when multiple candidate imports
/// of the same purpose tag exist for an engagement.
#[derive(Debug, Deserialize)]
pub struct RunMatcherInput {
    pub test_id: String,
    #[serde(default)]
    pub overrides: Option<HashMap<String, String>>,
}

/// Result of a matcher run. Fields come in three bands:
/// 1. Always present: `test_result_id`, `rule`, `outcome`, `exception_count`,
///    `primary_import_id`, `primary_import_filename`.
/// 2. Family-specific counters (UAR / CHG / BKP), each `Option<i64>` —
///    populated only for the rule that owns them.
/// 3. Optional `supporting_import_id` / `supporting_import_filename` for
///    rules that need a second input (today: terminated-but-active).
#[derive(Debug, Serialize)]
pub struct MatcherRunResult {
    pub test_result_id: String,
    pub rule: String,
    pub outcome: String,
    pub exception_count: i64,
    pub primary_import_id: String,
    pub primary_import_filename: Option<String>,
    pub supporting_import_id: Option<String>,
    pub supporting_import_filename: Option<String>,
    // --- User access review family ---
    pub ad_rows_considered: Option<i64>,
    pub ad_rows_skipped_disabled: Option<i64>,
    pub leaver_rows_considered: Option<i64>,
    pub hr_rows_considered: Option<i64>,
    pub ad_rows_skipped_unmatchable: Option<i64>,
    pub ad_rows_skipped_no_last_logon: Option<i64>,
    pub ad_rows_skipped_unparseable: Option<i64>,
    pub dormancy_threshold_days: Option<i64>,
    // --- Change management family ---
    pub changes_considered: Option<i64>,
    pub changes_skipped_standard: Option<i64>,
    pub changes_skipped_cancelled: Option<i64>,
    pub changes_skipped_not_deployed: Option<i64>,
    pub changes_skipped_no_id: Option<i64>,
    pub changes_skipped_unparseable_dates: Option<i64>,
    // CHG SoD (dev-vs-deploy) — two permission-list inputs, not a change log.
    pub deploy_rows_considered: Option<i64>,
    pub deploy_rows_skipped_unmatchable: Option<i64>,
    pub source_rows_considered: Option<i64>,
    pub source_rows_skipped_unmatchable: Option<i64>,
    pub intersecting_users: Option<i64>,
    // --- Backup family ---
    pub jobs_considered: Option<i64>,
    pub jobs_skipped_no_id: Option<i64>,
    pub jobs_skipped_unknown_status: Option<i64>,
    // --- IT application controls family ---
    pub transactions_considered: Option<i64>,
    pub transactions_skipped_unparseable: Option<i64>,
    pub transactions_skipped_zero: Option<i64>,
    pub digit_rows_evaluated: Option<i64>,
    // Duplicate-transaction detection (ITAC-T-002) adds three more counters.
    // `rows_skipped_no_key` covers rows that had a parseable non-zero amount
    // but no counterparty or no date — they can't be grouped without a key.
    pub transactions_skipped_no_key: Option<i64>,
    pub duplicate_group_count: Option<i64>,
    pub total_duplicate_rows: Option<i64>,
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
pub fn engagement_run_matcher(
    input: RunMatcherInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
    paths: State<'_, AppPaths>,
) -> AppResult<MatcherRunResult> {
    run_matcher(db.inner(), auth.inner(), paths.inner(), input)
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

        // Auto-create a browsable Evidence row wrapping the raw upload.
        // Source = data_import; not yet bound to a test (engagement-level).
        crate::commands::evidence::persist_evidence(
            &tx,
            crate::commands::evidence::NewEvidence {
                engagement_id: &engagement_id,
                test_id: None,
                test_result_id: None,
                engagement_control_id: None,
                blob_id: &written.id,
                data_import_id: Some(&data_import_id),
                title: format!("Data import — {}", filename),
                description: None,
                source: crate::commands::evidence::EVIDENCE_SOURCE_DATA_IMPORT,
                obtained_from: None,
                obtained_at: now,
                provenance_action: "data_import",
                provenance_actor_type: "user",
                provenance_actor_id: Some(&session.user_id),
                provenance_detail_json: Some(
                    json!({
                        "filename": filename,
                        "purpose_tag": purpose_tag,
                        "source_kind": source_kind,
                        "row_count": row_count,
                        "plaintext_size": plaintext_size,
                    })
                    .to_string(),
                ),
            },
            &session.user_id,
            now,
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

pub(crate) fn run_matcher(
    db: &DbState,
    auth: &AuthState,
    paths: &AppPaths,
    input: RunMatcherInput,
) -> AppResult<MatcherRunResult> {
    let (session, master_key) = auth.require_keyed()?;
    let test_id = input.test_id.trim().to_string();
    if test_id.is_empty() {
        return Err(AppError::Message("test is required".into()));
    }

    // Normalise overrides: trim keys and values, drop entries that become
    // empty. The UI normally passes `None`; advanced callers can pin a
    // specific `DataImport.id` per purpose tag.
    let overrides: HashMap<String, String> = input
        .overrides
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            if k.is_empty() || v.is_empty() {
                None
            } else {
                Some((k, v))
            }
        })
        .collect();

    let now = now_secs();

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        let test = tx
            .query_row(
                "SELECT id, engagement_id, engagement_control_id, code, name, status
                 FROM Test WHERE id = ?1",
                params![test_id],
                |r| {
                    Ok(TestRow {
                        id: r.get(0)?,
                        engagement_id: r.get(1)?,
                        engagement_control_id: r.get(2)?,
                        code: r.get(3)?,
                        name: r.get(4)?,
                        status: r.get(5)?,
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

        let rule = MatcherRule::for_test_code(&test.code)?;

        // Per-rule dispatch. Each branch resolves its own imports, reads the
        // encrypted blobs, parses the CSVs, calls the pure matcher, and
        // produces a `RuleOutcome` the shared persistence path below consumes.
        let outcome = match rule {
            MatcherRule::UarTerminatedButActive => {
                run_uar_terminated_but_active(&tx, paths, &engagement_id, &master_key, &overrides)?
            }
            MatcherRule::UarDormantAccounts => {
                run_uar_dormant_accounts(&tx, paths, &engagement_id, &master_key, &overrides, now)?
            }
            MatcherRule::UarOrphanAccounts => {
                run_uar_orphan_accounts(&tx, paths, &engagement_id, &master_key, &overrides)?
            }
            MatcherRule::ChgApprovalBeforeDeployment => run_chg_approval_before_deployment(
                &tx,
                paths,
                &engagement_id,
                &master_key,
                &overrides,
            )?,
            MatcherRule::ChgSodDevVsDeploy => {
                run_chg_sod_dev_vs_deploy(&tx, paths, &engagement_id, &master_key, &overrides)?
            }
            MatcherRule::BkpPerformance => {
                run_bkp_performance(&tx, paths, &engagement_id, &master_key, &overrides)?
            }
            MatcherRule::ItacBenfordFirstDigit => {
                run_itac_benford_first_digit(&tx, paths, &engagement_id, &master_key, &overrides)?
            }
            MatcherRule::ItacDuplicateTransactions => run_itac_duplicate_transactions(
                &tx,
                paths,
                &engagement_id,
                &master_key,
                &overrides,
            )?,
        };

        let report_filename = format!("{}-{}.json", outcome.report_kind_slug, &test.code);
        let written_report = blobs::write_engagement_blob(
            &tx,
            &paths.app_data_dir,
            &engagement_id,
            Some("TestResult"),
            None,
            Some(&report_filename),
            Some("application/json"),
            &outcome.report_json,
            &master_key,
            now,
        )?;

        let exception_count = outcome.exception_count;
        let result_outcome = if exception_count == 0 { "pass" } else { "exception" };
        let exception_summary = outcome.exception_summary.clone();
        let detail_json = outcome.detail_json.clone();
        let primary_blob_id = outcome.primary_import.blob_id.clone().ok_or_else(|| {
            AppError::Message(format!(
                "{} has no attached file",
                outcome.primary_import_label
            ))
        })?;
        let population_label = format!(
            "{}: {}",
            outcome.primary_import_label,
            outcome.primary_import.filename.as_deref().unwrap_or("(unnamed)")
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
                result_outcome,
                exception_summary,
                exception_count,
                session.user_id,
                now,
                written_report.id,
                primary_blob_id,
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
                    "outcome": result_outcome,
                    "exception_summary": exception_summary,
                    "evidence_count": exception_count,
                    "notes_blob_id": written_report.id,
                    "population_ref": primary_blob_id,
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
                    "{} matcher on {}: {}",
                    outcome.family_label, test.code, exception_summary
                ),
            ],
        )?;

        // Evidence for the matcher output itself: linked to the Test and
        // specific TestResult so reviewers can walk from a finding back to the
        // exact run that produced it.
        crate::commands::evidence::persist_evidence(
            &tx,
            crate::commands::evidence::NewEvidence {
                engagement_id: &engagement_id,
                test_id: Some(&test.id),
                test_result_id: Some(&test_result_id),
                engagement_control_id: Some(&test.engagement_control_id),
                blob_id: &written_report.id,
                data_import_id: None,
                title: format!("Matcher report — {}", test.code),
                description: Some(exception_summary.clone()),
                source: crate::commands::evidence::EVIDENCE_SOURCE_MATCHER_REPORT,
                obtained_from: None,
                obtained_at: now,
                provenance_action: "matcher_report",
                provenance_actor_type: "system",
                provenance_actor_id: None,
                provenance_detail_json: Some(
                    json!({
                        "rule": outcome.rule,
                        "outcome": result_outcome,
                        "exception_count": exception_count,
                    })
                    .to_string(),
                ),
            },
            &session.user_id,
            now,
        )?;

        // Link the source DataImports' Evidence rows to the test so they show
        // up as supporting evidence (relevance = 'supporting'). The primary
        // evidence for this test is the matcher_report row just created.
        let primary_evidence_id: Option<String> = tx
            .query_row(
                "SELECT id FROM Evidence WHERE data_import_id = ?1",
                params![outcome.primary_import.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(eid) = primary_evidence_id {
            crate::commands::evidence::link_evidence_to_test(
                &tx, &test.id, &eid, "supporting", now,
            )?;
        }
        if let Some(si) = outcome.supporting_import.as_ref() {
            let supporting_evidence_id: Option<String> = tx
                .query_row(
                    "SELECT id FROM Evidence WHERE data_import_id = ?1",
                    params![si.id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(eid) = supporting_evidence_id {
                crate::commands::evidence::link_evidence_to_test(
                    &tx, &test.id, &eid, "supporting", now,
                )?;
            }
        }

        tx.commit()?;

        tracing::info!(
            engagement_id = %engagement_id,
            test_id = %test.id,
            test_result_id = %test_result_id,
            rule = %outcome.rule,
            outcome = %result_outcome,
            exception_count,
            "matcher ran"
        );

        let (supporting_import_id, supporting_import_filename) = outcome
            .supporting_import
            .as_ref()
            .map(|si| (Some(si.id.clone()), si.filename.clone()))
            .unwrap_or((None, None));

        Ok(MatcherRunResult {
            test_result_id,
            rule: outcome.rule.clone(),
            outcome: result_outcome.to_string(),
            exception_count,
            primary_import_id: outcome.primary_import.id,
            primary_import_filename: outcome.primary_import.filename,
            supporting_import_id,
            supporting_import_filename,
            ad_rows_considered: outcome.ad_rows_considered,
            ad_rows_skipped_disabled: outcome.ad_rows_skipped_disabled,
            leaver_rows_considered: outcome.leaver_rows_considered,
            hr_rows_considered: outcome.hr_rows_considered,
            ad_rows_skipped_unmatchable: outcome.ad_rows_skipped_unmatchable,
            ad_rows_skipped_no_last_logon: outcome.ad_rows_skipped_no_last_logon,
            ad_rows_skipped_unparseable: outcome.ad_rows_skipped_unparseable,
            dormancy_threshold_days: outcome.dormancy_threshold_days,
            changes_considered: outcome.changes_considered,
            changes_skipped_standard: outcome.changes_skipped_standard,
            changes_skipped_cancelled: outcome.changes_skipped_cancelled,
            changes_skipped_not_deployed: outcome.changes_skipped_not_deployed,
            changes_skipped_no_id: outcome.changes_skipped_no_id,
            changes_skipped_unparseable_dates: outcome.changes_skipped_unparseable_dates,
            deploy_rows_considered: outcome.deploy_rows_considered,
            deploy_rows_skipped_unmatchable: outcome.deploy_rows_skipped_unmatchable,
            source_rows_considered: outcome.source_rows_considered,
            source_rows_skipped_unmatchable: outcome.source_rows_skipped_unmatchable,
            intersecting_users: outcome.intersecting_users,
            jobs_considered: outcome.jobs_considered,
            jobs_skipped_no_id: outcome.jobs_skipped_no_id,
            jobs_skipped_unknown_status: outcome.jobs_skipped_unknown_status,
            transactions_considered: outcome.transactions_considered,
            transactions_skipped_unparseable: outcome.transactions_skipped_unparseable,
            transactions_skipped_zero: outcome.transactions_skipped_zero,
            digit_rows_evaluated: outcome.digit_rows_evaluated,
            transactions_skipped_no_key: outcome.transactions_skipped_no_key,
            duplicate_group_count: outcome.duplicate_group_count,
            total_duplicate_rows: outcome.total_duplicate_rows,
        })
    })
}

/// Which matcher rule to run for a given test code. Dispatch lives here so
/// the command layer has a single point to reject tests whose code is not
/// wired up to a matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatcherRule {
    UarTerminatedButActive,
    UarDormantAccounts,
    UarOrphanAccounts,
    ChgApprovalBeforeDeployment,
    ChgSodDevVsDeploy,
    BkpPerformance,
    ItacBenfordFirstDigit,
    ItacDuplicateTransactions,
}

impl MatcherRule {
    fn for_test_code(code: &str) -> AppResult<Self> {
        match code {
            "UAM-T-001" => Ok(Self::UarTerminatedButActive),
            "UAM-T-003" => Ok(Self::UarDormantAccounts),
            "UAM-T-004" => Ok(Self::UarOrphanAccounts),
            "CHG-T-001" => Ok(Self::ChgApprovalBeforeDeployment),
            "CHG-T-002" => Ok(Self::ChgSodDevVsDeploy),
            "BKP-T-001" => Ok(Self::BkpPerformance),
            "ITAC-T-001" => Ok(Self::ItacBenfordFirstDigit),
            "ITAC-T-002" => Ok(Self::ItacDuplicateTransactions),
            other => Err(AppError::Message(format!(
                "no matcher is wired for test code {other}"
            ))),
        }
    }
}

/// What each per-rule branch produces. Most fields are optional because they
/// are specific to one rule family; the shared persistence path reads only
/// the fields it needs for that run.
struct RuleOutcome {
    rule: String,
    /// Human-readable family label used in the ActivityLog summary. One of
    /// "User access review", "Change management", "Backup".
    family_label: String,
    /// Slug used to name the persisted report blob, e.g.
    /// `access-review-UAM-T-001.json`.
    report_kind_slug: String,
    report_json: Vec<u8>,
    exception_count: i64,
    exception_summary: String,
    detail_json: String,
    /// The `DataImport` used as the primary population for the test. Its
    /// blob id goes into `TestResult.population_ref`.
    primary_import: ImportRef,
    /// Human-readable label for the primary import, e.g. "AD export" or
    /// "Change log", used in the `TestResult.population_ref_label`.
    primary_import_label: String,
    /// A second input, if the rule needs one. Terminated-but-active uses
    /// this for the HR leavers list; the other rules leave it `None`.
    supporting_import: Option<ImportRef>,
    // --- UAR counters ---
    ad_rows_considered: Option<i64>,
    ad_rows_skipped_disabled: Option<i64>,
    leaver_rows_considered: Option<i64>,
    ad_rows_skipped_unmatchable: Option<i64>,
    ad_rows_skipped_no_last_logon: Option<i64>,
    ad_rows_skipped_unparseable: Option<i64>,
    dormancy_threshold_days: Option<i64>,
    /// Orphan-accounts rule only: size of the HR master roster used for the
    /// membership check. Separate from `leaver_rows_considered` because it
    /// carries different semantics (active employees vs known terminations).
    hr_rows_considered: Option<i64>,
    // --- CHG counters ---
    changes_considered: Option<i64>,
    changes_skipped_standard: Option<i64>,
    changes_skipped_cancelled: Option<i64>,
    changes_skipped_not_deployed: Option<i64>,
    changes_skipped_no_id: Option<i64>,
    changes_skipped_unparseable_dates: Option<i64>,
    // CHG SoD (dev-vs-deploy) counters. Separate from the change-log counters
    // above because this rule consumes two permission exports, not a change
    // register — the auditor reads this block when interpreting a CHG-T-002
    // result.
    deploy_rows_considered: Option<i64>,
    deploy_rows_skipped_unmatchable: Option<i64>,
    source_rows_considered: Option<i64>,
    source_rows_skipped_unmatchable: Option<i64>,
    /// Count of users appearing in both the deploy-permission export and the
    /// source-repo access export. Equal to the SoD exception count.
    intersecting_users: Option<i64>,
    // --- BKP counters ---
    jobs_considered: Option<i64>,
    jobs_skipped_no_id: Option<i64>,
    jobs_skipped_unknown_status: Option<i64>,
    // --- ITAC counters ---
    transactions_considered: Option<i64>,
    transactions_skipped_unparseable: Option<i64>,
    transactions_skipped_zero: Option<i64>,
    /// Count of transactions whose leading digit actually made it into the
    /// Benford distribution calculation. `transactions_considered` minus
    /// the skipped buckets. Auditors compare this against `min_digit_rows`
    /// to know whether the test was run at full strength.
    digit_rows_evaluated: Option<i64>,
    /// Duplicate-transaction rule: rows that parsed a non-zero amount but
    /// had no counterparty or no date, so couldn't be assigned a grouping
    /// key. Recorded so the auditor sees how many rows never entered the
    /// duplicate-detection step.
    transactions_skipped_no_key: Option<i64>,
    /// Duplicate-transaction rule: number of groups of 2+ rows sharing the
    /// same `(amount, counterparty, date)` key. Equal to
    /// `exception_count` for this rule.
    duplicate_group_count: Option<i64>,
    /// Duplicate-transaction rule: total rows across all flagged groups.
    /// Always `>= duplicate_group_count * 2`.
    total_duplicate_rows: Option<i64>,
}

impl RuleOutcome {
    /// All optional counters default to `None`. Each per-rule branch fills in
    /// only the fields relevant to its family.
    fn base(
        rule: String,
        family_label: &str,
        report_kind_slug: &str,
        report_json: Vec<u8>,
        exception_count: i64,
        exception_summary: String,
        detail_json: String,
        primary_import: ImportRef,
        primary_import_label: &str,
    ) -> Self {
        Self {
            rule,
            family_label: family_label.to_string(),
            report_kind_slug: report_kind_slug.to_string(),
            report_json,
            exception_count,
            exception_summary,
            detail_json,
            primary_import,
            primary_import_label: primary_import_label.to_string(),
            supporting_import: None,
            ad_rows_considered: None,
            ad_rows_skipped_disabled: None,
            leaver_rows_considered: None,
            ad_rows_skipped_unmatchable: None,
            ad_rows_skipped_no_last_logon: None,
            ad_rows_skipped_unparseable: None,
            dormancy_threshold_days: None,
            hr_rows_considered: None,
            changes_considered: None,
            changes_skipped_standard: None,
            changes_skipped_cancelled: None,
            changes_skipped_not_deployed: None,
            changes_skipped_no_id: None,
            changes_skipped_unparseable_dates: None,
            deploy_rows_considered: None,
            deploy_rows_skipped_unmatchable: None,
            source_rows_considered: None,
            source_rows_skipped_unmatchable: None,
            intersecting_users: None,
            jobs_considered: None,
            jobs_skipped_no_id: None,
            jobs_skipped_unknown_status: None,
            transactions_considered: None,
            transactions_skipped_unparseable: None,
            transactions_skipped_zero: None,
            digit_rows_evaluated: None,
            transactions_skipped_no_key: None,
            duplicate_group_count: None,
            total_duplicate_rows: None,
        }
    }
}

/// Load an encrypted CSV blob as a parsed `Table`. Wraps the decrypt + UTF-8
/// check + parser into one fallible step per import. `label` appears in the
/// error message if the bytes aren't UTF-8.
fn load_csv_table(
    tx: &rusqlite::Transaction<'_>,
    paths: &AppPaths,
    master_key: &[u8; 32],
    import: &ImportRef,
    label: &str,
) -> AppResult<csv_parser::Table> {
    let blob_id = import
        .blob_id
        .clone()
        .ok_or_else(|| AppError::Message(format!("{label} has no attached file")))?;
    let bytes = blobs::read_blob(tx, &paths.app_data_dir, &blob_id, master_key)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|e| AppError::Message(format!("{label} is not valid UTF-8: {e}")))?;
    csv_parser::parse(text)
}

/// UAR terminated-but-active: AD + HR leavers list → exceptions where a
/// terminated user still has an enabled account.
fn run_uar_terminated_but_active(
    tx: &rusqlite::Transaction<'_>,
    paths: &AppPaths,
    engagement_id: &str,
    master_key: &[u8; 32],
    overrides: &HashMap<String, String>,
) -> AppResult<RuleOutcome> {
    let ad_override = overrides
        .get("ad_export")
        .or_else(|| overrides.get("entra_export"))
        .or_else(|| overrides.get("primary"))
        .map(String::as_str);
    let ad_import = resolve_import(
        tx,
        engagement_id,
        ad_override,
        &["ad_export", "entra_export"],
        "AD or Entra export",
    )?;
    let ad_table = load_csv_table(tx, paths, master_key, &ad_import, "AD export")?;

    let leavers_override = overrides
        .get("hr_leavers")
        .or_else(|| overrides.get("supporting"))
        .map(String::as_str);
    let leavers_import = resolve_import(
        tx,
        engagement_id,
        leavers_override,
        &["hr_leavers"],
        "HR leavers list",
    )?;
    let leavers_table =
        load_csv_table(tx, paths, master_key, &leavers_import, "HR leavers list")?;

    let report = access_review::run_terminated_but_active(&ad_table, &leavers_table);
    let exception_count = report.exceptions.len() as i64;
    let summary = if exception_count == 0 {
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
    let detail = json!({
        "rule": report.rule,
        "ad_import_id": ad_import.id,
        "leavers_import_id": leavers_import.id,
        "ad_rows_considered": report.ad_rows_considered,
        "leaver_rows_considered": report.leaver_rows_considered,
        "ad_rows_skipped_disabled": report.ad_rows_skipped_disabled,
        "ad_rows_skipped_unmatchable": report.ad_rows_skipped_unmatchable,
    })
    .to_string();
    let report_json = serde_json::to_vec_pretty(&report)?;

    let mut outcome = RuleOutcome::base(
        report.rule.clone(),
        "User access review",
        "access-review",
        report_json,
        exception_count,
        summary,
        detail,
        ad_import,
        "AD export",
    );
    outcome.supporting_import = Some(leavers_import);
    outcome.ad_rows_considered = Some(report.ad_rows_considered as i64);
    outcome.ad_rows_skipped_disabled = Some(report.ad_rows_skipped_disabled as i64);
    outcome.leaver_rows_considered = Some(report.leaver_rows_considered as i64);
    outcome.ad_rows_skipped_unmatchable = Some(report.ad_rows_skipped_unmatchable as i64);
    Ok(outcome)
}

/// UAR dormant-accounts: AD only → exceptions for enabled accounts whose
/// last logon is older than the configured threshold.
fn run_uar_dormant_accounts(
    tx: &rusqlite::Transaction<'_>,
    paths: &AppPaths,
    engagement_id: &str,
    master_key: &[u8; 32],
    overrides: &HashMap<String, String>,
    now: i64,
) -> AppResult<RuleOutcome> {
    let ad_override = overrides
        .get("ad_export")
        .or_else(|| overrides.get("entra_export"))
        .or_else(|| overrides.get("primary"))
        .map(String::as_str);
    let ad_import = resolve_import(
        tx,
        engagement_id,
        ad_override,
        &["ad_export", "entra_export"],
        "AD or Entra export",
    )?;
    let ad_table = load_csv_table(tx, paths, master_key, &ad_import, "AD export")?;

    let threshold_days = access_review::DORMANT_THRESHOLD_DAYS_DEFAULT;
    let report = access_review::run_dormant_accounts(&ad_table, now, threshold_days);
    let exception_count = report.exceptions.len() as i64;
    let summary = if exception_count == 0 {
        format!(
            "No dormant accounts across {} AD rows at a {}-day threshold",
            report.ad_rows_considered, threshold_days
        )
    } else {
        format!(
            "{} dormant account{} exceeding the {}-day threshold",
            exception_count,
            if exception_count == 1 { "" } else { "s" },
            threshold_days
        )
    };
    let detail = json!({
        "rule": report.rule,
        "ad_import_id": ad_import.id,
        "ad_rows_considered": report.ad_rows_considered,
        "ad_rows_skipped_disabled": report.ad_rows_skipped_disabled,
        "ad_rows_skipped_no_last_logon": report.ad_rows_skipped_no_last_logon,
        "ad_rows_skipped_unparseable": report.ad_rows_skipped_unparseable,
        "threshold_days": report.threshold_days,
        "as_of_secs": report.as_of_secs,
    })
    .to_string();
    let report_json = serde_json::to_vec_pretty(&report)?;

    let mut outcome = RuleOutcome::base(
        report.rule.clone(),
        "User access review",
        "access-review",
        report_json,
        exception_count,
        summary,
        detail,
        ad_import,
        "AD export",
    );
    outcome.ad_rows_considered = Some(report.ad_rows_considered as i64);
    outcome.ad_rows_skipped_disabled = Some(report.ad_rows_skipped_disabled as i64);
    outcome.ad_rows_skipped_no_last_logon = Some(report.ad_rows_skipped_no_last_logon as i64);
    outcome.ad_rows_skipped_unparseable = Some(report.ad_rows_skipped_unparseable as i64);
    outcome.dormancy_threshold_days = Some(report.threshold_days as i64);
    Ok(outcome)
}

/// UAR orphan-accounts: AD export + HR master → exceptions for enabled AD
/// accounts whose email/logon do not appear in the authoritative employee
/// roster. The HR master is the *current* employee list; terminations belong
/// on the leavers list consumed by the terminated-but-active rule, not here.
fn run_uar_orphan_accounts(
    tx: &rusqlite::Transaction<'_>,
    paths: &AppPaths,
    engagement_id: &str,
    master_key: &[u8; 32],
    overrides: &HashMap<String, String>,
) -> AppResult<RuleOutcome> {
    let ad_override = overrides
        .get("ad_export")
        .or_else(|| overrides.get("entra_export"))
        .or_else(|| overrides.get("primary"))
        .map(String::as_str);
    let ad_import = resolve_import(
        tx,
        engagement_id,
        ad_override,
        &["ad_export", "entra_export"],
        "AD or Entra export",
    )?;
    let ad_table = load_csv_table(tx, paths, master_key, &ad_import, "AD export")?;

    let hr_override = overrides
        .get("hr_master")
        .or_else(|| overrides.get("hr_roster"))
        .or_else(|| overrides.get("supporting"))
        .map(String::as_str);
    let hr_import = resolve_import(
        tx,
        engagement_id,
        hr_override,
        &["hr_master", "hr_roster"],
        "HR master roster",
    )?;
    let hr_table = load_csv_table(tx, paths, master_key, &hr_import, "HR master roster")?;

    let report = access_review::run_orphan_accounts(&ad_table, &hr_table);
    let exception_count = report.exceptions.len() as i64;
    let summary = if exception_count == 0 {
        format!(
            "No orphan accounts across {} AD row{} and {} HR row{}",
            report.ad_rows_considered,
            if report.ad_rows_considered == 1 { "" } else { "s" },
            report.hr_rows_considered,
            if report.hr_rows_considered == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} orphan account{} with no HR record",
            exception_count,
            if exception_count == 1 { "" } else { "s" }
        )
    };
    let detail = json!({
        "rule": report.rule,
        "ad_import_id": ad_import.id,
        "hr_master_import_id": hr_import.id,
        "ad_rows_considered": report.ad_rows_considered,
        "ad_rows_skipped_disabled": report.ad_rows_skipped_disabled,
        "ad_rows_skipped_unmatchable": report.ad_rows_skipped_unmatchable,
        "hr_rows_considered": report.hr_rows_considered,
    })
    .to_string();
    let report_json = serde_json::to_vec_pretty(&report)?;

    let mut outcome = RuleOutcome::base(
        report.rule.clone(),
        "User access review",
        "access-review",
        report_json,
        exception_count,
        summary,
        detail,
        ad_import,
        "AD export",
    );
    outcome.supporting_import = Some(hr_import);
    outcome.ad_rows_considered = Some(report.ad_rows_considered as i64);
    outcome.ad_rows_skipped_disabled = Some(report.ad_rows_skipped_disabled as i64);
    outcome.ad_rows_skipped_unmatchable = Some(report.ad_rows_skipped_unmatchable as i64);
    outcome.hr_rows_considered = Some(report.hr_rows_considered as i64);
    Ok(outcome)
}

/// CHG change-approval-before-deployment: change log → exceptions where a
/// deployed change has missing approval, was approved after deployment, or
/// had the same person approve and deploy.
fn run_chg_approval_before_deployment(
    tx: &rusqlite::Transaction<'_>,
    paths: &AppPaths,
    engagement_id: &str,
    master_key: &[u8; 32],
    overrides: &HashMap<String, String>,
) -> AppResult<RuleOutcome> {
    let changes_override = overrides
        .get("change_log")
        .or_else(|| overrides.get("change_register"))
        .or_else(|| overrides.get("primary"))
        .map(String::as_str);
    let changes_import = resolve_import(
        tx,
        engagement_id,
        changes_override,
        &["change_log", "change_register"],
        "change log",
    )?;
    let changes_table = load_csv_table(tx, paths, master_key, &changes_import, "change log")?;

    let report = change_management::run_change_approval_before_deployment(&changes_table);
    let exception_count = report.exceptions.len() as i64;
    let summary = if exception_count == 0 {
        format!(
            "No approval exceptions across {} in-scope change{}",
            report.changes_considered,
            if report.changes_considered == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} approval exception{} across {} in-scope change{}",
            exception_count,
            if exception_count == 1 { "" } else { "s" },
            report.changes_considered,
            if report.changes_considered == 1 { "" } else { "s" }
        )
    };
    let detail = json!({
        "rule": report.rule,
        "change_log_import_id": changes_import.id,
        "changes_considered": report.changes_considered,
        "changes_skipped_standard": report.changes_skipped_standard,
        "changes_skipped_cancelled": report.changes_skipped_cancelled,
        "changes_skipped_not_deployed": report.changes_skipped_not_deployed,
        "changes_skipped_no_id": report.changes_skipped_no_id,
        "changes_skipped_unparseable_dates": report.changes_skipped_unparseable_dates,
    })
    .to_string();
    let report_json = serde_json::to_vec_pretty(&report)?;

    let mut outcome = RuleOutcome::base(
        report.rule.clone(),
        "Change management",
        "change-management",
        report_json,
        exception_count,
        summary,
        detail,
        changes_import,
        "Change log",
    );
    outcome.changes_considered = Some(report.changes_considered as i64);
    outcome.changes_skipped_standard = Some(report.changes_skipped_standard as i64);
    outcome.changes_skipped_cancelled = Some(report.changes_skipped_cancelled as i64);
    outcome.changes_skipped_not_deployed = Some(report.changes_skipped_not_deployed as i64);
    outcome.changes_skipped_no_id = Some(report.changes_skipped_no_id as i64);
    outcome.changes_skipped_unparseable_dates =
        Some(report.changes_skipped_unparseable_dates as i64);
    Ok(outcome)
}

/// CHG SoD between dev and deploy: reconcile the production-deployment
/// tool's permission list against the source repository's write-access list.
/// Any user appearing in both is a potential SoD breach — they could author
/// a change and deploy it without review unless a documented compensating
/// control is evidenced.
///
/// This is a two-input reconciliation, shaped like the UAR terminated-but-
/// active matcher (AD × leavers). Primary = deploy permissions, supporting
/// = source repo access. Purpose-tag aliases: `deploy_permissions` /
/// `deployment_access` / `primary` for deploy; `source_access` /
/// `source_repo_access` / `supporting` for source.
fn run_chg_sod_dev_vs_deploy(
    tx: &rusqlite::Transaction<'_>,
    paths: &AppPaths,
    engagement_id: &str,
    master_key: &[u8; 32],
    overrides: &HashMap<String, String>,
) -> AppResult<RuleOutcome> {
    let deploy_override = overrides
        .get("deploy_permissions")
        .or_else(|| overrides.get("deployment_access"))
        .or_else(|| overrides.get("primary"))
        .map(String::as_str);
    let deploy_import = resolve_import(
        tx,
        engagement_id,
        deploy_override,
        &["deploy_permissions", "deployment_access"],
        "deployment permission export",
    )?;
    let deploy_table = load_csv_table(
        tx,
        paths,
        master_key,
        &deploy_import,
        "deployment permission export",
    )?;

    let source_override = overrides
        .get("source_access")
        .or_else(|| overrides.get("source_repo_access"))
        .or_else(|| overrides.get("supporting"))
        .map(String::as_str);
    let source_import = resolve_import(
        tx,
        engagement_id,
        source_override,
        &["source_access", "source_repo_access"],
        "source repository access export",
    )?;
    let source_table = load_csv_table(
        tx,
        paths,
        master_key,
        &source_import,
        "source repository access export",
    )?;

    let report = change_management::run_sod_dev_vs_deploy(&deploy_table, &source_table);
    let exception_count = report.exceptions.len() as i64;
    let summary = if exception_count == 0 {
        format!(
            "No SoD overlap across {} deploy-permission row{} and {} source-access row{}",
            report.deploy_rows_considered,
            if report.deploy_rows_considered == 1 { "" } else { "s" },
            report.source_rows_considered,
            if report.source_rows_considered == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} user{} with both deploy-to-production and source-write access",
            exception_count,
            if exception_count == 1 { "" } else { "s" }
        )
    };
    let detail = json!({
        "rule": report.rule,
        "deploy_import_id": deploy_import.id,
        "source_import_id": source_import.id,
        "deploy_rows_considered": report.deploy_rows_considered,
        "deploy_rows_skipped_unmatchable": report.deploy_rows_skipped_unmatchable,
        "source_rows_considered": report.source_rows_considered,
        "source_rows_skipped_unmatchable": report.source_rows_skipped_unmatchable,
        "deploy_unique_users": report.deploy_unique_users,
        "source_unique_users": report.source_unique_users,
        "intersecting_users": report.intersecting_users,
    })
    .to_string();
    let report_json = serde_json::to_vec_pretty(&report)?;

    let mut outcome = RuleOutcome::base(
        report.rule.clone(),
        "Change management",
        "change-management-sod",
        report_json,
        exception_count,
        summary,
        detail,
        deploy_import,
        "Deployment permission export",
    );
    outcome.supporting_import = Some(source_import);
    outcome.deploy_rows_considered = Some(report.deploy_rows_considered as i64);
    outcome.deploy_rows_skipped_unmatchable = Some(report.deploy_rows_skipped_unmatchable as i64);
    outcome.source_rows_considered = Some(report.source_rows_considered as i64);
    outcome.source_rows_skipped_unmatchable = Some(report.source_rows_skipped_unmatchable as i64);
    outcome.intersecting_users = Some(report.intersecting_users as i64);
    Ok(outcome)
}

/// BKP backup-performance: backup log → exceptions for failed jobs and (if
/// a verification column is present) jobs the tool reports as unverified.
fn run_bkp_performance(
    tx: &rusqlite::Transaction<'_>,
    paths: &AppPaths,
    engagement_id: &str,
    master_key: &[u8; 32],
    overrides: &HashMap<String, String>,
) -> AppResult<RuleOutcome> {
    let jobs_override = overrides
        .get("backup_log")
        .or_else(|| overrides.get("backup_register"))
        .or_else(|| overrides.get("primary"))
        .map(String::as_str);
    let jobs_import = resolve_import(
        tx,
        engagement_id,
        jobs_override,
        &["backup_log", "backup_register"],
        "backup log",
    )?;
    let jobs_table = load_csv_table(tx, paths, master_key, &jobs_import, "backup log")?;

    let report = backup::run_backup_performance(&jobs_table);
    let exception_count = report.exceptions.len() as i64;
    let summary = if exception_count == 0 {
        format!(
            "No failed or unverified backups across {} job{}",
            report.jobs_considered,
            if report.jobs_considered == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} backup exception{} across {} job{}",
            exception_count,
            if exception_count == 1 { "" } else { "s" },
            report.jobs_considered,
            if report.jobs_considered == 1 { "" } else { "s" }
        )
    };
    let detail = json!({
        "rule": report.rule,
        "backup_log_import_id": jobs_import.id,
        "jobs_considered": report.jobs_considered,
        "jobs_skipped_no_id": report.jobs_skipped_no_id,
        "jobs_skipped_unknown_status": report.jobs_skipped_unknown_status,
    })
    .to_string();
    let report_json = serde_json::to_vec_pretty(&report)?;

    let mut outcome = RuleOutcome::base(
        report.rule.clone(),
        "Backup",
        "backup",
        report_json,
        exception_count,
        summary,
        detail,
        jobs_import,
        "Backup log",
    );
    outcome.jobs_considered = Some(report.jobs_considered as i64);
    outcome.jobs_skipped_no_id = Some(report.jobs_skipped_no_id as i64);
    outcome.jobs_skipped_unknown_status = Some(report.jobs_skipped_unknown_status as i64);
    Ok(outcome)
}

/// ITAC Benford first-digit: transaction register → exceptions for leading
/// digits that materially deviate from the expected Benford frequencies.
/// The chi-square statistic lives in the detail JSON; each flagged digit
/// becomes one auditor-facing exception with the observed-vs-expected gap.
fn run_itac_benford_first_digit(
    tx: &rusqlite::Transaction<'_>,
    paths: &AppPaths,
    engagement_id: &str,
    master_key: &[u8; 32],
    overrides: &HashMap<String, String>,
) -> AppResult<RuleOutcome> {
    let txn_override = overrides
        .get("transaction_register")
        .or_else(|| overrides.get("transactions"))
        .or_else(|| overrides.get("gl_export"))
        .or_else(|| overrides.get("primary"))
        .map(String::as_str);
    let txn_import = resolve_import(
        tx,
        engagement_id,
        txn_override,
        &["transaction_register", "transactions", "gl_export"],
        "transaction register",
    )?;
    let txn_table = load_csv_table(tx, paths, master_key, &txn_import, "transaction register")?;

    let report = itac_benford::run_benford_first_digit(&txn_table);
    let exception_count = report.exceptions.len() as i64;
    let summary = if exception_count == 0 {
        format!(
            "Population consistent with Benford across {} evaluated transaction{}",
            report.digit_rows_evaluated,
            if report.digit_rows_evaluated == 1 { "" } else { "s" }
        )
    } else if report
        .exceptions
        .iter()
        .any(|e| e.kind == "population_too_small")
    {
        format!(
            "Population too small for Benford analysis ({} digit row{}; {} required)",
            report.digit_rows_evaluated,
            if report.digit_rows_evaluated == 1 { "" } else { "s" },
            report.min_digit_rows
        )
    } else {
        format!(
            "{} digit{} deviate materially from Benford across {} evaluated transaction{}",
            exception_count,
            if exception_count == 1 { "" } else { "s" },
            report.digit_rows_evaluated,
            if report.digit_rows_evaluated == 1 { "" } else { "s" }
        )
    };
    let detail = json!({
        "rule": report.rule,
        "transaction_register_import_id": txn_import.id,
        "rows_considered": report.rows_considered,
        "rows_skipped_unparseable": report.rows_skipped_unparseable,
        "rows_skipped_zero": report.rows_skipped_zero,
        "digit_rows_evaluated": report.digit_rows_evaluated,
        "chi_square": report.chi_square,
        "chi_square_critical": report.chi_square_critical,
        "min_digit_rows": report.min_digit_rows,
        "digit_deviation_threshold": report.digit_deviation_threshold,
    })
    .to_string();
    let report_json = serde_json::to_vec_pretty(&report)?;

    let mut outcome = RuleOutcome::base(
        report.rule.clone(),
        "IT application controls",
        "itac-benford",
        report_json,
        exception_count,
        summary,
        detail,
        txn_import,
        "Transaction register",
    );
    outcome.transactions_considered = Some(report.rows_considered as i64);
    outcome.transactions_skipped_unparseable = Some(report.rows_skipped_unparseable as i64);
    outcome.transactions_skipped_zero = Some(report.rows_skipped_zero as i64);
    outcome.digit_rows_evaluated = Some(report.digit_rows_evaluated as i64);
    Ok(outcome)
}

/// ITAC duplicate-transaction detection: group the population by
/// (amount, counterparty, date) and report each group of two or more rows.
/// Genuine business activity rarely produces exact triples, so flagged
/// groups are investigated as potential double-postings, duplicated
/// invoices, or fabricated records. Reuses the Benford rule's
/// `transaction_register` purpose tag — both consume the same export.
fn run_itac_duplicate_transactions(
    tx: &rusqlite::Transaction<'_>,
    paths: &AppPaths,
    engagement_id: &str,
    master_key: &[u8; 32],
    overrides: &HashMap<String, String>,
) -> AppResult<RuleOutcome> {
    let txn_override = overrides
        .get("transaction_register")
        .or_else(|| overrides.get("transactions"))
        .or_else(|| overrides.get("gl_export"))
        .or_else(|| overrides.get("primary"))
        .map(String::as_str);
    let txn_import = resolve_import(
        tx,
        engagement_id,
        txn_override,
        &["transaction_register", "transactions", "gl_export"],
        "transaction register",
    )?;
    let txn_table = load_csv_table(tx, paths, master_key, &txn_import, "transaction register")?;

    let report = itac_duplicates::run_duplicate_transactions(&txn_table);
    let exception_count = report.exceptions.len() as i64;
    let summary = if exception_count == 0 {
        format!(
            "No duplicate transactions found across {} considered row{}",
            report.rows_considered,
            if report.rows_considered == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} duplicate group{} covering {} row{} across {} considered row{}",
            report.duplicate_group_count,
            if report.duplicate_group_count == 1 { "" } else { "s" },
            report.total_duplicate_rows,
            if report.total_duplicate_rows == 1 { "" } else { "s" },
            report.rows_considered,
            if report.rows_considered == 1 { "" } else { "s" }
        )
    };
    let detail = json!({
        "rule": report.rule,
        "transaction_register_import_id": txn_import.id,
        "rows_considered": report.rows_considered,
        "rows_skipped_unparseable": report.rows_skipped_unparseable,
        "rows_skipped_zero": report.rows_skipped_zero,
        "rows_skipped_no_key": report.rows_skipped_no_key,
        "duplicate_group_count": report.duplicate_group_count,
        "total_duplicate_rows": report.total_duplicate_rows,
    })
    .to_string();
    let report_json = serde_json::to_vec_pretty(&report)?;

    let mut outcome = RuleOutcome::base(
        report.rule.clone(),
        "IT application controls",
        "itac-duplicates",
        report_json,
        exception_count,
        summary,
        detail,
        txn_import,
        "Transaction register",
    );
    outcome.transactions_considered = Some(report.rows_considered as i64);
    outcome.transactions_skipped_unparseable = Some(report.rows_skipped_unparseable as i64);
    outcome.transactions_skipped_zero = Some(report.rows_skipped_zero as i64);
    outcome.transactions_skipped_no_key = Some(report.rows_skipped_no_key as i64);
    outcome.duplicate_group_count = Some(report.duplicate_group_count as i64);
    outcome.total_duplicate_rows = Some(report.total_duplicate_rows as i64);
    Ok(outcome)
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
    engagement_control_id: String,
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
        // should collapse onto one EngagementRisk, two EngagementControls,
        // and one Test per test procedure beneath the cloned controls. The
        // current library (v0.3.0) ships UAM-T-001 under UAM-C-001 and
        // UAM-T-002 + UAM-T-003 + UAM-T-004 under UAM-C-002, so we expect
        // 4 Tests.
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
            assert_eq!(test_count, 4);
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
    fn run_matcher_uar_produces_exception_test_result_and_updates_test_status() {
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

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        assert_eq!(result.outcome, "exception");
        assert_eq!(result.exception_count, 1);
        assert_eq!(result.ad_rows_skipped_disabled, Some(1));

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
    fn run_matcher_uar_passes_when_no_leaver_is_enabled() {
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

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id,
                overrides: None,
            },
        )
        .unwrap();
        assert_eq!(result.outcome, "pass");
        assert_eq!(result.exception_count, 0);
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_uar_errors_when_no_ad_import_uploaded() {
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

        let err = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id,
                overrides: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Message(_)));
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_rejects_test_from_other_firm() {
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
        let err = run_matcher(
            &db,
            &other,
            &paths,
            RunMatcherInput {
                test_id,
                overrides: None,
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

        run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: test_id.clone(),
                overrides: None,
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

    #[test]
    fn run_matcher_uar_dormant_flags_stale_accounts_without_leavers_input() {
        // Clone UAM-C-002 to get the dormant-accounts test (UAM-T-003). Upload
        // only an AD export — no HR leavers list. The matcher should flag
        // alice (last logon 2020-01-01) and never-signed-in carol; bob (recent)
        // is clean.
        let (db, db_path) = seeded_db("firm-dm1", "user-dm1", "client-dm1", "eng-dm1");
        let auth = session_for("firm-dm1", "user-dm1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-dm1".into(),
                library_control_id: library_control_id(&db, "UAM-C-002"),
                system_id: None,
            },
        )
        .unwrap();

        let dormant_test_id: String = db
            .with(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-dm1' AND code = 'UAM-T-003'",
                )?;
                let id: String = stmt.query_row([], |r| r.get(0))?;
                Ok(id)
            })
            .unwrap();
        assert!(clone.test_ids.contains(&dormant_test_id));

        // AD export with one recent sign-in, one old sign-in, one never.
        let recent = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - (10 * 86_400);
        let recent_ft = (recent + 11_644_473_600) * 10_000_000;
        let ad_csv = format!(
            "sAMAccountName,email,enabled,lastLogonTimestamp\n\
             alice,alice@acme.com,TRUE,2020-01-01\n\
             bob,bob@acme.com,TRUE,{recent_ft}\n\
             carol,carol@acme.com,TRUE,0\n"
        );
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-dm1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "ad.csv".into(),
                mime_type: Some("text/csv".into()),
                content: ad_csv.into_bytes(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: dormant_test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        assert_eq!(result.outcome, "exception");
        assert_eq!(result.exception_count, 2);
        assert_eq!(result.rule, "dormant_accounts");
        assert_eq!(
            result.dormancy_threshold_days,
            Some(access_review::DORMANT_THRESHOLD_DAYS_DEFAULT as i64)
        );
        assert!(result.supporting_import_id.is_none());
        assert!(result.leaver_rows_considered.is_none());

        db.with(|conn| {
            let detail: String = conn.query_row(
                "SELECT detail_json FROM TestResult WHERE id = ?1",
                params![result.test_result_id],
                |r| r.get(0),
            )?;
            assert!(detail.contains("dormant_accounts"));
            assert!(detail.contains("threshold_days"));
            let test_status: String = conn.query_row(
                "SELECT status FROM Test WHERE id = ?1",
                params![dormant_test_id],
                |r| r.get(0),
            )?;
            assert_eq!(test_status, "in_review");
            Ok(())
        })
        .unwrap();
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_rejects_test_code_that_has_no_rule() {
        // UAM-T-002 (the periodic-recertification test) has no matcher wired
        // up; the command must reject it cleanly rather than silently falling
        // through to a default rule. UAM-T-002 sits under UAM-C-002 alongside
        // UAM-T-003 (dormant accounts), so cloning the parent control gives
        // us both tests.
        let (db, db_path) = seeded_db("firm-dm2", "user-dm2", "client-dm2", "eng-dm2");
        let auth = session_for("firm-dm2", "user-dm2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-dm2".into(),
                library_control_id: library_control_id(&db, "UAM-C-002"),
                system_id: None,
            },
        )
        .unwrap();

        let unwired_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-dm2' AND code = 'UAM-T-002'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        let err = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: unwired_test_id,
                overrides: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Message(_)));
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_chg_flags_approval_exceptions_for_deployed_changes() {
        // CHG-C-001 ships CHG-T-001 as a rule-based test. Seed a change log
        // with three production deployments: one missing approval, one
        // approved after deployment, one with approver == implementer. The
        // matcher should emit three exceptions and mark the test in_review.
        let (db, db_path) = seeded_db("firm-chg1", "user-chg1", "client-chg1", "eng-chg1");
        let auth = session_for("firm-chg1", "user-chg1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-chg1".into(),
                library_control_id: library_control_id(&db, "CHG-C-001"),
                system_id: None,
            },
        )
        .unwrap();

        let chg_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-chg1' AND code = 'CHG-T-001'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(clone.test_ids.contains(&chg_test_id));

        let csv = b"change_id,change_type,status,approver,approved_at,implementer,deployed_at\n\
                    CHG-1001,Normal,Deployed,,,alice,2025-03-01T10:00:00Z\n\
                    CHG-1002,Normal,Deployed,bob,2025-03-03T10:00:00Z,carol,2025-03-02T10:00:00Z\n\
                    CHG-1003,Normal,Deployed,dan,2025-03-01T08:00:00Z,dan,2025-03-01T10:00:00Z\n";
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-chg1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "change_log".into(),
                filename: "changes.csv".into(),
                mime_type: Some("text/csv".into()),
                content: csv.to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: chg_test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        assert_eq!(result.rule, "change_approval_before_deployment");
        assert_eq!(result.outcome, "exception");
        assert_eq!(result.exception_count, 3);
        assert_eq!(result.changes_considered, Some(3));
        assert_eq!(result.changes_skipped_standard, Some(0));
        assert!(result.supporting_import_id.is_none());
        assert_eq!(result.primary_import_filename.as_deref(), Some("changes.csv"));

        db.with(|conn| {
            let (outcome, evidence, pop_label, detail): (String, i64, String, String) = conn
                .query_row(
                    "SELECT outcome, evidence_count, population_ref_label, detail_json
                     FROM TestResult WHERE id = ?1",
                    params![result.test_result_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?;
            assert_eq!(outcome, "exception");
            assert_eq!(evidence, 3);
            assert!(pop_label.starts_with("Change log:"));
            assert!(detail.contains("change_approval_before_deployment"));

            let test_status: String = conn.query_row(
                "SELECT status FROM Test WHERE id = ?1",
                params![chg_test_id],
                |r| r.get(0),
            )?;
            assert_eq!(test_status, "in_review");

            let activity_summary: String = conn.query_row(
                "SELECT summary FROM ActivityLog
                 WHERE engagement_id = 'eng-chg1' AND action = 'matcher_run'",
                [],
                |r| r.get(0),
            )?;
            assert!(activity_summary.starts_with("Change management matcher on CHG-T-001"));
            Ok(())
        })
        .unwrap();
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_chg_passes_when_all_deployed_changes_have_clean_approvals() {
        let (db, db_path) = seeded_db("firm-chg2", "user-chg2", "client-chg2", "eng-chg2");
        let auth = session_for("firm-chg2", "user-chg2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-chg2".into(),
                library_control_id: library_control_id(&db, "CHG-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let chg_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-chg2' AND code = 'CHG-T-001'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        // Two clean deployments + one standard (pre-approved) change that the
        // rule must count and skip rather than flag.
        let csv = b"change_id,change_type,status,approver,approved_at,implementer,deployed_at\n\
                    CHG-2001,Normal,Deployed,bob,2025-03-01T08:00:00Z,alice,2025-03-01T10:00:00Z\n\
                    CHG-2002,Normal,Deployed,bob,2025-03-02T08:00:00Z,alice,2025-03-02T10:00:00Z\n\
                    CHG-2003,Standard,Deployed,,,alice,2025-03-03T10:00:00Z\n";
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-chg2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "change_log".into(),
                filename: "changes.csv".into(),
                mime_type: None,
                content: csv.to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: chg_test_id,
                overrides: None,
            },
        )
        .unwrap();
        assert_eq!(result.outcome, "pass");
        assert_eq!(result.exception_count, 0);
        // changes_considered counts every row in the CSV. In-scope rows are
        // what's left after skipped_standard / skipped_cancelled /
        // skipped_not_deployed / skipped_no_id / skipped_unparseable_dates
        // are subtracted.
        assert_eq!(result.changes_considered, Some(3));
        assert_eq!(result.changes_skipped_standard, Some(1));
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_bkp_flags_failed_jobs() {
        let (db, db_path) = seeded_db("firm-bkp1", "user-bkp1", "client-bkp1", "eng-bkp1");
        let auth = session_for("firm-bkp1", "user-bkp1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-bkp1".into(),
                library_control_id: library_control_id(&db, "BKP-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let bkp_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-bkp1' AND code = 'BKP-T-001'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(clone.test_ids.contains(&bkp_test_id));

        // Four jobs: one success, one failed, one cancelled, one with an
        // unknown status string that should be skipped rather than flagged.
        let csv = b"job_id,target,status,started_at,completed_at\n\
                    JOB-1,db-prod,Success,2025-03-01T02:00:00Z,2025-03-01T02:45:00Z\n\
                    JOB-2,db-prod,Failed,2025-03-02T02:00:00Z,2025-03-02T02:15:00Z\n\
                    JOB-3,db-prod,Cancelled,2025-03-03T02:00:00Z,\n\
                    JOB-4,db-prod,Queued,,\n";
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-bkp1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "backup_log".into(),
                filename: "backups.csv".into(),
                mime_type: Some("text/csv".into()),
                content: csv.to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: bkp_test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        assert_eq!(result.rule, "backup_performance");
        assert_eq!(result.outcome, "exception");
        assert_eq!(result.exception_count, 2);
        assert_eq!(result.jobs_considered, Some(4));
        assert_eq!(result.jobs_skipped_unknown_status, Some(1));
        assert_eq!(result.jobs_skipped_no_id, Some(0));
        assert_eq!(result.primary_import_filename.as_deref(), Some("backups.csv"));

        db.with(|conn| {
            let (outcome, pop_label, detail): (String, String, String) = conn.query_row(
                "SELECT outcome, population_ref_label, detail_json
                 FROM TestResult WHERE id = ?1",
                params![result.test_result_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            assert_eq!(outcome, "exception");
            assert!(pop_label.starts_with("Backup log:"));
            assert!(detail.contains("backup_performance"));

            let test_status: String = conn.query_row(
                "SELECT status FROM Test WHERE id = ?1",
                params![bkp_test_id],
                |r| r.get(0),
            )?;
            assert_eq!(test_status, "in_review");

            let activity_summary: String = conn.query_row(
                "SELECT summary FROM ActivityLog
                 WHERE engagement_id = 'eng-bkp1' AND action = 'matcher_run'",
                [],
                |r| r.get(0),
            )?;
            assert!(activity_summary.starts_with("Backup matcher on BKP-T-001"));
            Ok(())
        })
        .unwrap();
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_bkp_passes_when_all_jobs_succeed() {
        let (db, db_path) = seeded_db("firm-bkp2", "user-bkp2", "client-bkp2", "eng-bkp2");
        let auth = session_for("firm-bkp2", "user-bkp2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-bkp2".into(),
                library_control_id: library_control_id(&db, "BKP-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let bkp_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-bkp2' AND code = 'BKP-T-001'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        let csv = b"job_id,target,status,started_at,completed_at\n\
                    JOB-A,db-prod,Success,2025-03-01T02:00:00Z,2025-03-01T02:45:00Z\n\
                    JOB-B,file-srv,Completed,2025-03-02T02:00:00Z,2025-03-02T02:30:00Z\n";
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-bkp2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "backup_log".into(),
                filename: "backups.csv".into(),
                mime_type: None,
                content: csv.to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: bkp_test_id,
                overrides: None,
            },
        )
        .unwrap();
        assert_eq!(result.outcome, "pass");
        assert_eq!(result.exception_count, 0);
        assert_eq!(result.jobs_considered, Some(2));
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_orphan_accounts_flags_ad_rows_with_no_hr_match() {
        // UAM-C-002 ships UAM-T-004 as the orphan-accounts procedure.
        // Seed an AD export with three enabled users and one disabled user.
        // The HR master only contains alice and bob — carol is the orphan,
        // dan is disabled so should be skipped.
        let (db, db_path) = seeded_db("firm-orph1", "user-orph1", "client-orph1", "eng-orph1");
        let auth = session_for("firm-orph1", "user-orph1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-orph1".into(),
                library_control_id: library_control_id(&db, "UAM-C-002"),
                system_id: None,
            },
        )
        .unwrap();
        let orphan_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-orph1' AND code = 'UAM-T-004'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-orph1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "ad.csv".into(),
                mime_type: Some("text/csv".into()),
                content: b"sAMAccountName,email,enabled\n\
                           alice,alice@acme.com,TRUE\n\
                           bob,bob@acme.com,TRUE\n\
                           carol,carol@acme.com,TRUE\n\
                           dan,dan@acme.com,FALSE\n"
                    .to_vec(),
            },
        )
        .unwrap();
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-orph1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_master".into(),
                filename: "hr.csv".into(),
                mime_type: Some("text/csv".into()),
                content: b"employee_id,email,sAMAccountName\n\
                           1,alice@acme.com,alice\n\
                           2,bob@acme.com,bob\n"
                    .to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: orphan_test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        assert_eq!(result.rule, "orphan_accounts");
        assert_eq!(result.outcome, "exception");
        assert_eq!(result.exception_count, 1);
        // `ad_rows_considered` is the full AD row count (incl. disabled);
        // `ad_rows_skipped_disabled` is the disabled subset. Dan is disabled
        // so is not evaluated, but still counted in the total.
        assert_eq!(result.ad_rows_considered, Some(4));
        assert_eq!(result.ad_rows_skipped_disabled, Some(1));
        assert_eq!(result.hr_rows_considered, Some(2));
        // Orphan-accounts does not use the leavers counter — that belongs
        // to the terminated-but-active rule. Keep the families distinct.
        assert!(result.leaver_rows_considered.is_none());
        assert!(result.supporting_import_id.is_some());
        assert!(result
            .supporting_import_filename
            .as_deref()
            .map(|n| n == "hr.csv")
            .unwrap_or(false));

        db.with(|conn| {
            let (outcome, detail): (String, String) = conn.query_row(
                "SELECT outcome, detail_json FROM TestResult WHERE id = ?1",
                params![result.test_result_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(outcome, "exception");
            assert!(detail.contains("orphan_accounts"));
            assert!(detail.contains("hr_rows_considered"));

            let test_status: String = conn.query_row(
                "SELECT status FROM Test WHERE id = ?1",
                params![orphan_test_id],
                |r| r.get(0),
            )?;
            assert_eq!(test_status, "in_review");

            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE engagement_id = 'eng-orph1' AND action = 'matcher_run'",
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
    fn run_matcher_orphan_accounts_passes_when_all_ad_rows_have_hr_match() {
        // All enabled AD accounts appear in the HR master — matcher passes.
        let (db, db_path) = seeded_db("firm-orph2", "user-orph2", "client-orph2", "eng-orph2");
        let auth = session_for("firm-orph2", "user-orph2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-orph2".into(),
                library_control_id: library_control_id(&db, "UAM-C-002"),
                system_id: None,
            },
        )
        .unwrap();
        let orphan_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-orph2' AND code = 'UAM-T-004'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-orph2".into(),
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
                engagement_id: "eng-orph2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_master".into(),
                filename: "hr.csv".into(),
                mime_type: None,
                content: b"email\nalice@a.com\nbob@a.com\ncarol@a.com\n".to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: orphan_test_id,
                overrides: None,
            },
        )
        .unwrap();
        assert_eq!(result.rule, "orphan_accounts");
        assert_eq!(result.outcome, "pass");
        assert_eq!(result.exception_count, 0);
        assert_eq!(result.ad_rows_considered, Some(2));
        assert_eq!(result.hr_rows_considered, Some(3));
        cleanup(&db_path);
    }

    // -------- ITAC Benford first-digit tests --------

    /// Build a CSV population whose leading-digit distribution is
    /// approximately uniform across 1..9. At n=450 this is large enough to
    /// clear the 300-row minimum while being catastrophically non-Benford:
    /// the chi-square blows past the critical value and at least one digit
    /// trips the 2pp deviation threshold.
    fn uniform_transaction_csv() -> String {
        let mut s = String::from("txn_id,amount\n");
        let exemplars: [&str; 9] = [
            "123.45", "234.50", "345.00", "456.00", "567.00", "678.00", "789.00", "890.00",
            "987.00",
        ];
        let per_digit = 50usize;
        let mut idx = 1;
        for exemplar in &exemplars {
            for _ in 0..per_digit {
                s.push_str(&format!("{},{}\n", idx, exemplar));
                idx += 1;
            }
        }
        s
    }

    /// Build a CSV population whose leading-digit distribution is
    /// approximately Benford. At n≈1000 this sits comfortably under every
    /// deviation threshold and is the happy-path fixture.
    fn benford_transaction_csv() -> String {
        let targets: [usize; 9] = [301, 176, 125, 97, 79, 67, 58, 51, 46];
        let mut s = String::from("txn_id,amount\n");
        let exemplars: [&str; 9] = [
            "123.45", "2345.67", "34.50", "4500.00", "5.75", "64.20", "7123.99", "850.00",
            "9.99",
        ];
        let mut idx = 1;
        for (digit_i, count) in targets.iter().enumerate() {
            for _ in 0..*count {
                s.push_str(&format!("{},{}\n", idx, exemplars[digit_i]));
                idx += 1;
            }
        }
        s
    }

    #[test]
    fn run_matcher_itac_benford_flags_uniform_digit_distribution() {
        // ITAC-C-001 ships ITAC-T-001 as the Benford first-digit procedure.
        // Seed a uniform-digit population; the matcher should flag multiple
        // digits as deviating from Benford and elevate the test to in_review.
        let (db, db_path) = seeded_db("firm-bf1", "user-bf1", "client-bf1", "eng-bf1");
        let auth = session_for("firm-bf1", "user-bf1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-bf1".into(),
                library_control_id: library_control_id(&db, "ITAC-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let benford_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-bf1' AND code = 'ITAC-T-001'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-bf1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "transaction_register".into(),
                filename: "txns.csv".into(),
                mime_type: Some("text/csv".into()),
                content: uniform_transaction_csv().into_bytes(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: benford_test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        assert_eq!(result.rule, "benford_first_digit");
        assert_eq!(result.outcome, "exception");
        assert!(result.exception_count >= 1);
        assert_eq!(result.transactions_considered, Some(450));
        assert_eq!(result.transactions_skipped_unparseable, Some(0));
        assert_eq!(result.transactions_skipped_zero, Some(0));
        assert_eq!(result.digit_rows_evaluated, Some(450));
        // ITAC is a population-level test, no supporting import.
        assert!(result.supporting_import_id.is_none());
        assert_eq!(
            result.primary_import_filename.as_deref(),
            Some("txns.csv")
        );

        db.with(|conn| {
            let (outcome, pop_label, detail): (String, String, String) = conn.query_row(
                "SELECT outcome, population_ref_label, detail_json
                 FROM TestResult WHERE id = ?1",
                params![result.test_result_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            assert_eq!(outcome, "exception");
            assert!(pop_label.contains("txns.csv"));
            assert!(detail.contains("benford_first_digit"));
            assert!(detail.contains("chi_square"));
            assert!(detail.contains("digit_rows_evaluated"));

            let test_status: String = conn.query_row(
                "SELECT status FROM Test WHERE id = ?1",
                params![benford_test_id],
                |r| r.get(0),
            )?;
            assert_eq!(test_status, "in_review");

            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE engagement_id = 'eng-bf1' AND action = 'matcher_run'",
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
    fn run_matcher_itac_benford_passes_on_benford_like_population() {
        // Population is approximately Benford — matcher passes, detail_json
        // records a chi-square below the 8-df critical value.
        let (db, db_path) = seeded_db("firm-bf2", "user-bf2", "client-bf2", "eng-bf2");
        let auth = session_for("firm-bf2", "user-bf2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-bf2".into(),
                library_control_id: library_control_id(&db, "ITAC-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let benford_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-bf2' AND code = 'ITAC-T-001'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-bf2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "transaction_register".into(),
                filename: "txns.csv".into(),
                mime_type: None,
                content: benford_transaction_csv().into_bytes(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: benford_test_id,
                overrides: None,
            },
        )
        .unwrap();
        assert_eq!(result.rule, "benford_first_digit");
        assert_eq!(result.outcome, "pass");
        assert_eq!(result.exception_count, 0);
        assert!(result.digit_rows_evaluated.unwrap() >= 300);
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_itac_benford_flags_small_population() {
        // Below the 300-row minimum, the matcher emits a single
        // population_too_small exception and does no digit-level analysis.
        let (db, db_path) = seeded_db("firm-bf3", "user-bf3", "client-bf3", "eng-bf3");
        let auth = session_for("firm-bf3", "user-bf3");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-bf3".into(),
                library_control_id: library_control_id(&db, "ITAC-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let benford_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-bf3' AND code = 'ITAC-T-001'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        // 50-row population — well below the 300 minimum.
        let mut csv = String::from("txn_id,amount\n");
        for i in 1..=50 {
            csv.push_str(&format!("{},{}\n", i, 100 + i));
        }

        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-bf3".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "transaction_register".into(),
                filename: "small.csv".into(),
                mime_type: None,
                content: csv.into_bytes(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: benford_test_id,
                overrides: None,
            },
        )
        .unwrap();
        assert_eq!(result.rule, "benford_first_digit");
        assert_eq!(result.outcome, "exception");
        assert_eq!(result.exception_count, 1);
        assert_eq!(result.digit_rows_evaluated, Some(50));

        db.with(|conn| {
            let detail: String = conn.query_row(
                "SELECT detail_json FROM TestResult WHERE id = ?1",
                params![result.test_result_id],
                |r| r.get(0),
            )?;
            // chi_square is null when the population is too small.
            assert!(detail.contains("\"chi_square\":null"));
            Ok(())
        })
        .unwrap();
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_chg_sod_flags_users_on_both_deploy_and_source() {
        // CHG-C-002 ships CHG-T-002 as the dev-vs-deploy SoD procedure.
        // Deploy permissions: alice, bob, carol. Source access: alice,
        // dave, eve. Alice is on both — one exception.
        let (db, db_path) =
            seeded_db("firm-sod1", "user-sod1", "client-sod1", "eng-sod1");
        let auth = session_for("firm-sod1", "user-sod1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-sod1".into(),
                library_control_id: library_control_id(&db, "CHG-C-002"),
                system_id: None,
            },
        )
        .unwrap();
        let sod_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-sod1' AND code = 'CHG-T-002'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-sod1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "deploy_permissions".into(),
                filename: "deploy.csv".into(),
                mime_type: Some("text/csv".into()),
                content: b"username,role\n\
                           alice,release-manager\n\
                           bob,release-manager\n\
                           carol,release-manager\n"
                    .to_vec(),
            },
        )
        .unwrap();
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-sod1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "source_access".into(),
                filename: "source.csv".into(),
                mime_type: Some("text/csv".into()),
                content: b"username,permission\n\
                           alice,write\n\
                           dave,write\n\
                           eve,write\n"
                    .to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: sod_test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        assert_eq!(result.rule, "sod_dev_vs_deploy");
        assert_eq!(result.outcome, "exception");
        assert_eq!(result.exception_count, 1);
        assert_eq!(result.deploy_rows_considered, Some(3));
        assert_eq!(result.source_rows_considered, Some(3));
        assert_eq!(result.intersecting_users, Some(1));
        // SoD uses the supporting-import slot for the source-access export.
        assert!(result.supporting_import_id.is_some());
        assert!(result
            .supporting_import_filename
            .as_deref()
            .map(|n| n == "source.csv")
            .unwrap_or(false));
        // CHG approval-rule counters should not be set — those belong to
        // CHG-T-001's rule, not this one. Keep the per-rule blocks distinct.
        assert!(result.changes_considered.is_none());

        db.with(|conn| {
            let (outcome, detail): (String, String) = conn.query_row(
                "SELECT outcome, detail_json FROM TestResult WHERE id = ?1",
                params![result.test_result_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(outcome, "exception");
            assert!(detail.contains("sod_dev_vs_deploy"));
            assert!(detail.contains("intersecting_users"));

            let test_status: String = conn.query_row(
                "SELECT status FROM Test WHERE id = ?1",
                params![sod_test_id],
                |r| r.get(0),
            )?;
            assert_eq!(test_status, "in_review");

            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE engagement_id = 'eng-sod1' AND action = 'matcher_run'",
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
    fn run_matcher_chg_sod_passes_on_disjoint_permission_lists() {
        // Deploy permissions and source access are fully disjoint — no
        // user holds both, so the SoD rule passes with zero exceptions.
        let (db, db_path) =
            seeded_db("firm-sod2", "user-sod2", "client-sod2", "eng-sod2");
        let auth = session_for("firm-sod2", "user-sod2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-sod2".into(),
                library_control_id: library_control_id(&db, "CHG-C-002"),
                system_id: None,
            },
        )
        .unwrap();
        let sod_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-sod2' AND code = 'CHG-T-002'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-sod2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "deploy_permissions".into(),
                filename: "deploy.csv".into(),
                mime_type: Some("text/csv".into()),
                content: b"username,role\n\
                           alice,release-manager\n\
                           bob,release-manager\n"
                    .to_vec(),
            },
        )
        .unwrap();
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-sod2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "source_access".into(),
                filename: "source.csv".into(),
                mime_type: Some("text/csv".into()),
                content: b"username,permission\n\
                           carol,write\n\
                           dave,write\n"
                    .to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: sod_test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        assert_eq!(result.rule, "sod_dev_vs_deploy");
        assert_eq!(result.outcome, "pass");
        assert_eq!(result.exception_count, 0);
        assert_eq!(result.deploy_rows_considered, Some(2));
        assert_eq!(result.source_rows_considered, Some(2));
        assert_eq!(result.intersecting_users, Some(0));
        cleanup(&db_path);
    }

    #[test]
    fn run_matcher_itac_duplicates_flags_exact_repeats() {
        // ITAC-C-001 ships ITAC-T-002 as the duplicate-transaction procedure.
        // Seed a population with one exact-triple repeat (Acme, 100.00,
        // 2024-05-01, posted twice). The matcher should flag a single
        // duplicate group covering two rows and elevate the test to
        // in_review.
        let (db, db_path) =
            seeded_db("firm-dup1", "user-dup1", "client-dup1", "eng-dup1");
        let auth = session_for("firm-dup1", "user-dup1");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-dup1".into(),
                library_control_id: library_control_id(&db, "ITAC-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let dup_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-dup1' AND code = 'ITAC-T-002'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        // Three distinct rows + one exact duplicate of the first row.
        let csv = "txn_id,amount,counterparty,date\n\
                   1,100.00,Acme Ltd,2024-05-01\n\
                   2,250.00,Beta Corp,2024-05-02\n\
                   3,42.50,Gamma Inc,2024-05-03\n\
                   4,100.00,Acme Ltd,2024-05-01\n";
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-dup1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "transaction_register".into(),
                filename: "txns.csv".into(),
                mime_type: Some("text/csv".into()),
                content: csv.as_bytes().to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: dup_test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        assert_eq!(result.rule, "duplicate_transactions");
        assert_eq!(result.outcome, "exception");
        assert_eq!(result.exception_count, 1);
        assert_eq!(result.transactions_considered, Some(4));
        assert_eq!(result.transactions_skipped_unparseable, Some(0));
        assert_eq!(result.transactions_skipped_zero, Some(0));
        assert_eq!(result.transactions_skipped_no_key, Some(0));
        assert_eq!(result.duplicate_group_count, Some(1));
        assert_eq!(result.total_duplicate_rows, Some(2));
        // ITAC is a population-level test, no supporting import.
        assert!(result.supporting_import_id.is_none());
        assert_eq!(
            result.primary_import_filename.as_deref(),
            Some("txns.csv")
        );
        // Benford-only counters should remain unset on this rule.
        assert!(result.digit_rows_evaluated.is_none());

        db.with(|conn| {
            let (outcome, pop_label, detail): (String, String, String) = conn
                .query_row(
                    "SELECT outcome, population_ref_label, detail_json
                     FROM TestResult WHERE id = ?1",
                    params![result.test_result_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
            assert_eq!(outcome, "exception");
            assert!(pop_label.contains("txns.csv"));
            assert!(detail.contains("duplicate_transactions"));
            assert!(detail.contains("duplicate_group_count"));

            let test_status: String = conn.query_row(
                "SELECT status FROM Test WHERE id = ?1",
                params![dup_test_id],
                |r| r.get(0),
            )?;
            assert_eq!(test_status, "in_review");

            let activity: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ActivityLog
                 WHERE engagement_id = 'eng-dup1' AND action = 'matcher_run'",
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
    fn run_matcher_itac_duplicates_passes_on_unique_population() {
        // Every (amount, counterparty, date) triple is unique — the matcher
        // passes with zero exceptions.
        let (db, db_path) =
            seeded_db("firm-dup2", "user-dup2", "client-dup2", "eng-dup2");
        let auth = session_for("firm-dup2", "user-dup2");
        let blob_dir = tempfile::tempdir().unwrap();
        let paths = paths_for(blob_dir.path());

        clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-dup2".into(),
                library_control_id: library_control_id(&db, "ITAC-C-001"),
                system_id: None,
            },
        )
        .unwrap();
        let dup_test_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM Test WHERE engagement_id = 'eng-dup2' AND code = 'ITAC-T-002'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        // Same vendor, same amount, *different* dates → no key collision.
        let csv = "txn_id,amount,counterparty,date\n\
                   1,100.00,Acme Ltd,2024-05-01\n\
                   2,100.00,Acme Ltd,2024-05-08\n\
                   3,100.00,Acme Ltd,2024-05-15\n";
        upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-dup2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "transaction_register".into(),
                filename: "txns.csv".into(),
                mime_type: None,
                content: csv.as_bytes().to_vec(),
            },
        )
        .unwrap();

        let result = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: dup_test_id,
                overrides: None,
            },
        )
        .unwrap();
        assert_eq!(result.rule, "duplicate_transactions");
        assert_eq!(result.outcome, "pass");
        assert_eq!(result.exception_count, 0);
        assert_eq!(result.transactions_considered, Some(3));
        assert_eq!(result.duplicate_group_count, Some(0));
        assert_eq!(result.total_duplicate_rows, Some(0));
        cleanup(&db_path);
    }
}
