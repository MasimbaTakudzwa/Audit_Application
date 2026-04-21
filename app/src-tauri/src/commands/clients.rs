use serde::Serialize;
use tauri::State;

use crate::{db::DbState, error::AppResult};

#[derive(Debug, Serialize)]
pub struct ClientSummary {
    pub id: String,
    pub name: String,
    pub country: String,
    pub industry: Option<String>,
    pub status: String,
}

#[tauri::command]
pub fn list_clients(db: State<'_, DbState>) -> AppResult<Vec<ClientSummary>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT c.id, c.name, c.country, i.name, c.status
             FROM Client c
             LEFT JOIN Industry i ON i.id = c.industry_id
             WHERE c.status = 'active'
             ORDER BY c.name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ClientSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    country: row.get(2)?,
                    industry: row.get(3)?,
                    status: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}
