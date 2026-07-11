//! Builds the materialization plan from a validated manifest.
//!
//! Planning decides what would be written where; it does not resolve sources or touch the filesystem.

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::manifest::{Manifest, SourceReference, TargetAi, TargetConsumer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Skill,
    Agent,
}

impl ArtifactKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactKind::Skill => "skill",
            ArtifactKind::Agent => "agent",
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
    }
}

/// Plans materializations for every source selected by every declared consumer target.
///
/// Fails when two materializations resolve to the same target path, because later writes would silently overwrite earlier ones. The same source selected by both Claude and Codex resolves to distinct native paths, so it is not a collision.
pub fn plan(manifest: &Manifest) -> Result<Vec<PlannedMaterialization>, Vec<Diagnostic>> {
    let mut planned = Vec::new();

    for (target, consumer) in manifest.consumer.targets() {
        plan_target(manifest, target, consumer, &mut planned);
    }

    let mut diags = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in &planned {
        if !seen.insert(entry.target_rel_path.as_str()) {
            diags.push(Diagnostic::new(
                DiagnosticCode::DuplicateTargetPath,
                format!(
                    "multiple materializations resolve to the same target path `{}`",
                    entry.target_rel_path
                ),
            ));
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
    for name in &consumer.use_skills {
        // Reference existence is validated at parse time, so a missing lookup here is a programming error.
        let decl = manifest
            .provider
            .skills
            .iter()
            .find(|s| &s.name == name)
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn rejects_duplicate_target_paths() {
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
        let diags = plan(&manifest::parse(text).unwrap()).unwrap_err();
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::DuplicateTargetPath)
        );
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
    fn rejects_duplicate_target_paths_within_one_target() {
        // The same source selected twice by one target collides on its single native path.
        let text = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "a" { git { url "https://example.com/r"; branch "main"; path "s/a" } }
    }
  }
  consumer {
    codex {
      use-skills "a" "a"
    }
  }
}
"#;
        let diags = plan(&manifest::parse(text).unwrap()).unwrap_err();
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagnosticCode::DuplicateTargetPath)
        );
    }
}
