//! Builds the materialization plan from a validated manifest.
//!
//! Planning decides what would be written where; it does not resolve sources or touch the filesystem.

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::manifest::{Manifest, SourceReference, TargetAi, TargetConsumer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Skill,
    Agent,
    /// A generated root repository instruction file (`CLAUDE.md` / `AGENTS.md`), composed from a base document source and the target's Skill usage rules instead of copied verbatim.
    Instruction,
}

impl ArtifactKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactKind::Skill => "skill",
            ArtifactKind::Agent => "agent",
            ArtifactKind::Instruction => "instruction",
        }
    }

    /// Whether this artifact is one regular file rather than a directory tree.
    ///
    /// Shape, not kind, drives the shared source checks, so a file-shaped kind reuses one code path and its diagnostics name the actual kind.
    /// The match is exhaustive on purpose: adding a kind must force an explicit shape decision here, because a silently defaulted shape would send the new kind down the directory branch with another kind's wording.
    pub fn is_file_shaped(&self) -> bool {
        match self {
            ArtifactKind::Skill => false,
            ArtifactKind::Agent | ArtifactKind::Instruction => true,
        }
    }
}

/// One selected source and the project-relative target-AI-native path it materializes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMaterialization {
    pub source_name: String,
    pub kind: ArtifactKind,
    pub reference: SourceReference,
    pub target_ai: TargetAi,
    pub target_rel_path: String,
}

/// The project-relative native path a target AI reads an artifact of `kind` named `name` from.
///
/// Enozunu projects one source into each target's native layout without converting its format: Claude reads a Markdown agent under `.claude/agents/`, Codex reads a TOML agent under `.codex/agents/`, and both read a Skill directory (differing only in location).
/// The `.md` / `.toml` suffix here fixes the target filename; it is not required of, or matched against, the source path.
fn target_rel_path(target: TargetAi, kind: ArtifactKind, name: &str) -> String {
    match (target, kind) {
        (TargetAi::Claude, ArtifactKind::Skill) => format!(".claude/skills/{name}"),
        (TargetAi::Claude, ArtifactKind::Agent) => format!(".claude/agents/{name}.md"),
        (TargetAi::Codex, ArtifactKind::Skill) => format!(".agents/skills/{name}"),
        (TargetAi::Codex, ArtifactKind::Agent) => format!(".codex/agents/{name}.toml"),
        // The root instruction file is one fixed path per target; the declaration has no user-defined name.
        (TargetAi::Claude, ArtifactKind::Instruction) => "CLAUDE.md".to_owned(),
        (TargetAi::Codex, ArtifactKind::Instruction) => "AGENTS.md".to_owned(),
    }
}

/// Plans materializations for every source selected by every declared consumer target.
///
/// Selecting the same source more than once in one target is a valid consumer declaration (issue #60): the duplicate declarations stay in the manifest model, and the plan normalizes them to one write because every selection of a source resolves to the identical artifact. This normalization is plan-level only; it does not deduplicate `SkillUsage` declarations, whose `when` rules all reach the generated instruction.
///
/// Fails when two *different* materializations resolve to the same target path, because later writes would silently overwrite earlier ones. The same source selected by both Claude and Codex resolves to distinct native paths, so it is not a collision.
pub fn plan(manifest: &Manifest) -> Result<Vec<PlannedMaterialization>, Vec<Diagnostic>> {
    let mut selected = Vec::new();

    for (target, consumer) in manifest.consumer.targets() {
        plan_target(manifest, target, consumer, &mut selected);
    }

    normalize_writes(selected)
}

/// Collapses repeated selections of one artifact into one write, and rejects distinct artifacts sharing a target path.
///
/// Entries are compared whole: a repeat is only dropped when every field matches the first entry on that path, so two different sources can never silently merge.
fn normalize_writes(
    selected: Vec<PlannedMaterialization>,
) -> Result<Vec<PlannedMaterialization>, Vec<Diagnostic>> {
    let mut planned: Vec<PlannedMaterialization> = Vec::new();
    // Maps each target path to its first entry's index in `planned`, so repeats compare against the first occurrence without rescanning it.
    let mut first_by_path: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut diags = Vec::new();
    for entry in selected {
        match first_by_path.get(&entry.target_rel_path) {
            None => {
                first_by_path.insert(entry.target_rel_path.clone(), planned.len());
                planned.push(entry);
            }
            Some(&first) if planned[first] == entry => {}
            Some(_) => diags.push(Diagnostic::new(
                DiagnosticCode::DuplicateTargetPath,
                format!(
                    "multiple materializations resolve to the same target path `{}`",
                    entry.target_rel_path
                ),
            )),
        }
    }

    if diags.is_empty() {
        Ok(planned)
    } else {
        Err(diags)
    }
}

/// Appends the Skill and agent materializations one target selects, in selection order.
fn plan_target(
    manifest: &Manifest,
    target: TargetAi,
    consumer: &TargetConsumer,
    planned: &mut Vec<PlannedMaterialization>,
) {
    for usage in &consumer.use_skills {
        // Reference existence is validated at parse time, so a missing lookup here is a programming error.
        let decl = manifest
            .provider
            .skills
            .iter()
            .find(|s| s.name == usage.name)
            .expect("validated manifest references a declared skill");
        planned.push(PlannedMaterialization {
            source_name: decl.name.clone(),
            kind: ArtifactKind::Skill,
            reference: decl.reference.clone(),
            target_ai: target,
            target_rel_path: target_rel_path(target, ArtifactKind::Skill, &decl.name),
        });
    }

    for name in &consumer.use_agents {
        let decl = manifest
            .provider
            .agents
            .iter()
            .find(|s| &s.name == name)
            .expect("validated manifest references a declared agent");
        planned.push(PlannedMaterialization {
            source_name: decl.name.clone(),
            kind: ArtifactKind::Agent,
            reference: decl.reference.clone(),
            target_ai: target,
            target_rel_path: target_rel_path(target, ArtifactKind::Agent, &decl.name),
        });
    }

    // An instruction materializes only when both sides opted in: the target consumer exists (this function runs per declared target) and its base document source is declared. A source without a consumer stays unresolved, like an unselected Skill.
    if let Some(reference) = manifest.provider.instructions.get(target) {
        planned.push(PlannedMaterialization {
            // The declaration has no user-defined name, so the provider child node name is the source identity, in diagnostics and provenance alike.
            source_name: target.as_str().to_owned(),
            kind: ArtifactKind::Instruction,
            reference: reference.clone(),
            target_ai: target,
            target_rel_path: target_rel_path(target, ArtifactKind::Instruction, target.as_str()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitSelector;
    use crate::manifest;

    #[test]
    fn plans_selected_sources_only() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "used" { git { url "https://example.com/r"; branch "main"; path "s/used" } }
      skill "unused" { git { url "https://example.com/r"; branch "main"; path "s/unused" } }
      skill "local-used" { local { path "../sibling/s/local-used" } }
    }
    agents {
      agent "helper" { git { url "https://example.com/r"; branch "main"; path "a/helper.md" } }
    }
  }
  consumer {
    claude {
      use-skills "used" "local-used"
      use-agents "helper"
    }
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        assert_eq!(planned.len(), 3);
        assert_eq!(planned[0].target_rel_path, ".claude/skills/used");
        assert_eq!(planned[1].target_rel_path, ".claude/skills/local-used");
        assert_eq!(planned[2].target_rel_path, ".claude/agents/helper.md");
        assert_eq!(
            planned[1].reference,
            SourceReference::Local {
                path: "../sibling/s/local-used".to_owned()
            }
        );
    }

    #[test]
    fn plans_a_skill_selected_twice_in_one_node_as_one_write() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
    }
  }
  consumer {
    claude {
      use-skills "a" "a"
    }
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].target_rel_path, ".claude/skills/a");
    }

    /// Wraps consumer selection nodes in a manifest declaring skills `a` / `b` and agents `x` / `y`, so tests compare grouped and split selection forms against one provider pool.
    fn claude_selecting(selection: &str) -> String {
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
{selection}
    }}
  }}
}}
"#
        )
    }

    #[test]
    fn plans_split_use_skills_nodes_identically_to_the_grouped_form() {
        let grouped =
            plan(&manifest::parse(&claude_selecting(r#"      use-skills "a" "b""#)).unwrap())
                .unwrap();
        let split = plan(
            &manifest::parse(&claude_selecting(
                r#"      use-skills "a"
      use-skills "b""#,
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(grouped, split);
        assert_eq!(
            grouped
                .iter()
                .map(|e| e.target_rel_path.as_str())
                .collect::<Vec<_>>(),
            [".claude/skills/a", ".claude/skills/b"]
        );
    }

    #[test]
    fn plans_split_use_agents_nodes_identically_to_the_grouped_form() {
        let grouped =
            plan(&manifest::parse(&claude_selecting(r#"      use-agents "x" "y""#)).unwrap())
                .unwrap();
        let split = plan(
            &manifest::parse(&claude_selecting(
                r#"      use-agents "x"
      use-agents "y""#,
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(grouped, split);
        assert_eq!(
            grouped
                .iter()
                .map(|e| e.target_rel_path.as_str())
                .collect::<Vec<_>>(),
            [".claude/agents/x.md", ".claude/agents/y.md"]
        );
    }

    #[test]
    fn plans_a_skill_selected_across_split_nodes_as_one_write() {
        // The same name repeated across split nodes normalizes exactly like `use-skills "a" "a"` inside one node.
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
    }
  }
  consumer {
    claude {
      use-skills "a"
      use-skills "a"
    }
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].target_rel_path, ".claude/skills/a");
    }

    #[test]
    fn plans_codex_skill_and_agent_into_codex_native_paths() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "demo" { git { url "https://example.com/r"; branch "main"; path "s/demo" } }
    }
    agents {
      agent "reviewer" { git { url "https://example.com/r"; branch "main"; path "a/reviewer.toml" } }
    }
  }
  consumer {
    codex {
      use-skills "demo"
      use-agents "reviewer"
    }
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[0].target_ai, TargetAi::Codex);
        assert_eq!(planned[0].target_rel_path, ".agents/skills/demo");
        assert_eq!(planned[1].target_ai, TargetAi::Codex);
        assert_eq!(planned[1].target_rel_path, ".codex/agents/reviewer.toml");
    }

    #[test]
    fn plans_the_same_skill_for_both_targets_without_a_collision() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "demo" { git { url "https://example.com/r"; branch "main"; path "s/demo" } }
    }
  }
  consumer {
    claude { use-skills "demo" }
    codex { use-skills "demo" }
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[0].target_ai, TargetAi::Claude);
        assert_eq!(planned[0].target_rel_path, ".claude/skills/demo");
        assert_eq!(planned[1].target_ai, TargetAi::Codex);
        assert_eq!(planned[1].target_rel_path, ".agents/skills/demo");
    }

    #[test]
    fn plans_instructions_for_declared_targets_only() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
    }
    instructions {
      claude { local { path "CLAUDE.base.md" } }
      codex { local { path "AGENTS.base.md" } }
    }
  }
  consumer {
    claude {
      use-skills "a"
    }
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        // Only Claude declared a consumer, so only the Claude instruction is planned; the codex source stays unused.
        assert_eq!(
            planned
                .iter()
                .map(|e| e.target_rel_path.as_str())
                .collect::<Vec<_>>(),
            [".claude/skills/a", "CLAUDE.md"]
        );
        let instruction = &planned[1];
        assert_eq!(instruction.kind, ArtifactKind::Instruction);
        assert_eq!(instruction.source_name, "claude");
        assert_eq!(instruction.target_ai, TargetAi::Claude);
    }

    #[test]
    fn plans_the_codex_instruction_to_agents_md() {
        let text = r#"
enozunu config-version=1 {
  provider {
    instructions {
      codex { local { path "AGENTS.base.md" } }
    }
  }
  consumer {
    codex {}
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].target_rel_path, "AGENTS.md");
        assert_eq!(planned[0].source_name, "codex");
    }

    #[test]
    fn a_use_same_instruction_reference_keeps_the_referencing_targets_own_identity() {
        // Codex reuses Claude's instruction source with `use-same-instruction`, so its planned instruction must carry Claude's source reference but keep Codex's own path, name, and target AI.
        let text = r#"
enozunu config-version=1 {
  provider {
    instructions {
      claude {
        git {
          url "https://example.com/r"
          branch "main"
          path "instructions/base.md"
        }
      }
      codex {
        use-same-instruction "claude"
      }
    }
  }
  consumer {
    claude {}
    codex {}
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        let claude = planned
            .iter()
            .find(|e| e.kind == ArtifactKind::Instruction && e.target_ai == TargetAi::Claude)
            .expect("a Claude instruction is planned");
        let codex = planned
            .iter()
            .find(|e| e.kind == ArtifactKind::Instruction && e.target_ai == TargetAi::Codex)
            .expect("a Codex instruction is planned");

        // The reused source reference is shared, so both targets resolve the same commit and dedupe.
        assert_eq!(codex.reference, claude.reference);
        assert_eq!(
            codex.reference,
            SourceReference::Git {
                url: "https://example.com/r".to_owned(),
                selector: GitSelector::Branch("main".to_owned()),
                path: "instructions/base.md".to_owned(),
            }
        );
        // The referencing target keeps its own identity: Codex writes AGENTS.md under its own source name, not Claude's.
        assert_eq!(codex.source_name, "codex");
        assert_eq!(codex.target_rel_path, "AGENTS.md");
    }

    #[test]
    fn plans_no_instruction_without_a_declared_source() {
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
    }
  }
  consumer {
    claude { use-skills "a" }
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        assert!(planned.iter().all(|e| e.kind != ArtifactKind::Instruction));
    }

    #[test]
    fn plans_a_duplicate_selection_once_per_target_and_keeps_targets_distinct() {
        // Duplicate selection normalizes per target: Codex's repeat collapses to one write, while Claude's own write stays separate.
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
    }
  }
  consumer {
    claude {
      use-skills "a"
    }
    codex {
      use-skills "a" "a"
    }
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        assert_eq!(
            planned
                .iter()
                .map(|e| e.target_rel_path.as_str())
                .collect::<Vec<_>>(),
            [".claude/skills/a", ".agents/skills/a"]
        );
    }

    #[test]
    fn plans_an_agent_selected_twice_as_one_write() {
        let text = r#"
enozunu config-version=1 {
  provider {
    agents {
      agent "helper" { git { url "https://example.com/r"; branch "main"; path "a/helper.md" } }
    }
  }
  consumer {
    claude {
      use-agents "helper"
      use-agents "helper"
    }
  }
}
"#;
        let planned = plan(&manifest::parse(text).unwrap()).unwrap();
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].target_rel_path, ".claude/agents/helper.md");
    }

    #[test]
    fn keeps_every_duplicate_skill_usage_in_the_manifest_model() {
        // Plan-level normalization must not reach back into the consumer declarations: both selections of `a`, each with its own `when`, stay in the manifest for instruction generation.
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
        when "before completion"
      }
      use-skills "a" {
        when "after implementation"
      }
    }
  }
}
"#;
        let parsed = manifest::parse(text).unwrap();
        let planned = plan(&parsed).unwrap();
        assert_eq!(
            planned
                .iter()
                .filter(|e| e.kind == ArtifactKind::Skill)
                .count(),
            1
        );
        let consumer = parsed.consumer.targets().next().unwrap().1;
        assert_eq!(consumer.use_skills.len(), 2);
        assert_eq!(consumer.use_skills[0].whens, ["before completion"]);
        assert_eq!(consumer.use_skills[1].whens, ["after implementation"]);
    }

    #[test]
    fn rejects_different_artifacts_colliding_on_one_target_path() {
        // No current declaration shape reaches this collision (provider names are unique per kind and every kind has its own path scheme), but the invariant guards future path mappings, so the test drives the normalization directly.
        let entry = |path: &str| PlannedMaterialization {
            source_name: "a".to_owned(),
            kind: ArtifactKind::Skill,
            reference: SourceReference::Git {
                url: "https://example.com/r".to_owned(),
                selector: GitSelector::Branch("main".to_owned()),
                path: path.to_owned(),
            },
            target_ai: TargetAi::Claude,
            target_rel_path: ".claude/skills/a".to_owned(),
        };

        let diags = normalize_writes(vec![entry("s/a"), entry("s/other")]).unwrap_err();
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::DuplicateTargetPath)
        );
    }
}
