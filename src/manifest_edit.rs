//! Shared machinery for format-preserving edits of `enozunu.kdl`.
//!
//! Every manifest-editing command follows the same discipline: read and parse the manifest
//! with the ordinary rules, mutate a fresh KDL syntax tree (which round-trips comments,
//! order, and formatting the edit does not touch), re-validate the candidate, and replace
//! the file atomically. This module owns that discipline plus the KDL formatting helpers,
//! so each command contributes only its own node placement and snippets.

use std::io::Write;
use std::path::Path;

use kdl::{KdlDocument, KdlDocumentFormat, KdlNode};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::manifest;

/// Reads and parses the manifest, refusing a symlinked manifest path.
///
/// The atomic replace renames over `manifest_path`, which would sever a symlink and fork the
/// configuration between the link and its old target; symlinked manifests are refused
/// outright, matching how symlinked sources are treated everywhere else. The pre-edit
/// manifest must satisfy the ordinary parse rules; editing around an invalid manifest could
/// entrench the very declarations validation rejects.
pub(crate) fn load_manifest_for_edit(
    manifest_path: &Path,
) -> Result<(String, manifest::Manifest), Vec<Diagnostic>> {
    if manifest_path
        .symlink_metadata()
        .is_ok_and(|m| m.is_symlink())
    {
        return Err(vec![Diagnostic::new(
            DiagnosticCode::UnsafePath,
            format!(
                "{} is a symlink; refusing to edit it because replacing the manifest would sever the link; run the command against the real file",
                manifest_path.display()
            ),
        )]);
    }

    let text = std::fs::read_to_string(manifest_path).map_err(|e| {
        vec![Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to read {}: {e}", manifest_path.display()),
        )]
    })?;
    let parsed = manifest::parse(&text)?;
    Ok((text, parsed))
}

/// Applies `edit` to a fresh syntax tree of `text`, re-validates, and replaces atomically.
///
/// `action` names the attempted change (for example ``adding skill `review` ``) in the
/// diagnostic emitted when the candidate fails validation and the manifest is left untouched.
pub(crate) fn commit_edit(
    manifest_path: &Path,
    text: &str,
    action: &str,
    edit: impl FnOnce(&mut KdlDocument),
) -> Result<(), Vec<Diagnostic>> {
    let mut doc: KdlDocument = text
        .parse()
        .expect("text already parsed successfully via manifest::parse");
    edit(&mut doc);

    let candidate = doc.to_string();
    if let Err(mut inner) = manifest::parse(&candidate) {
        let mut diags = vec![Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "{action} would make {} invalid; the manifest was not modified",
                manifest_path.display()
            ),
        )];
        diags.append(&mut inner);
        return Err(diags);
    }

    write_atomic(manifest_path, &candidate)
}

/// The first child node named `name`, matching which block `manifest::parse` reads.
pub(crate) fn child_mut<'a>(parent: &'a mut KdlNode, name: &str) -> Option<&'a mut KdlNode> {
    parent
        .children_mut()
        .as_mut()?
        .nodes_mut()
        .iter_mut()
        .find(|n| n.name().value() == name)
}

/// The indentation of `node` itself: what follows the last newline of its leading trivia.
pub(crate) fn own_indent(node: &KdlNode) -> String {
    let leading = node.format().map(|f| f.leading.as_str()).unwrap_or("");
    leading.rsplit('\n').next().unwrap_or("").to_owned()
}

/// The indentation for children of `parent`.
///
/// An existing first child's own indentation wins, so an inserted node lines up with its
/// siblings whatever the manifest's indent style; a block without indented children falls
/// back to the parent's own indentation plus one two-space level.
pub(crate) fn child_indent(parent: &KdlNode) -> String {
    parent
        .children()
        .and_then(|children| children.nodes().first())
        .map(own_indent)
        .filter(|indent| !indent.is_empty())
        .unwrap_or_else(|| format!("{}  ", own_indent(parent)))
}

/// Inserts `node` as the first child of `parent`.
///
/// The node carries its own newline in `leading` and no terminator, so the previous first
/// child's leading newline keeps separating the two; when `parent` has no children yet the
/// node becomes a sole child instead.
pub(crate) fn insert_front(parent: &mut KdlNode, mut node: KdlNode, indent: &str) {
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
pub(crate) fn append_back(parent: &mut KdlNode, mut node: KdlNode, indent: &str) {
    // A parent declared without braces gains them here; the space keeps `name {` readable.
    if parent.children().is_none()
        && let Some(fmt) = parent.format_mut()
        && fmt.before_children.is_empty()
    {
        fmt.before_children = " ".to_owned();
    }
    let close_indent = own_indent(parent);
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
pub(crate) fn parse_snippet_node(snippet: &str) -> KdlNode {
    let doc: KdlDocument = snippet.parse().expect("generated snippet is valid KDL");
    doc.nodes()[0].clone()
}

/// Renders a value as a quoted KDL string literal.
///
/// `KdlValue`'s own rendering drops the quotes when a value is a legal bare identifier;
/// existing manifests quote every name and path, so the quotes are forced here.
pub(crate) fn kdl_string(value: &str) -> String {
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
    // An exclusive create makes concurrent manifest edits fail loudly instead of both runs
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
