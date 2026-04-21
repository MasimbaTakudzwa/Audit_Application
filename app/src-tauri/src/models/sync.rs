use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    LocalOnly,
    PendingUpload,
    Synced,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRecord {
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub last_modified_at: i64,
    pub last_modified_by: Option<String>,
    pub version: i64,
    pub deleted: bool,
    pub sync_state: SyncState,
    pub remote_version: Option<i64>,
}
