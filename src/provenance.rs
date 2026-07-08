//! Writes the machine-generated provenance record.
//!
//! `.enozunu/provenance.json` records what the previous materialization produced.
//! It is not a lockfile and is not read back as a resolution input in v0.0.x.
//! See docs/generated-output.md for the provenance policy.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::diagnostics::{Diagnostic, DiagnosticCode};

pub const PROVENANCE_REL_PATH: &str = ".enozunu/provenance.json";

#[derive(Debug, Serialize)]
pub struct ProvenanceRecord {
    pub version: u32,
    pub entries: Vec<ProvenanceEntry>,
}

#[derive(Debug, Serialize)]
pub struct ProvenanceEntry {
    pub source_name: String,
    pub kind: String,
    pub source_url: String,
    pub branch: String,
    pub resolved_revision: String,
    pub source_path: String,
    pub target_ai: String,
    pub target_path: String,
}

pub fn write(project_root: &Path, record: &ProvenanceRecord) -> Result<(), Diagnostic> {
    let path = project_root.join(PROVENANCE_REL_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            Diagnostic::new(
                DiagnosticCode::Io,
                format!("failed to create provenance directory: {e}"),
            )
        })?;
    }
    let json = serde_json::to_string_pretty(record).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to serialize provenance record: {e}"),
        )
    })?;
    fs::write(&path, json + "\n").map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to write {}: {e}", path.display()),
        )
    })
}
