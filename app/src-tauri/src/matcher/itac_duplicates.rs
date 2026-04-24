//! ITAC duplicate-transaction detection.
//!
//! Flag groups of transaction rows sharing the same `(amount, counterparty,
//! date)` key in the same population. A duplicate group is at least two rows
//! with an identical key after normalisation.
//!
//! **What counts as a duplicate.** The rule fires on exact-match keys after
//! minimal normalisation:
//!   - Amount parsed through `itac_benford::parse_amount` (strips currency
//!     symbols, ISO codes, thousands separators, paren-negatives, CR/DR)
//!     and compared as the absolute value rounded to the nearest cent.
//!   - Counterparty trimmed and lower-cased. Empty or missing → row skipped.
//!   - Date kept as a trimmed literal string. Same source usually emits a
//!     consistent date format within a file, so `2025-01-15` appearing twice
//!     is a duplicate hit; mixed-format dates in the same file are not
//!     normalised (that's a data-quality issue for the auditor to query
//!     separately — bridging `2025-01-15` ↔ `15/01/2025` needs a locale
//!     guess this rule deliberately won't make).
//!
//! **What's out of scope.** Fuzzy duplicates (same amount + same vendor
//! within N days), second-pass same-invoice-different-amount detection, and
//! cross-period duplicate-payment checks all belong in future rules. This
//! one finds the unambiguous mechanical duplicates first.
//!
//! **Zero and missing values.** A row with amount = 0, or an unparseable
//! amount, or an empty counterparty / date is skipped into the relevant
//! counter (`rows_skipped_zero`, `rows_skipped_unparseable`,
//! `rows_skipped_no_key`). Skipping silently avoids bogus "duplicate"
//! groups of all-empty-key rows.

use std::collections::BTreeMap;

use serde::Serialize;

use super::csv::{find_column, Row, Table};
use super::itac_benford::{parse_amount, AMOUNT_CANDIDATES};

/// Columns that plausibly hold the counterparty identity — the other side
/// of the transaction: vendor, customer, payee, beneficiary, etc. Deliberately
/// broad since transaction exports differ wildly between AP, AR, GL, and
/// treasury systems.
const COUNTERPARTY_CANDIDATES: &[&str] = &[
    "counterparty",
    "vendor",
    "vendor_name",
    "vendorname",
    "supplier",
    "supplier_name",
    "suppliername",
    "payee",
    "payee_name",
    "payeename",
    "customer",
    "customer_name",
    "customername",
    "client",
    "client_name",
    "clientname",
    "debtor",
    "creditor",
    "beneficiary",
    "beneficiary_name",
    "beneficiaryname",
    "account_name",
    "accountname",
    "party",
    "party_name",
    "partyname",
];

/// Columns that plausibly hold the transaction date.
const DATE_CANDIDATES: &[&str] = &[
    "date",
    "transaction_date",
    "transactiondate",
    "posting_date",
    "postingdate",
    "post_date",
    "postdate",
    "invoice_date",
    "invoicedate",
    "payment_date",
    "paymentdate",
    "value_date",
    "valuedate",
    "effective_date",
    "effectivedate",
    "document_date",
    "documentdate",
    "entry_date",
    "entrydate",
    "booking_date",
    "bookingdate",
];

/// A single duplicate group — two or more rows sharing the same key.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateException {
    /// Rule-specific kind tag. Currently only `duplicate_transaction_group`.
    pub kind: String,
    /// Display amount — the first-occurrence row's raw amount string,
    /// so the auditor reads the exact currency / formatting they uploaded.
    pub display_amount: String,
    /// Parsed amount rounded to the nearest cent. The grouping key.
    pub amount_cents: i64,
    /// Normalised counterparty key (trimmed, lower-cased). The auditor
    /// reads this plus `display_counterparty` to confirm.
    pub counterparty: String,
    /// Display counterparty — first-occurrence row's raw counterparty cell.
    pub display_counterparty: String,
    /// Date cell value, trimmed. Kept literal — see module doc for why.
    pub date: String,
    /// Number of rows in the group. Always >= 2.
    pub row_count: usize,
    /// 1-based ordinals of every row in the group.
    pub row_ordinals: Vec<usize>,
    /// First few rows (up to `SAMPLE_ROWS_PER_GROUP`) for auditor review.
    /// Cap is defensive — a 1,000-row duplicate group shouldn't blow up
    /// the report JSON.
    pub sample_rows: Vec<Vec<String>>,
}

/// Cap on the sample rows preserved per duplicate group. Auditors usually
/// need 2–3 rows to confirm the duplicate is real; beyond that the file
/// ordinals (kept in full) are enough to locate everything.
pub const SAMPLE_ROWS_PER_GROUP: usize = 5;

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateReport {
    pub rule: String,
    pub rows_considered: usize,
    pub rows_skipped_unparseable: usize,
    pub rows_skipped_zero: usize,
    /// Rows where counterparty or date was empty or missing — the row can't
    /// be grouped, so it's skipped rather than flagged.
    pub rows_skipped_no_key: usize,
    pub duplicate_group_count: usize,
    /// Total rows across all duplicate groups. A duplicate-count-of-three
    /// group contributes 3 here; the auditor reads this as the total
    /// exposed population.
    pub total_duplicate_rows: usize,
    pub exceptions: Vec<DuplicateException>,
}

/// Scan the transaction register for duplicate groups on the
/// `(amount, counterparty, date)` key. Returns one exception per group.
pub fn run_duplicate_transactions(transactions: &Table) -> DuplicateReport {
    let amount_col = find_column(transactions, AMOUNT_CANDIDATES);
    let counterparty_col = find_column(transactions, COUNTERPARTY_CANDIDATES);
    let date_col = find_column(transactions, DATE_CANDIDATES);

    // Group accumulator. BTreeMap gives stable iteration order so exception
    // output is reproducible across runs.
    #[derive(Default)]
    struct GroupAccum {
        row_ordinals: Vec<usize>,
        sample_rows: Vec<Vec<String>>,
        display_amount: String,
        display_counterparty: String,
    }
    let mut groups: BTreeMap<(i64, String, String), GroupAccum> = BTreeMap::new();

    let mut skipped_unparseable = 0usize;
    let mut skipped_zero = 0usize;
    let mut skipped_no_key = 0usize;

    for row in &transactions.rows {
        // Amount — parse via the shared ITAC helper.
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
        // Round to the nearest cent on the absolute value. Duplicate test
        // doesn't care about sign — a reversal posted twice is still a
        // double-post worth flagging. Integer key avoids float-key aliasing.
        let amount_cents = (amount.abs() * 100.0).round() as i64;

        // Counterparty — trimmed, lower-cased; skip if missing.
        let counterparty_display = cell(row, counterparty_col.as_deref())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if counterparty_display.is_empty() {
            skipped_no_key += 1;
            continue;
        }
        let counterparty_key = counterparty_display.to_ascii_lowercase();

        // Date — trimmed literal; skip if missing.
        let date_display = cell(row, date_col.as_deref())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if date_display.is_empty() {
            skipped_no_key += 1;
            continue;
        }

        let key = (amount_cents, counterparty_key, date_display.clone());
        let entry = groups.entry(key).or_default();
        entry.row_ordinals.push(row.ordinal);
        if entry.display_amount.is_empty() {
            entry.display_amount = amount_display;
        }
        if entry.display_counterparty.is_empty() {
            entry.display_counterparty = counterparty_display;
        }
        if entry.sample_rows.len() < SAMPLE_ROWS_PER_GROUP {
            entry.sample_rows.push(row.raw_values.clone());
        }
    }

    // Emit one exception per group with >= 2 rows.
    let mut exceptions = Vec::new();
    let mut total_duplicate_rows = 0usize;
    for ((amount_cents, counterparty_key, date), accum) in groups.into_iter() {
        if accum.row_ordinals.len() < 2 {
            continue;
        }
        total_duplicate_rows += accum.row_ordinals.len();
        exceptions.push(DuplicateException {
            kind: "duplicate_transaction_group".into(),
            display_amount: accum.display_amount,
            amount_cents,
            counterparty: counterparty_key,
            display_counterparty: accum.display_counterparty,
            date,
            row_count: accum.row_ordinals.len(),
            row_ordinals: accum.row_ordinals,
            sample_rows: accum.sample_rows,
        });
    }

    DuplicateReport {
        rule: "duplicate_transactions".into(),
        rows_considered: transactions.rows.len(),
        rows_skipped_unparseable: skipped_unparseable,
        rows_skipped_zero: skipped_zero,
        rows_skipped_no_key: skipped_no_key,
        duplicate_group_count: exceptions.len(),
        total_duplicate_rows,
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

    #[test]
    fn flags_exact_duplicate_pair() {
        let table = parse(
            "amount,vendor,date\n\
             1500.00,Acme Supplies,2025-01-15\n\
             1500.00,Acme Supplies,2025-01-15\n\
             250.00,Other Vendor,2025-02-01\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert_eq!(report.exceptions.len(), 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.kind, "duplicate_transaction_group");
        assert_eq!(ex.row_count, 2);
        assert_eq!(ex.amount_cents, 150_000);
        assert_eq!(ex.counterparty, "acme supplies");
        assert_eq!(ex.row_ordinals, vec![1, 2]);
        assert_eq!(report.duplicate_group_count, 1);
        assert_eq!(report.total_duplicate_rows, 2);
    }

    #[test]
    fn flags_larger_group_of_three() {
        let table = parse(
            "amount,vendor,date\n\
             50,Foo Inc,2025-03-01\n\
             50,Foo Inc,2025-03-01\n\
             50,Foo Inc,2025-03-01\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].row_count, 3);
        assert_eq!(report.total_duplicate_rows, 3);
    }

    #[test]
    fn passes_on_population_with_no_duplicates() {
        let table = parse(
            "amount,vendor,date\n\
             100,A,2025-01-01\n\
             200,B,2025-01-02\n\
             300,C,2025-01-03\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.rows_considered, 3);
        assert_eq!(report.duplicate_group_count, 0);
        assert_eq!(report.total_duplicate_rows, 0);
    }

    #[test]
    fn different_vendor_same_amount_same_date_is_not_a_duplicate() {
        // Classic test — two vendors each invoicing $500 on the same day is
        // perfectly normal business activity, not a duplicate.
        let table = parse(
            "amount,vendor,date\n\
             500,Vendor A,2025-04-10\n\
             500,Vendor B,2025-04-10\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn counterparty_match_is_case_insensitive() {
        let table = parse(
            "amount,vendor,date\n\
             1000,Acme Supplies,2025-05-01\n\
             1000,ACME SUPPLIES,2025-05-01\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].row_count, 2);
    }

    #[test]
    fn amount_with_currency_symbol_and_commas_normalises_for_match() {
        let table = parse(
            "amount,vendor,date\n\
             \"$1,250.00\",Foo,2025-06-01\n\
             1250.00,Foo,2025-06-01\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].amount_cents, 125_000);
    }

    #[test]
    fn zero_amount_rows_are_skipped_not_grouped() {
        // A population of 10 zero-amount rows to the same vendor on the
        // same date shouldn't produce a single massive "duplicate zeros"
        // exception — zeros get skipped at parse time.
        let table = parse(
            "amount,vendor,date\n\
             0,Foo,2025-07-01\n\
             0,Foo,2025-07-01\n\
             0,Foo,2025-07-01\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.rows_skipped_zero, 3);
    }

    #[test]
    fn unparseable_amount_rows_are_counted_but_not_grouped() {
        let table = parse(
            "amount,vendor,date\n\
             not-a-number,Foo,2025-01-01\n\
             500,Foo,2025-01-01\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert!(report.exceptions.is_empty());
        assert_eq!(report.rows_skipped_unparseable, 1);
    }

    #[test]
    fn missing_counterparty_or_date_counts_as_no_key() {
        let table = parse(
            "amount,vendor,date\n\
             100,,2025-01-01\n\
             100,Foo,\n\
             100,Foo,2025-01-01\n\
             100,Foo,2025-01-01\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        // First two rows skipped (missing counterparty / missing date),
        // last two rows form one duplicate group.
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.rows_skipped_no_key, 2);
    }

    #[test]
    fn sign_is_absolute_for_duplicate_detection() {
        // A reversal posted as `-500` against an original `500` is still a
        // pair worth flagging — both hit the same vendor/date at the same
        // magnitude. Parentheses-negative and CR-suffix also normalise.
        let table = parse(
            "amount,vendor,date\n\
             500,Foo,2025-08-01\n\
             -500,Foo,2025-08-01\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].row_count, 2);
    }

    #[test]
    fn mixed_date_formats_do_not_merge_across_formats() {
        // `2025-01-15` and `15/01/2025` represent the same date but the
        // rule does not bridge them — it would need a locale guess to be
        // safe. Two format-distinct rows don't group.
        let table = parse(
            "amount,vendor,date\n\
             100,Foo,2025-01-15\n\
             100,Foo,15/01/2025\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert!(report.exceptions.is_empty());
    }

    #[test]
    fn sample_rows_are_capped_at_five() {
        // Build a group of seven identical rows — report keeps only five
        // samples but all seven ordinals.
        let mut csv = String::from("amount,vendor,date\n");
        for _ in 0..7 {
            csv.push_str("100,Foo,2025-09-01\n");
        }
        let table = parse(&csv).unwrap();
        let report = run_duplicate_transactions(&table);
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].row_count, 7);
        assert_eq!(report.exceptions[0].row_ordinals.len(), 7);
        assert_eq!(
            report.exceptions[0].sample_rows.len(),
            SAMPLE_ROWS_PER_GROUP
        );
    }

    #[test]
    fn exception_order_is_deterministic_across_runs() {
        // BTreeMap over the key gives stable ordering even when groups
        // accumulate in arbitrary order in the input.
        let table = parse(
            "amount,vendor,date\n\
             300,Zulu,2025-01-03\n\
             100,Alpha,2025-01-01\n\
             300,Zulu,2025-01-03\n\
             200,Bravo,2025-01-02\n\
             100,Alpha,2025-01-01\n\
             200,Bravo,2025-01-02\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert_eq!(report.exceptions.len(), 3);
        // Sorted by (amount_cents, counterparty, date) — amount ascending.
        let amounts: Vec<i64> = report
            .exceptions
            .iter()
            .map(|e| e.amount_cents)
            .collect();
        assert_eq!(amounts, vec![10_000, 20_000, 30_000]);
    }

    #[test]
    fn header_variants_are_normalised() {
        let table = parse(
            "Transaction Amount,Vendor Name,Posting Date\n\
             1000.00,Foo,2025-10-01\n\
             1000.00,Foo,2025-10-01\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert_eq!(report.exceptions.len(), 1);
    }

    #[test]
    fn missing_amount_column_skips_every_row() {
        let table = parse(
            "vendor,date,description\n\
             Foo,2025-01-01,thing\n\
             Foo,2025-01-01,thing\n",
        )
        .unwrap();
        let report = run_duplicate_transactions(&table);
        assert_eq!(report.rows_considered, 2);
        assert_eq!(report.rows_skipped_unparseable, 2);
        assert!(report.exceptions.is_empty());
    }
}
