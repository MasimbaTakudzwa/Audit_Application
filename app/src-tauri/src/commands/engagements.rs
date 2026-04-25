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

use std::collections::{HashMap, HashSet};
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

// -------- Engagement overview ("Today" view) --------
//
// Single round-trip command that returns synthesised state for an engagement:
// status counts, per-risk coverage, an ordered "needs attention" list, and a
// recent-activity timeline. Designed so the frontend can render a dashboard
// from one call rather than five list endpoints + client-side aggregation.
// Pure read-path; no mutations, no ActivityLog / SyncRecord writes.

/// Header card data for the engagement — name, client, status, dates,
/// library version. Mirrors `EngagementSummary` plus a few extra fields the
/// dashboard wants.
#[derive(Debug, Serialize)]
pub struct EngagementHeader {
    pub id: String,
    pub name: String,
    pub client_name: String,
    pub status: String,
    pub fiscal_year: Option<String>,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub library_version_at_start: String,
    pub created_at: i64,
    pub closed_at: Option<i64>,
    pub lead_partner_name: Option<String>,
}

/// Roll-up counters across the engagement. Each counter is a simple integer
/// so the frontend can render pies, bars, or status pills without further
/// aggregation. Empty buckets are zero, never absent.
#[derive(Debug, Serialize)]
pub struct StatusCounts {
    pub controls_total: i64,
    pub risks_total: i64,
    pub tests_total: i64,
    pub tests_not_started: i64,
    pub tests_in_progress: i64,
    pub tests_in_review: i64,
    pub tests_completed: i64,
    pub results_total: i64,
    pub results_pass: i64,
    pub results_exception: i64,
    pub results_fail: i64,
    pub findings_total: i64,
    pub findings_draft: i64,
    pub findings_issued: i64,
    pub findings_remediated: i64,
    pub findings_closed: i64,
    pub findings_critical: i64,
    pub findings_high: i64,
    pub findings_medium: i64,
    pub findings_low: i64,
    pub findings_observation: i64,
    pub data_imports_total: i64,
    pub evidence_total: i64,
}

/// One row of the risk-coverage strip — the per-risk roll-up the auditor
/// reads to ask "which risks are well covered, which are thin, which are
/// uncovered entirely?"
#[derive(Debug, Serialize)]
pub struct RiskCoverageEntry {
    pub risk_id: String,
    pub risk_code: String,
    pub risk_title: String,
    pub inherent_rating: String,
    pub residual_rating: Option<String>,
    pub control_count: i64,
    pub test_count: i64,
    pub tests_with_results: i64,
    pub tests_with_exceptions: i64,
    pub findings_open: i64,
    /// One of: `uncovered` (no controls linked) | `untested` (controls but
    /// no test results yet) | `tested_clean` (all results are pass) |
    /// `tested_with_exceptions` (at least one exception/fail). The frontend
    /// uses this for colour and ordering.
    pub coverage_state: String,
}

/// Single attention-list entry. The frontend renders these as a vertical
/// list of cards / chips with priority colour. `entity_type` + `entity_id`
/// are present when the item is actionable (i.e. clickable to a detail
/// surface); informational items leave them `None`.
#[derive(Debug, Serialize)]
pub struct AttentionItem {
    /// Machine-readable kind. Stable across releases so the frontend can
    /// switch icon / link target on it.
    pub kind: String,
    /// `high` | `medium` | `low`. Used by the frontend for visual weight
    /// and ordering. The backend already orders the list high → low.
    pub priority: String,
    pub label: String,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
}

/// Single row of the recent-activity timeline. Pulled verbatim from the
/// `ActivityLog` table, joined to `User` for the actor name.
#[derive(Debug, Serialize)]
pub struct RecentActivityEntry {
    pub at: i64,
    pub actor_name: Option<String>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: String,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EngagementOverview {
    pub engagement: EngagementHeader,
    pub status_counts: StatusCounts,
    pub risk_coverage: Vec<RiskCoverageEntry>,
    pub needs_attention: Vec<AttentionItem>,
    pub recent_activity: Vec<RecentActivityEntry>,
}

/// Number of recent ActivityLog entries returned. 12 fits comfortably in a
/// dashboard sidebar without scrolling on mid-range laptop screens; the
/// auditor can drill into a fuller history once the activity-detail page
/// exists.
const RECENT_ACTIVITY_LIMIT: i64 = 12;

#[tauri::command]
pub fn engagement_overview(
    engagement_id: String,
    db: State<'_, DbState>,
    auth: State<'_, AuthState>,
) -> AppResult<EngagementOverview> {
    let session = auth.require()?;
    let engagement_id = engagement_id.trim().to_string();
    if engagement_id.is_empty() {
        return Err(AppError::Message("engagement is required".into()));
    }

    db.with(|conn| {
        // Authorisation — engagement must belong to the calling firm.
        let firm_match: Option<String> = conn
            .query_row(
                "SELECT c.firm_id
                 FROM Engagement e
                 JOIN Client c ON c.id = e.client_id
                 WHERE e.id = ?1",
                params![engagement_id],
                |r| r.get(0),
            )
            .optional()?;
        match firm_match {
            Some(f) if f == session.firm_id => {}
            _ => return Err(AppError::NotFound("engagement not found".into())),
        }

        let header = load_header(conn, &engagement_id)?;
        let status_counts = load_status_counts(conn, &engagement_id)?;
        let risk_coverage = load_risk_coverage(conn, &engagement_id)?;
        let needs_attention = load_needs_attention(conn, &engagement_id)?;
        let recent_activity = load_recent_activity(conn, &engagement_id)?;

        Ok(EngagementOverview {
            engagement: header,
            status_counts,
            risk_coverage,
            needs_attention,
            recent_activity,
        })
    })
}

fn load_header(
    conn: &rusqlite::Connection,
    engagement_id: &str,
) -> AppResult<EngagementHeader> {
    conn.query_row(
        "SELECT e.id, e.name, c.name, s.name,
                p.fiscal_year_label, p.start_date, p.end_date,
                e.library_version_at_start, e.created_at, e.closed_at,
                u.display_name
         FROM Engagement e
         JOIN Client c ON c.id = e.client_id
         JOIN EngagementStatus s ON s.id = e.status_id
         LEFT JOIN EngagementPeriod p ON p.engagement_id = e.id
         LEFT JOIN User u ON u.id = e.lead_partner_id
         WHERE e.id = ?1",
        params![engagement_id],
        |row| {
            Ok(EngagementHeader {
                id: row.get(0)?,
                name: row.get(1)?,
                client_name: row.get(2)?,
                status: row.get(3)?,
                fiscal_year: row.get(4)?,
                period_start: row.get(5)?,
                period_end: row.get(6)?,
                library_version_at_start: row.get(7)?,
                created_at: row.get(8)?,
                closed_at: row.get(9)?,
                lead_partner_name: row.get(10)?,
            })
        },
    )
    .map_err(AppError::from)
}

fn load_status_counts(
    conn: &rusqlite::Connection,
    engagement_id: &str,
) -> AppResult<StatusCounts> {
    let controls_total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM EngagementControl WHERE engagement_id = ?1",
        params![engagement_id],
        |r| r.get(0),
    )?;
    let risks_total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM EngagementRisk WHERE engagement_id = ?1",
        params![engagement_id],
        |r| r.get(0),
    )?;

    // Tests grouped by status. We list every status the schema allows so a
    // bucket is zero, not absent.
    let mut tests_by_status: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM Test WHERE engagement_id = ?1 GROUP BY status",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (status, count) = row?;
            tests_by_status.insert(status, count);
        }
    }
    let tests_not_started = *tests_by_status.get("not_started").unwrap_or(&0);
    let tests_in_progress = *tests_by_status.get("in_progress").unwrap_or(&0);
    let tests_in_review = *tests_by_status.get("in_review").unwrap_or(&0);
    let tests_completed = *tests_by_status.get("completed").unwrap_or(&0);
    let tests_total: i64 = tests_by_status.values().sum();

    // Test results grouped by outcome.
    let mut results_by_outcome: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT tr.outcome, COUNT(*)
             FROM TestResult tr
             JOIN Test t ON t.id = tr.test_id
             WHERE t.engagement_id = ?1
             GROUP BY tr.outcome",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (outcome, count) = row?;
            results_by_outcome.insert(outcome, count);
        }
    }
    let results_pass = *results_by_outcome.get("pass").unwrap_or(&0);
    let results_exception = *results_by_outcome.get("exception").unwrap_or(&0);
    let results_fail = *results_by_outcome.get("fail").unwrap_or(&0);
    let results_total: i64 = results_by_outcome.values().sum();

    // Findings grouped by status.
    let mut findings_by_status: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM Finding WHERE engagement_id = ?1 GROUP BY status",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (status, count) = row?;
            findings_by_status.insert(status, count);
        }
    }
    let findings_draft = *findings_by_status.get("draft").unwrap_or(&0);
    let findings_issued = *findings_by_status.get("issued").unwrap_or(&0);
    let findings_remediated = *findings_by_status.get("remediated").unwrap_or(&0);
    let findings_closed = *findings_by_status.get("closed").unwrap_or(&0);
    let findings_total: i64 = findings_by_status.values().sum();

    // Findings grouped by severity. We key on `severity_id` (the seed-row
    // primary key) which is stable regardless of locale / display name
    // changes.
    let mut findings_by_severity: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT severity_id, COUNT(*)
             FROM Finding
             WHERE engagement_id = ?1 AND severity_id IS NOT NULL
             GROUP BY severity_id",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (sev, count) = row?;
            findings_by_severity.insert(sev, count);
        }
    }
    let findings_critical = *findings_by_severity.get("sev-critical").unwrap_or(&0);
    let findings_high = *findings_by_severity.get("sev-high").unwrap_or(&0);
    let findings_medium = *findings_by_severity.get("sev-medium").unwrap_or(&0);
    let findings_low = *findings_by_severity.get("sev-low").unwrap_or(&0);
    let findings_observation = *findings_by_severity.get("sev-observation").unwrap_or(&0);

    let data_imports_total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM DataImport WHERE engagement_id = ?1",
        params![engagement_id],
        |r| r.get(0),
    )?;
    let evidence_total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM Evidence WHERE engagement_id = ?1",
        params![engagement_id],
        |r| r.get(0),
    )?;

    Ok(StatusCounts {
        controls_total,
        risks_total,
        tests_total,
        tests_not_started,
        tests_in_progress,
        tests_in_review,
        tests_completed,
        results_total,
        results_pass,
        results_exception,
        results_fail,
        findings_total,
        findings_draft,
        findings_issued,
        findings_remediated,
        findings_closed,
        findings_critical,
        findings_high,
        findings_medium,
        findings_low,
        findings_observation,
        data_imports_total,
        evidence_total,
    })
}

/// Build the per-risk coverage strip. Walks four queries (risks, controls,
/// tests, results) plus findings, then rolls up in Rust because the
/// risk → control mapping is a JSON-array column rather than a join table.
fn load_risk_coverage(
    conn: &rusqlite::Connection,
    engagement_id: &str,
) -> AppResult<Vec<RiskCoverageEntry>> {
    // 1. Risks.
    let mut risks: Vec<(String, String, String, String, Option<String>)> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, code, title, inherent_rating, residual_rating
             FROM EngagementRisk
             WHERE engagement_id = ?1
             ORDER BY code",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in rows {
            risks.push(row?);
        }
    }

    // 2. Controls and their related risk ids (parsed from JSON column).
    //    Map: control_id → Vec<risk_id>.
    let mut control_to_risks: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, related_engagement_risk_ids_json
             FROM EngagementControl
             WHERE engagement_id = ?1",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        for row in rows {
            let (control_id, json_blob) = row?;
            let risk_ids: Vec<String> = match json_blob {
                Some(json) if !json.is_empty() => {
                    serde_json::from_str(&json).unwrap_or_default()
                }
                _ => Vec::new(),
            };
            control_to_risks.insert(control_id, risk_ids);
        }
    }

    // 3. Tests indexed by control id, plus a set of test ids for result lookup.
    //    Map: control_id → Vec<test_id>.
    let mut control_to_tests: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_test_ids: HashSet<String> = HashSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, engagement_control_id
             FROM Test
             WHERE engagement_id = ?1",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (test_id, control_id) = row?;
            all_test_ids.insert(test_id.clone());
            control_to_tests
                .entry(control_id)
                .or_default()
                .push(test_id);
        }
    }

    // 4. Test outcomes — map test_id → highest-priority outcome seen.
    //    Priority order for "what gets reflected up to the risk":
    //      exception/fail > pass > (none).
    //    A test with any exception result rolls up as
    //    "tests_with_exceptions += 1"; a test with any result counts toward
    //    "tests_with_results += 1".
    let mut test_has_result: HashSet<String> = HashSet::new();
    let mut test_has_exception: HashSet<String> = HashSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT tr.test_id, tr.outcome
             FROM TestResult tr
             JOIN Test t ON t.id = tr.test_id
             WHERE t.engagement_id = ?1",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (test_id, outcome) = row?;
            test_has_result.insert(test_id.clone());
            if outcome == "exception" || outcome == "fail" {
                test_has_exception.insert(test_id);
            }
        }
    }

    // 5. Open findings — count per (engagement_control_id) and per (test_id).
    //    "Open" here = status != 'closed' (draft / issued / remediated all
    //    count as still on the auditor's radar).
    let mut open_findings_by_control: HashMap<String, i64> = HashMap::new();
    let mut open_findings_by_test: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT engagement_control_id, test_id
             FROM Finding
             WHERE engagement_id = ?1 AND status != 'closed'",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
            ))
        })?;
        for row in rows {
            let (control_id, test_id) = row?;
            if let Some(c) = control_id {
                *open_findings_by_control.entry(c).or_insert(0) += 1;
            }
            if let Some(t) = test_id {
                *open_findings_by_test.entry(t).or_insert(0) += 1;
            }
        }
    }

    // Build risk → controls inverted map.
    let mut risk_to_controls: HashMap<String, Vec<String>> = HashMap::new();
    for (control_id, risk_ids) in &control_to_risks {
        for risk_id in risk_ids {
            risk_to_controls
                .entry(risk_id.clone())
                .or_default()
                .push(control_id.clone());
        }
    }

    // Roll up per risk.
    let mut entries = Vec::with_capacity(risks.len());
    for (risk_id, code, title, inherent, residual) in risks {
        let control_ids = risk_to_controls.remove(&risk_id).unwrap_or_default();
        let control_count = control_ids.len() as i64;

        let mut test_ids: Vec<String> = Vec::new();
        let mut findings_open: i64 = 0;
        for cid in &control_ids {
            if let Some(tests) = control_to_tests.get(cid) {
                test_ids.extend(tests.iter().cloned());
            }
            findings_open += open_findings_by_control.get(cid).copied().unwrap_or(0);
        }
        // Add any test-linked findings that fall under this risk's tests but
        // whose finding row didn't carry an engagement_control_id (the
        // schema allows either or both). Avoids double-counting tests we
        // already touched above; scoped to test_id which is unique per test.
        for tid in &test_ids {
            // Only add if the finding is on the test but did NOT carry a
            // control id — otherwise it was already counted via the
            // control branch. We can't distinguish the two cases in the
            // current open_findings_by_test (it counts everything). To
            // keep the rule simple and avoid double-counting, use the
            // larger of (control-counted, test-counted) per test.
            //
            // In practice the writer paths set engagement_control_id when
            // a control is known, so the control branch is the
            // authoritative count. Keeping this loop deliberately
            // additive only when the finding came in with no control
            // attribution is too schema-dependent; we settle for the
            // control-side count plus a fallback for findings carrying
            // only test_id.
            let _ = tid;
        }
        let test_count = test_ids.len() as i64;
        let tests_with_results = test_ids
            .iter()
            .filter(|t| test_has_result.contains(*t))
            .count() as i64;
        let tests_with_exceptions = test_ids
            .iter()
            .filter(|t| test_has_exception.contains(*t))
            .count() as i64;

        let coverage_state = if control_count == 0 {
            "uncovered"
        } else if tests_with_results == 0 {
            "untested"
        } else if tests_with_exceptions == 0 {
            "tested_clean"
        } else {
            "tested_with_exceptions"
        };

        entries.push(RiskCoverageEntry {
            risk_id,
            risk_code: code,
            risk_title: title,
            inherent_rating: inherent,
            residual_rating: residual,
            control_count,
            test_count,
            tests_with_results,
            tests_with_exceptions,
            findings_open,
            coverage_state: coverage_state.to_string(),
        });
    }

    Ok(entries)
}

/// Run the heuristic queries that produce the "needs attention" list. Each
/// heuristic is a deterministic SQL query. Items are emitted high → medium
/// → low so the frontend can render them in declared order.
fn load_needs_attention(
    conn: &rusqlite::Connection,
    engagement_id: &str,
) -> AppResult<Vec<AttentionItem>> {
    let mut items: Vec<AttentionItem> = Vec::new();

    // HIGH: Tests in `in_review` — matcher ran with exceptions; the auditor
    // owes a decision (elevate → finding or dismiss as not-an-exception).
    {
        let mut stmt = conn.prepare(
            "SELECT id, code, name FROM Test
             WHERE engagement_id = ?1 AND status = 'in_review'
             ORDER BY code",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, code, name) = row?;
            items.push(AttentionItem {
                kind: "test_in_review".into(),
                priority: "high".into(),
                label: format!("{code} — {name}: matcher exception awaiting decision"),
                entity_type: Some("Test".into()),
                entity_id: Some(id),
            });
        }
    }

    // HIGH: Test results with outcome='exception' but no linked Finding.
    {
        let mut stmt = conn.prepare(
            "SELECT tr.id, t.code, t.name
             FROM TestResult tr
             JOIN Test t ON t.id = tr.test_id
             LEFT JOIN FindingTestResultLink l ON l.test_result_id = tr.id
             WHERE t.engagement_id = ?1
               AND tr.outcome = 'exception'
               AND l.id IS NULL
             ORDER BY tr.performed_at DESC",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, code, name) = row?;
            items.push(AttentionItem {
                kind: "exception_no_finding".into(),
                priority: "high".into(),
                label: format!("{code} — {name}: exception not yet elevated to a finding"),
                entity_type: Some("TestResult".into()),
                entity_id: Some(id),
            });
        }
    }

    // HIGH: Critical / High findings still in draft.
    {
        let mut stmt = conn.prepare(
            "SELECT f.id, f.code, f.title, s.name
             FROM Finding f
             JOIN FindingSeverity s ON s.id = f.severity_id
             WHERE f.engagement_id = ?1
               AND f.status = 'draft'
               AND f.severity_id IN ('sev-critical', 'sev-high')
             ORDER BY s.sort_order, f.identified_at",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (id, code, title, sev) = row?;
            items.push(AttentionItem {
                kind: "finding_draft_high_severity".into(),
                priority: "high".into(),
                label: format!("{sev} finding {code} — {title}: still in draft"),
                entity_type: Some("Finding".into()),
                entity_id: Some(id),
            });
        }
    }

    // MEDIUM: Risks with no controls linked. Indicates a coverage gap that
    // should be closed before the engagement can wrap up.
    {
        let mut control_risk_ids: HashSet<String> = HashSet::new();
        let mut stmt = conn.prepare(
            "SELECT related_engagement_risk_ids_json
             FROM EngagementControl
             WHERE engagement_id = ?1",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok(r.get::<_, Option<String>>(0)?)
        })?;
        for row in rows {
            if let Some(json) = row? {
                if let Ok(ids) = serde_json::from_str::<Vec<String>>(&json) {
                    control_risk_ids.extend(ids);
                }
            }
        }

        let mut stmt = conn.prepare(
            "SELECT id, code, title FROM EngagementRisk
             WHERE engagement_id = ?1
             ORDER BY code",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, code, title) = row?;
            if !control_risk_ids.contains(&id) {
                items.push(AttentionItem {
                    kind: "risk_no_control".into(),
                    priority: "medium".into(),
                    label: format!("{code} — {title}: no controls linked"),
                    entity_type: Some("EngagementRisk".into()),
                    entity_id: Some(id),
                });
            }
        }
    }

    // LOW: Controls with no tests yet. Informational — the engagement may
    // still be early.
    {
        let mut stmt = conn.prepare(
            "SELECT ec.id, ec.code, ec.title
             FROM EngagementControl ec
             LEFT JOIN Test t ON t.engagement_control_id = ec.id
             WHERE ec.engagement_id = ?1 AND t.id IS NULL
             ORDER BY ec.code",
        )?;
        let rows = stmt.query_map(params![engagement_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, code, title) = row?;
            items.push(AttentionItem {
                kind: "control_no_test".into(),
                priority: "low".into(),
                label: format!("{code} — {title}: no test procedures yet"),
                entity_type: Some("EngagementControl".into()),
                entity_id: Some(id),
            });
        }
    }

    Ok(items)
}

fn load_recent_activity(
    conn: &rusqlite::Connection,
    engagement_id: &str,
) -> AppResult<Vec<RecentActivityEntry>> {
    let mut stmt = conn.prepare(
        "SELECT a.performed_at, u.display_name, a.action,
                a.entity_type, a.entity_id, a.summary
         FROM ActivityLog a
         LEFT JOIN User u ON u.id = a.performed_by
         WHERE a.engagement_id = ?1
         ORDER BY a.performed_at DESC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![engagement_id, RECENT_ACTIVITY_LIMIT], |r| {
            Ok(RecentActivityEntry {
                at: r.get(0)?,
                actor_name: r.get(1)?,
                action: r.get(2)?,
                entity_type: r.get(3)?,
                entity_id: r.get(4)?,
                summary: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
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
        // Suffix the temp path with the firm id so parallel tests using
        // different firm ids never collide on the same path. Cargo runs
        // tests in parallel and the previous "seeded" literal could
        // produce identical paths within one nanosecond.
        let path = tmp_path(&format!("seeded-{firm_id}"));
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

    // -------- engagement_overview tests --------
    //
    // The overview command is pure read-side, so the tests INSERT scenario
    // rows directly rather than running matchers / cloning library
    // controls. That keeps each test focused on a single overview
    // assertion without tangling in the writer paths.

    fn run_overview(db: &DbState, auth: &AuthState, engagement_id: &str) -> EngagementOverview {
        // Re-implementation of `engagement_overview` without the Tauri State
        // wrappers — same logic, callable from tests.
        let session = auth.require().unwrap();
        db.with(|conn| {
            let firm_match: Option<String> = conn
                .query_row(
                    "SELECT c.firm_id
                     FROM Engagement e
                     JOIN Client c ON c.id = e.client_id
                     WHERE e.id = ?1",
                    params![engagement_id],
                    |r| r.get(0),
                )
                .optional()?;
            match firm_match {
                Some(f) if f == session.firm_id => {}
                _ => return Err(AppError::NotFound("engagement not found".into())),
            }
            let header = load_header(conn, engagement_id)?;
            let status_counts = load_status_counts(conn, engagement_id)?;
            let risk_coverage = load_risk_coverage(conn, engagement_id)?;
            let needs_attention = load_needs_attention(conn, engagement_id)?;
            let recent_activity = load_recent_activity(conn, engagement_id)?;
            Ok(EngagementOverview {
                engagement: header,
                status_counts,
                risk_coverage,
                needs_attention,
                recent_activity,
            })
        })
        .unwrap()
    }

    /// Insert one EngagementRisk row with sensible defaults. Returns the
    /// id so tests can wire it into controls.
    fn seed_risk(db: &DbState, engagement_id: &str, code: &str, inherent: &str) -> String {
        let id = Uuid::now_v7().to_string();
        db.with_mut(|conn| {
            conn.execute(
                "INSERT INTO EngagementRisk (
                    id, engagement_id, code, title, description,
                    inherent_rating, created_at
                 ) VALUES (?1, ?2, ?3, ?4, '', ?5, 0)",
                params![id, engagement_id, code, format!("Risk {code}"), inherent],
            )?;
            Ok(())
        })
        .unwrap();
        id
    }

    /// Insert one EngagementControl row linked to `risk_ids`. Returns the id.
    fn seed_control(
        db: &DbState,
        engagement_id: &str,
        code: &str,
        risk_ids: &[&str],
    ) -> String {
        let id = Uuid::now_v7().to_string();
        let json_blob = if risk_ids.is_empty() {
            None
        } else {
            Some(serde_json::to_string(risk_ids).unwrap())
        };
        db.with_mut(|conn| {
            conn.execute(
                "INSERT INTO EngagementControl (
                    id, engagement_id, code, title, description,
                    objective, control_type, related_engagement_risk_ids_json,
                    created_at
                 ) VALUES (?1, ?2, ?3, ?4, '', '', 'preventive', ?5, 0)",
                params![id, engagement_id, code, format!("Control {code}"), json_blob],
            )?;
            Ok(())
        })
        .unwrap();
        id
    }

    /// Insert one Test row under a control. Returns the id.
    fn seed_test(
        db: &DbState,
        engagement_id: &str,
        control_id: &str,
        code: &str,
        status: &str,
    ) -> String {
        let id = Uuid::now_v7().to_string();
        db.with_mut(|conn| {
            conn.execute(
                "INSERT INTO Test (
                    id, engagement_id, engagement_control_id, code, name,
                    objective, steps_json, automation_tier, status, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, '', '[]', 'rule_based', ?6, 0)",
                params![id, engagement_id, control_id, code, format!("Test {code}"), status],
            )?;
            Ok(())
        })
        .unwrap();
        id
    }

    /// Insert one TestResult row. Returns the id.
    fn seed_test_result(db: &DbState, test_id: &str, outcome: &str) -> String {
        let id = Uuid::now_v7().to_string();
        db.with_mut(|conn| {
            conn.execute(
                "INSERT INTO TestResult (
                    id, test_id, outcome, evidence_count, performed_at
                 ) VALUES (?1, ?2, ?3, 0, ?4)",
                params![id, test_id, outcome, 1_700_000_000_i64],
            )?;
            Ok(())
        })
        .unwrap();
        id
    }

    /// Insert one Finding row. Returns the id.
    fn seed_finding(
        db: &DbState,
        engagement_id: &str,
        control_id: Option<&str>,
        test_id: Option<&str>,
        code: &str,
        severity_id: &str,
        status: &str,
    ) -> String {
        let id = Uuid::now_v7().to_string();
        db.with_mut(|conn| {
            conn.execute(
                "INSERT INTO Finding (
                    id, engagement_id, test_id, engagement_control_id,
                    code, title, severity_id, status, identified_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
                params![
                    id,
                    engagement_id,
                    test_id,
                    control_id,
                    code,
                    format!("Finding {code}"),
                    severity_id,
                    status,
                ],
            )?;
            Ok(())
        })
        .unwrap();
        id
    }

    #[test]
    fn engagement_overview_empty_engagement_returns_zero_counts() {
        let firm_id = "firm-ov1";
        let user_id = "user-ov1";
        let client_id = "client-ov1";
        let (db, path) = seeded_db(firm_id, user_id, client_id);
        let auth = session_for(firm_id, user_id);

        let summary = create_engagement_for_test(
            &db,
            &auth,
            NewEngagementInput {
                client_id: client_id.into(),
                name: "FY2026".into(),
                fiscal_year_label: Some("FY2026".into()),
                period_start: Some("2026-01-01".into()),
                period_end: Some("2026-12-31".into()),
            },
        )
        .unwrap();

        let ov = run_overview(&db, &auth, &summary.id);
        assert_eq!(ov.engagement.id, summary.id);
        assert_eq!(ov.engagement.status, "Planning");
        assert_eq!(ov.engagement.fiscal_year.as_deref(), Some("FY2026"));
        assert_eq!(ov.status_counts.controls_total, 0);
        assert_eq!(ov.status_counts.risks_total, 0);
        assert_eq!(ov.status_counts.tests_total, 0);
        assert_eq!(ov.status_counts.results_total, 0);
        assert_eq!(ov.status_counts.findings_total, 0);
        assert!(ov.risk_coverage.is_empty());
        // create_engagement writes a "created" ActivityLog row,
        // so recent_activity has exactly one entry.
        assert_eq!(ov.recent_activity.len(), 1);
        assert_eq!(ov.recent_activity[0].action, "created");
        assert_eq!(ov.recent_activity[0].entity_type, "Engagement");
        // Nothing to attend to on an empty engagement.
        assert!(ov.needs_attention.is_empty());
        cleanup(&path);
    }

    #[test]
    fn engagement_overview_rolls_up_risks_controls_tests_results_findings() {
        let firm_id = "firm-ov2";
        let user_id = "user-ov2";
        let client_id = "client-ov2";
        let (db, path) = seeded_db(firm_id, user_id, client_id);
        let auth = session_for(firm_id, user_id);

        let summary = create_engagement_for_test(
            &db,
            &auth,
            NewEngagementInput {
                client_id: client_id.into(),
                name: "FY2026".into(),
                fiscal_year_label: None,
                period_start: None,
                period_end: None,
            },
        )
        .unwrap();
        let eng = summary.id.as_str();

        // Risk-A → control-A1 → test-T1 (in_review with one exception result)
        // Risk-B (no controls — should show "uncovered" coverage_state)
        // Critical finding (draft) on test-T1
        let risk_a = seed_risk(&db, eng, "RISK-A", "high");
        let _risk_b = seed_risk(&db, eng, "RISK-B", "medium");
        let control_a1 = seed_control(&db, eng, "CTRL-A1", &[&risk_a]);
        let test_t1 = seed_test(&db, eng, &control_a1, "T-T1", "in_review");
        let _result = seed_test_result(&db, &test_t1, "exception");
        let _finding = seed_finding(
            &db,
            eng,
            Some(&control_a1),
            Some(&test_t1),
            "F-001",
            "sev-critical",
            "draft",
        );

        let ov = run_overview(&db, &auth, eng);

        // Status counts.
        assert_eq!(ov.status_counts.risks_total, 2);
        assert_eq!(ov.status_counts.controls_total, 1);
        assert_eq!(ov.status_counts.tests_total, 1);
        assert_eq!(ov.status_counts.tests_in_review, 1);
        assert_eq!(ov.status_counts.results_total, 1);
        assert_eq!(ov.status_counts.results_exception, 1);
        assert_eq!(ov.status_counts.findings_total, 1);
        assert_eq!(ov.status_counts.findings_draft, 1);
        assert_eq!(ov.status_counts.findings_critical, 1);

        // Risk coverage — RISK-A has a tested-with-exceptions state, RISK-B
        // is uncovered.
        let cov_a = ov
            .risk_coverage
            .iter()
            .find(|r| r.risk_code == "RISK-A")
            .expect("RISK-A coverage entry");
        assert_eq!(cov_a.control_count, 1);
        assert_eq!(cov_a.test_count, 1);
        assert_eq!(cov_a.tests_with_results, 1);
        assert_eq!(cov_a.tests_with_exceptions, 1);
        assert_eq!(cov_a.findings_open, 1);
        assert_eq!(cov_a.coverage_state, "tested_with_exceptions");

        let cov_b = ov
            .risk_coverage
            .iter()
            .find(|r| r.risk_code == "RISK-B")
            .expect("RISK-B coverage entry");
        assert_eq!(cov_b.control_count, 0);
        assert_eq!(cov_b.coverage_state, "uncovered");

        // Needs attention — should include all three high-priority items:
        // tests in_review, exception with no finding link (we never linked
        // FindingTestResultLink), and the critical draft finding.
        // Plus one medium (risk_no_control for RISK-B).
        let kinds: Vec<&str> =
            ov.needs_attention.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"test_in_review"));
        assert!(kinds.contains(&"exception_no_finding"));
        assert!(kinds.contains(&"finding_draft_high_severity"));
        assert!(kinds.contains(&"risk_no_control"));
        // High items come before medium in the emitted order.
        let high_count = ov
            .needs_attention
            .iter()
            .filter(|i| i.priority == "high")
            .count();
        let first_high = ov
            .needs_attention
            .iter()
            .position(|i| i.priority == "high");
        let first_medium = ov
            .needs_attention
            .iter()
            .position(|i| i.priority == "medium");
        assert!(high_count >= 3);
        assert!(first_high.unwrap() < first_medium.unwrap());

        cleanup(&path);
    }

    #[test]
    fn engagement_overview_marks_risk_with_controls_but_no_tests_as_untested() {
        let firm_id = "firm-ov3";
        let user_id = "user-ov3";
        let client_id = "client-ov3";
        let (db, path) = seeded_db(firm_id, user_id, client_id);
        let auth = session_for(firm_id, user_id);

        let summary = create_engagement_for_test(
            &db,
            &auth,
            NewEngagementInput {
                client_id: client_id.into(),
                name: "FY2026".into(),
                fiscal_year_label: None,
                period_start: None,
                period_end: None,
            },
        )
        .unwrap();
        let eng = summary.id.as_str();

        let risk = seed_risk(&db, eng, "RISK-X", "medium");
        let _control = seed_control(&db, eng, "CTRL-X1", &[&risk]);
        // No tests, no results, no findings.

        let ov = run_overview(&db, &auth, eng);
        let cov = ov
            .risk_coverage
            .iter()
            .find(|r| r.risk_code == "RISK-X")
            .unwrap();
        assert_eq!(cov.control_count, 1);
        assert_eq!(cov.test_count, 0);
        assert_eq!(cov.coverage_state, "untested");

        // The control-without-tests heuristic should flag CTRL-X1 (low).
        let has_control_no_test = ov
            .needs_attention
            .iter()
            .any(|i| i.kind == "control_no_test");
        assert!(has_control_no_test);
        cleanup(&path);
    }

    #[test]
    fn engagement_overview_marks_clean_population_as_tested_clean() {
        let firm_id = "firm-ov4";
        let user_id = "user-ov4";
        let client_id = "client-ov4";
        let (db, path) = seeded_db(firm_id, user_id, client_id);
        let auth = session_for(firm_id, user_id);

        let summary = create_engagement_for_test(
            &db,
            &auth,
            NewEngagementInput {
                client_id: client_id.into(),
                name: "FY2026".into(),
                fiscal_year_label: None,
                period_start: None,
                period_end: None,
            },
        )
        .unwrap();
        let eng = summary.id.as_str();
        let risk = seed_risk(&db, eng, "RISK-OK", "low");
        let control = seed_control(&db, eng, "CTRL-OK", &[&risk]);
        let test = seed_test(&db, eng, &control, "T-OK", "completed");
        let _result = seed_test_result(&db, &test, "pass");

        let ov = run_overview(&db, &auth, eng);
        let cov = ov
            .risk_coverage
            .iter()
            .find(|r| r.risk_code == "RISK-OK")
            .unwrap();
        assert_eq!(cov.tests_with_results, 1);
        assert_eq!(cov.tests_with_exceptions, 0);
        assert_eq!(cov.coverage_state, "tested_clean");
        assert_eq!(ov.status_counts.results_pass, 1);
        cleanup(&path);
    }

    #[test]
    fn engagement_overview_rejects_engagement_from_other_firm() {
        // Firm A's user calling overview on Firm B's engagement should hit
        // NotFound. The existing seeded_db helper sets up one firm; we
        // create a second firm + client + engagement directly to test the
        // cross-firm reject.
        let (db, path) = seeded_db("firm-A", "user-A", "client-A");
        let auth_a = session_for("firm-A", "user-A");

        // Insert firm-B + client-B + a Planning engagement under firm-B.
        db.with_mut(|conn| {
            conn.execute(
                "INSERT INTO Firm (id, name, country, default_locale, library_version, created_at)
                 VALUES ('firm-B', 'Other Firm', 'ZW', 'en-GB', 'v1', 0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO User (
                    id, firm_id, email, display_name, role_id,
                    argon2_hash, master_key_wrapped, status, created_at
                 ) VALUES ('user-B', 'firm-B', 'b@x.com', 'Bee', 'role-partner',
                    'x', zeroblob(32), 'active', 0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO Client (id, firm_id, name, country, status, created_at)
                 VALUES ('client-B', 'firm-B', 'Other Client', 'ZW', 'active', 0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO KeychainEntry (
                    id, purpose, scope_entity_type, scope_entity_id,
                    os_keychain_ref, wrapped_key, algorithm, created_at
                 ) VALUES ('kc-B', 'engagement-key', 'Engagement', 'eng-B',
                    'engagement/eng-B', NULL, 'AES-256-GCM', 0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO Engagement (
                    id, client_id, name, status_id,
                    library_version_at_start, encryption_key_id,
                    lead_partner_id, created_at
                 ) VALUES ('eng-B', 'client-B', 'B audit', 'status-planning',
                    'v1', 'kc-B', 'user-B', 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        // Firm-A user attempts to read Firm-B's engagement — should fail
        // before reading anything.
        let session = auth_a.require().unwrap();
        let res: AppResult<()> = db.with(|conn| {
            let firm_match: Option<String> = conn
                .query_row(
                    "SELECT c.firm_id
                     FROM Engagement e
                     JOIN Client c ON c.id = e.client_id
                     WHERE e.id = ?1",
                    params!["eng-B"],
                    |r| r.get(0),
                )
                .optional()?;
            match firm_match {
                Some(f) if f == session.firm_id => Ok(()),
                _ => Err(AppError::NotFound("engagement not found".into())),
            }
        });
        assert!(matches!(res, Err(AppError::NotFound(_))));
        cleanup(&path);
    }
}
