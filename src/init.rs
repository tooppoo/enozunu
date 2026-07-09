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

// Excludes the resolver cache that `summon` populates at `.enozunu/cache`.
// The pattern is relative to the `.enozunu` directory holding this file.
// provenance.json is deliberately left tracked; only the reproducible cache is ignored.
const GITIGNORE_TEMPLATE: &str = "cache/\n";

/// Writes the starter manifest and the `.enozunu/.gitignore` that excludes the resolver cache.
///
/// Refuses to overwrite an existing manifest.
/// `init` bootstraps new projects; silently replacing a hand-edited manifest would destroy user configuration.
pub fn run_init(manifest_path: &Path, project_root: &Path) -> Result<(), Diagnostic> {
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
    })?;

    write_cache_gitignore(project_root)
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
        assert!(!manifest.consumer.claude.use_skills.is_empty());
        assert!(!manifest.consumer.claude.use_agents.is_empty());
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
}
