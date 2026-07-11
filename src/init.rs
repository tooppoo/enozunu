//! Writes a starter `enozunu.kdl` for a new project.
//!
//! The starter manifest is a fixed document that exercises every field the config-version 1 schema accepts, so users can discover what can be written by reading it.
//! All values are placeholders meant to be replaced; resolving them with `enozunu summon` is not expected to succeed.

use std::io::Write;
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
    // Every skill and agent declares exactly one source reference block: `git` or `local`.
    skills {
      skill "example-skill" {
        // Git source reference: resolves the skill from a Git repository.
        git {
          // Repository hosting the skill. GitHub tree/blob URL shorthand is not supported.
          url "https://github.com/your-org/your-skills-repo"
          // Branch to follow. Each run resolves the current head of this branch.
          branch "main"
          // Repository-relative path to the skill directory.
          path "path/to/skills/example-skill"
        }
      }

      skill "another-skill" {
        // Local source reference: resolves the skill from the filesystem.
        local {
          // Path relative to this manifest's directory. `..` may reference sibling repositories; absolute paths are rejected.
          path "../sibling-repo/path/to/skills/another-skill"
        }
      }
    }

    // Agent sources. Each agent must resolve to a single file.
    // Enozunu materializes an agent source verbatim; it does not convert a Claude
    // Markdown agent into a Codex TOML agent, or the reverse. Declare a
    // target-native source for each target AI you select the agent from.
    agents {
      agent "example-agent-claude" {
        git {
          url "https://github.com/your-org/your-agents-repo"
          branch "main"
          // Repository-relative path to the Claude Markdown agent file.
          path "path/to/agents/example-agent.md"
        }
      }

      agent "example-agent-codex" {
        git {
          url "https://github.com/your-org/your-agents-repo"
          branch "main"
          // Repository-relative path to the Codex TOML agent file.
          path "path/to/agents/example-agent.toml"
        }
      }
    }
  }

  // `consumer` selects what to materialize for each target AI.
  // Claude and Codex select from the same `provider` pool. A Skill source can be
  // selected from both; an agent source is target-native, so each target selects
  // the agent written for it. Enozunu projects each selection into the target's
  // native path without converting the artifact's format.
  consumer {
    claude {
      // Selected skills are materialized to .claude/skills/<name>/.
      use-skills "example-skill" "another-skill"
      // Selected agents are materialized to .claude/agents/<name>.md.
      use-agents "example-agent-claude"
    }

    codex {
      // The same Skill sources are materialized to .agents/skills/<name>/.
      use-skills "example-skill" "another-skill"
      // Selected agents are materialized to .codex/agents/<name>.toml.
      use-agents "example-agent-codex"
    }
  }
}
"#;

// Excludes the resolver cache that `summon` populates at `.enozunu/cache`.
// The pattern is relative to the `.enozunu` directory holding this file.
// provenance.json is deliberately left tracked; only the reproducible cache is ignored.
const GITIGNORE_TEMPLATE: &str = "cache/\n";

/// Writes the starter manifest and the `.enozunu/.gitignore` that excludes the resolver cache.
///
/// Refuses to overwrite an existing manifest.
/// `init` bootstraps new projects; silently replacing a hand-edited manifest would destroy user configuration.
pub fn run_init(manifest_path: &Path, project_root: &Path) -> Result<(), Diagnostic> {
    // Ensure the cache ignore before the manifest write.
    // The manifest write refuses to overwrite and returns early; a project that already has a
    // manifest but predates this ignore could then never gain it. Writing the ignore first,
    // and idempotently, lets a re-run add it to such a project.
    write_cache_gitignore(project_root)?;

    // An exclusive create (`O_CREAT | O_EXCL`) instead of an exists-then-write sequence.
    // The exists check would follow a dangling symlink and let init write outside the intended path, and it would race with a concurrently created manifest.
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(manifest_path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(Diagnostic::new(
                DiagnosticCode::Io,
                format!(
                    "{} already exists; refusing to overwrite it",
                    manifest_path.display()
                ),
            ));
        }
        Err(e) => {
            return Err(Diagnostic::new(
                DiagnosticCode::Io,
                format!("failed to write {}: {e}", manifest_path.display()),
            ));
        }
    };
    file.write_all(TEMPLATE.as_bytes()).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to write {}: {e}", manifest_path.display()),
        )
    })
}

/// Writes `.enozunu/.gitignore` so the resolver cache stays out of version control.
///
/// Leaves an existing `.gitignore` untouched so a hand-edited file survives a re-run;
/// `init` only guarantees the file's presence, not its exact contents.
fn write_cache_gitignore(project_root: &Path) -> Result<(), Diagnostic> {
    let dir = project_root.join(".enozunu");
    std::fs::create_dir_all(&dir).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to create {}: {e}", dir.display()),
        )
    })?;

    let path = dir.join(".gitignore");
    // Exclusive create for the same dangling-symlink and race reasons as the manifest write above.
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => file.write_all(GITIGNORE_TEMPLATE.as_bytes()).map_err(|e| {
            Diagnostic::new(
                DiagnosticCode::Io,
                format!("failed to write {}: {e}", path.display()),
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(Diagnostic::new(
            DiagnosticCode::Io,
            format!("failed to write {}: {e}", path.display()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_is_a_valid_manifest_with_every_section_populated() {
        let manifest = crate::manifest::parse(TEMPLATE).unwrap();
        assert!(!manifest.provider.skills.is_empty());
        assert!(!manifest.provider.agents.is_empty());

        let claude = manifest.consumer.claude.as_ref().unwrap();
        assert!(!claude.use_skills.is_empty());
        assert!(!claude.use_agents.is_empty());

        // The template exercises both target AIs, including a Skill source shared across them.
        let codex = manifest.consumer.codex.as_ref().unwrap();
        assert!(!codex.use_skills.is_empty());
        assert!(!codex.use_agents.is_empty());
        assert!(
            claude
                .use_skills
                .iter()
                .any(|s| codex.use_skills.contains(s)),
            "template should show one Skill source selected from both targets"
        );
    }

    #[test]
    fn writes_starter_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("enozunu.kdl");

        run_init(&path, tmp.path()).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, TEMPLATE);
    }

    #[test]
    fn writes_cache_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("enozunu.kdl");

        run_init(&path, tmp.path()).unwrap();

        let gitignore = std::fs::read_to_string(tmp.path().join(".enozunu/.gitignore")).unwrap();
        assert_eq!(gitignore, GITIGNORE_TEMPLATE);
    }

    #[test]
    fn keeps_existing_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("enozunu.kdl");
        let gitignore_path = tmp.path().join(".enozunu/.gitignore");
        std::fs::create_dir_all(gitignore_path.parent().unwrap()).unwrap();
        std::fs::write(&gitignore_path, "cache/\nnotes.txt\n").unwrap();

        run_init(&path, tmp.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(&gitignore_path).unwrap(),
            "cache/\nnotes.txt\n"
        );
    }

    #[test]
    fn refuses_dangling_symlink_at_manifest_path() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("enozunu.kdl");
        let target = tmp.path().join("elsewhere.kdl");
        std::os::unix::fs::symlink(&target, &path).unwrap();

        let diag = run_init(&path, tmp.path()).unwrap_err();

        assert_eq!(diag.code, DiagnosticCode::Io);
        assert!(!target.exists());
    }

    #[test]
    fn reports_write_failure_as_io_diagnostic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("missing-dir/enozunu.kdl");

        let diag = run_init(&path, tmp.path()).unwrap_err();

        assert_eq!(diag.code, DiagnosticCode::Io);
    }

    #[test]
    fn reports_gitignore_write_failure_as_io_diagnostic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("enozunu.kdl");
        // A regular file where the `.enozunu` directory must go blocks its creation.
        std::fs::write(tmp.path().join(".enozunu"), "").unwrap();

        let diag = run_init(&path, tmp.path()).unwrap_err();

        assert_eq!(diag.code, DiagnosticCode::Io);
    }

    #[test]
    fn refuses_to_overwrite_existing_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("enozunu.kdl");
        std::fs::write(&path, "hand-edited\n").unwrap();

        let diag = run_init(&path, tmp.path()).unwrap_err();

        assert_eq!(diag.code, DiagnosticCode::Io);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hand-edited\n");
    }

    #[test]
    fn adds_gitignore_to_a_project_that_already_has_a_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("enozunu.kdl");
        std::fs::write(&path, "hand-edited\n").unwrap();

        // The manifest already exists, so init still reports the refusal.
        run_init(&path, tmp.path()).unwrap_err();

        // The ignore is created regardless, so re-running init backfills it for older projects.
        let gitignore = std::fs::read_to_string(tmp.path().join(".enozunu/.gitignore")).unwrap();
        assert_eq!(gitignore, GITIGNORE_TEMPLATE);
    }
}
