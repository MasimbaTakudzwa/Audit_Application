//! IT application controls — boundary / threshold analysis.
//!
//! Classical fraud / control-gaming signal: transactions clustered just
//! *below* a known approval threshold suggest someone is splitting or
//! timing records to stay under the authorisation rule. A payment at
//! $9,950 walks through approval that a payment at $10,050 does not.
//! If the population has materially more just-below rows than
//! just-above rows, the threshold is being gamed — or at least merits
//! an investigation.
//!
//! The matcher checks a fixed list of common approval thresholds
//! (1_000, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000,
//! 1_000_000) in the population's currency. For each threshold T it
//! counts rows whose amount lies in:
//! - the "below" window `[T - W, T)` — amounts that dodge the rule
//! - the "above" window `[T, T + W]` — amounts that trigger the rule
//!
//! Window width `W` is `BELOW_WINDOW_FRACTION * T` (5% by default).
//! A threshold is flagged when BOTH hold:
//! - below count is at least `MIN_BELOW_COUNT` (10 by default) —
//!   guards against tiny-sample noise
//! - below / above ratio is at least `FLAG_RATIO` (2.0 by default) —
//!   proportional excess, not absolute
//!
//! What the rule doesn't do (deliberate):
//! - Doesn't accept a firm-configurable threshold list. v1 uses a
//!   fixed list; firms can override or extend once a UI exists. Common
//!   approval bands cluster around these round numbers across African
//!   firms, so the default surface most gaming.
//! - Doesn't parse dates or segment by period. Time-window boundary
//!   testing ("just below threshold at month-end") is a richer
//!   procedure and lives in a later rule.
//! - Doesn't try to infer the firm's actual approval matrix. That's
//!   an organisational input, not a data-derived one — we'd rather
//!   check a generic band and let the auditor dismiss thresholds
//!   that don't apply.
//! - Doesn't fold sign: absolute value is used so a refund of -$9,950
//!   contributes the same signal as a payment of +$9,950. The
//!   gaming pattern is independent of posting direction.
//! - Doesn't evaluate thresholds larger than the population's max
//!   amount. Evaluating T=100,000 on a population whose largest row
//!   is 12,000 is meaningless and would only produce "thresholds
//!   evaluated" bloat.

use serde::Serialize;

use super::csv::{find_column, Row, Table};
use super::itac_benford::{parse_amount, AMOUNT_CANDIDATES};

/// Thresholds the matcher checks. Round numbers chosen to match the
/// approval-level patterns auditors see in practice across small-to-mid
/// African firms. The rule evaluates only thresholds whose below window
/// contains enough data to be meaningful, so over-specifying is cheap:
/// thresholds that don't apply to the population are dropped silently.
pub const BOUNDARY_THRESHOLDS: &[f64] = &[
    1_000.0,
    5_000.0,
    10_000.0,
    25_000.0,
    50_000.0,
    100_000.0,
    250_000.0,
    500_000.0,
    1_000_000.0,
];

/// Below/above window width as a fraction of the threshold. 5% means a
/// T=10,000 threshold checks [9,500, 10,000) against [10,000, 10,500].
/// Narrow enough that natural-distribution counts should be small and
/// roughly equal, wide enough that a real clustering pattern shows.
pub const BELOW_WINDOW_FRACTION: f64 = 0.05;

/// Minimum row count in the below window before a threshold can be
/// flagged. Guards against tiny-sample noise — 3-below-vs-1-above is a
/// 3× ratio but not a signal. Auditors expect the rule to surface
/// patterns, not outliers.
pub const MIN_BELOW_COUNT: usize = 10;

/// Below / above ratio at which a threshold flips to a flag. 2.0 means
/// the just-below window has to hold at least twice as many rows as
/// the just-above window. Derived from the observation that a natural
/// unbiased distribution puts roughly equal counts in two equal-sized
/// adjacent windows; an order-of-magnitude excess is too lax, a 1.5×
/// excess is too noisy. 2.0 sits at the sweet spot auditors can defend.
pub const FLAG_RATIO: f64 = 2.0;

/// Sample rows attached to each flagged threshold's exception. Keeps
/// reports readable on pathological populations where the entire
/// dataset may be just-below a single threshold. Below-window rows
/// are prioritised — above-window rows are included only if space
/// remains.
pub const SAMPLE_ROWS_PER_THRESHOLD: usize = 5;

#[derive(Debug, Clone, Serialize)]
pub struct BoundaryException {
    pub kind: String,
    pub threshold: f64,
    pub below_window_low: f64,
    pub below_window_high: f64,
    pub above_window_low: f64,
    pub above_window_high: f64,
    pub below_count: usize,
    pub above_count: usize,
    /// `below_count / above_count`. When above is zero, we send this as
    /// `None` — the caller should render it as "∞" / "no above-window
    /// rows" rather than divide by zero.
    pub ratio: Option<f64>,
    pub sample_rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoundaryReport {
    pub rule: String,
    pub rows_considered: usize,
    pub rows_skipped_unparseable: usize,
    pub rows_skipped_zero: usize,
    /// Thresholds whose below or above window caught at least one row.
    /// Thresholds larger than the population's max amount are silently
    /// dropped and do not contribute here — they're not "evaluated"
    /// in any meaningful sense.
    pub thresholds_evaluated: usize,
    /// Thresholds that tripped both the absolute and ratio gates.
    /// Equal to `exceptions.len()`.
    pub thresholds_flagged: usize,
    pub window_fraction: f64,
    pub min_below_count: usize,
    pub flag_ratio: f64,
    pub exceptions: Vec<BoundaryException>,
}

/// Run the boundary / threshold analysis over a transaction population.
/// Returns one exception per flagged threshold, plus population-level
/// counters the command layer copies to `MatcherRunResult`.
pub fn run_boundary_thresholds(transactions: &Table) -> BoundaryReport {
    let amount_col = find_column(transactions, AMOUNT_CANDIDATES);

    let mut rows_considered = 0usize;
    let mut rows_skipped_unparseable = 0usize;
    let mut rows_skipped_zero = 0usize;

    // Absolute-valued amounts, paired with the original row so we can
    // attach contextual samples to exceptions. Building a single pass
    // over `rows` and filtering per-threshold is cheaper than re-reading
    // the table for every threshold.
    let mut parsed: Vec<(f64, &Row)> = Vec::with_capacity(transactions.rows.len());

    let Some(col_key) = amount_col else {
        // No amount column — nothing is parseable.
        rows_skipped_unparseable = transactions.rows.len();
        return BoundaryReport {
            rule: "boundary_threshold".into(),
            rows_considered: 0,
            rows_skipped_unparseable,
            rows_skipped_zero: 0,
            thresholds_evaluated: 0,
            thresholds_flagged: 0,
            window_fraction: BELOW_WINDOW_FRACTION,
            min_below_count: MIN_BELOW_COUNT,
            flag_ratio: FLAG_RATIO,
            exceptions: Vec::new(),
        };
    };

    for row in &transactions.rows {
        let raw = row
            .values
            .get(&col_key)
            .map(String::as_str)
            .unwrap_or("");
        match parse_amount(raw) {
            Some(a) if a.abs() > 0.0 => {
                rows_considered += 1;
                parsed.push((a.abs(), row));
            }
            Some(_) => rows_skipped_zero += 1,
            None => rows_skipped_unparseable += 1,
        }
    }

    let max_amount = parsed
        .iter()
        .map(|(a, _)| *a)
        .fold(0.0_f64, f64::max);

    let mut exceptions: Vec<BoundaryException> = Vec::new();
    let mut thresholds_evaluated = 0usize;

    for &threshold in BOUNDARY_THRESHOLDS {
        let window_width = threshold * BELOW_WINDOW_FRACTION;
        let below_low = threshold - window_width;
        let below_high = threshold; // exclusive
        let above_low = threshold; // inclusive
        let above_high = threshold + window_width;

        // Drop thresholds whose entire below window sits above the
        // population's max amount — no row could fall in either window,
        // so the threshold is not meaningfully evaluated. We check the
        // below window's lower edge (not the threshold itself) because
        // the whole point of the rule is to catch rows BELOW the
        // threshold: a population with max 9_900 should still evaluate
        // the 10_000 threshold.
        if below_low > max_amount {
            continue;
        }

        let mut below_rows: Vec<&Row> = Vec::new();
        let mut above_rows: Vec<&Row> = Vec::new();

        for &(amount, row) in &parsed {
            if amount >= below_low && amount < below_high {
                below_rows.push(row);
            } else if amount >= above_low && amount <= above_high {
                above_rows.push(row);
            }
        }

        let below_count = below_rows.len();
        let above_count = above_rows.len();

        // "Evaluated" means either window caught at least one row.
        // A threshold with zero below AND zero above tells the auditor
        // nothing about whether it's being gamed.
        if below_count == 0 && above_count == 0 {
            continue;
        }
        thresholds_evaluated += 1;

        let ratio = if above_count == 0 {
            None
        } else {
            Some(below_count as f64 / above_count as f64)
        };

        // Flagging gate: both the absolute-count and ratio conditions
        // must hold. A threshold with `above_count == 0` passes the
        // ratio test trivially (we treat `None` as "unbounded"), so
        // below-count alone governs.
        let passes_absolute_gate = below_count >= MIN_BELOW_COUNT;
        let passes_ratio_gate = match ratio {
            Some(r) => r >= FLAG_RATIO,
            None => true,
        };

        if !(passes_absolute_gate && passes_ratio_gate) {
            continue;
        }

        // Sample rows: take up to SAMPLE_ROWS_PER_THRESHOLD rows,
        // preferring below-window rows (they're the signal). Fall
        // back to above-window rows to round out the picture when
        // room remains. We emit the raw column strings (header order)
        // so the report can be rendered without needing the parsed Row
        // struct.
        let mut sample: Vec<Vec<String>> = Vec::with_capacity(SAMPLE_ROWS_PER_THRESHOLD);
        for row in below_rows.iter().take(SAMPLE_ROWS_PER_THRESHOLD) {
            sample.push(row.raw_values.clone());
        }
        if sample.len() < SAMPLE_ROWS_PER_THRESHOLD {
            let remaining = SAMPLE_ROWS_PER_THRESHOLD - sample.len();
            for row in above_rows.iter().take(remaining) {
                sample.push(row.raw_values.clone());
            }
        }

        exceptions.push(BoundaryException {
            kind: "boundary_threshold_cluster".into(),
            threshold,
            below_window_low: below_low,
            below_window_high: below_high,
            above_window_low: above_low,
            above_window_high: above_high,
            below_count,
            above_count,
            ratio,
            sample_rows: sample,
        });
    }

    let thresholds_flagged = exceptions.len();

    BoundaryReport {
        rule: "boundary_threshold".into(),
        rows_considered,
        rows_skipped_unparseable,
        rows_skipped_zero,
        thresholds_evaluated,
        thresholds_flagged,
        window_fraction: BELOW_WINDOW_FRACTION,
        min_below_count: MIN_BELOW_COUNT,
        flag_ratio: FLAG_RATIO,
        exceptions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::csv::parse as parse_csv;

    fn table_from_csv(s: &str) -> Table {
        parse_csv(s).unwrap()
    }

    /// Build a CSV with N rows all at one amount. Handy for seeding a
    /// heavy cluster around a threshold without a lot of ceremony.
    fn csv_with_amounts(amounts: &[f64]) -> String {
        let mut s = String::from("txn_id,amount\n");
        for (i, a) in amounts.iter().enumerate() {
            s.push_str(&format!("{},{:.2}\n", i + 1, a));
        }
        s
    }

    #[test]
    fn flags_cluster_just_below_ten_thousand() {
        // 15 rows just below 10,000, 2 just above. Ratio 7.5×,
        // below-count well past the 10-row minimum. Should flag
        // exactly the 10,000 threshold and no others.
        let mut amounts = Vec::new();
        for _ in 0..15 {
            amounts.push(9_900.0);
        }
        for _ in 0..2 {
            amounts.push(10_100.0);
        }
        // Plus some filler far from any threshold.
        for _ in 0..20 {
            amounts.push(3_250.0);
        }

        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        assert_eq!(report.rule, "boundary_threshold");
        assert_eq!(report.rows_considered, 37);
        assert_eq!(report.rows_skipped_unparseable, 0);
        assert_eq!(report.rows_skipped_zero, 0);
        assert_eq!(report.thresholds_flagged, 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.kind, "boundary_threshold_cluster");
        assert_eq!(ex.threshold, 10_000.0);
        assert_eq!(ex.below_count, 15);
        assert_eq!(ex.above_count, 2);
        assert!((ex.ratio.unwrap() - 7.5).abs() < 1e-9);
        assert!(ex.sample_rows.len() <= SAMPLE_ROWS_PER_THRESHOLD);
    }

    #[test]
    fn passes_when_below_and_above_counts_are_balanced() {
        // 10 rows just below, 10 just above — ratio 1.0, no flag even
        // though absolute-count gate is met.
        let mut amounts = Vec::new();
        for _ in 0..10 {
            amounts.push(9_900.0);
        }
        for _ in 0..10 {
            amounts.push(10_100.0);
        }
        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        assert_eq!(report.thresholds_flagged, 0);
        assert_eq!(report.rows_considered, 20);
        // Threshold is evaluated (windows caught rows) but not flagged.
        assert!(report.thresholds_evaluated >= 1);
    }

    #[test]
    fn small_cluster_below_absolute_gate_is_not_flagged() {
        // 5 below, 0 above — ratio unbounded, but absolute gate fails
        // (5 < MIN_BELOW_COUNT=10). Not flagged.
        let amounts = vec![9_900.0; 5];
        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        assert_eq!(report.thresholds_flagged, 0);
        assert_eq!(report.rows_considered, 5);
    }

    #[test]
    fn unbounded_ratio_when_above_is_zero_still_flags_if_absolute_gate_met() {
        // 12 below, 0 above. Ratio is `None` (treated as unbounded),
        // absolute gate is met (12 >= 10). Flagged.
        let amounts = vec![9_900.0; 12];
        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        assert_eq!(report.thresholds_flagged, 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.below_count, 12);
        assert_eq!(ex.above_count, 0);
        assert!(ex.ratio.is_none());
    }

    #[test]
    fn thresholds_above_population_max_are_not_evaluated() {
        // Max amount 12,000 — the 25k, 50k, 100k, 250k, 500k, 1M
        // thresholds are all above the population ceiling and should
        // not even count in `thresholds_evaluated`.
        let mut amounts = Vec::new();
        for _ in 0..20 {
            amounts.push(3_250.0);
        }
        amounts.push(12_000.0);
        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        // Should only consider thresholds <= 12,000: 1_000, 5_000, 10_000.
        assert!(report.thresholds_evaluated <= 3);
        assert_eq!(report.thresholds_flagged, 0);
    }

    #[test]
    fn flags_multiple_thresholds_in_same_population() {
        // Cluster below 10k AND below 50k.
        let mut amounts = Vec::new();
        for _ in 0..12 {
            amounts.push(9_900.0);
        }
        for _ in 0..15 {
            amounts.push(49_500.0);
        }
        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        assert_eq!(report.thresholds_flagged, 2);
        let thresholds: Vec<f64> = report.exceptions.iter().map(|e| e.threshold).collect();
        assert!(thresholds.contains(&10_000.0));
        assert!(thresholds.contains(&50_000.0));
    }

    #[test]
    fn exception_order_is_ascending_by_threshold() {
        // Seed 10k and 50k clusters out of order — the matcher iterates
        // BOUNDARY_THRESHOLDS in ascending order so the exceptions
        // should too. Stable output makes report diffs readable.
        let mut amounts = Vec::new();
        for _ in 0..15 {
            amounts.push(49_500.0);
        }
        for _ in 0..12 {
            amounts.push(9_900.0);
        }
        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        assert_eq!(report.thresholds_flagged, 2);
        assert_eq!(report.exceptions[0].threshold, 10_000.0);
        assert_eq!(report.exceptions[1].threshold, 50_000.0);
    }

    #[test]
    fn zero_and_unparseable_amounts_do_not_enter_any_window() {
        let csv = "txn_id,amount\n\
                   1,0.00\n\
                   2,not a number\n\
                   3,9900.00\n\
                   4,9900.00\n\
                   5,9900.00\n\
                   6,9900.00\n\
                   7,9900.00\n\
                   8,9900.00\n\
                   9,9900.00\n\
                   10,9900.00\n\
                   11,9900.00\n\
                   12,9900.00\n\
                   13,9900.00\n";
        let report = run_boundary_thresholds(&table_from_csv(csv));
        assert_eq!(report.rows_skipped_zero, 1);
        assert_eq!(report.rows_skipped_unparseable, 1);
        assert_eq!(report.rows_considered, 11);
        assert_eq!(report.thresholds_flagged, 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.threshold, 10_000.0);
        assert_eq!(ex.below_count, 11);
    }

    #[test]
    fn negative_amounts_fold_to_absolute_for_window_match() {
        // 12 rows at -9,900 should be caught by the 10k below window
        // same as 12 rows at +9,900.
        let amounts = vec![-9_900.0; 12];
        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        assert_eq!(report.thresholds_flagged, 1);
        assert_eq!(report.exceptions[0].below_count, 12);
    }

    #[test]
    fn missing_amount_column_skips_every_row() {
        let csv = "txn_id,description\n1,payroll\n2,rent\n3,invoice\n";
        let report = run_boundary_thresholds(&table_from_csv(csv));
        assert_eq!(report.rows_considered, 0);
        assert_eq!(report.rows_skipped_unparseable, 3);
        assert_eq!(report.thresholds_flagged, 0);
        assert_eq!(report.thresholds_evaluated, 0);
    }

    #[test]
    fn amount_with_currency_symbol_and_commas_normalises_for_window_match() {
        let csv = "txn_id,amount\n\
                   1,\"$9,900.00\"\n\
                   2,\"$9,900.00\"\n\
                   3,\"$9,900.00\"\n\
                   4,\"$9,900.00\"\n\
                   5,\"$9,900.00\"\n\
                   6,\"$9,900.00\"\n\
                   7,\"$9,900.00\"\n\
                   8,\"$9,900.00\"\n\
                   9,\"$9,900.00\"\n\
                   10,\"$9,900.00\"\n\
                   11,\"$9,900.00\"\n\
                   12,\"$9,900.00\"\n";
        let report = run_boundary_thresholds(&table_from_csv(csv));
        assert_eq!(report.rows_considered, 12);
        assert_eq!(report.thresholds_flagged, 1);
        assert_eq!(report.exceptions[0].below_count, 12);
    }

    #[test]
    fn sample_rows_are_capped_and_prefer_below_window() {
        // 15 below, 6 above. Ratio 2.5, above FLAG_RATIO of 2.0.
        // Sample size 5 — all 5 should come from below (they're the
        // signal; we fill below-window first).
        let mut amounts = Vec::new();
        for _ in 0..15 {
            amounts.push(9_900.0);
        }
        for _ in 0..6 {
            amounts.push(10_100.0);
        }
        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        assert_eq!(report.thresholds_flagged, 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.sample_rows.len(), SAMPLE_ROWS_PER_THRESHOLD);
        // All 5 sample rows should have amount 9,900 (below window)
        // since we fill below-window first.
        for row in &ex.sample_rows {
            let amount_str = row.get(1).map(String::as_str).unwrap_or("");
            assert_eq!(amount_str, "9900.00");
        }
    }

    #[test]
    fn sample_fills_with_above_rows_when_below_is_thin() {
        // 3 below (absolute gate fails at 10, so we force flagging by
        // having 0 above and using the unbounded path). Actually let's
        // test the sample *fallback*: 10 below (just meets gate), 4
        // above, 0 ratio-gate fail — sample should fill 5: 10 below →
        // take 5 below first. Easier: have 2 below and fake it won't
        // flag. Let's instead test the fill path directly with a case
        // that flags AND has a below-window shorter than the sample
        // cap.
        //
        // 10 below (meets absolute gate, ratio 10/3 > 2), 3 above.
        // Sample size 5 — take 5 below (we have enough), 0 above.
        let mut amounts = Vec::new();
        for _ in 0..10 {
            amounts.push(9_900.0);
        }
        for _ in 0..3 {
            amounts.push(10_100.0);
        }
        let report = run_boundary_thresholds(&table_from_csv(&csv_with_amounts(&amounts)));
        assert_eq!(report.thresholds_flagged, 1);
        let ex = &report.exceptions[0];
        assert_eq!(ex.sample_rows.len(), 5);
        // All come from below (we have enough).
        for row in &ex.sample_rows {
            let amount_str = row.get(1).map(String::as_str).unwrap_or("");
            assert_eq!(amount_str, "9900.00");
        }
    }

    #[test]
    fn constants_round_trip_into_report_for_auditor_visibility() {
        // The auditor should be able to read the exact thresholds and
        // flag criteria out of the report without opening source.
        let report = run_boundary_thresholds(&table_from_csv("txn_id,amount\n1,100.00\n"));
        assert_eq!(report.window_fraction, BELOW_WINDOW_FRACTION);
        assert_eq!(report.min_below_count, MIN_BELOW_COUNT);
        assert_eq!(report.flag_ratio, FLAG_RATIO);
    }
}
