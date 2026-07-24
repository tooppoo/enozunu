//! Writes the machine-generated provenance record.
//!
//! `.enozunu/provenance.json` records what the previous materialization produced.
//! It is not a lockfile and is never read back as a resolution input; the resolution input is
//! `enozunu.lock.json` (see docs/design/adr/20260724T021001Z_lockfile-based-reproducibility.md).
//! See docs/guide/generated-output.md for the provenance policy.

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
    pub source: ProvenanceSource,
    pub target_ai: String,
    pub target_path: String,
}

/// The declared Git selector, recorded as one tagged shape for branch, tag, and revision sources.
///
/// A single `selector` object rather than independent optional `branch` / `tag` / `revision` fields keeps the record aligned with the manifest contract: exactly one selector exists, so consumers dispatch on `type` instead of probing which field is present.
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum ProvenanceGitSelector {
    Branch(String),
    Tag(String),
    Revision(String),
}

/// Source-kind-specific provenance fields, tagged so consumers can dispatch on `type` instead of probing for Git-only fields.
///
/// A Git source records both the declared `selector` and the materialized `resolved_revision`; for a revision selector the two carry the same commit id, which records that the pin was honored. For a branch or tag selector, `resolved_revision` is the only record of which commit the mutable ref pointed at during this run.
/// A Gist source records `type: "gist"` with its id and pinned revision; it is never represented as `type: "git"` even though Git transport materialized it.
/// `file` is present only for agent Gists, which select one file; a Skill Gist materializes the revision root and records no `file` key.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProvenanceSource {
    Git {
        url: String,
        selector: ProvenanceGitSelector,
        path: String,
        resolved_revision: String,
    },
    Local {
        path: String,
        resolved_path: String,
    },
    Gist {
        id: String,
        revision: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
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
            entries: vec![
                ProvenanceEntry {
                    source_name: "demo".to_owned(),
                    kind: "skill".to_owned(),
                    source: ProvenanceSource::Git {
                        url: "https://example.com/repo".to_owned(),
                        selector: ProvenanceGitSelector::Branch("main".to_owned()),
                        path: "skills/demo".to_owned(),
                        resolved_revision: "abc123".to_owned(),
                    },
                    target_ai: "claude".to_owned(),
                    target_path: ".claude/skills/demo".to_owned(),
                },
                ProvenanceEntry {
                    source_name: "local-demo".to_owned(),
                    kind: "skill".to_owned(),
                    source: ProvenanceSource::Local {
                        path: "../sibling/skills/demo".to_owned(),
                        resolved_path: "/canonical/sibling/skills/demo".to_owned(),
                    },
                    target_ai: "claude".to_owned(),
                    target_path: ".claude/skills/local-demo".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn serializes_a_tag_selector_under_the_shared_tagged_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let mut record = sample_record();
        record.entries[0].source = ProvenanceSource::Git {
            url: "https://example.com/repo".to_owned(),
            selector: ProvenanceGitSelector::Tag("v1.0.0".to_owned()),
            path: "skills/demo".to_owned(),
            resolved_revision: "abc123".to_owned(),
        };
        write(tmp.path(), &record).unwrap();

        let written = fs::read_to_string(tmp.path().join(PROVENANCE_REL_PATH)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
        let source = &parsed["entries"][0]["source"];
        assert_eq!(source["type"], "git");
        assert_eq!(source["selector"]["type"], "tag");
        assert_eq!(source["selector"]["value"], "v1.0.0");
        assert!(source.get("tag").is_none());
        // A tag is mutable, so the resolved commit is recorded separately rather than being implied by the selector.
        assert_eq!(source["resolved_revision"], "abc123");
    }

    #[test]
    fn write_creates_the_record_under_a_missing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), &sample_record()).unwrap();

        let written = fs::read_to_string(tmp.path().join(PROVENANCE_REL_PATH)).unwrap();
        assert!(written.ends_with("\n"));
        assert!(written.contains("\"target_path\": \".claude/skills/demo\""));

        // Source fields live under a typed `source` object rather than as Git-specific top-level fields.
        let record: serde_json::Value = serde_json::from_str(&written).unwrap();
        let entries = record["entries"].as_array().unwrap();
        assert_eq!(entries[0]["source"]["type"], "git");
        // The declared selector is one tagged object, not parallel optional `branch` / `tag` / `revision` fields.
        assert_eq!(entries[0]["source"]["selector"]["type"], "branch");
        assert_eq!(entries[0]["source"]["selector"]["value"], "main");
        assert!(entries[0]["source"].get("branch").is_none());
        assert_eq!(entries[0]["source"]["resolved_revision"], "abc123");
        assert!(entries[0].get("resolved_revision").is_none());
        assert_eq!(entries[1]["source"]["type"], "local");
        assert_eq!(
            entries[1]["source"]["resolved_path"],
            "/canonical/sibling/skills/demo"
        );
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
