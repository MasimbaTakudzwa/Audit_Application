//! User-access-review rules.
//!
//! Right now we run the highest-value rule only: **terminated-but-active**.
//! Given an AD (or Entra) export and an HR leavers list, flag every AD
//! account still marked as enabled whose owner appears in the leavers list
//! (matched by email, with a fallback to logon name).
//!
//! Intentional simplicity:
//!   - The matcher is pure. It takes parsed tables and returns a report.
//!     Blob I/O and `TestResult` persistence live in the command layer so
//!     this file has no test-harness wiring beyond `Table` construction.
//!   - Matching is by lower-cased email; the fallback logon-name match only
//!     kicks in when the AD row has no email column (rare but possible on
//!     script-hand-rolled exports).
//!   - The rule assumes "enabled = true" unless an explicit `enabled`,
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
}
