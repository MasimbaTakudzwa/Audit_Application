//! Backup rules.
//!
//! One rule so far:
//!   - **backup-performance** — flag scheduled backups that did not complete
//!     successfully. Covers `Failed`, `Cancelled`, `Skipped`, and `Partial`
//!     status values, with an optional secondary flag when a
//!     `verified` / `verification_result` column says the backup was not
//!     verified.
//!
//! The matcher expects a single CSV from the backup tool (Veeam,
//! Commvault, Rubrik, Backup Exec, TSM, in-house schedulers, …) with one
//! row per job run. Rerun detection — "a failed job that was followed by a
//! successful rerun against the same target within X hours" — is
//! deliberately out of scope for v1: it adds policy judgement the auditor
//! should make explicitly, not a rule-level assumption. Every failed row is
//! surfaced; the auditor suppresses the ones with evidenced reruns.
//!
//! Intentional simplicity:
//!   - Pure function. Takes a `Table`, returns a report. No I/O.
//!   - Rows with an unrecognised or empty status are counted as
//!     `jobs_skipped_unknown_status` rather than flagged. "Unknown" ≠ "bad".
//!   - Rows without a job identifier are counted and skipped — the auditor
//!     would not be able to action an exception with no job ID.

use serde::Serialize;

use super::csv::{find_column, Row, Table};

const JOB_ID_CANDIDATES: &[&str] = &[
    "jobid",
    "job_id",
    "jobname",
    "job_name",
    "backupid",
    "backup_id",
    "backupjobname",
    "backup_job_name",
    "name",
    "id",
];

const JOB_TYPE_CANDIDATES: &[&str] = &[
    "jobtype",
    "job_type",
    "backuptype",
    "backup_type",
    "type",
];

const STATUS_CANDIDATES: &[&str] = &["status", "result", "outcome", "state"];

const TARGET_CANDIDATES: &[&str] = &[
    "target",
    "targetserver",
    "target_server",
    "client",
    "server",
    "host",
    "system",
    "source",
    "resource",
    "object",
];

const STARTED_AT_CANDIDATES: &[&str] = &[
    "startedat",
    "started_at",
    "starttime",
    "start_time",
    "startdate",
    "start_date",
    "begin",
    "beginat",
    "begin_at",
];

const COMPLETED_AT_CANDIDATES: &[&str] = &[
    "completedat",
    "completed_at",
    "endedat",
    "ended_at",
    "endtime",
    "end_time",
    "enddate",
    "end_date",
    "finishedat",
    "finished_at",
];

const VERIFIED_CANDIDATES: &[&str] = &[
    "verified",
    "verification",
    "verificationresult",
    "verification_result",
    "integrityverified",
    "integrity_verified",
    "checksumverified",
    "checksum_verified",
];

#[derive(Debug, Clone, Serialize)]
pub struct BackupException {
    /// Rule-specific kind tag. One of:
    /// - `backup_failed` — status is Failed / Cancelled / Skipped / Partial
    /// - `backup_not_verified` — status is Success, but a verification column
    ///   is present and indicates the backup was not verified
    pub kind: String,
    pub job_id: String,
    pub job_ordinal: usize,
    pub job_type: Option<String>,
    pub target: Option<String>,
    pub status: String,
    pub started_at_secs: Option<i64>,
    pub completed_at_secs: Option<i64>,
    pub job_row: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupReport {
    pub rule: String,
    pub jobs_considered: usize,
    pub jobs_skipped_no_id: usize,
    pub jobs_skipped_unknown_status: usize,
    pub exceptions: Vec<BackupException>,
}

pub fn run_backup_performance(jobs: &Table) -> BackupReport {
    let id_col = find_column(jobs, JOB_ID_CANDIDATES);
    let type_col = find_column(jobs, JOB_TYPE_CANDIDATES);
    let status_col = find_column(jobs, STATUS_CANDIDATES);
    let target_col = find_column(jobs, TARGET_CANDIDATES);
    let started_col = find_column(jobs, STARTED_AT_CANDIDATES);
    let completed_col = find_column(jobs, COMPLETED_AT_CANDIDATES);
    let verified_col = find_column(jobs, VERIFIED_CANDIDATES);

    let mut exceptions = Vec::new();
    let mut skipped_no_id = 0usize;
    let mut skipped_unknown_status = 0usize;

    for row in &jobs.rows {
        let job_id = cell(row, id_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let Some(job_id) = job_id else {
            skipped_no_id += 1;
            continue;
        };

        let raw_status = cell(row, status_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let Some(raw_status) = raw_status else {
            skipped_unknown_status += 1;
            continue;
        };

        let classification = classify_status(&raw_status);
        let classification = match classification {
            Some(c) => c,
            None => {
                skipped_unknown_status += 1;
                continue;
            }
        };

        let job_type = cell(row, type_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let target = cell(row, target_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let started_at_secs = cell(row, started_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .and_then(|raw| parse_timestamp(&raw));
        let completed_at_secs = cell(row, completed_col.as_deref())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .and_then(|raw| parse_timestamp(&raw));

        match classification {
            StatusKind::Failed => {
                exceptions.push(BackupException {
                    kind: "backup_failed".into(),
                    job_id: job_id.clone(),
                    job_ordinal: row.ordinal,
                    job_type: job_type.clone(),
                    target: target.clone(),
                    status: raw_status.clone(),
                    started_at_secs,
                    completed_at_secs,
                    job_row: row.raw_values.clone(),
                });
            }
            StatusKind::Success => {
                // Only surface "not verified" when a verification column is
                // actually present. If there's no column, the absence of
                // verification information is not the auditor's problem.
                if let Some(col) = &verified_col {
                    if let Some(raw) = row.values.get(col) {
                        if !is_truthy(raw) {
                            exceptions.push(BackupException {
                                kind: "backup_not_verified".into(),
                                job_id: job_id.clone(),
                                job_ordinal: row.ordinal,
                                job_type: job_type.clone(),
                                target: target.clone(),
                                status: raw_status.clone(),
                                started_at_secs,
                                completed_at_secs,
                                job_row: row.raw_values.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    BackupReport {
        rule: "backup_performance".into(),
        jobs_considered: jobs.rows.len(),
        jobs_skipped_no_id: skipped_no_id,
        jobs_skipped_unknown_status: skipped_unknown_status,
        exceptions,
    }
}

fn cell<'a>(row: &'a Row, column: Option<&str>) -> Option<&'a String> {
    column.and_then(|c| row.values.get(c))
}

#[derive(Debug, Clone, Copy)]
enum StatusKind {
    Success,
    Failed,
}

fn classify_status(raw: &str) -> Option<StatusKind> {
    let v = raw.trim().to_ascii_lowercase();
    match v.as_str() {
        "success" | "succeeded" | "successful" | "ok" | "completed" | "complete" | "done" => {
            Some(StatusKind::Success)
        }
        "failed" | "failure" | "fail" | "error" | "errored" | "skipped" | "cancelled"
        | "canceled" | "aborted" | "partial" | "partiallyfailed" | "partially_failed"
        | "warning" | "missed" => Some(StatusKind::Failed),
        _ => None,
    }
}

fn is_truthy(raw: &str) -> bool {
    let v = raw.trim().to_ascii_lowercase();
    matches!(
        v.as_str(),
        "true" | "yes" | "y" | "1" | "verified" | "ok" | "pass" | "passed" | "success"
    )
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::csv::parse;

    #[test]
    fn flags_failed_and_skipped_jobs() {
        let jobs = parse(
            "job_id,status,target,start_time,end_time\n\
             j-001,Success,web01,2025-03-01T01:00:00Z,2025-03-01T01:12:00Z\n\
             j-002,Failed,web02,2025-03-02T01:00:00Z,2025-03-02T01:07:00Z\n\
             j-003,Skipped,web03,2025-03-03T01:00:00Z,\n",
        )
        .unwrap();
        let report = run_backup_performance(&jobs);
        assert_eq!(report.exceptions.len(), 2);
        let ids: Vec<&str> = report.exceptions.iter().map(|e| e.job_id.as_str()).collect();
        assert!(ids.contains(&"j-002"));
        assert!(ids.contains(&"j-003"));
    }

    #[test]
    fn flags_cancelled_and_partial_as_failed() {
        let jobs = parse(
            "job_id,status\n\
             j-010,Cancelled\n\
             j-011,Partial\n\
             j-012,Warning\n",
        )
        .unwrap();
        let report = run_backup_performance(&jobs);
        assert_eq!(report.exceptions.len(), 3);
        assert!(
            report
                .exceptions
                .iter()
                .all(|e| e.kind == "backup_failed")
        );
    }

    #[test]
    fn successful_jobs_with_no_verification_column_are_not_exceptions() {
        let jobs = parse(
            "job_id,status\n\
             j-020,Success\n\
             j-021,Completed\n",
        )
        .unwrap();
        let report = run_backup_performance(&jobs);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.jobs_considered, 2);
    }

    #[test]
    fn successful_jobs_flagged_when_verification_column_says_no() {
        let jobs = parse(
            "job_id,status,verified\n\
             j-030,Success,true\n\
             j-031,Success,false\n\
             j-032,Success,\n",
        )
        .unwrap();
        let report = run_backup_performance(&jobs);
        // j-031 is explicitly not verified. j-032 has an empty verified cell
        // which is also not truthy — flag it too; an empty value when a
        // verification column exists is a policy gap the auditor should see.
        assert_eq!(report.exceptions.len(), 2);
        assert!(
            report
                .exceptions
                .iter()
                .all(|e| e.kind == "backup_not_verified")
        );
    }

    #[test]
    fn rows_without_job_id_are_counted_skipped_not_flagged() {
        let jobs = parse(
            "job_id,status\n\
             ,Failed\n\
             j-040,Failed\n",
        )
        .unwrap();
        let report = run_backup_performance(&jobs);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.jobs_skipped_no_id, 1);
    }

    #[test]
    fn rows_with_unknown_or_empty_status_counted_skipped() {
        let jobs = parse(
            "job_id,status\n\
             j-050,\n\
             j-051,InProgress\n\
             j-052,Queued\n",
        )
        .unwrap();
        let report = run_backup_performance(&jobs);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.jobs_skipped_unknown_status, 3);
    }

    #[test]
    fn header_variants_normalise() {
        let jobs = parse(
            "Job Name,Backup Type,Status,Target Server,Start Time,End Time\n\
             nightly-web,Full,Failed,web01,2025-03-01T01:00:00Z,2025-03-01T01:12:00Z\n",
        )
        .unwrap();
        let report = run_backup_performance(&jobs);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].job_id, "nightly-web");
        assert_eq!(report.exceptions[0].target.as_deref(), Some("web01"));
        assert_eq!(report.exceptions[0].job_type.as_deref(), Some("Full"));
    }

    #[test]
    fn epoch_timestamps_parse() {
        let jobs = parse(
            "job_id,status,start_time,end_time\n\
             j-060,Failed,1740788100,1740789000\n",
        )
        .unwrap();
        let report = run_backup_performance(&jobs);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].started_at_secs, Some(1_740_788_100));
        assert_eq!(report.exceptions[0].completed_at_secs, Some(1_740_789_000));
    }
}
