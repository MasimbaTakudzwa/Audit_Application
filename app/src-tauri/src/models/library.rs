use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryControl {
    pub id: String,
    pub code: String,
    pub title: String,
    pub description: String,
    pub objective: String,
    pub control_type: String,
    pub frequency: Option<String>,
    pub library_version: String,
}
