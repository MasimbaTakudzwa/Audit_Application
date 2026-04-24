//! Change-management rules.
//!
//! Two rules:
//!   - **change-approval-before-deployment** — flag production changes that
//!     either (a) were deployed without a recorded approval, (b) were
//!     approved *after* deployment, or (c) were approved by the same
//!     individual who deployed them (per-change SoD breach observed on the
//!     change record itself).
//!   - **sod-dev-vs-deploy** — structural SoD check: reconcile the
//!     production-deployment tool's permission list against the source
//!     repository's write-access list, and flag any user appearing on both.
//!     This is distinct from the per-change check above — it asks "who
//!     *could* bypass review?" rather than "did anyone bypass review on
//!     this specific change?". An overlapping user is an exception unless
//!     a documented compensating control is evidenced.
//!
//! Both matchers expect CSV exports. For the approval-before-deployment
//! rule, a single change-management tool export (ServiceNow, Jira, Remedy,
//! in-house ticketing). For the SoD rule, two exports: a deployment-tool
//! permission matrix and a source-repository access export. Column names
//! are canonicalised (lower-cased, underscore/dash/space stripped) before
//! lookup, so "User Name", "user_name", and "User-Name" all match.
//!
//! Intentional simplicity:
//!   - Pure functions. Take `Table` inputs, return reports. I/O and
//!     persistence live in the command layer.
//!   - Standard (pre-approved) changes are out of scope by convention for
//!     the approval rule — blanket-approved via CAB policy, not per-change.
//!   - Cancelled changes are skipped — they never deployed.
//!   - Date parsing covers ISO 8601 date and datetime plus Unix epoch.
//!     ServiceNow / Jira / Remedy all export in ISO. No FILETIME here —
//!     change-management tools don't use it.
//!   - The SoD rule does not attempt to identify compensating controls
//!     (that's evidence-based, not data-driven). It flags every
//!     intersecting user; the auditor reviews and documents the response.

use std::collections::HashMap;

use serde::Serialize;

use super::csv::{find_column, Row, Table};

const CHANGE_ID_CANDIDATES: &[&str] = &[
    "changeid",
    "change_id",
    "changenumber",
    "ticket",
    "ticketid",
    "ticket_id",
    "number",
    "id",
];

const CHANGE_TYPE_CANDIDATES: &[&str] =
    &["changetype", "change_type", "type", "category", "classification"];

const CHANGE_STATUS_CANDIDATES: &[&str] = &["status", "state", "stage"];

const APPROVER_CANDIDATES: &[&str] = &[
    "approver",
    "approvedby",
    "approved_by",
    "authoriser",
    "authorisedby",
    "authorised_by",
    "authorizer",
    "authorizedby",
    "authorized_by",
    "approval_owner",
];

const APPROVED_AT_CANDIDATES: &[&str] = &[
    "approvedat",
    "approved_at",
    "approveddate",
    "approved_date",
    "approvaldate",
    "approval_date",
    "authorisedat",
    "authorised_at",
    "authorizedat",
    "authorized_at",
];

const IMPLEMENTER_CANDIDATES: &[&str] = &[
    "implementer",
    "implementedby",
    "implemented_by",
    "deployer",
    "deployedby",
    "deployed_by",
    "releasedby",
    "released_by",
    "executedby",
    "executed_by",
    "closedby",
    "closed_by",
];

const DEPLOYED_AT_CANDIDATES: &[&str] = &[
    "deployedat",
    "deployed_at",
    "deploymentdate",
    "deployment_date",
    "releaseddate",
    "released_date",
    "releasedate",
    "release_date",
    "actualenddate",
    "actual_end_date",
    "actualend",
    "actual_end",
    "implementedat",
    "implemented_at",
    "implementeddate",
    "implemented_date",
    "closedat",
    "closed_at",
];

/// A change-management rule exception.
#[derive(Debug, Clone, Serialize)]
pub struct ChangeException {
    /// Rule-specific kind tag. One of:
    /// - `missing_approval` — deployed but no recorded approval
    /// - `approval_after_deployment` — approval timestamp is after deployment
    /// - `approver_is_implementer` — same person approved and deployed
    pub kind: String,
    pub change_id: String,
    pub change_ordinal: usize,
    pub change_type: Option<String>,
    pub status: Option<String>,
    pub approver: Option<String>,
    pub approved_at_secs: Option<i64>,
    pub implementer: Option<String>,
    pub deployed_at_secs: Option<i64>,
    pub change_row: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeReport {
    pub rule: String,
    pub changes_considered: usize,
    pub changes_skipped_standard: usize,
    pub changes_skipped_cancelled: usize,
    pub changes_skipped_not_deployed: usize,
    pub changes_skipped_no_id: usize,
    pub changes_skipped_unparseable_dates: usize,
    pub exceptions: Vec<ChangeException>,
}

pub fn run_change_approval_before_deployment(changes: &Table) -> ChangeReport {
    let id_col = find_column(changes, CHANGE_ID_CANDIDATES);
    let type_col = find_column(changes, CHANGE_TYPE_CANDIDATES);
    let status_col = find_column(changes, CHANGE_STATUS_CANDIDATES);
    let approver_col = find_column(changes, APPROVER_CANDIDATES);
    let approved_at_col = find_column(changes, APPROVED_AT_CANDIDATES);
    let implementer_col = find_column(changes, IMPLEMENTER_CANDIDATES);
    let deployed_at_col = find_column(changes, DEPLOYED_AT_CANDIDATES);

    let mut exceptions = Vec::new();
    let mut skipped_standard = 0usize;
    let mut skipped_cancelled = 0usize;
    let mut skipped_not_deployed = 0usize;
    let mut skipped_no_id = 0usize;
    let mut skipped_unparseable = 0usize;

    for row in &changes.rows {
        let change_id = cell(row, id_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let Some(change_id) = change_id else {
            skipped_no_id += 1;
            continue;
        };

        let change_type = cell(row, type_col.as_deref()).map(|s| s.trim().to_string());
        let status = cell(row, status_col.as_deref()).map(|s| s.trim().to_string());

        // Standard changes are blanket-pre-approved — out of scope for a
        // per-change approval test.
        if change_type
            .as_deref()
            .map(is_standard_change)
            .unwrap_or(false)
        {
            skipped_standard += 1;
            continue;
        }

        // Cancelled / rejected changes didn't deploy, so "approved before
        // deployment" doesn't apply.
        if status.as_deref().map(is_terminal_not_deployed).unwrap_or(false) {
            skipped_cancelled += 1;
            continue;
        }

        // Deployment-side evidence: at least one of a deployment timestamp
        // or a deployer identity must be present for the rule to apply. A
        // change with neither hasn't been deployed (yet), so it's out of
        // scope for this test.
        let deployed_at_raw = cell(row, deployed_at_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let implementer = cell(row, implementer_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if deployed_at_raw.is_none() && implementer.is_none() && !looks_deployed(status.as_deref())
        {
            skipped_not_deployed += 1;
            continue;
        }

        let deployed_at_secs = match deployed_at_raw.as_deref() {
            Some(raw) => match parse_timestamp(raw) {
                Some(secs) => Some(secs),
                None => {
                    skipped_unparseable += 1;
                    continue;
                }
            },
            None => None,
        };

        let approver = cell(row, approver_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let approved_at_raw = cell(row, approved_at_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let approved_at_secs = match approved_at_raw.as_deref() {
            Some(raw) => match parse_timestamp(raw) {
                Some(secs) => Some(secs),
                None => {
                    skipped_unparseable += 1;
                    continue;
                }
            },
            None => None,
        };

        // Evaluate the three rules in order. A single change can trigger
        // multiple kinds of exception (e.g. missing approval AND same-person);
        // we emit one row per distinct problem so the auditor sees each
        // symptom rather than a conflated superset.

        let missing_approval = approver.is_none() && approved_at_secs.is_none();
        if missing_approval {
            exceptions.push(ChangeException {
                kind: "missing_approval".into(),
                change_id: change_id.clone(),
                change_ordinal: row.ordinal,
                change_type: change_type.clone(),
                status: status.clone(),
                approver: approver.clone(),
                approved_at_secs,
                implementer: implementer.clone(),
                deployed_at_secs,
                change_row: row.raw_values.clone(),
            });
        }

        if let (Some(a), Some(d)) = (approved_at_secs, deployed_at_secs) {
            if a > d {
                exceptions.push(ChangeException {
                    kind: "approval_after_deployment".into(),
                    change_id: change_id.clone(),
                    change_ordinal: row.ordinal,
                    change_type: change_type.clone(),
                    status: status.clone(),
                    approver: approver.clone(),
                    approved_at_secs,
                    implementer: implementer.clone(),
                    deployed_at_secs,
                    change_row: row.raw_values.clone(),
                });
            }
        }

        if let (Some(a), Some(i)) = (approver.as_deref(), implementer.as_deref()) {
            if normalise_identity(a) == normalise_identity(i) && !a.is_empty() {
                exceptions.push(ChangeException {
                    kind: "approver_is_implementer".into(),
                    change_id: change_id.clone(),
                    change_ordinal: row.ordinal,
                    change_type: change_type.clone(),
                    status: status.clone(),
                    approver: approver.clone(),
                    approved_at_secs,
                    implementer: implementer.clone(),
                    deployed_at_secs,
                    change_row: row.raw_values.clone(),
                });
            }
        }
    }

    ChangeReport {
        rule: "change_approval_before_deployment".into(),
        changes_considered: changes.rows.len(),
        changes_skipped_standard: skipped_standard,
        changes_skipped_cancelled: skipped_cancelled,
        changes_skipped_not_deployed: skipped_not_deployed,
        changes_skipped_no_id: skipped_no_id,
        changes_skipped_unparseable_dates: skipped_unparseable,
        exceptions,
    }
}

fn cell<'a>(row: &'a Row, column: Option<&str>) -> Option<&'a String> {
    column.and_then(|c| row.values.get(c))
}

fn is_standard_change(raw: &str) -> bool {
    let v = raw.trim().to_ascii_lowercase();
    matches!(v.as_str(), "standard" | "std" | "pre-approved" | "preapproved")
}

fn is_terminal_not_deployed(raw: &str) -> bool {
    // Only genuinely terminal states — "this change will never deploy".
    // In-flight states (Draft, New, Assess, Approval, Awaiting Approval,
    // Pending, On Hold) are handled by the deployment-evidence check below,
    // which sorts them into `skipped_not_deployed`.
    let v = raw.trim().to_ascii_lowercase();
    matches!(v.as_str(), "cancelled" | "canceled" | "rejected" | "withdrawn")
}

fn looks_deployed(status: Option<&str>) -> bool {
    let Some(raw) = status else {
        return false;
    };
    let v = raw.trim().to_ascii_lowercase();
    matches!(
        v.as_str(),
        "deployed"
            | "released"
            | "implemented"
            | "closed"
            | "complete"
            | "completed"
            | "successful"
            | "success"
            | "review"
            | "post-implementation-review"
            | "postimplementationreview"
    )
}

fn normalise_identity(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

/// Parse an ISO 8601 date/datetime or Unix epoch timestamp to seconds since
/// the epoch. Mirrors `matcher::access_review::parse_last_logon` but without
/// the Windows FILETIME branch — change-management tools export ISO/epoch.
fn parse_timestamp(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        let n: i128 = s.parse().ok()?;
        return match s.len() {
            9..=10 => i64::try_from(n).ok(),
            12..=13 => i64::try_from(n / 1000).ok(),
            _ => None,
        };
    }
    parse_iso_8601(s)
}

fn parse_iso_8601(s: &str) -> Option<i64> {
    let normalised = s.trim_end_matches('Z').trim().to_string();
    let split: Vec<&str> = normalised
        .splitn(2, |c| c == 'T' || c == ' ')
        .collect();
    let date_part = split.first()?;
    let time_part = split.get(1);

    let date_bits: Vec<&str> = date_part.split('-').collect();
    if date_bits.len() != 3 {
        return None;
    }
    let year: i32 = date_bits[0].parse().ok()?;
    let month: u32 = date_bits[1].parse().ok()?;
    let day: u32 = date_bits[2].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let mut secs = days * 86_400;

    if let Some(tp) = time_part {
        let tp = tp.split('.').next().unwrap_or("");
        let time_bits: Vec<&str> = tp.split(':').collect();
        if time_bits.len() < 2 {
            return None;
        }
        let hh: i64 = time_bits[0].parse().ok()?;
        let mm: i64 = time_bits[1].parse().ok()?;
        let ss: i64 = time_bits.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        if !(0..24).contains(&hh) || !(0..60).contains(&mm) || !(0..60).contains(&ss) {
            return None;
        }
        secs += hh * 3600 + mm * 60 + ss;
    }

    Some(secs)
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * if m > 2 { m - 3 } else { m + 9 } + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era as i64 * 146_097 + doe as i64 - 719_468
}

// ---------------------------------------------------------------------------
// Rule 2: segregation of duties between dev and deploy
// ---------------------------------------------------------------------------

/// Candidate columns for a username / identity on either the deploy
/// permission export or the source-repository access export. Kept
/// deliberately broad — these exports come from unrelated tools
/// (deployment platforms, CI/CD, source hosts, IAM) that all spell the
/// user identity column differently. The match is on canonicalised
/// headers (lower-cased, underscore/dash/space stripped).
const SOD_USERNAME_CANDIDATES: &[&str] = &[
    "user",
    "username",
    "user_name",
    "login",
    "logon",
    "samaccountname",
    "principal",
    "account",
    "userid",
    "user_id",
    "identity",
    "displayname",
    "display_name",
    "email",
    "email_address",
    "upn",
    "user_principal_name",
    "member",
    "developer",
    "engineer",
    "operator",
];

/// A segregation-of-duties exception: a single user appears in both the
/// deploy-to-production permission list and the source-repository write
/// access list. The auditor confirms either (a) the user has since been
/// removed from one side, or (b) a documented compensating control
/// (four-eyes review, post-deployment monitoring) operated during the
/// period.
#[derive(Debug, Clone, Serialize)]
pub struct SoDException {
    /// Rule-specific kind tag. Currently only `user_has_dev_and_deploy`.
    pub kind: String,
    /// Normalised username the intersection hit on (lower-cased, trimmed).
    /// The auditor reads this plus the raw rows below to confirm identity.
    pub username: String,
    /// The matching deploy-side row, in original column order.
    pub deploy_row: Vec<String>,
    /// The matching source-side row, in original column order.
    pub source_row: Vec<String>,
    /// 1-based ordinal of the deploy-side row in its source file.
    pub deploy_ordinal: usize,
    /// 1-based ordinal of the source-side row in its source file.
    pub source_ordinal: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SoDReport {
    pub rule: String,
    pub deploy_rows_considered: usize,
    pub deploy_rows_skipped_unmatchable: usize,
    pub source_rows_considered: usize,
    pub source_rows_skipped_unmatchable: usize,
    /// Count of distinct users on the deploy side (after normalisation).
    pub deploy_unique_users: usize,
    /// Count of distinct users on the source side (after normalisation).
    pub source_unique_users: usize,
    /// Count of users appearing in both sets. Equal to `exceptions.len()`.
    pub intersecting_users: usize,
    pub exceptions: Vec<SoDException>,
}

/// Reconcile a deployment-tool permission export against a source
/// repository access export. Any user appearing in both is a potential
/// SoD breach — they have both "author a change" and "deploy to
/// production" capability, bypassing the `CHG-C-002` segregation control
/// unless a compensating control is evidenced.
///
/// Normalisation: usernames are trimmed and lower-cased before matching,
/// so `"Alice"` in one export matches `"alice"` in the other. A user
/// appearing multiple times in one side is deduplicated (we keep the
/// first occurrence and its raw row for the exception; `*_unique_users`
/// counters reflect the distinct count).
pub fn run_sod_dev_vs_deploy(
    deploy_access: &Table,
    source_access: &Table,
) -> SoDReport {
    let deploy_col = find_column(deploy_access, SOD_USERNAME_CANDIDATES);
    let source_col = find_column(source_access, SOD_USERNAME_CANDIDATES);

    // First occurrence per normalised username on each side.
    // (ordinal, raw_row) tuple gives the auditor something concrete to
    // locate in the original file when reviewing the exception.
    let mut deploy_by_user: HashMap<String, (usize, Vec<String>)> = HashMap::new();
    let mut deploy_skipped = 0usize;
    for row in &deploy_access.rows {
        let raw = cell(row, deploy_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let Some(raw) = raw else {
            deploy_skipped += 1;
            continue;
        };
        let key = normalise_identity(&raw);
        deploy_by_user
            .entry(key)
            .or_insert((row.ordinal, row.raw_values.clone()));
    }

    let mut source_by_user: HashMap<String, (usize, Vec<String>)> = HashMap::new();
    let mut source_skipped = 0usize;
    for row in &source_access.rows {
        let raw = cell(row, source_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let Some(raw) = raw else {
            source_skipped += 1;
            continue;
        };
        let key = normalise_identity(&raw);
        source_by_user
            .entry(key)
            .or_insert((row.ordinal, row.raw_values.clone()));
    }

    // Deterministic ordering — sort the intersection keys so exception
    // output is stable across runs (easier to diff in reviews).
    let mut intersection_keys: Vec<String> = deploy_by_user
        .keys()
        .filter(|k| source_by_user.contains_key(*k))
        .cloned()
        .collect();
    intersection_keys.sort();

    let mut exceptions = Vec::with_capacity(intersection_keys.len());
    for key in &intersection_keys {
        let (deploy_ordinal, deploy_row) = deploy_by_user
            .get(key)
            .cloned()
            .expect("intersection key is present in deploy map");
        let (source_ordinal, source_row) = source_by_user
            .get(key)
            .cloned()
            .expect("intersection key is present in source map");
        exceptions.push(SoDException {
            kind: "user_has_dev_and_deploy".into(),
            username: key.clone(),
            deploy_row,
            source_row,
            deploy_ordinal,
            source_ordinal,
        });
    }

    SoDReport {
        rule: "sod_dev_vs_deploy".into(),
        deploy_rows_considered: deploy_access.rows.len(),
        deploy_rows_skipped_unmatchable: deploy_skipped,
        source_rows_considered: source_access.rows.len(),
        source_rows_skipped_unmatchable: source_skipped,
        deploy_unique_users: deploy_by_user.len(),
        source_unique_users: source_by_user.len(),
        intersecting_users: exceptions.len(),
        exceptions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::csv::parse;

    #[test]
    fn flags_missing_approval_on_deployed_change() {
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             CHG0001,Normal,Closed,,,jane,2025-03-10T10:00:00Z\n\
             CHG0002,Normal,Closed,alice,2025-03-01T10:00:00Z,jane,2025-03-02T10:00:00Z\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].kind, "missing_approval");
        assert_eq!(report.exceptions[0].change_id, "CHG0001");
    }

    #[test]
    fn flags_approval_after_deployment() {
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             CHG0003,Normal,Closed,alice,2025-03-11T10:00:00Z,jane,2025-03-10T10:00:00Z\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].kind, "approval_after_deployment");
    }

    #[test]
    fn flags_approver_equal_to_implementer() {
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             CHG0004,Normal,Closed,Jane Doe,2025-03-01T10:00:00Z,jane doe,2025-03-02T10:00:00Z\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].kind, "approver_is_implementer");
    }

    #[test]
    fn single_change_can_produce_multiple_exception_rows() {
        // Same person approves and deploys, AND the approval is stamped after
        // the deployment. Both rules fire — we emit one exception row per
        // distinct symptom rather than conflate them.
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             CHG0005,Normal,Closed,alice,2025-03-03T10:00:00Z,alice,2025-03-02T10:00:00Z\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert_eq!(report.exceptions.len(), 2);
        let kinds: Vec<&str> = report.exceptions.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"approval_after_deployment"));
        assert!(kinds.contains(&"approver_is_implementer"));
    }

    #[test]
    fn standard_changes_skipped_not_counted_as_exceptions() {
        // Standard (pre-approved) changes don't require per-change approval.
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             CHG0010,Standard,Closed,,,jane,2025-03-02T10:00:00Z\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.changes_skipped_standard, 1);
    }

    #[test]
    fn cancelled_and_draft_changes_skipped_into_correct_buckets() {
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             CHG0011,Normal,Cancelled,,,,\n\
             CHG0012,Normal,Rejected,,,,\n\
             CHG0013,Normal,Draft,,,,\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert!(report.exceptions.is_empty());
        // Cancelled + Rejected are terminal — won't deploy.
        assert_eq!(report.changes_skipped_cancelled, 2);
        // Draft is in-flight — hasn't deployed yet.
        assert_eq!(report.changes_skipped_not_deployed, 1);
    }

    #[test]
    fn changes_without_any_deployment_evidence_skipped_not_exception() {
        // No deployed_at, no implementer, status "Assess" — this change
        // hasn't deployed yet. Not in scope for the rule, not an exception.
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             CHG0020,Normal,Assess,alice,2025-03-01T10:00:00Z,,\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.changes_skipped_not_deployed, 1);
    }

    #[test]
    fn rows_without_change_id_are_counted_but_not_evaluated() {
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             ,Normal,Closed,,,jane,2025-03-02T10:00:00Z\n\
             CHG0030,Normal,Closed,alice,2025-03-01T10:00:00Z,jane,2025-03-02T10:00:00Z\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.changes_skipped_no_id, 1);
    }

    #[test]
    fn unparseable_dates_count_skipped_not_exception() {
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             CHG0040,Normal,Closed,alice,not-a-date,jane,2025-03-02T10:00:00Z\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.changes_skipped_unparseable_dates, 1);
    }

    #[test]
    fn epoch_seconds_accepted_for_timestamps() {
        // 2025-03-01T10:00:00Z = 1740823200, 2025-03-02T10:00:00Z = 1740909600
        // 2025-03-03T10:00:00Z = 1740996000 (approval after deployment)
        let changes = parse(
            "change_id,type,status,approver,approved_at,implementer,deployed_at\n\
             CHG0050,Normal,Closed,alice,1740996000,jane,1740909600\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].kind, "approval_after_deployment");
    }

    #[test]
    fn header_variants_are_normalised() {
        // Spaces, hyphens, and case variants in headers all match.
        let changes = parse(
            "Change Number,Change-Type,State,Approved By,Approval Date,Implemented By,Actual End Date\n\
             CHG0060,Normal,Closed,alice,2025-03-01,jane,2025-03-02\n",
        )
        .unwrap();
        let report = run_change_approval_before_deployment(&changes);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.changes_considered, 1);
    }

    // ---- sod-dev-vs-deploy rule --------------------------------------------

    #[test]
    fn sod_flags_users_appearing_in_both_lists() {
        let deploy = parse(
            "username,role\n\
             alice,release-manager\n\
             bob,release-manager\n\
             carol,release-manager\n",
        )
        .unwrap();
        let source = parse(
            "username,permission\n\
             alice,write\n\
             dave,write\n\
             eve,write\n",
        )
        .unwrap();
        let report = run_sod_dev_vs_deploy(&deploy, &source);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].kind, "user_has_dev_and_deploy");
        assert_eq!(report.exceptions[0].username, "alice");
        assert_eq!(report.deploy_rows_considered, 3);
        assert_eq!(report.source_rows_considered, 3);
        assert_eq!(report.deploy_unique_users, 3);
        assert_eq!(report.source_unique_users, 3);
        assert_eq!(report.intersecting_users, 1);
    }

    #[test]
    fn sod_passes_on_disjoint_lists() {
        let deploy = parse(
            "user,role\n\
             alice,releaser\n\
             bob,releaser\n",
        )
        .unwrap();
        let source = parse(
            "user,permission\n\
             carol,write\n\
             dave,write\n",
        )
        .unwrap();
        let report = run_sod_dev_vs_deploy(&deploy, &source);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.intersecting_users, 0);
    }

    #[test]
    fn sod_match_is_case_and_whitespace_insensitive() {
        // Common real-world shape: deploy tool exports display names ("Alice
        // Example"), source host exports email-form usernames ("alice@…").
        // The match needs to be *at least* case-insensitive and
        // whitespace-tolerant. Beyond that — cross-form (display name vs
        // email vs SAM) matching would need firm-provided identity mapping;
        // out of scope for the rule. Same-form case differences must match.
        let deploy = parse(
            "username,role\n\
             Alice,release-manager\n\
             BOB,release-manager\n",
        )
        .unwrap();
        let source = parse(
            "username,permission\n\
             alice ,write\n\
             bob,write\n",
        )
        .unwrap();
        let report = run_sod_dev_vs_deploy(&deploy, &source);
        assert_eq!(report.exceptions.len(), 2);
        let names: Vec<&str> = report
            .exceptions
            .iter()
            .map(|e| e.username.as_str())
            .collect();
        assert!(names.contains(&"alice"));
        assert!(names.contains(&"bob"));
    }

    #[test]
    fn sod_counts_rows_with_missing_username_as_skipped() {
        let deploy = parse(
            "username,role\n\
             alice,releaser\n\
             ,releaser\n\
             bob,releaser\n",
        )
        .unwrap();
        let source = parse(
            "username,permission\n\
             alice,write\n\
             ,write\n",
        )
        .unwrap();
        let report = run_sod_dev_vs_deploy(&deploy, &source);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.deploy_rows_considered, 3);
        assert_eq!(report.deploy_rows_skipped_unmatchable, 1);
        assert_eq!(report.source_rows_considered, 2);
        assert_eq!(report.source_rows_skipped_unmatchable, 1);
        assert_eq!(report.deploy_unique_users, 2);
        assert_eq!(report.source_unique_users, 1);
    }

    #[test]
    fn sod_deduplicates_repeated_user_within_one_side() {
        // A user appearing on multiple deploy rows (e.g. member of several
        // release-manager groups) should count once in unique_users, and
        // emit exactly one exception if they're also on the source side.
        let deploy = parse(
            "username,group\n\
             alice,release-managers\n\
             alice,emergency-deployers\n\
             bob,release-managers\n",
        )
        .unwrap();
        let source = parse(
            "username,permission\n\
             alice,write\n",
        )
        .unwrap();
        let report = run_sod_dev_vs_deploy(&deploy, &source);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.deploy_unique_users, 2);
        // First ordinal wins for the exception's deploy_row.
        assert_eq!(report.exceptions[0].deploy_ordinal, 1);
    }

    #[test]
    fn sod_emits_exceptions_in_deterministic_order() {
        // HashMap iteration order isn't stable across runs; we sort the
        // intersection keys so exception output is stable.
        let deploy = parse(
            "username,role\n\
             zoe,releaser\n\
             alice,releaser\n\
             mike,releaser\n",
        )
        .unwrap();
        let source = parse(
            "username,permission\n\
             mike,write\n\
             zoe,write\n\
             alice,write\n",
        )
        .unwrap();
        let report = run_sod_dev_vs_deploy(&deploy, &source);
        assert_eq!(report.exceptions.len(), 3);
        let order: Vec<&str> = report
            .exceptions
            .iter()
            .map(|e| e.username.as_str())
            .collect();
        assert_eq!(order, vec!["alice", "mike", "zoe"]);
    }

    #[test]
    fn sod_header_variants_are_normalised() {
        // Deploy export calls it "User Principal Name", source export calls
        // it "login". Both canonicalise and match on the same key.
        let deploy = parse(
            "User Principal Name,Role\n\
             alice@example.com,release-manager\n",
        )
        .unwrap();
        let source = parse(
            "Login,Permission\n\
             alice@example.com,write\n",
        )
        .unwrap();
        let report = run_sod_dev_vs_deploy(&deploy, &source);
        assert_eq!(report.exceptions.len(), 1);
    }
}
