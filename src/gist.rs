//! Resolves Gist source references over Git transport.
//!
//! A Gist source is a first-class source kind, not a Git source. Git is only the transport used to fetch an immutable Gist revision.
//! This module keeps that boundary explicit: it constructs the Gist Git remote from the Gist id, drives the `GitResolver` with an exact-revision selector, and translates transport failures into Gist-specific diagnostics.
//! See docs/design/adr/20260710T220338Z_gist-first-class-source-reference.md for the source-identity-versus-transport decision.

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::git::{
    CommitSha, GitError, GitResolutionRequest, GitResolver, GitSelector, ResolvedSource,
};
use crate::manifest::GistId;

/// The immutable identity of a Gist checkout: an id and a pinned revision.
///
/// The selected `file` is not part of this identity, so multiple agents selecting different files from the same Gist revision resolve to one shared checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GistRequest {
    pub id: GistId,
    pub revision: CommitSha,
}

pub trait GistResolver {
    /// Resolves a Gist revision to a local checkout.
    fn resolve(&self, request: &GistRequest) -> Result<ResolvedSource, Diagnostic>;
}

/// Builds the Git remote for a Gist from its id alone.
///
/// The owner is deliberately not required: the Gist id is sufficient for Git resolution, and a validated id is interpolated directly, so no arbitrary manifest input is percent-encoded into the remote URL.
pub fn gist_remote_url(id: &GistId) -> String {
    format!("https://gist.github.com/{}.git", id.as_str())
}

/// Resolves Gist sources by driving a `GitResolver` with an exact-revision selector.
///
/// The wrapped `GitResolver` is the replaceable remote-access boundary: production uses the command-line Git resolver, and tests substitute a deterministic fake transport.
pub struct GitTransportGistResolver<'a> {
    git: &'a dyn GitResolver,
}

impl<'a> GitTransportGistResolver<'a> {
    pub fn new(git: &'a dyn GitResolver) -> Self {
        Self { git }
    }
}

impl GistResolver for GitTransportGistResolver<'_> {
    fn resolve(&self, request: &GistRequest) -> Result<ResolvedSource, Diagnostic> {
        let git_request = GitResolutionRequest {
            url: gist_remote_url(&request.id),
            // A Gist pins an exact revision, so it always resolves by revision, never by branch.
            selector: GitSelector::Revision(request.revision.clone()),
        };
        self.git
            .resolve(&git_request)
            .map_err(|e| gist_diagnostic(e, request))
    }
}

/// Maps a transport failure to a Gist-specific diagnostic code.
///
/// A Gist fetch failure is classified as `GistFetch`, not `GitResolution`, even though Git transport is used internally, so classification reflects the source kind rather than the transport.
fn gist_diagnostic(error: GitError, request: &GistRequest) -> Diagnostic {
    match error {
        GitError::Fetch(message) => Diagnostic::new(
            DiagnosticCode::GistFetch,
            format!("failed to fetch gist `{}`: {message}", request.id.as_str()),
        ),
        GitError::RevisionNotFound(message) => Diagnostic::new(
            DiagnosticCode::GistRevisionNotFound,
            format!(
                "gist `{}` has no revision `{}`: {message}",
                request.id.as_str(),
                request.revision.as_str()
            ),
        ),
        GitError::Io(message) => Diagnostic::new(DiagnosticCode::Io, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn gist_id(raw: &str) -> GistId {
        GistId::parse(raw).expect("test gist id must be valid")
    }

    fn commit_sha() -> CommitSha {
        CommitSha::parse("468aac8caed5f0c3b859b8286968e2c78e2b8760").expect("valid sha")
    }

    /// A fake transport that records requests and returns a scripted outcome.
    struct FakeGit {
        outcome: Result<ResolvedSource, GitError>,
        requests: RefCell<Vec<GitResolutionRequest>>,
    }

    impl FakeGit {
        fn new(outcome: Result<ResolvedSource, GitError>) -> Self {
            Self {
                outcome,
                requests: RefCell::new(Vec::new()),
            }
        }
    }

    impl GitResolver for FakeGit {
        fn resolve(&self, request: &GitResolutionRequest) -> Result<ResolvedSource, GitError> {
            self.requests.borrow_mut().push(request.clone());
            self.outcome.clone()
        }
    }

    #[test]
    fn builds_the_gist_remote_from_the_id_alone() {
        assert_eq!(
            gist_remote_url(&gist_id("2decf6c462d9b4418f2")),
            "https://gist.github.com/2decf6c462d9b4418f2.git"
        );
    }

    #[test]
    fn drives_the_transport_with_an_exact_revision_selector() {
        let resolved = ResolvedSource {
            commit: commit_sha().as_str().to_owned(),
            checkout_dir: PathBuf::from("/tmp/checkout"),
        };
        let git = FakeGit::new(Ok(resolved));
        let resolver = GitTransportGistResolver::new(&git);

        resolver
            .resolve(&GistRequest {
                id: gist_id("2decf6c462d9b4418f2"),
                revision: commit_sha(),
            })
            .unwrap();

        let requests = git.requests.borrow();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url,
            "https://gist.github.com/2decf6c462d9b4418f2.git"
        );
        assert_eq!(requests[0].selector, GitSelector::Revision(commit_sha()));
    }

    #[test]
    fn maps_fetch_failure_to_gist_fetch() {
        let git = FakeGit::new(Err(GitError::Fetch("unreachable".to_owned())));
        let resolver = GitTransportGistResolver::new(&git);
        let diag = resolver
            .resolve(&GistRequest {
                id: gist_id("2decf6c462d9b4418f2"),
                revision: commit_sha(),
            })
            .unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::GistFetch);
    }

    #[test]
    fn maps_revision_not_found_to_gist_revision_not_found() {
        let git = FakeGit::new(Err(GitError::RevisionNotFound("absent".to_owned())));
        let resolver = GitTransportGistResolver::new(&git);
        let diag = resolver
            .resolve(&GistRequest {
                id: gist_id("2decf6c462d9b4418f2"),
                revision: commit_sha(),
            })
            .unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::GistRevisionNotFound);
    }
}
