//! Library bundle loader.
//!
//! Called after `DbState::open_with_key` finishes migrations. For each bundle
//! compiled into the binary via `include_bytes!`:
//!   1. Verify its Ed25519 signature against the baked public key.
//!   2. Parse the JSON payload.
//!   3. If the `library_version` is already present in the DB, exit early.
//!   4. Otherwise, insert all rows inside a single transaction, generating
//!      UUIDs for new rows and marking any prior-version rows with the same
//!      `code` as superseded.
//!
//! Idempotent: running twice with the same bundle is a no-op. Safe to run on
//! every DB open.

use std::collections::HashMap;

use rusqlite::{params, Connection};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::verify;

/// Baseline bundles shipped with the app. Future versions are added here in
/// ascending order — the loader applies them in sequence so `superseded_by`
/// chains are built correctly.
const BUNDLE_V0_1_0: &[u8] =
    include_bytes!("../../resources/library/v0.1.0.json");
const BUNDLE_V0_1_0_SIG: &str =
    include_str!("../../resources/library/v0.1.0.json.sig");

const BUNDLE_V0_2_0: &[u8] =
    include_bytes!("../../resources/library/v0.2.0.json");
const BUNDLE_V0_2_0_SIG: &str =
    include_str!("../../resources/library/v0.2.0.json.sig");

const BUNDLE_V0_3_0: &[u8] =
    include_bytes!("../../resources/library/v0.3.0.json");
const BUNDLE_V0_3_0_SIG: &str =
    include_str!("../../resources/library/v0.3.0.json.sig");

const BUNDLE_V0_4_0: &[u8] =
    include_bytes!("../../resources/library/v0.4.0.json");
const BUNDLE_V0_4_0_SIG: &str =
    include_str!("../../resources/library/v0.4.0.json.sig");

const BUNDLE_V0_5_0: &[u8] =
    include_bytes!("../../resources/library/v0.5.0.json");
const BUNDLE_V0_5_0_SIG: &str =
    include_str!("../../resources/library/v0.5.0.json.sig");

/// Install every baseline bundle shipped with the app into `conn`. Idempotent.
pub fn install_baseline_bundles(conn: &mut Connection) -> AppResult<()> {
    install_bundle(conn, BUNDLE_V0_1_0, BUNDLE_V0_1_0_SIG)?;
    install_bundle(conn, BUNDLE_V0_2_0, BUNDLE_V0_2_0_SIG)?;
    install_bundle(conn, BUNDLE_V0_3_0, BUNDLE_V0_3_0_SIG)?;
    install_bundle(conn, BUNDLE_V0_4_0, BUNDLE_V0_4_0_SIG)?;
    install_bundle(conn, BUNDLE_V0_5_0, BUNDLE_V0_5_0_SIG)?;
    Ok(())
}

fn install_bundle(
    conn: &mut Connection,
    payload: &[u8],
    signature_hex: &str,
) -> AppResult<()> {
    verify::verify_bundle(payload, signature_hex)?;

    let bundle: Bundle = serde_json::from_slice(payload)?;

    // Idempotency: any row keyed by this version means we've already
    // installed it. Checking LibraryRisk is sufficient because every bundle
    // inserts at least one risk in the same transaction that inserts
    // everything else — you cannot be half-installed.
    let already_installed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM LibraryRisk WHERE library_version = ?1",
        params![bundle.version],
        |r| r.get(0),
    )?;
    if already_installed > 0 {
        tracing::debug!(version = %bundle.version, "library bundle already installed, skipping");
        return Ok(());
    }

    let tx = conn.transaction()?;

    let mut risk_code_to_id: HashMap<String, String> = HashMap::new();
    for risk in &bundle.risks {
        let id = Uuid::now_v7().to_string();
        risk_code_to_id.insert(risk.code.clone(), id.clone());

        let system_types_json = if risk.applicable_system_types.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&risk.applicable_system_types)?)
        };

        tx.execute(
            "INSERT INTO LibraryRisk (
                id, code, title, description,
                applicable_system_types_json, default_inherent_rating,
                library_version, superseded_by
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
            params![
                id,
                risk.code,
                risk.title,
                risk.description,
                system_types_json,
                risk.default_inherent_rating,
                bundle.version,
            ],
        )?;

        mark_prior_versions_superseded(
            &tx,
            "LibraryRisk",
            &risk.code,
            &bundle.version,
            &id,
        )?;

        for fm in &risk.framework_mappings {
            insert_framework_mapping(&tx, "LibraryRisk", &id, fm, &bundle.version)?;
        }
    }

    let mut control_code_to_id: HashMap<String, String> = HashMap::new();
    for control in &bundle.controls {
        let id = Uuid::now_v7().to_string();
        control_code_to_id.insert(control.code.clone(), id.clone());

        let system_types_json = if control.applicable_system_types.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&control.applicable_system_types)?)
        };

        let related_risk_ids: Vec<String> = control
            .related_risk_codes
            .iter()
            .map(|code| {
                risk_code_to_id.get(code).cloned().ok_or_else(|| {
                    AppError::Message(format!(
                        "library bundle v{}: control {} references unknown risk code {}",
                        bundle.version, control.code, code
                    ))
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let related_risk_ids_json = if related_risk_ids.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&related_risk_ids)?)
        };

        tx.execute(
            "INSERT INTO LibraryControl (
                id, code, title, description, objective,
                applicable_system_types_json, control_type, frequency,
                related_risk_ids_json, library_version, superseded_by
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
            params![
                id,
                control.code,
                control.title,
                control.description,
                control.objective,
                system_types_json,
                control.control_type,
                control.frequency,
                related_risk_ids_json,
                bundle.version,
            ],
        )?;

        mark_prior_versions_superseded(
            &tx,
            "LibraryControl",
            &control.code,
            &bundle.version,
            &id,
        )?;

        for fm in &control.framework_mappings {
            insert_framework_mapping(&tx, "LibraryControl", &id, fm, &bundle.version)?;
        }
    }

    for tp in &bundle.test_procedures {
        let control_id = control_code_to_id.get(&tp.control_code).ok_or_else(|| {
            AppError::Message(format!(
                "library bundle v{}: test procedure {} references unknown control code {}",
                bundle.version, tp.code, tp.control_code
            ))
        })?;

        let eec_id = if let Some(checklist) = &tp.evidence_checklist {
            let id = Uuid::now_v7().to_string();
            let items_json = serde_json::to_string(&checklist.items)?;
            tx.execute(
                "INSERT INTO ExpectedEvidenceChecklist (
                    id, test_procedure_id, items_json, library_version
                 ) VALUES (?1, NULL, ?2, ?3)",
                params![id, items_json, bundle.version],
            )?;
            Some(id)
        } else {
            None
        };

        let tp_id = Uuid::now_v7().to_string();
        let steps_json = serde_json::to_string(&tp.steps)?;
        tx.execute(
            "INSERT INTO TestProcedure (
                id, control_id, code, name, objective, steps_json,
                expected_evidence_checklist_id, sampling_default,
                automation_hint, library_version
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                tp_id,
                control_id,
                tp.code,
                tp.name,
                tp.objective,
                steps_json,
                eec_id,
                tp.sampling_default,
                tp.automation_hint,
                bundle.version,
            ],
        )?;

        // Back-link the EEC to its owning test procedure. The column is
        // nullable so we could skip this, but maintaining the link keeps the
        // relationship discoverable from either side.
        if let Some(ref eec_id) = eec_id {
            tx.execute(
                "UPDATE ExpectedEvidenceChecklist
                 SET test_procedure_id = ?1
                 WHERE id = ?2",
                params![tp_id, eec_id],
            )?;
        }
    }

    tx.commit()?;
    tracing::info!(
        version = %bundle.version,
        risks = bundle.risks.len(),
        controls = bundle.controls.len(),
        test_procedures = bundle.test_procedures.len(),
        "library bundle installed"
    );
    Ok(())
}

fn mark_prior_versions_superseded(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    code: &str,
    new_version: &str,
    new_id: &str,
) -> AppResult<()> {
    // Only updates rows whose superseded_by is still NULL and whose
    // library_version predates the one being installed. Safe to run
    // unconditionally even in a clean install — the query just matches
    // nothing.
    let sql = format!(
        "UPDATE {table}
         SET superseded_by = ?1
         WHERE code = ?2 AND library_version <> ?3 AND superseded_by IS NULL"
    );
    tx.execute(&sql, params![new_id, code, new_version])?;
    Ok(())
}

fn insert_framework_mapping(
    tx: &rusqlite::Transaction<'_>,
    entity_type: &str,
    entity_id: &str,
    fm: &BundleFrameworkMapping,
    library_version: &str,
) -> AppResult<()> {
    tx.execute(
        "INSERT INTO FrameworkMapping (
            id, entity_type, entity_id, framework, reference, library_version
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            Uuid::now_v7().to_string(),
            entity_type,
            entity_id,
            fm.framework,
            fm.reference,
            library_version,
        ],
    )?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Bundle {
    version: String,
    #[allow(dead_code)]
    published_at: i64,
    #[allow(dead_code)]
    frameworks_included: Vec<String>,
    risks: Vec<BundleRisk>,
    controls: Vec<BundleControl>,
    test_procedures: Vec<BundleTestProcedure>,
}

#[derive(Debug, Deserialize)]
struct BundleRisk {
    code: String,
    title: String,
    description: String,
    #[serde(default)]
    applicable_system_types: Vec<String>,
    default_inherent_rating: Option<String>,
    #[serde(default)]
    framework_mappings: Vec<BundleFrameworkMapping>,
}

#[derive(Debug, Deserialize)]
struct BundleControl {
    code: String,
    title: String,
    description: String,
    objective: String,
    #[serde(default)]
    applicable_system_types: Vec<String>,
    control_type: String,
    frequency: Option<String>,
    #[serde(default)]
    related_risk_codes: Vec<String>,
    #[serde(default)]
    framework_mappings: Vec<BundleFrameworkMapping>,
}

#[derive(Debug, Deserialize)]
struct BundleTestProcedure {
    code: String,
    control_code: String,
    name: String,
    objective: String,
    steps: Vec<String>,
    sampling_default: String,
    automation_hint: String,
    evidence_checklist: Option<BundleEvidenceChecklist>,
}

#[derive(Debug, Deserialize)]
struct BundleEvidenceChecklist {
    items: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BundleFrameworkMapping {
    framework: String,
    reference: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbState;

    fn tmp_path(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.join(format!("audit-library-test-{stamp}-{suffix}.db"))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn baseline_bundle_loads_into_fresh_db() {
        let path = tmp_path("fresh-install");
        let key = [3u8; 32];
        let db = DbState::new();
        db.open_with_key(&path, &key).unwrap();

        db.with_mut(|conn| install_baseline_bundles(conn)).unwrap();

        db.with(|conn| {
            // Every prior bundle's rows remain in the DB with
            // `superseded_by` pointing at the same-code row in the next
            // bundle up. Only v0.5.0's rows have `superseded_by IS NULL`.
            let risks_v1: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk WHERE library_version = '0.1.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v1, 3);
            let risks_v2: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk WHERE library_version = '0.2.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v2, 3);
            let risks_v3: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk WHERE library_version = '0.3.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v3, 3);
            let risks_v4: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk WHERE library_version = '0.4.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v4, 4);
            let risks_v5: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk WHERE library_version = '0.5.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v5, 4);
            let risks_v1_superseded: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk
                 WHERE library_version = '0.1.0' AND superseded_by IS NOT NULL",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v1_superseded, 3);
            let risks_v2_superseded: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk
                 WHERE library_version = '0.2.0' AND superseded_by IS NOT NULL",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v2_superseded, 3);
            let risks_v3_superseded: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk
                 WHERE library_version = '0.3.0' AND superseded_by IS NOT NULL",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v3_superseded, 3);
            let risks_v4_superseded: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk
                 WHERE library_version = '0.4.0' AND superseded_by IS NOT NULL",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v4_superseded, 4);
            let risks_v5_current: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryRisk
                 WHERE library_version = '0.5.0' AND superseded_by IS NULL",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(risks_v5_current, 4);

            let controls_v1: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryControl WHERE library_version = '0.1.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(controls_v1, 5);
            let controls_v2: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryControl WHERE library_version = '0.2.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(controls_v2, 5);
            let controls_v3: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryControl WHERE library_version = '0.3.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(controls_v3, 5);
            let controls_v4: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryControl WHERE library_version = '0.4.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(controls_v4, 6);
            let controls_v5: i64 = conn.query_row(
                "SELECT COUNT(*) FROM LibraryControl WHERE library_version = '0.5.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(controls_v5, 6);

            let test_procedures_v1: i64 = conn.query_row(
                "SELECT COUNT(*) FROM TestProcedure WHERE library_version = '0.1.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(test_procedures_v1, 5);
            let test_procedures_v2: i64 = conn.query_row(
                "SELECT COUNT(*) FROM TestProcedure WHERE library_version = '0.2.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(test_procedures_v2, 6);
            let test_procedures_v3: i64 = conn.query_row(
                "SELECT COUNT(*) FROM TestProcedure WHERE library_version = '0.3.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(test_procedures_v3, 7);
            let test_procedures_v4: i64 = conn.query_row(
                "SELECT COUNT(*) FROM TestProcedure WHERE library_version = '0.4.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(test_procedures_v4, 8);
            let test_procedures_v5: i64 = conn.query_row(
                "SELECT COUNT(*) FROM TestProcedure WHERE library_version = '0.5.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(test_procedures_v5, 9);

            // v0.2.0 introduced UAM-T-003 (dormant accounts); v0.3.0
            // introduced UAM-T-004 (orphan accounts); v0.4.0 introduced
            // ITAC-T-001 (Benford first-digit analysis); v0.5.0 introduces
            // ITAC-T-002 (duplicate-transaction detection). Each should be
            // present at its introducing version.
            let dormant_present: i64 = conn.query_row(
                "SELECT COUNT(*) FROM TestProcedure
                 WHERE code = 'UAM-T-003' AND library_version = '0.2.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(dormant_present, 1);
            let orphan_present: i64 = conn.query_row(
                "SELECT COUNT(*) FROM TestProcedure
                 WHERE code = 'UAM-T-004' AND library_version = '0.3.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(orphan_present, 1);
            let benford_present: i64 = conn.query_row(
                "SELECT COUNT(*) FROM TestProcedure
                 WHERE code = 'ITAC-T-001' AND library_version = '0.4.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(benford_present, 1);
            let duplicate_present: i64 = conn.query_row(
                "SELECT COUNT(*) FROM TestProcedure
                 WHERE code = 'ITAC-T-002' AND library_version = '0.5.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(duplicate_present, 1);

            let checklists_v2: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ExpectedEvidenceChecklist WHERE library_version = '0.2.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(checklists_v2, 6);
            let checklists_v3: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ExpectedEvidenceChecklist WHERE library_version = '0.3.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(checklists_v3, 7);
            let checklists_v4: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ExpectedEvidenceChecklist WHERE library_version = '0.4.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(checklists_v4, 8);
            let checklists_v5: i64 = conn.query_row(
                "SELECT COUNT(*) FROM ExpectedEvidenceChecklist WHERE library_version = '0.5.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(checklists_v5, 9);

            // Each control carries 2 framework mappings. 5 controls per
            // version × 2 = 10 per version; v0.4.0 and v0.5.0 each have 6
            // controls → 12.
            let mappings_v1: i64 = conn.query_row(
                "SELECT COUNT(*) FROM FrameworkMapping WHERE library_version = '0.1.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(mappings_v1, 10);
            let mappings_v2: i64 = conn.query_row(
                "SELECT COUNT(*) FROM FrameworkMapping WHERE library_version = '0.2.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(mappings_v2, 10);
            let mappings_v3: i64 = conn.query_row(
                "SELECT COUNT(*) FROM FrameworkMapping WHERE library_version = '0.3.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(mappings_v3, 10);
            let mappings_v4: i64 = conn.query_row(
                "SELECT COUNT(*) FROM FrameworkMapping WHERE library_version = '0.4.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(mappings_v4, 12);
            let mappings_v5: i64 = conn.query_row(
                "SELECT COUNT(*) FROM FrameworkMapping WHERE library_version = '0.5.0'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(mappings_v5, 12);
            Ok(())
        })
        .unwrap();

        db.close();
        cleanup(&path);
    }

    #[test]
    fn reinstalling_same_bundle_is_noop() {
        let path = tmp_path("idempotent");
        let key = [4u8; 32];
        let db = DbState::new();
        db.open_with_key(&path, &key).unwrap();

        db.with_mut(|conn| install_baseline_bundles(conn)).unwrap();
        let first_control_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM LibraryControl WHERE code = 'UAM-C-001'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();

        // Second call should see the rows already present and exit early,
        // leaving row identities unchanged.
        db.with_mut(|conn| install_baseline_bundles(conn)).unwrap();
        let same_control_id: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT id FROM LibraryControl WHERE code = 'UAM-C-001'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(first_control_id, same_control_id);

        db.close();
        cleanup(&path);
    }

    #[test]
    fn tampered_bundle_is_rejected() {
        // Flip a byte in the shipped bundle to confirm the verifier rejects
        // the altered payload. Uses the real LIBRARY_PUBLIC_KEY via
        // `verify_bundle`, which is the path exercised at runtime.
        let mut tampered = BUNDLE_V0_1_0.to_vec();
        tampered[0] ^= 0x01;

        let path = tmp_path("tampered");
        let key = [5u8; 32];
        let db = DbState::new();
        db.open_with_key(&path, &key).unwrap();

        let err = db
            .with_mut(|conn| install_bundle(conn, &tampered, BUNDLE_V0_1_0_SIG))
            .unwrap_err();
        assert!(matches!(err, AppError::Crypto(_)));

        db.close();
        cleanup(&path);
    }
}
