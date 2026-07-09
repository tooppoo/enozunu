//! Parses and validates `enozunu.kdl` into the domain model.
//!
//! This module owns syntax-level and schema-level validation of the human-authored manifest.
//! It does not resolve Git sources or inspect the filesystem; those checks belong to the resolution and materialization layers.

use kdl::{KdlDocument, KdlNode};

use crate::diagnostics::{Diagnostic, DiagnosticCode};

pub const SUPPORTED_CONFIG_VERSION: i128 = 1;

/// A manifest that passed schema validation.
///
/// Reference existence (`use-skills` / `use-agents`) is already checked, so consumers of this type can index provider sources by name without failure handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub provider: Provider,
    pub consumer: Consumer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub skills: Vec<SourceDecl>,
    pub agents: Vec<SourceDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDecl {
    pub name: String,
    pub reference: SourceReference,
}

/// A tagged source reference: exactly one `git` or `local` block per source declaration.
///
/// v0.0.x accepts only these two kinds; shorthand forms are rejected during validation.
/// See docs/manifest.md for the supported source reference policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceReference {
    Git {
        url: String,
        branch: String,
        path: String,
    },
    Local {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumer {
    pub claude: ClaudeConsumer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeConsumer {
    pub use_skills: Vec<String>,
    pub use_agents: Vec<String>,
}

/// Parses and validates manifest text.
///
/// Validation collects evidence rather than failing fast: a single run reports as many user-fixable errors as possible.
pub fn parse(text: &str) -> Result<Manifest, Vec<Diagnostic>> {
    let doc: KdlDocument = text.parse().map_err(|e: kdl::KdlError| {
        vec![Diagnostic::new(
            DiagnosticCode::ManifestSyntax,
            format!("manifest is not valid KDL: {e}"),
        )]
    })?;

    let mut diags = Vec::new();

    let root = match single_node(&doc, "enozunu") {
        Ok(node) => node,
        Err(d) => return Err(vec![d]),
    };

    check_config_version(root, &mut diags);

    let children = root.children();
    let provider = children
        .and_then(|c| c.get("provider"))
        .map(|n| parse_provider(n, &mut diags));
    let consumer = children
        .and_then(|c| c.get("consumer"))
        .map(|n| parse_consumer(n, &mut diags));

    if let Some(children) = children {
        for node in children.nodes() {
            let name = node.name().value();
            if name != "provider" && name != "consumer" {
                diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("unknown block `{name}` under `enozunu`"),
                ));
            }
        }
    }

    let provider = provider.unwrap_or(Provider {
        skills: Vec::new(),
        agents: Vec::new(),
    });
    let consumer = match consumer {
        Some(c) => c,
        None => {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                "manifest must declare a `consumer` block",
            ));
            Consumer {
                claude: ClaudeConsumer {
                    use_skills: Vec::new(),
                    use_agents: Vec::new(),
                },
            }
        }
    };

    let manifest = Manifest { provider, consumer };
    validate_references(&manifest, &mut diags);

    if diags.is_empty() {
        Ok(manifest)
    } else {
        Err(diags)
    }
}

fn single_node<'a>(doc: &'a KdlDocument, name: &str) -> Result<&'a KdlNode, Diagnostic> {
    let nodes: Vec<_> = doc.nodes().iter().collect();
    match nodes.as_slice() {
        [node] if node.name().value() == name => Ok(node),
        _ => Err(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!("manifest must have exactly one root node named `{name}`"),
        )),
    }
}

fn check_config_version(root: &KdlNode, diags: &mut Vec<Diagnostic>) {
    match root.get("config-version").and_then(|v| v.as_integer()) {
        Some(SUPPORTED_CONFIG_VERSION) => {}
        Some(other) => diags.push(Diagnostic::new(
            DiagnosticCode::UnsupportedConfigVersion,
            format!("config-version {other} is not supported; this build supports config-version={SUPPORTED_CONFIG_VERSION}"),
        )),
        None => diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!("root node must declare config-version={SUPPORTED_CONFIG_VERSION}"),
        )),
    }
}

fn parse_provider(node: &KdlNode, diags: &mut Vec<Diagnostic>) -> Provider {
    let mut skills = Vec::new();
    let mut agents = Vec::new();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "skills" => skills = parse_source_decls(child, "skill", diags),
                "agents" => agents = parse_source_decls(child, "agent", diags),
                other => diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("unknown block `{other}` under `provider`"),
                )),
            }
        }
    }

    Provider { skills, agents }
}

fn parse_source_decls(node: &KdlNode, kind: &str, diags: &mut Vec<Diagnostic>) -> Vec<SourceDecl> {
    let mut decls: Vec<SourceDecl> = Vec::new();

    let Some(children) = node.children() else {
        return decls;
    };

    for child in children.nodes() {
        if child.name().value() != kind {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "unknown node `{}` under `provider.{}s`; expected `{kind}`",
                    child.name().value(),
                    kind
                ),
            ));
            continue;
        }

        let Some(name) = first_string_arg(child) else {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!("`{kind}` node must have a string name argument"),
            ));
            continue;
        };

        if let Err(d) = validate_name(&name, kind) {
            diags.push(d);
            continue;
        }

        if decls.iter().any(|d| d.name == name) {
            diags.push(Diagnostic::new(
                DiagnosticCode::DuplicateSourceName,
                format!("{kind} source `{name}` is declared more than once"),
            ));
            continue;
        }

        if let Some(reference) = parse_source_reference(child, kind, &name, diags) {
            decls.push(SourceDecl { name, reference });
        }
    }

    decls
}

fn parse_source_reference(
    node: &KdlNode,
    kind: &str,
    name: &str,
    diags: &mut Vec<Diagnostic>,
) -> Option<SourceReference> {
    let mut git_blocks = Vec::new();
    let mut local_blocks = Vec::new();
    let mut ok = true;

    if let Some(children) = node.children() {
        for block in children.nodes() {
            match block.name().value() {
                "git" => git_blocks.push(block),
                "local" => local_blocks.push(block),
                // `branch` and `path` were top-level fields before source reference blocks existed, so their appearance here most likely means an unmigrated manifest.
                other @ ("branch" | "path" | "url") => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::UnsupportedSourceReference,
                        format!(
                            "{kind} `{name}` declares `{other}` outside a source reference block; declare it inside a `git` block"
                        ),
                    ));
                    ok = false;
                }
                other => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::UnsupportedSourceReference,
                        format!(
                            "{kind} `{name}` uses unsupported source reference block `{other}`; supported blocks are `git` and `local`"
                        ),
                    ));
                    ok = false;
                }
            }
        }
    }

    let reference = match (git_blocks.as_slice(), local_blocks.as_slice()) {
        ([git], []) => parse_git_reference(git, kind, name, diags),
        ([], [local]) => parse_local_reference(local, kind, name, diags),
        ([], []) => {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "{kind} `{name}` must contain exactly one source reference block (`git` or `local`)"
                ),
            ));
            None
        }
        (git, local) => {
            let found = if !git.is_empty() && !local.is_empty() {
                "both `git` and `local` blocks".to_owned()
            } else if git.len() > 1 {
                format!("{} `git` blocks", git.len())
            } else {
                format!("{} `local` blocks", local.len())
            };
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "{kind} `{name}` declares {found}; exactly one source reference block is allowed"
                ),
            ));
            None
        }
    };

    if ok { reference } else { None }
}

fn parse_git_reference(
    node: &KdlNode,
    kind: &str,
    name: &str,
    diags: &mut Vec<Diagnostic>,
) -> Option<SourceReference> {
    // A positional argument on `git` is the pre-block manifest form (`git "<url>"`), so point at the migration instead of reporting a missing `url`.
    if first_string_arg(node).is_some() {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "`git` block of {kind} `{name}` takes no argument; declare `url` inside the block"
            ),
        ));
        return None;
    }

    let mut url = None;
    let mut branch = None;
    let mut path = None;
    let mut ok = true;

    if let Some(children) = node.children() {
        for field in children.nodes() {
            let field_name = field.name().value();
            let value = first_string_arg(field);
            match (field_name, value) {
                ("url", Some(v)) => url = Some(v),
                ("branch", Some(v)) => branch = Some(v),
                ("path", Some(v)) => path = Some(v),
                ("url" | "branch" | "path", None) => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::ManifestShape,
                        format!("`{field_name}` of {kind} `{name}` must have a string value"),
                    ));
                    ok = false;
                }
                (other, _) => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::UnsupportedSourceReference,
                        format!(
                            "`git` block of {kind} `{name}` uses unsupported field `{other}`; it accepts only url + branch + path"
                        ),
                    ));
                    ok = false;
                }
            }
        }
    }

    for (field, value) in [("url", &url), ("branch", &branch), ("path", &path)] {
        if value.is_none() {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!("`git` block of {kind} `{name}` is missing required field `{field}`"),
            ));
            ok = false;
        }
    }

    if !ok {
        return None;
    }

    let url = url.unwrap();
    let branch = branch.unwrap();
    let path = path.unwrap();

    // Manifest values reach the external git command as arguments.
    // A leading `-` would let git parse a value as an option (for example `--upload-pack=<command>`), so such values are rejected as configuration errors.
    for (field, value) in [("url", &url), ("branch", &branch)] {
        if value.starts_with('-') {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "`{field}` of {kind} `{name}` must not start with `-`; it would be interpreted as a git option"
                ),
            ));
            return None;
        }
    }
    if branch.is_empty() {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!("`branch` of {kind} `{name}` must not be empty"),
        ));
        return None;
    }

    if is_github_tree_blob_shorthand(&url) {
        diags.push(Diagnostic::new(
            DiagnosticCode::UnsupportedSourceReference,
            format!(
                "{kind} `{name}` uses a GitHub tree/blob URL shorthand; use the normalized url + branch + path form"
            ),
        ));
        return None;
    }

    if let Err(d) = validate_source_path(&path, kind, name) {
        diags.push(d);
        return None;
    }

    Some(SourceReference::Git { url, branch, path })
}

fn parse_local_reference(
    node: &KdlNode,
    kind: &str,
    name: &str,
    diags: &mut Vec<Diagnostic>,
) -> Option<SourceReference> {
    if first_string_arg(node).is_some() {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "`local` block of {kind} `{name}` takes no argument; declare `path` inside the block"
            ),
        ));
        return None;
    }

    let mut path = None;
    let mut ok = true;

    if let Some(children) = node.children() {
        for field in children.nodes() {
            let field_name = field.name().value();
            let value = first_string_arg(field);
            match (field_name, value) {
                ("path", Some(v)) => path = Some(v),
                ("path", None) => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::ManifestShape,
                        format!("`path` of {kind} `{name}` must have a string value"),
                    ));
                    ok = false;
                }
                (other, _) => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::UnsupportedSourceReference,
                        format!(
                            "`local` block of {kind} `{name}` uses unsupported field `{other}`; it accepts only path"
                        ),
                    ));
                    ok = false;
                }
            }
        }
    }

    let Some(path) = path else {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!("`local` block of {kind} `{name}` is missing required field `path`"),
        ));
        return None;
    };

    if !ok {
        return None;
    }

    if let Err(d) = validate_local_source_path(&path, kind, name) {
        diags.push(d);
        return None;
    }

    Some(SourceReference::Local { path })
}

fn parse_consumer(node: &KdlNode, diags: &mut Vec<Diagnostic>) -> Consumer {
    let mut claude = None;

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "claude" => claude = Some(parse_claude_consumer(child, diags)),
                "codex" => diags.push(Diagnostic::new(
                    DiagnosticCode::UnsupportedConsumer,
                    "`consumer.codex` is not supported in v0.0.x; the only supported target AI is Claude",
                )),
                other => diags.push(Diagnostic::new(
                    DiagnosticCode::UnsupportedConsumer,
                    format!("`consumer.{other}` is not supported in v0.0.x; the only supported target AI is Claude"),
                )),
            }
        }
    }

    Consumer {
        claude: claude.unwrap_or(ClaudeConsumer {
            use_skills: Vec::new(),
            use_agents: Vec::new(),
        }),
    }
}

fn parse_claude_consumer(node: &KdlNode, diags: &mut Vec<Diagnostic>) -> ClaudeConsumer {
    let mut use_skills = Vec::new();
    let mut use_agents = Vec::new();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "use-skills" => use_skills = string_args(child, "use-skills", diags),
                "use-agents" => use_agents = string_args(child, "use-agents", diags),
                other => diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("unknown node `{other}` under `consumer.claude`"),
                )),
            }
        }
    }

    ClaudeConsumer {
        use_skills,
        use_agents,
    }
}

fn validate_references(manifest: &Manifest, diags: &mut Vec<Diagnostic>) {
    for name in &manifest.consumer.claude.use_skills {
        if !manifest.provider.skills.iter().any(|s| &s.name == name) {
            diags.push(Diagnostic::new(
                DiagnosticCode::UnknownSourceReference,
                format!("`use-skills` references `{name}`, which is not declared under `provider.skills`"),
            ));
        }
    }
    for name in &manifest.consumer.claude.use_agents {
        if !manifest.provider.agents.iter().any(|s| &s.name == name) {
            diags.push(Diagnostic::new(
                DiagnosticCode::UnknownSourceReference,
                format!("`use-agents` references `{name}`, which is not declared under `provider.agents`"),
            ));
        }
    }
}

/// Source names become path segments under `.claude/`, so they must be single safe segments.
fn validate_name(name: &str, kind: &str) -> Result<(), Diagnostic> {
    let safe = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if safe {
        Ok(())
    } else {
        Err(Diagnostic::new(
            DiagnosticCode::InvalidName,
            format!(
                "{kind} name `{name}` must be a single path-safe segment (ASCII letters, digits, `-`, `_`, `.`)"
            ),
        ))
    }
}

/// Local paths resolve from the manifest directory, so `..` segments are allowed for sibling repositories.
/// Absolute paths are a portability hazard in a shared manifest, so v0.0.x rejects them until support is decided explicitly.
fn validate_local_source_path(path: &str, kind: &str, name: &str) -> Result<(), Diagnostic> {
    if path.starts_with('/') {
        return Err(Diagnostic::new(
            DiagnosticCode::UnsupportedSourceReference,
            format!(
                "{kind} `{name}` local path `{path}` is absolute; v0.0.x accepts only paths relative to the manifest directory"
            ),
        ));
    }
    if path.is_empty() || path.split('/').any(|seg| seg.is_empty()) {
        return Err(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "{kind} `{name}` local path `{path}` must be a non-empty relative path without empty segments"
            ),
        ));
    }
    Ok(())
}

/// Dot segments are rejected rather than normalized so that path containment does not depend on host-specific normalization.
fn validate_source_path(path: &str, kind: &str, name: &str) -> Result<(), Diagnostic> {
    let invalid = path.is_empty()
        || path.starts_with('/')
        || path.split('/').any(|seg| seg.is_empty() || seg == "..");
    if invalid {
        Err(Diagnostic::new(
            DiagnosticCode::UnsafePath,
            format!(
                "{kind} `{name}` path `{path}` must be a relative path without empty or `..` segments"
            ),
        ))
    } else {
        Ok(())
    }
}

fn is_github_tree_blob_shorthand(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    else {
        return false;
    };
    let segments: Vec<&str> = rest.split('/').collect();
    segments.len() > 2 && matches!(segments[2], "tree" | "blob")
}

fn first_string_arg(node: &KdlNode) -> Option<String> {
    node.entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .map(str::to_owned)
}

fn string_args(node: &KdlNode, label: &str, diags: &mut Vec<Diagnostic>) -> Vec<String> {
    let mut values = Vec::new();
    for entry in node.entries() {
        if entry.name().is_some() {
            continue;
        }
        match entry.value().as_string() {
            Some(v) => values.push(v.to_owned()),
            None => diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!("`{label}` arguments must be strings"),
            )),
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "git-kura" {
        git {
          url "https://github.com/tooppoo/reportage"
          branch "main"
          path ".claude/skills/git-kura"
        }
      }

      skill "local-git-kura" {
        local {
          path "../reportage/.claude/skills/git-kura"
        }
      }
    }

    agents {
      agent "shell-script-reviewer" {
        git {
          url "https://github.com/tooppoo/installerer"
          branch "main"
          path ".claude/agents/shell-script-reviewer.md"
        }
      }
    }
  }

  consumer {
    claude {
      use-skills "git-kura" "local-git-kura"
      use-agents "shell-script-reviewer"
    }
  }
}
"#;

    fn codes(result: Result<Manifest, Vec<Diagnostic>>) -> Vec<DiagnosticCode> {
        result.unwrap_err().into_iter().map(|d| d.code).collect()
    }

    #[test]
    fn parses_valid_manifest() {
        let manifest = parse(VALID).unwrap();
        assert_eq!(manifest.provider.skills.len(), 2);
        assert_eq!(manifest.provider.agents.len(), 1);
        assert_eq!(
            manifest.consumer.claude.use_skills,
            ["git-kura", "local-git-kura"]
        );
        assert_eq!(
            manifest.provider.skills[0].reference,
            SourceReference::Git {
                url: "https://github.com/tooppoo/reportage".to_owned(),
                branch: "main".to_owned(),
                path: ".claude/skills/git-kura".to_owned(),
            }
        );
        assert_eq!(
            manifest.provider.skills[1].reference,
            SourceReference::Local {
                path: "../reportage/.claude/skills/git-kura".to_owned(),
            }
        );
    }

    #[test]
    fn rejects_invalid_kdl_syntax() {
        assert!(codes(parse("enozunu {")).contains(&DiagnosticCode::ManifestSyntax));
    }

    #[test]
    fn rejects_missing_config_version() {
        assert!(
            codes(parse("enozunu { consumer { claude {} } }"))
                .contains(&DiagnosticCode::ManifestShape)
        );
    }

    #[test]
    fn rejects_unsupported_config_version() {
        assert!(
            codes(parse("enozunu config-version=2 { consumer { claude {} } }"))
                .contains(&DiagnosticCode::UnsupportedConfigVersion)
        );
    }

    #[test]
    fn rejects_consumer_codex() {
        let text = r#"
enozunu config-version=1 {
  consumer {
    codex {}
  }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::UnsupportedConsumer));
    }

    #[test]
    fn rejects_duplicate_source_names_within_kind() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "p" } }
      skill "a" { git { url "https://example.com/r"; branch "main"; path "q" } }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::DuplicateSourceName));
    }

    #[test]
    fn rejects_unknown_use_reference() {
        let text = r#"
enozunu config-version=1 {
  consumer {
    claude {
      use-skills "missing"
    }
  }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::UnknownSourceReference));
    }

    #[test]
    fn rejects_github_tree_shorthand() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" {
        git {
          url "https://github.com/owner/repo/tree/main/path"
          branch "main"
          path "p"
        }
      }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::UnsupportedSourceReference));
    }

    #[test]
    fn rejects_missing_git_reference_fields() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r" } }
    }
  }
  consumer { claude {} }
}
"#;
        let codes = codes(parse(text));
        assert_eq!(
            codes
                .iter()
                .filter(|c| **c == DiagnosticCode::ManifestShape)
                .count(),
            2
        );
    }

    #[test]
    fn rejects_source_without_reference_block() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" {}
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_source_with_both_git_and_local() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" {
        git { url "https://example.com/r"; branch "main"; path "p" }
        local { path "../p" }
      }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_source_with_multiple_git_blocks() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" {
        git { url "https://example.com/r"; branch "main"; path "p" }
        git { url "https://example.com/r"; branch "main"; path "q" }
      }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_source_with_multiple_local_blocks() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" {
        local { path "../p" }
        local { path "../q" }
      }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_unsupported_source_reference_block() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { svn { path "p" } }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::UnsupportedSourceReference));
    }

    #[test]
    fn rejects_pre_block_flat_reference_form() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" {
        git "https://example.com/r"
        branch "main"
        path "p"
      }
    }
  }
  consumer { claude {} }
}
"#;
        let codes = codes(parse(text));
        // `branch` and `path` outside a block are reported as unsupported, and the argument-carrying `git` node as a shape error.
        assert!(codes.contains(&DiagnosticCode::UnsupportedSourceReference));
        assert!(codes.contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_traversal_git_source_path() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "../escape" } }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::UnsafePath));
    }

    #[test]
    fn rejects_unsafe_source_name() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "../evil" { git { url "https://example.com/r"; branch "main"; path "p" } }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::InvalidName));
    }

    #[test]
    fn rejects_branch_that_looks_like_a_git_option() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "--upload-pack=evil"; path "p" } }
    }
  }
  consumer { claude { use-skills "a" } }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_git_url_that_looks_like_a_git_option() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "--upload-pack=evil"; branch "main"; path "p" } }
    }
  }
  consumer { claude { use-skills "a" } }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn accepts_source_path_outside_dot_claude() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "anywhere/else" } }
    }
  }
  consumer { claude { use-skills "a" } }
}
"#;
        assert!(parse(text).is_ok());
    }

    #[test]
    fn accepts_local_path_with_parent_segments() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { local { path "../../sibling/skills/a" } }
    }
  }
  consumer { claude { use-skills "a" } }
}
"#;
        assert!(parse(text).is_ok());
    }

    #[test]
    fn rejects_absolute_local_path() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { local { path "/etc/skills/a" } }
    }
  }
  consumer { claude { use-skills "a" } }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::UnsupportedSourceReference));
    }

    #[test]
    fn rejects_local_block_missing_path() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { local {} }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_unsupported_field_inside_local_block() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { local { path "../p"; branch "main" } }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::UnsupportedSourceReference));
    }
}
