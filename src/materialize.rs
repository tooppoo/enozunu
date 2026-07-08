//! Executes materialization plans against the project filesystem.
//!
//! This module owns artifact-shape checks and filesystem safety.
//! It never writes outside the project root and rejects symlinked sources instead of following them.

use std::fs;
use std::path::{Path, PathBuf};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::plan::{ArtifactKind, PlannedMaterialization};

/// A materialization whose source shape and path containment are already verified.
///
/// Separating checking from execution lets the pipeline validate every entry before the first write, so an invalid manifest does not leave the target half-updated.
#[derive(Debug, Clone)]
pub struct CheckedMaterialization {
    pub source_abs: PathBuf,
    pub target_abs: PathBuf,
    pub kind: ArtifactKind,
}

/// Verifies the source artifact for `entry` inside `checkout_dir` without touching the target.
pub fn check(
    entry: &PlannedMaterialization,
    checkout_dir: &Path,
    project_root: &Path,
) -> Result<CheckedMaterialization, Diagnostic> {
    let checkout_canon = checkout_dir.canonicalize().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to resolve checkout directory: {e}"),
        )
    })?;

    let source_abs = checkout_canon.join(&entry.reference.path);
    let source_canon = source_abs.canonicalize().map_err(|_| {
        Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!(
                "{} `{}`: source path `{}` does not exist in the resolved repository",
                entry.kind.as_str(),
                entry.source_name,
                entry.reference.path
            ),
        )
    })?;

    // Canonicalization resolves symlinks, so this containment check also rejects links pointing outside the checkout.
    if !source_canon.starts_with(&checkout_canon) {
        return Err(Diagnostic::new(
            DiagnosticCode::UnsafePath,
            format!(
                "{} `{}`: source path `{}` escapes the resolved repository",
                entry.kind.as_str(),
                entry.source_name,
                entry.reference.path
            ),
        ));
    }

    match entry.kind {
        ArtifactKind::Skill => {
            if !source_canon.is_dir() {
                return Err(Diagnostic::new(
                    DiagnosticCode::ArtifactShape,
                    format!(
                        "skill `{}`: source path `{}` is not a directory",
                        entry.source_name, entry.reference.path
                    ),
                ));
            }
            if !source_canon.join("SKILL.md").is_file() {
                return Err(Diagnostic::new(
                    DiagnosticCode::ArtifactShape,
                    format!(
                        "skill `{}`: source directory `{}` does not contain SKILL.md",
                        entry.source_name, entry.reference.path
                    ),
                ));
            }
            reject_symlinks(&source_canon, &entry.source_name)?;
        }
        ArtifactKind::Agent => {
            if !source_canon.is_file() {
                return Err(Diagnostic::new(
                    DiagnosticCode::ArtifactShape,
                    format!(
                        "agent `{}`: source path `{}` is not a file",
                        entry.source_name, entry.reference.path
                    ),
                ));
            }
        }
    }

    Ok(CheckedMaterialization {
        source_abs: source_canon,
        target_abs: project_root.join(&entry.target_rel_path),
        kind: entry.kind,
    })
}

/// Writes a checked materialization to its target path.
///
/// Existing targets are replaced, not merged, so files removed from the source also disappear from the target.
/// See docs/generated-output.md for the replace-semantics policy.
pub fn execute(checked: &CheckedMaterialization) -> Result<(), Diagnostic> {
    let target = &checked.target_abs;

    if target.symlink_metadata().is_ok() {
        remove_any(target)?;
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(io_diag)?;
    }

    match checked.kind {
        ArtifactKind::Skill => copy_dir(&checked.source_abs, target),
        ArtifactKind::Agent => {
            fs::copy(&checked.source_abs, target).map_err(io_diag)?;
            Ok(())
        }
    }
}

fn copy_dir(source: &Path, target: &Path) -> Result<(), Diagnostic> {
    fs::create_dir_all(target).map_err(io_diag)?;
    for entry in fs::read_dir(source).map_err(io_diag)? {
        let entry = entry.map_err(io_diag)?;
        let file_type = entry.file_type().map_err(io_diag)?;
        let target_child = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&entry.path(), &target_child)?;
        } else {
            fs::copy(entry.path(), &target_child).map_err(io_diag)?;
        }
    }
    Ok(())
}

/// Symlinks inside a Skill source are rejected outright.
/// Following them could copy content from outside the checkout, and reproducing them could point generated output outside the target root.
fn reject_symlinks(dir: &Path, source_name: &str) -> Result<(), Diagnostic> {
    for entry in fs::read_dir(dir).map_err(io_diag)? {
        let entry = entry.map_err(io_diag)?;
        let file_type = entry.file_type().map_err(io_diag)?;
        if file_type.is_symlink() {
            return Err(Diagnostic::new(
                DiagnosticCode::UnsafePath,
                format!(
                    "skill `{}`: source contains a symlink at `{}`; symlinks are not materialized",
                    source_name,
                    entry.path().display()
                ),
            ));
        }
        if file_type.is_dir() {
            reject_symlinks(&entry.path(), source_name)?;
        }
    }
    Ok(())
}

fn remove_any(path: &Path) -> Result<(), Diagnostic> {
    let metadata = path.symlink_metadata().map_err(io_diag)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(io_diag)
    } else {
        fs::remove_file(path).map_err(io_diag)
    }
}

fn io_diag(e: std::io::Error) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::Io,
        format!("filesystem operation failed: {e}"),
    )
}
