//! Enozunu materializes declared AI-agent configuration sources into target AI-native paths.
//!
//! The pipeline is: parse and validate the manifest, plan target paths, resolve Git and local sources, check every artifact, then write outputs, the provenance record, and the lock file.

pub mod diagnostics;
pub mod gist;
pub mod git;
pub mod init;
pub mod lock;
pub mod manifest;
pub mod materialize;
pub mod plan;
pub mod provenance;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use diagnostics::{Diagnostic, DiagnosticCode};
use gist::{GistRequest, GistResolver, GitTransportGistResolver};
use git::{CommitSha, GitError, GitResolutionRequest, GitResolver, GitSelector, ResolvedSource};
use manifest::{GistArtifactSelector, SourceReference, TargetAi};
use plan::PlannedMaterialization;
use provenance::{ProvenanceEntry, ProvenanceGitSelector, ProvenanceRecord, ProvenanceSource};

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

/// How one run treats the lock file.
///
/// `Locked` is the default: reproducibility is opt-out, not opt-in, so a plain `summon` never
/// silently follows a moved branch or tag once a revision is recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// Resolve mutable selectors from recorded revisions when present; resolve and record anything missing.
    Locked,
    /// Ignore recorded revisions, re-resolve every mutable selector, and rewrite the lock file.
    Update,
    /// Resolve strictly from the lock file and never write it; fail when any mutable source is unlocked.
    Frozen,
}

/// What this run did to the lock file, reported so the CLI announces only real file changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockOutcome {
    Created,
    Updated,
    Unchanged,
    /// Frozen mode reads the lock as an input and must leave it untouched.
    NotWritten,
}

/// The full result of one materialization run.
#[derive(Debug)]
pub struct MaterializeOutcome {
    pub entries: Vec<MaterializedEntry>,
    pub lock: LockOutcome,
    /// Where the lock file lives for this run, so the CLI reports the same path the pipeline used.
    pub lock_path: PathBuf,
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

/// Runs the full materialization pipeline and writes the provenance record and the lock file.
///
/// All sources are resolved and all artifacts are checked before the first target write, so a failing entry does not leave the project half-updated.
pub fn run_materialize(
    manifest_path: &Path,
    project_root: &Path,
    resolver: &dyn GitResolver,
    lock_mode: LockMode,
) -> Result<MaterializeOutcome, Vec<Diagnostic>> {
    let manifest = load_manifest(manifest_path)?;
    let planned = plan::plan(&manifest)?;

    // Relative `local.path` values resolve from the manifest file's directory, not the process working directory.
    let manifest_dir = match manifest_path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };

    // A corrupt or unsupported lock file fails every mode, including `Update`: silently rebuilding
    // over a record the user may have meant to keep would destroy the pinned revisions it held.
    let lock_path = manifest_dir.join(lock::LOCK_FILE_NAME);
    let lock_record = lock::read(&lock_path).map_err(|d| vec![d])?;
    let locked = match lock_mode {
        LockMode::Update => HashMap::new(),
        LockMode::Locked | LockMode::Frozen => lock_record
            .as_ref()
            .map(lock::locked_revisions)
            .unwrap_or_default(),
    };

    if lock_mode == LockMode::Frozen {
        let diags = frozen_lock_diagnostics(&planned, lock_record.is_some(), &lock_path, &locked);
        if !diags.is_empty() {
            return Err(diags);
        }
    }

    let resolved = resolve_git_sources(&planned, resolver, &locked)?;
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
            SourceReference::Git { url, selector, .. } => {
                &resolved[&(url.clone(), selector.clone())].content_root
            }
            SourceReference::Local { .. } => manifest_dir,
            SourceReference::Gist { id, revision, .. } => {
                &resolved_gists[&gist_key(id, revision)].content_root
            }
        };
        match materialize::check(entry, source_base, project_root, &target_rel_paths) {
            Ok(c) => {
                let origin = match &entry.reference {
                    SourceReference::Git { url, selector, .. } => ResolvedOrigin::Git {
                        revision: resolved[&(url.clone(), selector.clone())].commit.clone(),
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

    let lock_outcome = match lock_mode {
        LockMode::Frozen => LockOutcome::NotWritten,
        LockMode::Locked | LockMode::Update => {
            let record = lock::build(&resolved).map_err(|d| vec![d])?;
            match lock::write(&lock_path, &record).map_err(|d| vec![d])? {
                lock::WriteOutcome::Created => LockOutcome::Created,
                lock::WriteOutcome::Updated => LockOutcome::Updated,
                lock::WriteOutcome::Unchanged => LockOutcome::Unchanged,
            }
        }
    };

    Ok(MaterializeOutcome {
        entries: results,
        lock: lock_outcome,
        lock_path,
    })
}

/// Collects every reason frozen mode cannot resolve this run from the lock file.
///
/// A missing lock file fails unconditionally, even for a manifest with no mutable sources: every
/// successful non-frozen run writes a lock — an empty one when nothing is mutable — so a missing
/// file always means the lock was never created or never committed, which is exactly what frozen
/// mode exists to catch.
/// Per-source gaps are gathered before failing — matching how manifest validation reports
/// evidence — so one frozen run names every source that needs locking instead of failing one
/// source at a time.
/// The check runs before any resolution, keeping a frozen failure free of network side effects.
fn frozen_lock_diagnostics(
    planned: &[PlannedMaterialization],
    lock_file_exists: bool,
    lock_path: &Path,
    locked: &HashMap<(String, GitSelector), CommitSha>,
) -> Vec<Diagnostic> {
    if !lock_file_exists {
        return vec![Diagnostic::new(
            DiagnosticCode::LockOutOfDate,
            format!(
                "cannot materialize with --frozen: {} not found; run `enozunu summon` to create it",
                lock_path.display()
            ),
        )];
    }
    let mut diags = Vec::new();
    let mut seen = HashSet::new();
    for entry in planned {
        let SourceReference::Git { url, selector, .. } = &entry.reference else {
            continue;
        };
        let (kind, value) = match selector {
            GitSelector::Branch(branch) => ("branch", branch),
            GitSelector::Tag(tag) => ("tag", tag),
            GitSelector::Revision(_) => continue,
        };
        let key = (url.clone(), selector.clone());
        if locked.contains_key(&key) || !seen.insert(key) {
            continue;
        }
        diags.push(Diagnostic::new(
            DiagnosticCode::LockOutOfDate,
            format!(
                "cannot materialize with --frozen: source `{url}` ({kind} `{value}`) has no entry in {}; run `enozunu summon` to lock it",
                lock_path.display()
            ),
        ));
    }
    diags
}

fn provenance_source(reference: &SourceReference, origin: &ResolvedOrigin) -> ProvenanceSource {
    match (reference, origin) {
        (
            SourceReference::Git {
                url,
                selector,
                path,
            },
            ResolvedOrigin::Git { revision },
        ) => ProvenanceSource::Git {
            url: url.clone(),
            selector: match selector {
                GitSelector::Branch(branch) => ProvenanceGitSelector::Branch(branch.clone()),
                GitSelector::Tag(tag) => ProvenanceGitSelector::Tag(tag.clone()),
                GitSelector::Revision(sha) => {
                    ProvenanceGitSelector::Revision(sha.as_str().to_owned())
                }
            },
            path: path.clone(),
            resolved_revision: revision.clone(),
        },
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

/// Resolves each distinct Git (url, selector) pair once so a single run sees one consistent commit per selector.
///
/// The selector — kind and value — is part of the key, so a branch whose name looks like a commit id never shares a resolution with a revision of the same text, and a branch never shares one with a tag of the same name.
/// Local and Gist references are skipped here: local paths are checked directly against the filesystem, and Gist sources resolve through the Gist boundary.
///
/// A `locked` hit swaps the request selector for the recorded revision while the result stays keyed
/// by the declared selector, so callers and provenance keep addressing sources by what the manifest
/// says. Resolving through the revision path also re-verifies that the materialized commit equals
/// the recorded one, so a locked revision is checked, not trusted.
fn resolve_git_sources(
    planned: &[PlannedMaterialization],
    resolver: &dyn GitResolver,
    locked: &HashMap<(String, GitSelector), CommitSha>,
) -> Result<HashMap<(String, GitSelector), ResolvedSource>, Vec<Diagnostic>> {
    let mut resolved = HashMap::new();
    let mut diags = Vec::new();
    for entry in planned {
        let SourceReference::Git { url, selector, .. } = &entry.reference else {
            continue;
        };
        let key = (url.clone(), selector.clone());
        if resolved.contains_key(&key) {
            continue;
        }
        let locked_revision = locked.get(&key);
        let request = GitResolutionRequest {
            url: url.clone(),
            selector: match locked_revision {
                Some(revision) => GitSelector::Revision(revision.clone()),
                None => selector.clone(),
            },
        };
        match resolver.resolve(&request) {
            Ok(source) => {
                resolved.insert(key, source);
            }
            Err(e) => diags.push(match (locked_revision, e) {
                // A recorded revision can vanish upstream (force-push plus pruning); the declared
                // selector alone cannot say so, so the report points at the lock as the cause and
                // names the way out.
                (Some(_), GitError::RevisionNotFound(message)) => Diagnostic::new(
                    DiagnosticCode::GitResolution,
                    format!(
                        "{message}; the locked revision may no longer exist upstream; run `enozunu summon --update` to re-resolve it"
                    ),
                ),
                (_, e) => git_error_diagnostic(e),
            }),
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
/// For a Git source, a fetch failure and an unresolved branch or revision are all `GitResolution`; only a local filesystem failure is `Io`. Gist sources use their own mapping so their transport failures are not reported as `GitResolution`.
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
