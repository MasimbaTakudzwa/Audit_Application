//! User-access-review rules.
//!
//! Three deterministic rules so far, all driven by an AD (or Entra) export:
//!   - **terminated-but-active** — reconcile the AD export against an HR
//!     leavers list; flag enabled accounts whose owner appears in the
//!     leavers list.
//!   - **dormant-accounts** — flag enabled accounts whose last logon is
//!     older than a configured threshold (default 90 days).
//!   - **orphan-accounts** — reconcile the AD export against an HR master
//!     list of current employees; flag enabled accounts whose owner does
//!     not appear in the master. Catches accounts left behind when
//!     employees left before HR recordkeeping was trustworthy, shared
//!     accounts that escaped the service-account naming convention, and
//!     (worst case) accounts never backed by a real employee at all.
//!
//! Intentional simplicity:
//!   - Each matcher is pure. It takes parsed tables plus its scalar
//!     parameters and returns a report. Blob I/O and `TestResult`
//!     persistence live in the command layer so this file has no
//!     test-harness wiring beyond `Table` construction.
//!   - Matching is by lower-cased email; the fallback logon-name match only
//!     kicks in when the AD row has no email column (rare but possible on
//!     script-hand-rolled exports).
//!   - The rules assume "enabled = true" unless an explicit `enabled`,
//!     `accountenabled`, or `status` column says otherwise. That's the safe
//!     default — a genuinely disabled row is a *negative* finding, so a
//!     false positive from a missing column is recoverable by the auditor.

use serde::Serialize;

use super::csv::{find_column, Row, Table};

const EMAIL_CANDIDATES: &[&str] = &[
    "email",
    "emailaddress",
    "mail",
    "userprincipalname",
    "upn",
    "workemail",
];

const LOGON_CANDIDATES: &[&str] = &["samaccountname", "logonname", "username", "accountname"];

const ENABLED_CANDIDATES: &[&str] = &["enabled", "accountenabled", "isenabled", "status"];

const LAST_LOGON_CANDIDATES: &[&str] = &[
    "lastlogontimestamp",
    "lastlogon",
    "lastlogondate",
    "lastsignindatetime",
    "lastsignin",
    "lastsignindate",
    "lastactivity",
];

/// Default dormancy threshold in days, if the caller does not override.
pub const DORMANT_THRESHOLD_DAYS_DEFAULT: u32 = 90;

#[derive(Debug, Clone, Serialize)]
pub struct Exception {
    pub kind: String,
    pub email: Option<String>,
    pub logon: Option<String>,
    pub ad_ordinal: usize,
    pub leaver_ordinal: usize,
    pub ad_row: Vec<String>,
    pub leaver_row: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub rule: String,
    pub ad_rows_considered: usize,
    pub leaver_rows_considered: usize,
    pub ad_rows_skipped_disabled: usize,
    pub ad_rows_skipped_unmatchable: usize,
    pub exceptions: Vec<Exception>,
}

pub fn run_terminated_but_active(ad: &Table, leavers: &Table) -> Report {
    let ad_email_col = find_column(ad, EMAIL_CANDIDATES);
    let ad_logon_col = find_column(ad, LOGON_CANDIDATES);
    let ad_enabled_col = find_column(ad, ENABLED_CANDIDATES);

    let leaver_email_col = find_column(leavers, EMAIL_CANDIDATES);
    let leaver_logon_col = find_column(leavers, LOGON_CANDIDATES);

    let mut leaver_by_email: std::collections::HashMap<String, &Row> =
        std::collections::HashMap::new();
    let mut leaver_by_logon: std::collections::HashMap<String, &Row> =
        std::collections::HashMap::new();
    for row in &leavers.rows {
        if let Some(col) = &leaver_email_col {
            if let Some(val) = row.values.get(col) {
                let key = normalise(val);
                if !key.is_empty() {
                    leaver_by_email.entry(key).or_insert(row);
                }
            }
        }
        if let Some(col) = &leaver_logon_col {
            if let Some(val) = row.values.get(col) {
                let key = normalise(val);
                if !key.is_empty() {
                    leaver_by_logon.entry(key).or_insert(row);
                }
            }
        }
    }

    let mut exceptions = Vec::new();
    let mut skipped_disabled = 0usize;
    let mut skipped_unmatchable = 0usize;

    for ad_row in &ad.rows {
        if !is_enabled(ad_row, ad_enabled_col.as_deref()) {
            skipped_disabled += 1;
            continue;
        }

        let ad_email = ad_email_col
            .as_ref()
            .and_then(|c| ad_row.values.get(c))
            .map(|s| normalise(s))
            .filter(|s| !s.is_empty());
        let ad_logon = ad_logon_col
            .as_ref()
            .and_then(|c| ad_row.values.get(c))
            .map(|s| normalise(s))
            .filter(|s| !s.is_empty());

        if ad_email.is_none() && ad_logon.is_none() {
            skipped_unmatchable += 1;
            continue;
        }

        let hit = ad_email
            .as_ref()
            .and_then(|e| leaver_by_email.get(e))
            .or_else(|| ad_logon.as_ref().and_then(|l| leaver_by_logon.get(l)));

        if let Some(leaver_row) = hit {
            exceptions.push(Exception {
                kind: "terminated_but_active".into(),
                email: ad_email.clone(),
                logon: ad_logon.clone(),
                ad_ordinal: ad_row.ordinal,
                leaver_ordinal: leaver_row.ordinal,
                ad_row: ad_row.raw_values.clone(),
                leaver_row: leaver_row.raw_values.clone(),
            });
        }
    }

    Report {
        rule: "terminated_but_active".into(),
        ad_rows_considered: ad.rows.len(),
        leaver_rows_considered: leavers.rows.len(),
        ad_rows_skipped_disabled: skipped_disabled,
        ad_rows_skipped_unmatchable: skipped_unmatchable,
        exceptions,
    }
}

fn normalise(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn is_enabled(row: &Row, column: Option<&str>) -> bool {
    let Some(col) = column else {
        return true;
    };
    let Some(raw) = row.values.get(col) else {
        return true;
    };
    let v = raw.trim().to_ascii_lowercase();
    !matches!(
        v.as_str(),
        "false" | "0" | "no" | "disabled" | "terminated" | "inactive" | "off"
    )
}

/// Report for the orphan-accounts rule. Shape mirrors the terminated-but-active
/// `Report` but the reconciliation runs the other way: an exception is an AD
/// row that has *no* match in the HR master list. `hr_rows_considered` is the
/// size of the authoritative master so the auditor can sanity-check that the
/// HR file covers who they expect.
#[derive(Debug, Clone, Serialize)]
pub struct OrphanReport {
    pub rule: String,
    pub ad_rows_considered: usize,
    pub ad_rows_skipped_disabled: usize,
    pub ad_rows_skipped_unmatchable: usize,
    pub hr_rows_considered: usize,
    pub exceptions: Vec<OrphanException>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrphanException {
    pub kind: String,
    pub email: Option<String>,
    pub logon: Option<String>,
    pub ad_ordinal: usize,
    pub ad_row: Vec<String>,
}

/// Flag enabled AD accounts whose email / logon does not appear anywhere in
/// the HR master list. The master list is expected to be the authoritative
/// roster of *current* employees — terminated people belong on the leavers
/// list consumed by `run_terminated_but_active`, not here.
///
/// Matching uses the same email-primary / logon-fallback logic as the
/// terminated rule; an AD row matches if *either* identifier appears in the
/// master. This errs toward fewer exceptions: a row that matches on logon but
/// has an unfamiliar email is treated as "known employee with a renamed
/// mailbox", not an orphan.
pub fn run_orphan_accounts(ad: &Table, hr_master: &Table) -> OrphanReport {
    let ad_email_col = find_column(ad, EMAIL_CANDIDATES);
    let ad_logon_col = find_column(ad, LOGON_CANDIDATES);
    let ad_enabled_col = find_column(ad, ENABLED_CANDIDATES);

    let hr_email_col = find_column(hr_master, EMAIL_CANDIDATES);
    let hr_logon_col = find_column(hr_master, LOGON_CANDIDATES);

    let mut hr_emails: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut hr_logons: std::collections::HashSet<String> = std::collections::HashSet::new();
    for row in &hr_master.rows {
        if let Some(col) = &hr_email_col {
            if let Some(val) = row.values.get(col) {
                let key = normalise(val);
                if !key.is_empty() {
                    hr_emails.insert(key);
                }
            }
        }
        if let Some(col) = &hr_logon_col {
            if let Some(val) = row.values.get(col) {
                let key = normalise(val);
                if !key.is_empty() {
                    hr_logons.insert(key);
                }
            }
        }
    }

    let mut exceptions = Vec::new();
    let mut skipped_disabled = 0usize;
    let mut skipped_unmatchable = 0usize;

    for ad_row in &ad.rows {
        if !is_enabled(ad_row, ad_enabled_col.as_deref()) {
            skipped_disabled += 1;
            continue;
        }

        let ad_email = ad_email_col
            .as_ref()
            .and_then(|c| ad_row.values.get(c))
            .map(|s| normalise(s))
            .filter(|s| !s.is_empty());
        let ad_logon = ad_logon_col
            .as_ref()
            .and_then(|c| ad_row.values.get(c))
            .map(|s| normalise(s))
            .filter(|s| !s.is_empty());

        if ad_email.is_none() && ad_logon.is_none() {
            skipped_unmatchable += 1;
            continue;
        }

        let email_hit = ad_email
            .as_ref()
            .map(|e| hr_emails.contains(e))
            .unwrap_or(false);
        let logon_hit = ad_logon
            .as_ref()
            .map(|l| hr_logons.contains(l))
            .unwrap_or(false);

        if !(email_hit || logon_hit) {
            exceptions.push(OrphanException {
                kind: "orphan_account".into(),
                email: ad_email,
                logon: ad_logon,
                ad_ordinal: ad_row.ordinal,
                ad_row: ad_row.raw_values.clone(),
            });
        }
    }

    OrphanReport {
        rule: "orphan_accounts".into(),
        ad_rows_considered: ad.rows.len(),
        ad_rows_skipped_disabled: skipped_disabled,
        ad_rows_skipped_unmatchable: skipped_unmatchable,
        hr_rows_considered: hr_master.rows.len(),
        exceptions,
    }
}

/// Report for the dormant-accounts rule. Structurally distinct from `Report`
/// because dormant has no HR-leavers input and carries a threshold.
#[derive(Debug, Clone, Serialize)]
pub struct DormantReport {
    pub rule: String,
    pub ad_rows_considered: usize,
    pub ad_rows_skipped_disabled: usize,
    pub ad_rows_skipped_no_last_logon: usize,
    pub ad_rows_skipped_unparseable: usize,
    pub threshold_days: u32,
    pub as_of_secs: i64,
    pub exceptions: Vec<DormantException>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DormantException {
    pub kind: String,
    pub email: Option<String>,
    pub logon: Option<String>,
    pub ad_ordinal: usize,
    pub last_logon_secs: i64,
    pub days_since_last_logon: u32,
    pub ad_row: Vec<String>,
}

pub fn run_dormant_accounts(ad: &Table, as_of_secs: i64, threshold_days: u32) -> DormantReport {
    let ad_email_col = find_column(ad, EMAIL_CANDIDATES);
    let ad_logon_col = find_column(ad, LOGON_CANDIDATES);
    let ad_enabled_col = find_column(ad, ENABLED_CANDIDATES);
    let ad_last_logon_col = find_column(ad, LAST_LOGON_CANDIDATES);

    let threshold_secs = threshold_days as i64 * 86_400;

    let mut exceptions = Vec::new();
    let mut skipped_disabled = 0usize;
    let mut skipped_no_last_logon = 0usize;
    let mut skipped_unparseable = 0usize;

    for ad_row in &ad.rows {
        if !is_enabled(ad_row, ad_enabled_col.as_deref()) {
            skipped_disabled += 1;
            continue;
        }

        let raw_last_logon = ad_last_logon_col
            .as_ref()
            .and_then(|c| ad_row.values.get(c))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());

        let Some(raw) = raw_last_logon else {
            skipped_no_last_logon += 1;
            continue;
        };

        let Some(last_logon_secs) = parse_last_logon(raw) else {
            skipped_unparseable += 1;
            continue;
        };

        if last_logon_secs <= 0 {
            // AD reports lastLogonTimestamp = 0 for accounts that have never
            // signed in. That's dormancy by any reasonable reading.
            let ad_email = ad_email_col
                .as_ref()
                .and_then(|c| ad_row.values.get(c))
                .map(|s| normalise(s))
                .filter(|s| !s.is_empty());
            let ad_logon = ad_logon_col
                .as_ref()
                .and_then(|c| ad_row.values.get(c))
                .map(|s| normalise(s))
                .filter(|s| !s.is_empty());
            exceptions.push(DormantException {
                kind: "dormant_never_signed_in".into(),
                email: ad_email,
                logon: ad_logon,
                ad_ordinal: ad_row.ordinal,
                last_logon_secs: 0,
                days_since_last_logon: u32::MAX,
                ad_row: ad_row.raw_values.clone(),
            });
            continue;
        }

        let delta = as_of_secs - last_logon_secs;
        if delta >= threshold_secs {
            let ad_email = ad_email_col
                .as_ref()
                .and_then(|c| ad_row.values.get(c))
                .map(|s| normalise(s))
                .filter(|s| !s.is_empty());
            let ad_logon = ad_logon_col
                .as_ref()
                .and_then(|c| ad_row.values.get(c))
                .map(|s| normalise(s))
                .filter(|s| !s.is_empty());
            let days = (delta / 86_400).clamp(0, u32::MAX as i64) as u32;
            exceptions.push(DormantException {
                kind: "dormant_account".into(),
                email: ad_email,
                logon: ad_logon,
                ad_ordinal: ad_row.ordinal,
                last_logon_secs,
                days_since_last_logon: days,
                ad_row: ad_row.raw_values.clone(),
            });
        }
    }

    DormantReport {
        rule: "dormant_accounts".into(),
        ad_rows_considered: ad.rows.len(),
        ad_rows_skipped_disabled: skipped_disabled,
        ad_rows_skipped_no_last_logon: skipped_no_last_logon,
        ad_rows_skipped_unparseable: skipped_unparseable,
        threshold_days,
        as_of_secs,
        exceptions,
    }
}

/// Default remediation window for the periodic-recertification rule, in days.
/// If the caller does not override, an exception whose review date is older
/// than this and whose `remediation_status` still reads open is flagged as
/// "unremediated". 90 days mirrors the dormancy default — most firms we've
/// seen either operate on the same cadence for both, or run recertification
/// quarterly and give remediation a full follow-up cycle, which is 90 days in
/// both cases.
pub const REMEDIATION_WINDOW_DAYS_DEFAULT: u32 = 90;

const REVIEW_DATE_CANDIDATES: &[&str] = &[
    "review_date",
    "reviewdate",
    "reviewed_at",
    "reviewedat",
    "reviewed_on",
    "reviewedon",
    "certification_date",
    "certificationdate",
    "certified_at",
    "certifiedat",
    "date_reviewed",
    "datereviewed",
];

/// Explicit "was an exception raised on this review row" columns. Checked
/// before the decision column so a dedicated boolean wins over a narrative
/// decision label.
const EXCEPTION_RAISED_CANDIDATES: &[&str] = &[
    "exception_raised",
    "exceptionraised",
    "has_exception",
    "hasexception",
    "exception",
    "is_exception",
    "isexception",
];

/// Reviewer-decision columns, read as a fallback exception signal when the
/// review log has no explicit `exception_raised` column. Values matching
/// `EXCEPTION_DECISION_VALUES` are treated as "exception raised".
const DECISION_CANDIDATES: &[&str] = &[
    "decision",
    "reviewer_decision",
    "reviewerdecision",
    "review_decision",
    "reviewdecision",
    "disposition",
    "outcome",
];

const REMEDIATION_STATUS_CANDIDATES: &[&str] = &[
    "remediation_status",
    "remediationstatus",
    "remediation",
    "remediation_state",
    "remediationstate",
    "remediation_outcome",
    "remediationoutcome",
    "disposition_status",
    "dispositionstatus",
];

/// Report for the periodic-recertification rule.
///
/// The rule produces two distinct exception kinds in one pass:
///   - `unreviewed_account` — an enabled AD row whose identity does not appear
///     in the access-review log. The population being tested is the *review
///     log's coverage*, not the AD export; an unreviewed account is a
///     completeness gap in the review, not a finding against the user.
///   - `unremediated_exception` — a review-log row where the reviewer raised
///     an exception (via an `exception_raised` column or an `exception`-type
///     decision), the remediation status is still open, and (if a review date
///     is present) the row is older than the configured remediation window.
///
/// The remediation check is opt-in on the data: it runs only when the review
/// log carries both an exception-signal column *and* a remediation-status
/// column. A log that has neither is still tested for completeness, and the
/// report records `remediation_check_applied = false` so the auditor can see
/// that the second leg was skipped by absent columns, not by zero findings.
#[derive(Debug, Clone, Serialize)]
pub struct RecertificationReport {
    pub rule: String,
    pub ad_rows_considered: usize,
    pub ad_rows_skipped_disabled: usize,
    pub ad_rows_skipped_unmatchable: usize,
    pub review_rows_considered: usize,
    pub review_rows_skipped_unmatchable: usize,
    pub unreviewed_count: usize,
    pub unremediated_count: usize,
    pub remediation_check_applied: bool,
    pub remediation_window_days: u32,
    pub as_of_secs: i64,
    pub exceptions: Vec<RecertificationException>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecertificationException {
    pub kind: String,
    pub email: Option<String>,
    pub logon: Option<String>,
    /// Set for `unreviewed_account` exceptions; the 1-based ordinal of the AD
    /// row that was not covered by the review log.
    pub ad_ordinal: Option<usize>,
    /// Set for `unremediated_exception`; the 1-based ordinal of the review
    /// row that carries the unresolved exception.
    pub review_ordinal: Option<usize>,
    /// Set when the review row parses a valid `review_date`. `None` means
    /// either no review-date column is present or the value was not
    /// parseable; both cases flag the row conservatively (see `run_periodic_recertification`).
    pub days_since_review: Option<u32>,
    /// Raw AD row for `unreviewed_account`; empty otherwise.
    pub ad_row: Vec<String>,
    /// Raw review-log row for `unremediated_exception`; empty otherwise.
    pub review_row: Vec<String>,
}

/// UAR periodic-recertification. Reconciles a dated access-review log against
/// the enabled AD population at the review date, then checks whether any
/// exceptions raised during the review have remained open past the remediation
/// window.
///
/// Inputs:
///   - `review_log` — rows of the access-review report. One row per user
///     reviewed (or per exception raised, depending on the firm's format).
///     Identity columns follow the same EMAIL/LOGON candidates as AD.
///   - `ad` — AD (or application user) export as at the review date. Enabled
///     rows are the authoritative population.
///
/// Reconciliation is email-primary with a logon fallback, matching the rest
/// of the UAR family. AD rows whose identity cannot be extracted at all are
/// counted in `ad_rows_skipped_unmatchable` and excluded from the
/// completeness check; the same applies to review rows.
///
/// The remediation check activates when at least one exception-signal column
/// (`exception_raised` / `has_exception` / or a `decision` column) is present
/// *and* a `remediation_status` column is present. The signal column is read
/// in priority order: an explicit `exception_raised` column wins over a
/// `decision` column carrying "exception" / "revoked" / "denied" / etc., so
/// firms whose reviewers record a conflict between the two see the explicit
/// signal used.
pub fn run_periodic_recertification(
    review_log: &Table,
    ad: &Table,
    as_of_secs: i64,
    remediation_window_days: u32,
) -> RecertificationReport {
    let ad_email_col = find_column(ad, EMAIL_CANDIDATES);
    let ad_logon_col = find_column(ad, LOGON_CANDIDATES);
    let ad_enabled_col = find_column(ad, ENABLED_CANDIDATES);

    let review_email_col = find_column(review_log, EMAIL_CANDIDATES);
    let review_logon_col = find_column(review_log, LOGON_CANDIDATES);
    let review_date_col = find_column(review_log, REVIEW_DATE_CANDIDATES);
    let exception_raised_col = find_column(review_log, EXCEPTION_RAISED_CANDIDATES);
    let decision_col = find_column(review_log, DECISION_CANDIDATES);
    let remediation_status_col = find_column(review_log, REMEDIATION_STATUS_CANDIDATES);

    let remediation_check_applied = (exception_raised_col.is_some()
        || decision_col.is_some())
        && remediation_status_col.is_some();

    // Build sets of reviewed identities for the completeness check. Both an
    // email hit and a logon hit count as "this user was reviewed", mirroring
    // the orphan-accounts convention.
    let mut reviewed_emails: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut reviewed_logons: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut review_rows_skipped_unmatchable = 0usize;
    for row in &review_log.rows {
        let email = review_email_col
            .as_ref()
            .and_then(|c| row.values.get(c))
            .map(|s| normalise(s))
            .filter(|s| !s.is_empty());
        let logon = review_logon_col
            .as_ref()
            .and_then(|c| row.values.get(c))
            .map(|s| normalise(s))
            .filter(|s| !s.is_empty());
        if email.is_none() && logon.is_none() {
            review_rows_skipped_unmatchable += 1;
            continue;
        }
        if let Some(e) = email {
            reviewed_emails.insert(e);
        }
        if let Some(l) = logon {
            reviewed_logons.insert(l);
        }
    }

    let mut unreviewed: Vec<RecertificationException> = Vec::new();
    let mut ad_rows_skipped_disabled = 0usize;
    let mut ad_rows_skipped_unmatchable = 0usize;

    for ad_row in &ad.rows {
        if !is_enabled(ad_row, ad_enabled_col.as_deref()) {
            ad_rows_skipped_disabled += 1;
            continue;
        }

        let ad_email = ad_email_col
            .as_ref()
            .and_then(|c| ad_row.values.get(c))
            .map(|s| normalise(s))
            .filter(|s| !s.is_empty());
        let ad_logon = ad_logon_col
            .as_ref()
            .and_then(|c| ad_row.values.get(c))
            .map(|s| normalise(s))
            .filter(|s| !s.is_empty());

        if ad_email.is_none() && ad_logon.is_none() {
            ad_rows_skipped_unmatchable += 1;
            continue;
        }

        let email_hit = ad_email
            .as_ref()
            .map(|e| reviewed_emails.contains(e))
            .unwrap_or(false);
        let logon_hit = ad_logon
            .as_ref()
            .map(|l| reviewed_logons.contains(l))
            .unwrap_or(false);

        if !(email_hit || logon_hit) {
            unreviewed.push(RecertificationException {
                kind: "unreviewed_account".into(),
                email: ad_email,
                logon: ad_logon,
                ad_ordinal: Some(ad_row.ordinal),
                review_ordinal: None,
                days_since_review: None,
                ad_row: ad_row.raw_values.clone(),
                review_row: Vec::new(),
            });
        }
    }

    let mut unremediated: Vec<RecertificationException> = Vec::new();
    if remediation_check_applied {
        let threshold_secs = remediation_window_days as i64 * 86_400;

        for row in &review_log.rows {
            let exception_raised = if let Some(col) = exception_raised_col.as_ref() {
                // Dedicated boolean column: read only this. An explicit "no"
                // wins over a decision column that might disagree.
                row.values
                    .get(col)
                    .map(|v| is_exception_raised_flag(v))
                    .unwrap_or(false)
            } else if let Some(col) = decision_col.as_ref() {
                row.values
                    .get(col)
                    .map(|v| is_exception_decision(v))
                    .unwrap_or(false)
            } else {
                false
            };
            if !exception_raised {
                continue;
            }

            // Must not be remediated. Absent / empty / unrecognised
            // remediation_status counts as "still open" — more conservative
            // than the firm's schema implies, but deliberately so: a value the
            // rule doesn't recognise shouldn't silently close an exception.
            let remediation_open = remediation_status_col
                .as_ref()
                .and_then(|c| row.values.get(c))
                .map(|v| !is_remediated(v))
                .unwrap_or(true);
            if !remediation_open {
                continue;
            }

            // Age gate. If a review_date column exists and parses: apply the
            // window. If the column exists but the value is blank/unparseable:
            // flag conservatively (can't verify grace). If no column at all:
            // flag conservatively (no grace to apply).
            let days_since = review_date_col
                .as_ref()
                .and_then(|c| row.values.get(c))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .and_then(|s| parse_last_logon(s))
                .map(|review_secs| {
                    let delta = (as_of_secs - review_secs).max(0);
                    (delta / 86_400).clamp(0, u32::MAX as i64) as u32
                });
            let is_stale = match days_since {
                Some(d) => (d as i64) * 86_400 >= threshold_secs,
                None => true,
            };
            if !is_stale {
                continue;
            }

            let email = review_email_col
                .as_ref()
                .and_then(|c| row.values.get(c))
                .map(|s| normalise(s))
                .filter(|s| !s.is_empty());
            let logon = review_logon_col
                .as_ref()
                .and_then(|c| row.values.get(c))
                .map(|s| normalise(s))
                .filter(|s| !s.is_empty());

            unremediated.push(RecertificationException {
                kind: "unremediated_exception".into(),
                email,
                logon,
                ad_ordinal: None,
                review_ordinal: Some(row.ordinal),
                days_since_review: days_since,
                ad_row: Vec::new(),
                review_row: row.raw_values.clone(),
            });
        }
    }

    let unreviewed_count = unreviewed.len();
    let unremediated_count = unremediated.len();

    let mut exceptions = Vec::with_capacity(unreviewed_count + unremediated_count);
    exceptions.extend(unreviewed);
    exceptions.extend(unremediated);

    RecertificationReport {
        rule: "periodic_recertification".into(),
        ad_rows_considered: ad.rows.len(),
        ad_rows_skipped_disabled,
        ad_rows_skipped_unmatchable,
        review_rows_considered: review_log.rows.len(),
        review_rows_skipped_unmatchable,
        unreviewed_count,
        unremediated_count,
        remediation_check_applied,
        remediation_window_days,
        as_of_secs,
        exceptions,
    }
}

fn is_exception_raised_flag(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "yes" | "y" | "true" | "1" | "raised" | "exception" | "flagged"
    )
}

fn is_exception_decision(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "exception"
            | "exceptions"
            | "revoke"
            | "revoked"
            | "remove"
            | "removed"
            | "reject"
            | "rejected"
            | "deny"
            | "denied"
            | "fail"
            | "failed"
            | "non-compliant"
            | "noncompliant"
            | "not_approved"
            | "notapproved"
            | "not approved"
    )
}

fn is_remediated(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "closed"
            | "close"
            | "complete"
            | "completed"
            | "done"
            | "remediated"
            | "resolved"
            | "accepted"
            | "risk_accepted"
            | "riskaccepted"
            | "risk-accepted"
            | "approved"
            | "passed"
            | "fixed"
            | "cleared"
    )
}

/// Parse a last-logon value into Unix epoch seconds. Tries, in order:
///   - the marker strings "never" / "none" / "null" / "0" → returns Some(0)
///     so callers can tell "never signed in" apart from "unparseable"
///   - Windows FILETIME (17- to 19-digit integer — 100-nanosecond intervals
///     since 1601-01-01 UTC)
///   - Unix epoch seconds (9- to 10-digit integer)
///   - Unix epoch milliseconds (12- to 13-digit integer)
///   - ISO 8601 date `YYYY-MM-DD`
///   - ISO 8601 datetime `YYYY-MM-DDTHH:MM:SS[.fff][Z]` or with a space separator
fn parse_last_logon(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "never" | "none" | "null" | "0") {
        return Some(0);
    }

    if s.chars().all(|c| c.is_ascii_digit()) {
        let n: i128 = s.parse().ok()?;
        return match s.len() {
            17..=19 => {
                // Windows FILETIME: 100-ns since 1601-01-01 UTC.
                // Unix epoch = (filetime / 10_000_000) - 11_644_473_600.
                let secs = (n / 10_000_000) - 11_644_473_600;
                i64::try_from(secs).ok()
            }
            9..=10 => i64::try_from(n).ok(),
            12..=13 => i64::try_from(n / 1000).ok(),
            _ => None,
        };
    }

    parse_iso_8601(s)
}

/// Minimal ISO 8601 parser covering date and date-time forms we actually see
/// in AD and Entra exports. Time zone is assumed to be UTC — AD exports are
/// already UTC, and any small TZ offset is immaterial against a 90-day
/// dormancy threshold.
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
        let ss: i64 = time_bits
            .get(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if !(0..24).contains(&hh) || !(0..60).contains(&mm) || !(0..60).contains(&ss) {
            return None;
        }
        secs += hh * 3600 + mm * 60 + ss;
    }

    Some(secs)
}

/// Howard Hinnant's `days_from_civil` — proleptic Gregorian calendar days
/// since 1970-01-01 for any (year, month, day). Reference:
/// https://howardhinnant.github.io/date_algorithms.html#days_from_civil
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * if m > 2 { m - 3 } else { m + 9 } + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era as i64 * 146_097 + doe as i64 - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::csv::parse;

    #[test]
    fn flags_terminated_user_still_enabled_in_ad() {
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             bob,bob@acme.com,TRUE\n\
             carol,carol@acme.com,FALSE\n",
        )
        .unwrap();
        let leavers = parse("employee_id,email\n1,alice@acme.com\n2,carol@acme.com\n").unwrap();

        let report = run_terminated_but_active(&ad, &leavers);
        assert_eq!(report.exceptions.len(), 1, "alice is a leaver with enabled AD");
        assert_eq!(
            report.exceptions[0].email.as_deref(),
            Some("alice@acme.com")
        );
        assert_eq!(report.ad_rows_skipped_disabled, 1, "carol is already disabled");
    }

    #[test]
    fn logon_name_fallback_when_email_absent_in_ad() {
        let ad = parse("sAMAccountName,enabled\nalice,TRUE\nbob,TRUE\n").unwrap();
        let leavers = parse("employee_id,logon_name\n1,alice\n").unwrap();
        let report = run_terminated_but_active(&ad, &leavers);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].logon.as_deref(), Some("alice"));
    }

    #[test]
    fn empty_when_no_shared_users() {
        let ad = parse("email,enabled\na@x.com,TRUE\n").unwrap();
        let leavers = parse("email\nz@x.com\n").unwrap();
        let report = run_terminated_but_active(&ad, &leavers);
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn missing_enabled_column_treats_all_ad_rows_as_enabled() {
        let ad = parse("email\nalice@x.com\nbob@x.com\n").unwrap();
        let leavers = parse("email\nalice@x.com\n").unwrap();
        let report = run_terminated_but_active(&ad, &leavers);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.ad_rows_skipped_disabled, 0);
    }

    // Pick a fixed `as_of` so the tests are independent of wall clock. Any
    // value after the test dates is fine — use 2025-06-01 00:00:00 UTC.
    const AS_OF_2025_06_01: i64 = 1_748_736_000;

    #[test]
    fn dormant_flags_accounts_past_threshold() {
        // alice last logged on 2024-11-15 (≈198 days before as_of); flagged.
        // bob last logged on 2025-05-01 (≈31 days before); not flagged.
        // carol is disabled; skipped.
        let ad = parse(
            "sAMAccountName,email,enabled,lastLogonDate\n\
             alice,alice@x.com,TRUE,2024-11-15\n\
             bob,bob@x.com,TRUE,2025-05-01\n\
             carol,carol@x.com,FALSE,2020-01-01\n",
        )
        .unwrap();
        let report = run_dormant_accounts(&ad, AS_OF_2025_06_01, 90);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].email.as_deref(), Some("alice@x.com"));
        assert_eq!(report.exceptions[0].kind, "dormant_account");
        assert!(report.exceptions[0].days_since_last_logon >= 90);
        assert_eq!(report.ad_rows_skipped_disabled, 1);
    }

    #[test]
    fn dormant_accepts_windows_filetime_and_never_sentinel() {
        // FILETIME for 2024-01-01 00:00:00 UTC is (1704067200 + 11644473600) * 10_000_000.
        let ft = (1_704_067_200_i64 + 11_644_473_600) * 10_000_000;
        let ad = parse(&format!(
            "sAMAccountName,enabled,lastLogonTimestamp\n\
             alice,TRUE,{ft}\n\
             bob,TRUE,0\n\
             carol,TRUE,Never\n"
        ))
        .unwrap();
        let report = run_dormant_accounts(&ad, AS_OF_2025_06_01, 90);
        // alice dormant by threshold, bob dormant by never-signed-in, carol likewise.
        assert_eq!(report.exceptions.len(), 3);
        let alice = report
            .exceptions
            .iter()
            .find(|e| e.logon.as_deref() == Some("alice"))
            .unwrap();
        assert_eq!(alice.kind, "dormant_account");
        let bob = report
            .exceptions
            .iter()
            .find(|e| e.logon.as_deref() == Some("bob"))
            .unwrap();
        assert_eq!(bob.kind, "dormant_never_signed_in");
    }

    #[test]
    fn dormant_skips_rows_without_last_logon_column_value() {
        let ad = parse(
            "sAMAccountName,enabled,lastLogonDate\n\
             alice,TRUE,\n\
             bob,TRUE,2020-01-01\n",
        )
        .unwrap();
        let report = run_dormant_accounts(&ad, AS_OF_2025_06_01, 90);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].logon.as_deref(), Some("bob"));
        assert_eq!(report.ad_rows_skipped_no_last_logon, 1);
    }

    #[test]
    fn dormant_counts_unparseable_rows() {
        let ad = parse(
            "sAMAccountName,enabled,lastLogonDate\n\
             alice,TRUE,not-a-date\n\
             bob,TRUE,2020-01-01\n",
        )
        .unwrap();
        let report = run_dormant_accounts(&ad, AS_OF_2025_06_01, 90);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.ad_rows_skipped_unparseable, 1);
    }

    #[test]
    fn dormant_no_last_logon_column_skips_every_row() {
        // If the export has no last-logon column at all, we can't evaluate
        // dormancy — skip everything rather than false-flag.
        let ad = parse("sAMAccountName,enabled\nalice,TRUE\nbob,TRUE\n").unwrap();
        let report = run_dormant_accounts(&ad, AS_OF_2025_06_01, 90);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.ad_rows_skipped_no_last_logon, 2);
    }

    #[test]
    fn parse_iso_date_and_datetime() {
        // 1970-01-01 is the epoch.
        assert_eq!(parse_last_logon("1970-01-01"), Some(0));
        assert_eq!(parse_last_logon("1970-01-01T00:00:00Z"), Some(0));
        // 2024-01-01 00:00:00 UTC = 1704067200.
        assert_eq!(parse_last_logon("2024-01-01"), Some(1_704_067_200));
        assert_eq!(parse_last_logon("2024-01-01T00:00:00"), Some(1_704_067_200));
        assert_eq!(
            parse_last_logon("2024-01-01 00:00:00"),
            Some(1_704_067_200)
        );
        assert_eq!(
            parse_last_logon("2024-01-01T12:30:45Z"),
            Some(1_704_067_200 + 12 * 3600 + 30 * 60 + 45)
        );
        assert_eq!(parse_last_logon("not-a-date"), None);
    }

    #[test]
    fn parse_filetime_and_epoch_forms() {
        // FILETIME for 1970-01-01 00:00:00 UTC is 116444736000000000.
        assert_eq!(parse_last_logon("116444736000000000"), Some(0));
        // Epoch seconds, 10 digits.
        assert_eq!(parse_last_logon("1704067200"), Some(1_704_067_200));
        // Epoch milliseconds, 13 digits.
        assert_eq!(parse_last_logon("1704067200000"), Some(1_704_067_200));
        // Short integer → unparseable for our purposes.
        assert_eq!(parse_last_logon("12345"), None);
    }

    #[test]
    fn orphan_flags_ad_accounts_missing_from_hr_master() {
        // alice and carol are on the master roster; bob is an orphan.
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             bob,bob@acme.com,TRUE\n\
             carol,carol@acme.com,TRUE\n",
        )
        .unwrap();
        let hr_master =
            parse("employee_id,email\n1,alice@acme.com\n2,carol@acme.com\n").unwrap();

        let report = run_orphan_accounts(&ad, &hr_master);

        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(
            report.exceptions[0].email.as_deref(),
            Some("bob@acme.com")
        );
        assert_eq!(report.exceptions[0].kind, "orphan_account");
        assert_eq!(report.ad_rows_considered, 3);
        assert_eq!(report.hr_rows_considered, 2);
    }

    #[test]
    fn orphan_skips_disabled_rows_so_they_are_not_exceptions() {
        // A disabled AD row with no HR match is *not* an orphan — it's just a
        // disabled stale account, which a dormant/cleanup review handles.
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             ghost,ghost@acme.com,FALSE\n",
        )
        .unwrap();
        let hr_master = parse("employee_id,email\n1,alice@acme.com\n").unwrap();

        let report = run_orphan_accounts(&ad, &hr_master);

        assert!(report.exceptions.is_empty());
        assert_eq!(report.ad_rows_skipped_disabled, 1);
    }

    #[test]
    fn orphan_logon_match_alone_is_enough_when_email_column_missing_in_hr() {
        // AD has an email column, HR master is logon-only. Matching still
        // works via the logon fallback.
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             bob,bob@acme.com,TRUE\n",
        )
        .unwrap();
        let hr_master = parse("employee_id,logon_name\n1,alice\n").unwrap();

        let report = run_orphan_accounts(&ad, &hr_master);

        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].logon.as_deref(), Some("bob"));
    }

    #[test]
    fn orphan_skips_rows_with_neither_email_nor_logon() {
        // A row that cannot be canonicalised at all can't be checked for
        // membership; it's counted in ad_rows_skipped_unmatchable, not
        // exceptions.
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             ,,TRUE\n",
        )
        .unwrap();
        let hr_master = parse("email\nalice@acme.com\n").unwrap();

        let report = run_orphan_accounts(&ad, &hr_master);

        assert!(report.exceptions.is_empty());
        assert_eq!(report.ad_rows_skipped_unmatchable, 1);
    }

    #[test]
    fn orphan_empty_when_every_ad_row_is_in_hr_master() {
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             bob,bob@acme.com,TRUE\n",
        )
        .unwrap();
        let hr_master = parse(
            "employee_id,email\n\
             1,alice@acme.com\n\
             2,bob@acme.com\n\
             3,carol@acme.com\n",
        )
        .unwrap();

        let report = run_orphan_accounts(&ad, &hr_master);

        // HR master being a superset is fine — the rule only checks the AD
        // side for absence.
        assert!(report.exceptions.is_empty());
        assert_eq!(report.hr_rows_considered, 3);
    }

    #[test]
    fn orphan_case_and_whitespace_insensitive_matching() {
        // Email values are trimmed and lower-cased on both sides before
        // membership check; mixed-case HR data should still match clean AD
        // data and vice versa.
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,Alice@Acme.com,TRUE\n\
             bob,  bob@acme.com  ,TRUE\n",
        )
        .unwrap();
        let hr_master = parse(
            "employee_id,email\n\
             1,ALICE@ACME.COM\n\
             2,bob@acme.com\n",
        )
        .unwrap();

        let report = run_orphan_accounts(&ad, &hr_master);

        assert!(report.exceptions.is_empty());
    }

    // ---- Periodic recertification (UAM-T-002) ----

    #[test]
    fn recert_flags_ad_accounts_missing_from_review_log() {
        // alice + bob are in the review log; carol is enabled in AD but not
        // reviewed → one unreviewed_account exception.
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             bob,bob@acme.com,TRUE\n\
             carol,carol@acme.com,TRUE\n",
        )
        .unwrap();
        let review = parse(
            "email\n\
             alice@acme.com\n\
             bob@acme.com\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unreviewed_count, 1);
        assert_eq!(report.unremediated_count, 0);
        assert_eq!(report.ad_rows_considered, 3);
        assert_eq!(report.review_rows_considered, 2);
        let sample = &report.exceptions[0];
        assert_eq!(sample.kind, "unreviewed_account");
        assert_eq!(sample.email.as_deref(), Some("carol@acme.com"));
        assert_eq!(report.remediation_check_applied, false);
    }

    #[test]
    fn recert_ignores_disabled_ad_rows_in_completeness_check() {
        // A disabled AD account that's not in the review log is not an
        // exception — same convention as orphan-accounts.
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             ghost,ghost@acme.com,FALSE\n",
        )
        .unwrap();
        let review = parse("email\nalice@acme.com\n").unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unreviewed_count, 0);
        assert_eq!(report.ad_rows_skipped_disabled, 1);
        assert_eq!(report.ad_rows_considered, 2);
    }

    #[test]
    fn recert_passes_when_every_enabled_ad_row_is_reviewed_and_no_exceptions() {
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             bob,bob@acme.com,TRUE\n",
        )
        .unwrap();
        let review = parse(
            "email,decision,remediation_status\n\
             alice@acme.com,approved,closed\n\
             bob@acme.com,approved,closed\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unreviewed_count, 0);
        assert_eq!(report.unremediated_count, 0);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.remediation_check_applied, true);
    }

    #[test]
    fn recert_flags_unremediated_exception_past_the_window() {
        // bob's review happened 2024-11-01 (>= 90 days before 2025-06-01),
        // the decision is "exception", and remediation_status is still open →
        // one unremediated_exception.
        let ad = parse(
            "email,enabled\n\
             alice@acme.com,TRUE\n\
             bob@acme.com,TRUE\n",
        )
        .unwrap();
        let review = parse(
            "email,review_date,decision,remediation_status\n\
             alice@acme.com,2024-11-01,approved,closed\n\
             bob@acme.com,2024-11-01,exception,open\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unreviewed_count, 0);
        assert_eq!(report.unremediated_count, 1);
        assert_eq!(report.remediation_check_applied, true);
        let sample = &report.exceptions[0];
        assert_eq!(sample.kind, "unremediated_exception");
        assert_eq!(sample.email.as_deref(), Some("bob@acme.com"));
        assert!(sample.days_since_review.unwrap() >= 90);
    }

    #[test]
    fn recert_does_not_flag_recent_exception_inside_window() {
        // bob's review happened 2025-05-01 (~31 days before as_of); still
        // inside the 90-day remediation window.
        let ad = parse("email,enabled\nbob@acme.com,TRUE\n").unwrap();
        let review = parse(
            "email,review_date,decision,remediation_status\n\
             bob@acme.com,2025-05-01,exception,open\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unremediated_count, 0);
    }

    #[test]
    fn recert_flags_unremediated_when_review_date_is_absent_on_row() {
        // An exception row with no review_date should flag conservatively
        // rather than silently pass.
        let ad = parse("email,enabled\nbob@acme.com,TRUE\n").unwrap();
        let review = parse(
            "email,review_date,decision,remediation_status\n\
             bob@acme.com,,exception,open\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unremediated_count, 1);
        assert!(report.exceptions[0].days_since_review.is_none());
    }

    #[test]
    fn recert_flags_unremediated_when_review_date_column_is_missing() {
        // No review_date column at all: same conservative behaviour.
        let ad = parse("email,enabled\nbob@acme.com,TRUE\n").unwrap();
        let review = parse(
            "email,decision,remediation_status\n\
             bob@acme.com,exception,open\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unremediated_count, 1);
    }

    #[test]
    fn recert_flags_unremediated_when_review_date_unparseable() {
        let ad = parse("email,enabled\nbob@acme.com,TRUE\n").unwrap();
        let review = parse(
            "email,review_date,decision,remediation_status\n\
             bob@acme.com,not-a-date,exception,open\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unremediated_count, 1);
        assert!(report.exceptions[0].days_since_review.is_none());
    }

    #[test]
    fn recert_treats_empty_remediation_status_as_open() {
        // A present remediation_status column whose value is blank is
        // treated as still open. Firms that omit remediation data shouldn't
        // have that silently close an exception.
        let ad = parse("email,enabled\nbob@acme.com,TRUE\n").unwrap();
        let review = parse(
            "email,review_date,decision,remediation_status\n\
             bob@acme.com,2024-11-01,exception,\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unremediated_count, 1);
    }

    #[test]
    fn recert_skips_remediation_check_without_exception_signal_column() {
        // Review log has remediation_status but no exception/decision column.
        // We can't tell which rows were exceptions, so the check is skipped.
        let ad = parse("email,enabled\nbob@acme.com,TRUE\n").unwrap();
        let review = parse(
            "email,review_date,remediation_status\n\
             bob@acme.com,2024-11-01,open\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.remediation_check_applied, false);
        assert_eq!(report.unremediated_count, 0);
    }

    #[test]
    fn recert_skips_remediation_check_without_remediation_status_column() {
        // Exception column present but no remediation_status → skip.
        let ad = parse("email,enabled\nbob@acme.com,TRUE\n").unwrap();
        let review = parse(
            "email,review_date,decision\n\
             bob@acme.com,2024-11-01,exception\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.remediation_check_applied, false);
        assert_eq!(report.unremediated_count, 0);
    }

    #[test]
    fn recert_explicit_exception_raised_column_beats_decision_disagreement() {
        // exception_raised=no with decision=exception should NOT flag the
        // row; the dedicated boolean wins.
        let ad = parse("email,enabled\nbob@acme.com,TRUE\n").unwrap();
        let review = parse(
            "email,review_date,exception_raised,decision,remediation_status\n\
             bob@acme.com,2024-11-01,no,exception,open\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.remediation_check_applied, true);
        assert_eq!(report.unremediated_count, 0);
    }

    #[test]
    fn recert_logon_fallback_works_when_review_log_has_no_email_column() {
        // Review log is logon-only; AD has both. Completeness match should
        // still hit via the logon fallback.
        let ad = parse(
            "sAMAccountName,email,enabled\n\
             alice,alice@acme.com,TRUE\n\
             bob,bob@acme.com,TRUE\n",
        )
        .unwrap();
        let review = parse("logon_name\nalice\n").unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unreviewed_count, 1);
        assert_eq!(report.exceptions[0].logon.as_deref(), Some("bob"));
    }

    #[test]
    fn recert_skips_review_rows_with_neither_email_nor_logon() {
        // A review row that can't be canonicalised gets counted in
        // review_rows_skipped_unmatchable and excluded from the reviewed set,
        // so any AD row it was meant to cover will surface as unreviewed.
        let ad = parse("email,enabled\nalice@acme.com,TRUE\n").unwrap();
        let review = parse("email,notes\n,nothing\n").unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.review_rows_skipped_unmatchable, 1);
        assert_eq!(report.unreviewed_count, 1);
    }

    #[test]
    fn recert_both_kinds_of_exception_in_one_run() {
        // carol is unreviewed; bob is unremediated. Mixed-exception run.
        let ad = parse(
            "email,enabled\n\
             alice@acme.com,TRUE\n\
             bob@acme.com,TRUE\n\
             carol@acme.com,TRUE\n",
        )
        .unwrap();
        let review = parse(
            "email,review_date,decision,remediation_status\n\
             alice@acme.com,2024-11-01,approved,closed\n\
             bob@acme.com,2024-11-01,exception,open\n",
        )
        .unwrap();

        let report = run_periodic_recertification(&review, &ad, AS_OF_2025_06_01, 90);

        assert_eq!(report.unreviewed_count, 1);
        assert_eq!(report.unremediated_count, 1);
        assert_eq!(report.exceptions.len(), 2);
        // unreviewed come first, then unremediated (see ordering in
        // run_periodic_recertification).
        assert_eq!(report.exceptions[0].kind, "unreviewed_account");
        assert_eq!(report.exceptions[1].kind, "unremediated_exception");
    }
}
