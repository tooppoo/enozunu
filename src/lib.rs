//! Enozunu materializes declared AI-agent configuration sources into target AI-native paths.
//!
//! The pipeline is: parse and validate the manifest, plan target paths, resolve Git and local sources, check every artifact, then write outputs and the provenance record.

pub mod diagnostics;
pub mod gist;
pub mod git;
pub mod init;
pub mod manifest;
pub mod materialize;
pub mod plan;
pub mod provenance;

use std::collections::HashMap;
use std::path::Path;

use diagnostics::{Diagnostic, DiagnosticCode};
use gist::{GistRequest, GistResolver, GitTransportGistResolver};
use git::{GitError, GitResolutionRequest, GitResolver, GitSelector, ResolvedSource};
use manifest::{GistArtifactSelector, SourceReference, TargetAi};
use plan::PlannedMaterialization;
use provenance::{ProvenanceEntry, ProvenanceRecord, ProvenanceSource};

pub const MANIFEST_FILE_NAME: &str = "enozunu.kdl";
pub const PROVENANCE_VERSION: u32 = 1;

/// What a source reference resolved to in this run.
///
/// Each variant preserves the identity of its source kind: a Git source reports its resolved revision, a local source its canonical path, and a Gist source its id and pinned revision. A Gist is kept distinct from a Git origin even though Git transport materialized it.
#[derive(Debug)]
pub enum ResolvedOrigin {
    Git { revision: String },
    Local { resolved_path: String },
    Gist { id: String, revision: String },
}

impl ResolvedOrigin {
    /// Renders the origin for CLI output, keeping each source kind identifiable.
    ///
    /// A Gist is rendered as `gist: <id>@<revision>` so materialized Gist sources are never mistaken for ordinary Git sources.
    pub fn describe(&self) -> String {
        match self {
            ResolvedOrigin::Git { revision } => revision.clone(),
            ResolvedOrigin::Local { resolved_path } => format!("local: {resolved_path}"),
            ResolvedOrigin::Gist { id, revision } => format!("gist: {id}@{revision}"),
        }
    }
}

/// The outcome of one materialized entry, reported back to the CLI.
#[derive(Debug)]
pub struct MaterializedEntry {
    pub source_name: String,
    pub kind: plan::ArtifactKind,
    pub target_ai: TargetAi,
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
    // Gist sources resolve over the same Git transport, but through the Gist boundary so failures carry Gist-specific diagnostic codes.
    let gist_resolver = GitTransportGistResolver::new(resolver);
    let resolved_gists = resolve_gist_sources(&planned, &gist_resolver)?;

    // Local sources are checked against every target this run writes, not only their own.
    let target_rel_paths: Vec<String> = planned
        .iter()
        .map(|entry| entry.target_rel_path.clone())
        .collect();

    let mut checked = Vec::new();
    let mut diags = Vec::new();
    for entry in &planned {
        let source_base = match &entry.reference {
            SourceReference::Git { url, branch, .. } => {
                &resolved[&(url.clone(), branch.clone())].content_root
            }
            SourceReference::Local { .. } => manifest_dir,
            SourceReference::Gist { id, revision, .. } => {
                &resolved_gists[&gist_key(id, revision)].content_root
            }
        };
        match materialize::check(entry, source_base, project_root, &target_rel_paths) {
            Ok(c) => {
                let origin = match &entry.reference {
                    SourceReference::Git { url, branch, .. } => ResolvedOrigin::Git {
                        revision: resolved[&(url.clone(), branch.clone())].commit.clone(),
                    },
                    SourceReference::Local { .. } => ResolvedOrigin::Local {
                        resolved_path: c.source_abs.display().to_string(),
                    },
                    SourceReference::Gist { id, revision, .. } => ResolvedOrigin::Gist {
                        id: id.as_str().to_owned(),
                        revision: resolved_gists[&gist_key(id, revision)].commit.clone(),
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
            target_ai: entry.target_ai.as_str().to_owned(),
            target_path: entry.target_rel_path.clone(),
        });
        results.push(MaterializedEntry {
            source_name: entry.source_name.clone(),
            kind: entry.kind,
            target_ai: entry.target_ai,
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
        // The resolved revision equals the pinned revision (checkout is verified against it), so it is recorded as `gist`, never as `git`, even though Git transport produced it.
        (SourceReference::Gist { selector, .. }, ResolvedOrigin::Gist { id, revision }) => {
            ProvenanceSource::Gist {
                id: id.clone(),
                revision: revision.clone(),
                // A root-selecting Skill Gist records no `file`, keeping the provenance shape aligned with the manifest contract.
                file: match selector {
                    GistArtifactSelector::Root => None,
                    GistArtifactSelector::File { path } => Some(path.clone()),
                },
            }
        }
        // The origin is constructed from the reference in run_materialize, so the variants always match.
        _ => unreachable!("resolved origin kind diverged from its source reference kind"),
    }
}

/// The immutable cache key for a resolved Gist: `(id, revision)`.
///
/// The artifact kind and selector are not part of the key, so Skill and agent sources referencing one Gist revision share a single resolved content tree.
fn gist_key(id: &manifest::GistId, revision: &git::CommitSha) -> (String, String) {
    (id.as_str().to_owned(), revision.as_str().to_owned())
}

/// Resolves each distinct Git (url, branch) pair once so a single run sees one consistent commit per branch.
///
/// Local and Gist references are skipped here: local paths are checked directly against the filesystem, and Gist sources resolve through the Gist boundary.
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
        let request = GitResolutionRequest {
            url: url.clone(),
            selector: GitSelector::Branch(branch.clone()),
        };
        match resolver.resolve(&request) {
            Ok(source) => {
                resolved.insert(key, source);
            }
            Err(e) => diags.push(git_error_diagnostic(e)),
        }
    }
    if diags.is_empty() {
        Ok(resolved)
    } else {
        Err(diags)
    }
}

/// Maps a Git-source transport failure to a diagnostic.
///
/// For a Git source, both a fetch failure and an unresolved branch are `GitResolution`; only a local filesystem failure is `Io`. Gist sources use their own mapping so their transport failures are not reported as `GitResolution`.
fn git_error_diagnostic(error: GitError) -> Diagnostic {
    match error {
        GitError::Fetch(message) | GitError::RevisionNotFound(message) => {
            Diagnostic::new(DiagnosticCode::GitResolution, message)
        }
        GitError::Io(message) => Diagnostic::new(DiagnosticCode::Io, message),
    }
}

/// Resolves each distinct Gist `(id, revision)` once so every source referencing that revision — Skill or agent — shares one exported content tree.
fn resolve_gist_sources(
    planned: &[PlannedMaterialization],
    resolver: &dyn GistResolver,
) -> Result<HashMap<(String, String), ResolvedSource>, Vec<Diagnostic>> {
    let mut resolved = HashMap::new();
    let mut diags = Vec::new();
    for entry in planned {
        let SourceReference::Gist { id, revision, .. } = &entry.reference else {
            continue;
        };
        let key = gist_key(id, revision);
        if resolved.contains_key(&key) {
            continue;
        }
        let request = GistRequest {
            id: id.clone(),
            revision: revision.clone(),
        };
        match resolver.resolve(&request) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_keeps_each_origin_kind_identifiable() {
        assert_eq!(
            ResolvedOrigin::Git {
                revision: "abc123".to_owned()
            }
            .describe(),
            "abc123"
        );
        assert_eq!(
            ResolvedOrigin::Local {
                resolved_path: "/repo/skills/demo".to_owned()
            }
            .describe(),
            "local: /repo/skills/demo"
        );
        assert_eq!(
            ResolvedOrigin::Gist {
                id: "2decf6c462d9b4418f2".to_owned(),
                revision: "468aac8caed5f0c3b859b8286968e2c78e2b8760".to_owned(),
            }
            .describe(),
            "gist: 2decf6c462d9b4418f2@468aac8caed5f0c3b859b8286968e2c78e2b8760"
        );
    }
}
