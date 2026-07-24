//! Pipeline and CLI-visible tests for Gist Skill and agent sources.
//!
//! The remote-access boundary is replaced with a deterministic fake Git transport, so these cover everything except the actual request to a live Gist:
//! URL construction, exact-revision selection, de-duplication by `(id, revision)` across artifact kinds, root/file artifact validation, materialization targets, Gist runtime origin, typed provenance, and diagnostic-code propagation.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use enozunu::diagnostics::DiagnosticCode;
use enozunu::git::{GitError, GitResolutionRequest, GitResolver, GitSelector, ResolvedSource};
use enozunu::{LockMode, ResolvedOrigin, run_materialize};

const GIST_ID: &str = "2decf6c462d9b4418f2";
const REVISION: &str = "468aac8caed5f0c3b859b8286968e2c78e2b8760";

/// A fake Git transport that records requests and returns a scripted outcome, never touching the network.
struct FakeTransport {
    content_root: PathBuf,
    outcome: Result<(), GitError>,
    requests: RefCell<Vec<GitResolutionRequest>>,
}

impl FakeTransport {
    fn ok(content_root: PathBuf) -> Self {
        Self {
            content_root,
            outcome: Ok(()),
            requests: RefCell::new(Vec::new()),
        }
    }

    fn failing(outcome: GitError) -> Self {
        Self {
            content_root: PathBuf::new(),
            outcome: Err(outcome),
            requests: RefCell::new(Vec::new()),
        }
    }
}

impl GitResolver for FakeTransport {
    fn resolve(&self, request: &GitResolutionRequest) -> Result<ResolvedSource, GitError> {
        self.requests.borrow_mut().push(request.clone());
        self.outcome.clone()?;
        Ok(ResolvedSource {
            // The production resolver verifies HEAD equals the pinned revision, so the fake reports that same commit.
            commit: REVISION.to_owned(),
            content_root: self.content_root.clone(),
        })
    }
}

/// Prepares an exported content root holding the given `<name, content>` files.
fn content_with(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let content = tmp.path().join("content");
    fs::create_dir_all(&content).unwrap();
    for (name, content_text) in files {
        let path = content.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content_text).unwrap();
    }
    (tmp, content)
}

/// Writes a manifest declaring gist skills (by name) and gist agents (by `(name, file)`) into a fresh project directory.
fn project_with(skills: &[&str], agents: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let mut skill_decls = String::new();
    let mut skill_uses = String::new();
    for name in skills {
        skill_decls.push_str(&format!(
            r#"      skill "{name}" {{
        gist {{
          id "{GIST_ID}"
          revision "{REVISION}"
        }}
      }}
"#
        ));
        skill_uses.push_str(&format!(" \"{name}\""));
    }

    let mut agent_decls = String::new();
    let mut agent_uses = String::new();
    for (name, file) in agents {
        agent_decls.push_str(&format!(
            r#"      agent "{name}" {{
        gist {{
          id "{GIST_ID}"
          revision "{REVISION}"
          file "{file}"
        }}
      }}
"#
        ));
        agent_uses.push_str(&format!(" \"{name}\""));
    }

    let use_skills = if skills.is_empty() {
        String::new()
    } else {
        format!("      use-skills{skill_uses}\n")
    };
    let use_agents = if agents.is_empty() {
        String::new()
    } else {
        format!("      use-agents{agent_uses}\n")
    };

    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    skills {{
{skill_decls}    }}
    agents {{
{agent_decls}    }}
  }}
  consumer {{
    claude {{
{use_skills}{use_agents}    }}
  }}
}}
"#
    );
    fs::write(root.join("enozunu.kdl"), manifest).unwrap();
    (tmp, root)
}

fn project_with_agents(agents: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    project_with(&[], agents)
}

fn provenance(root: &Path) -> serde_json::Value {
    let text = fs::read_to_string(root.join(".enozunu/provenance.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn materializes_a_gist_agent_via_the_git_transport_boundary() {
    let (_c, content) = content_with(&[("shell-script-reviewer.md", "# reviewer\n")]);
    let (_p, root) = project_with_agents(&[("shell-script-reviewer", "shell-script-reviewer.md")]);
    let transport = FakeTransport::ok(content);

    let entries = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap()
    .entries;

    // The agent file is materialized to the Claude-native agent path.
    assert_eq!(
        fs::read_to_string(root.join(".claude/agents/shell-script-reviewer.md")).unwrap(),
        "# reviewer\n"
    );

    // The transport is driven with the Gist remote URL and an exact-revision selector, not a branch.
    let requests = transport.requests.borrow();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url,
        "https://gist.github.com/2decf6c462d9b4418f2.git"
    );
    assert!(matches!(requests[0].selector, GitSelector::Revision(_)));

    // The runtime origin keeps the Gist identity separate from an ordinary Git origin.
    match &entries[0].origin {
        ResolvedOrigin::Gist { id, revision } => {
            assert_eq!(id, GIST_ID);
            assert_eq!(revision, REVISION);
        }
        other => panic!("expected a gist origin, got {other:?}"),
    }
    assert_eq!(
        entries[0].origin.describe(),
        format!("gist: {GIST_ID}@{REVISION}")
    );
}

#[test]
fn materializes_a_gist_skill_tree_from_the_revision_root() {
    let (_c, content) = content_with(&[
        ("SKILL.md", "# semantic line breaks\n"),
        ("references/example.md", "supporting file\n"),
    ]);
    let (_p, root) = project_with(&["semantic-line-breaks"], &[]);
    let transport = FakeTransport::ok(content);

    let entries = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap()
    .entries;

    // The whole revision root, including nested supporting files, lands under the Claude-native skill path.
    assert_eq!(
        fs::read_to_string(root.join(".claude/skills/semantic-line-breaks/SKILL.md")).unwrap(),
        "# semantic line breaks\n"
    );
    assert_eq!(
        fs::read_to_string(root.join(".claude/skills/semantic-line-breaks/references/example.md"))
            .unwrap(),
        "supporting file\n"
    );

    // The transport is driven with the same Gist remote and exact-revision selector as an agent Gist.
    let requests = transport.requests.borrow();
    assert_eq!(requests.len(), 1);
    assert!(matches!(requests[0].selector, GitSelector::Revision(_)));

    // The runtime origin uses the same Gist identity rendering as agent Gists.
    assert_eq!(
        entries[0].origin.describe(),
        format!("gist: {GIST_ID}@{REVISION}")
    );
}

#[test]
fn records_typed_gist_provenance() {
    let (_c, content) = content_with(&[("reviewer.md", "# reviewer\n")]);
    let (_p, root) = project_with_agents(&[("reviewer", "reviewer.md")]);
    let transport = FakeTransport::ok(content);

    run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap();

    let record = provenance(&root);
    let source = &record["entries"][0]["source"];
    assert_eq!(source["type"], "gist");
    assert_eq!(source["id"], GIST_ID);
    assert_eq!(source["revision"], REVISION);
    assert_eq!(source["file"], "reviewer.md");
    // A Gist is never represented as a Git source merely because Git transport was used.
    assert!(source.get("url").is_none());
    assert!(source.get("branch").is_none());
}

#[test]
fn records_skill_gist_provenance_without_a_file_key() {
    let (_c, content) = content_with(&[("SKILL.md", "# demo\n")]);
    let (_p, root) = project_with(&["demo"], &[]);
    let transport = FakeTransport::ok(content);

    run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap();

    let record = provenance(&root);
    let entry = &record["entries"][0];
    assert_eq!(entry["kind"], "skill");
    assert_eq!(entry["target_path"], ".claude/skills/demo");
    let source = &entry["source"];
    assert_eq!(source["type"], "gist");
    assert_eq!(source["id"], GIST_ID);
    assert_eq!(source["revision"], REVISION);
    // A Skill Gist materializes the revision root, so no `file` is recorded.
    assert!(source.get("file").is_none());
    assert!(source.get("url").is_none());
}

#[test]
fn deduplicates_one_checkout_across_files_of_the_same_revision() {
    let (_c, content) = content_with(&[("a.md", "# a\n"), ("b.md", "# b\n")]);
    let (_p, root) = project_with_agents(&[("agent-a", "a.md"), ("agent-b", "b.md")]);
    let transport = FakeTransport::ok(content);

    run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap();

    // Two agents select different files from one `(id, revision)`, so the transport resolves it exactly once.
    assert_eq!(transport.requests.borrow().len(), 1);
    assert_eq!(
        fs::read_to_string(root.join(".claude/agents/agent-a.md")).unwrap(),
        "# a\n"
    );
    assert_eq!(
        fs::read_to_string(root.join(".claude/agents/agent-b.md")).unwrap(),
        "# b\n"
    );
}

#[test]
fn deduplicates_one_resolution_across_a_skill_and_an_agent() {
    let (_c, content) = content_with(&[("SKILL.md", "# demo\n"), ("reviewer.md", "# reviewer\n")]);
    let (_p, root) = project_with(&["demo"], &[("reviewer", "reviewer.md")]);
    let transport = FakeTransport::ok(content);

    run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap();

    // A Skill and an agent referencing the same `(id, revision)` share one resolution and one exported content tree.
    assert_eq!(transport.requests.borrow().len(), 1);
    assert_eq!(
        fs::read_to_string(root.join(".claude/skills/demo/SKILL.md")).unwrap(),
        "# demo\n"
    );
    assert_eq!(
        fs::read_to_string(root.join(".claude/agents/reviewer.md")).unwrap(),
        "# reviewer\n"
    );
}

#[test]
fn deduplicates_git_sources_by_url_and_branch_within_a_run() {
    // Regression guard for the shared resolver boundary: Git sources keep their run-level `(url, branch)` de-duplication.
    let (_c, content) = content_with(&[
        ("skills/demo/SKILL.md", "# demo\n"),
        ("agents/reviewer.md", "# reviewer\n"),
    ]);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let manifest = r#"
enozunu config-version=1 {
  provider {
    skills {
      skill "demo" {
        git { url "https://example.com/repo"; branch "main"; path "skills/demo" }
      }
    }
    agents {
      agent "reviewer" {
        git { url "https://example.com/repo"; branch "main"; path "agents/reviewer.md" }
      }
    }
  }
  consumer {
    claude {
      use-skills "demo"
      use-agents "reviewer"
    }
  }
}
"#;
    fs::write(root.join("enozunu.kdl"), manifest).unwrap();
    let transport = FakeTransport::ok(content);

    run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap();

    assert_eq!(transport.requests.borrow().len(), 1);
    assert!(root.join(".claude/skills/demo/SKILL.md").is_file());
    assert!(root.join(".claude/agents/reviewer.md").is_file());
}

#[test]
fn rejects_a_missing_gist_file_with_source_path_not_found() {
    let (_c, content) = content_with(&[("present.md", "# present\n")]);
    let (_p, root) = project_with_agents(&[("reviewer", "absent.md")]);
    let transport = FakeTransport::ok(content);

    let diags = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap_err();

    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::SourcePathNotFound)
    );
    assert!(!root.join(".claude").exists());
}

#[test]
fn rejects_a_gist_skill_root_without_skill_md() {
    let (_c, content) = content_with(&[("README.md", "# not a skill\n")]);
    let (_p, root) = project_with(&["demo"], &[]);
    let transport = FakeTransport::ok(content);

    let diags = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap_err();

    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::ArtifactShape)
    );
    // Artifact validation fails before the first target write.
    assert!(!root.join(".claude").exists());
}

#[test]
#[cfg(unix)]
fn rejects_a_symlink_inside_a_gist_skill_tree() {
    use std::os::unix::fs::symlink;
    let (_c, content) = content_with(&[("SKILL.md", "# demo\n")]);
    let outside = content.parent().unwrap().join("outside.md");
    fs::write(&outside, "# outside\n").unwrap();
    symlink("../outside.md", content.join("link.md")).unwrap();
    let (_p, root) = project_with(&["demo"], &[]);
    let transport = FakeTransport::ok(content);

    let diags = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap_err();

    assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnsafePath));
    assert!(!root.join(".claude").exists());
}

#[test]
fn propagates_a_gist_fetch_failure() {
    let (_p, root) = project_with_agents(&[("reviewer", "reviewer.md")]);
    let transport = FakeTransport::failing(GitError::Fetch("unreachable".to_owned()));

    let diags = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap_err();

    // A Gist transport failure is classified as GistFetch, never GitResolution.
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::GistFetch));
    assert!(
        diags
            .iter()
            .all(|d| d.code != DiagnosticCode::GitResolution)
    );
}

#[test]
fn propagates_a_gist_fetch_failure_for_a_skill() {
    let (_p, root) = project_with(&["demo"], &[]);
    let transport = FakeTransport::failing(GitError::Fetch("unreachable".to_owned()));

    let diags = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap_err();

    assert!(diags.iter().any(|d| d.code == DiagnosticCode::GistFetch));
    assert!(!root.join(".claude").exists());
}

#[test]
fn propagates_a_gist_revision_not_found_failure() {
    let (_p, root) = project_with_agents(&[("reviewer", "reviewer.md")]);
    let transport = FakeTransport::failing(GitError::RevisionNotFound("absent".to_owned()));

    let diags = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap_err();

    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::GistRevisionNotFound)
    );
}

#[test]
fn rejects_a_gist_file_that_is_not_a_regular_file() {
    // `file` points at a directory inside the exported content rather than an agent file.
    let (_c, content) = content_with(&[]);
    fs::create_dir_all(content.join("a-directory")).unwrap();
    let (_p, root) = project_with_agents(&[("reviewer", "a-directory")]);
    let transport = FakeTransport::ok(content);

    let diags = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap_err();

    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::ArtifactShape)
    );
    assert!(!root.join(".claude").exists());
}

#[test]
#[cfg(unix)]
fn accepts_a_gist_file_symlinked_to_a_regular_file_inside_the_checkout() {
    use std::os::unix::fs::symlink;
    // `file` is a symlink whose canonical target is a regular file that stays inside the exported content.
    let (_c, content) = content_with(&[("target.md", "# linked reviewer\n")]);
    symlink("target.md", content.join("link.md")).unwrap();
    let (_p, root) = project_with_agents(&[("reviewer", "link.md")]);
    let transport = FakeTransport::ok(content);

    run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(root.join(".claude/agents/reviewer.md")).unwrap(),
        "# linked reviewer\n"
    );
}

#[test]
#[cfg(unix)]
fn rejects_a_gist_file_symlink_that_escapes_the_checkout() {
    use std::os::unix::fs::symlink;
    // `file` is a symlink whose target is a real file outside the exported content, so it escapes containment.
    let (_c, content) = content_with(&[]);
    let outside = content.parent().unwrap().join("outside.md");
    fs::write(&outside, "# outside\n").unwrap();
    symlink("../outside.md", content.join("escape.md")).unwrap();
    let (_p, root) = project_with_agents(&[("reviewer", "escape.md")]);
    let transport = FakeTransport::ok(content);

    let diags = run_materialize(
        &root.join("enozunu.kdl"),
        &root,
        &transport,
        LockMode::Locked,
    )
    .unwrap_err();

    assert!(diags.iter().any(|d| d.code == DiagnosticCode::UnsafePath));
    assert!(!root.join(".claude").exists());
    // The escaped source must survive the rejected run untouched.
    assert_eq!(fs::read_to_string(&outside).unwrap(), "# outside\n");
}
