//! Builds the materialization plan from a validated manifest.
//!
//! Planning decides what would be written where; it does not resolve sources or touch the filesystem.

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::manifest::{Manifest, SourceReference};

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

/// One selected source and the project-relative Claude target path it materializes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMaterialization {
    pub source_name: String,
    pub kind: ArtifactKind,
    pub reference: SourceReference,
    pub target_rel_path: String,
}

/// Plans materializations for every source selected by `consumer.claude`.
///
/// Fails when two materializations resolve to the same target path, because later writes would silently overwrite earlier ones.
pub fn plan(manifest: &Manifest) -> Result<Vec<PlannedMaterialization>, Vec<Diagnostic>> {
    let mut planned = Vec::new();

    for name in &manifest.consumer.claude.use_skills {
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
            target_rel_path: format!(".claude/skills/{}", decl.name),
        });
    }

    for name in &manifest.consumer.claude.use_agents {
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
            target_rel_path: format!(".claude/agents/{}.md", decl.name),
        });
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
}
