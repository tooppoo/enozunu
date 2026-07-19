//! Parses and validates `enozunu.kdl` into the domain model.
//!
//! This module owns syntax-level and schema-level validation of the human-authored manifest.
//! It does not resolve Git sources or inspect the filesystem; those checks belong to the resolution and materialization layers.

use kdl::{KdlDocument, KdlNode};

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::git::{CommitSha, GitSelector};

pub const SUPPORTED_CONFIG_VERSION: i128 = 1;

/// A validated Gist identifier: the final path segment of a Gist URL.
///
/// v0 accepts a non-empty lowercase ASCII hexadecimal string with no fixed length. This is a conservative accepted-input contract, not a claim that GitHub guarantees a particular Gist id length or representation.
/// Percent-encoded and otherwise non-canonical forms are rejected because a validated id is interpolated directly into the Gist Git remote URL; Enozunu does not percent-encode arbitrary manifest input to build the remote.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GistId(String);

impl GistId {
    /// Parses `raw` as a Gist id, returning `None` for any non-canonical form.
    pub fn parse(raw: &str) -> Option<Self> {
        let canonical = !raw.is_empty()
            && raw
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        canonical.then(|| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

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

/// How a Gist source selects its artifact inside the resolved revision.
///
/// The Skill/agent difference is a typed selector rather than an optional `file`, so a root-selecting Skill Gist can never carry a stray `file` value into resolution or materialization.
/// The parser guarantees the mapping: `provider.skills` + `gist` produces `Root`, and `provider.agents` + `gist` produces `File`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GistArtifactSelector {
    /// The root of the pinned Gist revision is the artifact (Skill sources).
    Root,
    /// One file inside the pinned Gist revision is the artifact (agent sources).
    File { path: String },
}

/// A tagged source reference: exactly one `git`, `local`, or `gist` block per source declaration.
///
/// v0.0.x accepts only these kinds; shorthand forms are rejected during validation.
/// See docs/guide/manifest.md for the supported source reference policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceReference {
    /// The commit is selected by `selector` — a sum type rather than parallel optional `branch` / `tag` / `revision` fields — so the invalid states "more than one selector" and "no selector" cannot reach the resolution layer.
    Git {
        url: String,
        selector: GitSelector,
        path: String,
    },
    Local {
        path: String,
    },
    Gist {
        id: GistId,
        revision: CommitSha,
        selector: GistArtifactSelector,
    },
}

/// A target AI that Enozunu materializes configuration for.
///
/// Target AI is a closed domain type rather than a manifest string carried downstream, so native paths, the provenance `target_ai`, and CLI rendering all derive from the same validated value.
/// The set is closed at parse time: an unrecognized `consumer` block is a diagnostic, never a new `TargetAi`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetAi {
    Claude,
    Codex,
}

impl TargetAi {
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetAi::Claude => "claude",
            TargetAi::Codex => "codex",
        }
    }
}

/// The sources one target AI selects from the shared provider pool.
///
/// The type is target-independent: Claude and Codex select from the same `provider.skills` and `provider.agents`, and the target only changes where each selection materializes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetConsumer {
    pub use_skills: Vec<String>,
    pub use_agents: Vec<String>,
}

/// The `consumer` block: which target AIs to materialize for.
///
/// A target is `None` when its block is absent, so a Claude-only manifest and a Codex-only manifest are both valid without a placeholder selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumer {
    pub claude: Option<TargetConsumer>,
    pub codex: Option<TargetConsumer>,
}

impl Consumer {
    /// Iterates the declared targets, pairing each with its `TargetAi`, in a fixed order.
    ///
    /// Planning and reference validation walk every target through this one accessor, so neither has to special-case which targets exist.
    pub fn targets(&self) -> impl Iterator<Item = (TargetAi, &TargetConsumer)> {
        [
            (TargetAi::Claude, self.claude.as_ref()),
            (TargetAi::Codex, self.codex.as_ref()),
        ]
        .into_iter()
        .filter_map(|(ai, consumer)| consumer.map(|c| (ai, c)))
    }
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
                claude: None,
                codex: None,
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
    let mut gist_blocks = Vec::new();
    let mut ok = true;

    if let Some(children) = node.children() {
        for block in children.nodes() {
            match block.name().value() {
                "git" => git_blocks.push(block),
                "local" => local_blocks.push(block),
                "gist" => gist_blocks.push(block),
                // `branch` and `path` were top-level fields before source reference blocks existed, so their appearance here most likely means an unmigrated manifest; `revision` and `tag` are grouped with them because they are also `git` block fields.
                other @ ("branch" | "revision" | "tag" | "path" | "url") => {
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
                            "{kind} `{name}` uses unsupported source reference block `{other}`; supported blocks are `git`, `local`, and `gist`"
                        ),
                    ));
                    ok = false;
                }
            }
        }
    }

    let total = git_blocks.len() + local_blocks.len() + gist_blocks.len();
    let reference = if total == 0 {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "{kind} `{name}` must contain exactly one source reference block (`git`, `local`, or `gist`)"
            ),
        ));
        None
    } else if total > 1 {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "{kind} `{name}` declares {total} source reference blocks; exactly one is allowed"
            ),
        ));
        None
    } else if let [git] = git_blocks.as_slice() {
        parse_git_reference(git, kind, name, diags)
    } else if let [local] = local_blocks.as_slice() {
        parse_local_reference(local, kind, name, diags)
    } else if let [gist] = gist_blocks.as_slice() {
        parse_gist_reference(gist, kind, name, diags)
    } else {
        None
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
    let mut tag = None;
    let mut revision = None;
    let mut path = None;
    let mut ok = true;

    if let Some(children) = node.children() {
        for field in children.nodes() {
            let field_name = field.name().value();
            let value = first_string_arg(field);
            // A repeated field is a shape error rather than last-value-wins, so a manifest cannot silently resolve a different commit than the one it appears to declare.
            let slot = match field_name {
                "url" => Some(&mut url),
                "branch" => Some(&mut branch),
                "tag" => Some(&mut tag),
                "revision" => Some(&mut revision),
                "path" => Some(&mut path),
                _ => None,
            };
            match (slot, value) {
                (Some(slot), Some(v)) => {
                    if slot.is_some() {
                        diags.push(Diagnostic::new(
                            DiagnosticCode::ManifestShape,
                            format!(
                                "`git` block of {kind} `{name}` declares `{field_name}` more than once"
                            ),
                        ));
                        ok = false;
                    } else {
                        *slot = Some(v);
                    }
                }
                (Some(_), None) => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::ManifestShape,
                        format!("`{field_name}` of {kind} `{name}` must have a string value"),
                    ));
                    ok = false;
                }
                (None, _) => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::UnsupportedSourceReference,
                        format!(
                            "`git` block of {kind} `{name}` uses unsupported field `{field_name}`; it accepts url + path + exactly one of branch, tag, revision"
                        ),
                    ));
                    ok = false;
                }
            }
        }
    }

    for (field, value) in [("url", &url), ("path", &path)] {
        if value.is_none() {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!("`git` block of {kind} `{name}` is missing required field `{field}`"),
            ));
            ok = false;
        }
    }

    // Exclusivity is checked over the whole selector set rather than pairwise, so the report names exactly which selectors were declared for any combination of the three.
    let declared_selectors: Vec<&str> = [
        ("branch", branch.is_some()),
        ("tag", tag.is_some()),
        ("revision", revision.is_some()),
    ]
    .into_iter()
    .filter_map(|(field, declared)| declared.then_some(field))
    .collect();

    if declared_selectors.len() != 1 {
        let detail = if declared_selectors.is_empty() {
            "none is declared".to_owned()
        } else {
            let declared = declared_selectors
                .iter()
                .map(|field| format!("`{field}`"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{declared} are declared")
        };
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "`git` block of {kind} `{name}` must declare exactly one selector out of `branch`, `tag`, and `revision`, but {detail}"
            ),
        ));
        ok = false;
    }

    if !ok {
        return None;
    }

    let url = url.unwrap();
    let path = path.unwrap();

    // Manifest values reach the external git command as arguments.
    // A leading `-` would let git parse a value as an option (for example `--upload-pack=<command>`), so such values are rejected as configuration errors.
    // A validated revision is lowercase hexadecimal, so only `url`, `branch`, and `tag` can carry such a value.
    for (field, value) in [
        ("url", Some(&url)),
        ("branch", branch.as_ref()),
        ("tag", tag.as_ref()),
    ] {
        if value.is_some_and(|v| v.starts_with('-')) {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "`{field}` of {kind} `{name}` must not start with `-`; it would be interpreted as a git option"
                ),
            ));
            return None;
        }
    }

    let selector = match (branch, tag, revision) {
        (Some(branch), None, None) => {
            if branch.is_empty() {
                diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("`branch` of {kind} `{name}` must not be empty"),
                ));
                return None;
            }
            GitSelector::Branch(branch)
        }
        (None, Some(tag), None) => {
            if tag.is_empty() {
                diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("`tag` of {kind} `{name}` must not be empty"),
                ));
                return None;
            }
            // The value reaches git inside a `refs/tags/<tag>` refspec, where `:` separates source from destination; a tag carrying one would fetch a different ref than the manifest declares.
            if tag.contains(':') {
                diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!(
                        "`tag` of {kind} `{name}` must not contain `:`; it would be interpreted as a git refspec separator"
                    ),
                ));
                return None;
            }
            // The resolver adds the `refs/tags/` prefix, so an already-qualified value would resolve `refs/tags/refs/tags/<tag>`. Rejecting it here reports a manifest mistake as a manifest error instead of as a remote-resolution failure after a network round-trip.
            if tag.starts_with("refs/") {
                diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!(
                        "`tag` of {kind} `{name}` must be a bare tag name without a `refs/` prefix; the `refs/tags/` namespace is applied during resolution"
                    ),
                ));
                return None;
            }
            GitSelector::Tag(tag)
        }
        (None, None, Some(revision_raw)) => {
            let Some(revision) = CommitSha::parse(&revision_raw) else {
                diags.push(Diagnostic::new(
                    DiagnosticCode::InvalidRevision,
                    format!(
                        "`revision` of {kind} `{name}` (`{revision_raw}`) must be exactly 40 lowercase hexadecimal characters"
                    ),
                ));
                return None;
            };
            GitSelector::Revision(revision)
        }
        // Exactly one selector is present here; the exclusivity check above already returned otherwise.
        _ => unreachable!("selector exclusivity was validated above"),
    };

    if is_github_tree_blob_shorthand(&url) {
        diags.push(Diagnostic::new(
            DiagnosticCode::UnsupportedSourceReference,
            format!(
                "{kind} `{name}` uses a GitHub tree/blob URL shorthand; use the normalized url + selector + path form"
            ),
        ));
        return None;
    }

    if let Err(d) = validate_source_path(&path, kind, name) {
        diags.push(d);
        return None;
    }

    Some(SourceReference::Git {
        url,
        selector,
        path,
    })
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

fn parse_gist_reference(
    node: &KdlNode,
    kind: &str,
    name: &str,
    diags: &mut Vec<Diagnostic>,
) -> Option<SourceReference> {
    // A Skill Gist selects the pinned revision root, so `file` is part of the agent contract only.
    let file_supported = kind == "agent";
    let accepted_fields = if file_supported {
        "id + revision + file"
    } else {
        "id + revision"
    };

    if first_string_arg(node).is_some() {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "`gist` block of {kind} `{name}` takes no argument; declare {accepted_fields} inside the block"
            ),
        ));
        return None;
    }

    let mut id = None;
    let mut revision = None;
    let mut file = None;
    let mut ok = true;

    if let Some(children) = node.children() {
        for field in children.nodes() {
            let field_name = field.name().value();
            let value = first_string_arg(field);
            // A repeated field is a shape error rather than last-value-wins, so a manifest cannot silently pin a different revision than the one it appears to declare.
            let slot = match field_name {
                "id" => Some(&mut id),
                "revision" => Some(&mut revision),
                "file" if file_supported => Some(&mut file),
                _ => None,
            };
            match (slot, value) {
                (Some(slot), Some(v)) => {
                    if slot.is_some() {
                        diags.push(Diagnostic::new(
                            DiagnosticCode::ManifestShape,
                            format!(
                                "`gist` block of {kind} `{name}` declares `{field_name}` more than once"
                            ),
                        ));
                        ok = false;
                    } else {
                        *slot = Some(v);
                    }
                }
                (Some(_), None) => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::ManifestShape,
                        format!("`{field_name}` of {kind} `{name}` must have a string value"),
                    ));
                    ok = false;
                }
                (None, _) => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::UnsupportedSourceReference,
                        format!(
                            "`gist` block of {kind} `{name}` uses unsupported field `{field_name}`; it accepts only {accepted_fields}"
                        ),
                    ));
                    ok = false;
                }
            }
        }
    }

    let mut required = vec![("id", &id), ("revision", &revision)];
    if file_supported {
        required.push(("file", &file));
    }
    for (field, value) in required {
        if value.is_none() {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!("`gist` block of {kind} `{name}` is missing required field `{field}`"),
            ));
            ok = false;
        }
    }

    if !ok {
        return None;
    }

    let id_raw = id.unwrap();
    let revision_raw = revision.unwrap();

    let Some(id) = GistId::parse(&id_raw) else {
        diags.push(Diagnostic::new(
            DiagnosticCode::InvalidGistId,
            format!(
                "`id` of {kind} `{name}` (`{id_raw}`) must be a non-empty lowercase hexadecimal Gist id"
            ),
        ));
        return None;
    };

    let Some(revision) = CommitSha::parse(&revision_raw) else {
        diags.push(Diagnostic::new(
            DiagnosticCode::InvalidRevision,
            format!(
                "`revision` of {kind} `{name}` (`{revision_raw}`) must be exactly 40 lowercase hexadecimal characters"
            ),
        ));
        return None;
    };

    let selector = if let Some(file) = file {
        // The `file` path follows the same safe-relative-path policy as a Git source path; it is resolved and containment-checked against the exported Gist content during materialization.
        if let Err(d) = validate_source_path(&file, kind, name) {
            diags.push(d);
            return None;
        }
        GistArtifactSelector::File { path: file }
    } else {
        GistArtifactSelector::Root
    };

    Some(SourceReference::Gist {
        id,
        revision,
        selector,
    })
}

fn parse_consumer(node: &KdlNode, diags: &mut Vec<Diagnostic>) -> Consumer {
    let mut claude = None;
    let mut codex = None;

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "claude" => claude = Some(parse_target_consumer(child, "claude", diags)),
                "codex" => codex = Some(parse_target_consumer(child, "codex", diags)),
                other => diags.push(Diagnostic::new(
                    DiagnosticCode::UnsupportedConsumer,
                    format!(
                        "`consumer.{other}` is not a supported target AI; supported target AIs are `claude` and `codex`"
                    ),
                )),
            }
        }
    }

    Consumer { claude, codex }
}

fn parse_target_consumer(
    node: &KdlNode,
    target: &str,
    diags: &mut Vec<Diagnostic>,
) -> TargetConsumer {
    let mut use_skills = Vec::new();
    let mut use_agents = Vec::new();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "use-skills" => use_skills = string_args(child, "use-skills", diags),
                "use-agents" => use_agents = string_args(child, "use-agents", diags),
                other => diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("unknown node `{other}` under `consumer.{target}`"),
                )),
            }
        }
    }

    TargetConsumer {
        use_skills,
        use_agents,
    }
}

fn validate_references(manifest: &Manifest, diags: &mut Vec<Diagnostic>) {
    // Every target selects from the same provider pool, so each target's references are checked against the same `provider.skills` / `provider.agents`.
    for (ai, consumer) in manifest.consumer.targets() {
        let target = ai.as_str();
        for name in &consumer.use_skills {
            if !manifest.provider.skills.iter().any(|s| &s.name == name) {
                diags.push(Diagnostic::new(
                    DiagnosticCode::UnknownSourceReference,
                    format!(
                        "`consumer.{target}` `use-skills` references `{name}`, which is not declared under `provider.skills`"
                    ),
                ));
            }
        }
        for name in &consumer.use_agents {
            if !manifest.provider.agents.iter().any(|s| &s.name == name) {
                diags.push(Diagnostic::new(
                    DiagnosticCode::UnknownSourceReference,
                    format!(
                        "`consumer.{target}` `use-agents` references `{name}`, which is not declared under `provider.agents`"
                    ),
                ));
            }
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
    // The manifest is shared across hosts, so Windows-style absolute forms (drive letter, `\` root, UNC) are rejected on every platform, not only where `Path::is_absolute` recognizes them.
    let absolute_like = path.starts_with('/')
        || path.starts_with('\\')
        || path.chars().nth(1) == Some(':')
        || std::path::Path::new(path).is_absolute();
    if absolute_like {
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

    fn messages(result: Result<Manifest, Vec<Diagnostic>>) -> Vec<String> {
        result.unwrap_err().into_iter().map(|d| d.message).collect()
    }

    #[test]
    fn parses_valid_manifest() {
        let manifest = parse(VALID).unwrap();
        assert_eq!(manifest.provider.skills.len(), 2);
        assert_eq!(manifest.provider.agents.len(), 1);
        assert_eq!(
            manifest.consumer.claude.as_ref().unwrap().use_skills,
            ["git-kura", "local-git-kura"]
        );
        assert_eq!(
            manifest.provider.skills[0].reference,
            SourceReference::Git {
                url: "https://github.com/tooppoo/reportage".to_owned(),
                selector: GitSelector::Branch("main".to_owned()),
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
    fn accepts_a_codex_only_manifest() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "git-kura" { git { url "https://example.com/r"; branch "main"; path "s/git-kura" } }
    }
    agents {
      agent "reviewer-codex" { git { url "https://example.com/r"; branch "main"; path "a/reviewer.toml" } }
    }
  }
  consumer {
    codex {
      use-skills "git-kura"
      use-agents "reviewer-codex"
    }
  }
}
"#;
        let manifest = parse(text).unwrap();
        assert!(manifest.consumer.claude.is_none());
        let codex = manifest.consumer.codex.as_ref().unwrap();
        assert_eq!(codex.use_skills, ["git-kura"]);
        assert_eq!(codex.use_agents, ["reviewer-codex"]);
    }

    #[test]
    fn accepts_a_manifest_selecting_one_skill_from_both_targets() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "git-kura" { git { url "https://example.com/r"; branch "main"; path "s/git-kura" } }
    }
  }
  consumer {
    claude {
      use-skills "git-kura"
    }
    codex {
      use-skills "git-kura"
    }
  }
}
"#;
        let manifest = parse(text).unwrap();
        assert_eq!(
            manifest.consumer.claude.as_ref().unwrap().use_skills,
            ["git-kura"]
        );
        assert_eq!(
            manifest.consumer.codex.as_ref().unwrap().use_skills,
            ["git-kura"]
        );
    }

    #[test]
    fn rejects_an_unknown_node_under_consumer_codex() {
        let text = r#"
enozunu config-version=1 {
  consumer {
    codex {
      use-plugins "x"
    }
  }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_an_undeclared_codex_reference() {
        let text = r#"
enozunu config-version=1 {
  consumer {
    codex {
      use-skills "missing"
    }
  }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::UnknownSourceReference));
    }

    #[test]
    fn rejects_an_unknown_consumer_target() {
        let text = r#"
enozunu config-version=1 {
  consumer {
    gemini {}
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
        // A url-only block is missing `path` and a selector, so both are reported in one run.
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

    /// Wraps a `git { ... }` block body in a skill declaration, without selecting it, so tests exercise git block parsing in isolation.
    fn skill_git(git_body: &str) -> String {
        format!(
            r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "example" {{
        git {{
{git_body}
        }}
      }}
    }}
  }}
  consumer {{ claude {{}} }}
}}
"#
        )
    }

    #[test]
    fn parses_a_git_source_with_a_revision_selector() {
        let text = skill_git(
            r#"          url "https://example.com/r"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          path "s/example""#,
        );
        let manifest = parse(&text).unwrap();
        assert_eq!(
            manifest.provider.skills[0].reference,
            SourceReference::Git {
                url: "https://example.com/r".to_owned(),
                selector: GitSelector::Revision(
                    CommitSha::parse("468aac8caed5f0c3b859b8286968e2c78e2b8760").unwrap()
                ),
                path: "s/example".to_owned(),
            }
        );
    }

    #[test]
    fn rejects_a_git_source_with_both_branch_and_revision() {
        let text = skill_git(
            r#"          url "https://example.com/r"
          branch "main"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          path "s/example""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_a_git_source_without_a_selector() {
        let text = skill_git(
            r#"          url "https://example.com/r"
          path "s/example""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
        // The report names every selector the author may choose from, so a manifest missing one does not have to be cross-referenced against the guide.
        assert!(
            messages(parse(&text)).iter().any(|m| {
                m.contains("`branch`") && m.contains("`tag`") && m.contains("`revision`")
            }),
            "the missing-selector report must name all three selectors"
        );
    }

    #[test]
    fn parses_a_git_source_with_a_tag_selector() {
        let text = skill_git(
            r#"          url "https://example.com/r"
          tag "v1.0.0"
          path "s/example""#,
        );
        let manifest = parse(&text).unwrap();
        assert_eq!(
            manifest.provider.skills[0].reference,
            SourceReference::Git {
                url: "https://example.com/r".to_owned(),
                selector: GitSelector::Tag("v1.0.0".to_owned()),
                path: "s/example".to_owned(),
            }
        );
    }

    #[test]
    fn rejects_a_git_source_declaring_a_tag_with_another_selector() {
        for other in [
            r#"branch "main""#,
            r#"revision "468aac8caed5f0c3b859b8286968e2c78e2b8760""#,
        ] {
            let text = skill_git(&format!(
                r#"          url "https://example.com/r"
          tag "v1.0.0"
          {other}
          path "s/example""#
            ));
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::ManifestShape),
                "`tag` combined with `{other}` must be rejected"
            );
        }
    }

    #[test]
    fn rejects_a_git_source_declaring_all_three_selectors() {
        let text = skill_git(
            r#"          url "https://example.com/r"
          branch "main"
          tag "v1.0.0"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          path "s/example""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
        // The report lists exactly which selectors collided, not just that more than one was present.
        assert!(
            messages(parse(&text)).iter().any(|m| {
                m.contains("`branch`") && m.contains("`tag`") && m.contains("`revision`")
            }),
            "the conflicting-selector report must name each declared selector"
        );
    }

    #[test]
    fn rejects_an_empty_tag() {
        let text = skill_git(
            r#"          url "https://example.com/r"
          tag ""
          path "s/example""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_a_tag_that_looks_like_a_git_option() {
        let text = skill_git(
            r#"          url "https://example.com/r"
          tag "--upload-pack=touch /tmp/pwned"
          path "s/example""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_a_tag_containing_a_refspec_separator() {
        // The value is interpolated into `refs/tags/<tag>`, so a `:` would turn the refspec into a source:destination pair.
        let text = skill_git(
            r#"          url "https://example.com/r"
          tag "v1.0.0:refs/heads/main"
          path "s/example""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_an_already_qualified_tag() {
        // The resolver applies `refs/tags/` itself, so these would resolve `refs/tags/refs/...` and fail only at fetch time.
        for tag in ["refs/tags/v1.0.0", "refs/heads/main"] {
            let text = skill_git(&format!(
                r#"          url "https://example.com/r"
          tag "{tag}"
          path "s/example""#
            ));
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::ManifestShape),
                "an already-qualified tag `{tag}` must be rejected at parse time"
            );
        }
    }

    #[test]
    fn rejects_duplicate_git_fields() {
        for (field, duplicate) in [
            ("url", r#"url "https://example.com/other""#),
            ("branch", r#"branch "develop""#),
            ("path", r#"path "s/other""#),
        ] {
            let text = skill_git(&format!(
                r#"          url "https://example.com/r"
          branch "main"
          path "s/example"
          {duplicate}"#
            ));
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::ManifestShape),
                "duplicate `{field}` must be rejected"
            );
        }

        // A duplicate `revision` is checked with a revision selector so the duplication is the only error.
        let text = skill_git(
            r#"          url "https://example.com/r"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          path "s/example""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));

        // A duplicate `tag` is likewise checked with a tag selector so the duplication is the only error.
        let text = skill_git(
            r#"          url "https://example.com/r"
          tag "v1.0.0"
          tag "v1.0.0"
          path "s/example""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_non_canonical_git_revisions() {
        for revision in [
            "468aac8",                                   // abbreviated
            "468AAC8CAED5F0C3B859B8286968E2C78E2B8760",  // uppercase
            " 468aac8caed5f0c3b859b8286968e2c78e2b876",  // whitespace-padded
            "468aac8caed5f0c3b859b8286968e2c78e2b8760a", // too long
            // 64-char SHA-256 object id
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "v1.0.0", // tag
            "HEAD",   // symbolic ref
            "main~3", // relative revspec
        ] {
            let text = skill_git(&format!(
                r#"          url "https://example.com/r"
          revision "{revision}"
          path "s/example""#
            ));
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::InvalidRevision),
                "revision `{revision}` must be rejected"
            );
        }
    }

    #[test]
    fn rejects_unknown_field_inside_a_git_block() {
        let text = skill_git(
            r#"          url "https://example.com/r"
          branch "main"
          path "s/example"
          depth "1""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::UnsupportedSourceReference));
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
    fn rejects_windows_style_absolute_local_paths() {
        for path in [r"C:/skills/a", r"C:\skills\a", r"\\server\share\a"] {
            // KDL strings escape `\` as `\\`.
            let kdl_path = path.replace('\\', "\\\\");
            let text = format!(
                r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "a" {{ local {{ path "{kdl_path}" }} }}
    }}
  }}
  consumer {{ claude {{ use-skills "a" }} }}
}}
"#
            );
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::UnsupportedSourceReference),
                "path {path} must be rejected as absolute"
            );
        }
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

    /// Wraps a `gist { ... }` block body in an agent declaration, without selecting it, so tests exercise gist parsing in isolation.
    fn agent_gist(gist_body: &str) -> String {
        format!(
            r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "reviewer" {{
        gist {{
{gist_body}
        }}
      }}
    }}
  }}
  consumer {{ claude {{}} }}
}}
"#
        )
    }

    #[test]
    fn parses_valid_gist_agent() {
        let text = r#"
enozunu config-version=1 {
  provider {
    agents {
      agent "shell-script-reviewer" {
        gist {
          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "shell-script-reviewer.md"
        }
      }
    }
  }
  consumer { claude { use-agents "shell-script-reviewer" } }
}
"#;
        let manifest = parse(text).unwrap();
        let SourceReference::Gist {
            id,
            revision,
            selector,
        } = &manifest.provider.agents[0].reference
        else {
            panic!("expected a gist source reference");
        };
        assert_eq!(id.as_str(), "2decf6c462d9b4418f2");
        assert_eq!(
            revision.as_str(),
            "468aac8caed5f0c3b859b8286968e2c78e2b8760"
        );
        assert_eq!(
            selector,
            &GistArtifactSelector::File {
                path: "shell-script-reviewer.md".to_owned()
            }
        );
    }

    /// Wraps a `gist { ... }` block body in a skill declaration, without selecting it, so tests exercise Skill Gist parsing in isolation.
    fn skill_gist(gist_body: &str) -> String {
        format!(
            r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "semantic-line-breaks" {{
        gist {{
{gist_body}
        }}
      }}
    }}
  }}
  consumer {{ claude {{}} }}
}}
"#
        )
    }

    #[test]
    fn parses_a_skill_gist_as_a_root_selector() {
        let text = skill_gist(
            r#"          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760""#,
        );
        let manifest = parse(&text).unwrap();
        let SourceReference::Gist {
            id,
            revision,
            selector,
        } = &manifest.provider.skills[0].reference
        else {
            panic!("expected a gist source reference");
        };
        assert_eq!(id.as_str(), "2decf6c462d9b4418f2");
        assert_eq!(
            revision.as_str(),
            "468aac8caed5f0c3b859b8286968e2c78e2b8760"
        );
        assert_eq!(selector, &GistArtifactSelector::Root);
    }

    #[test]
    fn rejects_file_inside_a_skill_gist() {
        let text = skill_gist(
            r#"          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "a.md""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::UnsupportedSourceReference));
    }

    #[test]
    fn rejects_unknown_field_inside_a_skill_gist() {
        let text = skill_gist(
            r#"          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          owner "monalisa""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::UnsupportedSourceReference));
    }

    #[test]
    fn rejects_a_skill_gist_missing_a_required_field() {
        let text = skill_gist(r#"          id "2decf6c462d9b4418f2""#);
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_a_duplicate_skill_gist_field() {
        let text = skill_gist(
            r#"          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_a_skill_gist_combined_with_git() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" {
        gist {
          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
        }
        git {
          url "https://example.com/r"
          branch "main"
          path "s/a"
        }
      }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_gist_combined_with_git() {
        let text = r#"
enozunu config-version=1 {
  provider {
    agents {
      agent "reviewer" {
        gist {
          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "a.md"
        }
        git {
          url "https://example.com/r"
          branch "main"
          path "a.md"
        }
      }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_multiple_gist_blocks() {
        let text = r#"
enozunu config-version=1 {
  provider {
    agents {
      agent "reviewer" {
        gist {
          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "a.md"
        }
        gist {
          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "b.md"
        }
      }
    }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_gist_missing_required_field() {
        let text = agent_gist(
            r#"          id "2decf6c462d9b4418f2"
          file "a.md""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_duplicate_gist_field() {
        let text = agent_gist(
            r#"          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "a.md""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_unknown_gist_field() {
        let text = agent_gist(
            r#"          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "a.md"
          owner "monalisa""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::UnsupportedSourceReference));
    }

    #[test]
    fn rejects_non_canonical_gist_id() {
        for id in ["2DECF6C462D9B4418F2", "2decf6c4%62d9", "not-hex", ""] {
            let text = agent_gist(&format!(
                r#"          id "{id}"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "a.md""#
            ));
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::InvalidGistId),
                "id `{id}` must be rejected"
            );
        }
    }

    #[test]
    fn rejects_unsupported_revision_forms() {
        for revision in [
            "468aac8",                                   // abbreviated
            "468AAC8CAED5F0C3B859B8286968E2C78E2B8760",  // uppercase
            "468aac8caed5f0c3b859b8286968e2c78e2b8760a", // too long
            // 64-char SHA-256 object id
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "main", // branch name
        ] {
            let text = agent_gist(&format!(
                r#"          id "2decf6c462d9b4418f2"
          revision "{revision}"
          file "a.md""#
            ));
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::InvalidRevision),
                "revision `{revision}` must be rejected"
            );
        }
    }

    #[test]
    fn rejects_unsafe_gist_file_path() {
        let text = agent_gist(
            r#"          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "../escape.md""#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::UnsafePath));
    }
}
