//! Enozunu materializes declared AI-agent configuration sources into target AI-native paths.
//!
//! The pipeline is: parse and validate the manifest, plan target paths, resolve Git and local sources, check every artifact, then write outputs and the provenance record.

pub mod diagnostics;
pub mod git;
pub mod init;
pub mod manifest;
pub mod materialize;
pub mod plan;
pub mod provenance;

use std::collections::HashMap;
use std::path::Path;

use diagnostics::{Diagnostic, DiagnosticCode};
use git::{GitResolver, ResolvedSource};
use manifest::SourceReference;
use plan::PlannedMaterialization;
use provenance::{ProvenanceEntry, ProvenanceRecord, ProvenanceSource};

pub const MANIFEST_FILE_NAME: &str = "enozunu.kdl";
pub const PROVENANCE_VERSION: u32 = 1;

/// What a source reference resolved to in this run.
///
/// Only Git sources have a resolved revision; local sources report their canonical path instead.
#[derive(Debug)]
pub enum ResolvedOrigin {
    Git { revision: String },
    Local { resolved_path: String },
}

/// The outcome of one materialized entry, reported back to the CLI.
#[derive(Debug)]
pub struct MaterializedEntry {
    pub source_name: String,
    pub kind: plan::ArtifactKind,
    pub origin: ResolvedOrigin,
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

    // Relative `local.path` values resolve from the manifest file's directory, not the process working directory.
    let manifest_dir = match manifest_path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };

    let resolved = resolve_git_sources(&planned, resolver)?;

    let mut checked = Vec::new();
    let mut diags = Vec::new();
    for entry in &planned {
        let source_base = match &entry.reference {
            SourceReference::Git { url, branch, .. } => {
                &resolved[&(url.clone(), branch.clone())].checkout_dir
            }
            SourceReference::Local { .. } => manifest_dir,
        };
        match materialize::check(entry, source_base, project_root) {
            Ok(c) => {
                let origin = match &entry.reference {
                    SourceReference::Git { url, branch, .. } => ResolvedOrigin::Git {
                        revision: resolved[&(url.clone(), branch.clone())].commit.clone(),
                    },
                    SourceReference::Local { .. } => ResolvedOrigin::Local {
                        resolved_path: c.source_abs.display().to_string(),
                    },
                };
                checked.push((entry, origin, c));
            }
            Err(d) => diags.push(d),
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }

    let mut results = Vec::new();
    let mut provenance_entries = Vec::new();
    for (entry, origin, checked) in checked {
        materialize::execute(&checked).map_err(|d| vec![d])?;
        provenance_entries.push(ProvenanceEntry {
            source_name: entry.source_name.clone(),
            kind: entry.kind.as_str().to_owned(),
            source: provenance_source(&entry.reference, &origin),
            target_ai: "claude".to_owned(),
            target_path: entry.target_rel_path.clone(),
        });
        results.push(MaterializedEntry {
            source_name: entry.source_name.clone(),
            kind: entry.kind,
            origin,
            target_rel_path: entry.target_rel_path.clone(),
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

fn provenance_source(reference: &SourceReference, origin: &ResolvedOrigin) -> ProvenanceSource {
    match (reference, origin) {
        (SourceReference::Git { url, branch, path }, ResolvedOrigin::Git { revision }) => {
            ProvenanceSource::Git {
                url: url.clone(),
                branch: branch.clone(),
                path: path.clone(),
                resolved_revision: revision.clone(),
            }
        }
        (SourceReference::Local { path }, ResolvedOrigin::Local { resolved_path }) => {
            ProvenanceSource::Local {
                path: path.clone(),
                resolved_path: resolved_path.clone(),
            }
        }
        // The origin is constructed from the reference in run_materialize, so the variants always match.
        _ => unreachable!("resolved origin kind diverged from its source reference kind"),
    }
}

/// Resolves each distinct Git (url, branch) pair once so a single run sees one consistent commit per branch.
///
/// Local references need no resolver: their path is checked directly against the filesystem.
fn resolve_git_sources(
    planned: &[PlannedMaterialization],
    resolver: &dyn GitResolver,
) -> Result<HashMap<(String, String), ResolvedSource>, Vec<Diagnostic>> {
    let mut resolved = HashMap::new();
    let mut diags = Vec::new();
    for entry in planned {
        let SourceReference::Git { url, branch, .. } = &entry.reference else {
            continue;
        };
        let key = (url.clone(), branch.clone());
        if resolved.contains_key(&key) {
            continue;
        }
        match resolver.resolve(url, branch) {
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
