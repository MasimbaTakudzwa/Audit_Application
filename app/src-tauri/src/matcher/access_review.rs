//! User-access-review rules.
//!
//! Two deterministic rules so far, both driven by an AD (or Entra) export:
//!   - **terminated-but-active** — reconcile the AD export against an HR
//!     leavers list; flag enabled accounts whose owner appears in the
//!     leavers list.
//!   - **dormant-accounts** — flag enabled accounts whose last logon is
//!     older than a configured threshold (default 90 days).
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
}
