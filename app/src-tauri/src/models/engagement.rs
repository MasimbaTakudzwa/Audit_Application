use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Engagement {
    pub id: String,
    pub client_id: String,
    pub name: String,
    pub period_id: Option<String>,
    pub status_id: String,
    pub prior_engagement_id: Option<String>,
    pub library_version_at_start: String,
    pub encryption_key_id: String,
    pub lead_partner_id: Option<String>,
    pub created_at: i64,
    pub closed_at: Option<i64>,
    pub archive_bundle_blob_id: Option<String>,
}
