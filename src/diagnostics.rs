//! Structured diagnostics for invalid configuration and unsafe materialization plans.
//!
//! Diagnostics are stable values first and rendered text second, so that machine-readable output can be added later without changing callers.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code.as_str(), self.message)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    /// The manifest is not syntactically valid KDL.
    ManifestSyntax,
    /// The manifest is valid KDL but does not match the expected structure.
    ManifestShape,
    /// The manifest declares a `config-version` this build does not support.
    UnsupportedConfigVersion,
    /// The manifest selects a `consumer` target that is not a supported target AI; the supported targets are `claude` and `codex`.
    UnsupportedConsumer,
    /// The source reference uses a form outside the supported `git` / `local` / `gist` blocks, such as an unknown block kind, a GitHub tree/blob URL shorthand, an absolute local path, or a `file` field in a Skill Gist.
    UnsupportedSourceReference,
    /// Two sources of the same kind share a name.
    DuplicateSourceName,
    /// A source or selection name is empty or contains unsafe path characters.
    InvalidName,
    /// A `use-skills` / `use-agents` entry references a source that is not declared.
    UnknownSourceReference,
    /// A `gist` block declares an `id` outside the v0 accepted form (non-empty lowercase ASCII hexadecimal).
    InvalidGistId,
    /// A `gist` block declares a `revision` outside the v0 accepted form (exactly 40 lowercase ASCII hexadecimal characters).
    InvalidRevision,
    /// Two materializations resolve to the same target path.
    DuplicateTargetPath,
    /// Resolving a Git source failed.
    GitResolution,
    /// The lock file exists but is not valid JSON, does not match the expected shape, or records a non-canonical revision.
    LockParse,
    /// The lock file declares a `version` this build does not support.
    UnsupportedLockVersion,
    /// Frozen materialization requires an up-to-date lock file, and it is missing or lacks an entry for a mutable source.
    LockOutOfDate,
    /// A Gist remote could not be fetched. Distinct from `GitResolution` so Gist transport failures are classified as Gist failures even though Git transport is used internally.
    GistFetch,
    /// The pinned Gist revision does not exist in the fetched Gist.
    GistRevisionNotFound,
    /// A selected source path does not exist in the resolved source.
    SourcePathNotFound,
    /// The resolved source does not have the artifact shape the target operation requires.
    ArtifactShape,
    /// A path would escape its permitted root via traversal or symlinks.
    UnsafePath,
    /// A filesystem operation failed.
    Io,
}

impl DiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticCode::ManifestSyntax => "manifest-syntax",
            DiagnosticCode::ManifestShape => "manifest-shape",
            DiagnosticCode::UnsupportedConfigVersion => "unsupported-config-version",
            DiagnosticCode::UnsupportedConsumer => "unsupported-consumer",
            DiagnosticCode::UnsupportedSourceReference => "unsupported-source-reference",
            DiagnosticCode::DuplicateSourceName => "duplicate-source-name",
            DiagnosticCode::InvalidName => "invalid-name",
            DiagnosticCode::UnknownSourceReference => "unknown-source-reference",
            DiagnosticCode::InvalidGistId => "invalid-gist-id",
            DiagnosticCode::InvalidRevision => "invalid-revision",
            DiagnosticCode::DuplicateTargetPath => "duplicate-target-path",
            DiagnosticCode::GitResolution => "git-resolution",
            DiagnosticCode::LockParse => "lock-parse",
            DiagnosticCode::UnsupportedLockVersion => "unsupported-lock-version",
            DiagnosticCode::LockOutOfDate => "lock-out-of-date",
            DiagnosticCode::GistFetch => "gist-fetch",
            DiagnosticCode::GistRevisionNotFound => "gist-revision-not-found",
            DiagnosticCode::SourcePathNotFound => "source-path-not-found",
            DiagnosticCode::ArtifactShape => "artifact-shape",
            DiagnosticCode::UnsafePath => "unsafe-path",
            DiagnosticCode::Io => "io",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_prefixes_the_code_and_message() {
        let diag = Diagnostic::new(DiagnosticCode::Io, "disk full");
        assert_eq!(diag.code, DiagnosticCode::Io);
        assert_eq!(diag.message, "disk full");
        assert_eq!(diag.to_string(), "error[io]: disk full");
    }

    #[test]
    fn every_code_renders_a_stable_slug() {
        let cases = [
            (DiagnosticCode::ManifestSyntax, "manifest-syntax"),
            (DiagnosticCode::ManifestShape, "manifest-shape"),
            (
                DiagnosticCode::UnsupportedConfigVersion,
                "unsupported-config-version",
            ),
            (DiagnosticCode::UnsupportedConsumer, "unsupported-consumer"),
            (
                DiagnosticCode::UnsupportedSourceReference,
                "unsupported-source-reference",
            ),
            (DiagnosticCode::DuplicateSourceName, "duplicate-source-name"),
            (DiagnosticCode::InvalidName, "invalid-name"),
            (
                DiagnosticCode::UnknownSourceReference,
                "unknown-source-reference",
            ),
            (DiagnosticCode::InvalidGistId, "invalid-gist-id"),
            (DiagnosticCode::InvalidRevision, "invalid-revision"),
            (DiagnosticCode::DuplicateTargetPath, "duplicate-target-path"),
            (DiagnosticCode::GitResolution, "git-resolution"),
            (DiagnosticCode::LockParse, "lock-parse"),
            (
                DiagnosticCode::UnsupportedLockVersion,
                "unsupported-lock-version",
            ),
            (DiagnosticCode::LockOutOfDate, "lock-out-of-date"),
            (DiagnosticCode::GistFetch, "gist-fetch"),
            (
                DiagnosticCode::GistRevisionNotFound,
                "gist-revision-not-found",
            ),
            (DiagnosticCode::SourcePathNotFound, "source-path-not-found"),
            (DiagnosticCode::ArtifactShape, "artifact-shape"),
            (DiagnosticCode::UnsafePath, "unsafe-path"),
            (DiagnosticCode::Io, "io"),
        ];
        for (code, slug) in cases {
            assert_eq!(code.as_str(), slug);
        }
    }
}
