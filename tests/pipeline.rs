//! End-to-end pipeline tests against local Git repositories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use enozunu::diagnostics::DiagnosticCode;
use enozunu::git::CommandGitResolver;

struct TestProject {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    source_repo: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("failed to run git");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn commit_all(repo: &Path, message: &str) {
    git(repo, &["add", "--all"]);
    git(repo, &["commit", "--quiet", "-m", message]);
}

/// Creates a project directory plus a source repository containing one skill and one agent.
fn setup() -> TestProject {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    let source_repo = tmp.path().join("source-repo");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&source_repo).unwrap();

    git(
        &source_repo,
        &["init", "--quiet", "--initial-branch", "main"],
    );
    let skill_dir = source_repo.join("skills/demo-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "# demo skill\n").unwrap();
    fs::write(skill_dir.join("helper.txt"), "supporting file\n").unwrap();
    let agent_dir = source_repo.join("agents");
    fs::create_dir_all(&agent_dir).unwrap();
    fs::write(agent_dir.join("demo-agent.md"), "# demo agent\n").unwrap();
    commit_all(&source_repo, "initial");

    TestProject {
        _tmp: tmp,
        root,
        source_repo,
    }
}

fn write_manifest(project: &TestProject) {
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "demo-skill" {{
        git "{url}"
        branch "main"
        path "skills/demo-skill"
      }}
    }}
    agents {{
      agent "demo-agent" {{
        git "{url}"
        branch "main"
        path "agents/demo-agent.md"
      }}
    }}
  }}
  consumer {{
    claude {{
      use-skills "demo-skill"
      use-agents "demo-agent"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();
}

fn materialize(project: &TestProject) -> Result<(), Vec<enozunu::diagnostics::Diagnostic>> {
    let resolver = CommandGitResolver::new(project.root.join(".enozunu/cache"));
    enozunu::run_materialize(&project.root.join("enozunu.kdl"), &project.root, &resolver)
        .map(|_| ())
}

#[test]
fn materializes_skill_and_agent_into_claude_paths() {
    let project = setup();
    write_manifest(&project);

    materialize(&project).unwrap();

    let skill_md = project.root.join(".claude/skills/demo-skill/SKILL.md");
    let helper = project.root.join(".claude/skills/demo-skill/helper.txt");
    let agent = project.root.join(".claude/agents/demo-agent.md");
    assert!(skill_md.is_file());
    assert!(helper.is_file());
    assert_eq!(fs::read_to_string(agent).unwrap(), "# demo agent\n");
}

#[test]
fn records_provenance_with_resolved_revision() {
    let project = setup();
    write_manifest(&project);

    materialize(&project).unwrap();

    let text = fs::read_to_string(project.root.join(".enozunu/provenance.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = record["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);

    let head = Command::new("git")
        .arg("-C")
        .arg(&project.source_repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();

    for entry in entries {
        assert_eq!(entry["resolved_revision"], head.as_str());
        assert_eq!(entry["target_ai"], "claude");
        assert_eq!(entry["branch"], "main");
    }
    assert_eq!(entries[0]["kind"], "skill");
    assert_eq!(entries[0]["target_path"], ".claude/skills/demo-skill");
    assert_eq!(entries[1]["kind"], "agent");
    assert_eq!(entries[1]["target_path"], ".claude/agents/demo-agent.md");
}

#[test]
fn rematerialize_replaces_instead_of_merging() {
    let project = setup();
    write_manifest(&project);
    materialize(&project).unwrap();

    let helper = project.root.join(".claude/skills/demo-skill/helper.txt");
    assert!(helper.is_file());

    // Remove the supporting file from the source; regeneration must remove it from the target too.
    fs::remove_file(project.source_repo.join("skills/demo-skill/helper.txt")).unwrap();
    commit_all(&project.source_repo, "remove helper");

    materialize(&project).unwrap();

    assert!(!helper.exists());
    assert!(
        project
            .root
            .join(".claude/skills/demo-skill/SKILL.md")
            .is_file()
    );
}

#[test]
fn manual_edits_in_generated_output_are_not_preserved() {
    let project = setup();
    write_manifest(&project);
    materialize(&project).unwrap();

    let edited = project
        .root
        .join(".claude/skills/demo-skill/manual-edit.txt");
    fs::write(&edited, "hand-written\n").unwrap();

    materialize(&project).unwrap();

    assert!(!edited.exists());
}

#[test]
fn rejects_skill_source_without_skill_md() {
    let project = setup();
    fs::remove_file(project.source_repo.join("skills/demo-skill/SKILL.md")).unwrap();
    commit_all(&project.source_repo, "drop SKILL.md");
    write_manifest(&project);

    let diags = materialize(&project).unwrap_err();
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::ArtifactShape)
    );
    assert!(!project.root.join(".claude").exists());
}

#[test]
fn rejects_symlink_inside_skill_source() {
    let project = setup();
    let secret = project.source_repo.join("secret.txt");
    fs::write(&secret, "outside the skill\n").unwrap();
    std::os::unix::fs::symlink(
        "../../secret.txt",
        project.source_repo.join("skills/demo-skill/link.txt"),
    )
    .unwrap();
    commit_all(&project.source_repo, "add symlink");
    write_manifest(&project);

    let diags = materialize(&project).unwrap_err();
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnsafePath));
    assert!(!project.root.join(".claude").exists());
}

#[test]
fn rejects_unresolvable_source_path() {
    let project = setup();
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "missing" {{
        git "{url}"
        branch "main"
        path "skills/does-not-exist"
      }}
    }}
  }}
  consumer {{
    claude {{
      use-skills "missing"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    let diags = materialize(&project).unwrap_err();
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::ArtifactShape)
    );
}

#[test]
fn rejects_unknown_branch_with_git_resolution_diagnostic() {
    let project = setup();
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "demo-agent" {{
        git "{url}"
        branch "no-such-branch"
        path "agents/demo-agent.md"
      }}
    }}
  }}
  consumer {{
    claude {{
      use-agents "demo-agent"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    let diags = materialize(&project).unwrap_err();
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::GitResolution)
    );
}

#[test]
fn materializes_two_branches_of_the_same_repository_independently() {
    let project = setup();

    // A second branch with different agent content; the pipeline must keep per-branch checkouts apart.
    git(
        &project.source_repo,
        &["checkout", "--quiet", "-b", "other"],
    );
    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent on other\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "change agent on other");
    git(&project.source_repo, &["checkout", "--quiet", "main"]);

    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "agent-main" {{
        git "{url}"
        branch "main"
        path "agents/demo-agent.md"
      }}
      agent "agent-other" {{
        git "{url}"
        branch "other"
        path "agents/demo-agent.md"
      }}
    }}
  }}
  consumer {{
    claude {{
      use-agents "agent-main" "agent-other"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    materialize(&project).unwrap();

    let main_agent = fs::read_to_string(project.root.join(".claude/agents/agent-main.md")).unwrap();
    let other_agent =
        fs::read_to_string(project.root.join(".claude/agents/agent-other.md")).unwrap();
    assert_eq!(main_agent, "# demo agent\n");
    assert_eq!(other_agent, "# demo agent on other\n");

    let rev = |branch: &str| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&project.source_repo)
            .args(["rev-parse", branch])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    let text = fs::read_to_string(project.root.join(".enozunu/provenance.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = record["entries"].as_array().unwrap();
    assert_eq!(entries[0]["resolved_revision"], rev("main").as_str());
    assert_eq!(entries[1]["resolved_revision"], rev("other").as_str());
}

#[test]
fn follows_branch_updates_across_runs() {
    let project = setup();
    write_manifest(&project);
    materialize(&project).unwrap();

    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent v2\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "update agent");

    materialize(&project).unwrap();

    let agent = fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap();
    assert_eq!(agent, "# demo agent v2\n");
}
