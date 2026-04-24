//! Evidence commands (Module 7).
//!
//! An `Evidence` row is the browsable face of an `EncryptedBlob`. The blob
//! alone tells an auditor nothing; `Evidence` annotates it with a title, a
//! source, a chain of custody, and (optionally) the test or finding it
//! supports.
//!
//! Auto-created evidence:
//! - `upload_data_import` creates an Evidence row with `source =
//!   data_import` so the raw file is browsable from day one.
//! - `run_matcher` creates an Evidence row with `source =
//!   matcher_report` linked to the Test and TestResult, and adds
//!   `TestEvidenceLink` rows pointing at the DataImports that fed the run.
//!
//! Manual flows exposed here:
//! - `engagement_upload_evidence` — free-form attachment (screenshots,
//!   attestations, emails).
//! - `engagement_attach_evidence_to_finding` / `..._detach_...` —
//!   used by the finding editor to cite evidence.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;
use uuid::Uuid;

use crate::{
    auth::AuthState,
    blobs,
    db::DbState,
    error::{AppError, AppResult},
    paths::AppPaths,
};

pub const EVIDENCE_SOURCE_AUDITOR_UPLOAD: &str = "auditor_upload";
pub const EVIDENCE_SOURCE_DATA_IMPORT: &str = "data_import";
pub const EVIDENCE_SOURCE_MATCHER_REPORT: &str = "matcher_report";

const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct UploadEvidenceInput {
    pub engagement_id: String,
    pub title: String,
    pub description: Option<String>,
    pub obtained_from: Option<String>,
    /// Auditor-supplied obtained timestamp (epoch seconds). Defaults to now.
    pub obtained_at: Option<i64>,
    pub test_id: Option<String>,
    pub finding_id: Option<String>,
    pub filename: String,
    pub mime_type: Option<String>,
    pub content: Vec<u8>,
}

#[derive(Debug, Deserialize)]
pub struct EvidenceLinkInput {
    pub finding_id: String,
    pub evidence_id: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct EvidenceSummary {
    pub id: String,
    pub engagement_id: String,
    pub title: String,
    pub description: Option<String>,
    pub source: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub plaintext_size: Option<i64>,
    pub test_id: Option<String>,
    pub test_code: Option<String>,
    pub test_result_id: Option<String>,
    pub data_import_id: Option<String>,
    pub obtained_at: i64,
    pub obtained_from: Option<String>,
    pub created_at: i64,
    pub created_by_name: Option<String>,
    pub linked_test_ids: Vec<String>,
    pub linked_finding_ids: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct EvidencePayload {
    pub id: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub content: Vec<u8>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Data needed to insert a new Evidence row with an origin provenance entry.
/// Borrowed from the caller so we don't force allocations on the hot path.
pub(crate) struct NewEvidence<'a> {
    pub engagement_id: &'a str,
    pub test_id: Option<&'a str>,
    pub test_result_id: Option<&'a str>,
    pub engagement_control_id: Option<&'a str>,
    pub blob_id: &'a str,
    pub data_import_id: Option<&'a str>,
    pub title: String,
    pub description: Option<String>,
    pub source: &'a str,
    pub obtained_from: Option<String>,
    pub obtained_at: i64,
    pub provenance_action: &'a str,
    pub provenance_actor_type: &'a str,
    pub provenance_actor_id: Option<&'a str>,
    pub provenance_detail_json: Option<String>,
}

/// Inserts Evidence + origin EvidenceProvenance + SyncRecord + whole-row
/// ChangeLog. Caller owns the transaction. Returns the new evidence id.
pub(crate) fn persist_evidence(
    tx: &Transaction<'_>,
    new: NewEvidence<'_>,
    actor_user_id: &str,
    now: i64,
) -> AppResult<String> {
    let evidence_id = Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO Evidence (
            id, engagement_id, test_id, test_result_id, engagement_control_id,
            blob_id, data_import_id, title, description, source,
            obtained_at, obtained_from, created_by, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            evidence_id,
            new.engagement_id,
            new.test_id,
            new.test_result_id,
            new.engagement_control_id,
            new.blob_id,
            new.data_import_id,
            new.title,
            new.description,
            new.source,
            new.obtained_at,
            new.obtained_from,
            actor_user_id,
            now,
        ],
    )?;

    tx.execute(
        "INSERT INTO EvidenceProvenance (
            id, evidence_id, chain_ordinal, action,
            actor_type, actor_id, occurred_at, detail_json
         ) VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7)",
        params![
            Uuid::now_v7().to_string(),
            evidence_id,
            new.provenance_action,
            new.provenance_actor_type,
            new.provenance_actor_id,
            now,
            new.provenance_detail_json,
        ],
    )?;

    let sync_id = Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO SyncRecord (
            id, entity_type, entity_id, last_modified_at,
            last_modified_by, version, deleted, sync_state
         ) VALUES (?1, 'Evidence', ?2, ?3, ?4, 1, 0, 'local_only')",
        params![sync_id, evidence_id, now, actor_user_id],
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
            actor_user_id,
            json!({
                "engagement_id": new.engagement_id,
                "test_id": new.test_id,
                "test_result_id": new.test_result_id,
                "source": new.source,
                "blob_id": new.blob_id,
                "data_import_id": new.data_import_id,
                "title": new.title,
            })
            .to_string(),
        ],
    )?;

    Ok(evidence_id)
}

/// Insert a TestEvidenceLink row if one does not already exist. Returns true
/// when a new link was written.
pub(crate) fn link_evidence_to_test(
    tx: &Transaction<'_>,
    test_id: &str,
    evidence_id: &str,
    relevance: &str,
    now: i64,
) -> AppResult<bool> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM TestEvidenceLink
             WHERE test_id = ?1 AND evidence_id = ?2",
            params![test_id, evidence_id],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO TestEvidenceLink (id, test_id, evidence_id, relevance, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            Uuid::now_v7().to_string(),
            test_id,
            evidence_id,
            relevance,
            now,
        ],
    )?;
    Ok(true)
}

// -- Tauri commands ---------------------------------------------------------

#[tauri::command]
pub fn engagement_list_evidence(
    engagement_id: String,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Vec<EvidenceSummary>> {
    list_engagement_evidence(db.inner(), auth.inner(), engagement_id)
}

#[tauri::command]
pub fn engagement_upload_evidence(
    input: UploadEvidenceInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
    paths: State<'_, AppPaths>,
) -> AppResult<EvidenceSummary> {
    upload_evidence(db.inner(), auth.inner(), paths.inner(), input)
}

#[tauri::command]
pub fn engagement_download_evidence(
    evidence_id: String,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
    paths: State<'_, AppPaths>,
) -> AppResult<EvidencePayload> {
    download_evidence(db.inner(), auth.inner(), paths.inner(), evidence_id)
}

#[tauri::command]
pub fn finding_attach_evidence(
    input: EvidenceLinkInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<EvidenceSummary> {
    attach_evidence_to_finding(db.inner(), auth.inner(), input)
}

#[tauri::command]
pub fn finding_detach_evidence(
    input: EvidenceLinkInput,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<()> {
    detach_evidence_from_finding(db.inner(), auth.inner(), input)
}

#[tauri::command]
pub fn finding_list_evidence(
    finding_id: String,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<Vec<EvidenceSummary>> {
    list_finding_evidence(db.inner(), auth.inner(), finding_id)
}

// -- Implementations --------------------------------------------------------

pub(crate) fn list_engagement_evidence(
    db: &DbState,
    auth: &AuthState,
    engagement_id: String,
) -> AppResult<Vec<EvidenceSummary>> {
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

        load_summaries(
            conn,
            "WHERE ev.engagement_id = ?1 ORDER BY ev.created_at DESC",
            params![engagement_id],
        )
    })
}

pub(crate) fn list_finding_evidence(
    db: &DbState,
    auth: &AuthState,
    finding_id: String,
) -> AppResult<Vec<EvidenceSummary>> {
    let session = auth.require()?;
    let finding_id = finding_id.trim().to_string();
    if finding_id.is_empty() {
        return Err(AppError::Message("finding is required".into()));
    }

    db.with(|conn| {
        let firm: Option<String> = conn
            .query_row(
                "SELECT c.firm_id
                 FROM Finding f
                 JOIN Engagement e ON e.id = f.engagement_id
                 JOIN Client c ON c.id = e.client_id
                 WHERE f.id = ?1",
                params![finding_id],
                |r| r.get(0),
            )
            .optional()?;
        match firm {
            Some(f) if f == session.firm_id => {}
            _ => return Err(AppError::NotFound("finding not found".into())),
        }

        load_summaries(
            conn,
            "INNER JOIN FindingEvidenceLink fel ON fel.evidence_id = ev.id
             WHERE fel.finding_id = ?1
             ORDER BY fel.created_at DESC",
            params![finding_id],
        )
    })
}

pub(crate) fn upload_evidence(
    db: &DbState,
    auth: &AuthState,
    paths: &AppPaths,
    input: UploadEvidenceInput,
) -> AppResult<EvidenceSummary> {
    let (session, master_key) = auth.require_keyed()?;
    let engagement_id = input.engagement_id.trim().to_string();
    let title = input.title.trim().to_string();
    let filename = input.filename.trim().to_string();
    let description = input
        .description
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let obtained_from = input
        .obtained_from
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let test_id = input
        .test_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let finding_id = input
        .finding_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let mime_type = input
        .mime_type
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if engagement_id.is_empty() {
        return Err(AppError::Message("engagement is required".into()));
    }
    if title.is_empty() {
        return Err(AppError::Message("title is required".into()));
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

    let now = now_secs();
    let obtained_at = input.obtained_at.unwrap_or(now);

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

        let mut engagement_control_id: Option<String> = None;
        if let Some(tid) = &test_id {
            let row: Option<(String, String)> = tx
                .query_row(
                    "SELECT t.engagement_id, t.engagement_control_id
                     FROM Test t WHERE t.id = ?1",
                    params![tid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match row {
                Some((e, ec)) if e == engagement_id => {
                    engagement_control_id = Some(ec);
                }
                _ => return Err(AppError::NotFound("test not found".into())),
            }
        }

        if let Some(fid) = &finding_id {
            let fin_eng: Option<String> = tx
                .query_row(
                    "SELECT engagement_id FROM Finding WHERE id = ?1",
                    params![fid],
                    |r| r.get(0),
                )
                .optional()?;
            match fin_eng {
                Some(e) if e == engagement_id => {}
                _ => return Err(AppError::NotFound("finding not found".into())),
            }
        }

        let written = blobs::write_engagement_blob(
            &tx,
            &paths.app_data_dir,
            &engagement_id,
            Some("Evidence"),
            None,
            Some(&filename),
            mime_type.as_deref(),
            &input.content,
            &master_key,
            now,
        )?;

        let evidence_id = persist_evidence(
            &tx,
            NewEvidence {
                engagement_id: &engagement_id,
                test_id: test_id.as_deref(),
                test_result_id: None,
                engagement_control_id: engagement_control_id.as_deref(),
                blob_id: &written.id,
                data_import_id: None,
                title: title.clone(),
                description: description.clone(),
                source: EVIDENCE_SOURCE_AUDITOR_UPLOAD,
                obtained_from: obtained_from.clone(),
                obtained_at,
                provenance_action: "uploaded",
                provenance_actor_type: "user",
                provenance_actor_id: Some(&session.user_id),
                provenance_detail_json: Some(
                    json!({
                        "filename": filename,
                        "mime_type": mime_type,
                        "plaintext_size": written.plaintext_size,
                    })
                    .to_string(),
                ),
            },
            &session.user_id,
            now,
        )?;

        if let Some(fid) = &finding_id {
            link_evidence_to_finding_in_tx(&tx, fid, &evidence_id, now)?;
        }

        tx.execute(
            "INSERT INTO ActivityLog (
                id, engagement_id, entity_type, entity_id,
                action, performed_by, performed_at, summary
             ) VALUES (?1, ?2, 'Evidence', ?3, 'uploaded', ?4, ?5, ?6)",
            params![
                Uuid::now_v7().to_string(),
                engagement_id,
                evidence_id,
                session.user_id,
                now,
                format!("Uploaded evidence '{}' ({})", title, filename),
            ],
        )?;

        tx.commit()?;

        tracing::info!(
            engagement_id = %engagement_id,
            evidence_id = %evidence_id,
            title = %title,
            "evidence uploaded"
        );

        Ok(evidence_id)
    })
    .and_then(|id| load_summary(db, &id))
}

pub(crate) fn download_evidence(
    db: &DbState,
    auth: &AuthState,
    paths: &AppPaths,
    evidence_id: String,
) -> AppResult<EvidencePayload> {
    let (session, master_key) = auth.require_keyed()?;
    let evidence_id = evidence_id.trim().to_string();
    if evidence_id.is_empty() {
        return Err(AppError::Message("evidence is required".into()));
    }

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        let row: Option<(String, String, Option<String>, Option<String>, String)> = tx
            .query_row(
                "SELECT ev.blob_id, c.firm_id, eb.filename, eb.mime_type, ev.engagement_id
                 FROM Evidence ev
                 JOIN Engagement e ON e.id = ev.engagement_id
                 JOIN Client c ON c.id = e.client_id
                 JOIN EncryptedBlob eb ON eb.id = ev.blob_id
                 WHERE ev.id = ?1",
                params![evidence_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;

        let (blob_id, firm_id, filename, mime_type, _engagement_id) = row
            .ok_or_else(|| AppError::NotFound("evidence not found".into()))?;
        if firm_id != session.firm_id {
            return Err(AppError::NotFound("evidence not found".into()));
        }

        let content =
            blobs::read_blob(&tx, &paths.app_data_dir, &blob_id, &master_key)?;
        tx.commit()?;

        Ok(EvidencePayload {
            id: evidence_id,
            filename,
            mime_type,
            content,
        })
    })
}

pub(crate) fn attach_evidence_to_finding(
    db: &DbState,
    auth: &AuthState,
    input: EvidenceLinkInput,
) -> AppResult<EvidenceSummary> {
    let session = auth.require()?;
    let finding_id = input.finding_id.trim().to_string();
    let evidence_id = input.evidence_id.trim().to_string();
    if finding_id.is_empty() || evidence_id.is_empty() {
        return Err(AppError::Message(
            "finding and evidence are required".into(),
        ));
    }
    let now = now_secs();

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        let row: Option<(String, String, String, String, String)> = tx
            .query_row(
                "SELECT f.engagement_id, f.code, ev.engagement_id, ev.title, c.firm_id
                 FROM Finding f
                 JOIN Evidence ev ON ev.id = ?2
                 JOIN Engagement e ON e.id = f.engagement_id
                 JOIN Client c ON c.id = e.client_id
                 WHERE f.id = ?1",
                params![finding_id, evidence_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        let (finding_engagement, finding_code, evidence_engagement, evidence_title, firm_id) =
            row.ok_or_else(|| AppError::NotFound("finding or evidence not found".into()))?;
        if firm_id != session.firm_id {
            return Err(AppError::NotFound("finding not found".into()));
        }
        if finding_engagement != evidence_engagement {
            return Err(AppError::Message(
                "evidence belongs to a different engagement".into(),
            ));
        }

        let inserted = link_evidence_to_finding_in_tx(&tx, &finding_id, &evidence_id, now)?;
        if inserted {
            tx.execute(
                "INSERT INTO ActivityLog (
                    id, engagement_id, entity_type, entity_id,
                    action, performed_by, performed_at, summary
                 ) VALUES (?1, ?2, 'Finding', ?3, 'evidence_attached', ?4, ?5, ?6)",
                params![
                    Uuid::now_v7().to_string(),
                    finding_engagement,
                    finding_id,
                    session.user_id,
                    now,
                    format!("Attached evidence '{}' to {}", evidence_title, finding_code),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    })?;

    load_summary(db, &evidence_id)
}

pub(crate) fn detach_evidence_from_finding(
    db: &DbState,
    auth: &AuthState,
    input: EvidenceLinkInput,
) -> AppResult<()> {
    let session = auth.require()?;
    let finding_id = input.finding_id.trim().to_string();
    let evidence_id = input.evidence_id.trim().to_string();
    if finding_id.is_empty() || evidence_id.is_empty() {
        return Err(AppError::Message(
            "finding and evidence are required".into(),
        ));
    }
    let now = now_secs();

    db.with_mut(|conn| {
        let tx = conn.transaction()?;

        let row: Option<(String, String, String)> = tx
            .query_row(
                "SELECT f.engagement_id, f.code, c.firm_id
                 FROM Finding f
                 JOIN Engagement e ON e.id = f.engagement_id
                 JOIN Client c ON c.id = e.client_id
                 WHERE f.id = ?1",
                params![finding_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let (engagement_id, finding_code, firm_id) =
            row.ok_or_else(|| AppError::NotFound("finding not found".into()))?;
        if firm_id != session.firm_id {
            return Err(AppError::NotFound("finding not found".into()));
        }

        let deleted = tx.execute(
            "DELETE FROM FindingEvidenceLink
             WHERE finding_id = ?1 AND evidence_id = ?2",
            params![finding_id, evidence_id],
        )?;
        if deleted > 0 {
            tx.execute(
                "INSERT INTO ActivityLog (
                    id, engagement_id, entity_type, entity_id,
                    action, performed_by, performed_at, summary
                 ) VALUES (?1, ?2, 'Finding', ?3, 'evidence_detached', ?4, ?5, ?6)",
                params![
                    Uuid::now_v7().to_string(),
                    engagement_id,
                    finding_id,
                    session.user_id,
                    now,
                    format!("Detached evidence from {}", finding_code),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
}

fn link_evidence_to_finding_in_tx(
    tx: &Transaction<'_>,
    finding_id: &str,
    evidence_id: &str,
    now: i64,
) -> AppResult<bool> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM FindingEvidenceLink
             WHERE finding_id = ?1 AND evidence_id = ?2",
            params![finding_id, evidence_id],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO FindingEvidenceLink (id, finding_id, evidence_id, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            Uuid::now_v7().to_string(),
            finding_id,
            evidence_id,
            now,
        ],
    )?;
    Ok(true)
}

fn load_summary(db: &DbState, evidence_id: &str) -> AppResult<EvidenceSummary> {
    db.with(|conn| {
        let rows = load_summaries(
            conn,
            "WHERE ev.id = ?1",
            params![evidence_id],
        )?;
        rows.into_iter()
            .next()
            .ok_or_else(|| AppError::NotFound("evidence not found".into()))
    })
}

fn load_summaries(
    conn: &rusqlite::Connection,
    where_and_order_clause: &str,
    params: impl rusqlite::Params,
) -> AppResult<Vec<EvidenceSummary>> {
    let sql = format!(
        "SELECT ev.id, ev.engagement_id, ev.title, ev.description, ev.source,
                eb.filename, eb.mime_type, eb.plaintext_size,
                ev.test_id, t.code,
                ev.test_result_id, ev.data_import_id,
                ev.obtained_at, ev.obtained_from,
                ev.created_at, u.display_name
         FROM Evidence ev
         JOIN EncryptedBlob eb ON eb.id = ev.blob_id
         LEFT JOIN Test t ON t.id = ev.test_id
         LEFT JOIN User u ON u.id = ev.created_by
         {where_and_order_clause}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<EvidenceSummary> = stmt
        .query_map(params, |r| {
            Ok(EvidenceSummary {
                id: r.get(0)?,
                engagement_id: r.get(1)?,
                title: r.get(2)?,
                description: r.get(3)?,
                source: r.get(4)?,
                filename: r.get(5)?,
                mime_type: r.get(6)?,
                plaintext_size: r.get(7)?,
                test_id: r.get(8)?,
                test_code: r.get(9)?,
                test_result_id: r.get(10)?,
                data_import_id: r.get(11)?,
                obtained_at: r.get(12)?,
                obtained_from: r.get(13)?,
                created_at: r.get(14)?,
                created_by_name: r.get(15)?,
                linked_test_ids: Vec::new(),
                linked_finding_ids: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Second pass: attach link arrays per evidence row.
    let mut enriched = Vec::with_capacity(rows.len());
    let mut test_stmt = conn
        .prepare("SELECT test_id FROM TestEvidenceLink WHERE evidence_id = ?1")?;
    let mut finding_stmt = conn
        .prepare("SELECT finding_id FROM FindingEvidenceLink WHERE evidence_id = ?1")?;
    for mut row in rows {
        let ts: Vec<String> = test_stmt
            .query_map(params![row.id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let fs: Vec<String> = finding_stmt
            .query_map(params![row.id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        row.linked_test_ids = ts;
        row.linked_finding_ids = fs;
        enriched.push(row);
    }
    Ok(enriched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Session;
    use crate::commands::findings::{elevate_finding, ElevateFindingInput};
    use crate::commands::testing::{
        clone_library_control, run_matcher, upload_data_import,
        AddLibraryControlInput, RunMatcherInput, UploadDataImportInput,
    };
    use crate::paths::AppPaths;

    fn tmp_path(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.join(format!("audit-evidence-test-{stamp}-{suffix}.db"))
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
        let key = [3u8; 32];
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

    #[test]
    fn upload_data_import_auto_creates_evidence() {
        let (db, path) = seeded_db("firm-e1", "user-e1", "client-e1", "eng-e1");
        let auth = session_for("firm-e1", "user-e1");
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());

        let imp = upload_data_import(
            &db,
            &auth,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-e1".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "ad.csv".into(),
                mime_type: None,
                content: b"email\nalice@a.com\n".to_vec(),
            },
        )
        .unwrap();

        let rows = list_engagement_evidence(&db, &auth, "eng-e1".into()).unwrap();
        assert_eq!(rows.len(), 1);
        let ev = &rows[0];
        assert_eq!(ev.source, EVIDENCE_SOURCE_DATA_IMPORT);
        assert_eq!(ev.data_import_id.as_deref(), Some(imp.id.as_str()));
        assert_eq!(ev.filename.as_deref(), Some("ad.csv"));
        assert!(ev.test_id.is_none());
        cleanup(&path);
    }

    #[test]
    fn matcher_run_creates_matcher_report_and_links_data_imports() {
        let (db, path) = seeded_db("firm-e2", "user-e2", "client-e2", "eng-e2");
        let auth = session_for("firm-e2", "user-e2");
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-e2".into(),
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
                engagement_id: "eng-e2".into(),
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
                engagement_id: "eng-e2".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_leavers".into(),
                filename: "leavers.csv".into(),
                mime_type: None,
                content: b"email\nalice@a.com\n".to_vec(),
            },
        )
        .unwrap();

        let run = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();

        let all = list_engagement_evidence(&db, &auth, "eng-e2".into()).unwrap();
        // 2 data_import evidence + 1 matcher_report evidence.
        assert_eq!(all.len(), 3);
        let report = all
            .iter()
            .find(|e| e.source == EVIDENCE_SOURCE_MATCHER_REPORT)
            .expect("matcher_report evidence row");
        assert_eq!(report.test_id.as_deref(), Some(test_id.as_str()));
        assert_eq!(report.test_result_id.as_deref(), Some(run.test_result_id.as_str()));

        // Both data imports were linked to the test by the matcher run.
        let linked_to_test: Vec<_> = all
            .iter()
            .filter(|e| e.linked_test_ids.contains(&test_id))
            .collect();
        assert_eq!(linked_to_test.len(), 2);
        cleanup(&path);
    }

    #[test]
    fn attach_evidence_to_finding_and_detach_roundtrip() {
        let (db, path) = seeded_db("firm-e3", "user-e3", "client-e3", "eng-e3");
        let auth = session_for("firm-e3", "user-e3");
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-e3".into(),
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
                engagement_id: "eng-e3".into(),
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
                engagement_id: "eng-e3".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_leavers".into(),
                filename: "leavers.csv".into(),
                mime_type: None,
                content: b"email\nalice@a.com\n".to_vec(),
            },
        )
        .unwrap();
        let run = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id,
                overrides: None,
            },
        )
        .unwrap();
        let finding = elevate_finding(
            &db,
            &auth,
            ElevateFindingInput {
                test_result_id: run.test_result_id.clone(),
                title: None,
                severity_id: None,
            },
        )
        .unwrap();

        let all = list_engagement_evidence(&db, &auth, "eng-e3".into()).unwrap();
        let ad_ev = all
            .iter()
            .find(|e| e.filename.as_deref() == Some("ad.csv"))
            .unwrap();

        let summary = attach_evidence_to_finding(
            &db,
            &auth,
            EvidenceLinkInput {
                finding_id: finding.id.clone(),
                evidence_id: ad_ev.id.clone(),
            },
        )
        .unwrap();
        assert!(summary.linked_finding_ids.contains(&finding.id));

        let attached =
            list_finding_evidence(&db, &auth, finding.id.clone()).unwrap();
        assert_eq!(attached.len(), 1);
        assert_eq!(attached[0].id, ad_ev.id);

        // Attaching twice is idempotent.
        attach_evidence_to_finding(
            &db,
            &auth,
            EvidenceLinkInput {
                finding_id: finding.id.clone(),
                evidence_id: ad_ev.id.clone(),
            },
        )
        .unwrap();
        let still = list_finding_evidence(&db, &auth, finding.id.clone()).unwrap();
        assert_eq!(still.len(), 1);

        detach_evidence_from_finding(
            &db,
            &auth,
            EvidenceLinkInput {
                finding_id: finding.id.clone(),
                evidence_id: ad_ev.id.clone(),
            },
        )
        .unwrap();
        let after = list_finding_evidence(&db, &auth, finding.id.clone()).unwrap();
        assert_eq!(after.len(), 0);
        cleanup(&path);
    }

    #[test]
    fn upload_evidence_persists_blob_and_links_optional_finding() {
        let (db, path) = seeded_db("firm-e4", "user-e4", "client-e4", "eng-e4");
        let auth = session_for("firm-e4", "user-e4");
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());

        let clone = clone_library_control(
            &db,
            &auth,
            AddLibraryControlInput {
                engagement_id: "eng-e4".into(),
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
                engagement_id: "eng-e4".into(),
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
                engagement_id: "eng-e4".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "hr_leavers".into(),
                filename: "leavers.csv".into(),
                mime_type: None,
                content: b"email\nalice@a.com\n".to_vec(),
            },
        )
        .unwrap();
        let run = run_matcher(
            &db,
            &auth,
            &paths,
            RunMatcherInput {
                test_id: test_id.clone(),
                overrides: None,
            },
        )
        .unwrap();
        let finding = elevate_finding(
            &db,
            &auth,
            ElevateFindingInput {
                test_result_id: run.test_result_id,
                title: None,
                severity_id: None,
            },
        )
        .unwrap();

        let uploaded = upload_evidence(
            &db,
            &auth,
            &paths,
            UploadEvidenceInput {
                engagement_id: "eng-e4".into(),
                title: "Email from IT manager confirming disablement".into(),
                description: None,
                obtained_from: Some("simba@client.com".into()),
                obtained_at: None,
                test_id: Some(test_id),
                finding_id: Some(finding.id.clone()),
                filename: "email.eml".into(),
                mime_type: Some("message/rfc822".into()),
                content: b"From: simba@client.com\nSubject: access revoked\n\n".to_vec(),
            },
        )
        .unwrap();

        assert_eq!(uploaded.source, EVIDENCE_SOURCE_AUDITOR_UPLOAD);
        assert!(uploaded.linked_finding_ids.contains(&finding.id));

        let payload = download_evidence(&db, &auth, &paths, uploaded.id.clone()).unwrap();
        assert!(String::from_utf8_lossy(&payload.content).contains("access revoked"));
        cleanup(&path);
    }

    #[test]
    fn list_engagement_evidence_rejects_cross_firm() {
        let (db, path) = seeded_db("firm-e5", "user-e5", "client-e5", "eng-e5");
        let owner = session_for("firm-e5", "user-e5");
        let other = session_for("firm-other", "user-e5");
        let tmp = tempfile::tempdir().unwrap();
        let paths = paths_for(tmp.path());

        upload_data_import(
            &db,
            &owner,
            &paths,
            UploadDataImportInput {
                engagement_id: "eng-e5".into(),
                system_id: None,
                source_kind: "csv".into(),
                purpose_tag: "ad_export".into(),
                filename: "ad.csv".into(),
                mime_type: None,
                content: b"email\nbob@a.com\n".to_vec(),
            },
        )
        .unwrap();

        let err =
            list_engagement_evidence(&db, &other, "eng-e5".into()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        cleanup(&path);
    }
}
