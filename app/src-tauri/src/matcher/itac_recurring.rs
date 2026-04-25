//! ITAC recurring-amount detection across counterparties.
//!
//! Group transaction rows by absolute monetary amount alone (integer cents)
//! and flag any amount that recurs at unusual frequency *across distinct
//! counterparties*. Genuine business activity rarely produces the same
//! amount paid to or received from many unrelated parties; clusters with
//! that signature warrant investigation as potential template-driven
//! postings, kickback patterns, structured payments under internal
//! thresholds, or fabricated records assembled from a single placeholder
//! figure.
//!
//! **What's distinct from the other ITAC rules.**
//!   - `ITAC-T-001` Benford asks "does the leading-digit distribution look
//!     natural?" from first principles (whole-population statistic).
//!   - `ITAC-T-002` duplicates asks "are there exact `(amount, counterparty,
//!     date)` triples?" — same posting at the same place on the same day.
//!   - `ITAC-T-003` boundary asks "are amounts clustering just below
//!     authorisation thresholds?"
//!   - `ITAC-T-004` (this rule) asks "is the same amount showing up at
//!     many *different* counterparties?" Same key as duplicates without
//!     the counterparty + date constraints — the `(amount)` axis with
//!     diversity-of-counterparty as the signal.
//!
//! **Gates applied to flag a group.** All three must hold:
//!   1. `row_count >= MIN_GROUP_ROWS` (default 5) — the group must contain
//!      at least 5 rows. Below that, the cluster is too small to
//!      distinguish from coincidence.
//!   2. `distinct_counterparties >= MIN_DISTINCT_COUNTERPARTIES` (default 5)
//!      — the *primary* gate. A row count without distinct-counterparty
//!      diversity is just one vendor billing the same amount repeatedly,
//!      which is the duplicates rule's territory, not this one.
//!   3. `amount_cents >= MIN_AMOUNT_CENTS` (default 10_000 = $100) —
//!      excludes trivial repeating amounts (small fees, parking-lot
//!      charges, $1 verification postings) that recur naturally and would
//!      drown the report in noise.
//!
//! **Sign-insensitive.** Refunds and reversals fold into the same
//! magnitude bucket as the corresponding payments via `amount.abs()`.
//! A pattern of "the same $1,234.56 going to many vendors" is the signal
//! whether posted as positive (payment out) or negative (refund in) —
//! both shapes are equally suspicious and both should land in the same
//! group.
//!
//! **Verbatim counterparty match for the diversity count.** The "distinct
//! counterparties" measure compares normalised (trimmed + lower-cased)
//! counterparty strings. `Acme Ltd.` and `acme ltd.` count as one
//! counterparty; `Acme Ltd.` and `Acme Limited` count as two. Fuzzy
//! vendor identity is out of scope here — same reasoning as the duplicates
//! rule: an alias table is firm-specific configuration, not a
//! data-derivable assumption.

use std::collections::BTreeMap;

use serde::Serialize;

use super::csv::{find_column, Row, Table};
use super::itac_benford::{parse_amount, AMOUNT_CANDIDATES};
use super::itac_duplicates::COUNTERPARTY_CANDIDATES;

/// Minimum number of rows in a group before it can be flagged. Five rows
/// is the smallest cluster that's plausibly a pattern rather than
/// coincidence given that each row must come from a distinct counterparty.
pub const MIN_GROUP_ROWS: usize = 5;

/// Minimum number of distinct counterparties in a group before it can be
/// flagged. The *primary* gate — without diversity-of-counterparty the
/// signal is single-vendor repetition (duplicates territory).
pub const MIN_DISTINCT_COUNTERPARTIES: usize = 5;

/// Significance floor (in cents) below which an amount is too small to
/// flag. $100 is round, defensible, and excludes the small fees and
/// verification postings that naturally repeat across many vendors.
pub const MIN_AMOUNT_CENTS: i64 = 10_000;

/// Cap on sample rows preserved per exception. A pathological 1,000-row
/// group shouldn't blow up the report JSON — the auditor reads 3–5 rows
/// to confirm the pattern; the row ordinals (kept in full) locate
/// everything else.
pub const SAMPLE_ROWS_PER_GROUP: usize = 5;

/// Cap on how many distinct counterparties to enumerate per exception.
/// Defensive — a 50-counterparty cluster is the rule firing as intended
/// but the JSON should not balloon. Auditor reads the count, the first
/// 10 names (alphabetical), and uses `row_ordinals` to pull the full set
/// from the source CSV when investigating.
pub const MAX_COUNTERPARTIES_LISTED: usize = 10;

/// One recurring-amount cluster — a single integer-cents amount whose
/// occurrence pattern across distinct counterparties exceeds all three
/// gates.
#[derive(Debug, Clone, Serialize)]
pub struct RecurringAmountException {
    /// Rule-specific kind tag. Currently only `recurring_amount_group`.
    pub kind: String,
    /// Display amount — the first-occurrence row's raw amount string,
    /// so the auditor reads the exact currency / formatting they uploaded.
    pub display_amount: String,
    /// Parsed amount rounded to the nearest cent. The grouping key.
    pub amount_cents: i64,
    /// Total rows in the group. Always >= `MIN_GROUP_ROWS`.
    pub row_count: usize,
    /// Number of distinct normalised counterparties in the group.
    /// Always >= `MIN_DISTINCT_COUNTERPARTIES`.
    pub distinct_counterparty_count: usize,
    /// Up to `MAX_COUNTERPARTIES_LISTED` counterparties (display form,
    /// sorted alphabetically by their normalised key for determinism)
    /// from the group. Auditor reads this to assess whether the cluster
    /// has a plausible business explanation (uniform fee schedule,
    /// regulatory levy) or warrants escalation.
    pub counterparties: Vec<String>,
    /// 1-based ordinals of every row in the group.
    pub row_ordinals: Vec<usize>,
    /// First few rows (up to `SAMPLE_ROWS_PER_GROUP`) for auditor review.
    pub sample_rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecurringAmountReport {
    pub rule: String,
    pub rows_considered: usize,
    pub rows_skipped_unparseable: usize,
    pub rows_skipped_zero: usize,
    /// Rows with empty / missing counterparty — can't contribute to the
    /// distinct-counterparty count, so skipped rather than grouped.
    pub rows_skipped_no_counterparty: usize,
    /// Rows whose absolute amount is below the significance floor. These
    /// are not grouped at all (the rule deliberately doesn't surface
    /// $5.00 amounts even if they recur across 50 vendors — that's small-
    /// fee noise).
    pub rows_skipped_below_significance: usize,
    /// Constants exposed in the report so an auditor reading the JSON
    /// detail sees what the rule was tuned to without opening source.
    pub min_group_rows: usize,
    pub min_distinct_counterparties: usize,
    pub min_amount_cents: i64,
    pub recurring_group_count: usize,
    /// Total rows across all flagged groups. Sum of `row_count` per
    /// exception — gives the auditor the total exposed population in
    /// one figure.
    pub total_recurring_rows: usize,
    pub exceptions: Vec<RecurringAmountException>,
}

/// Scan the transaction register for amounts that recur across many
/// distinct counterparties. Returns one exception per amount group that
/// passes all three gates.
pub fn run_recurring_amounts(transactions: &Table) -> RecurringAmountReport {
    let amount_col = find_column(transactions, AMOUNT_CANDIDATES);
    let counterparty_col = find_column(transactions, COUNTERPARTY_CANDIDATES);

    // Group accumulator. BTreeMap on amount_cents gives stable iteration
    // order; we'll re-sort by descending amount before emitting exceptions.
    #[derive(Default)]
    struct GroupAccum {
        row_ordinals: Vec<usize>,
        sample_rows: Vec<Vec<String>>,
        // Map of normalised counterparty key → first-seen display form.
        // BTreeMap so the alphabetical "first 10" listing is deterministic.
        counterparties: BTreeMap<String, String>,
        display_amount: String,
    }
    let mut groups: BTreeMap<i64, GroupAccum> = BTreeMap::new();

    let mut skipped_unparseable = 0usize;
    let mut skipped_zero = 0usize;
    let mut skipped_no_counterparty = 0usize;
    let mut skipped_below_significance = 0usize;

    for row in &transactions.rows {
        // Amount via the shared ITAC parser.
        let amount_raw = cell(row, amount_col.as_deref()).map(String::as_str);
        let amount_display = amount_raw.unwrap_or("").trim().to_string();
        let amount = match amount_raw.and_then(parse_amount) {
            Some(a) => a,
            None => {
                skipped_unparseable += 1;
                continue;
            }
        };
        if amount == 0.0 {
            skipped_zero += 1;
            continue;
        }
        // Round to nearest cent on the absolute value. Sign-insensitive on
        // purpose — refund mirroring a payment is the same magnitude
        // signal.
        let amount_cents = (amount.abs() * 100.0).round() as i64;

        // Counterparty — trimmed; lower-cased for the diversity key.
        let counterparty_display = cell(row, counterparty_col.as_deref())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if counterparty_display.is_empty() {
            skipped_no_counterparty += 1;
            continue;
        }
        let counterparty_key = counterparty_display.to_ascii_lowercase();

        // Significance floor. Below the floor, the row doesn't contribute
        // to any group at all.
        if amount_cents < MIN_AMOUNT_CENTS {
            skipped_below_significance += 1;
            continue;
        }

        let entry = groups.entry(amount_cents).or_default();
        entry.row_ordinals.push(row.ordinal);
        entry
            .counterparties
            .entry(counterparty_key)
            .or_insert(counterparty_display);
        if entry.display_amount.is_empty() {
            entry.display_amount = amount_display;
        }
        if entry.sample_rows.len() < SAMPLE_ROWS_PER_GROUP {
            entry.sample_rows.push(row.raw_values.clone());
        }
    }

    // Walk groups; emit exceptions for those passing all three gates.
    let mut exceptions: Vec<RecurringAmountException> = Vec::new();
    let mut total_recurring_rows = 0usize;
    for (amount_cents, accum) in groups.into_iter() {
        let row_count = accum.row_ordinals.len();
        let distinct = accum.counterparties.len();
        if row_count < MIN_GROUP_ROWS {
            continue;
        }
        if distinct < MIN_DISTINCT_COUNTERPARTIES {
            continue;
        }
        // amount_cents floor is applied at row-skip time, so any group
        // present here has amount_cents >= MIN_AMOUNT_CENTS already.
        debug_assert!(amount_cents >= MIN_AMOUNT_CENTS);

        total_recurring_rows += row_count;

        // First N counterparties (alphabetical by normalised key — BTreeMap
        // iteration order — using their display form).
        let counterparties: Vec<String> = accum
            .counterparties
            .into_values()
            .take(MAX_COUNTERPARTIES_LISTED)
            .collect();

        exceptions.push(RecurringAmountException {
            kind: "recurring_amount_group".into(),
            display_amount: accum.display_amount,
            amount_cents,
            row_count,
            distinct_counterparty_count: distinct,
            counterparties,
            row_ordinals: accum.row_ordinals,
            sample_rows: accum.sample_rows,
        });
    }

    // Order: descending by amount_cents — largest dollar fish first for
    // auditor attention. Stable secondary by row_count desc just to keep
    // ties deterministic; in practice amount_cents alone is unique per
    // group.
    exceptions.sort_by(|a, b| {
        b.amount_cents
            .cmp(&a.amount_cents)
            .then(b.row_count.cmp(&a.row_count))
    });

    // De-duplicate group counts to a single set used for both the report
    // total and per-exception counters. (`exceptions` already passed all
    // gates, so this is just total_recurring_rows + len.)
    let recurring_group_count = exceptions.len();

    RecurringAmountReport {
        rule: "recurring_amounts".into(),
        rows_considered: transactions.rows.len(),
        rows_skipped_unparseable: skipped_unparseable,
        rows_skipped_zero: skipped_zero,
        rows_skipped_no_counterparty: skipped_no_counterparty,
        rows_skipped_below_significance: skipped_below_significance,
        min_group_rows: MIN_GROUP_ROWS,
        min_distinct_counterparties: MIN_DISTINCT_COUNTERPARTIES,
        min_amount_cents: MIN_AMOUNT_CENTS,
        recurring_group_count,
        total_recurring_rows,
        exceptions,
    }
}

fn cell<'a>(row: &'a Row, column: Option<&str>) -> Option<&'a String> {
    column.and_then(|c| row.values.get(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::csv::parse;

    /// Build a transaction CSV header + N rows where amount=`amt` recurs
    /// across distinct vendors v1..vN. Helper used by several tests.
    fn build_recurring_csv(amt: &str, distinct_vendors: usize, header: &str) -> String {
        let mut out = format!("{header}\n");
        for i in 1..=distinct_vendors {
            out.push_str(&format!("{amt},Vendor {i},2025-01-{:02}\n", i));
        }
        out
    }

    #[test]
    fn flags_amount_recurring_across_five_distinct_counterparties() {
        // 5 rows of $1,234.56 spread across 5 different vendors → flagged.
        let csv = build_recurring_csv("1234.56", 5, "amount,vendor,date");
        let table = parse(&csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.exceptions.len(), 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.kind, "recurring_amount_group");
        assert_eq!(ex.row_count, 5);
        assert_eq!(ex.distinct_counterparty_count, 5);
        assert_eq!(ex.amount_cents, 123_456);
        assert_eq!(report.total_recurring_rows, 5);
    }

    #[test]
    fn does_not_flag_repetition_at_single_counterparty() {
        // 10 rows, same vendor — distinct count is 1, well below the
        // MIN_DISTINCT_COUNTERPARTIES gate. That's duplicates territory,
        // not recurring-amount territory.
        let mut csv = String::from("amount,vendor,date\n");
        for i in 1..=10 {
            csv.push_str(&format!("500.00,Acme Supplies,2025-01-{:02}\n", i));
        }
        let table = parse(&csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.recurring_group_count, 0);
    }

    #[test]
    fn does_not_flag_below_min_distinct_counterparties() {
        // 4 rows across 4 distinct vendors — fails the >=5 distinct gate.
        let csv = build_recurring_csv("750.00", 4, "amount,vendor,date");
        let table = parse(&csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn does_not_flag_amount_below_significance_floor() {
        // $50.00 < $100 floor. Even with 10 distinct counterparties the
        // rule should skip at row-skip time and never group.
        let csv = build_recurring_csv("50.00", 10, "amount,vendor,date");
        let table = parse(&csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.rows_skipped_below_significance, 10);
    }

    #[test]
    fn header_variants_are_normalised() {
        // Use `Transaction Amount` and `Counter Party` — both should resolve
        // via the shared candidate lists' case + space tolerance (the CSV
        // canonicaliser strips `_`, `-`, ` ` and lower-cases). `Transaction
        // Amount` → `transactionamount` (in AMOUNT_CANDIDATES); `Counter
        // Party` → `counterparty` (in COUNTERPARTY_CANDIDATES).
        let mut csv = String::from("Transaction Amount,Counter Party,Date\n");
        for i in 1..=5 {
            csv.push_str(&format!("999.00,\"Vendor {i}\",2025-02-{:02}\n", i));
        }
        let table = parse(&csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.exceptions.len(), 1);
    }

    #[test]
    fn rows_missing_counterparty_are_counted_skipped_not_grouped() {
        let csv = "amount,vendor,date\n\
                   500.00,,2025-01-01\n\
                   500.00,Vendor 1,2025-01-02\n\
                   500.00,Vendor 2,2025-01-03\n";
        let table = parse(csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.rows_skipped_no_counterparty, 1);
        // The remaining two rows fail the distinct gate (need >=5).
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn unparseable_amount_rows_are_counted_but_not_grouped() {
        let csv = "amount,vendor,date\n\
                   abc,Vendor 1,2025-01-01\n\
                   500.00,Vendor 2,2025-01-02\n\
                   500.00,Vendor 3,2025-01-03\n";
        let table = parse(csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.rows_skipped_unparseable, 1);
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn zero_amount_rows_are_skipped() {
        let csv = "amount,vendor,date\n\
                   0,Vendor 1,2025-01-01\n\
                   0.00,Vendor 2,2025-01-02\n";
        let table = parse(csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.rows_skipped_zero, 2);
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn counterparty_match_is_case_and_whitespace_insensitive() {
        // `acme ltd.` and `Acme Ltd.` should fold to the same key, so the
        // diversity count is 4 distinct (not 5) — the row from `Acme
        // Ltd.` collapses into the same bucket as `acme ltd.`. Resulting
        // group has 5 rows but only 4 distinct → not flagged.
        let csv = "amount,vendor,date\n\
                   500.00,acme ltd.,2025-01-01\n\
                   500.00,Acme Ltd.,2025-01-02\n\
                   500.00,Beta Co,2025-01-03\n\
                   500.00,Gamma Inc,2025-01-04\n\
                   500.00,Delta LLC,2025-01-05\n";
        let table = parse(csv).unwrap();
        let report = run_recurring_amounts(&table);
        // 5 rows, 4 distinct counterparties → fails distinct gate.
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn negative_amounts_fold_to_absolute_for_grouping() {
        // Mix of +500 and -500 across 5 distinct vendors — folds to the
        // same group via abs().
        let csv = "amount,vendor,date\n\
                   500.00,Vendor 1,2025-01-01\n\
                   -500.00,Vendor 2,2025-01-02\n\
                   500.00,Vendor 3,2025-01-03\n\
                   -500.00,Vendor 4,2025-01-04\n\
                   500.00,Vendor 5,2025-01-05\n";
        let table = parse(csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].row_count, 5);
        assert_eq!(report.exceptions[0].amount_cents, 50_000);
    }

    #[test]
    fn currency_symbol_and_thousands_separators_normalise_for_grouping() {
        // `$1,234.56` should parse to the same amount as `1234.56` and
        // `USD 1234.56` — all five rows should fold to one group.
        let csv = "amount,vendor,date\n\
                   \"$1,234.56\",Vendor 1,2025-01-01\n\
                   1234.56,Vendor 2,2025-01-02\n\
                   USD 1234.56,Vendor 3,2025-01-03\n\
                   1234.56,Vendor 4,2025-01-04\n\
                   \"1,234.56\",Vendor 5,2025-01-05\n";
        let table = parse(csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].amount_cents, 123_456);
    }

    #[test]
    fn exception_order_is_descending_by_amount() {
        // Three flagged groups at $100, $1000, $10000 — should appear in
        // descending-amount order.
        let mut csv = String::from("amount,vendor,date\n");
        for amt in &["100.00", "1000.00", "10000.00"] {
            for i in 1..=5 {
                csv.push_str(&format!("{amt},\"{amt}-V{i}\",2025-01-{:02}\n", i));
            }
        }
        let table = parse(&csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.exceptions.len(), 3);
        assert_eq!(report.exceptions[0].amount_cents, 1_000_000);
        assert_eq!(report.exceptions[1].amount_cents, 100_000);
        assert_eq!(report.exceptions[2].amount_cents, 10_000);
    }

    #[test]
    fn sample_rows_capped_at_five() {
        // 8 rows in one group → sample should cap at 5; row_ordinals
        // keeps the full list of 8.
        let mut csv = String::from("amount,vendor,date\n");
        for i in 1..=8 {
            csv.push_str(&format!("777.00,Vendor {i},2025-01-{:02}\n", i));
        }
        let table = parse(&csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.exceptions.len(), 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.row_count, 8);
        assert_eq!(ex.sample_rows.len(), SAMPLE_ROWS_PER_GROUP);
        assert_eq!(ex.row_ordinals.len(), 8);
    }

    #[test]
    fn counterparty_list_capped_at_ten() {
        // 15 distinct counterparties → counterparties list capped at 10;
        // distinct_counterparty_count still reports the full 15.
        let mut csv = String::from("amount,vendor,date\n");
        for i in 1..=15 {
            csv.push_str(&format!("888.00,Vendor {:02},2025-01-{:02}\n", i, i));
        }
        let table = parse(&csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.exceptions.len(), 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.distinct_counterparty_count, 15);
        assert_eq!(ex.counterparties.len(), MAX_COUNTERPARTIES_LISTED);
    }

    #[test]
    fn constants_round_trip_into_report() {
        // The auditor reads min_group_rows / min_distinct_counterparties /
        // min_amount_cents from the JSON detail without opening source.
        let table = parse("amount,vendor,date\n").unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.min_group_rows, MIN_GROUP_ROWS);
        assert_eq!(
            report.min_distinct_counterparties,
            MIN_DISTINCT_COUNTERPARTIES
        );
        assert_eq!(report.min_amount_cents, MIN_AMOUNT_CENTS);
    }

    #[test]
    fn missing_amount_column_skips_every_row() {
        // No header that resolves to an amount column → every row
        // becomes `rows_skipped_unparseable` (parse_amount on no input
        // returns None).
        let csv = "vendor,date\n\
                   Vendor 1,2025-01-01\n\
                   Vendor 2,2025-01-02\n";
        let table = parse(csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.rows_considered, 2);
        assert_eq!(report.rows_skipped_unparseable, 2);
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn missing_counterparty_column_skips_every_row() {
        // No counterparty column → every parseable amount becomes
        // `rows_skipped_no_counterparty` and never groups.
        let csv = "amount,date\n\
                   500.00,2025-01-01\n\
                   500.00,2025-01-02\n";
        let table = parse(csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.rows_considered, 2);
        assert_eq!(report.rows_skipped_no_counterparty, 2);
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn single_counterparty_repeated_in_one_group_does_not_double_count_distinct() {
        // 6 rows: 2 at Vendor A, 1 each at Vendors B–E. Total 6 rows,
        // 5 distinct → meets both gates. Distinct counterparty count
        // should be exactly 5 (Vendor A counted once, not twice).
        let csv = "amount,vendor,date\n\
                   600.00,Vendor A,2025-01-01\n\
                   600.00,Vendor A,2025-01-02\n\
                   600.00,Vendor B,2025-01-03\n\
                   600.00,Vendor C,2025-01-04\n\
                   600.00,Vendor D,2025-01-05\n\
                   600.00,Vendor E,2025-01-06\n";
        let table = parse(csv).unwrap();
        let report = run_recurring_amounts(&table);
        assert_eq!(report.exceptions.len(), 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.row_count, 6);
        assert_eq!(ex.distinct_counterparty_count, 5);
    }
}

