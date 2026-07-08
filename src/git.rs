//! Resolves Git source references to concrete commits and local checkouts.
//!
//! Git operations stay behind the `GitResolver` trait so the external `git` command can later be replaced by a library implementation.
//! See docs/adr/20260708T075713Z_implement-enozunu-in-rust.md for that decision.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::diagnostics::{Diagnostic, DiagnosticCode};

/// A source repository checked out at a resolved commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSource {
    pub commit: String,
    pub checkout_dir: PathBuf,
}

pub trait GitResolver {
    /// Resolves `branch` of the repository at `url` to a commit and returns a local checkout of that commit.
    fn resolve(&self, url: &str, branch: &str) -> Result<ResolvedSource, Diagnostic>;
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

    // The cache key must include the branch: each (url, branch) pair keeps its own checkout, and a shared checkout would let the last resolved branch silently overwrite content another `ResolvedSource` still points at.
    fn cache_dir(&self, url: &str, branch: &str) -> PathBuf {
        // The readable prefix aids debugging; the hash disambiguates keys that sanitize to the same prefix.
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        branch.hash(&mut hasher);
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
}

impl GitResolver for CommandGitResolver {
    fn resolve(&self, url: &str, branch: &str) -> Result<ResolvedSource, Diagnostic> {
        let dir = self.cache_dir(url, branch);

        if dir.join(".git").exists() {
            // `--` keeps a hostile branch value from being parsed as a git option; validation also rejects leading `-`, and this guards the subprocess boundary directly.
            run_git(
                &dir,
                &["fetch", "--quiet", "--depth", "1", "origin", "--", branch],
                url,
            )?;
            run_git(
                &dir,
                &["checkout", "--quiet", "--detach", "FETCH_HEAD"],
                url,
            )?;
            // The cache must mirror the fetched commit exactly; leftover files would leak into materialized output.
            run_git(&dir, &["reset", "--quiet", "--hard", "HEAD"], url)?;
            run_git(&dir, &["clean", "--quiet", "-ffdx"], url)?;
        } else {
            std::fs::create_dir_all(&self.cache_root).map_err(|e| {
                Diagnostic::new(
                    DiagnosticCode::Io,
                    format!("failed to create cache directory: {e}"),
                )
            })?;
            let dir_str = dir.to_string_lossy().into_owned();
            run_git_anywhere(
                &[
                    "clone", "--quiet", "--depth", "1", "--branch", branch, "--", url, &dir_str,
                ],
                url,
            )?;
        }

        let commit = run_git(&dir, &["rev-parse", "HEAD"], url)?
            .trim()
            .to_owned();

        Ok(ResolvedSource {
            commit,
            checkout_dir: dir,
        })
    }
}

fn run_git(dir: &Path, args: &[&str], url: &str) -> Result<String, Diagnostic> {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(args);
    run(command, url)
}

fn run_git_anywhere(args: &[&str], url: &str) -> Result<String, Diagnostic> {
    let mut command = Command::new("git");
    command.args(args);
    run(command, url)
}

fn run(mut command: Command, url: &str) -> Result<String, Diagnostic> {
    // Resolution must fail with a diagnostic instead of blocking on an interactive credential prompt.
    command.env("GIT_TERMINAL_PROMPT", "0");

    let output = command.output().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::GitResolution,
            format!("failed to run git for `{url}`: {e}"),
        )
    })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Diagnostic::new(
            DiagnosticCode::GitResolution,
            format!("git failed for `{url}`: {}", stderr.trim()),
        ))
    }
}
