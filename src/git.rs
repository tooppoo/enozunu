//! Resolves Git source references to concrete commits and local checkouts.
//!
//! Git operations stay behind the `GitResolver` trait so the external `git` command can later be replaced by a library implementation.
//! See docs/design/adr/20260708T075713Z_implement-enozunu-in-rust.md for that decision.
//!
//! Branch resolution and exact-revision resolution are separate selectors on one request, not one call whose contract is "branch".
//! Keeping them type-distinct prevents an immutable revision from being silently resolved as if it were a branch name.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A validated Git commit SHA-1: exactly 40 lowercase ASCII hexadecimal characters.
///
/// v0 pins revisions to full SHA-1 object IDs. Abbreviated, uppercase, whitespace-padded, and SHA-256 forms are rejected at construction, so a `CommitSha` value always names one exact object.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommitSha(String);

impl CommitSha {
    /// Parses `raw` as a full SHA-1 commit id, returning `None` for any non-canonical form.
    pub fn parse(raw: &str) -> Option<Self> {
        let canonical = raw.len() == 40 && raw.bytes().all(is_lower_hex);
        canonical.then(|| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which commit of a repository to resolve.
///
/// The two selectors are deliberately distinct types so a caller cannot pass a revision where a branch is expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitSelector {
    Branch(String),
    Revision(CommitSha),
}

/// A request to resolve one repository at one selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitResolutionRequest {
    pub url: String,
    pub selector: GitSelector,
}

/// A source repository checked out at a resolved commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSource {
    pub commit: String,
    pub checkout_dir: PathBuf,
}

/// A resolution failure, kept transport-neutral so each caller classifies it in its own diagnostic vocabulary.
///
/// The resolver knows whether it failed to reach the remote or failed to find the requested revision; only the caller knows whether the source is a Git source or a Gist source. Returning the distinction rather than a finished diagnostic lets a Gist caller map `Fetch` to `GistFetch` while a Git caller maps it to `GitResolution`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// The remote could not be reached or the selector could not be fetched.
    Fetch(String),
    /// The requested revision does not exist in the fetched repository.
    RevisionNotFound(String),
    /// A local filesystem operation failed during resolution.
    Io(String),
}

pub trait GitResolver {
    /// Resolves `request` to a commit and returns a local checkout of that commit.
    fn resolve(&self, request: &GitResolutionRequest) -> Result<ResolvedSource, GitError>;
}

/// Resolves sources with the external `git` command, caching checkouts under `cache_root`.
pub struct CommandGitResolver {
    cache_root: PathBuf,
}

impl CommandGitResolver {
    pub fn new(cache_root: impl Into<PathBuf>) -> Self {
        Self {
            cache_root: cache_root.into(),
        }
    }

    // The cache key must include the selector: each (url, selector) pair keeps its own checkout, and a shared checkout would let the last resolved selector silently overwrite content another `ResolvedSource` still points at.
    // The selector kind is part of the key so a branch named like a SHA cannot collide with a revision of the same text.
    fn cache_dir(&self, url: &str, selector_kind: &str, selector: &str) -> PathBuf {
        // The readable prefix aids debugging; the hash disambiguates keys that sanitize to the same prefix.
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        selector_kind.hash(&mut hasher);
        selector.hash(&mut hasher);
        let sanitized: String = url
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let prefix: String = sanitized
            .trim_matches('-')
            .chars()
            .rev()
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        self.cache_root
            .join(format!("{prefix}-{:016x}", hasher.finish()))
    }

    fn resolve_branch(&self, url: &str, branch: &str) -> Result<ResolvedSource, GitError> {
        let dir = self.cache_dir(url, "branch", branch);

        if dir.join(".git").exists() {
            // `--` keeps a hostile branch value from being parsed as a git option; validation also rejects leading `-`, and this guards the subprocess boundary directly.
            run_git(
                &dir,
                &["fetch", "--quiet", "--depth", "1", "origin", "--", branch],
            )
            .map_err(|e| fetch_error(url, e))?;
            run_git(&dir, &["checkout", "--quiet", "--detach", "FETCH_HEAD"])
                .map_err(|e| fetch_error(url, e))?;
            // The cache must mirror the fetched commit exactly; leftover files would leak into materialized output.
            run_git(&dir, &["reset", "--quiet", "--hard", "HEAD"])
                .map_err(|e| fetch_error(url, e))?;
            run_git(&dir, &["clean", "--quiet", "-ffdx"]).map_err(|e| fetch_error(url, e))?;
        } else {
            self.create_cache_root()?;
            let dir_str = dir.to_string_lossy().into_owned();
            run_git_anywhere(&[
                "clone", "--quiet", "--depth", "1", "--branch", branch, "--", url, &dir_str,
            ])
            .map_err(|e| fetch_error(url, e))?;
        }

        let commit = run_git(&dir, &["rev-parse", "HEAD"])
            .map_err(|e| fetch_error(url, e))?
            .trim()
            .to_owned();

        Ok(ResolvedSource {
            commit,
            checkout_dir: dir,
        })
    }

    fn resolve_revision(
        &self,
        url: &str,
        revision: &CommitSha,
    ) -> Result<ResolvedSource, GitError> {
        let dir = self.cache_dir(url, "revision", revision.as_str());

        if !dir.join(".git").exists() {
            self.create_cache_root()?;
            let dir_str = dir.to_string_lossy().into_owned();
            // A full clone (no `--depth`) so an older, non-HEAD revision is present in history; a shallow clone would only contain the branch tip.
            run_git_anywhere(&["clone", "--quiet", "--no-checkout", "--", url, &dir_str])
                .map_err(|e| fetch_error(url, e))?;
        }

        // Verify the pinned revision exists before checkout so a missing revision is reported distinctly from a fetch failure.
        // The `^{commit}` peel also rejects an object id that exists but is not a commit.
        let commit_spec = format!("{}^{{commit}}", revision.as_str());
        run_git(&dir, &["cat-file", "-e", &commit_spec]).map_err(|e| {
            GitError::RevisionNotFound(format!(
                "revision `{}` not found in `{url}`: {e}",
                revision.as_str()
            ))
        })?;

        run_git(
            &dir,
            &["checkout", "--quiet", "--detach", revision.as_str()],
        )
        .map_err(|e| fetch_error(url, e))?;
        // The cache must mirror the pinned commit exactly; leftover files would leak into materialized output.
        run_git(&dir, &["reset", "--quiet", "--hard", "HEAD"]).map_err(|e| fetch_error(url, e))?;
        run_git(&dir, &["clean", "--quiet", "-ffdx"]).map_err(|e| fetch_error(url, e))?;

        let head = run_git(&dir, &["rev-parse", "HEAD"])
            .map_err(|e| fetch_error(url, e))?
            .trim()
            .to_owned();
        // The checkout target was the exact revision, so a mismatch means the requested object was not what got materialized; treat it as an unresolved revision rather than reporting a bogus commit.
        if head != revision.as_str() {
            return Err(GitError::RevisionNotFound(format!(
                "resolved HEAD `{head}` does not match requested revision `{}` for `{url}`",
                revision.as_str()
            )));
        }

        Ok(ResolvedSource {
            commit: head,
            checkout_dir: dir,
        })
    }

    fn create_cache_root(&self) -> Result<(), GitError> {
        std::fs::create_dir_all(&self.cache_root)
            .map_err(|e| GitError::Io(format!("failed to create cache directory: {e}")))
    }
}

impl GitResolver for CommandGitResolver {
    fn resolve(&self, request: &GitResolutionRequest) -> Result<ResolvedSource, GitError> {
        match &request.selector {
            GitSelector::Branch(branch) => self.resolve_branch(&request.url, branch),
            GitSelector::Revision(revision) => self.resolve_revision(&request.url, revision),
        }
    }
}

fn is_lower_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}

fn fetch_error(url: &str, stderr: String) -> GitError {
    GitError::Fetch(format!("git failed for `{url}`: {stderr}"))
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(args);
    run(command)
}

fn run_git_anywhere(args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.args(args);
    run(command)
}

/// Runs a prepared git command, returning stdout on success and the trimmed stderr (or the spawn error) on failure.
///
/// The failure text stays context-free here; callers wrap it into the `GitError` variant that fits the operation.
fn run(mut command: Command) -> Result<String, String> {
    // Resolution must fail instead of blocking on an interactive credential prompt.
    command.env("GIT_TERMINAL_PROMPT", "0");

    let output = command
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_sha_accepts_exactly_40_lowercase_hex() {
        let sha = "468aac8caed5f0c3b859b8286968e2c78e2b8760";
        assert_eq!(sha.len(), 40);
        assert_eq!(CommitSha::parse(sha).unwrap().as_str(), sha);
    }

    #[test]
    fn commit_sha_rejects_non_canonical_forms() {
        let cases = [
            "468aac8",                                   // abbreviated
            "468AAC8CAED5F0C3B859B8286968E2C78E2B8760",  // uppercase
            " 468aac8caed5f0c3b859b8286968e2c78e2b876",  // whitespace-padded
            "468aac8caed5f0c3b859b8286968e2c78e2b8760a", // 41 chars
            // 64-char SHA-256 object id
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "main",
            "",
        ];
        for case in cases {
            assert!(CommitSha::parse(case).is_none(), "must reject `{case}`");
        }
    }
}
