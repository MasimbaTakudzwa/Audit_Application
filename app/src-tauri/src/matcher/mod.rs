//! Deterministic rule-based matchers.
//!
//! Per CLAUDE.md, the default audit automation tier is rules, not ML or LLMs.
//! Each submodule exposes a pure function that takes parsed population rows
//! and produces a list of exceptions. The caller (usually a command handler)
//! wraps this with CSV parsing, blob decryption, and `TestResult`
//! persistence, but the matcher itself knows nothing about storage — which
//! keeps it trivially testable.

pub mod access_review;
pub mod backup;
pub mod change_management;
pub mod csv;
pub mod itac_benford;
pub mod itac_duplicates;
