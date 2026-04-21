use serde::Serialize;

use crate::error::AppResult;

#[derive(Debug, Serialize)]
pub struct LibraryVersion {
    pub version: &'static str,
    pub frameworks: Vec<&'static str>,
}

#[tauri::command]
pub fn library_version() -> AppResult<LibraryVersion> {
    Ok(LibraryVersion {
        version: "0.1.0",
        frameworks: vec!["COBIT 2019", "NIST CSF", "ISO 27001", "PCI DSS"],
    })
}
