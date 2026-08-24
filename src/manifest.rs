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
    pub instructions: InstructionsDecl,
}

/// Per-target base document sources for generated root instruction files.
///
/// Unlike the shared `skills` / `agents` pools, an instruction source is bound to one target AI, because the artifact itself is target-bound: a root instruction file exists once per target and has no cross-target selection to express. See the root repository instructions ADR (docs/design/adr/20260803T172859Z_generate-root-repository-instructions-from-manifest.md).
/// A declared source without a corresponding `consumer` target is valid and simply stays unused, matching unselected Skill and agent sources.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstructionsDecl {
    pub claude: Option<SourceReference>,
    pub codex: Option<SourceReference>,
}

impl InstructionsDecl {
    pub fn get(&self, ai: TargetAi) -> Option<&SourceReference> {
        match ai {
            TargetAi::Claude => self.claude.as_ref(),
            TargetAi::Codex => self.codex.as_ref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDecl {
    pub name: String,
    pub reference: SourceReference,
}

/// How a Gist source selects its artifact inside the resolved revision.
///
/// The shape difference is a typed selector rather than an optional `file`, so a root-selecting Skill Gist can never carry a stray `file` value into resolution or materialization.
/// The parser guarantees the mapping: only the directory-shaped `provider.skills` + `gist` produces `Root`; every file-shaped kind (`provider.agents`, `provider.instructions`) produces `File`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GistArtifactSelector {
    /// The root of the pinned Gist revision is the artifact (directory-shaped Skill sources).
    Root,
    /// One file inside the pinned Gist revision is the artifact (file-shaped sources).
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

/// One Skill selection of a target consumer, normalized from the `use-skills` surface forms.
///
/// Both the blockless form (`use-skills "a" "b"`) and the block form with `when` annotations planned by issue #38 normalize to one `SkillUsage` per Skill name, so downstream planning and rendering never branch on which surface syntax selected the Skill.
/// `whens` is empty for a blockless selection; the block form is not parsed yet, so today every parsed usage has empty `whens`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillUsage {
    pub name: String,
    pub whens: Vec<String>,
}

/// The sources one target AI selects from the shared provider pool.
///
/// The type is target-independent: Claude and Codex select from the same `provider.skills` and `provider.agents`, and the target only changes where each selection materializes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetConsumer {
    pub use_skills: Vec<SkillUsage>,
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
        instructions: InstructionsDecl::default(),
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
    let mut instructions = None;

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                "skills" => skills = parse_source_decls(child, "skill", diags),
                "agents" => agents = parse_source_decls(child, "agent", diags),
                "instructions" => {
                    // A repeated block is a shape error rather than last-wins, so a manifest cannot silently generate from a different base document than the one it appears to declare.
                    if instructions.is_some() {
                        diags.push(Diagnostic::new(
                            DiagnosticCode::ManifestShape,
                            "`provider.instructions` is declared more than once",
                        ));
                    } else {
                        instructions = Some(parse_instructions(child, diags));
                    }
                }
                other => diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("unknown block `{other}` under `provider`"),
                )),
            }
        }
    }

    Provider {
        skills,
        agents,
        instructions: instructions.unwrap_or_default(),
    }
}

/// One `provider.instructions.<target>` declaration before cross-target reference resolution.
///
/// A `use-same-instruction` reference resolves against sibling declarations only after every target is parsed, so parsing records the unresolved reference and resolution replaces it with the referenced target's effective `SourceReference`.
/// The reference never becomes its own source identity: only the reused source reference crosses into the resolved `InstructionsDecl`, so the referencing target keeps its own provenance and target path.
/// A declaration that failed to parse is kept as `Invalid` rather than dropped, so a reference to a malformed target does not misreport it as undeclared and repeats no error the failed declaration already produced.
enum InstructionDeclParsed {
    Normal(SourceReference),
    /// A `use-same-instruction "<target>"` reference (issue #60): reuses the referenced target's effective source.
    UseSame {
        referenced: TargetAi,
    },
    Invalid,
}

/// The parsed-but-unresolved `provider.instructions` block.
#[derive(Default)]
struct InstructionsParsed {
    claude: Option<InstructionDeclParsed>,
    codex: Option<InstructionDeclParsed>,
}

impl InstructionsParsed {
    fn slot(&mut self, ai: TargetAi) -> &mut Option<InstructionDeclParsed> {
        match ai {
            TargetAi::Claude => &mut self.claude,
            TargetAi::Codex => &mut self.codex,
        }
    }

    fn get(&self, ai: TargetAi) -> Option<&InstructionDeclParsed> {
        match ai {
            TargetAi::Claude => self.claude.as_ref(),
            TargetAi::Codex => self.codex.as_ref(),
        }
    }
}

/// Parses `provider.instructions`: at most one base document source per target AI, declared directly or reused from another target with `use-same-instruction`.
///
/// The child node name doubles as the source's diagnostic and provenance identity (`instruction `claude``), because an instruction declaration has no user-defined name.
fn parse_instructions(node: &KdlNode, diags: &mut Vec<Diagnostic>) -> InstructionsDecl {
    let mut parsed = InstructionsParsed::default();

    if first_string_arg(node).is_some() {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            "`provider.instructions` takes no argument",
        ));
    }

    if let Some(children) = node.children() {
        for child in children.nodes() {
            let target = child.name().value();
            let target_ai = match target {
                "claude" => TargetAi::Claude,
                "codex" => TargetAi::Codex,
                other => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::UnsupportedConsumer,
                        format!(
                            "`provider.instructions.{other}` is not a supported target AI; supported target AIs are `claude` and `codex`"
                        ),
                    ));
                    continue;
                }
            };
            if first_string_arg(child).is_some() {
                diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("`provider.instructions.{target}` takes no argument"),
                ));
                continue;
            }
            if parsed.get(target_ai).is_some() {
                diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("`provider.instructions.{target}` is declared more than once"),
                ));
                continue;
            }
            // The removed issue #53 alias form is detected by its `same-as` property and rejected with a migration hint rather than falling through to the normal-declaration shape errors, because released config-version=1 manifests used it (issue #68).
            let declaration = if child.get("same-as").is_some() {
                diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!(
                        "`provider.instructions.{target}` `same-as` is no longer supported; declare `use-same-instruction \"<target>\"` inside the target block instead"
                    ),
                ));
                None
            } else if has_use_same_instruction(child) {
                parse_instruction_use_same(child, target, target_ai, diags)
            } else {
                parse_source_reference(child, "instruction", target, diags)
                    .map(InstructionDeclParsed::Normal)
            };
            // A parse failure records `Invalid` rather than absence, so the target still counts as declared for a later reference and duplicate check.
            *parsed.slot(target_ai) = Some(declaration.unwrap_or(InstructionDeclParsed::Invalid));
        }
    }

    resolve_instructions(&parsed, diags)
}

fn has_use_same_instruction(node: &KdlNode) -> bool {
    node.children().is_some_and(|c| {
        c.nodes()
            .iter()
            .any(|n| n.name().value() == "use-same-instruction")
    })
}

/// Parses the `use-same-instruction` form of a `provider.instructions.<target>` declaration (issue #60).
///
/// An instruction source is singular, so the reference is the whole declaration: exactly one `use-same-instruction` node and no source reference block may share the target, because combining them would mean choosing between two sources for one value.
fn parse_instruction_use_same(
    node: &KdlNode,
    target: &str,
    target_ai: TargetAi,
    diags: &mut Vec<Diagnostic>,
) -> Option<InstructionDeclParsed> {
    let children = node
        .children()
        .expect("the caller found a `use-same-instruction` child");
    let (use_same_nodes, other_nodes): (Vec<_>, Vec<_>) = children
        .nodes()
        .iter()
        .partition(|n| n.name().value() == "use-same-instruction");

    if !other_nodes.is_empty() {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "`provider.instructions.{target}` must not combine `use-same-instruction` with a source reference block; an instruction source is exactly one of a source reference block or `use-same-instruction`"
            ),
        ));
        return None;
    }

    let [use_same] = use_same_nodes.as_slice() else {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "`provider.instructions.{target}` declares {} `use-same-instruction` nodes; at most one is allowed",
                use_same_nodes.len()
            ),
        ));
        return None;
    };

    parse_use_same(
        use_same,
        "use-same-instruction",
        &format!("provider.instructions.{target}"),
        target_ai,
        diags,
    )
    .map(|referenced| InstructionDeclParsed::UseSame { referenced })
}

/// Resolves parsed instruction declarations into the domain `InstructionsDecl`.
///
/// A normal declaration keeps its own source. A `use-same-instruction` reference reuses the referenced target's *effective* source, resolved after the whole block is parsed so sibling order does not matter. A reference to an undeclared target is rejected; a reference chain is followed and only a cycle is rejected.
fn resolve_instructions(
    parsed: &InstructionsParsed,
    diags: &mut Vec<Diagnostic>,
) -> InstructionsDecl {
    let mut resolver = InstructionResolver {
        states: [SequenceState::Pending, SequenceState::Pending],
    };
    InstructionsDecl {
        claude: resolver.effective(TargetAi::Claude, parsed, diags),
        codex: resolver.effective(TargetAi::Codex, parsed, diags),
    }
}

/// Resolves each target's effective instruction source, memoized so every slot is computed once and a cycle is reported once.
struct InstructionResolver {
    states: [SequenceState<Option<SourceReference>>; 2],
}

impl InstructionResolver {
    fn effective(
        &mut self,
        ai: TargetAi,
        parsed: &InstructionsParsed,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<SourceReference> {
        match &self.states[state_index(ai)] {
            SequenceState::Done(reference) => return reference.clone(),
            // Cycles are detected at the referencing declaration before recursing, so resolution never re-enters an in-progress slot.
            SequenceState::InProgress => unreachable!("cycle detected before recursion"),
            SequenceState::Pending => {}
        }
        self.states[state_index(ai)] = SequenceState::InProgress;

        let reference = match parsed.get(ai) {
            None => None,
            Some(InstructionDeclParsed::Normal(reference)) => Some(reference.clone()),
            // A declaration that failed to parse already reported its own error; it resolves to no source without a second one.
            Some(InstructionDeclParsed::Invalid) => None,
            Some(InstructionDeclParsed::UseSame { referenced }) => {
                self.reused(ai, *referenced, parsed, diags)
            }
        };

        self.states[state_index(ai)] = SequenceState::Done(reference.clone());
        reference
    }

    /// The referenced target's effective source, or `None` with a diagnostic when the reference is invalid.
    fn reused(
        &mut self,
        ai: TargetAi,
        referenced: TargetAi,
        parsed: &InstructionsParsed,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<SourceReference> {
        if parsed.get(referenced).is_none() {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "`provider.instructions.{}` `use-same-instruction` references `provider.instructions.{}`, which is not declared",
                    ai.as_str(),
                    referenced.as_str()
                ),
            ));
            return None;
        }

        if matches!(
            self.states[state_index(referenced)],
            SequenceState::InProgress
        ) {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "`provider.instructions.{}` `use-same-instruction` reference to `provider.instructions.{}` forms a reference cycle",
                    ai.as_str(),
                    referenced.as_str()
                ),
            ));
            return None;
        }

        self.effective(referenced, parsed, diags)
    }
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
    // Only the directory-shaped Skill Gist selects the pinned revision root; every file-shaped kind (`agent`, `instruction`) selects one `file` inside it, so a new file-shaped kind inherits this contract instead of adding a kind-specific branch.
    let file_supported = kind != "skill";
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

/// One ordered item of a repeatable consumer declaration sequence.
///
/// `Local` holds one node's own selections; `UseSame` expands the referenced target's effective sequence at this position (issue #60). Item order is the manifest node order, and expansion preserves it, so a `use-same-*` node behaves like the referenced declarations written inline.
enum ConsumerItem<T> {
    Local(Vec<T>),
    UseSame(TargetAi),
}

/// One `consumer.<target>` block before cross-target reference resolution.
struct TargetConsumerParsed {
    use_skills: Vec<ConsumerItem<SkillUsage>>,
    use_agents: Vec<ConsumerItem<String>>,
}

/// The parsed-but-unresolved `consumer` block.
#[derive(Default)]
struct ConsumerParsed {
    claude: Option<TargetConsumerParsed>,
    codex: Option<TargetConsumerParsed>,
}

impl ConsumerParsed {
    fn slot(&mut self, ai: TargetAi) -> &mut Option<TargetConsumerParsed> {
        match ai {
            TargetAi::Claude => &mut self.claude,
            TargetAi::Codex => &mut self.codex,
        }
    }

    fn get(&self, ai: TargetAi) -> Option<&TargetConsumerParsed> {
        match ai {
            TargetAi::Claude => self.claude.as_ref(),
            TargetAi::Codex => self.codex.as_ref(),
        }
    }
}

fn parse_consumer(node: &KdlNode, diags: &mut Vec<Diagnostic>) -> Consumer {
    let mut parsed = ConsumerParsed::default();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            let target_ai = match child.name().value() {
                "claude" => TargetAi::Claude,
                "codex" => TargetAi::Codex,
                other => {
                    diags.push(Diagnostic::new(
                        DiagnosticCode::UnsupportedConsumer,
                        format!(
                            "`consumer.{other}` is not a supported target AI; supported target AIs are `claude` and `codex`"
                        ),
                    ));
                    continue;
                }
            };
            *parsed.slot(target_ai) = Some(parse_target_consumer(child, target_ai, diags));
        }
    }

    resolve_consumer(&parsed, diags)
}

fn parse_target_consumer(
    node: &KdlNode,
    target_ai: TargetAi,
    diags: &mut Vec<Diagnostic>,
) -> TargetConsumerParsed {
    let target = target_ai.as_str();
    let mut skill_items: Vec<ConsumerItem<SkillUsage>> = Vec::new();
    let mut agent_items: Vec<ConsumerItem<String>> = Vec::new();

    if let Some(children) = node.children() {
        for child in children.nodes() {
            match child.name().value() {
                // A `use-skills` / `use-agents` node declares selections; it is not a value later nodes override.
                // Nodes concatenate in declaration order, so the grouped form (`use-skills "a" "b"`) and the split form (`use-skills "a"` + `use-skills "b"`) parse identically and node boundaries carry no meaning after parse.
                // A `use-same-*` node joins the same order-preserving concatenation, expanding at its own position (issue #60).
                "use-skills" => {
                    // The removed issue #53 alias form is detected by its `same-as` property and rejected with a migration hint rather than falling through to the generic property rejection, because released config-version=1 manifests used it (issue #68).
                    if child.get("same-as").is_some() {
                        diags.push(Diagnostic::new(
                            DiagnosticCode::ManifestShape,
                            format!(
                                "`use-skills same-as` under `consumer.{target}` is no longer supported; use `use-same-skills \"<target>\"` instead"
                            ),
                        ));
                    } else {
                        skill_items
                            .push(ConsumerItem::Local(parse_use_skills(child, target, diags)));
                    }
                }
                "use-same-skills" => {
                    if let Some(referenced) = parse_use_same(
                        child,
                        "use-same-skills",
                        &format!("consumer.{target}"),
                        target_ai,
                        diags,
                    ) {
                        skill_items.push(ConsumerItem::UseSame(referenced));
                    }
                }
                "use-agents" => {
                    agent_items.push(ConsumerItem::Local(string_args(child, "use-agents", diags)));
                }
                "use-same-agents" => {
                    if let Some(referenced) = parse_use_same(
                        child,
                        "use-same-agents",
                        &format!("consumer.{target}"),
                        target_ai,
                        diags,
                    ) {
                        agent_items.push(ConsumerItem::UseSame(referenced));
                    }
                }
                other => diags.push(Diagnostic::new(
                    DiagnosticCode::ManifestShape,
                    format!("unknown node `{other}` under `consumer.{target}`"),
                )),
            }
        }
    }

    TargetConsumerParsed {
        use_skills: skill_items,
        use_agents: agent_items,
    }
}

/// Parses a `use-same-skills` / `use-same-agents` / `use-same-instruction` node (issue #60).
///
/// The node carries exactly one positional string argument naming the referenced target, and nothing else: no properties, no children.
/// A self-reference is rejected here with a direct message; a transitive cycle through the other target is only detectable during resolution and is rejected there.
/// `context` is the declaring manifest path (`consumer.<target>` or `provider.instructions.<target>`), used verbatim in diagnostics.
fn parse_use_same(
    node: &KdlNode,
    node_name: &str,
    context: &str,
    target_ai: TargetAi,
    diags: &mut Vec<Diagnostic>,
) -> Option<TargetAi> {
    let mut ok = true;
    let fail = |diags: &mut Vec<Diagnostic>, detail: &str| {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!("`{node_name}` under `{context}` {detail}"),
        ));
    };

    if node.entries().iter().any(|e| e.name().is_some()) {
        fail(diags, "must not declare properties");
        ok = false;
    }
    if node.children().is_some() {
        fail(diags, "must not have a child block");
        ok = false;
    }

    let positional: Vec<_> = node
        .entries()
        .iter()
        .filter(|e| e.name().is_none())
        .collect();
    let [entry] = positional.as_slice() else {
        fail(diags, "must have exactly one target argument");
        return None;
    };
    let Some(value) = entry.value().as_string() else {
        fail(diags, "must have a string target argument");
        return None;
    };

    let referenced = match value {
        "claude" => TargetAi::Claude,
        "codex" => TargetAi::Codex,
        other => {
            fail(
                diags,
                &format!(
                    "references unsupported target `{other}`; supported targets are `claude` and `codex`"
                ),
            );
            return None;
        }
    };

    if referenced == target_ai {
        fail(diags, "must not reference itself");
        return None;
    }

    if !ok {
        return None;
    }

    Some(referenced)
}

/// Resolves parsed consumer targets into the domain `Consumer`.
///
/// Resolution runs after the whole `consumer` block is parsed, so a `use-same-*` reference reads the referenced target's effective value regardless of sibling declaration order.
/// Skills and agents resolve as independent reference graphs, each memoized across targets so every sequence is computed once and a cycle is reported once.
fn resolve_consumer(parsed: &ConsumerParsed, diags: &mut Vec<Diagnostic>) -> Consumer {
    let mut skills = SequenceResolver::new(
        |target: &TargetConsumerParsed| &target.use_skills,
        "use-same-skills",
    );
    let mut agents = SequenceResolver::new(
        |target: &TargetConsumerParsed| &target.use_agents,
        "use-same-agents",
    );

    let mut resolve = |ai: TargetAi, diags: &mut Vec<Diagnostic>| {
        parsed.get(ai)?;
        Some(TargetConsumer {
            use_skills: skills.effective(ai, parsed, diags),
            use_agents: agents.effective(ai, parsed, diags),
        })
    };

    Consumer {
        claude: resolve(TargetAi::Claude, diags),
        codex: resolve(TargetAi::Codex, diags),
    }
}

/// Resolution state of one target's slot in a cross-target reference walk.
enum SequenceState<T> {
    Pending,
    /// The slot is on the current resolution path; reaching it again means a reference cycle.
    InProgress,
    Done(T),
}

/// Resolves one logical repeatable value (`use-skills` or `use-agents`) across targets.
///
/// `use-same-*` items expand the referenced target's *effective* sequence — itself resolved through this resolver — so references compose, and item order is preserved through expansion (issue #60).
struct SequenceResolver<T, F> {
    get: F,
    /// The `use-same-*` node name for this logical value, used verbatim in diagnostics.
    node_name: &'static str,
    states: [SequenceState<Vec<T>>; 2],
}

fn state_index(ai: TargetAi) -> usize {
    match ai {
        TargetAi::Claude => 0,
        TargetAi::Codex => 1,
    }
}

impl<T, F> SequenceResolver<T, F>
where
    T: Clone,
    F: Fn(&TargetConsumerParsed) -> &Vec<ConsumerItem<T>>,
{
    fn new(get: F, node_name: &'static str) -> Self {
        SequenceResolver {
            get,
            node_name,
            states: [SequenceState::Pending, SequenceState::Pending],
        }
    }

    fn effective(
        &mut self,
        ai: TargetAi,
        parsed: &ConsumerParsed,
        diags: &mut Vec<Diagnostic>,
    ) -> Vec<T> {
        match &self.states[state_index(ai)] {
            SequenceState::Done(values) => return values.clone(),
            // Cycles are detected at the referencing item before recursing, so resolution never re-enters an in-progress sequence.
            SequenceState::InProgress => unreachable!("cycle detected before recursion"),
            SequenceState::Pending => {}
        }
        self.states[state_index(ai)] = SequenceState::InProgress;

        let items = (self.get)(
            parsed
                .get(ai)
                .expect("only declared targets are resolved or referenced"),
        );
        let mut values = Vec::new();
        for item in items {
            match item {
                ConsumerItem::Local(local) => values.extend(local.iter().cloned()),
                ConsumerItem::UseSame(referenced) => {
                    if let Some(expanded) = self.expansion(ai, *referenced, parsed, diags) {
                        values.extend(expanded);
                    }
                }
            }
        }

        self.states[state_index(ai)] = SequenceState::Done(values.clone());
        values
    }

    /// The referenced target's effective sequence, or `None` with a diagnostic when the reference is invalid.
    ///
    /// An existing but empty referenced sequence is a valid no-op expansion, not an error.
    fn expansion(
        &mut self,
        ai: TargetAi,
        referenced: TargetAi,
        parsed: &ConsumerParsed,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<Vec<T>> {
        if parsed.get(referenced).is_none() {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "`consumer.{}` `{}` references `consumer.{}`, but `consumer.{}` is not declared",
                    ai.as_str(),
                    self.node_name,
                    referenced.as_str(),
                    referenced.as_str()
                ),
            ));
            return None;
        }

        if matches!(
            self.states[state_index(referenced)],
            SequenceState::InProgress
        ) {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "`consumer.{}` `{}` reference to `consumer.{}` forms a reference cycle",
                    ai.as_str(),
                    self.node_name,
                    referenced.as_str()
                ),
            ));
            return None;
        }

        Some(self.effective(referenced, parsed, diags))
    }
}

/// Parses one `use-skills` node into `SkillUsage` declarations.
///
/// The blockless form selects one or more Skill names with no usage annotation.
/// The block form (`use-skills "name" { when "..." }`) names exactly one Skill and attaches one usage rule per `when`; grouping several Skills under one block is deliberately unsupported (issue #38).
/// Properties are rejected in both forms, so a mistyped `use-skills skill="a"` cannot silently select nothing.
fn parse_use_skills(node: &KdlNode, target: &str, diags: &mut Vec<Diagnostic>) -> Vec<SkillUsage> {
    if node.entries().iter().any(|e| e.name().is_some()) {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!("`use-skills` under `consumer.{target}` must not declare properties"),
        ));
        return Vec::new();
    }

    let names = string_args(node, "use-skills", diags);

    let Some(children) = node.children() else {
        if names.is_empty() {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "`use-skills` under `consumer.{target}` must select at least one Skill name"
                ),
            ));
        }
        return names
            .into_iter()
            .map(|name| SkillUsage {
                name,
                whens: Vec::new(),
            })
            .collect();
    };

    if names.len() != 1 {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "a `use-skills` block under `consumer.{target}` must name exactly one Skill, but {} are named",
                names.len()
            ),
        ));
        return Vec::new();
    }

    let mut whens = Vec::new();
    let mut ok = true;
    for child in children.nodes() {
        if child.name().value() != "when" {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "unknown node `{}` inside a `use-skills` block under `consumer.{target}`; only `when` is allowed",
                    child.name().value()
                ),
            ));
            ok = false;
            continue;
        }
        match parse_when(child, target, diags) {
            Some(when) => whens.push(when),
            None => ok = false,
        }
    }

    if ok && whens.is_empty() {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!(
                "a `use-skills` block under `consumer.{target}` must declare at least one `when`"
            ),
        ));
        ok = false;
    }

    if !ok {
        return Vec::new();
    }

    vec![SkillUsage {
        name: names
            .into_iter()
            .next()
            .expect("exactly one name was checked above"),
        whens,
    }]
}

/// Validates the `when` contract fixed by issue #38.
///
/// A `when` value is free text inserted verbatim into a generated usage rule, so the contract is purely lexical: exactly one single-line string argument with no surrounding whitespace, and no properties or children.
/// Surrounding whitespace is rejected rather than trimmed, because silently rewriting the value would contradict verbatim insertion.
/// The natural-language content is deliberately not inspected.
fn parse_when(node: &KdlNode, target: &str, diags: &mut Vec<Diagnostic>) -> Option<String> {
    let mut fail = |detail: &str| {
        diags.push(Diagnostic::new(
            DiagnosticCode::ManifestShape,
            format!("`when` under `consumer.{target}` {detail}"),
        ));
        None
    };

    if node.entries().iter().any(|e| e.name().is_some()) {
        return fail("must not declare properties");
    }
    if node.children().is_some() {
        return fail("must not have children");
    }

    let positional: Vec<_> = node
        .entries()
        .iter()
        .filter(|e| e.name().is_none())
        .collect();
    let [entry] = positional.as_slice() else {
        return fail("must have exactly one argument");
    };
    let Some(value) = entry.value().as_string() else {
        return fail("must have a string argument");
    };

    if value.trim().is_empty() {
        return fail("must not be empty");
    }
    if value.contains(['\r', '\n']) {
        return fail("must be a single line without CR or LF");
    }
    if value != value.trim() {
        return fail(
            "must not have leading or trailing whitespace; the value is inserted verbatim and never trimmed",
        );
    }

    Some(value.to_owned())
}

fn validate_references(manifest: &Manifest, diags: &mut Vec<Diagnostic>) {
    // Every target selects from the same provider pool, so each target's references are checked against the same `provider.skills` / `provider.agents`.
    for (ai, consumer) in manifest.consumer.targets() {
        let target = ai.as_str();
        for usage in &consumer.use_skills {
            let name = &usage.name;
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

        // A `when` rule only exists in the generated instruction file, so annotating a selection without the target's base document source would accept syntax the pipeline then ignores.
        if consumer.use_skills.iter().any(|u| !u.whens.is_empty())
            && manifest.provider.instructions.get(ai).is_none()
        {
            diags.push(Diagnostic::new(
                DiagnosticCode::ManifestShape,
                format!(
                    "`consumer.{target}` declares `when` usage rules, but `provider.instructions.{target}` is not declared"
                ),
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

    /// Projects a consumer's Skill selections back to their names, so assertions about selection order stay readable.
    fn skill_names(consumer: &TargetConsumer) -> Vec<&str> {
        consumer
            .use_skills
            .iter()
            .map(|u| u.name.as_str())
            .collect()
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
            skill_names(manifest.consumer.claude.as_ref().unwrap()),
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
        assert_eq!(skill_names(codex), ["git-kura"]);
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
            skill_names(manifest.consumer.claude.as_ref().unwrap()),
            ["git-kura"]
        );
        assert_eq!(
            skill_names(manifest.consumer.codex.as_ref().unwrap()),
            ["git-kura"]
        );
    }

    /// Wraps consumer node bodies in a manifest declaring skills `a` / `b` and agents `x` / `y`, so tests compare grouped and split selection forms against one provider pool.
    fn consumer_claude(body: &str) -> String {
        format!(
            r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "a" {{ git {{ url "https://example.com/r"; branch "main"; path "s/a" }} }}
      skill "b" {{ git {{ url "https://example.com/r"; branch "main"; path "s/b" }} }}
    }}
    agents {{
      agent "x" {{ git {{ url "https://example.com/r"; branch "main"; path "a/x.md" }} }}
      agent "y" {{ git {{ url "https://example.com/r"; branch "main"; path "a/y.md" }} }}
    }}
  }}
  consumer {{
    claude {{
{body}
    }}
  }}
}}
"#
        )
    }

    #[test]
    fn parses_split_use_skills_nodes_identically_to_the_grouped_form() {
        let grouped = parse(&consumer_claude(r#"      use-skills "a" "b""#)).unwrap();
        let split = parse(&consumer_claude(
            r#"      use-skills "a"
      use-skills "b""#,
        ))
        .unwrap();
        assert_eq!(
            skill_names(grouped.consumer.claude.as_ref().unwrap()),
            ["a", "b"]
        );
        assert_eq!(grouped, split);
    }

    #[test]
    fn normalizes_blockless_selections_to_skill_usages_without_whens() {
        // The blockless form carries no usage annotations, so each name normalizes to a `SkillUsage` with empty `whens`.
        let manifest = parse(&consumer_claude(r#"      use-skills "a" "b""#)).unwrap();
        let claude = manifest.consumer.claude.as_ref().unwrap();
        assert_eq!(claude.use_skills.len(), 2);
        assert!(claude.use_skills.iter().all(|u| u.whens.is_empty()));
    }

    #[test]
    fn parses_split_use_agents_nodes_identically_to_the_grouped_form() {
        let grouped = parse(&consumer_claude(r#"      use-agents "x" "y""#)).unwrap();
        let split = parse(&consumer_claude(
            r#"      use-agents "x"
      use-agents "y""#,
        ))
        .unwrap();
        assert_eq!(
            grouped.consumer.claude.as_ref().unwrap().use_agents,
            ["x", "y"]
        );
        assert_eq!(grouped, split);
    }

    #[test]
    fn keeps_declaration_order_across_mixed_grouped_and_split_nodes() {
        let manifest = parse(&consumer_claude(
            r#"      use-skills "b"
      use-agents "y"
      use-skills "a"
      use-agents "x""#,
        ))
        .unwrap();
        let claude = manifest.consumer.claude.as_ref().unwrap();
        assert_eq!(skill_names(claude), ["b", "a"]);
        assert_eq!(claude.use_agents, ["y", "x"]);
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

    /// Wraps `provider.instructions` children and consumer children in a manifest declaring skill `a`, so tests exercise instruction and `when` parsing together.
    fn manifest_with_instructions(instructions_body: &str, claude_body: &str) -> String {
        format!(
            r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "a" {{ git {{ url "https://example.com/r"; branch "main"; path "s/a" }} }}
    }}
    instructions {{
{instructions_body}
    }}
  }}
  consumer {{
    claude {{
{claude_body}
    }}
  }}
}}
"#
        )
    }

    const CLAUDE_LOCAL_INSTRUCTION: &str = r#"      claude { local { path "CLAUDE.base.md" } }"#;

    #[test]
    fn parses_instruction_sources_per_target() {
        let text = r#"
enozunu config-version=1 {
  provider {
    instructions {
      claude { git { url "https://example.com/r"; branch "main"; path "instructions/CLAUDE.base.md" } }
      codex { local { path "AGENTS.base.md" } }
    }
  }
  consumer { claude {} }
}
"#;
        let manifest = parse(text).unwrap();
        assert_eq!(
            manifest.provider.instructions.claude,
            Some(SourceReference::Git {
                url: "https://example.com/r".to_owned(),
                selector: GitSelector::Branch("main".to_owned()),
                path: "instructions/CLAUDE.base.md".to_owned(),
            })
        );
        assert_eq!(
            manifest.provider.instructions.codex,
            Some(SourceReference::Local {
                path: "AGENTS.base.md".to_owned(),
            })
        );
    }

    #[test]
    fn parses_an_instruction_gist_with_the_file_shaped_contract() {
        let text = manifest_with_instructions(
            r#"      claude {
        gist {
          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "CLAUDE.base.md"
        }
      }"#,
            r#"      use-skills "a""#,
        );
        let manifest = parse(&text).unwrap();
        let Some(SourceReference::Gist { selector, .. }) = &manifest.provider.instructions.claude
        else {
            panic!("expected a gist instruction source");
        };
        assert_eq!(
            selector,
            &GistArtifactSelector::File {
                path: "CLAUDE.base.md".to_owned()
            }
        );
    }

    #[test]
    fn reports_an_instruction_gist_missing_file_as_instruction_not_agent() {
        // The file-shaped contract is shared with agents, but the report must name the actual kind.
        let text = manifest_with_instructions(
            r#"      claude {
        gist {
          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
        }
      }"#,
            r#"      use-skills "a""#,
        );
        let messages = messages(parse(&text));
        assert!(
            messages
                .iter()
                .any(|m| m.contains("instruction `claude`") && m.contains("`file`")),
            "missing `file` must be reported for `instruction `claude``: {messages:?}"
        );
        assert!(
            messages.iter().all(|m| !m.contains("agent `")),
            "an instruction failure must not be reported as an agent's: {messages:?}"
        );
    }

    #[test]
    fn rejects_a_duplicate_instructions_block() {
        let text = r#"
enozunu config-version=1 {
  provider {
    instructions { claude { local { path "a.md" } } }
    instructions { codex { local { path "b.md" } } }
  }
  consumer { claude {} }
}
"#;
        assert!(codes(parse(text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_a_duplicate_instruction_target() {
        let text = manifest_with_instructions(
            r#"      claude { local { path "a.md" } }
      claude { local { path "b.md" } }"#,
            "",
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_an_unknown_instruction_target() {
        let text = manifest_with_instructions(r#"      gemini { local { path "a.md" } }"#, "");
        assert!(codes(parse(&text)).contains(&DiagnosticCode::UnsupportedConsumer));
    }

    #[test]
    fn rejects_an_instruction_target_without_a_source_reference() {
        let text = manifest_with_instructions(r#"      claude {}"#, "");
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_arguments_on_instruction_nodes() {
        for body in [r#"      claude "x" { local { path "a.md" } }"#] {
            let text = manifest_with_instructions(body, "");
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::ManifestShape),
                "instruction node argument must be rejected: {body}"
            );
        }
    }

    #[test]
    fn rejects_the_removed_instruction_same_as_alias_with_a_migration_hint() {
        // The migration wording is pinned by e2e/validate_use_same.repor; every `same-as` variant on an instruction target yields a configuration error and resolves to no source.
        for codex_body in [
            r#"      codex same-as="provider.instructions.claude""#,
            r#"      codex same-as="provider.instructions.claude" {
        local { path "AGENTS.base.md" }
      }"#,
            r#"      codex "x" same-as="provider.instructions.claude""#,
            r#"      codex same-as=1"#,
        ] {
            let text = manifest_with_instructions(
                &format!("      claude {{ local {{ path \"CLAUDE.base.md\" }} }}\n{codex_body}"),
                "",
            );
            let codes = codes(parse(&text));
            assert!(
                codes.contains(&DiagnosticCode::ManifestShape),
                "`{codex_body}` must be rejected: {codes:?}"
            );
        }
    }

    #[test]
    fn resolves_use_same_instruction_to_the_referenced_source() {
        let text = manifest_with_instructions(
            r#"      claude { git { url "https://example.com/r"; branch "main"; path "instructions/base.md" } }
      codex {
        use-same-instruction "claude"
      }"#,
            "",
        );
        let manifest = parse(&text).unwrap();
        // The referencing target reuses the referenced declaration's source reference itself, not a copy re-derived from its own context.
        assert_eq!(
            manifest.provider.instructions.get(TargetAi::Codex),
            manifest.provider.instructions.get(TargetAi::Claude)
        );
        assert!(
            manifest
                .provider
                .instructions
                .get(TargetAi::Codex)
                .is_some()
        );
    }

    #[test]
    fn resolves_use_same_instruction_declared_before_its_referent() {
        let text = manifest_with_instructions(
            r#"      claude {
        use-same-instruction "codex"
      }
      codex { local { path "AGENTS.base.md" } }"#,
            "",
        );
        let manifest = parse(&text).unwrap();
        assert_eq!(
            manifest.provider.instructions.get(TargetAi::Claude),
            Some(&SourceReference::Local {
                path: "AGENTS.base.md".to_owned()
            })
        );
    }

    #[test]
    fn rejects_use_same_instruction_referencing_an_undeclared_target() {
        // Diagnostic wording is pinned by e2e/validate_use_same.repor; unit tests assert the code only.
        let text = manifest_with_instructions(
            r#"      codex {
        use-same-instruction "claude"
      }"#,
            "",
        );
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_a_self_referential_use_same_instruction() {
        let text = manifest_with_instructions(
            r#"      claude {
        use-same-instruction "claude"
      }"#,
            "",
        );
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_a_use_same_instruction_reference_cycle() {
        let text = manifest_with_instructions(
            r#"      claude {
        use-same-instruction "codex"
      }
      codex {
        use-same-instruction "claude"
      }"#,
            "",
        );
        // Memoization reports the cycle once, so the manifest yields exactly one diagnostic.
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_multiple_use_same_instruction_nodes() {
        let text = manifest_with_instructions(
            r#"      claude { local { path "CLAUDE.base.md" } }
      codex {
        use-same-instruction "claude"
        use-same-instruction "claude"
      }"#,
            "",
        );
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_combining_use_same_instruction_with_a_source_reference_block() {
        let text = manifest_with_instructions(
            r#"      claude { local { path "CLAUDE.base.md" } }
      codex {
        use-same-instruction "claude"
        local { path "AGENTS.base.md" }
      }"#,
            "",
        );
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_use_same_instruction_shape_violations() {
        for body in [
            r#"      codex {
        use-same-instruction
      }"#,
            r#"      codex {
        use-same-instruction "claude" "claude"
      }"#,
            r#"      codex {
        use-same-instruction 5
      }"#,
            r#"      codex {
        use-same-instruction target="claude"
      }"#,
            r#"      codex {
        use-same-instruction "claude" {
          local { path "AGENTS.base.md" }
        }
      }"#,
            r#"      codex {
        use-same-instruction "gemini"
      }"#,
        ] {
            let text = manifest_with_instructions(
                &format!("      claude {{ local {{ path \"CLAUDE.base.md\" }} }}\n{body}"),
                "",
            );
            let codes = codes(parse(&text));
            assert!(
                codes.contains(&DiagnosticCode::ManifestShape),
                "`{body}` must be rejected: {codes:?}"
            );
        }
    }

    #[test]
    fn a_use_same_instruction_to_a_malformed_referent_adds_no_second_error() {
        // The referenced declaration's own parse failure is the only diagnostic; the reference resolves to no source without a misleading "not declared" report.
        let text = manifest_with_instructions(
            r#"      claude { bogus {} }
      codex {
        use-same-instruction "claude"
      }"#,
            "",
        );
        let diags = parse(&text).unwrap_err();
        assert!(
            diags
                .iter()
                .all(|d| !d.message.contains("use-same-instruction")),
            "{diags:?}"
        );
    }

    /// Wraps `consumer.claude` and `consumer.codex` bodies in a manifest declaring skills `a` / `b`, agent `x`, and both instruction sources, so `use-same-*` tests exercise both targets against one provider pool without tripping reference or `when` validation.
    fn consumer_targets(claude_body: &str, codex_body: &str) -> String {
        format!(
            r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "a" {{ git {{ url "https://example.com/r"; branch "main"; path "s/a" }} }}
      skill "b" {{ git {{ url "https://example.com/r"; branch "main"; path "s/b" }} }}
    }}
    agents {{
      agent "x" {{ git {{ url "https://example.com/r"; branch "main"; path "a/x.md" }} }}
    }}
    instructions {{
      claude {{ local {{ path "CLAUDE.base.md" }} }}
      codex {{ local {{ path "AGENTS.base.md" }} }}
    }}
  }}
  consumer {{
    claude {{
{claude_body}
    }}
    codex {{
{codex_body}
    }}
  }}
}}
"#
        )
    }

    #[test]
    fn rejects_the_removed_use_skills_same_as_alias_with_a_migration_hint() {
        // The migration wording is pinned by e2e/validate_use_same.repor; this asserts the removed alias yields exactly one configuration error and nothing resolves from it.
        let text = consumer_targets(
            r#"      use-skills "a""#,
            r#"      use-skills same-as="consumer.claude.use-skills""#,
        );
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn expands_use_same_skills_at_its_declaration_position() {
        let text = consumer_targets(
            r#"      use-skills "a" "b""#,
            r#"      use-skills "b"
      use-same-skills "claude"
      use-skills "a""#,
        );
        let manifest = parse(&text).unwrap();
        let codex = manifest.consumer.codex.as_ref().unwrap();
        // The expansion replaces the `use-same-skills` node in place, so the referenced sequence lands between the local selections.
        assert_eq!(skill_names(codex), ["b", "a", "b", "a"]);
    }

    #[test]
    fn expands_use_same_skills_declared_before_its_referent() {
        // The claude block references the codex block declared after it, so resolution must be independent of sibling order.
        let text = consumer_targets(
            r#"      use-same-skills "codex""#,
            r#"      use-skills "a""#,
        );
        let manifest = parse(&text).unwrap();
        assert_eq!(
            skill_names(manifest.consumer.claude.as_ref().unwrap()),
            ["a"]
        );
    }

    #[test]
    fn expands_repeated_use_same_skills_nodes_keeping_duplicates() {
        let text = consumer_targets(
            r#"      use-skills "a""#,
            r#"      use-same-skills "claude"
      use-skills "b"
      use-same-skills "claude""#,
        );
        let manifest = parse(&text).unwrap();
        let codex = manifest.consumer.codex.as_ref().unwrap();
        assert_eq!(skill_names(codex), ["a", "b", "a"]);
    }

    #[test]
    fn expands_use_same_skills_preserving_when_rules_per_declaration() {
        let text = consumer_targets(
            r#"      use-skills "a" {
        when "before completion"
      }"#,
            r#"      use-same-skills "claude"
      use-skills "a" {
        when "after implementation"
      }"#,
        );
        let manifest = parse(&text).unwrap();
        let codex = manifest.consumer.codex.as_ref().unwrap();
        // Both declarations of `a` stay separate, each with its own `when`; nothing folds them into one usage.
        assert_eq!(
            codex.use_skills,
            [
                SkillUsage {
                    name: "a".to_owned(),
                    whens: vec!["before completion".to_owned()],
                },
                SkillUsage {
                    name: "a".to_owned(),
                    whens: vec!["after implementation".to_owned()],
                },
            ]
        );
    }

    #[test]
    fn expands_an_empty_referenced_use_skills_as_a_no_op() {
        let text = consumer_targets(
            "",
            r#"      use-same-skills "claude"
      use-skills "a""#,
        );
        let manifest = parse(&text).unwrap();
        assert_eq!(
            skill_names(manifest.consumer.codex.as_ref().unwrap()),
            ["a"]
        );
    }

    #[test]
    fn rejects_use_same_skills_referencing_an_undeclared_consumer() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
    }
  }
  consumer {
    codex {
      use-same-skills "claude"
    }
  }
}
"#;
        // Unit tests assert only the diagnostic code; the user-visible wording of each `use-same-*` diagnostic is pinned by e2e/validate_use_same.repor.
        assert_eq!(codes(parse(text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_a_self_referential_use_same_skills() {
        let text = consumer_targets(r#"      use-same-skills "claude""#, "");
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_a_use_same_skills_reference_cycle() {
        let text = consumer_targets(
            r#"      use-same-skills "codex""#,
            r#"      use-same-skills "claude""#,
        );
        // Memoization reports the cycle once, so the manifest yields exactly one diagnostic.
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_use_same_skills_shape_violations() {
        for body in [
            r#"      use-same-skills"#,
            r#"      use-same-skills "claude" "claude""#,
            r#"      use-same-skills 5"#,
            r#"      use-same-skills target="claude""#,
            r#"      use-same-skills "claude" {
        when "reviewing code"
      }"#,
            r#"      use-same-skills "gemini""#,
        ] {
            let text = consumer_targets(r#"      use-skills "a""#, body);
            let codes = codes(parse(&text));
            assert!(
                codes.contains(&DiagnosticCode::ManifestShape),
                "`{body}` must be rejected: {codes:?}"
            );
        }
    }

    #[test]
    fn validates_provider_references_of_expanded_use_same_skills() {
        // Claude references a Skill missing from the provider pool; the expansion must fail codex's reference validation too, not just claude's.
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
    }
  }
  consumer {
    claude {
      use-skills "missing"
    }
    codex {
      use-same-skills "claude"
    }
  }
}
"#;
        let diags = parse(text).unwrap_err();
        let unknown: Vec<_> = diags
            .iter()
            .filter(|d| d.code == DiagnosticCode::UnknownSourceReference)
            .collect();
        assert_eq!(unknown.len(), 2, "{diags:?}");
    }

    #[test]
    fn use_same_skills_carries_when_rules_into_the_when_instruction_requirement() {
        // Claude's selection carries a `when`, so the codex expansion inherits it and codex must declare its own instruction source.
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
    }
    instructions {
      claude { local { path "CLAUDE.base.md" } }
    }
  }
  consumer {
    claude {
      use-skills "a" {
        when "reviewing code"
      }
    }
    codex {
      use-same-skills "claude"
    }
  }
}
"#;
        let messages = messages(parse(text));
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`provider.instructions.codex` is not declared")),
            "{messages:?}"
        );
    }

    #[test]
    fn expands_use_same_agents_at_its_declaration_position() {
        let text = consumer_targets(
            r#"      use-agents "x""#,
            r#"      use-agents "x"
      use-same-agents "claude"
      use-agents "x""#,
        );
        let manifest = parse(&text).unwrap();
        let codex = manifest.consumer.codex.as_ref().unwrap();
        assert_eq!(codex.use_agents, ["x", "x", "x"]);
    }

    #[test]
    fn expands_use_same_agents_independently_of_use_same_skills() {
        // Skills and agents are separate reference graphs: codex reuses claude's agents while declaring its own skills, and neither leaks into the other.
        let text = consumer_targets(
            r#"      use-skills "a"
      use-agents "x""#,
            r#"      use-same-agents "claude"
      use-skills "b""#,
        );
        let manifest = parse(&text).unwrap();
        let codex = manifest.consumer.codex.as_ref().unwrap();
        assert_eq!(skill_names(codex), ["b"]);
        assert_eq!(codex.use_agents, ["x"]);
    }

    #[test]
    fn rejects_use_same_agents_referencing_an_undeclared_consumer() {
        let text = r#"
enozunu config-version=1 {
  provider {
    agents {
      agent "x" { git { url "https://example.com/r"; branch "main"; path "a/x.md" } }
    }
  }
  consumer {
    codex {
      use-same-agents "claude"
    }
  }
}
"#;
        assert_eq!(codes(parse(text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_a_self_referential_use_same_agents() {
        let text = consumer_targets(r#"      use-same-agents "claude""#, "");
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn rejects_a_use_same_agents_reference_cycle() {
        let text = consumer_targets(
            r#"      use-same-agents "codex""#,
            r#"      use-same-agents "claude""#,
        );
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn a_use_same_skills_cycle_does_not_reject_use_same_agents() {
        // Cycle detection runs per logical value: the skills cycle is the single diagnostic, while claude's valid agents expansion resolves without one.
        let text = consumer_targets(
            r#"      use-same-skills "codex"
      use-same-agents "codex""#,
            r#"      use-same-skills "claude"
      use-agents "x""#,
        );
        assert_eq!(codes(parse(&text)), [DiagnosticCode::ManifestShape]);
    }

    #[test]
    fn parses_a_use_skills_block_with_multiple_whens_in_order() {
        let text = manifest_with_instructions(
            CLAUDE_LOCAL_INSTRUCTION,
            r#"      use-skills "a" {
        when "updating documentation"
        when "writing or modifying code comments"
      }"#,
        );
        let manifest = parse(&text).unwrap();
        let claude = manifest.consumer.claude.as_ref().unwrap();
        assert_eq!(
            claude.use_skills,
            [SkillUsage {
                name: "a".to_owned(),
                whens: vec![
                    "updating documentation".to_owned(),
                    "writing or modifying code comments".to_owned(),
                ],
            }]
        );
    }

    #[test]
    fn rejects_a_use_skills_block_naming_zero_or_multiple_skills() {
        for selection in [
            r#"      use-skills { when "x" }"#,
            r#"      use-skills "a" "a2" { when "x" }"#,
        ] {
            let text = manifest_with_instructions(CLAUDE_LOCAL_INSTRUCTION, selection);
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::ManifestShape),
                "block form with wrong name count must be rejected: {selection}"
            );
        }
    }

    #[test]
    fn rejects_a_use_skills_block_without_a_when() {
        let text =
            manifest_with_instructions(CLAUDE_LOCAL_INSTRUCTION, r#"      use-skills "a" {}"#);
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_an_unknown_node_inside_a_use_skills_block() {
        let text = manifest_with_instructions(
            CLAUDE_LOCAL_INSTRUCTION,
            r#"      use-skills "a" { condition "x" }"#,
        );
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_use_skills_properties_in_both_forms() {
        for selection in [
            r#"      use-skills skill="a""#,
            r#"      use-skills "a" priority="high" { when "x" }"#,
        ] {
            let text = manifest_with_instructions(CLAUDE_LOCAL_INSTRUCTION, selection);
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::ManifestShape),
                "use-skills property must be rejected: {selection}"
            );
        }
    }

    #[test]
    fn rejects_a_blockless_use_skills_without_names() {
        let text = manifest_with_instructions(CLAUDE_LOCAL_INSTRUCTION, r#"      use-skills"#);
        assert!(codes(parse(&text)).contains(&DiagnosticCode::ManifestShape));
    }

    #[test]
    fn rejects_when_contract_violations() {
        for when in [
            r#"when"#,                    // no argument
            r#"when "x" "y""#,            // two arguments
            r#"when 1"#,                  // non-string
            r#"when """#,                 // empty
            r#"when "   ""#,              // whitespace-only
            r#"when " padded""#,          // leading whitespace
            r#"when "padded ""#,          // trailing whitespace
            r#"when "multi\nline""#,      // LF
            r#"when "carriage\rreturn""#, // CR
            r#"when "x" lang="en""#,      // property
            r#"when "x" { nested "y" }"#, // children
        ] {
            let text = manifest_with_instructions(
                CLAUDE_LOCAL_INSTRUCTION,
                &format!(
                    r#"      use-skills "a" {{
        {when}
      }}"#
                ),
            );
            assert!(
                codes(parse(&text)).contains(&DiagnosticCode::ManifestShape),
                "`{when}` must be rejected"
            );
        }
    }

    #[test]
    fn rejects_when_rules_without_a_declared_instruction_source() {
        let text = manifest_with_instructions(
            "",
            r#"      use-skills "a" {
        when "updating documentation"
      }"#,
        );
        let messages = messages(parse(&text));
        assert!(
            messages
                .iter()
                .any(|m| m.contains("`provider.instructions.claude` is not declared")),
            "{messages:?}"
        );
    }

    #[test]
    fn accepts_instruction_sources_without_a_consumer() {
        // A declared source without a consumer stays unused, like an unselected Skill.
        let text = r#"
enozunu config-version=1 {
  provider {
    instructions { codex { local { path "AGENTS.base.md" } } }
  }
  consumer { claude {} }
}
"#;
        assert!(parse(text).is_ok());
    }

    #[test]
    fn keeps_declaration_order_across_blockless_and_block_forms() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
      skill "b" { git { url "https://example.com/r"; branch "main"; path "s/b" } }
      skill "c" { git { url "https://example.com/r"; branch "main"; path "s/c" } }
    }
    instructions { claude { local { path "CLAUDE.base.md" } } }
  }
  consumer {
    claude {
      use-skills "a"
      use-skills "b" { when "reviewing" }
      use-skills "c"
    }
  }
}
"#;
        let manifest = parse(text).unwrap();
        let claude = manifest.consumer.claude.as_ref().unwrap();
        assert_eq!(skill_names(claude), ["a", "b", "c"]);
        assert_eq!(claude.use_skills[1].whens, ["reviewing"]);
        assert!(claude.use_skills[0].whens.is_empty());
        assert!(claude.use_skills[2].whens.is_empty());
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
