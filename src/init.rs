//! Writes a starter `enozunu.kdl` for a new project.
//!
//! The starter manifest is a fixed document that exercises every field the config-version 1 schema accepts, so users can discover what can be written by reading it.
//! All values are placeholders meant to be replaced; resolving them with `enozunu summon` is not expected to succeed.

use std::path::Path;

use crate::diagnostics::{Diagnostic, DiagnosticCode};

// The template must stay schema-valid so `enozunu validate` succeeds right after `enozunu init`; a test enforces this.
const TEMPLATE: &str = r#"// Enozunu manifest.
// This starter file shows every field supported by config-version 1.
// Replace the placeholder values with your real sources, then check the result with `enozunu validate`.
enozunu config-version=1 {
  // `provider` declares the sources available for materialization.
  provider {
    // Skill sources. Each skill must resolve to a directory containing SKILL.md.
    skills {
      skill "example-skill" {
        // Git repository hosting the skill. GitHub tree/blob URL shorthand is not supported.
        git "https://github.com/your-org/your-skills-repo"
        // Branch to follow. Each run resolves the current head of this branch.
        branch "main"
        // Repository-relative path to the skill directory.
        path "path/to/skills/example-skill"
      }

      skill "another-skill" {
        git "https://github.com/your-org/your-skills-repo"
        branch "main"
        path "path/to/skills/another-skill"
      }
    }

    // Agent sources. Each agent must resolve to a single file.
    agents {
      agent "example-agent" {
        git "https://github.com/your-org/your-agents-repo"
        branch "main"
        // Repository-relative path to the agent file.
        path "path/to/agents/example-agent.md"
      }
    }
  }

  // `consumer` selects what to materialize for each target AI.
  // v0.0.x supports Claude only.
  consumer {
    claude {
      // Selected skills are materialized to .claude/skills/<name>/.
      use-skills "example-skill" "another-skill"
      // Selected agents are materialized to .claude/agents/<name>.md.
      use-agents "example-agent"
    }
  }
}
"#;

/// Writes the starter manifest to `manifest_path`.
///
/// Refuses to overwrite an existing file.
/// `init` bootstraps new projects; silently replacing a hand-edited manifest would destroy user configuration.
pub fn run_init(manifest_path: &Path) -> Result<(), Diagnostic> {
    if manifest_path.exists() {
        return Err(Diagnostic::new(
            DiagnosticCode::Io,
            format!(
                "{} already exists; refusing to overwrite it",
                manifest_path.display()
            ),
        ));
    }
    std::fs::write(manifest_path, TEMPLATE).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to write {}: {e}", manifest_path.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_is_a_valid_manifest_with_every_section_populated() {
        let manifest = crate::manifest::parse(TEMPLATE).unwrap();
        assert!(!manifest.provider.skills.is_empty());
        assert!(!manifest.provider.agents.is_empty());
        assert!(!manifest.consumer.claude.use_skills.is_empty());
        assert!(!manifest.consumer.claude.use_agents.is_empty());
    }

    #[test]
    fn writes_starter_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("enozunu.kdl");

        run_init(&path).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, TEMPLATE);
    }

    #[test]
    fn reports_write_failure_as_io_diagnostic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("missing-dir/enozunu.kdl");

        let diag = run_init(&path).unwrap_err();

        assert_eq!(diag.code, DiagnosticCode::Io);
    }

    #[test]
    fn refuses_to_overwrite_existing_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("enozunu.kdl");
        std::fs::write(&path, "hand-edited\n").unwrap();

        let diag = run_init(&path).unwrap_err();

        assert_eq!(diag.code, DiagnosticCode::Io);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hand-edited\n");
    }
}
