//! Executes materialization plans against the project filesystem.
//!
//! This module owns artifact-shape checks and filesystem safety.
//! It never writes outside the project root and rejects symlinked sources instead of following them.

use std::fs;
use std::path::{Path, PathBuf};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::manifest::SourceReference;
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

/// Verifies the source artifact for `entry` without touching the target.
///
/// `source_base` is the resolved checkout directory for Git references and the manifest file's containing directory for local references.
/// `planned_target_rel_paths` lists every target path the current run will write; local sources are checked for overlap against all of them, because any of those writes could destroy an overlapping local source mid-run.
pub fn check(
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
        SourceReference::Gist { file, .. } => {
            check_gist_source(entry, file, source_base, project_root)
        }
    }
}

/// Verifies a Gist agent artifact inside its resolved checkout.
///
/// A missing file is reported as `SourcePathNotFound` and a non-file artifact as `ArtifactShape`, so a mistyped `file` is distinguished from a `file` that points at a directory.
/// Containment is enforced after canonicalization, so a symlink whose target escapes the checkout is rejected even though Git transport produced the checkout.
fn check_gist_source(
    entry: &PlannedMaterialization,
    file: &str,
    checkout_dir: &Path,
    project_root: &Path,
) -> Result<CheckedMaterialization, Diagnostic> {
    let checkout_canon = checkout_dir.canonicalize().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to resolve gist checkout directory: {e}"),
        )
    })?;

    let source_abs = checkout_canon.join(file);
    let source_canon = source_abs.canonicalize().map_err(|_| {
        Diagnostic::new(
            DiagnosticCode::SourcePathNotFound,
            format!(
                "agent `{}`: gist file `{}` does not exist in the resolved revision",
                entry.source_name, file
            ),
        )
    })?;

    // Canonicalization resolves symlinks, so this containment check also rejects a link whose target points outside the checkout.
    if !source_canon.starts_with(&checkout_canon) {
        return Err(Diagnostic::new(
            DiagnosticCode::UnsafePath,
            format!(
                "agent `{}`: gist file `{}` escapes the resolved revision",
                entry.source_name, file
            ),
        ));
    }

    // A Gist agent artifact must be a regular file; `file` pointing at a directory (the Gist root or a subdirectory) is a shape error.
    if !source_canon.is_file() {
        return Err(Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!(
                "agent `{}`: gist file `{}` is not a regular file",
                entry.source_name, file
            ),
        ));
    }

    Ok(CheckedMaterialization {
        source_abs: source_canon,
        target_abs: project_root.join(&entry.target_rel_path),
        kind: entry.kind,
    })
}

fn check_git_source(
    entry: &PlannedMaterialization,
    source_path: &str,
    checkout_dir: &Path,
    project_root: &Path,
) -> Result<CheckedMaterialization, Diagnostic> {
    let checkout_canon = checkout_dir.canonicalize().map_err(|e| {
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
    let mut own_target_abs = None;
    for target_rel_path in planned_target_rel_paths {
        let target_abs = canonicalize_target(&project_canon.join(target_rel_path))?;
        if source_canon.starts_with(&target_abs) || target_abs.starts_with(&source_canon) {
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
        if target_rel_path == &entry.target_rel_path {
            own_target_abs = Some(target_abs);
        }
    }
    let target_abs = match own_target_abs {
        Some(target) => target,
        None => canonicalize_target(&project_canon.join(&entry.target_rel_path))?,
    };

    check_artifact_shape(entry, &source_canon, source_path)?;

    Ok(CheckedMaterialization {
        source_abs: source_canon,
        target_abs,
        kind: entry.kind,
    })
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
    match entry.kind {
        ArtifactKind::Skill => {
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
            reject_symlinks(source_canon, &entry.source_name)?;
        }
        ArtifactKind::Agent => {
            if !source_canon.is_file() {
                return Err(Diagnostic::new(
                    DiagnosticCode::ArtifactShape,
                    format!(
                        "agent `{}`: source path `{}` is not a file",
                        entry.source_name, source_path
                    ),
                ));
            }
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::SourceReference;

    fn target_rel_path(kind: ArtifactKind) -> String {
        match kind {
            ArtifactKind::Skill => ".claude/skills/demo".to_owned(),
            ArtifactKind::Agent => ".claude/agents/demo.md".to_owned(),
        }
    }

    fn planned(kind: ArtifactKind, path: &str) -> PlannedMaterialization {
        PlannedMaterialization {
            source_name: "demo".to_owned(),
            kind,
            reference: SourceReference::Git {
                url: "https://example.com/repo".to_owned(),
                branch: "main".to_owned(),
                path: path.to_owned(),
            },
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
        };
        let diag = execute(&checked).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::Io);
    }
}
