//! Integration tests for exact-revision Git resolution against a local Git repository.
//!
//! These exercise the production `CommandGitResolver` through real `git`, without touching a live Gist or the network.
//! The pinned revision is deliberately an older commit that is not the branch tip, so a resolver that ignored the requested revision and checked out the default HEAD would fail these tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use enozunu::git::{
    CommandGitResolver, CommitSha, GitError, GitResolutionRequest, GitResolver, GitSelector,
};

struct Fixture {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    cache: PathBuf,
    old_commit: String,
    head_commit: String,
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

fn rev_parse(dir: &Path, rev: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", rev])
        .output()
        .expect("failed to run git rev-parse");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Builds a repository with two commits so the first is an older, non-HEAD revision.
fn setup() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let cache = tmp.path().join("cache");
    fs::create_dir_all(&repo).unwrap();

    git(&repo, &["init", "--quiet", "--initial-branch", "main"]);
    fs::write(repo.join("agent.md"), "# agent v1\n").unwrap();
    git(&repo, &["add", "--all"]);
    git(&repo, &["commit", "--quiet", "-m", "v1"]);
    let old_commit = rev_parse(&repo, "HEAD");

    fs::write(repo.join("agent.md"), "# agent v2\n").unwrap();
    git(&repo, &["add", "--all"]);
    git(&repo, &["commit", "--quiet", "-m", "v2"]);
    let head_commit = rev_parse(&repo, "HEAD");

    assert_ne!(old_commit, head_commit);

    Fixture {
        _tmp: tmp,
        repo,
        cache,
        old_commit,
        head_commit,
    }
}

fn file_url(repo: &Path) -> String {
    format!("file://{}", repo.display())
}

#[test]
fn resolves_an_exact_non_head_revision() {
    let fixture = setup();
    let resolver = CommandGitResolver::new(&fixture.cache);
    let request = GitResolutionRequest {
        url: file_url(&fixture.repo),
        selector: GitSelector::Revision(CommitSha::parse(&fixture.old_commit).unwrap()),
    };

    let resolved = resolver.resolve(&request).unwrap();

    // The checkout must reflect the pinned older revision, not the branch tip.
    assert_eq!(resolved.commit, fixture.old_commit);
    assert_eq!(
        rev_parse(&resolved.checkout_dir, "HEAD"),
        fixture.old_commit
    );
    assert_eq!(
        fs::read_to_string(resolved.checkout_dir.join("agent.md")).unwrap(),
        "# agent v1\n"
    );
}

#[test]
fn reports_revision_not_found_for_a_missing_revision() {
    let fixture = setup();
    let resolver = CommandGitResolver::new(&fixture.cache);
    // A syntactically valid SHA that does not exist in the repository.
    let missing = "0000000000000000000000000000000000000000";
    let request = GitResolutionRequest {
        url: file_url(&fixture.repo),
        selector: GitSelector::Revision(CommitSha::parse(missing).unwrap()),
    };

    let error = resolver.resolve(&request).unwrap_err();

    assert!(
        matches!(error, GitError::RevisionNotFound(_)),
        "expected RevisionNotFound, got {error:?}"
    );
}

#[test]
fn branch_and_revision_selectors_resolve_distinct_commits() {
    let fixture = setup();
    let resolver = CommandGitResolver::new(&fixture.cache);

    let branch = resolver
        .resolve(&GitResolutionRequest {
            url: file_url(&fixture.repo),
            selector: GitSelector::Branch("main".to_owned()),
        })
        .unwrap();
    let revision = resolver
        .resolve(&GitResolutionRequest {
            url: file_url(&fixture.repo),
            selector: GitSelector::Revision(CommitSha::parse(&fixture.old_commit).unwrap()),
        })
        .unwrap();

    // The branch selector follows the tip; the revision selector pins the older commit. They must not collapse onto one checkout.
    assert_eq!(branch.commit, fixture.head_commit);
    assert_eq!(revision.commit, fixture.old_commit);
    assert_ne!(branch.checkout_dir, revision.checkout_dir);
}
