//! End-to-end pipeline tests against local Git repositories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use enozunu::diagnostics::DiagnosticCode;
use enozunu::git::{CommandGitResolver, CommitSha, GitResolutionRequest, GitResolver, GitSelector};
use enozunu::{LockMode, LockOutcome, MaterializeOutcome};

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
        git {{
          url "{url}"
          branch "main"
          path "skills/demo-skill"
        }}
      }}
    }}
    agents {{
      agent "demo-agent" {{
        git {{
          url "{url}"
          branch "main"
          path "agents/demo-agent.md"
        }}
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

/// Creates a local (non-Git) source tree next to the project, containing one skill and one agent.
fn setup_local_source(project: &TestProject) -> PathBuf {
    let local_src = project.root.parent().unwrap().join("local-src");
    let skill_dir = local_src.join("skills/local-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "# local skill\n").unwrap();
    fs::write(skill_dir.join("helper.txt"), "local helper\n").unwrap();
    let agent_dir = local_src.join("agents");
    fs::create_dir_all(&agent_dir).unwrap();
    fs::write(agent_dir.join("local-agent.md"), "# local agent\n").unwrap();
    local_src
}

fn write_local_manifest(project: &TestProject) {
    let manifest = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "local-skill" {
        local {
          path "../local-src/skills/local-skill"
        }
      }
    }
    agents {
      agent "local-agent" {
        local {
          path "../local-src/agents/local-agent.md"
        }
      }
    }
  }
  consumer {
    claude {
      use-skills "local-skill"
      use-agents "local-agent"
    }
  }
}
"#;
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();
}

fn materialize(project: &TestProject) -> Result<(), Vec<enozunu::diagnostics::Diagnostic>> {
    materialize_with(project, LockMode::Locked).map(|_| ())
}

fn materialize_with(
    project: &TestProject,
    mode: LockMode,
) -> Result<MaterializeOutcome, Vec<enozunu::diagnostics::Diagnostic>> {
    let resolver = CommandGitResolver::new(project.root.join(".enozunu/cache"));
    enozunu::run_materialize(
        &project.root.join("enozunu.kdl"),
        &project.root,
        &resolver,
        mode,
    )
}

fn read_lock(project: &TestProject) -> serde_json::Value {
    let text = fs::read_to_string(project.root.join("enozunu.lock.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Adds a Codex-native TOML agent file to the source repository and commits it.
///
/// Enozunu does not convert between agent formats, so a Codex agent source is a target-native file the provider supplies; this gives the Codex tests one to select.
fn add_codex_agent(project: &TestProject) {
    fs::write(
        project.source_repo.join("agents/demo-agent.toml"),
        "name = \"demo agent\"\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "add codex agent");
}

#[test]
fn materializes_codex_skill_and_agent_into_codex_native_paths() {
    let project = setup();
    add_codex_agent(&project);
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "demo-skill" {{
        git {{
          url "{url}"
          branch "main"
          path "skills/demo-skill"
        }}
      }}
    }}
    agents {{
      agent "demo-agent" {{
        git {{
          url "{url}"
          branch "main"
          path "agents/demo-agent.toml"
        }}
      }}
    }}
  }}
  consumer {{
    codex {{
      use-skills "demo-skill"
      use-agents "demo-agent"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    materialize(&project).unwrap();

    // The Skill tree lands under Codex's native Skill path, and the agent under its native agent path.
    assert!(
        project
            .root
            .join(".agents/skills/demo-skill/SKILL.md")
            .is_file()
    );
    assert!(
        project
            .root
            .join(".agents/skills/demo-skill/helper.txt")
            .is_file()
    );
    assert_eq!(
        fs::read_to_string(project.root.join(".codex/agents/demo-agent.toml")).unwrap(),
        "name = \"demo agent\"\n"
    );
    // No Claude output is produced for a Codex-only manifest.
    assert!(!project.root.join(".claude").exists());
}

#[test]
fn materializes_the_same_skill_for_claude_and_codex_in_one_run() {
    let project = setup();
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "demo-skill" {{
        git {{
          url "{url}"
          branch "main"
          path "skills/demo-skill"
        }}
      }}
    }}
  }}
  consumer {{
    claude {{
      use-skills "demo-skill"
    }}
    codex {{
      use-skills "demo-skill"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    materialize(&project).unwrap();

    assert!(
        project
            .root
            .join(".claude/skills/demo-skill/SKILL.md")
            .is_file()
    );
    assert!(
        project
            .root
            .join(".agents/skills/demo-skill/SKILL.md")
            .is_file()
    );

    // One source placed in two targets records one provenance entry per target, sharing source identity.
    let text = fs::read_to_string(project.root.join(".enozunu/provenance.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = record["entries"].as_array().unwrap();
    assert_eq!(record["version"], 1);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["target_ai"], "claude");
    assert_eq!(entries[0]["target_path"], ".claude/skills/demo-skill");
    assert_eq!(entries[1]["target_ai"], "codex");
    assert_eq!(entries[1]["target_path"], ".agents/skills/demo-skill");
    // The source object is identical across targets: same kind, same Git identity.
    assert_eq!(entries[0]["kind"], "skill");
    assert_eq!(entries[1]["kind"], "skill");
    assert_eq!(entries[0]["source"], entries[1]["source"]);
    assert_eq!(entries[0]["source"]["type"], "git");
}

#[test]
fn resolves_a_shared_git_source_once_across_targets() {
    // A resolver that records every request, so a source selected by both targets is proven to resolve once.
    struct CountingResolver {
        inner: CommandGitResolver,
        requests: std::cell::RefCell<Vec<GitResolutionRequest>>,
    }
    impl GitResolver for CountingResolver {
        fn resolve(
            &self,
            request: &GitResolutionRequest,
        ) -> Result<enozunu::git::ResolvedSource, enozunu::git::GitError> {
            self.requests.borrow_mut().push(request.clone());
            self.inner.resolve(request)
        }
    }

    let project = setup();
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "demo-skill" {{
        git {{
          url "{url}"
          branch "main"
          path "skills/demo-skill"
        }}
      }}
    }}
  }}
  consumer {{
    claude {{ use-skills "demo-skill" }}
    codex {{ use-skills "demo-skill" }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    let resolver = CountingResolver {
        inner: CommandGitResolver::new(project.root.join(".enozunu/cache")),
        requests: std::cell::RefCell::new(Vec::new()),
    };
    enozunu::run_materialize(
        &project.root.join("enozunu.kdl"),
        &project.root,
        &resolver,
        LockMode::Locked,
    )
    .unwrap();

    // The same (url, branch) selected by two targets is resolved exactly once.
    assert_eq!(resolver.requests.borrow().len(), 1);
}

#[test]
fn rejects_an_invalid_artifact_before_writing_any_target() {
    // The skill lacks SKILL.md, so validation must fail before either target is written.
    let project = setup();
    fs::remove_file(project.source_repo.join("skills/demo-skill/SKILL.md")).unwrap();
    commit_all(&project.source_repo, "drop SKILL.md");
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "demo-skill" {{
        git {{
          url "{url}"
          branch "main"
          path "skills/demo-skill"
        }}
      }}
    }}
  }}
  consumer {{
    claude {{ use-skills "demo-skill" }}
    codex {{ use-skills "demo-skill" }}
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
    // Neither target's output directory is created when validation fails up front.
    assert!(!project.root.join(".claude").exists());
    assert!(!project.root.join(".agents").exists());
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
        assert_eq!(entry["source"]["type"], "git");
        assert_eq!(entry["source"]["resolved_revision"], head.as_str());
        // The declared selector is recorded as one tagged object, not as a `branch` field.
        assert_eq!(entry["source"]["selector"]["type"], "branch");
        assert_eq!(entry["source"]["selector"]["value"], "main");
        assert!(entry["source"].get("branch").is_none());
        assert_eq!(entry["target_ai"], "claude");
    }
    assert_eq!(entries[0]["kind"], "skill");
    assert_eq!(entries[0]["source"]["path"], "skills/demo-skill");
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
    // The default mode would materialize the locked pre-removal commit, so the run opts into update.
    fs::remove_file(project.source_repo.join("skills/demo-skill/helper.txt")).unwrap();
    commit_all(&project.source_repo, "remove helper");

    materialize_with(&project, LockMode::Update).unwrap();

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
        git {{
          url "{url}"
          branch "main"
          path "skills/does-not-exist"
        }}
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
        git {{
          url "{url}"
          branch "no-such-branch"
          path "agents/demo-agent.md"
        }}
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
        git {{
          url "{url}"
          branch "main"
          path "agents/demo-agent.md"
        }}
      }}
      agent "agent-other" {{
        git {{
          url "{url}"
          branch "other"
          path "agents/demo-agent.md"
        }}
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
    assert_eq!(
        entries[0]["source"]["resolved_revision"],
        rev("main").as_str()
    );
    assert_eq!(
        entries[1]["source"]["resolved_revision"],
        rev("other").as_str()
    );
}

#[test]
fn materializes_local_skill_and_agent_from_a_sibling_directory() {
    let project = setup();
    setup_local_source(&project);
    write_local_manifest(&project);

    materialize(&project).unwrap();

    let skill_md = project.root.join(".claude/skills/local-skill/SKILL.md");
    let helper = project.root.join(".claude/skills/local-skill/helper.txt");
    let agent = project.root.join(".claude/agents/local-agent.md");
    assert_eq!(fs::read_to_string(skill_md).unwrap(), "# local skill\n");
    assert!(helper.is_file());
    assert_eq!(fs::read_to_string(agent).unwrap(), "# local agent\n");
}

#[test]
fn resolves_local_paths_from_the_manifest_directory_not_the_working_directory() {
    let project = setup();
    let local_src = setup_local_source(&project);
    write_local_manifest(&project);

    // The test process's working directory is unrelated to the project, so `../local-src/...` only resolves if the pipeline anchors it at the manifest directory.
    assert_ne!(
        std::env::current_dir().unwrap(),
        project.root,
        "test precondition: working directory must differ from the project root"
    );
    assert!(local_src.exists());

    materialize(&project).unwrap();

    assert!(
        project
            .root
            .join(".claude/skills/local-skill/SKILL.md")
            .is_file()
    );
}

#[test]
fn records_local_provenance_with_resolved_path() {
    let project = setup();
    let local_src = setup_local_source(&project);
    write_local_manifest(&project);

    materialize(&project).unwrap();

    let text = fs::read_to_string(project.root.join(".enozunu/provenance.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = record["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);

    let skill_source = &entries[0]["source"];
    assert_eq!(skill_source["type"], "local");
    assert_eq!(skill_source["path"], "../local-src/skills/local-skill");
    assert_eq!(
        skill_source["resolved_path"],
        local_src
            .join("skills/local-skill")
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    );
    assert!(skill_source.get("resolved_revision").is_none());
    assert_eq!(entries[0]["target_path"], ".claude/skills/local-skill");

    assert_eq!(entries[1]["source"]["type"], "local");
    assert_eq!(entries[1]["target_path"], ".claude/agents/local-agent.md");
}

#[test]
fn rejects_symlinked_local_skill_source_path() {
    let project = setup();
    let local_src = setup_local_source(&project);
    std::os::unix::fs::symlink(
        local_src.join("skills/local-skill"),
        local_src.join("skills/linked-skill"),
    )
    .unwrap();

    let manifest = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "linked-skill" {
        local {
          path "../local-src/skills/linked-skill"
        }
      }
    }
  }
  consumer {
    claude {
      use-skills "linked-skill"
    }
  }
}
"#;
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    let diags = materialize(&project).unwrap_err();
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnsafePath));
    assert!(!project.root.join(".claude").exists());
}

#[test]
fn rejects_local_source_that_overlaps_its_target() {
    let project = setup();
    // The source lives at the exact path the materialization would replace.
    let source = project.root.join(".claude/skills/self-skill");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "# self\n").unwrap();

    let manifest = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "self-skill" {
        local {
          path ".claude/skills/self-skill"
        }
      }
    }
  }
  consumer {
    claude {
      use-skills "self-skill"
    }
  }
}
"#;
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    let diags = materialize(&project).unwrap_err();
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnsafePath));
    // The overlapping source must survive the rejected run untouched.
    assert!(source.join("SKILL.md").is_file());
}

#[test]
fn rejects_local_source_whose_target_resolves_onto_it_through_a_symlink() {
    let project = setup();
    // `.claude/skills` is a symlink out of the project, so the `shared-skill` target resolves onto the source itself; materializing would destroy the source.
    let shared = project.root.parent().unwrap().join("shared");
    fs::create_dir_all(shared.join("shared-skill")).unwrap();
    fs::write(shared.join("shared-skill/SKILL.md"), "# shared\n").unwrap();
    fs::create_dir_all(project.root.join(".claude")).unwrap();
    std::os::unix::fs::symlink(&shared, project.root.join(".claude/skills")).unwrap();

    let manifest = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "shared-skill" {
        local {
          path "../shared/shared-skill"
        }
      }
    }
  }
  consumer {
    claude {
      use-skills "shared-skill"
    }
  }
}
"#;
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    let diags = materialize(&project).unwrap_err();
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnsafePath));
    assert_eq!(
        fs::read_to_string(shared.join("shared-skill/SKILL.md")).unwrap(),
        "# shared\n"
    );
}

#[test]
fn rejects_local_source_that_overlaps_another_entries_target() {
    let project = setup();
    setup_local_source(&project);
    // `inner-skill`'s source sits at the target that `local-skill` will replace in the same run.
    let inner = project.root.join(".claude/skills/local-skill");
    fs::create_dir_all(&inner).unwrap();
    fs::write(inner.join("SKILL.md"), "# inner\n").unwrap();

    let manifest = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "local-skill" {
        local {
          path "../local-src/skills/local-skill"
        }
      }
      skill "inner-skill" {
        local {
          path ".claude/skills/local-skill"
        }
      }
    }
  }
  consumer {
    claude {
      use-skills "local-skill" "inner-skill"
    }
  }
}
"#;
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    let diags = materialize(&project).unwrap_err();
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnsafePath));
    // The rejected run must not have replaced the overlapping source.
    assert_eq!(
        fs::read_to_string(inner.join("SKILL.md")).unwrap(),
        "# inner\n"
    );
}

#[test]
fn materializes_git_and_local_sources_in_one_run() {
    let project = setup();
    setup_local_source(&project);
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "demo-skill" {{
        git {{
          url "{url}"
          branch "main"
          path "skills/demo-skill"
        }}
      }}
      skill "local-skill" {{
        local {{
          path "../local-src/skills/local-skill"
        }}
      }}
    }}
  }}
  consumer {{
    claude {{
      use-skills "demo-skill" "local-skill"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    materialize(&project).unwrap();

    assert!(
        project
            .root
            .join(".claude/skills/demo-skill/SKILL.md")
            .is_file()
    );
    assert!(
        project
            .root
            .join(".claude/skills/local-skill/SKILL.md")
            .is_file()
    );

    let text = fs::read_to_string(project.root.join(".enozunu/provenance.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = record["entries"].as_array().unwrap();
    assert_eq!(entries[0]["source"]["type"], "git");
    assert_eq!(entries[1]["source"]["type"], "local");
}

#[test]
fn a_default_summon_materializes_the_locked_revision_not_the_branch_tip() {
    let project = setup();
    let locked = rev_parse(&project.source_repo, "main");
    write_manifest(&project);
    let outcome = materialize_with(&project, LockMode::Locked).unwrap();
    assert_eq!(outcome.lock, LockOutcome::Created);

    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent v2\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "update agent");

    let outcome = materialize_with(&project, LockMode::Locked).unwrap();

    // The branch moved, but the default run stays on the recorded revision and leaves the lock alone.
    let agent = fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap();
    assert_eq!(agent, "# demo agent\n");
    assert_eq!(outcome.lock, LockOutcome::Unchanged);

    // Provenance reports the locked revision as what this run actually materialized.
    let text = fs::read_to_string(project.root.join(".enozunu/provenance.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        record["entries"][0]["source"]["resolved_revision"],
        locked.as_str()
    );
}

#[test]
fn summon_update_follows_the_new_branch_tip_and_rewrites_the_lock() {
    let project = setup();
    write_manifest(&project);
    materialize(&project).unwrap();

    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent v2\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "update agent");

    let outcome = materialize_with(&project, LockMode::Update).unwrap();

    let agent = fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap();
    assert_eq!(agent, "# demo agent v2\n");
    assert_eq!(outcome.lock, LockOutcome::Updated);
    let lock = read_lock(&project);
    assert_eq!(
        lock["entries"][0]["resolved_revision"],
        rev_parse(&project.source_repo, "main").as_str()
    );
}

#[test]
fn materializes_a_git_source_pinned_to_an_exact_revision() {
    let project = setup();
    let pinned = rev_parse(&project.source_repo, "main");
    // Advance the branch so the pinned revision is no longer the branch tip.
    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent v2\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "advance past the pinned revision");

    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "demo-agent" {{
        git {{
          url "{url}"
          revision "{pinned}"
          path "agents/demo-agent.md"
        }}
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

    materialize(&project).unwrap();

    // The pinned revision is materialized, not the advanced branch tip.
    let agent = fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap();
    assert_eq!(agent, "# demo agent\n");

    // Provenance records the declared revision selector and the identical resolved revision.
    let text = fs::read_to_string(project.root.join(".enozunu/provenance.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = record["entries"].as_array().unwrap();
    assert_eq!(record["version"], 1);
    assert_eq!(entries[0]["source"]["type"], "git");
    assert_eq!(entries[0]["source"]["selector"]["type"], "revision");
    assert_eq!(entries[0]["source"]["selector"]["value"], pinned.as_str());
    assert_eq!(entries[0]["source"]["resolved_revision"], pinned.as_str());
}

#[test]
fn branch_and_revision_selectors_with_identical_text_do_not_collide() {
    let project = setup();
    let pinned = rev_parse(&project.source_repo, "main");
    // A branch literally named after the pinned commit id, carrying different content: if the selector kind were absent from resolution or cache keys, the two sources below would share one checkout.
    git(
        &project.source_repo,
        &["checkout", "--quiet", "-b", &pinned],
    );
    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent on the sha-named branch\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "diverge on the sha-named branch");
    git(&project.source_repo, &["checkout", "--quiet", "main"]);

    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "by-branch" {{
        git {{
          url "{url}"
          branch "{pinned}"
          path "agents/demo-agent.md"
        }}
      }}
      agent "by-revision" {{
        git {{
          url "{url}"
          revision "{pinned}"
          path "agents/demo-agent.md"
        }}
      }}
    }}
  }}
  consumer {{
    claude {{
      use-agents "by-branch" "by-revision"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    materialize(&project).unwrap();

    let by_branch = fs::read_to_string(project.root.join(".claude/agents/by-branch.md")).unwrap();
    let by_revision =
        fs::read_to_string(project.root.join(".claude/agents/by-revision.md")).unwrap();
    assert_eq!(by_branch, "# demo agent on the sha-named branch\n");
    assert_eq!(by_revision, "# demo agent\n");
}

#[test]
fn materializes_a_git_source_selected_by_tag() {
    let project = setup();
    let tagged = rev_parse(&project.source_repo, "main");
    git(&project.source_repo, &["tag", "v1.0.0"]);
    // Advance the branch so the tag is no longer the branch tip; following the branch would materialize different content.
    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent v2\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "advance past the tag");

    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "demo-agent" {{
        git {{
          url "{url}"
          tag "v1.0.0"
          path "agents/demo-agent.md"
        }}
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

    materialize(&project).unwrap();

    let agent = fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap();
    assert_eq!(agent, "# demo agent\n");

    // Provenance records the declared tag and the commit it resolved to, which is the only record of where a mutable tag pointed during this run.
    let text = fs::read_to_string(project.root.join(".enozunu/provenance.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = record["entries"].as_array().unwrap();
    assert_eq!(record["version"], 1);
    assert_eq!(entries[0]["source"]["type"], "git");
    assert_eq!(entries[0]["source"]["selector"]["type"], "tag");
    assert_eq!(entries[0]["source"]["selector"]["value"], "v1.0.0");
    assert_eq!(entries[0]["source"]["resolved_revision"], tagged.as_str());
}

#[test]
fn resolves_an_annotated_tag_to_its_commit() {
    let project = setup();
    let tagged = rev_parse(&project.source_repo, "main");
    // An annotated tag is its own object; resolution must report the commit it points at, not the tag object id.
    git(
        &project.source_repo,
        &["tag", "--annotate", "v2.0.0", "-m", "release v2.0.0"],
    );
    assert_ne!(
        rev_parse(&project.source_repo, "v2.0.0"),
        tagged,
        "the annotated tag must be a distinct object for this test to mean anything"
    );

    let resolver = CommandGitResolver::new(project.root.join(".enozunu/cache"));
    let resolved = resolver
        .resolve(&GitResolutionRequest {
            url: format!("file://{}", project.source_repo.display()),
            selector: GitSelector::Tag("v2.0.0".to_owned()),
        })
        .unwrap();

    assert_eq!(resolved.commit, tagged);
    assert_eq!(
        fs::read_to_string(resolved.content_root.join("agents/demo-agent.md")).unwrap(),
        "# demo agent\n"
    );
    assert!(!resolved.content_root.join(".git").exists());
}

#[test]
fn branch_and_tag_selectors_with_identical_name_do_not_collide() {
    let project = setup();
    // A tag and a branch sharing one name, carrying different content: the tag selector must resolve through `refs/tags/`, and the selector kind must keep the two out of one cache slot.
    git(&project.source_repo, &["tag", "shared-name"]);
    git(
        &project.source_repo,
        &["checkout", "--quiet", "-b", "shared-name"],
    );
    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent on the branch\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "diverge on the branch");
    git(&project.source_repo, &["checkout", "--quiet", "main"]);

    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "by-branch" {{
        git {{
          url "{url}"
          branch "shared-name"
          path "agents/demo-agent.md"
        }}
      }}
      agent "by-tag" {{
        git {{
          url "{url}"
          tag "shared-name"
          path "agents/demo-agent.md"
        }}
      }}
    }}
  }}
  consumer {{
    claude {{
      use-agents "by-branch" "by-tag"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();

    materialize(&project).unwrap();

    let by_branch = fs::read_to_string(project.root.join(".claude/agents/by-branch.md")).unwrap();
    let by_tag = fs::read_to_string(project.root.join(".claude/agents/by-tag.md")).unwrap();
    assert_eq!(by_branch, "# demo agent on the branch\n");
    assert_eq!(by_tag, "# demo agent\n");
}

#[test]
fn follows_tag_moves_across_runs_with_update() {
    let project = setup();
    write_tag_manifest(&project, "v1.0.0");
    git(&project.source_repo, &["tag", "v1.0.0"]);
    materialize(&project).unwrap();
    assert_eq!(
        fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap(),
        "# demo agent\n"
    );

    // A tag is a mutable ref, but the default run holds the recorded commit; following a forced
    // tag move is an explicit update decision.
    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent v2\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "advance past the original tag");
    git(&project.source_repo, &["tag", "--force", "v1.0.0"]);

    materialize(&project).unwrap();
    assert_eq!(
        fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap(),
        "# demo agent\n"
    );

    materialize_with(&project, LockMode::Update).unwrap();

    let agent = fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap();
    assert_eq!(agent, "# demo agent v2\n");
}

#[test]
fn rejects_an_unknown_tag_with_git_resolution_diagnostic() {
    let project = setup();
    write_tag_manifest(&project, "v9.9.9");

    let diags = materialize(&project).unwrap_err();
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::GitResolution)
    );
    assert!(!project.root.join(".claude").exists());
}

/// Writes a manifest whose single agent source selects `tag` from the test source repository.
fn write_tag_manifest(project: &TestProject, tag: &str) {
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "demo-agent" {{
        git {{
          url "{url}"
          tag "{tag}"
          path "agents/demo-agent.md"
        }}
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
}

#[test]
fn resolves_identical_revision_sources_once() {
    // A resolver that records every request, so two sources pinning the same (url, revision) are proven to resolve once.
    struct CountingResolver {
        inner: CommandGitResolver,
        requests: std::cell::RefCell<Vec<GitResolutionRequest>>,
    }
    impl GitResolver for CountingResolver {
        fn resolve(
            &self,
            request: &GitResolutionRequest,
        ) -> Result<enozunu::git::ResolvedSource, enozunu::git::GitError> {
            self.requests.borrow_mut().push(request.clone());
            self.inner.resolve(request)
        }
    }

    let project = setup();
    let pinned = rev_parse(&project.source_repo, "main");
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "demo-skill" {{
        git {{
          url "{url}"
          revision "{pinned}"
          path "skills/demo-skill"
        }}
      }}
    }}
    agents {{
      agent "demo-agent" {{
        git {{
          url "{url}"
          revision "{pinned}"
          path "agents/demo-agent.md"
        }}
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

    let resolver = CountingResolver {
        inner: CommandGitResolver::new(project.root.join(".enozunu/cache")),
        requests: std::cell::RefCell::new(Vec::new()),
    };
    enozunu::run_materialize(
        &project.root.join("enozunu.kdl"),
        &project.root,
        &resolver,
        LockMode::Locked,
    )
    .unwrap();

    assert_eq!(resolver.requests.borrow().len(), 1);
    assert_eq!(
        resolver.requests.borrow()[0].selector,
        GitSelector::Revision(CommitSha::parse(&pinned).unwrap())
    );
}

#[test]
fn rejects_a_missing_revision_with_git_resolution_diagnostic() {
    let project = setup();
    let url = format!("file://{}", project.source_repo.display());
    // A well-formed commit id that does not exist in the repository.
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "demo-agent" {{
        git {{
          url "{url}"
          revision "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
          path "agents/demo-agent.md"
        }}
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
    assert!(!project.root.join(".claude").exists());
}

// Resolver/export boundary tests: the production resolver must hand materialization a content tree equal to the clean working tree, minus Git metadata.

fn rev_parse(repo: &Path, spec: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", spec])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn branch_request(project: &TestProject) -> GitResolutionRequest {
    GitResolutionRequest {
        url: format!("file://{}", project.source_repo.display()),
        selector: GitSelector::Branch("main".to_owned()),
    }
}

#[test]
fn resolver_exports_a_content_tree_without_git_metadata() {
    let project = setup();
    let resolver = CommandGitResolver::new(project.root.join(".enozunu/cache"));

    let resolved = resolver.resolve(&branch_request(&project)).unwrap();

    // The exported content equals the clean working tree.
    assert_eq!(
        fs::read_to_string(resolved.content_root.join("skills/demo-skill/SKILL.md")).unwrap(),
        "# demo skill\n"
    );
    assert_eq!(
        fs::read_to_string(resolved.content_root.join("agents/demo-agent.md")).unwrap(),
        "# demo agent\n"
    );
    // The content root carries no Git repository metadata, so materialization never reads a resolver cache checkout.
    assert!(!resolved.content_root.join(".git").exists());
    assert_eq!(resolved.commit, rev_parse(&project.source_repo, "main"));
}

#[test]
#[cfg(unix)]
fn resolver_export_preserves_the_executable_file_mode() {
    use std::os::unix::fs::PermissionsExt;
    let project = setup();
    let script = project.source_repo.join("run.sh");
    fs::write(&script, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    commit_all(&project.source_repo, "add executable");

    let resolver = CommandGitResolver::new(project.root.join(".enozunu/cache"));
    let resolved = resolver.resolve(&branch_request(&project)).unwrap();

    let mode = fs::metadata(resolved.content_root.join("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0, "executable bit must survive the export");
}

#[test]
fn resolver_resolves_a_pinned_non_head_revision() {
    let project = setup();
    let pinned = rev_parse(&project.source_repo, "main");
    // Advance the branch so the pinned revision is no longer HEAD.
    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent v2\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "advance past the pinned revision");

    let resolver = CommandGitResolver::new(project.root.join(".enozunu/cache"));
    let resolved = resolver
        .resolve(&GitResolutionRequest {
            url: format!("file://{}", project.source_repo.display()),
            selector: GitSelector::Revision(CommitSha::parse(&pinned).unwrap()),
        })
        .unwrap();

    assert_eq!(resolved.commit, pinned);
    // The export reflects the pinned revision, not the advanced branch tip.
    assert_eq!(
        fs::read_to_string(resolved.content_root.join("agents/demo-agent.md")).unwrap(),
        "# demo agent\n"
    );
    assert!(!resolved.content_root.join(".git").exists());
}

#[test]
fn resolver_export_drops_stale_files_from_a_previous_resolution() {
    let project = setup();
    let resolver = CommandGitResolver::new(project.root.join(".enozunu/cache"));

    let first = resolver.resolve(&branch_request(&project)).unwrap();
    // Simulate a leftover file from earlier processing inside the exported tree.
    fs::write(first.content_root.join("stale.txt"), "stale\n").unwrap();
    // Remove a tracked file so the next export must not carry it over either.
    fs::remove_file(project.source_repo.join("skills/demo-skill/helper.txt")).unwrap();
    commit_all(&project.source_repo, "remove helper");

    let second = resolver.resolve(&branch_request(&project)).unwrap();

    assert!(!second.content_root.join("stale.txt").exists());
    assert!(
        !second
            .content_root
            .join("skills/demo-skill/helper.txt")
            .exists()
    );
    assert!(
        second
            .content_root
            .join("skills/demo-skill/SKILL.md")
            .is_file()
    );
}

// Lockfile tests: the lock is the resolution input for mutable selectors, provenance stays the output record.

/// Writes a manifest covering every source kind: a branch skill, a local skill, a tag agent, and a revision-pinned agent.
fn write_all_source_kinds_manifest(project: &TestProject, pinned: &str) {
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "demo-skill" {{
        git {{
          url "{url}"
          branch "main"
          path "skills/demo-skill"
        }}
      }}
      skill "local-skill" {{
        local {{
          path "../local-src/skills/local-skill"
        }}
      }}
    }}
    agents {{
      agent "agent-tag" {{
        git {{
          url "{url}"
          tag "v1.0.0"
          path "agents/demo-agent.md"
        }}
      }}
      agent "agent-rev" {{
        git {{
          url "{url}"
          revision "{pinned}"
          path "agents/demo-agent.md"
        }}
      }}
    }}
  }}
  consumer {{
    claude {{
      use-skills "demo-skill" "local-skill"
      use-agents "agent-tag" "agent-rev"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), manifest).unwrap();
}

#[test]
fn summon_locks_only_mutable_git_sources() {
    let project = setup();
    setup_local_source(&project);
    let pinned = rev_parse(&project.source_repo, "main");
    git(&project.source_repo, &["tag", "v1.0.0"]);
    write_all_source_kinds_manifest(&project, &pinned);

    let outcome = materialize_with(&project, LockMode::Locked).unwrap();
    assert_eq!(outcome.lock, LockOutcome::Created);

    // Only the branch and tag selectors are locked; the revision pin and the local source stay out.
    let lock = read_lock(&project);
    assert_eq!(lock["version"], 1);
    let entries = lock["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["selector"]["type"], "branch");
    assert_eq!(entries[0]["selector"]["value"], "main");
    assert_eq!(entries[0]["resolved_revision"], pinned.as_str());
    assert_eq!(entries[1]["selector"]["type"], "tag");
    assert_eq!(entries[1]["selector"]["value"], "v1.0.0");
    assert_eq!(entries[1]["resolved_revision"], pinned.as_str());
}

#[test]
fn a_manifest_without_mutable_sources_writes_an_empty_lock() {
    let project = setup();
    setup_local_source(&project);
    write_local_manifest(&project);

    let outcome = materialize_with(&project, LockMode::Locked).unwrap();

    // The lock is written uniformly, so a later frozen run has a record to check against.
    assert_eq!(outcome.lock, LockOutcome::Created);
    let lock = read_lock(&project);
    assert_eq!(lock["entries"].as_array().unwrap().len(), 0);
}

#[test]
fn removing_a_source_prunes_its_lock_entry() {
    let project = setup();
    git(&project.source_repo, &["branch", "other"]);
    let url = format!("file://{}", project.source_repo.display());
    let two_sources = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "agent-main" {{
        git {{
          url "{url}"
          branch "main"
          path "agents/demo-agent.md"
        }}
      }}
      agent "agent-other" {{
        git {{
          url "{url}"
          branch "other"
          path "agents/demo-agent.md"
        }}
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
    fs::write(project.root.join("enozunu.kdl"), &two_sources).unwrap();
    materialize(&project).unwrap();
    assert_eq!(read_lock(&project)["entries"].as_array().unwrap().len(), 2);

    let one_source = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "agent-main" {{
        git {{
          url "{url}"
          branch "main"
          path "agents/demo-agent.md"
        }}
      }}
    }}
  }}
  consumer {{
    claude {{
      use-agents "agent-main"
    }}
  }}
}}
"#
    );
    fs::write(project.root.join("enozunu.kdl"), one_source).unwrap();
    let outcome = materialize_with(&project, LockMode::Locked).unwrap();

    // The lock is rebuilt from what this run resolved, so the removed source's entry is gone.
    assert_eq!(outcome.lock, LockOutcome::Updated);
    let entries = read_lock(&project);
    let entries = entries["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["selector"]["value"], "main");
}

#[test]
fn changing_a_selector_resolves_fresh_and_replaces_the_lock_entry() {
    let project = setup();
    write_manifest(&project);
    materialize(&project).unwrap();

    // Diverge a second branch after locking `main`, then repoint the manifest at it.
    git(
        &project.source_repo,
        &["checkout", "--quiet", "-b", "other"],
    );
    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent on other\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "diverge on other");
    git(&project.source_repo, &["checkout", "--quiet", "main"]);
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
      agent "demo-agent" {{
        git {{
          url "{url}"
          branch "other"
          path "agents/demo-agent.md"
        }}
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

    materialize(&project).unwrap();

    // The new selector has no lock entry, so it resolves fresh even in the default mode.
    assert_eq!(
        fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap(),
        "# demo agent on other\n"
    );
    let lock = read_lock(&project);
    let entries = lock["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["selector"]["value"], "other");
    assert_eq!(
        entries[0]["resolved_revision"],
        rev_parse(&project.source_repo, "other").as_str()
    );
}

#[test]
fn a_locked_branch_resolves_through_the_revision_selector() {
    // A resolver that records every request, so the second run is proven to ask for the locked
    // revision instead of the branch.
    struct CountingResolver {
        inner: CommandGitResolver,
        requests: std::cell::RefCell<Vec<GitResolutionRequest>>,
    }
    impl GitResolver for CountingResolver {
        fn resolve(
            &self,
            request: &GitResolutionRequest,
        ) -> Result<enozunu::git::ResolvedSource, enozunu::git::GitError> {
            self.requests.borrow_mut().push(request.clone());
            self.inner.resolve(request)
        }
    }

    let project = setup();
    let locked = rev_parse(&project.source_repo, "main");
    write_manifest(&project);
    let resolver = CountingResolver {
        inner: CommandGitResolver::new(project.root.join(".enozunu/cache")),
        requests: std::cell::RefCell::new(Vec::new()),
    };

    for _ in 0..2 {
        enozunu::run_materialize(
            &project.root.join("enozunu.kdl"),
            &project.root,
            &resolver,
            LockMode::Locked,
        )
        .unwrap();
    }

    let requests = resolver.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].selector, GitSelector::Branch("main".to_owned()));
    assert_eq!(
        requests[1].selector,
        GitSelector::Revision(CommitSha::parse(&locked).unwrap())
    );
}

#[test]
fn frozen_fails_without_a_lockfile() {
    let project = setup();
    write_manifest(&project);

    let diags = materialize_with(&project, LockMode::Frozen).unwrap_err();

    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::LockOutOfDate)
    );
    // A failed frozen run must not materialize anything or create the lock it said was missing.
    assert!(!project.root.join(".claude").exists());
    assert!(!project.root.join("enozunu.lock.json").exists());
}

#[test]
fn frozen_fails_when_a_mutable_source_is_unlocked() {
    let project = setup();
    write_manifest(&project);
    materialize(&project).unwrap();

    // A second branch source enters the manifest after the lock was written.
    git(&project.source_repo, &["branch", "other"]);
    let url = format!("file://{}", project.source_repo.display());
    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
      skill "demo-skill" {{
        git {{
          url "{url}"
          branch "main"
          path "skills/demo-skill"
        }}
      }}
    }}
    agents {{
      agent "demo-agent" {{
        git {{
          url "{url}"
          branch "other"
          path "agents/demo-agent.md"
        }}
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

    let diags = materialize_with(&project, LockMode::Frozen).unwrap_err();

    let missing: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::LockOutOfDate)
        .collect();
    assert_eq!(missing.len(), 1);
    assert!(missing[0].message.contains("branch `other`"));
    assert!(missing[0].message.contains("has no entry"));
}

#[test]
fn frozen_materializes_from_the_lock_without_writing_it() {
    let project = setup();
    write_manifest(&project);
    materialize(&project).unwrap();
    let lock_before = fs::read_to_string(project.root.join("enozunu.lock.json")).unwrap();

    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent v2\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "advance past the lock");

    let outcome = materialize_with(&project, LockMode::Frozen).unwrap();

    assert_eq!(outcome.lock, LockOutcome::NotWritten);
    assert_eq!(
        fs::read_to_string(project.root.join(".claude/agents/demo-agent.md")).unwrap(),
        "# demo agent\n"
    );
    assert_eq!(
        fs::read_to_string(project.root.join("enozunu.lock.json")).unwrap(),
        lock_before
    );
}

#[test]
fn a_corrupt_lockfile_fails_before_materializing() {
    let project = setup();
    write_manifest(&project);
    fs::write(project.root.join("enozunu.lock.json"), "{ not json").unwrap();

    let diags = materialize(&project).unwrap_err();

    assert!(diags.iter().any(|d| d.code == DiagnosticCode::LockParse));
    assert!(!project.root.join(".claude").exists());
    // The corrupt file must survive untouched for inspection instead of being clobbered.
    assert_eq!(
        fs::read_to_string(project.root.join("enozunu.lock.json")).unwrap(),
        "{ not json"
    );
}

#[test]
fn an_unreachable_locked_revision_suggests_update() {
    let project = setup();
    write_manifest(&project);
    materialize(&project).unwrap();

    // Advance the branch, lock the new tip, then destroy that commit upstream so the locked
    // revision can no longer be fetched.
    fs::write(
        project.source_repo.join("agents/demo-agent.md"),
        "# demo agent v2\n",
    )
    .unwrap();
    commit_all(&project.source_repo, "commit that will vanish");
    materialize_with(&project, LockMode::Update).unwrap();
    git(
        &project.source_repo,
        &["reset", "--quiet", "--hard", "HEAD~1"],
    );
    git(
        &project.source_repo,
        &["reflog", "expire", "--expire=now", "--all"],
    );
    git(&project.source_repo, &["gc", "--quiet", "--prune=now"]);

    let diags = materialize(&project).unwrap_err();

    let unreachable: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::GitResolution)
        .collect();
    assert!(!unreachable.is_empty());
    assert!(
        unreachable
            .iter()
            .all(|d| d.message.contains("enozunu summon --update"))
    );
}
