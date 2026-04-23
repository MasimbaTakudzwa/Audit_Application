//! Library browsing commands.
//!
//! Read-only. The library content is installed from a signed bundle at DB
//! open (see `library::loader`) and filtered to the currently-shipped version
//! via `superseded_by IS NULL`. Firm overrides layer on top and are not
//! exposed through these endpoints yet — they come in a follow-up.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::State;

use crate::{
    db::DbState,
    error::{AppError, AppResult},
};

#[derive(Debug, Serialize)]
pub struct LibraryVersion {
    pub version: String,
    pub frameworks: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LibraryRiskSummary {
    pub id: String,
    pub code: String,
    pub title: String,
    pub default_inherent_rating: Option<String>,
    pub applicable_system_types: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LibraryControlSummary {
    pub id: String,
    pub code: String,
    pub title: String,
    pub objective: String,
    pub control_type: String,
    pub frequency: Option<String>,
    pub applicable_system_types: Vec<String>,
    pub frameworks: Vec<String>,
    pub test_procedure_count: i64,
}

#[derive(Debug, Serialize)]
pub struct LibraryControlDetail {
    pub id: String,
    pub code: String,
    pub title: String,
    pub description: String,
    pub objective: String,
    pub control_type: String,
    pub frequency: Option<String>,
    pub applicable_system_types: Vec<String>,
    pub related_risks: Vec<LibraryRiskSummary>,
    pub framework_mappings: Vec<LibraryFrameworkMapping>,
    pub test_procedures: Vec<LibraryTestProcedureSummary>,
    pub library_version: String,
}

#[derive(Debug, Serialize)]
pub struct LibraryFrameworkMapping {
    pub framework: String,
    pub reference: String,
}

#[derive(Debug, Serialize)]
pub struct LibraryTestProcedureSummary {
    pub id: String,
    pub code: String,
    pub name: String,
    pub objective: String,
    pub steps: Vec<String>,
    pub sampling_default: String,
    pub automation_hint: String,
    pub evidence_checklist: Vec<String>,
}

/// Latest installed library version and the distinct frameworks referenced by
/// its controls. If no bundle has been installed yet, returns `version = ""`
/// and an empty framework list rather than erroring — the UI renders this as
/// "No library installed".
#[tauri::command]
pub fn library_version(db: State<'_, DbState>) -> AppResult<LibraryVersion> {
    db.with(|conn| {
        let version = current_library_version(conn)?.unwrap_or_default();
        if version.is_empty() {
            return Ok(LibraryVersion {
                version,
                frameworks: vec![],
            });
        }

        let mut stmt = conn.prepare(
            "SELECT DISTINCT framework
             FROM FrameworkMapping
             WHERE library_version = ?1
             ORDER BY framework",
        )?;
        let frameworks: Vec<String> = stmt
            .query_map(params![version], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(LibraryVersion { version, frameworks })
    })
}

#[tauri::command]
pub fn library_list_risks(db: State<'_, DbState>) -> AppResult<Vec<LibraryRiskSummary>> {
    db.with(|conn| {
        let Some(version) = current_library_version(conn)? else {
            return Ok(vec![]);
        };
        let mut stmt = conn.prepare(
            "SELECT id, code, title, default_inherent_rating, applicable_system_types_json
             FROM LibraryRisk
             WHERE library_version = ?1 AND superseded_by IS NULL
             ORDER BY code",
        )?;
        let rows = stmt
            .query_map(params![version], |row| {
                let system_types_json: Option<String> = row.get(4)?;
                Ok(LibraryRiskSummary {
                    id: row.get(0)?,
                    code: row.get(1)?,
                    title: row.get(2)?,
                    default_inherent_rating: row.get(3)?,
                    applicable_system_types: parse_string_array(system_types_json),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[tauri::command]
pub fn library_list_controls(
    db: State<'_, DbState>,
) -> AppResult<Vec<LibraryControlSummary>> {
    db.with(|conn| {
        let Some(version) = current_library_version(conn)? else {
            return Ok(vec![]);
        };
        let mut stmt = conn.prepare(
            "SELECT
                c.id, c.code, c.title, c.objective, c.control_type, c.frequency,
                c.applicable_system_types_json,
                (SELECT COUNT(*) FROM TestProcedure tp
                   WHERE tp.control_id = c.id AND tp.library_version = c.library_version)
             FROM LibraryControl c
             WHERE c.library_version = ?1 AND c.superseded_by IS NULL
             ORDER BY c.code",
        )?;
        let rows: Vec<(String, String, String, String, String, Option<String>, Option<String>, i64)> =
            stmt.query_map(params![version], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut summaries: Vec<LibraryControlSummary> = Vec::with_capacity(rows.len());
        for (id, code, title, objective, control_type, frequency, system_types_json, tp_count) in rows {
            let frameworks = frameworks_for_entity(conn, "LibraryControl", &id, &version)?;
            summaries.push(LibraryControlSummary {
                id,
                code,
                title,
                objective,
                control_type,
                frequency,
                applicable_system_types: parse_string_array(system_types_json),
                frameworks,
                test_procedure_count: tp_count,
            });
        }
        Ok(summaries)
    })
}

#[tauri::command]
pub fn library_get_control(
    id: String,
    db: State<'_, DbState>,
) -> AppResult<LibraryControlDetail> {
    db.with(|conn| {
        let (
            id,
            code,
            title,
            description,
            objective,
            control_type,
            frequency,
            system_types_json,
            related_risk_ids_json,
            library_version,
        ): (
            String,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
        ) = conn
            .query_row(
                "SELECT id, code, title, description, objective, control_type, frequency,
                        applicable_system_types_json, related_risk_ids_json, library_version
                 FROM LibraryControl
                 WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("library control {id}")))?;

        let related_risk_ids: Vec<String> = related_risk_ids_json
            .as_deref()
            .map(|s| serde_json::from_str::<Vec<String>>(s).unwrap_or_default())
            .unwrap_or_default();

        let related_risks = if related_risk_ids.is_empty() {
            vec![]
        } else {
            load_risks_by_ids(conn, &related_risk_ids)?
        };

        let framework_mappings = framework_mappings_for_entity(
            conn,
            "LibraryControl",
            &id,
            &library_version,
        )?;

        let test_procedures = load_test_procedures_for_control(conn, &id, &library_version)?;

        Ok(LibraryControlDetail {
            id,
            code,
            title,
            description,
            objective,
            control_type,
            frequency,
            applicable_system_types: parse_string_array(system_types_json),
            related_risks,
            framework_mappings,
            test_procedures,
            library_version,
        })
    })
}

fn current_library_version(conn: &Connection) -> AppResult<Option<String>> {
    let version: Option<String> = conn
        .query_row(
            "SELECT MAX(library_version) FROM LibraryControl WHERE superseded_by IS NULL",
            [],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(version)
}

fn frameworks_for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    library_version: &str,
) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT framework
         FROM FrameworkMapping
         WHERE entity_type = ?1 AND entity_id = ?2 AND library_version = ?3
         ORDER BY framework",
    )?;
    let rows: Vec<String> = stmt
        .query_map(params![entity_type, entity_id, library_version], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn framework_mappings_for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    library_version: &str,
) -> AppResult<Vec<LibraryFrameworkMapping>> {
    let mut stmt = conn.prepare(
        "SELECT framework, reference
         FROM FrameworkMapping
         WHERE entity_type = ?1 AND entity_id = ?2 AND library_version = ?3
         ORDER BY framework, reference",
    )?;
    let rows = stmt
        .query_map(params![entity_type, entity_id, library_version], |row| {
            Ok(LibraryFrameworkMapping {
                framework: row.get(0)?,
                reference: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn load_risks_by_ids(
    conn: &Connection,
    ids: &[String],
) -> AppResult<Vec<LibraryRiskSummary>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    // rusqlite's `rarray` feature would make this cleaner; absent that, build
    // an IN clause with ?1, ?2, ... placeholders sized to the input.
    let placeholders = (1..=ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT id, code, title, default_inherent_rating, applicable_system_types_json
         FROM LibraryRisk
         WHERE id IN ({placeholders})
         ORDER BY code"
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_iter: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params_iter.iter()), |row| {
            let system_types_json: Option<String> = row.get(4)?;
            Ok(LibraryRiskSummary {
                id: row.get(0)?,
                code: row.get(1)?,
                title: row.get(2)?,
                default_inherent_rating: row.get(3)?,
                applicable_system_types: parse_string_array(system_types_json),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn load_test_procedures_for_control(
    conn: &Connection,
    control_id: &str,
    library_version: &str,
) -> AppResult<Vec<LibraryTestProcedureSummary>> {
    let mut stmt = conn.prepare(
        "SELECT tp.id, tp.code, tp.name, tp.objective, tp.steps_json,
                tp.sampling_default, tp.automation_hint,
                eec.items_json
         FROM TestProcedure tp
         LEFT JOIN ExpectedEvidenceChecklist eec
                ON eec.id = tp.expected_evidence_checklist_id
         WHERE tp.control_id = ?1 AND tp.library_version = ?2
         ORDER BY tp.code",
    )?;
    let rows = stmt
        .query_map(params![control_id, library_version], |row| {
            let steps_json: String = row.get(4)?;
            let items_json: Option<String> = row.get(7)?;
            Ok(LibraryTestProcedureSummary {
                id: row.get(0)?,
                code: row.get(1)?,
                name: row.get(2)?,
                objective: row.get(3)?,
                steps: serde_json::from_str(&steps_json).unwrap_or_default(),
                sampling_default: row.get(5)?,
                automation_hint: row.get(6)?,
                evidence_checklist: items_json
                    .as_deref()
                    .map(|s| serde_json::from_str::<Vec<String>>(s).unwrap_or_default())
                    .unwrap_or_default(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn parse_string_array(json: Option<String>) -> Vec<String> {
    json.as_deref()
        .map(|s| serde_json::from_str::<Vec<String>>(s).unwrap_or_default())
        .unwrap_or_default()
}
