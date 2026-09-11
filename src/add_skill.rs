//! Adds a Skill source declaration to `enozunu.kdl` without hand editing.
//!
//! The command edits only the root manifest it was given: it appends one `skill` declaration
//! (creating the `provider` / `skills` blocks when missing) and never rewrites unrelated
//! nodes, comments, or formatting. The candidate manifest is re-validated with the ordinary
//! parse rules and replaces the original atomically; every error and no-op path leaves the
//! manifest untouched.

use std::path::{Path, PathBuf};

use kdl::KdlDocument;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::git::{GitRefLister, GitResolutionRequest, GitResolver};
use crate::github_url::{GitSourceSpec, parse_github_skill_url, resolve_boundary};
use crate::manifest::{self, SourceReference};
use crate::manifest_edit::{
    append_back, child_indent, child_mut, commit_edit, insert_front, kdl_string,
    load_manifest_for_edit, parse_snippet_node,
};

/// What `add-skill` did to the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddSkillOutcome {
    /// The declaration was appended and the manifest was replaced atomically.
    Added(AddedSource),
    /// The manifest already declares this Skill with the same source; nothing was written.
    AlreadyDeclared,
    /// The user declined the confirmation; nothing was written.
    Aborted,
}

/// The source reference `add-skill` recorded, reported back for CLI output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddedSource {
    Local {
        /// The recorded `local.path`, relative to the manifest directory.
        manifest_relative_path: String,
    },
    Git(GitSourceSpec),
}

impl AddedSource {
    /// Renders the recorded reference for CLI output, keeping each source kind identifiable.
    pub fn describe(&self) -> String {
        match self {
            AddedSource::Local {
                manifest_relative_path,
            } => format!("local: {manifest_relative_path}"),
            AddedSource::Git(spec) => {
                let (selector_field, selector_value) = spec.selector_field();
                format!(
                    "git: {} {selector_field} {selector_value}, path {}",
                    spec.url, spec.path
                )
            }
        }
    }
}

/// Prints the exact values a Git source confirmation is about (issue #71's confirmation
/// display) and reads one line's answer; only an explicit `y` / `yes` proceeds.
///
/// The streams are parameters so the prompt logic is testable without a terminal; the CLI
/// passes stdin and stdout.
pub fn prompt_git_source_confirmation(
    input: &mut dyn std::io::BufRead,
    output: &mut dyn std::io::Write,
    skill_id: &str,
    manifest_path: &Path,
    spec: &GitSourceSpec,
) -> Result<bool, Diagnostic> {
    let io_diag = |e: std::io::Error| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to confirm the source: {e}"),
        )
    };
    let (selector_field, selector_value) = spec.selector_field();
    write!(
        output,
        "skill-id: {skill_id}\nurl: {}\n{selector_field}: {selector_value}\npath: {}\nadd this Skill source to {}? [y/N] ",
        spec.url,
        spec.path,
        manifest_path.display()
    )
    .map_err(io_diag)?;
    output.flush().map_err(io_diag)?;
    let mut answer = String::new();
    input.read_line(&mut answer).map_err(io_diag)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Adds a local Skill source to the manifest at `manifest_path`.
///
/// `source` is a filesystem path to a Skill directory; a relative path resolves from
/// `base_dir` (the process working directory), while the manifest records the path
/// relative to the manifest's own directory, matching how `summon` resolves local sources.
pub fn run_add_skill(
    manifest_path: &Path,
    skill_id: &str,
    source: &str,
    base_dir: &Path,
) -> Result<AddSkillOutcome, Vec<Diagnostic>> {
    manifest::validate_name(skill_id, "skill").map_err(|d| vec![d])?;

    // URL sources go through `run_add_skill_from_url`; a URL reaching this flow gets a
    // deliberate rejection instead of the misleading "path does not exist" a lookup would give.
    if source.starts_with("http://") || source.starts_with("https://") {
        return Err(vec![Diagnostic::new(
            DiagnosticCode::UnsupportedSourceReference,
            format!(
                "skill source `{source}` is a URL; this flow accepts only a local Skill directory path"
            ),
        )]);
    }

    let (text, parsed) = load_manifest_for_edit(manifest_path)?;

    let source_abs = if Path::new(source).is_absolute() {
        PathBuf::from(source)
    } else {
        base_dir.join(source)
    };
    check_skill_directory(&source_abs, skill_id, source)?;

    let manifest_dir = match manifest_path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    // Both sides are canonicalized so the recorded relative path is independent of how the
    // user spelled the input (`./x`, `..` segments, symlinked working directories).
    let manifest_dir_canon = manifest_dir.canonicalize().map_err(|e| {
        vec![Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to resolve manifest directory: {e}"),
        )]
    })?;
    let source_canon = source_abs.canonicalize().map_err(|e| {
        vec![Diagnostic::new(
            DiagnosticCode::Io,
            format!("skill `{skill_id}`: failed to resolve source path `{source}`: {e}"),
        )]
    })?;
    let rel_path = relative_path(&manifest_dir_canon, &source_canon).ok_or_else(|| {
        vec![Diagnostic::new(
            DiagnosticCode::UnsupportedSourceReference,
            format!(
                "skill `{skill_id}`: source path `{source}` cannot be expressed relative to the manifest directory"
            ),
        )]
    })?;
    manifest::validate_local_source_path(&rel_path, "skill", skill_id).map_err(|d| vec![d])?;

    if let Some(existing) = parsed.provider.skills.iter().find(|d| d.name == skill_id) {
        return if same_local_source(&existing.reference, &rel_path, &manifest_dir_canon) {
            Ok(AddSkillOutcome::AlreadyDeclared)
        } else {
            Err(conflict_error(skill_id, manifest_path))
        };
    }

    commit_skill_edit(
        manifest_path,
        &text,
        skill_id,
        &NewReference::Local { path: &rel_path },
    )?;
    Ok(AddSkillOutcome::Added(AddedSource::Local {
        manifest_relative_path: rel_path,
    }))
}

/// Adds a Git Skill source resolved from a GitHub tree/blob URL.
///
/// The URL's ref/path boundary is resolved against the remote's advertised refs, the resolved
/// source is checked against the Skill source contract, and `confirm` is asked with the exact
/// values to record before the manifest changes. Declining leaves the manifest untouched, as
/// does every error path. The lock file is neither read nor written: branch and tag lock
/// semantics stay entirely with `summon`.
pub fn run_add_skill_from_url(
    manifest_path: &Path,
    skill_id: &str,
    url: &str,
    ref_lister: &dyn GitRefLister,
    resolver: &dyn GitResolver,
    confirm: &mut dyn FnMut(&GitSourceSpec) -> Result<bool, Diagnostic>,
) -> Result<AddSkillOutcome, Vec<Diagnostic>> {
    manifest::validate_name(skill_id, "skill").map_err(|d| vec![d])?;
    let (text, parsed) = load_manifest_for_edit(manifest_path)?;

    let parsed_url = parse_github_skill_url(url).map_err(|d| vec![d])?;

    // The existing-declaration decision comes before any remote access: a remainder that
    // textually spells out the declared selector and path proves a no-op offline, a different
    // repository (or a non-Git source) is a conflict offline, and only a same-repository URL
    // whose remainder does not match needs the remote's refs to settle no-op versus conflict
    // (a listing only — the repository is never fetched on either outcome).
    if let Some(existing) = parsed.provider.skills.iter().find(|d| d.name == skill_id) {
        return match &existing.reference {
            SourceReference::Git {
                url: existing_url,
                selector,
                path,
            } if *existing_url == parsed_url.repo_url => {
                if remainder_matches(selector, path, &parsed_url.ref_and_path) {
                    Ok(AddSkillOutcome::AlreadyDeclared)
                } else {
                    let refs = ref_lister
                        .list_refs(&parsed_url.repo_url)
                        .map_err(|e| vec![crate::git_error_diagnostic(e)])?;
                    let spec = resolve_boundary(&parsed_url, &refs).map_err(|d| vec![d])?;
                    let same = existing.reference
                        == SourceReference::Git {
                            url: spec.url.clone(),
                            selector: spec.selector.clone(),
                            path: spec.path.clone(),
                        };
                    if same {
                        Ok(AddSkillOutcome::AlreadyDeclared)
                    } else {
                        Err(conflict_error(skill_id, manifest_path))
                    }
                }
            }
            _ => Err(conflict_error(skill_id, manifest_path)),
        };
    }

    let refs = ref_lister
        .list_refs(&parsed_url.repo_url)
        .map_err(|e| vec![crate::git_error_diagnostic(e)])?;
    let spec = resolve_boundary(&parsed_url, &refs).map_err(|d| vec![d])?;
    manifest::validate_source_path(&spec.path, "skill", skill_id).map_err(|d| vec![d])?;

    // The resolved commit's content must satisfy the Skill source contract before the user is
    // even asked; a confirmation for a source summon would reject helps nobody.
    let resolved = resolver
        .resolve(&GitResolutionRequest {
            url: spec.url.clone(),
            selector: spec.selector.clone(),
        })
        .map_err(|e| vec![crate::git_error_diagnostic(e)])?;
    check_skill_directory(
        &resolved.content_root.join(&spec.path),
        skill_id,
        &spec.path,
    )?;

    if !confirm(&spec).map_err(|d| vec![d])? {
        return Ok(AddSkillOutcome::Aborted);
    }

    commit_skill_edit(
        manifest_path,
        &text,
        skill_id,
        &NewReference::Git { spec: &spec },
    )?;
    Ok(AddSkillOutcome::Added(AddedSource::Git(spec)))
}

/// Whether the URL remainder spells out exactly the declared selector followed by the
/// declared path (a root path `.` meaning the selector consumes the whole remainder).
///
/// A textual match ignores the declared selector's kind, so it cannot see remote-side drift:
/// it accepts a URL the remote would read as an additional interpretation (a tag shadowing
/// the declared branch), and even one whose sole current reading has a different kind than
/// the declaration (a declared tag whose name is now a branch), where the online path would
/// report a conflict instead. That imprecision is deliberate: a match only ever produces a
/// no-op against an unchanged declaration, so resolving it could not change the manifest —
/// at worst the "same source" report papers over ref churn on the remote.
fn remainder_matches(selector: &crate::git::GitSelector, path: &str, remainder: &[String]) -> bool {
    let selector_value = match selector {
        crate::git::GitSelector::Branch(branch) => branch.as_str(),
        crate::git::GitSelector::Tag(tag) => tag.as_str(),
        crate::git::GitSelector::Revision(sha) => sha.as_str(),
    };
    let expected = if path == "." {
        selector_value.to_owned()
    } else {
        format!("{selector_value}/{path}")
    };
    remainder.join("/") == expected
}

fn conflict_error(skill_id: &str, manifest_path: &Path) -> Vec<Diagnostic> {
    vec![Diagnostic::new(
        DiagnosticCode::DuplicateSourceName,
        format!(
            "skill `{skill_id}` is already declared with a different source; edit {} directly if you mean to replace it",
            manifest_path.display()
        ),
    )]
}

/// Inserts the declaration into a fresh syntax tree, re-validates, and replaces atomically.
fn commit_skill_edit(
    manifest_path: &Path,
    text: &str,
    skill_id: &str,
    reference: &NewReference<'_>,
) -> Result<(), Vec<Diagnostic>> {
    commit_edit(
        manifest_path,
        text,
        &format!("adding skill `{skill_id}`"),
        |doc| insert_skill(doc, skill_id, reference),
    )
}

/// The source reference block a new `skill` declaration will carry.
enum NewReference<'a> {
    Local { path: &'a str },
    Git { spec: &'a GitSourceSpec },
}

/// Verifies the Skill source contract before any manifest change: an existing, non-symlink
/// directory containing a regular-file `SKILL.md` and no symlinks anywhere in its tree,
/// matching what `summon` will later check.
fn check_skill_directory(abs: &Path, skill_id: &str, input: &str) -> Result<(), Vec<Diagnostic>> {
    let metadata = abs.symlink_metadata().map_err(|_| {
        vec![Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!("skill `{skill_id}`: source path `{input}` does not exist"),
        )]
    })?;
    // Local sources have no checkout boundary containing symlinks, so a symlink at the source
    // path is rejected here exactly as materialization would reject it later.
    if metadata.is_symlink() {
        return Err(vec![Diagnostic::new(
            DiagnosticCode::UnsafePath,
            format!(
                "skill `{skill_id}`: source path `{input}` is a symlink; symlinked sources are not materialized"
            ),
        )]);
    }
    if !metadata.is_dir() {
        return Err(vec![Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!(
                "skill `{skill_id}`: source path `{input}` is not a directory; a Skill source must be a directory containing SKILL.md"
            ),
        )]);
    }
    // The same tree walk summon runs later, so a symlink anywhere in the source — including a
    // symlinked SKILL.md — is rejected now instead of surfacing at materialization time.
    crate::materialize::reject_symlinks(abs, skill_id).map_err(|d| vec![d])?;
    // `symlink_metadata`, not `is_file`: the walk above already rejected a symlinked SKILL.md,
    // and this check must not follow links when judging what the directory itself contains.
    if !abs
        .join("SKILL.md")
        .symlink_metadata()
        .is_ok_and(|m| m.is_file())
    {
        return Err(vec![Diagnostic::new(
            DiagnosticCode::ArtifactShape,
            format!("skill `{skill_id}`: source path `{input}` does not contain SKILL.md"),
        )]);
    }
    Ok(())
}

/// Whether the declared reference and the resolved input denote the same local Skill directory.
///
/// A string match catches the common case; the canonical comparison additionally recognizes a
/// differently spelled path to the same directory, so a re-run is a no-op instead of a conflict.
fn same_local_source(
    existing: &SourceReference,
    rel_path: &str,
    manifest_dir_canon: &Path,
) -> bool {
    match existing {
        SourceReference::Local { path } => {
            path == rel_path
                || matches!(
                    (
                        manifest_dir_canon.join(path).canonicalize(),
                        manifest_dir_canon.join(rel_path).canonicalize(),
                    ),
                    (Ok(a), Ok(b)) if a == b
                )
        }
        _ => false,
    }
}

/// Expresses canonical path `to` relative to canonical directory `from`, `/`-separated.
///
/// Returns `None` when a component is not valid UTF-8 or the paths share no common root
/// (Windows drives); the manifest stores paths as UTF-8 strings with `/` separators.
fn relative_path(from: &Path, to: &Path) -> Option<String> {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from
        .iter()
        .zip(to.iter())
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    for component in &to[common..] {
        parts.push(component.as_os_str().to_str()?.to_owned());
    }
    if parts.is_empty() {
        parts.push(".".to_owned());
    }
    Some(parts.join("/"))
}

/// Inserts the `skill` declaration, creating the `provider` / `skills` blocks when missing.
///
/// A created block is placed first among its siblings, matching the conventional manifest
/// order (`provider` before `consumer`, `skills` first under `provider`); the skill itself
/// appends to the end of an existing `skills` block.
fn insert_skill(doc: &mut KdlDocument, skill_id: &str, reference: &NewReference<'_>) {
    // `manifest::parse` succeeded, so the document has exactly one root node.
    let root = &mut doc.nodes_mut()[0];

    let Some(provider) = child_mut(root, "provider") else {
        let indent = child_indent(root);
        let snippet = provider_snippet(skill_id, reference, &indent);
        insert_front(root, parse_snippet_node(&snippet), &indent);
        return;
    };

    let Some(skills) = child_mut(provider, "skills") else {
        let indent = child_indent(provider);
        let snippet = skills_snippet(skill_id, reference, &indent);
        insert_front(provider, parse_snippet_node(&snippet), &indent);
        return;
    };

    let indent = child_indent(skills);
    let snippet = skill_snippet(skill_id, reference, &indent);
    append_back(skills, parse_snippet_node(&snippet), &indent);
}

fn skill_snippet(skill_id: &str, reference: &NewReference<'_>, indent: &str) -> String {
    let (block, fields): (&str, Vec<(&str, &str)>) = match reference {
        NewReference::Local { path } => ("local", vec![("path", path)]),
        NewReference::Git { spec } => {
            let (selector_field, selector_value) = spec.selector_field();
            (
                "git",
                vec![
                    ("url", spec.url.as_str()),
                    (selector_field, selector_value),
                    ("path", spec.path.as_str()),
                ],
            )
        }
    };
    let mut snippet = format!(
        "skill {id} {{\n{indent}  {block} {{\n",
        id = kdl_string(skill_id),
    );
    for (field, value) in fields {
        snippet.push_str(&format!("{indent}    {field} {}\n", kdl_string(value)));
    }
    snippet.push_str(&format!("{indent}  }}\n{indent}}}"));
    snippet
}

fn skills_snippet(skill_id: &str, reference: &NewReference<'_>, indent: &str) -> String {
    let inner = format!("{indent}  ");
    format!(
        "skills {{\n{inner}{skill}\n{indent}}}",
        skill = skill_snippet(skill_id, reference, &inner),
    )
}

fn provider_snippet(skill_id: &str, reference: &NewReference<'_>, indent: &str) -> String {
    let inner = format!("{indent}  ");
    format!(
        "provider {{\n{inner}{skills}\n{indent}}}",
        skills = skills_snippet(skill_id, reference, &inner),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_skill_dir(root: &Path, rel: &str) -> PathBuf {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# skill\n").unwrap();
        dir
    }

    const BASE_MANIFEST: &str = r#"// Project manifest.
enozunu config-version=1 {
  provider {
    skills {
      // An existing skill with a comment.
      skill "existing" {
        local {
          path "skills/existing"
        }
      }
    }
  }
  consumer {
    claude {
      use-skills "existing"
    }
  }
}
"#;

    #[test]
    fn appends_skill_to_existing_skills_block_preserving_unrelated_text() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();
        write_skill_dir(tmp.path(), "skills/existing");
        write_skill_dir(tmp.path(), "skills/review");

        let outcome = run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap();

        assert_eq!(
            outcome,
            AddSkillOutcome::Added(AddedSource::Local {
                manifest_relative_path: "skills/review".to_owned()
            })
        );
        let written = fs::read_to_string(&manifest_path).unwrap();
        let expected = r#"// Project manifest.
enozunu config-version=1 {
  provider {
    skills {
      // An existing skill with a comment.
      skill "existing" {
        local {
          path "skills/existing"
        }
      }
      skill "review" {
        local {
          path "skills/review"
        }
      }
    }
  }
  consumer {
    claude {
      use-skills "existing"
    }
  }
}
"#;
        assert_eq!(written, expected);
    }

    #[test]
    fn creates_skills_block_when_provider_lacks_one() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(
            &manifest_path,
            "enozunu config-version=1 {\n  provider {\n    agents {\n      agent \"a\" {\n        local {\n          path \"agents/a.md\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n      use-agents \"a\"\n    }\n  }\n}\n",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("agents")).unwrap();
        fs::write(tmp.path().join("agents/a.md"), "").unwrap();
        write_skill_dir(tmp.path(), "skills/review");

        run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap();

        let written = fs::read_to_string(&manifest_path).unwrap();
        let expected = "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n    }\n    agents {\n      agent \"a\" {\n        local {\n          path \"agents/a.md\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n      use-agents \"a\"\n    }\n  }\n}\n";
        assert_eq!(written, expected);
    }

    #[test]
    fn creates_provider_block_before_consumer_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(
            &manifest_path,
            "enozunu config-version=1 {\n  consumer {\n    claude {\n    }\n  }\n}\n",
        )
        .unwrap();
        write_skill_dir(tmp.path(), "skills/review");

        run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap();

        let written = fs::read_to_string(&manifest_path).unwrap();
        let expected = "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n    }\n  }\n}\n";
        assert_eq!(written, expected);
    }

    #[test]
    fn resolves_relative_input_from_base_dir_and_records_manifest_relative_path() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let manifest_path = project.join("enozunu.kdl");
        fs::write(
            &manifest_path,
            "enozunu config-version=1 {\n  consumer {\n    claude {\n    }\n  }\n}\n",
        )
        .unwrap();
        write_skill_dir(tmp.path(), "catalog/skills/review");

        // The input is relative to the working directory (`tmp`), one level above the project.
        let outcome = run_add_skill(
            &manifest_path,
            "review",
            "catalog/skills/review",
            tmp.path(),
        )
        .unwrap();

        assert_eq!(
            outcome,
            AddSkillOutcome::Added(AddedSource::Local {
                manifest_relative_path: "../catalog/skills/review".to_owned()
            })
        );
        let written = fs::read_to_string(&manifest_path).unwrap();
        assert!(written.contains("path \"../catalog/skills/review\""));
    }

    #[test]
    fn resolves_absolute_input_to_manifest_relative_path() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(
            &manifest_path,
            "enozunu config-version=1 {\n  consumer {\n    claude {\n    }\n  }\n}\n",
        )
        .unwrap();
        let skill_dir = write_skill_dir(tmp.path(), "skills/review");

        let outcome = run_add_skill(
            &manifest_path,
            "review",
            skill_dir.to_str().unwrap(),
            // A base dir elsewhere shows the absolute input ignores it.
            Path::new("/"),
        )
        .unwrap();

        assert_eq!(
            outcome,
            AddSkillOutcome::Added(AddedSource::Local {
                manifest_relative_path: "skills/review".to_owned()
            })
        );
    }

    #[test]
    fn same_declared_source_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();
        write_skill_dir(tmp.path(), "skills/existing");

        let outcome =
            run_add_skill(&manifest_path, "existing", "skills/existing", tmp.path()).unwrap();

        assert_eq!(outcome, AddSkillOutcome::AlreadyDeclared);
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), BASE_MANIFEST);
    }

    #[test]
    fn different_declared_source_is_a_conflict_and_leaves_the_manifest_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();
        write_skill_dir(tmp.path(), "skills/existing");
        write_skill_dir(tmp.path(), "elsewhere/existing");

        let diags = run_add_skill(&manifest_path, "existing", "elsewhere/existing", tmp.path())
            .unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::DuplicateSourceName);
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), BASE_MANIFEST);
    }

    #[test]
    fn existing_git_source_under_the_same_id_is_a_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        let manifest = "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        git {\n          url \"https://github.com/example/repo\"\n          branch \"main\"\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n    }\n  }\n}\n";
        fs::write(&manifest_path, manifest).unwrap();
        write_skill_dir(tmp.path(), "skills/review");

        let diags =
            run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::DuplicateSourceName);
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), manifest);
    }

    #[test]
    fn appended_skill_follows_the_siblings_indent_style() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        // A four-space-indented manifest: the appended sibling must line up with the
        // existing one instead of assuming the two-space house style.
        let manifest = "enozunu config-version=1 {\n    provider {\n        skills {\n            skill \"existing\" {\n                local {\n                    path \"skills/existing\"\n                }\n            }\n        }\n    }\n    consumer {\n        claude {\n        }\n    }\n}\n";
        fs::write(&manifest_path, manifest).unwrap();
        write_skill_dir(tmp.path(), "skills/existing");
        write_skill_dir(tmp.path(), "skills/review");

        run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap();

        let written = fs::read_to_string(&manifest_path).unwrap();
        assert!(
            written.contains("\n            skill \"review\" {\n"),
            "appended skill should sit at the sibling's twelve-space indent:\n{written}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn symlinked_manifest_path_is_rejected_without_a_write() {
        let tmp = tempfile::tempdir().unwrap();
        let real_path = tmp.path().join("real.kdl");
        fs::write(&real_path, BASE_MANIFEST).unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        std::os::unix::fs::symlink(&real_path, &manifest_path).unwrap();
        write_skill_dir(tmp.path(), "skills/review");

        let diags =
            run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::UnsafePath);
        assert!(manifest_path.symlink_metadata().unwrap().is_symlink());
        assert_eq!(fs::read_to_string(&real_path).unwrap(), BASE_MANIFEST);
    }

    #[test]
    fn missing_skill_md_is_rejected_before_any_manifest_change() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();
        fs::create_dir_all(tmp.path().join("skills/review")).unwrap();

        let diags =
            run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::ArtifactShape);
        assert!(diags[0].message.contains("does not contain SKILL.md"));
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), BASE_MANIFEST);
    }

    #[test]
    fn missing_source_path_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();

        let diags =
            run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::ArtifactShape);
        assert!(diags[0].message.contains("does not exist"));
    }

    #[test]
    #[cfg(unix)]
    fn symlinked_source_path_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();
        write_skill_dir(tmp.path(), "real/review");
        std::os::unix::fs::symlink(tmp.path().join("real/review"), tmp.path().join("review"))
            .unwrap();

        let diags = run_add_skill(&manifest_path, "review", "review", tmp.path()).unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::UnsafePath);
    }

    #[test]
    #[cfg(unix)]
    fn symlink_inside_the_source_directory_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();
        // The directory itself is real, but SKILL.md is a symlink to a real file —
        // exactly what summon's tree walk would reject at materialization time.
        fs::write(tmp.path().join("real-skill.md"), "# skill\n").unwrap();
        fs::create_dir_all(tmp.path().join("skills/review")).unwrap();
        std::os::unix::fs::symlink(
            tmp.path().join("real-skill.md"),
            tmp.path().join("skills/review/SKILL.md"),
        )
        .unwrap();

        let diags =
            run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::UnsafePath);
        assert!(diags[0].message.contains("contains a symlink"));
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), BASE_MANIFEST);
    }

    #[test]
    fn url_source_is_rejected_with_a_dedicated_message() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();

        let diags = run_add_skill(
            &manifest_path,
            "review",
            "https://github.com/example/repo/tree/main/skills/review",
            tmp.path(),
        )
        .unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::UnsupportedSourceReference);
        assert!(diags[0].message.contains("is a URL"));
    }

    #[test]
    fn invalid_skill_id_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();

        let diags = run_add_skill(&manifest_path, "bad/name", "skills/x", tmp.path()).unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::InvalidName);
    }

    #[test]
    fn missing_manifest_is_an_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");

        let diags = run_add_skill(&manifest_path, "review", "skills/x", tmp.path()).unwrap_err();

        assert_eq!(diags[0].code, DiagnosticCode::Io);
    }

    #[test]
    fn invalid_manifest_is_rejected_without_a_write() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, "enozunu config-version=1 {\n}\n").unwrap();
        write_skill_dir(tmp.path(), "skills/review");

        let diags =
            run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap_err();

        // The missing `consumer` block fails the pre-edit parse.
        assert_eq!(diags[0].code, DiagnosticCode::ManifestShape);
        assert_eq!(
            fs::read_to_string(&manifest_path).unwrap(),
            "enozunu config-version=1 {\n}\n"
        );
    }

    #[test]
    fn no_temporary_file_remains_after_a_successful_write() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest_path = tmp.path().join("enozunu.kdl");
        fs::write(&manifest_path, BASE_MANIFEST).unwrap();
        write_skill_dir(tmp.path(), "skills/review");

        run_add_skill(&manifest_path, "review", "skills/review", tmp.path()).unwrap();

        assert!(!tmp.path().join(".enozunu.kdl.tmp").exists());
    }

    #[test]
    fn relative_path_walks_up_and_down_from_the_manifest_directory() {
        assert_eq!(
            relative_path(Path::new("/a/b/c"), Path::new("/a/x/y")),
            Some("../../x/y".to_owned())
        );
        assert_eq!(
            relative_path(Path::new("/a"), Path::new("/a/b")),
            Some("b".to_owned())
        );
        assert_eq!(
            relative_path(Path::new("/a"), Path::new("/a")),
            Some(".".to_owned())
        );
    }

    mod url_flow {
        use super::*;
        use crate::git::{GitError, GitSelector, RemoteRefs, ResolvedSource};
        use std::cell::Cell;

        const URL: &str = "https://github.com/example/repo/tree/main/skills/review";
        const MINIMAL_MANIFEST: &str =
            "enozunu config-version=1 {\n  consumer {\n    claude {\n    }\n  }\n}\n";

        struct FakeRefLister(RemoteRefs);
        impl GitRefLister for FakeRefLister {
            fn list_refs(&self, _url: &str) -> Result<RemoteRefs, GitError> {
                Ok(self.0.clone())
            }
        }

        struct FailingRefLister;
        impl GitRefLister for FailingRefLister {
            fn list_refs(&self, url: &str) -> Result<RemoteRefs, GitError> {
                Err(GitError::Fetch(format!("cannot reach `{url}`")))
            }
        }

        struct FakeResolver {
            content_root: PathBuf,
            resolved: Cell<bool>,
        }
        impl FakeResolver {
            fn new(content_root: PathBuf) -> Self {
                Self {
                    content_root,
                    resolved: Cell::new(false),
                }
            }
        }
        impl GitResolver for FakeResolver {
            fn resolve(&self, _request: &GitResolutionRequest) -> Result<ResolvedSource, GitError> {
                self.resolved.set(true);
                Ok(ResolvedSource {
                    commit: "468aac8caed5f0c3b859b8286968e2c78e2b8760".to_owned(),
                    content_root: self.content_root.clone(),
                })
            }
        }

        fn main_refs() -> RemoteRefs {
            RemoteRefs {
                branches: vec!["main".to_owned()],
                tags: Vec::new(),
                head_branch: Some("main".to_owned()),
            }
        }

        /// A manifest plus a fake resolved content tree holding `skills/review/SKILL.md`.
        fn setup() -> (tempfile::TempDir, PathBuf, FakeResolver) {
            let tmp = tempfile::tempdir().unwrap();
            let manifest_path = tmp.path().join("enozunu.kdl");
            fs::write(&manifest_path, MINIMAL_MANIFEST).unwrap();
            let content_root = tmp.path().join("resolved");
            fs::create_dir_all(content_root.join("skills/review")).unwrap();
            fs::write(content_root.join("skills/review/SKILL.md"), "# skill\n").unwrap();
            let resolver = FakeResolver::new(content_root);
            (tmp, manifest_path, resolver)
        }

        #[test]
        fn records_a_git_source_after_confirmation() {
            let (_tmp, manifest_path, resolver) = setup();
            let mut confirmed_with = None;
            let mut confirm = |spec: &GitSourceSpec| {
                confirmed_with = Some(spec.clone());
                Ok(true)
            };

            let outcome = run_add_skill_from_url(
                &manifest_path,
                "review",
                URL,
                &FakeRefLister(main_refs()),
                &resolver,
                &mut confirm,
            )
            .unwrap();

            let expected_spec = GitSourceSpec {
                url: "https://github.com/example/repo".to_owned(),
                selector: GitSelector::Branch("main".to_owned()),
                path: "skills/review".to_owned(),
            };
            assert_eq!(
                outcome,
                AddSkillOutcome::Added(AddedSource::Git(expected_spec.clone()))
            );
            assert_eq!(confirmed_with, Some(expected_spec));
            let written = fs::read_to_string(&manifest_path).unwrap();
            let expected = "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        git {\n          url \"https://github.com/example/repo\"\n          branch \"main\"\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n    }\n  }\n}\n";
            assert_eq!(written, expected);
        }

        #[test]
        fn declined_confirmation_leaves_the_manifest_unchanged() {
            let (_tmp, manifest_path, resolver) = setup();

            let outcome = run_add_skill_from_url(
                &manifest_path,
                "review",
                URL,
                &FakeRefLister(main_refs()),
                &resolver,
                &mut |_| Ok(false),
            )
            .unwrap();

            assert_eq!(outcome, AddSkillOutcome::Aborted);
            assert_eq!(
                fs::read_to_string(&manifest_path).unwrap(),
                MINIMAL_MANIFEST
            );
        }

        #[test]
        fn a_resolved_source_without_skill_md_is_rejected_before_confirmation() {
            let (_tmp, manifest_path, resolver) = setup();
            fs::remove_file(resolver.content_root.join("skills/review/SKILL.md")).unwrap();
            let mut confirm_called = false;

            let diags = run_add_skill_from_url(
                &manifest_path,
                "review",
                URL,
                &FakeRefLister(main_refs()),
                &resolver,
                &mut |_| {
                    confirm_called = true;
                    Ok(true)
                },
            )
            .unwrap_err();

            assert_eq!(diags[0].code, DiagnosticCode::ArtifactShape);
            assert!(
                !confirm_called,
                "confirmation must not run for an invalid source"
            );
            assert_eq!(
                fs::read_to_string(&manifest_path).unwrap(),
                MINIMAL_MANIFEST
            );
        }

        #[test]
        fn the_same_declared_git_source_is_a_no_op_without_any_remote_access() {
            let (_tmp, manifest_path, resolver) = setup();
            fs::write(
                &manifest_path,
                "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        git {\n          url \"https://github.com/example/repo\"\n          branch \"main\"\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n    }\n  }\n}\n",
            )
            .unwrap();

            // A failing lister proves the textual remainder match settles the no-op offline.
            let outcome = run_add_skill_from_url(
                &manifest_path,
                "review",
                URL,
                &FailingRefLister,
                &resolver,
                &mut |_| Ok(true),
            )
            .unwrap();

            assert_eq!(outcome, AddSkillOutcome::AlreadyDeclared);
            assert!(
                !resolver.resolved.get(),
                "a no-op must not fetch the repository"
            );
        }

        #[test]
        fn a_bare_url_matching_the_declared_default_branch_is_a_no_op_after_listing_refs() {
            let (_tmp, manifest_path, resolver) = setup();
            fs::write(
                &manifest_path,
                "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        git {\n          url \"https://github.com/example/repo\"\n          branch \"main\"\n          path \".\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n    }\n  }\n}\n",
            )
            .unwrap();

            // An empty remainder cannot match textually, so the refs listing decides.
            let outcome = run_add_skill_from_url(
                &manifest_path,
                "review",
                "https://github.com/example/repo",
                &FakeRefLister(main_refs()),
                &resolver,
                &mut |_| Ok(true),
            )
            .unwrap();

            assert_eq!(outcome, AddSkillOutcome::AlreadyDeclared);
            assert!(
                !resolver.resolved.get(),
                "settling no-op versus conflict must not fetch the repository"
            );
        }

        #[test]
        fn a_different_declared_source_is_a_conflict_without_any_remote_access() {
            let (_tmp, manifest_path, resolver) = setup();
            fs::write(
                &manifest_path,
                "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        local {\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n    }\n  }\n}\n",
            )
            .unwrap();

            // A non-Git declaration conflicts offline: a failing lister proves it.
            let diags = run_add_skill_from_url(
                &manifest_path,
                "review",
                URL,
                &FailingRefLister,
                &resolver,
                &mut |_| Ok(true),
            )
            .unwrap_err();

            assert_eq!(diags[0].code, DiagnosticCode::DuplicateSourceName);
        }

        #[test]
        fn a_same_repository_url_naming_another_ref_is_a_conflict_after_listing_refs() {
            let (_tmp, manifest_path, resolver) = setup();
            fs::write(
                &manifest_path,
                "enozunu config-version=1 {\n  provider {\n    skills {\n      skill \"review\" {\n        git {\n          url \"https://github.com/example/repo\"\n          branch \"develop\"\n          path \"skills/review\"\n        }\n      }\n    }\n  }\n  consumer {\n    claude {\n    }\n  }\n}\n",
            )
            .unwrap();

            let diags = run_add_skill_from_url(
                &manifest_path,
                "review",
                URL,
                &FakeRefLister(main_refs()),
                &resolver,
                &mut |_| Ok(true),
            )
            .unwrap_err();

            assert_eq!(diags[0].code, DiagnosticCode::DuplicateSourceName);
            assert!(
                !resolver.resolved.get(),
                "settling no-op versus conflict must not fetch the repository"
            );
        }

        #[test]
        fn records_a_repository_root_source_from_a_bare_url() {
            let tmp = tempfile::tempdir().unwrap();
            let manifest_path = tmp.path().join("enozunu.kdl");
            fs::write(&manifest_path, MINIMAL_MANIFEST).unwrap();
            // The resolved repository root is itself the Skill directory.
            let content_root = tmp.path().join("resolved");
            fs::create_dir_all(&content_root).unwrap();
            fs::write(content_root.join("SKILL.md"), "# skill\n").unwrap();
            let resolver = FakeResolver::new(content_root);

            let outcome = run_add_skill_from_url(
                &manifest_path,
                "review",
                "https://github.com/example/repo",
                &FakeRefLister(main_refs()),
                &resolver,
                &mut |_| Ok(true),
            )
            .unwrap();

            assert_eq!(
                outcome,
                AddSkillOutcome::Added(AddedSource::Git(GitSourceSpec {
                    url: "https://github.com/example/repo".to_owned(),
                    selector: GitSelector::Branch("main".to_owned()),
                    path: ".".to_owned(),
                }))
            );
            let written = fs::read_to_string(&manifest_path).unwrap();
            assert!(written.contains("path \".\""), "{written}");
        }

        #[test]
        fn describe_keeps_each_added_source_kind_identifiable() {
            assert_eq!(
                AddedSource::Local {
                    manifest_relative_path: "../catalog/skills/review".to_owned()
                }
                .describe(),
                "local: ../catalog/skills/review"
            );
            assert_eq!(
                AddedSource::Git(GitSourceSpec {
                    url: "https://github.com/example/repo".to_owned(),
                    selector: GitSelector::Branch("main".to_owned()),
                    path: "skills/review".to_owned(),
                })
                .describe(),
                "git: https://github.com/example/repo branch main, path skills/review"
            );
        }

        #[test]
        fn confirmation_prompt_shows_the_recorded_values_and_reads_the_answer() {
            let spec = GitSourceSpec {
                url: "https://github.com/example/repo".to_owned(),
                selector: GitSelector::Branch("main".to_owned()),
                path: "skills/review".to_owned(),
            };
            let cases = [
                ("y\n", true),
                ("Y\n", true),
                ("yes\n", true),
                ("YES\n", true),
                ("n\n", false),
                ("\n", false),
                // EOF without an answer declines rather than proceeding.
                ("", false),
                ("no\n", false),
                ("yep\n", false),
            ];
            for (answer, expected) in cases {
                let mut input = std::io::Cursor::new(answer.as_bytes());
                let mut output = Vec::new();
                let accepted = prompt_git_source_confirmation(
                    &mut input,
                    &mut output,
                    "review",
                    Path::new("enozunu.kdl"),
                    &spec,
                )
                .unwrap();
                assert_eq!(accepted, expected, "answer `{answer}`");
                let shown = String::from_utf8(output).unwrap();
                assert_eq!(
                    shown,
                    "skill-id: review\nurl: https://github.com/example/repo\nbranch: main\npath: skills/review\nadd this Skill source to enozunu.kdl? [y/N] "
                );
            }
        }

        #[test]
        fn an_unreachable_remote_is_a_git_resolution_error() {
            let (_tmp, manifest_path, resolver) = setup();

            let diags = run_add_skill_from_url(
                &manifest_path,
                "review",
                URL,
                &FailingRefLister,
                &resolver,
                &mut |_| Ok(true),
            )
            .unwrap_err();

            assert_eq!(diags[0].code, DiagnosticCode::GitResolution);
            assert_eq!(
                fs::read_to_string(&manifest_path).unwrap(),
                MINIMAL_MANIFEST
            );
        }
    }
}
