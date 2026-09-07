//! Adds a Skill source declaration to `enozunu.kdl` without hand editing.
//!
//! The command edits only the root manifest it was given: it appends one `skill` declaration
//! (creating the `provider` / `skills` blocks when missing) and never rewrites unrelated
//! nodes, comments, or formatting. The candidate manifest is re-validated with the ordinary
//! parse rules and replaces the original atomically; every error and no-op path leaves the
//! manifest untouched.

use std::io::Write;
use std::path::{Path, PathBuf};

use kdl::{KdlDocument, KdlDocumentFormat, KdlNode};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::manifest::{self, SourceReference};

/// What `add-skill` did to the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddSkillOutcome {
    /// The declaration was appended and the manifest was replaced atomically.
    Added {
        /// The recorded `local.path`, relative to the manifest directory.
        manifest_relative_path: String,
    },
    /// The manifest already declares this Skill with the same source; nothing was written.
    AlreadyDeclared,
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

    // A URL is a plausible Skill source (issue #71 plans GitHub URL support), so it gets a
    // deliberate rejection instead of the misleading "path does not exist" a lookup would give.
    if source.starts_with("http://") || source.starts_with("https://") {
        return Err(vec![Diagnostic::new(
            DiagnosticCode::UnsupportedSourceReference,
            format!(
                "skill source `{source}` is a URL; add-skill accepts only a local Skill directory path"
            ),
        )]);
    }

    let text = std::fs::read_to_string(manifest_path).map_err(|e| {
        vec![Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to read {}: {e}", manifest_path.display()),
        )]
    })?;
    // The pre-edit manifest must satisfy the ordinary parse rules; editing around an invalid
    // manifest could entrench the very declarations validation rejects.
    let parsed = manifest::parse(&text)?;

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
            Err(vec![Diagnostic::new(
                DiagnosticCode::DuplicateSourceName,
                format!(
                    "skill `{skill_id}` is already declared with a different source; edit {} directly if you mean to replace it",
                    manifest_path.display()
                ),
            )])
        };
    }

    // The domain `Manifest` is lossy (comments, order, formatting), so the edit works on a
    // fresh KDL syntax tree of the same text, which round-trips everything it does not touch.
    let mut doc: KdlDocument = text
        .parse()
        .expect("text already parsed successfully via manifest::parse");
    insert_skill(&mut doc, skill_id, &rel_path);

    let candidate = doc.to_string();
    if let Err(mut inner) = manifest::parse(&candidate) {
        let mut diags = vec![Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "adding skill `{skill_id}` would make {} invalid; the manifest was not modified",
                manifest_path.display()
            ),
        )];
        diags.append(&mut inner);
        return Err(diags);
    }

    write_atomic(manifest_path, &candidate)?;
    Ok(AddSkillOutcome::Added {
        manifest_relative_path: rel_path,
    })
}

/// Verifies the Skill source contract before any manifest change: an existing, non-symlink
/// directory containing a regular-file `SKILL.md`, matching what `summon` will later check.
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
    if !abs.join("SKILL.md").is_file() {
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
fn insert_skill(doc: &mut KdlDocument, skill_id: &str, rel_path: &str) {
    // `manifest::parse` succeeded, so the document has exactly one root node.
    let root = &mut doc.nodes_mut()[0];

    let Some(provider) = child_mut(root, "provider") else {
        let indent = child_indent(root);
        let snippet = provider_snippet(skill_id, rel_path, &indent);
        insert_front(root, parse_snippet_node(&snippet), &indent);
        return;
    };

    let Some(skills) = child_mut(provider, "skills") else {
        let indent = child_indent(provider);
        let snippet = skills_snippet(skill_id, rel_path, &indent);
        insert_front(provider, parse_snippet_node(&snippet), &indent);
        return;
    };

    let indent = child_indent(skills);
    let snippet = skill_snippet(skill_id, rel_path, &indent);
    append_back(skills, parse_snippet_node(&snippet), &indent);
}

/// The first child node named `name`, matching which block `manifest::parse` reads.
fn child_mut<'a>(parent: &'a mut KdlNode, name: &str) -> Option<&'a mut KdlNode> {
    parent
        .children_mut()
        .as_mut()?
        .nodes_mut()
        .iter_mut()
        .find(|n| n.name().value() == name)
}

/// The indentation for children of `parent`: the parent's own indentation plus one level.
///
/// The parent's indentation is what follows the last newline of its leading trivia, so a
/// created block lines up with however the surrounding manifest happens to be indented.
fn child_indent(parent: &KdlNode) -> String {
    let leading = parent.format().map(|f| f.leading.as_str()).unwrap_or("");
    let own = leading.rsplit('\n').next().unwrap_or("");
    format!("{own}  ")
}

/// Inserts `node` as the first child of `parent`.
///
/// The node carries its own newline in `leading` and no terminator, so the previous first
/// child's leading newline keeps separating the two; when `parent` has no children yet the
/// node becomes a sole child instead.
fn insert_front(parent: &mut KdlNode, mut node: KdlNode, indent: &str) {
    if parent
        .children()
        .is_none_or(|children| children.nodes().is_empty())
    {
        append_back(parent, node, indent);
        return;
    }
    let children = parent.ensure_children();
    let fmt = node.format_mut().expect("parsed nodes carry format");
    fmt.leading = format!("\n{indent}");
    // A first child whose leading lacks a newline would otherwise end up on the inserted
    // node's line without a terminator between them, which is not valid KDL.
    let next_supplies_newline = children
        .nodes()
        .first()
        .and_then(|n| n.format())
        .is_some_and(|f| f.leading.contains('\n'));
    fmt.terminator = if next_supplies_newline {
        String::new()
    } else {
        "\n".to_owned()
    };
    children.nodes_mut().insert(0, node);
}

/// Appends `node` as the last child of `parent`, creating the children block when missing.
fn append_back(parent: &mut KdlNode, mut node: KdlNode, indent: &str) {
    // A parent declared without braces gains them here; the space keeps `name {` readable.
    if parent.children().is_none()
        && let Some(fmt) = parent.format_mut()
        && fmt.before_children.is_empty()
    {
        fmt.before_children = " ".to_owned();
    }
    let close_indent = indent.strip_suffix("  ").unwrap_or("").to_owned();
    let children = parent.ensure_children();
    let first = children.nodes().is_empty();
    let fmt = node.format_mut().expect("parsed nodes carry format");
    // The first child owns the newline that opens the block; later children follow the
    // previous sibling's newline terminator and need only their indentation.
    fmt.leading = if first {
        format!("\n{indent}")
    } else {
        indent.to_owned()
    };
    fmt.terminator = "\n".to_owned();
    if first {
        children.set_format(KdlDocumentFormat {
            leading: String::new(),
            trailing: close_indent,
        });
    }
    children.nodes_mut().push(node);
}

/// Parses a snippet holding exactly one node and returns that node with its formatting.
fn parse_snippet_node(snippet: &str) -> KdlNode {
    let doc: KdlDocument = snippet.parse().expect("generated snippet is valid KDL");
    doc.nodes()[0].clone()
}

/// Renders a value as a quoted KDL string literal.
///
/// `KdlValue`'s own rendering drops the quotes when a value is a legal bare identifier;
/// existing manifests quote every name and path, so the quotes are forced here.
fn kdl_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn skill_snippet(skill_id: &str, rel_path: &str, indent: &str) -> String {
    format!(
        "skill {id} {{\n{indent}  local {{\n{indent}    path {path}\n{indent}  }}\n{indent}}}",
        id = kdl_string(skill_id),
        path = kdl_string(rel_path),
    )
}

fn skills_snippet(skill_id: &str, rel_path: &str, indent: &str) -> String {
    let inner = format!("{indent}  ");
    format!(
        "skills {{\n{inner}{skill}\n{indent}}}",
        skill = skill_snippet(skill_id, rel_path, &inner),
    )
}

fn provider_snippet(skill_id: &str, rel_path: &str, indent: &str) -> String {
    let inner = format!("{indent}  ");
    format!(
        "provider {{\n{inner}{skills}\n{indent}}}",
        skills = skills_snippet(skill_id, rel_path, &inner),
    )
}

/// Replaces `path` with `contents` via a same-directory temporary file and rename, so a
/// failure mid-write can never leave a truncated manifest behind.
fn write_atomic(path: &Path, contents: &str) -> Result<(), Vec<Diagnostic>> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(crate::MANIFEST_FILE_NAME);
    let tmp_path = dir.join(format!(".{file_name}.tmp"));
    let io_diag = |e: std::io::Error| {
        vec![Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to write {}: {e}", path.display()),
        )]
    };
    // An exclusive create makes a concurrent add-skill fail loudly instead of both runs
    // silently staging into — and renaming — the same temporary file.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .map_err(io_diag)?;
    let result = file
        .write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            std::fs::rename(&tmp_path, path)
        });
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(io_diag(e));
    }
    Ok(())
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
            AddSkillOutcome::Added {
                manifest_relative_path: "skills/review".to_owned()
            }
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
            AddSkillOutcome::Added {
                manifest_relative_path: "../catalog/skills/review".to_owned()
            }
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
            AddSkillOutcome::Added {
                manifest_relative_path: "skills/review".to_owned()
            }
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
}
