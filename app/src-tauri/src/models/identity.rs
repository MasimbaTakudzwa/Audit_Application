use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Firm {
    pub id: String,
    pub name: String,
    pub country: String,
    pub default_locale: String,
    pub license_id: Option<String>,
    pub library_version: Option<String>,
    pub settings_json: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub firm_id: String,
    pub email: String,
    pub display_name: String,
    pub role_id: String,
    pub status: UserStatus,
    pub last_seen_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    Active,
    Suspended,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LicenseTier {
    Subscription,
    Prepaid,
    ByoKey,
}
