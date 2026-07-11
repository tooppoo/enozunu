//! End-to-end pipeline tests against local Git repositories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use enozunu::diagnostics::DiagnosticCode;
use enozunu::git::{CommandGitResolver, CommitSha, GitResolutionRequest, GitResolver, GitSelector};

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
        assert_eq!(entry["source"]["type"], "git");
        assert_eq!(entry["source"]["resolved_revision"], head.as_str());
        assert_eq!(entry["source"]["branch"], "main");
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
