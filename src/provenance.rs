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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record() -> ProvenanceRecord {
        ProvenanceRecord {
            version: 1,
            entries: vec![ProvenanceEntry {
                source_name: "demo".to_owned(),
                kind: "skill".to_owned(),
                source_url: "https://example.com/repo".to_owned(),
                branch: "main".to_owned(),
                resolved_revision: "abc123".to_owned(),
                source_path: "skills/demo".to_owned(),
                target_ai: "claude".to_owned(),
                target_path: ".claude/skills/demo".to_owned(),
            }],
        }
    }

    #[test]
    fn write_creates_the_record_under_a_missing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), &sample_record()).unwrap();

        let written = fs::read_to_string(tmp.path().join(PROVENANCE_REL_PATH)).unwrap();
        assert!(written.ends_with("\n"));
        assert!(written.contains("\"resolved_revision\": \"abc123\""));
        assert!(written.contains("\"target_path\": \".claude/skills/demo\""));
    }

    #[test]
    fn write_reports_io_failure_when_the_parent_is_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        // Put a regular file where the `.enozunu` directory would go, so create_dir_all fails.
        fs::write(tmp.path().join(".enozunu"), "not a directory").unwrap();

        let diag = write(tmp.path(), &sample_record()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::Io);
    }

    #[test]
    fn write_reports_io_failure_when_the_target_is_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        // Occupy the record path with a directory so the final file write fails
        // even though its parent directory is created successfully.
        fs::create_dir_all(tmp.path().join(PROVENANCE_REL_PATH)).unwrap();

        let diag = write(tmp.path(), &sample_record()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::Io);
    }
}
