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
    /// The manifest selects a consumer target that v0.0.x does not support, such as `consumer.codex`.
    UnsupportedConsumer,
    /// The source reference is not the normalized `git` + `branch` + `path` form, such as a GitHub tree/blob URL shorthand.
    UnsupportedSourceReference,
    /// Two sources of the same kind share a name.
    DuplicateSourceName,
    /// A source or selection name is empty or contains unsafe path characters.
    InvalidName,
    /// A `use-skills` / `use-agents` entry references a source that is not declared.
    UnknownSourceReference,
    /// Two materializations resolve to the same target path.
    DuplicateTargetPath,
    /// Resolving a Git source failed.
    GitResolution,
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
            DiagnosticCode::DuplicateTargetPath => "duplicate-target-path",
            DiagnosticCode::GitResolution => "git-resolution",
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
            (DiagnosticCode::DuplicateTargetPath, "duplicate-target-path"),
            (DiagnosticCode::GitResolution, "git-resolution"),
            (DiagnosticCode::ArtifactShape, "artifact-shape"),
            (DiagnosticCode::UnsafePath, "unsafe-path"),
            (DiagnosticCode::Io, "io"),
        ];
        for (code, slug) in cases {
            assert_eq!(code.as_str(), slug);
        }
    }
}
