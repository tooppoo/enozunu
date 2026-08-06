//! Executes materialization plans against the project filesystem.
//!
//! This module owns artifact-shape checks and filesystem safety.
//! It rejects symlinked sources instead of following them.
//! A symlink at the final target component is replaced itself, never followed, so a summon cannot destroy the object the symlink points to; symlinked target ancestors are traversed like ordinary directories, so the physical write location can lie outside the project root.

use std::fs;
use std::path::{Path, PathBuf};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::manifest::{GistArtifactSelector, SourceReference};
use crate::plan::{ArtifactKind, PlannedMaterialization};

/// A materialization whose source shape and path containment are already verified.
///
/// Separating checking from execution lets the pipeline validate every entry before the first write, so an invalid manifest does not leave the target half-updated.
#[derive(Debug, Clone)]
pub struct CheckedMaterialization {
    pub source_abs: PathBuf,
    pub target_abs: PathBuf,
    pub kind: ArtifactKind,
    /// Pre-rendered file content for instruction artifacts; `None` materializes the source verbatim.
    ///
    /// The pipeline renders instructions during the check phase — before the first target write — so a decode or render failure cannot leave any target half-updated.
    pub rendered: Option<String>,
}

/// Verifies the source artifact for `entry` without touching the target.
///
/// `source_base` is the exported content root for Git and Gist references and the manifest file's containing directory for local references.
/// `planned_target_rel_paths` lists every target path the current run will write; local sources are checked for overlap against all of them, because any of those writes could destroy an overlapping local source mid-run.
pub fn check(
    entry: &PlannedMaterialization,
    source_base: &Path,
    project_root: &Path,
    planned_target_rel_paths: &[String],
) -> Result<CheckedMaterialization, Diagnostic> {
    let checked = check_source(entry, source_base, project_root, planned_target_rel_paths)?;

    // A root instruction file replaces an existing regular file or symlink under its declared ownership, but never a directory: deleting a directory tree to place a generated file exceeds that ownership, so summon fails instead.
    if entry.kind == ArtifactKind::Instruction
        && checked
            .target_abs
            .symlink_metadata()
            .is_ok_and(|m| m.is_dir())
    {
        return Err(Diagnostic::new(
            DiagnosticCode::UnsafePath,
            format!(
                "{} `{}`: target path `{}` is a directory; refusing to replace a directory with a generated file; remove or relocate the directory, then rerun `enozunu summon`",
                entry.kind.as_str(),
                entry.source_name,
                entry.target_rel_path
            ),
        ));
    }

    Ok(checked)
}

fn check_source(
    entry: &PlannedMaterialization,
    source_base: &Path,
    project_root: &Path,
    planned_target_rel_paths: &[String],
) -> Result<CheckedMaterialization, Diagnostic> {
    match &entry.reference {
        SourceReference::Git { path, .. } => {
            check_git_source(entry, path, source_base, project_root)
        }
        SourceReference::Local { path } => check_local_source(
            entry,
            path,
            source_base,
            project_root,
            planned_target_rel_paths,
        ),
        SourceReference::Gist { selector, .. } => match selector {
            GistArtifactSelector::Root => check_gist_root_source(entry, source_base, project_root),
            GistArtifactSelector::File { path } => {
                check_gist_file_source(entry, path, source_base, project_root)
            }
        },
    }
}

/// Verifies a Gist Skill artifact: the exported revision root itself.
///
/// The root is the artifact, so shape checks apply to it directly: it must be a directory containing a regular-file `SKILL.md`, and the tree follows the same symlink policy as every other Skill source.
fn check_gist_root_source(
    entry: &PlannedMaterialization,
    content_root: &Path,
    project_root: &Path,
) -> Result<CheckedMaterialization, Diagnostic> {
    let root_canon = content_root.canonicalize().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to resolve gist content root: {e}"),
        )
    })?;

    if !root_canon.is_dir() {
        return Err(Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!(
                "skill `{}`: gist revision root is not a directory",
                entry.source_name
            ),
        ));
    }
    if !root_canon.join("SKILL.md").is_file() {
        return Err(Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!(
                "skill `{}`: gist revision root does not contain SKILL.md",
                entry.source_name
            ),
        ));
    }
    reject_symlinks(&root_canon, &entry.source_name)?;

    Ok(CheckedMaterialization {
        source_abs: root_canon,
        target_abs: project_root.join(&entry.target_rel_path),
        kind: entry.kind,
        rendered: None,
    })
}

/// Verifies a file-shaped Gist artifact inside its exported content root.
///
/// A missing file is reported as `SourcePathNotFound` and a non-file artifact as `ArtifactShape`, so a mistyped `file` is distinguished from a `file` that points at a directory.
/// Containment is enforced after canonicalization, so a symlink whose target escapes the exported content is rejected even though Git transport produced it.
/// Diagnostics name the entry's actual artifact kind, so a failure of any file-shaped kind is never reported as a different kind's.
fn check_gist_file_source(
    entry: &PlannedMaterialization,
    file: &str,
    content_root: &Path,
    project_root: &Path,
) -> Result<CheckedMaterialization, Diagnostic> {
    let root_canon = content_root.canonicalize().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to resolve gist content root: {e}"),
        )
    })?;

    let source_abs = root_canon.join(file);
    let source_canon = source_abs.canonicalize().map_err(|_| {
        Diagnostic::new(
            DiagnosticCode::SourcePathNotFound,
            format!(
                "{} `{}`: gist file `{}` does not exist in the resolved revision",
                entry.kind.as_str(),
                entry.source_name,
                file
            ),
        )
    })?;

    // Canonicalization resolves symlinks, so this containment check also rejects a link whose target points outside the exported content.
    if !source_canon.starts_with(&root_canon) {
        return Err(Diagnostic::new(
            DiagnosticCode::UnsafePath,
            format!(
                "{} `{}`: gist file `{}` escapes the resolved revision",
                entry.kind.as_str(),
                entry.source_name,
                file
            ),
        ));
    }

    // A file-shaped Gist artifact must be a regular file; `file` pointing at a directory (the Gist root or a subdirectory) is a shape error.
    if !source_canon.is_file() {
        return Err(Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!(
                "{} `{}`: gist file `{}` is not a regular file",
                entry.kind.as_str(),
                entry.source_name,
                file
            ),
        ));
    }

    Ok(CheckedMaterialization {
        source_abs: source_canon,
        target_abs: project_root.join(&entry.target_rel_path),
        kind: entry.kind,
        rendered: None,
    })
}

fn check_git_source(
    entry: &PlannedMaterialization,
    source_path: &str,
    content_root: &Path,
    project_root: &Path,
) -> Result<CheckedMaterialization, Diagnostic> {
    let checkout_canon = content_root.canonicalize().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to resolve checkout directory: {e}"),
        )
    })?;

    let source_abs = checkout_canon.join(source_path);
    let source_canon = source_abs.canonicalize().map_err(|_| {
        Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!(
                "{} `{}`: source path `{}` does not exist in the resolved repository",
                entry.kind.as_str(),
                entry.source_name,
                source_path
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
                source_path
            ),
        ));
    }

    check_artifact_shape(entry, &source_canon, source_path)?;

    Ok(CheckedMaterialization {
        source_abs: source_canon,
        target_abs: project_root.join(&entry.target_rel_path),
        kind: entry.kind,
        rendered: None,
    })
}

fn check_local_source(
    entry: &PlannedMaterialization,
    source_path: &str,
    manifest_dir: &Path,
    project_root: &Path,
    planned_target_rel_paths: &[String],
) -> Result<CheckedMaterialization, Diagnostic> {
    let manifest_dir_canon = manifest_dir.canonicalize().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to resolve manifest directory: {e}"),
        )
    })?;

    let source_abs = manifest_dir_canon.join(source_path);

    // Git sources get symlink containment from the checkout boundary; local sources have no such boundary, so a symlink at the artifact path itself is rejected outright.
    match source_abs.symlink_metadata() {
        Ok(metadata) if metadata.is_symlink() => {
            return Err(Diagnostic::new(
                DiagnosticCode::UnsafePath,
                format!(
                    "{} `{}`: local source path `{}` is a symlink; symlinked sources are not materialized",
                    entry.kind.as_str(),
                    entry.source_name,
                    source_path
                ),
            ));
        }
        Ok(_) => {}
        Err(_) => {
            return Err(Diagnostic::new(
                DiagnosticCode::ArtifactShape,
                format!(
                    "{} `{}`: local source path `{}` does not exist (resolved from the manifest directory)",
                    entry.kind.as_str(),
                    entry.source_name,
                    source_path
                ),
            ));
        }
    }

    let source_canon = source_abs.canonicalize().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!(
                "{} `{}`: failed to resolve local source path `{}`: {e}",
                entry.kind.as_str(),
                entry.source_name,
                source_path
            ),
        )
    })?;

    // A local source can point back into the target project, so a source overlapping any target written this run would be deleted before copying or copied into itself.
    let project_canon = project_root.canonicalize().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to resolve project root: {e}"),
        )
    })?;
    for target_rel_path in planned_target_rel_paths {
        let overlap_target = overlap_check_target(&project_canon.join(target_rel_path))?;
        if source_canon.starts_with(&overlap_target) || overlap_target.starts_with(&source_canon) {
            return Err(Diagnostic::new(
                DiagnosticCode::UnsafePath,
                format!(
                    "{} `{}`: local source path `{}` overlaps the materialization target `{}`",
                    entry.kind.as_str(),
                    entry.source_name,
                    source_path,
                    target_rel_path
                ),
            ));
        }
    }
    // Execution replaces a symlink at the final target component itself (see the replace-semantics ADR), so the write target is the declared path, never its canonical form: writing through the canonical form would overwrite the symlink's destination — possibly outside the project root — instead of the symlink.
    let target_abs = project_canon.join(&entry.target_rel_path);

    check_artifact_shape(entry, &source_canon, source_path)?;

    Ok(CheckedMaterialization {
        source_abs: source_canon,
        target_abs,
        kind: entry.kind,
        rendered: None,
    })
}

/// Resolves a target path for overlap comparison so the comparison matches execution's delete-and-write semantics.
///
/// Symlinked ancestors are resolved because execution traverses them to the physical write location; a symlink at the final component is kept as its own path because execution replaces the symlink, not its destination, so only the symlink itself can collide with a source.
fn overlap_check_target(path: &Path) -> Result<PathBuf, Diagnostic> {
    if path.symlink_metadata().is_ok_and(|m| m.is_symlink()) {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(Diagnostic::new(
                DiagnosticCode::Io,
                format!("failed to resolve target path {}", path.display()),
            ));
        };
        let mut resolved = canonicalize_target(parent)?;
        resolved.push(name);
        return Ok(resolved);
    }
    canonicalize_target(path)
}

/// Canonicalizes a target path whose tail may not exist yet, by canonicalizing the deepest existing ancestor and re-appending the remaining components.
///
/// Comparing a canonical source against the lexical target path would miss overlap when an existing ancestor (such as a symlinked `.claude/skills`) resolves elsewhere; execution would then follow that symlink and destroy the source it resolves to.
fn canonicalize_target(path: &Path) -> Result<PathBuf, Diagnostic> {
    let mut existing = path.to_path_buf();
    let mut remainder = Vec::new();
    loop {
        match existing.canonicalize() {
            Ok(mut canon) => {
                for component in remainder.iter().rev() {
                    canon.push(component);
                }
                return Ok(canon);
            }
            Err(_) => {
                match existing.file_name() {
                    Some(name) => remainder.push(name.to_owned()),
                    None => {
                        return Err(Diagnostic::new(
                            DiagnosticCode::Io,
                            format!("failed to resolve target path {}", path.display()),
                        ));
                    }
                }
                if !existing.pop() {
                    return Err(Diagnostic::new(
                        DiagnosticCode::Io,
                        format!("failed to resolve target path {}", path.display()),
                    ));
                }
            }
        }
    }
}

/// Shape checks shared by Git and local sources: validation is shape-based, not origin-based.
/// See docs/design/adr/20260708T104202Z_no-source-origin-validation.md.
fn check_artifact_shape(
    entry: &PlannedMaterialization,
    source_canon: &Path,
    source_path: &str,
) -> Result<(), Diagnostic> {
    // Every file-shaped kind shares one check, and the report names the entry's actual kind, so a future file-shaped kind's failure is never reported as another kind's.
    if entry.kind.is_file_shaped() {
        if !source_canon.is_file() {
            return Err(Diagnostic::new(
                DiagnosticCode::ArtifactShape,
                format!(
                    "{} `{}`: source path `{}` is not a file",
                    entry.kind.as_str(),
                    entry.source_name,
                    source_path
                ),
            ));
        }
        return Ok(());
    }

    if !source_canon.is_dir() {
        return Err(Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!(
                "skill `{}`: source path `{}` is not a directory",
                entry.source_name, source_path
            ),
        ));
    }
    if !source_canon.join("SKILL.md").is_file() {
        return Err(Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!(
                "skill `{}`: source directory `{}` does not contain SKILL.md",
                entry.source_name, source_path
            ),
        ));
    }
    reject_symlinks(source_canon, &entry.source_name)
}

/// Writes a checked materialization to its target path.
///
/// Existing targets are replaced, not merged, so files removed from the source also disappear from the target.
/// See docs/design/adr/20260708T104205Z_generated-output-replace-semantics.md for the replace-semantics policy.
pub fn execute(checked: &CheckedMaterialization) -> Result<(), Diagnostic> {
    let target = &checked.target_abs;

    if target.symlink_metadata().is_ok() {
        remove_any(target)?;
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(io_diag)?;
    }

    if let Some(content) = &checked.rendered {
        fs::write(target, content).map_err(io_diag)
    } else if checked.kind.is_file_shaped() {
        fs::copy(&checked.source_abs, target).map_err(io_diag)?;
        Ok(())
    } else {
        copy_dir(&checked.source_abs, target)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{CommitSha, GitSelector};
    use crate::manifest::{GistId, SourceReference, TargetAi};

    fn target_rel_path(kind: ArtifactKind) -> String {
        match kind {
            ArtifactKind::Skill => ".claude/skills/demo".to_owned(),
            ArtifactKind::Agent => ".claude/agents/demo.md".to_owned(),
            ArtifactKind::Instruction => "CLAUDE.md".to_owned(),
        }
    }

    fn planned(kind: ArtifactKind, path: &str) -> PlannedMaterialization {
        PlannedMaterialization {
            source_name: "demo".to_owned(),
            kind,
            reference: SourceReference::Git {
                url: "https://example.com/repo".to_owned(),
                selector: GitSelector::Branch("main".to_owned()),
                path: path.to_owned(),
            },
            target_ai: TargetAi::Claude,
            target_rel_path: target_rel_path(kind),
        }
    }

    fn planned_local(kind: ArtifactKind, path: &str) -> PlannedMaterialization {
        PlannedMaterialization {
            source_name: "demo".to_owned(),
            kind,
            reference: SourceReference::Local {
                path: path.to_owned(),
            },
            target_ai: TargetAi::Claude,
            target_rel_path: target_rel_path(kind),
        }
    }

    fn planned_gist(kind: ArtifactKind, selector: GistArtifactSelector) -> PlannedMaterialization {
        PlannedMaterialization {
            source_name: "demo".to_owned(),
            kind,
            reference: SourceReference::Gist {
                id: GistId::parse("aa5a315d61ae9438b18d").unwrap(),
                revision: CommitSha::parse("468aac8caed5f0c3b859b8286968e2c78e2b8760").unwrap(),
                selector,
            },
            target_ai: TargetAi::Claude,
            target_rel_path: target_rel_path(kind),
        }
    }

    /// Wraps `check` with a single-entry run whose only planned target is the entry's own.
    fn check_single(
        entry: &PlannedMaterialization,
        source_base: &Path,
        project_root: &Path,
    ) -> Result<CheckedMaterialization, Diagnostic> {
        check(
            entry,
            source_base,
            project_root,
            std::slice::from_ref(&entry.target_rel_path),
        )
    }

    #[test]
    fn check_rejects_an_unresolvable_checkout_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let entry = planned(ArtifactKind::Agent, "agents/demo.md");
        let diag = check_single(&entry, &missing, tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::Io);
    }

    #[test]
    #[cfg(unix)]
    fn check_rejects_a_source_that_escapes_the_checkout() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let checkout = tmp.path().join("checkout");
        fs::create_dir_all(&checkout).unwrap();
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, checkout.join("escape")).unwrap();

        let entry = planned(ArtifactKind::Skill, "escape");
        let diag = check_single(&entry, &checkout, tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::UnsafePath);
    }

    #[test]
    fn check_rejects_a_skill_source_that_is_not_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let checkout = tmp.path().join("checkout");
        fs::create_dir_all(&checkout).unwrap();
        fs::write(checkout.join("demo"), "a file, not a directory").unwrap();

        let entry = planned(ArtifactKind::Skill, "demo");
        let diag = check_single(&entry, &checkout, tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::ArtifactShape);
    }

    #[test]
    fn check_rejects_an_agent_source_that_is_not_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let checkout = tmp.path().join("checkout");
        fs::create_dir_all(checkout.join("demo.md")).unwrap();

        let entry = planned(ArtifactKind::Agent, "demo.md");
        let diag = check_single(&entry, &checkout, tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::ArtifactShape);
        // The shared file-shaped check must report the entry's actual kind, not a hard-coded one.
        assert!(
            diag.message.starts_with("agent `demo`"),
            "diagnostic must name the entry's kind: {}",
            diag.message
        );
    }

    #[test]
    fn check_rejects_an_instruction_target_that_is_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("base.md"), "# base\n").unwrap();
        // A directory occupies the root instruction path; replacing it would require deleting a tree.
        fs::create_dir_all(tmp.path().join("CLAUDE.md")).unwrap();

        let entry = planned_local(ArtifactKind::Instruction, "base.md");
        let diag = check_single(&entry, tmp.path(), tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::UnsafePath);
        assert!(
            diag.message.starts_with("instruction `demo`"),
            "diagnostic must name the entry's kind: {}",
            diag.message
        );
        assert!(
            tmp.path().join("CLAUDE.md").is_dir(),
            "the directory must survive"
        );
    }

    #[test]
    fn check_replaces_an_instruction_target_that_is_a_regular_file() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("base.md"), "# base\n").unwrap();
        fs::write(tmp.path().join("CLAUDE.md"), "old hand-written content\n").unwrap();

        let entry = planned_local(ArtifactKind::Instruction, "base.md");
        let mut checked = check_single(&entry, tmp.path(), tmp.path()).unwrap();
        checked.rendered = Some("generated\n".to_owned());
        execute(&checked).unwrap();

        assert_eq!(
            fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap(),
            "generated\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_symlinked_local_instruction_target_and_keeps_the_pointee() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("base.md"), "# base\n").unwrap();
        fs::write(tmp.path().join("victim.md"), "must survive\n").unwrap();
        // A symlinked root target must be replaced itself, never followed to its destination.
        symlink(tmp.path().join("victim.md"), tmp.path().join("CLAUDE.md")).unwrap();

        let entry = planned_local(ArtifactKind::Instruction, "base.md");
        let mut checked = check_single(&entry, tmp.path(), tmp.path()).unwrap();
        checked.rendered = Some("generated\n".to_owned());
        execute(&checked).unwrap();

        let target = tmp.path().join("CLAUDE.md");
        assert!(
            target.symlink_metadata().unwrap().is_file(),
            "the symlink must become a regular file"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "generated\n");
        assert_eq!(
            fs::read_to_string(tmp.path().join("victim.md")).unwrap(),
            "must survive\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_symlinked_git_instruction_target_and_keeps_the_pointee() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let checkout = tmp.path().join("checkout");
        fs::create_dir_all(&checkout).unwrap();
        fs::write(checkout.join("base.md"), "# base\n").unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("victim.md"), "must survive\n").unwrap();
        symlink(project.join("victim.md"), project.join("CLAUDE.md")).unwrap();

        let entry = planned(ArtifactKind::Instruction, "base.md");
        let mut checked = check_single(&entry, &checkout, &project).unwrap();
        checked.rendered = Some("generated\n".to_owned());
        execute(&checked).unwrap();

        let target = project.join("CLAUDE.md");
        assert!(target.symlink_metadata().unwrap().is_file());
        assert_eq!(fs::read_to_string(&target).unwrap(), "generated\n");
        assert_eq!(
            fs::read_to_string(project.join("victim.md")).unwrap(),
            "must survive\n"
        );
    }

    #[test]
    fn check_then_execute_copies_a_nested_skill_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let checkout = tmp.path().join("checkout");
        let skill = checkout.join("skills/demo");
        fs::create_dir_all(skill.join("nested")).unwrap();
        fs::write(skill.join("SKILL.md"), "# demo\n").unwrap();
        fs::write(skill.join("nested/extra.txt"), "child\n").unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();

        let checked = check_single(
            &planned(ArtifactKind::Skill, "skills/demo"),
            &checkout,
            &project,
        )
        .unwrap();
        execute(&checked).unwrap();

        assert!(project.join(".claude/skills/demo/SKILL.md").is_file());
        assert_eq!(
            fs::read_to_string(project.join(".claude/skills/demo/nested/extra.txt")).unwrap(),
            "child\n"
        );
    }

    #[test]
    fn check_resolves_a_local_source_from_the_manifest_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path().join("project");
        let sibling_skill = tmp.path().join("sibling/skills/demo");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::create_dir_all(&sibling_skill).unwrap();
        fs::write(sibling_skill.join("SKILL.md"), "# demo\n").unwrap();

        let entry = planned_local(ArtifactKind::Skill, "../sibling/skills/demo");
        let checked = check_single(&entry, &manifest_dir, &manifest_dir).unwrap();

        assert_eq!(checked.source_abs, sibling_skill.canonicalize().unwrap());
    }

    #[test]
    fn check_rejects_a_missing_local_source() {
        let tmp = tempfile::tempdir().unwrap();
        let entry = planned_local(ArtifactKind::Skill, "does-not-exist");
        let diag = check_single(&entry, tmp.path(), tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::ArtifactShape);
    }

    #[test]
    fn check_rejects_a_local_agent_source_that_is_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("demo.md")).unwrap();

        let entry = planned_local(ArtifactKind::Agent, "demo.md");
        let diag = check_single(&entry, tmp.path(), tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::ArtifactShape);
    }

    #[test]
    #[cfg(unix)]
    fn check_rejects_a_local_source_path_that_is_a_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real-skill");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# demo\n").unwrap();
        symlink(&real, tmp.path().join("linked-skill")).unwrap();

        let entry = planned_local(ArtifactKind::Skill, "linked-skill");
        let diag = check_single(&entry, tmp.path(), tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::UnsafePath);
    }

    #[test]
    #[cfg(unix)]
    fn check_rejects_a_symlink_inside_a_local_skill_source() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let skill = tmp.path().join("skill");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# demo\n").unwrap();
        fs::write(tmp.path().join("secret.txt"), "outside\n").unwrap();
        symlink("../secret.txt", skill.join("link.txt")).unwrap();

        let entry = planned_local(ArtifactKind::Skill, "skill");
        let diag = check_single(&entry, tmp.path(), tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::UnsafePath);
    }

    #[test]
    fn check_rejects_a_local_source_equal_to_its_target() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join(".claude/skills/demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "# demo\n").unwrap();

        let entry = planned_local(ArtifactKind::Skill, ".claude/skills/demo");
        let diag = check_single(&entry, tmp.path(), tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::UnsafePath);
    }

    #[test]
    fn check_rejects_a_local_source_that_is_an_ancestor_of_its_target() {
        let tmp = tempfile::tempdir().unwrap();
        // `.claude` is an ancestor of the `.claude/skills/demo` target.
        fs::create_dir_all(tmp.path().join(".claude")).unwrap();

        let entry = planned_local(ArtifactKind::Skill, ".claude");
        let diag = check_single(&entry, tmp.path(), tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::UnsafePath);
    }

    #[test]
    fn check_rejects_a_local_source_that_is_a_descendant_of_its_target() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".claude/skills/demo/inner")).unwrap();

        let entry = planned_local(ArtifactKind::Skill, ".claude/skills/demo/inner");
        let diag = check_single(&entry, tmp.path(), tmp.path()).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::UnsafePath);
    }

    #[test]
    #[cfg(unix)]
    fn check_rejects_a_local_source_reached_through_a_symlinked_target_ancestor() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let shared = tmp.path().join("shared");
        fs::create_dir_all(project.join(".claude")).unwrap();
        fs::create_dir_all(shared.join("demo")).unwrap();
        fs::write(shared.join("demo/SKILL.md"), "# demo\n").unwrap();
        // `.claude/skills` resolves outside the project, so the `.claude/skills/demo` target is the source itself.
        symlink(&shared, project.join(".claude/skills")).unwrap();

        let entry = planned_local(ArtifactKind::Skill, "../shared/demo");
        let diag = check_single(&entry, &project, &project).unwrap_err();

        assert_eq!(diag.code, DiagnosticCode::UnsafePath);
        assert!(shared.join("demo/SKILL.md").is_file());
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_symlinked_local_agent_target_and_keeps_the_pointee() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".claude/agents")).unwrap();
        fs::write(project.join("agent-src.md"), "generated agent\n").unwrap();
        let pointee = tmp.path().join("shared-agent.md");
        fs::write(&pointee, "must survive\n").unwrap();
        // A symlinked agent target must be replaced itself, never followed to its destination.
        symlink(&pointee, project.join(".claude/agents/demo.md")).unwrap();

        let entry = planned_local(ArtifactKind::Agent, "agent-src.md");
        let checked = check_single(&entry, &project, &project).unwrap();
        execute(&checked).unwrap();

        let target = project.join(".claude/agents/demo.md");
        assert!(
            target.symlink_metadata().unwrap().is_file(),
            "the symlink must become a regular file"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "generated agent\n");
        assert_eq!(fs::read_to_string(&pointee).unwrap(), "must survive\n");
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_broken_symlinked_local_agent_target() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".claude/agents")).unwrap();
        fs::write(project.join("agent-src.md"), "generated agent\n").unwrap();
        symlink(
            tmp.path().join("does-not-exist"),
            project.join(".claude/agents/demo.md"),
        )
        .unwrap();

        let entry = planned_local(ArtifactKind::Agent, "agent-src.md");
        let checked = check_single(&entry, &project, &project).unwrap();
        execute(&checked).unwrap();

        let target = project.join(".claude/agents/demo.md");
        assert!(target.symlink_metadata().unwrap().is_file());
        assert_eq!(fs::read_to_string(&target).unwrap(), "generated agent\n");
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_symlinked_local_skill_target_and_keeps_the_pointee_tree() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".claude/skills")).unwrap();
        let source = tmp.path().join("source-skill");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "# demo\n").unwrap();
        // The pointee holds files unrelated to the source; following the symlink would recursively delete them.
        let pointee = tmp.path().join("shared/demo");
        fs::create_dir_all(&pointee).unwrap();
        fs::write(pointee.join("SKILL.md"), "# shared\n").unwrap();
        fs::write(pointee.join("unrelated.txt"), "keep\n").unwrap();
        symlink(&pointee, project.join(".claude/skills/demo")).unwrap();

        let entry = planned_local(ArtifactKind::Skill, "../source-skill");
        let checked = check_single(&entry, &project, &project).unwrap();
        execute(&checked).unwrap();

        let target = project.join(".claude/skills/demo");
        let target_meta = target.symlink_metadata().unwrap();
        assert!(
            target_meta.is_dir() && !target_meta.is_symlink(),
            "the symlink must become a real directory"
        );
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "# demo\n"
        );
        assert_eq!(
            fs::read_to_string(pointee.join("SKILL.md")).unwrap(),
            "# shared\n"
        );
        assert_eq!(
            fs::read_to_string(pointee.join("unrelated.txt")).unwrap(),
            "keep\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_broken_symlinked_local_skill_target() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".claude/skills")).unwrap();
        let source = tmp.path().join("source-skill");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "# demo\n").unwrap();
        symlink(
            tmp.path().join("does-not-exist"),
            project.join(".claude/skills/demo"),
        )
        .unwrap();

        let entry = planned_local(ArtifactKind::Skill, "../source-skill");
        let checked = check_single(&entry, &project, &project).unwrap();
        execute(&checked).unwrap();

        let target = project.join(".claude/skills/demo");
        let target_meta = target.symlink_metadata().unwrap();
        assert!(target_meta.is_dir() && !target_meta.is_symlink());
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "# demo\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn check_allows_a_local_source_that_is_only_the_final_target_symlinks_pointee() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".claude/skills")).unwrap();
        let source = tmp.path().join("shared/demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "# demo\n").unwrap();
        // The target symlink points at the source, but execution replaces the symlink itself, so the source is never the write destination.
        symlink(&source, project.join(".claude/skills/demo")).unwrap();

        let entry = planned_local(ArtifactKind::Skill, "../shared/demo");
        let checked = check_single(&entry, &project, &project).unwrap();
        execute(&checked).unwrap();

        let target = project.join(".claude/skills/demo");
        let target_meta = target.symlink_metadata().unwrap();
        assert!(target_meta.is_dir() && !target_meta.is_symlink());
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "# demo\n"
        );
        assert!(
            source.join("SKILL.md").is_file(),
            "the source must survive materialization"
        );
    }

    // The final-symlink replace contract must not depend on the source kind: the tests below pin the same behavior already pinned for local sources onto Git and Gist sources, for every artifact kind.

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_symlinked_git_agent_target_and_keeps_the_pointee() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let checkout = tmp.path().join("checkout");
        fs::create_dir_all(&checkout).unwrap();
        fs::write(checkout.join("demo.md"), "generated agent\n").unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".claude/agents")).unwrap();
        let pointee = tmp.path().join("shared-agent.md");
        fs::write(&pointee, "must survive\n").unwrap();
        symlink(&pointee, project.join(".claude/agents/demo.md")).unwrap();

        let entry = planned(ArtifactKind::Agent, "demo.md");
        let checked = check_single(&entry, &checkout, &project).unwrap();
        execute(&checked).unwrap();

        let target = project.join(".claude/agents/demo.md");
        assert!(
            target.symlink_metadata().unwrap().is_file(),
            "the symlink must become a regular file"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "generated agent\n");
        assert_eq!(fs::read_to_string(&pointee).unwrap(), "must survive\n");
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_symlinked_git_skill_target_and_keeps_the_pointee_tree() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let checkout = tmp.path().join("checkout");
        fs::create_dir_all(checkout.join("skills/demo")).unwrap();
        fs::write(checkout.join("skills/demo/SKILL.md"), "# demo\n").unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".claude/skills")).unwrap();
        let pointee = tmp.path().join("shared/demo");
        fs::create_dir_all(&pointee).unwrap();
        fs::write(pointee.join("SKILL.md"), "# shared\n").unwrap();
        fs::write(pointee.join("unrelated.txt"), "keep\n").unwrap();
        symlink(&pointee, project.join(".claude/skills/demo")).unwrap();

        let entry = planned(ArtifactKind::Skill, "skills/demo");
        let checked = check_single(&entry, &checkout, &project).unwrap();
        execute(&checked).unwrap();

        let target = project.join(".claude/skills/demo");
        let target_meta = target.symlink_metadata().unwrap();
        assert!(
            target_meta.is_dir() && !target_meta.is_symlink(),
            "the symlink must become a real directory"
        );
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "# demo\n"
        );
        assert_eq!(
            fs::read_to_string(pointee.join("SKILL.md")).unwrap(),
            "# shared\n"
        );
        assert_eq!(
            fs::read_to_string(pointee.join("unrelated.txt")).unwrap(),
            "keep\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_symlinked_gist_agent_target_and_keeps_the_pointee() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let content_root = tmp.path().join("gist");
        fs::create_dir_all(&content_root).unwrap();
        fs::write(content_root.join("demo.md"), "generated agent\n").unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".claude/agents")).unwrap();
        let pointee = tmp.path().join("shared-agent.md");
        fs::write(&pointee, "must survive\n").unwrap();
        symlink(&pointee, project.join(".claude/agents/demo.md")).unwrap();

        let entry = planned_gist(
            ArtifactKind::Agent,
            GistArtifactSelector::File {
                path: "demo.md".to_owned(),
            },
        );
        let checked = check_single(&entry, &content_root, &project).unwrap();
        execute(&checked).unwrap();

        let target = project.join(".claude/agents/demo.md");
        assert!(
            target.symlink_metadata().unwrap().is_file(),
            "the symlink must become a regular file"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "generated agent\n");
        assert_eq!(fs::read_to_string(&pointee).unwrap(), "must survive\n");
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_symlinked_gist_skill_target_and_keeps_the_pointee_tree() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        // The exported revision root itself is the Skill artifact for a Gist source.
        let content_root = tmp.path().join("gist");
        fs::create_dir_all(&content_root).unwrap();
        fs::write(content_root.join("SKILL.md"), "# demo\n").unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(project.join(".claude/skills")).unwrap();
        let pointee = tmp.path().join("shared/demo");
        fs::create_dir_all(&pointee).unwrap();
        fs::write(pointee.join("SKILL.md"), "# shared\n").unwrap();
        fs::write(pointee.join("unrelated.txt"), "keep\n").unwrap();
        symlink(&pointee, project.join(".claude/skills/demo")).unwrap();

        let entry = planned_gist(ArtifactKind::Skill, GistArtifactSelector::Root);
        let checked = check_single(&entry, &content_root, &project).unwrap();
        execute(&checked).unwrap();

        let target = project.join(".claude/skills/demo");
        let target_meta = target.symlink_metadata().unwrap();
        assert!(
            target_meta.is_dir() && !target_meta.is_symlink(),
            "the symlink must become a real directory"
        );
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "# demo\n"
        );
        assert_eq!(
            fs::read_to_string(pointee.join("SKILL.md")).unwrap(),
            "# shared\n"
        );
        assert_eq!(
            fs::read_to_string(pointee.join("unrelated.txt")).unwrap(),
            "keep\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn execute_replaces_a_symlinked_gist_instruction_target_and_keeps_the_pointee() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let content_root = tmp.path().join("gist");
        fs::create_dir_all(&content_root).unwrap();
        fs::write(content_root.join("base.md"), "# base\n").unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("victim.md"), "must survive\n").unwrap();
        symlink(project.join("victim.md"), project.join("CLAUDE.md")).unwrap();

        let entry = planned_gist(
            ArtifactKind::Instruction,
            GistArtifactSelector::File {
                path: "base.md".to_owned(),
            },
        );
        let mut checked = check_single(&entry, &content_root, &project).unwrap();
        checked.rendered = Some("generated\n".to_owned());
        execute(&checked).unwrap();

        let target = project.join("CLAUDE.md");
        assert!(
            target.symlink_metadata().unwrap().is_file(),
            "the symlink must become a regular file"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "generated\n");
        assert_eq!(
            fs::read_to_string(project.join("victim.md")).unwrap(),
            "must survive\n"
        );
    }

    #[test]
    fn check_rejects_a_local_source_overlapping_another_entries_target() {
        let tmp = tempfile::tempdir().unwrap();
        let other_target = tmp.path().join(".claude/skills/other");
        fs::create_dir_all(&other_target).unwrap();
        fs::write(other_target.join("SKILL.md"), "# other\n").unwrap();

        // The source is valid on its own, but another entry in the same run materializes over it.
        let entry = planned_local(ArtifactKind::Skill, ".claude/skills/other");
        let diag = check(
            &entry,
            tmp.path(),
            tmp.path(),
            &[
                entry.target_rel_path.clone(),
                ".claude/skills/other".to_owned(),
            ],
        )
        .unwrap_err();

        assert_eq!(diag.code, DiagnosticCode::UnsafePath);
    }

    #[test]
    fn check_then_execute_copies_a_local_skill_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_dir = tmp.path().join("project");
        let skill = tmp.path().join("sibling/skills/demo");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::create_dir_all(skill.join("nested")).unwrap();
        fs::write(skill.join("SKILL.md"), "# demo\n").unwrap();
        fs::write(skill.join("nested/extra.txt"), "child\n").unwrap();

        let entry = planned_local(ArtifactKind::Skill, "../sibling/skills/demo");
        let checked = check_single(&entry, &manifest_dir, &manifest_dir).unwrap();
        execute(&checked).unwrap();

        assert!(manifest_dir.join(".claude/skills/demo/SKILL.md").is_file());
        assert_eq!(
            fs::read_to_string(manifest_dir.join(".claude/skills/demo/nested/extra.txt")).unwrap(),
            "child\n"
        );
    }

    #[test]
    fn execute_reports_io_failure_for_a_missing_source() {
        let tmp = tempfile::tempdir().unwrap();
        let checked = CheckedMaterialization {
            source_abs: tmp.path().join("missing-skill"),
            target_abs: tmp.path().join("project/.claude/skills/demo"),
            kind: ArtifactKind::Skill,
            rendered: None,
        };
        let diag = execute(&checked).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::Io);
    }
}
