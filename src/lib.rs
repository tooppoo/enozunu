//! Enozunu materializes declared AI-agent configuration sources into target AI-native paths.
//!
//! The pipeline is: parse and validate the manifest, plan target paths, resolve Git sources, check every artifact, then write outputs and the provenance record.

pub mod diagnostics;
pub mod git;
pub mod manifest;
pub mod materialize;
pub mod plan;
pub mod provenance;

use std::collections::HashMap;
use std::path::Path;

use diagnostics::{Diagnostic, DiagnosticCode};
use git::{GitResolver, ResolvedSource};
use plan::PlannedMaterialization;
use provenance::{ProvenanceEntry, ProvenanceRecord};

pub const MANIFEST_FILE_NAME: &str = "enozunu.kdl";
pub const PROVENANCE_VERSION: u32 = 1;

/// The outcome of one materialized entry, reported back to the CLI.
#[derive(Debug)]
pub struct MaterializedEntry {
    pub source_name: String,
    pub kind: plan::ArtifactKind,
    pub resolved_revision: String,
    pub target_rel_path: String,
}

/// Loads and validates the manifest at `manifest_path`.
pub fn load_manifest(manifest_path: &Path) -> Result<manifest::Manifest, Vec<Diagnostic>> {
    let text = std::fs::read_to_string(manifest_path).map_err(|e| {
        vec![Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to read {}: {e}", manifest_path.display()),
        )]
    })?;
    manifest::parse(&text)
}

/// Runs the full materialization pipeline and writes the provenance record.
///
/// All sources are resolved and all artifacts are checked before the first target write, so a failing entry does not leave the project half-updated.
pub fn run_materialize(
    manifest_path: &Path,
    project_root: &Path,
    resolver: &dyn GitResolver,
) -> Result<Vec<MaterializedEntry>, Vec<Diagnostic>> {
    let manifest = load_manifest(manifest_path)?;
    let planned = plan::plan(&manifest)?;

    let resolved = resolve_sources(&planned, resolver)?;

    let mut checked = Vec::new();
    let mut diags = Vec::new();
    for entry in &planned {
        let key = source_key(entry);
        let source = &resolved[&key];
        match materialize::check(entry, &source.checkout_dir, project_root) {
            Ok(c) => checked.push((entry, source.commit.clone(), c)),
            Err(d) => diags.push(d),
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }

    let mut results = Vec::new();
    let mut provenance_entries = Vec::new();
    for (entry, commit, checked) in &checked {
        materialize::execute(checked).map_err(|d| vec![d])?;
        results.push(MaterializedEntry {
            source_name: entry.source_name.clone(),
            kind: entry.kind,
            resolved_revision: commit.clone(),
            target_rel_path: entry.target_rel_path.clone(),
        });
        provenance_entries.push(ProvenanceEntry {
            source_name: entry.source_name.clone(),
            kind: entry.kind.as_str().to_owned(),
            source_url: entry.reference.git_url.clone(),
            branch: entry.reference.branch.clone(),
            resolved_revision: commit.clone(),
            source_path: entry.reference.path.clone(),
            target_ai: "claude".to_owned(),
            target_path: entry.target_rel_path.clone(),
        });
    }

    provenance::write(
        project_root,
        &ProvenanceRecord {
            version: PROVENANCE_VERSION,
            entries: provenance_entries,
        },
    )
    .map_err(|d| vec![d])?;

    Ok(results)
}

fn source_key(entry: &PlannedMaterialization) -> (String, String) {
    (
        entry.reference.git_url.clone(),
        entry.reference.branch.clone(),
    )
}

/// Resolves each distinct (url, branch) pair once so a single run sees one consistent commit per branch.
fn resolve_sources(
    planned: &[PlannedMaterialization],
    resolver: &dyn GitResolver,
) -> Result<HashMap<(String, String), ResolvedSource>, Vec<Diagnostic>> {
    let mut resolved = HashMap::new();
    let mut diags = Vec::new();
    for entry in planned {
        let key = source_key(entry);
        if resolved.contains_key(&key) {
            continue;
        }
        match resolver.resolve(&entry.reference.git_url, &entry.reference.branch) {
            Ok(source) => {
                resolved.insert(key, source);
            }
            Err(d) => diags.push(d),
        }
    }
    if diags.is_empty() {
        Ok(resolved)
    } else {
        Err(diags)
    }
}
