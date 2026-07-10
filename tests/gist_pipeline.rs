//! Pipeline and CLI-visible tests for Gist agent sources.
//!
//! The remote-access boundary is replaced with a deterministic fake Git transport, so these cover everything except the actual request to a live Gist:
//! URL construction, exact-revision selection, de-duplication by `(id, revision)`, selected-file validation, materialization target, Gist runtime origin, typed provenance, and diagnostic-code propagation.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use enozunu::diagnostics::DiagnosticCode;
use enozunu::git::{GitError, GitResolutionRequest, GitResolver, GitSelector, ResolvedSource};
use enozunu::{ResolvedOrigin, run_materialize};

const GIST_ID: &str = "2decf6c462d9b4418f2";
const REVISION: &str = "468aac8caed5f0c3b859b8286968e2c78e2b8760";

/// A fake Git transport that records requests and returns a scripted outcome, never touching the network.
struct FakeTransport {
    checkout: PathBuf,
    outcome: Result<(), GitError>,
    requests: RefCell<Vec<GitResolutionRequest>>,
}

impl FakeTransport {
    fn ok(checkout: PathBuf) -> Self {
        Self {
            checkout,
            outcome: Ok(()),
            requests: RefCell::new(Vec::new()),
        }
    }

    fn failing(outcome: GitError) -> Self {
        Self {
            checkout: PathBuf::new(),
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
            checkout_dir: self.checkout.clone(),
        })
    }
}

/// Prepares a checkout directory holding the given `<name, content>` files.
fn checkout_with(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let checkout = tmp.path().join("checkout");
    fs::create_dir_all(&checkout).unwrap();
    for (name, content) in files {
        fs::write(checkout.join(name), content).unwrap();
    }
    (tmp, checkout)
}

/// Writes a manifest declaring one gist agent per `(name, file)` entry into a fresh project directory.
fn project_with_agents(agents: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    fs::create_dir_all(&root).unwrap();

    let mut decls = String::new();
    let mut uses = String::new();
    for (name, file) in agents {
        decls.push_str(&format!(
            r#"      agent "{name}" {{
        gist {{
          id "{GIST_ID}"
          revision "{REVISION}"
          file "{file}"
        }}
      }}
"#
        ));
        uses.push_str(&format!(" \"{name}\""));
    }

    let manifest = format!(
        r#"
enozunu config-version=1 {{
  provider {{
    agents {{
{decls}    }}
  }}
  consumer {{
    claude {{
      use-agents{uses}
    }}
  }}
}}
"#
    );
    fs::write(root.join("enozunu.kdl"), manifest).unwrap();
    (tmp, root)
}

fn provenance(root: &Path) -> serde_json::Value {
    let text = fs::read_to_string(root.join(".enozunu/provenance.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn materializes_a_gist_agent_via_the_git_transport_boundary() {
    let (_c, checkout) = checkout_with(&[("shell-script-reviewer.md", "# reviewer\n")]);
    let (_p, root) = project_with_agents(&[("shell-script-reviewer", "shell-script-reviewer.md")]);
    let transport = FakeTransport::ok(checkout);

    let entries = run_materialize(&root.join("enozunu.kdl"), &root, &transport).unwrap();

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
fn records_typed_gist_provenance() {
    let (_c, checkout) = checkout_with(&[("reviewer.md", "# reviewer\n")]);
    let (_p, root) = project_with_agents(&[("reviewer", "reviewer.md")]);
    let transport = FakeTransport::ok(checkout);

    run_materialize(&root.join("enozunu.kdl"), &root, &transport).unwrap();

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
fn deduplicates_one_checkout_across_files_of_the_same_revision() {
    let (_c, checkout) = checkout_with(&[("a.md", "# a\n"), ("b.md", "# b\n")]);
    let (_p, root) = project_with_agents(&[("agent-a", "a.md"), ("agent-b", "b.md")]);
    let transport = FakeTransport::ok(checkout);

    run_materialize(&root.join("enozunu.kdl"), &root, &transport).unwrap();

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
fn rejects_a_missing_gist_file_with_source_path_not_found() {
    let (_c, checkout) = checkout_with(&[("present.md", "# present\n")]);
    let (_p, root) = project_with_agents(&[("reviewer", "absent.md")]);
    let transport = FakeTransport::ok(checkout);

    let diags = run_materialize(&root.join("enozunu.kdl"), &root, &transport).unwrap_err();

    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::SourcePathNotFound)
    );
    assert!(!root.join(".claude").exists());
}

#[test]
fn propagates_a_gist_fetch_failure() {
    let (_p, root) = project_with_agents(&[("reviewer", "reviewer.md")]);
    let transport = FakeTransport::failing(GitError::Fetch("unreachable".to_owned()));

    let diags = run_materialize(&root.join("enozunu.kdl"), &root, &transport).unwrap_err();

    // A Gist transport failure is classified as GistFetch, never GitResolution.
    assert!(diags.iter().any(|d| d.code == DiagnosticCode::GistFetch));
    assert!(
        diags
            .iter()
            .all(|d| d.code != DiagnosticCode::GitResolution)
    );
}

#[test]
fn propagates_a_gist_revision_not_found_failure() {
    let (_p, root) = project_with_agents(&[("reviewer", "reviewer.md")]);
    let transport = FakeTransport::failing(GitError::RevisionNotFound("absent".to_owned()));

    let diags = run_materialize(&root.join("enozunu.kdl"), &root, &transport).unwrap_err();

    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::GistRevisionNotFound)
    );
}
