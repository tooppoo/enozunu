//! Resolves Git source references to concrete commits and exported content trees.
//!
//! Git operations stay behind the `GitResolver` trait so the external `git` command can later be replaced by a library implementation.
//! See docs/design/adr/20260708T075713Z_implement-enozunu-in-rust.md for that decision.
//!
//! Branch, tag, and exact-revision resolution are separate selectors on one request, not one call whose contract is "branch".
//! Keeping them type-distinct prevents an immutable revision from being silently resolved as if it were a branch name, and keeps a tag from being resolved through the branch namespace.
//!
//! The resolver owns its Git cache and checkout layout; callers receive a Git-metadata-free exported content tree.
//! Materialization therefore never reads a resolver cache directory.
//! See docs/design/adr/20260711T144232Z_use-pinned-gist-root-as-skill-artifact-root.md for the export boundary decision.

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
/// The three selectors are deliberately distinct variants so a caller cannot pass a revision where a branch is expected, nor a tag name where a branch name is expected.
/// The type also serves as a resolution and cache key component: a branch whose name looks like a commit id stays distinct from a revision with the same text, and a branch stays distinct from a tag of the same name.
///
/// `Branch` and `Tag` both name a mutable remote ref and resolve to whatever it points at on each run; only `Revision` names one fixed commit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GitSelector {
    Branch(String),
    Tag(String),
    Revision(CommitSha),
}

/// A request to resolve one repository at one selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitResolutionRequest {
    pub url: String,
    pub selector: GitSelector,
}

/// A source resolved to an exact commit and an exported content tree.
///
/// `content_root` holds the resolved commit's clean working-tree content without `.git` or any other Git repository metadata.
/// It is the only filesystem surface callers may read; the resolver's cache and checkout layout stay private behind this boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSource {
    pub commit: String,
    pub content_root: PathBuf,
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
    fn cache_entry(&self, url: &str, selector_kind: &str, selector: &str) -> CacheEntry {
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
        // The `v1` namespace keeps this layout apart from the pre-export layout, which used `cache_root/<key>` itself as the checkout; reusing that path would strand old checkouts and could collide with a repository that tracks a top-level `repo` entry.
        let base = self
            .cache_root
            .join("v1")
            .join(format!("{prefix}-{:016x}", hasher.finish()));
        CacheEntry {
            repo: base.join("repo"),
            export: base.join("export"),
        }
    }

    fn resolve_branch(&self, url: &str, branch: &str) -> Result<ResolvedSource, GitError> {
        let entry = self.cache_entry(url, "branch", branch);
        let dir = &entry.repo;

        if dir.join(".git").exists() {
            // `--` keeps a hostile branch value from being parsed as a git option; validation also rejects leading `-`, and this guards the subprocess boundary directly.
            run_git(
                dir,
                &["fetch", "--quiet", "--depth", "1", "origin", "--", branch],
            )
            .map_err(|e| fetch_error(url, e))?;
            checkout_fetch_head(dir, url)?;
        } else {
            create_parent_dirs(dir)?;
            let dir_str = dir.to_string_lossy().into_owned();
            run_git_anywhere(&[
                "clone", "--quiet", "--depth", "1", "--branch", branch, "--", url, &dir_str,
            ])
            .map_err(|e| fetch_error(url, e))?;
        }

        finish_resolution(dir, url, entry.export)
    }

    /// Resolves the commit a tag currently points at, through the fully-qualified `refs/tags/` namespace.
    ///
    /// A tag is resolved by an explicit `refs/tags/<tag>` refspec rather than the `--branch <name>` form `resolve_branch` uses, so a repository holding both a branch and a tag of one name always resolves the tag here. `--no-tags` keeps the clone from transferring every other tag, since resolution reads the requested tag from `FETCH_HEAD` and never consults a local tag ref.
    ///
    /// The fetch names a source ref with no destination, so the result lands in `FETCH_HEAD` only and no local tag ref is written. That keeps a tag that moved on the remote from needing a forced update, which a local `refs/tags/<tag>` would.
    fn resolve_tag(&self, url: &str, tag: &str) -> Result<ResolvedSource, GitError> {
        let entry = self.cache_entry(url, "tag", tag);
        let dir = &entry.repo;

        if !dir.join(".git").exists() {
            create_parent_dirs(dir)?;
            let dir_str = dir.to_string_lossy().into_owned();
            run_git_anywhere(&[
                "clone",
                "--quiet",
                "--no-checkout",
                "--depth",
                "1",
                "--no-tags",
                "--",
                url,
                &dir_str,
            ])
            .map_err(|e| fetch_error(url, e))?;
        }

        // Validation rejects a tag containing `:`, so the interpolated value cannot split into a two-sided refspec here.
        let tag_ref = format!("refs/tags/{tag}");
        run_git(
            dir,
            &["fetch", "--quiet", "--depth", "1", "origin", "--", &tag_ref],
        )
        .map_err(|e| fetch_error(url, e))?;
        checkout_fetch_head(dir, url)?;

        finish_resolution(dir, url, entry.export)
    }

    fn resolve_revision(
        &self,
        url: &str,
        revision: &CommitSha,
    ) -> Result<ResolvedSource, GitError> {
        let entry = self.cache_entry(url, "revision", revision.as_str());
        let dir = &entry.repo;

        if !dir.join(".git").exists() {
            create_parent_dirs(dir)?;
            let dir_str = dir.to_string_lossy().into_owned();
            // A full clone (no `--depth`) so an older, non-HEAD revision is present in history; a shallow clone would only contain the branch tip.
            run_git_anywhere(&["clone", "--quiet", "--no-checkout", "--", url, &dir_str])
                .map_err(|e| fetch_error(url, e))?;
        }

        // Verify the pinned revision exists before checkout so a missing revision is reported distinctly from a fetch failure.
        // The `^{commit}` peel also rejects an object id that exists but is not a commit.
        let commit_spec = format!("{}^{{commit}}", revision.as_str());
        run_git(dir, &["cat-file", "-e", &commit_spec]).map_err(|e| {
            GitError::RevisionNotFound(format!(
                "revision `{}` not found in `{url}`: {e}",
                revision.as_str()
            ))
        })?;

        run_git(dir, &["checkout", "--quiet", "--detach", revision.as_str()])
            .map_err(|e| fetch_error(url, e))?;
        // The cache must mirror the pinned commit exactly; leftover files would leak into the exported content tree.
        run_git(dir, &["reset", "--quiet", "--hard", "HEAD"]).map_err(|e| fetch_error(url, e))?;
        run_git(dir, &["clean", "--quiet", "-ffdx"]).map_err(|e| fetch_error(url, e))?;

        let head = run_git(dir, &["rev-parse", "HEAD"])
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

        export_content_tree(dir, &entry.export)?;

        Ok(ResolvedSource {
            commit: head,
            content_root: entry.export,
        })
    }
}

/// Checks out whatever the preceding fetch recorded in `FETCH_HEAD`, leaving the working tree an exact mirror of that commit.
///
/// Checkout peels an annotated tag object to its commit, so a subsequent `rev-parse HEAD` reports a commit id for both lightweight and annotated tags.
fn checkout_fetch_head(dir: &Path, url: &str) -> Result<(), GitError> {
    run_git(dir, &["checkout", "--quiet", "--detach", "FETCH_HEAD"])
        .map_err(|e| fetch_error(url, e))?;
    // The cache must mirror the fetched commit exactly; leftover files would leak into the exported content tree.
    run_git(dir, &["reset", "--quiet", "--hard", "HEAD"]).map_err(|e| fetch_error(url, e))?;
    run_git(dir, &["clean", "--quiet", "-ffdx"]).map_err(|e| fetch_error(url, e))?;
    Ok(())
}

fn finish_resolution(dir: &Path, url: &str, export: PathBuf) -> Result<ResolvedSource, GitError> {
    let commit = run_git(dir, &["rev-parse", "HEAD"])
        .map_err(|e| fetch_error(url, e))?
        .trim()
        .to_owned();

    export_content_tree(dir, &export)?;

    Ok(ResolvedSource {
        commit,
        content_root: export,
    })
}

/// One resolver cache slot: the Git checkout and the exported content tree derived from it.
struct CacheEntry {
    repo: PathBuf,
    export: PathBuf,
}

fn create_parent_dirs(dir: &Path) -> Result<(), GitError> {
    let parent = dir.parent().unwrap_or(dir);
    std::fs::create_dir_all(parent)
        .map_err(|e| GitError::Io(format!("failed to create cache directory: {e}")))
}

/// Exports the clean checkout at `repo` to `export` as a content tree without Git repository metadata.
///
/// The export mirrors the clean working tree exactly, minus `.git`: file types are preserved (symlinks stay symlinks) and permissions are copied with each file.
/// No `git archive` semantics such as `export-ignore` or `export-subst` are applied.
/// A previous export is removed first so stale files from an earlier resolution cannot leak into materialization.
fn export_content_tree(repo: &Path, export: &Path) -> Result<(), GitError> {
    if export.symlink_metadata().is_ok() {
        std::fs::remove_dir_all(export)
            .map_err(|e| GitError::Io(format!("failed to clear previous export: {e}")))?;
    }
    std::fs::create_dir_all(export)
        .map_err(|e| GitError::Io(format!("failed to create export directory: {e}")))?;
    copy_tree(repo, export, true)
}

/// Recursively copies a directory tree, skipping the top-level `.git` when `at_root` is set.
fn copy_tree(source: &Path, target: &Path, at_root: bool) -> Result<(), GitError> {
    let io = |e: std::io::Error| GitError::Io(format!("failed to export content tree: {e}"));
    for entry in std::fs::read_dir(source).map_err(io)? {
        let entry = entry.map_err(io)?;
        if at_root && entry.file_name() == ".git" {
            continue;
        }
        let file_type = entry.file_type().map_err(io)?;
        let target_child = target.join(entry.file_name());
        if file_type.is_symlink() {
            copy_symlink(&entry.path(), &target_child).map_err(io)?;
        } else if file_type.is_dir() {
            std::fs::create_dir(&target_child).map_err(io)?;
            copy_tree(&entry.path(), &target_child, false)?;
        } else {
            // `fs::copy` carries permissions with the file, so the executable bit survives the export.
            std::fs::copy(entry.path(), &target_child).map_err(io)?;
        }
    }
    Ok(())
}

// Symlinks are reproduced as symlinks so downstream artifact validation sees the same file types as the working tree; resolving them here would silently weaken the symlink policy.
#[cfg(unix)]
fn copy_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
    let link = std::fs::read_link(source)?;
    std::os::unix::fs::symlink(link, target)
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(format!(
        "cannot export symlink {} on this platform",
        source.display()
    )))
}

impl GitResolver for CommandGitResolver {
    fn resolve(&self, request: &GitResolutionRequest) -> Result<ResolvedSource, GitError> {
        match &request.selector {
            GitSelector::Branch(branch) => self.resolve_branch(&request.url, branch),
            GitSelector::Tag(tag) => self.resolve_tag(&request.url, tag),
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
