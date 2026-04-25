//! IT application controls — Benford's-Law first-digit analysis.
//!
//! Classical analytical procedure: the leading digit of naturally occurring
//! financial amounts follows Benford's distribution — P(d) = log10(1 + 1/d)
//! for d ∈ {1..9}. Populations that deviate materially from this distribution
//! often indicate fabrication, heavy rounding to internal thresholds, or data
//! manipulation. This matcher reads a transaction CSV, pulls first digits,
//! and compares observed frequencies against the expected Benford curve.
//!
//! Caveats the rule encodes:
//! - **Minimum sample size**: chi-square on Benford is unreliable below a few
//!   hundred observations. We require 300+ usable digit rows; below that the
//!   rule emits a single `population_too_small` exception and does no
//!   digit-level work. The threshold is a deliberate round number, not a
//!   derivation — auditors override with judgement.
//! - **Scope**: first-digit only. Second-digit and first-two-digits tests are
//!   well-known extensions but require larger populations (10k+) to be
//!   meaningful. Not shipped in v1; the matcher can be extended later without
//!   breaking the existing report shape.
//! - **Zero and unparseable amounts are skipped**, not flagged. A zero amount
//!   has no leading non-zero digit; an unparseable cell is a data-quality
//!   issue, not a Benford anomaly.
//! - **Negative amounts are folded via absolute value.** A refund of -$150
//!   contributes a first digit of 1 to the distribution, same as +$150. This
//!   matches the published convention (Nigrini 2012).
//!
//! Output:
//! - `chi_square`, compared against the 8-df critical value at α=0.05
//!   (15.507). Auditor reads this from the detail JSON.
//! - Per-digit exceptions for digits whose absolute frequency deviation from
//!   Benford exceeds `DIGIT_DEVIATION_THRESHOLD`. These are the "focus
//!   digits" for follow-up work — not in themselves a finding, but a
//!   starting list.
//! - A pass/fail outcome: fail if any per-digit deviation exceeds threshold
//!   OR the chi-square exceeds the critical value.

use serde::Serialize;

use super::csv::{find_column, Table};

/// Columns that plausibly hold a transaction's monetary amount. Shared
/// across ITAC matchers — Benford reads it for the leading digit, the
/// duplicate-detection rule reads it for the grouping key.
pub(super) const AMOUNT_CANDIDATES: &[&str] = &[
    "amount",
    "value",
    "transaction_amount",
    "transactionamount",
    "gross_amount",
    "grossamount",
    "net_amount",
    "netamount",
    "debit",
    "credit",
    "total",
    "sum",
    "posted_amount",
    "postedamount",
    "invoice_amount",
    "invoiceamount",
];

/// Expected first-digit frequencies under Benford's Law.
/// `EXPECTED[d-1]` = log10(1 + 1/d).
pub const EXPECTED_FREQUENCIES: [f64; 9] = [
    0.301029995663981_f64, // 1
    0.176091259055681_f64, // 2
    0.124938736608299_f64, // 3
    0.096910013008056_f64, // 4
    0.079181246047624_f64, // 5
    0.066946789630613_f64, // 6
    0.057991946977686_f64, // 7
    0.051152522447381_f64, // 8
    0.045757490560675_f64, // 9
];

/// 8-df chi-square critical value at α=0.05.
pub const CHI_SQUARE_CRITICAL_DF8_ALPHA05: f64 = 15.507;

/// Per-digit absolute-frequency deviation above which we emit an exception.
/// 0.02 = 2 percentage points. Round, defensible, and lets auditors adjust.
pub const DIGIT_DEVIATION_THRESHOLD: f64 = 0.02;

/// Minimum usable digit rows needed to run the chi-square test. Below this
/// we emit a single `population_too_small` exception instead.
pub const MIN_DIGIT_ROWS: usize = 300;

/// Report for a Benford first-digit test.
#[derive(Debug, Clone, Serialize)]
pub struct BenfordReport {
    pub rule: String,
    /// All rows in the input table (including those we later skip).
    pub rows_considered: usize,
    /// Rows whose amount cell could not be parsed into a number.
    pub rows_skipped_unparseable: usize,
    /// Rows whose parsed amount was exactly zero.
    pub rows_skipped_zero: usize,
    /// Rows that survived every filter and contributed to the digit counts.
    pub digit_rows_evaluated: usize,
    /// Observed counts for digits 1..9.
    pub observed_counts: [u32; 9],
    /// Observed frequencies (counts / digit_rows_evaluated).
    pub observed_frequencies: [f64; 9],
    /// Expected frequencies under Benford's Law.
    pub expected_frequencies: [f64; 9],
    /// Chi-square goodness-of-fit statistic. `None` when the population is
    /// too small to compute a meaningful test.
    pub chi_square: Option<f64>,
    /// Chi-square critical value at α=0.05, df=8. Present so the detail JSON
    /// carries its own interpretation key.
    pub chi_square_critical: f64,
    /// Minimum-population threshold applied.
    pub min_digit_rows: usize,
    /// Per-digit deviation threshold applied.
    pub digit_deviation_threshold: f64,
    pub exceptions: Vec<BenfordException>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenfordException {
    pub kind: String,
    /// The first digit (1..9), or `None` for the population-too-small case.
    pub digit: Option<u8>,
    pub observed_frequency: Option<f64>,
    pub expected_frequency: Option<f64>,
    /// Signed deviation (`observed - expected`). Positive means over-represented.
    pub deviation: Option<f64>,
    pub detail: String,
}

/// Run a Benford first-digit analysis over a transaction table.
pub fn run_benford_first_digit(transactions: &Table) -> BenfordReport {
    let amount_col = find_column(transactions, AMOUNT_CANDIDATES);

    let rows_considered = transactions.rows.len();
    let mut counts = [0u32; 9];
    let mut skipped_unparseable = 0usize;
    let mut skipped_zero = 0usize;

    for row in &transactions.rows {
        let raw = match amount_col
            .as_ref()
            .and_then(|c| row.values.get(c))
            .map(|s| s.trim().to_string())
        {
            Some(s) if !s.is_empty() => s,
            _ => {
                skipped_unparseable += 1;
                continue;
            }
        };

        let Some(amount) = parse_amount(&raw) else {
            skipped_unparseable += 1;
            continue;
        };

        if amount.abs() < f64::EPSILON {
            skipped_zero += 1;
            continue;
        }

        let Some(digit) = leading_digit(amount.abs()) else {
            // leading_digit only returns None for pathological inputs
            // (NaN, ±∞) which parse_amount has already filtered, but be
            // defensive.
            skipped_unparseable += 1;
            continue;
        };
        counts[(digit - 1) as usize] += 1;
    }

    let digit_rows_evaluated: usize = counts.iter().map(|c| *c as usize).sum();
    let observed_frequencies = compute_frequencies(&counts, digit_rows_evaluated);

    let (chi_square, exceptions) = if digit_rows_evaluated < MIN_DIGIT_ROWS {
        let ex = BenfordException {
            kind: "population_too_small".into(),
            digit: None,
            observed_frequency: None,
            expected_frequency: None,
            deviation: None,
            detail: format!(
                "Only {} rows contributed a first digit; at least {} are needed for the chi-square test to be meaningful",
                digit_rows_evaluated, MIN_DIGIT_ROWS
            ),
        };
        (None, vec![ex])
    } else {
        let chi2 = chi_square_statistic(&counts, digit_rows_evaluated);
        let mut exs = Vec::new();
        for (idx, (&obs, &exp)) in observed_frequencies
            .iter()
            .zip(EXPECTED_FREQUENCIES.iter())
            .enumerate()
        {
            let deviation = obs - exp;
            if deviation.abs() > DIGIT_DEVIATION_THRESHOLD {
                exs.push(BenfordException {
                    kind: "digit_frequency_anomaly".into(),
                    digit: Some((idx + 1) as u8),
                    observed_frequency: Some(obs),
                    expected_frequency: Some(exp),
                    deviation: Some(deviation),
                    detail: format!(
                        "Digit {} observed at {:.1}%, expected {:.1}% under Benford ({:+.1}pp)",
                        idx + 1,
                        obs * 100.0,
                        exp * 100.0,
                        deviation * 100.0
                    ),
                });
            }
        }
        // Overall-fit exception: if the chi-square is above critical and no
        // per-digit rule fired, still flag the population as a whole.
        if chi2 > CHI_SQUARE_CRITICAL_DF8_ALPHA05 && exs.is_empty() {
            exs.push(BenfordException {
                kind: "chi_square_exceeds_critical".into(),
                digit: None,
                observed_frequency: None,
                expected_frequency: None,
                deviation: None,
                detail: format!(
                    "Chi-square {:.2} exceeds 8-df critical {:.3} at α=0.05",
                    chi2, CHI_SQUARE_CRITICAL_DF8_ALPHA05
                ),
            });
        }
        (Some(chi2), exs)
    };

    BenfordReport {
        rule: "benford_first_digit".into(),
        rows_considered,
        rows_skipped_unparseable: skipped_unparseable,
        rows_skipped_zero: skipped_zero,
        digit_rows_evaluated,
        observed_counts: counts,
        observed_frequencies,
        expected_frequencies: EXPECTED_FREQUENCIES,
        chi_square,
        chi_square_critical: CHI_SQUARE_CRITICAL_DF8_ALPHA05,
        min_digit_rows: MIN_DIGIT_ROWS,
        digit_deviation_threshold: DIGIT_DEVIATION_THRESHOLD,
        exceptions,
    }
}

fn compute_frequencies(counts: &[u32; 9], total: usize) -> [f64; 9] {
    let mut out = [0f64; 9];
    if total == 0 {
        return out;
    }
    let total_f = total as f64;
    for (i, c) in counts.iter().enumerate() {
        out[i] = (*c as f64) / total_f;
    }
    out
}

fn chi_square_statistic(counts: &[u32; 9], total: usize) -> f64 {
    let total_f = total as f64;
    let mut chi = 0.0;
    for (i, expected_freq) in EXPECTED_FREQUENCIES.iter().enumerate() {
        let expected_count = total_f * expected_freq;
        let diff = counts[i] as f64 - expected_count;
        chi += (diff * diff) / expected_count;
    }
    chi
}

/// Parse a string as a monetary amount. Strips:
/// - Leading/trailing whitespace
/// - ISO currency codes (`USD`, `EUR`, `GBP`, `ZAR`, `ZWL`, `NGN`, `KES`,
///   `UGX`, `TZS`, `RWF`, `BWP`, `XAF`, `XOF`, `MWK`, `ZMW`)
/// - Currency symbols (`$`, `£`, `€`, `¥`, `₦`, `R`, `Z$`)
/// - Thousands-separator commas, spaces, and apostrophes
/// - Parentheses as negative-sign indicators (accounting convention: `(150)` = `-150`)
/// - A trailing `CR`/`DR` indicator (credit/debit) — `CR` negates, `DR` keeps sign
///
/// Exposed `pub(super)` so sibling ITAC matchers (duplicate detection,
/// boundary analysis, etc.) can share the same currency-handling rules as
/// Benford — keeps the messy-input normalisation in one place.
pub(super) fn parse_amount(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Paren-wrapped = negative.
    let (paren_neg, after_paren) = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        (true, &trimmed[1..trimmed.len() - 1])
    } else {
        (false, trimmed)
    };

    let upper = after_paren.to_ascii_uppercase();
    let mut cr_neg = false;
    let stripped_crdr = if let Some(rest) = upper.strip_suffix(" CR") {
        cr_neg = true;
        rest.trim_end().to_string()
    } else if let Some(rest) = upper.strip_suffix("CR") {
        cr_neg = true;
        rest.trim_end().to_string()
    } else if let Some(rest) = upper.strip_suffix(" DR") {
        rest.trim_end().to_string()
    } else if let Some(rest) = upper.strip_suffix("DR") {
        rest.trim_end().to_string()
    } else {
        upper.clone()
    };

    const CURRENCY_TOKENS: &[&str] = &[
        "USD", "EUR", "GBP", "ZAR", "ZWL", "ZWG", "NGN", "KES", "UGX", "TZS", "RWF", "BWP", "XAF",
        "XOF", "MWK", "ZMW", "EGP", "MAD", "GHS",
    ];
    let mut s = stripped_crdr;
    for tok in CURRENCY_TOKENS {
        if let Some(rest) = s.strip_prefix(tok) {
            s = rest.trim().to_string();
        } else if let Some(rest) = s.strip_suffix(tok) {
            s = rest.trim().to_string();
        }
    }

    // Strip single-char currency symbols and thousands separators.
    let cleaned: String = s
        .chars()
        .filter(|c| !matches!(c, '$' | '£' | '€' | '¥' | '₦' | ',' | ' ' | '\'' | 'R' | 'Z'))
        .collect();

    let parsed: f64 = cleaned.parse().ok()?;
    if !parsed.is_finite() {
        return None;
    }
    let signed = if paren_neg || cr_neg { -parsed } else { parsed };
    Some(signed)
}

/// Leading non-zero decimal digit of |value|. Returns `None` for non-finite
/// or zero input.
fn leading_digit(value: f64) -> Option<u8> {
    if !value.is_finite() || value == 0.0 {
        return None;
    }
    let mut v = value.abs();
    // Scale into [1, 10).
    while v >= 10.0 {
        v /= 10.0;
    }
    while v < 1.0 {
        v *= 10.0;
    }
    let d = v.trunc() as u8;
    if (1..=9).contains(&d) {
        Some(d)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::csv::parse;

    fn table(csv: &str) -> Table {
        parse(csv).unwrap()
    }

    #[test]
    fn leading_digit_handles_small_and_large_numbers() {
        assert_eq!(leading_digit(1.0), Some(1));
        assert_eq!(leading_digit(2.5), Some(2));
        assert_eq!(leading_digit(12345.0), Some(1));
        assert_eq!(leading_digit(98765.0), Some(9));
        assert_eq!(leading_digit(0.0043), Some(4));
        assert_eq!(leading_digit(0.0), None);
        assert_eq!(leading_digit(-157.2), Some(1));
        assert_eq!(leading_digit(f64::NAN), None);
        assert_eq!(leading_digit(f64::INFINITY), None);
    }

    #[test]
    fn parse_amount_strips_currency_and_thousands_separators() {
        assert_eq!(parse_amount("1234.50"), Some(1234.50));
        assert_eq!(parse_amount(" $1,234.50 "), Some(1234.50));
        assert_eq!(parse_amount("£1 234.50"), Some(1234.50));
        assert_eq!(parse_amount("USD 150.00"), Some(150.0));
        assert_eq!(parse_amount("150.00 USD"), Some(150.0));
        assert_eq!(parse_amount("ZAR 1,500"), Some(1500.0));
        assert_eq!(parse_amount("(250.00)"), Some(-250.0));
        assert_eq!(parse_amount("1,000.00 CR"), Some(-1000.0));
        assert_eq!(parse_amount("2,500.00 DR"), Some(2500.0));
    }

    #[test]
    fn parse_amount_rejects_non_numeric() {
        assert_eq!(parse_amount(""), None);
        assert_eq!(parse_amount("   "), None);
        assert_eq!(parse_amount("N/A"), None);
        assert_eq!(parse_amount("pending"), None);
    }

    /// Build a CSV whose first-digit distribution is approximately Benford.
    /// Populates digits 1..9 in Benford-proportional quantities at n=1000.
    fn benford_like_csv() -> String {
        let targets: [usize; 9] = [301, 176, 125, 97, 79, 67, 58, 51, 46]; // ≈ 1000
        let mut s = String::from("id,amount\n");
        let mut idx = 1;
        // Exemplar amounts whose first digits are 1..9. Values span a few
        // orders of magnitude so the matcher's leading-digit logic is
        // exercised properly (not always 1xx or 2xx).
        let exemplars: [&str; 9] = [
            "123.45", "2345.67", "34.50", "4500.00", "5.75", "64.20", "7123.99", "850.00",
            "9.99",
        ];
        for (digit_i, count) in targets.iter().enumerate() {
            for _ in 0..*count {
                s.push_str(&format!("{},{}\n", idx, exemplars[digit_i]));
                idx += 1;
            }
        }
        s
    }

    #[test]
    fn benford_like_population_passes() {
        let t = table(&benford_like_csv());
        let report = run_benford_first_digit(&t);
        assert_eq!(report.rule, "benford_first_digit");
        assert!(report.digit_rows_evaluated >= MIN_DIGIT_ROWS);
        assert_eq!(report.rows_skipped_zero, 0);
        assert_eq!(report.rows_skipped_unparseable, 0);
        assert!(report.chi_square.is_some());
        assert!(report.chi_square.unwrap() < CHI_SQUARE_CRITICAL_DF8_ALPHA05);
        assert!(
            report.exceptions.is_empty(),
            "expected no exceptions, got {:?}",
            report.exceptions
        );
    }

    #[test]
    fn uniform_first_digits_are_flagged_as_exceptions() {
        // Uniform distribution: every digit appears equally often. This is
        // the classic "rounded / fabricated" signature Benford catches.
        let per_digit = 50usize;
        let mut s = String::from("id,amount\n");
        let exemplars: [&str; 9] = [
            "123", "234", "345", "456", "567", "678", "789", "890", "999",
        ];
        let mut idx = 1;
        for exemplar in &exemplars {
            for _ in 0..per_digit {
                s.push_str(&format!("{},{}\n", idx, exemplar));
                idx += 1;
            }
        }
        let t = table(&s);
        let report = run_benford_first_digit(&t);
        assert_eq!(report.digit_rows_evaluated, per_digit * 9);
        assert!(report.chi_square.is_some());
        // Uniform is catastrophically non-Benford — chi-square should be
        // well above the critical value.
        assert!(
            report.chi_square.unwrap() > CHI_SQUARE_CRITICAL_DF8_ALPHA05,
            "chi-square {} should exceed critical {}",
            report.chi_square.unwrap(),
            CHI_SQUARE_CRITICAL_DF8_ALPHA05
        );
        // Digit 1 is overrepresented in Benford (30.1%) but here sits at
        // 11.1% — well above the 2pp threshold. At least one digit exception
        // must fire.
        assert!(
            report
                .exceptions
                .iter()
                .any(|e| e.kind == "digit_frequency_anomaly"),
            "expected per-digit exceptions, got {:?}",
            report.exceptions
        );
    }

    #[test]
    fn small_population_emits_population_too_small_exception() {
        let mut s = String::from("id,amount\n");
        for i in 1..=50 {
            s.push_str(&format!("{},123.00\n", i));
        }
        let t = table(&s);
        let report = run_benford_first_digit(&t);
        assert!(report.chi_square.is_none());
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].kind, "population_too_small");
    }

    #[test]
    fn zero_and_unparseable_amounts_are_skipped_not_flagged() {
        let mut s = String::from("id,amount\n");
        s.push_str("1,0.00\n");
        s.push_str("2,N/A\n");
        s.push_str("3,pending\n");
        s.push_str("4,\n");
        for i in 5..=504 {
            // Benford-like baseline so we don't trip the small-population branch.
            s.push_str(&format!("{},{}\n", i, 100 + i));
        }
        let t = table(&s);
        let report = run_benford_first_digit(&t);
        assert_eq!(report.rows_considered, 504);
        assert_eq!(report.rows_skipped_zero, 1);
        assert_eq!(report.rows_skipped_unparseable, 3);
        assert_eq!(report.digit_rows_evaluated, 500);
    }

    #[test]
    fn missing_amount_column_skips_every_row() {
        let csv = "id,description\n1,apple\n2,banana\n3,cherry\n";
        let t = table(csv);
        let report = run_benford_first_digit(&t);
        assert_eq!(report.rows_considered, 3);
        assert_eq!(report.rows_skipped_unparseable, 3);
        assert_eq!(report.digit_rows_evaluated, 0);
        // Population too small, with zero rows → single pop-too-small exception.
        assert_eq!(report.exceptions.len(), 1);
        assert_eq!(report.exceptions[0].kind, "population_too_small");
    }

    #[test]
    fn negative_and_paren_amounts_contribute_to_distribution() {
        // Half of the 500-row population uses parenthesised (accounting
        // negative) amounts, half uses bare positives. Leading digits should
        // come from the absolute values, so the distribution should look
        // Benford-like (unaltered by the sign treatment).
        let mut s = String::from("id,amount\n");
        let exemplars: [&str; 9] = [
            "123.45", "2345.67", "34.50", "4500.00", "5.75", "64.20", "7123.99", "850.00",
            "9.99",
        ];
        let targets: [usize; 9] = [151, 88, 63, 49, 40, 34, 29, 26, 23]; // ≈ 500, roughly Benford
        let mut idx = 1;
        for (digit_i, count) in targets.iter().enumerate() {
            for j in 0..*count {
                let amt = if j % 2 == 0 {
                    exemplars[digit_i].to_string()
                } else {
                    format!("({})", exemplars[digit_i])
                };
                s.push_str(&format!("{},{}\n", idx, amt));
                idx += 1;
            }
        }
        let t = table(&s);
        let report = run_benford_first_digit(&t);
        assert!(report.chi_square.is_some());
        assert!(report.digit_rows_evaluated >= MIN_DIGIT_ROWS);
        // Observed first-digit totals should be above zero for every digit.
        for (i, c) in report.observed_counts.iter().enumerate() {
            assert!(*c > 0, "digit {} has zero observations", i + 1);
        }
    }

    #[test]
    fn header_variants_are_resolved() {
        // The parser lowercases headers, so `Transaction_Amount` maps to
        // `transaction_amount` which is in our candidate list.
        let mut s = String::from("txn_id,Transaction_Amount\n");
        for i in 1..=500 {
            s.push_str(&format!("{},{}\n", i, 100 + i));
        }
        let t = table(&s);
        let report = run_benford_first_digit(&t);
        assert!(report.digit_rows_evaluated >= MIN_DIGIT_ROWS);
        assert!(report.chi_square.is_some());
    }
}
