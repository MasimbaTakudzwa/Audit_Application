use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct System {
    pub id: String,
    pub engagement_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub template_id: Option<String>,
    pub environment: String,
    pub criticality: String,
    pub derived_from: Option<String>,
    pub created_at: i64,
}
