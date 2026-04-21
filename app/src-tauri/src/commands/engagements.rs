use serde::Serialize;
use tauri::State;

use crate::{db::DbState, error::AppResult};

#[derive(Debug, Serialize)]
pub struct EngagementSummary {
    pub id: String,
    pub name: String,
    pub client_name: String,
    pub status: String,
    pub fiscal_year: Option<String>,
    pub created_at: i64,
}

#[tauri::command]
pub fn list_engagements(db: State<'_, DbState>) -> AppResult<Vec<EngagementSummary>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT e.id, e.name, c.name, s.name, p.fiscal_year_label, e.created_at
             FROM Engagement e
             JOIN Client c ON c.id = e.client_id
             JOIN EngagementStatus s ON s.id = e.status_id
             LEFT JOIN EngagementPeriod p ON p.engagement_id = e.id
             ORDER BY e.created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(EngagementSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    client_name: row.get(2)?,
                    status: row.get(3)?,
                    fiscal_year: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}
