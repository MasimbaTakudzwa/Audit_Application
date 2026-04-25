//! Minimal CSV parser.
//!
//! Handles the cases that script-generated AD, Entra, HR, and payroll exports
//! actually produce: UTF-8 text, comma separator, optionally double-quoted
//! fields (with `""` to embed a quote), a header row, `\n` or `\r\n` line
//! endings. A stray UTF-8 BOM on the first byte is tolerated.
//!
//! NOT handled: alternate delimiters (`;`, `\t`), embedded newlines inside
//! quoted fields, single-quoted fields. If a client sends us one of those we
//! add it when we see it — this is an internal preprocessing path, not a
//! public-facing CSV endpoint.

use std::collections::HashMap;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct Row {
    /// Source ordinal starting at 1 for the first data row (header is not counted).
    pub ordinal: usize,
    /// Columns indexed by original header (lower-cased and trimmed for lookup).
    pub values: HashMap<String, String>,
    /// Columns in original header order, useful when echoing the offending row back.
    pub raw_values: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Row>,
}

pub fn parse(text: &str) -> AppResult<Table> {
    let stripped = text.trim_start_matches('\u{feff}');
    let mut lines = stripped.lines().filter(|l| !l.trim().is_empty());

    let header_line = match lines.next() {
        Some(h) => h,
        None => {
            return Ok(Table {
                headers: Vec::new(),
                rows: Vec::new(),
            })
        }
    };
    let headers: Vec<String> = split_fields(header_line)
        .into_iter()
        .map(|s| s.trim().to_string())
        .collect();
    if headers.iter().any(|h| h.is_empty()) {
        return Err(AppError::Message("CSV header contains an empty column".into()));
    }
    let header_keys: Vec<String> = headers.iter().map(|h| canonical(h)).collect();

    let mut rows = Vec::new();
    for (i, line) in lines.enumerate() {
        let fields = split_fields(line);
        let mut values = HashMap::new();
        for (idx, key) in header_keys.iter().enumerate() {
            let value = fields.get(idx).cloned().unwrap_or_default();
            values.insert(key.clone(), value);
        }
        rows.push(Row {
            ordinal: i + 1,
            values,
            raw_values: fields,
        });
    }

    Ok(Table { headers, rows })
}

fn split_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, in_quotes) {
            ('"', false) if field.is_empty() => {
                in_quotes = true;
            }
            ('"', true) => {
                if let Some('"') = chars.peek() {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            (',', false) => {
                out.push(std::mem::take(&mut field));
            }
            (ch, _) => field.push(ch),
        }
    }
    out.push(field);
    out
}

pub fn canonical(key: &str) -> String {
    key.trim().to_ascii_lowercase().replace(['_', '-', ' '], "")
}

/// Find the first header in `candidates` that appears in the table (canonical
/// comparison). Returns the matching canonical key the rows are indexed by,
/// or `None` if none of the candidates are present.
pub fn find_column<'a>(table: &'a Table, candidates: &[&str]) -> Option<String> {
    let keys: Vec<String> = table.headers.iter().map(|h| canonical(h)).collect();
    for candidate in candidates {
        let canon = canonical(candidate);
        if keys.iter().any(|k| k == &canon) {
            return Some(canon);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_csv_with_bom_and_crlf() {
        let text = "\u{feff}Email,Enabled\r\na@x.com,TRUE\r\nb@y.com,FALSE\r\n";
        let table = parse(text).unwrap();
        assert_eq!(table.headers, vec!["Email", "Enabled"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].values.get("email").unwrap(), "a@x.com");
        assert_eq!(table.rows[1].values.get("enabled").unwrap(), "FALSE");
    }

    #[test]
    fn quoted_fields_with_embedded_commas_and_quotes() {
        let text = "name,note\n\"Doe, John\",\"says \"\"hello\"\"\"\n";
        let table = parse(text).unwrap();
        assert_eq!(table.rows[0].raw_values[0], "Doe, John");
        assert_eq!(table.rows[0].raw_values[1], "says \"hello\"");
    }

    #[test]
    fn empty_headers_rejected() {
        let err = parse("a,,c\n1,2,3\n").unwrap_err();
        assert!(matches!(err, AppError::Message(_)));
    }

    #[test]
    fn find_column_matches_underscores_case_and_dashes() {
        let table = parse("User Principal Name,Account-Enabled\nu@x,true\n").unwrap();
        assert_eq!(
            find_column(&table, &["userPrincipalName", "upn"]).as_deref(),
            Some("userprincipalname")
        );
        assert_eq!(
            find_column(&table, &["AccountEnabled"]).as_deref(),
            Some("accountenabled")
        );
    }
}
