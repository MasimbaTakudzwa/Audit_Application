use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    pub id: String,
    pub firm_id: String,
    pub name: String,
    pub industry_id: Option<String>,
    pub country: String,
    pub status: ClientStatus,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClientStatus {
    Active,
    Archived,
}
