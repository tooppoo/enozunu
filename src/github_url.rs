//! Resolves GitHub tree/blob web URLs into Git source references for `add-skill`.
//!
//! The accepted forms are:
//!
//! ```text
//! https://github.com/<owner>/<repo>/tree/<ref>/<skill-path>
//! https://github.com/<owner>/<repo>/blob/<ref>/<skill-path>/SKILL.md
//! https://github.com/<owner>/<repo>            (repository root at the default branch)
//! ```
//!
//! A repository whose root is itself the Skill directory (`SKILL.md` at the top level) is
//! addressed by the repository URL or by a tree/blob URL whose ref consumes the whole
//! remainder; both record the Skill path as `.`.
//!
//! The `<ref>` / `<skill-path>` boundary is never guessed from the string alone: the remote's
//! advertised branches and tags decide it, so a branch or tag containing `/` resolves
//! correctly, and a URL that reads as more than one selector/path split is an error rather
//! than a silent preference.

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::git::{CommitSha, GitSelector, RemoteRefs};

/// A GitHub Skill URL parsed down to its repository and undivided ref-and-path remainder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGithubUrl {
    /// `https://github.com/<owner>/<repo>` — what the manifest records as the `git` `url`.
    pub repo_url: String,
    /// The segments after `tree/` or `blob/` (a blob's trailing `SKILL.md` already removed):
    /// selector and Skill path together, boundary not yet known.
    pub ref_and_path: Vec<String>,
}

/// A Git source reference resolved from a GitHub URL: exactly what `add-skill` records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSourceSpec {
    pub url: String,
    pub selector: GitSelector,
    pub path: String,
}

impl GitSourceSpec {
    /// The selector as its manifest field name and value (`branch` / `tag` / `revision`).
    pub fn selector_field(&self) -> (&'static str, &str) {
        match &self.selector {
            GitSelector::Branch(branch) => ("branch", branch.as_str()),
            GitSelector::Tag(tag) => ("tag", tag.as_str()),
            GitSelector::Revision(sha) => ("revision", sha.as_str()),
        }
    }
}

/// Parses `url` as one of the supported GitHub Skill URL forms.
///
/// Everything else — profile URLs, raw and Gist URLs, other hosts, Git remote shorthand —
/// is rejected here with a diagnostic naming what is supported, before any network access
/// happens.
pub fn parse_github_skill_url(url: &str) -> Result<ParsedGithubUrl, Diagnostic> {
    let unsupported =
        |message: String| Diagnostic::new(DiagnosticCode::UnsupportedSourceReference, message);

    let Some(rest) = url.strip_prefix("https://github.com/") else {
        // Recognizable near-miss hosts get a pointed message; everything else the generic one.
        let hint = if url.starts_with("https://raw.githubusercontent.com/") {
            "; raw URLs are not supported"
        } else if url.starts_with("https://gist.github.com/") {
            "; Gist URLs are not supported here (declare a `gist` source in the manifest instead)"
        } else {
            ""
        };
        return Err(unsupported(format!(
            "skill source URL `{url}` is not supported{hint}; expected https://github.com/<owner>/<repo>/tree/<ref>/<skill-path> or .../blob/<ref>/<skill-path>/SKILL.md"
        )));
    };

    // A query or fragment could hide part of the path or a line anchor; there is no meaningful
    // interpretation for either in a source reference.
    if rest.contains('?') || rest.contains('#') {
        return Err(unsupported(format!(
            "skill source URL `{url}` carries a query or fragment; pass the plain tree/blob URL"
        )));
    }

    // A valid GitHub tree/blob URL never contains a backslash, and letting one through would
    // hand `Path::join` a Windows separator later, bypassing the `/`-based path validation.
    if rest.contains('\\') {
        return Err(unsupported(format!(
            "skill source URL `{url}` contains a backslash; expected a GitHub tree/blob URL with `/` separators"
        )));
    }

    let segments: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    if segments.len() < 2 || segments.iter().take(2).any(|s| s.is_empty()) {
        return Err(unsupported(format!(
            "skill source URL `{url}` points at a profile, not a repository or Skill; expected a repository URL, a tree URL naming the Skill directory, or a blob URL naming its SKILL.md"
        )));
    }

    let (owner, repo) = (segments[0], segments[1]);
    let repo_url = format!("https://github.com/{owner}/{repo}");

    // A bare repository URL addresses the repository root at its default branch; whether the
    // root actually is a Skill directory is checked against the resolved content later.
    if segments.len() == 2 {
        return Ok(ParsedGithubUrl {
            repo_url,
            ref_and_path: Vec::new(),
        });
    }

    let marker = segments[2];
    let mut ref_and_path: Vec<String> = segments[3..].iter().map(|s| s.to_string()).collect();
    if ref_and_path.iter().any(|s| s.is_empty()) {
        return Err(unsupported(format!(
            "skill source URL `{url}` contains an empty path segment"
        )));
    }

    match marker {
        "tree" => {
            if ref_and_path.is_empty() {
                return Err(unsupported(format!(
                    "skill source URL `{url}` is missing a ref after /tree/"
                )));
            }
        }
        "blob" => {
            if ref_and_path.last().map(String::as_str) != Some("SKILL.md") {
                return Err(unsupported(format!(
                    "skill source blob URL `{url}` must point at the Skill's SKILL.md"
                )));
            }
            ref_and_path.pop();
            if ref_and_path.is_empty() {
                return Err(unsupported(format!(
                    "skill source URL `{url}` is missing a ref before SKILL.md"
                )));
            }
        }
        other => {
            return Err(unsupported(format!(
                "skill source URL `{url}` uses `/{other}/`; only tree and blob URLs are supported"
            )));
        }
    }

    Ok(ParsedGithubUrl {
        repo_url,
        ref_and_path,
    })
}

/// Splits the parsed URL's remainder into a selector and a Skill path using the remote's refs.
///
/// Every leading-segment prefix that names an advertised branch or tag is a candidate — a ref
/// consuming the whole remainder addresses the repository root, recorded as path `.` — and a
/// full-commit-id first segment is a revision candidate; exactly one candidate must remain.
/// Zero candidates means the URL names no known ref; more than one means the URL is ambiguous
/// and the caller must disambiguate — neither is resolved by preference. An empty remainder
/// (bare repository URL) resolves to the remote's default branch at the repository root.
pub fn resolve_boundary(
    parsed: &ParsedGithubUrl,
    refs: &RemoteRefs,
) -> Result<GitSourceSpec, Diagnostic> {
    let segments = &parsed.ref_and_path;

    if segments.is_empty() {
        let Some(head) = &refs.head_branch else {
            return Err(Diagnostic::new(
                DiagnosticCode::GitResolution,
                format!(
                    "cannot determine the default branch of `{}`: the remote advertises no HEAD; pass a tree URL naming the ref",
                    parsed.repo_url
                ),
            ));
        };
        return Ok(GitSourceSpec {
            url: parsed.repo_url.clone(),
            selector: GitSelector::Branch(head.clone()),
            path: ".".to_owned(),
        });
    }

    let path_after = |k: usize| {
        if k == segments.len() {
            ".".to_owned()
        } else {
            segments[k..].join("/")
        }
    };
    let mut candidates: Vec<(GitSelector, String)> = Vec::new();
    for k in 1..=segments.len() {
        let name = segments[..k].join("/");
        if refs.branches.contains(&name) {
            candidates.push((GitSelector::Branch(name.clone()), path_after(k)));
        }
        if refs.tags.contains(&name) {
            candidates.push((GitSelector::Tag(name), path_after(k)));
        }
    }
    if let Some(sha) = CommitSha::parse(&segments[0]) {
        candidates.push((GitSelector::Revision(sha), path_after(1)));
    }

    match candidates.len() {
        1 => {
            let (selector, path) = candidates.into_iter().next().expect("one candidate");
            Ok(GitSourceSpec {
                url: parsed.repo_url.clone(),
                selector,
                path,
            })
        }
        0 => Err(Diagnostic::new(
            DiagnosticCode::GitResolution,
            format!(
                "cannot resolve `{}` against `{}`: no leading segments name an advertised branch or tag, and the first segment is not a full commit id",
                segments.join("/"),
                parsed.repo_url
            ),
        )),
        _ => {
            let readings: Vec<String> = candidates
                .iter()
                .map(|(selector, path)| {
                    let (kind, value) = match selector {
                        GitSelector::Branch(b) => ("branch", b.as_str()),
                        GitSelector::Tag(t) => ("tag", t.as_str()),
                        GitSelector::Revision(r) => ("revision", r.as_str()),
                    };
                    format!("{kind} `{value}` with path `{path}`")
                })
                .collect();
            Err(Diagnostic::new(
                DiagnosticCode::GitResolution,
                format!(
                    "`{}` under `{}` is ambiguous: it reads as {}; rename the colliding refs or declare the source in the manifest directly",
                    segments.join("/"),
                    parsed.repo_url,
                    readings.join(" or ")
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(branches: &[&str], tags: &[&str]) -> RemoteRefs {
        RemoteRefs {
            branches: branches.iter().map(|s| s.to_string()).collect(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            head_branch: None,
        }
    }

    fn parse(url: &str) -> ParsedGithubUrl {
        parse_github_skill_url(url).unwrap()
    }

    #[test]
    fn parses_a_tree_url_into_repo_and_remainder() {
        let parsed = parse("https://github.com/example/repo/tree/main/skills/review");
        assert_eq!(parsed.repo_url, "https://github.com/example/repo");
        assert_eq!(parsed.ref_and_path, ["main", "skills", "review"]);
    }

    #[test]
    fn parses_a_blob_url_dropping_the_trailing_skill_md() {
        let parsed = parse("https://github.com/example/repo/blob/main/skills/review/SKILL.md");
        assert_eq!(parsed.repo_url, "https://github.com/example/repo");
        assert_eq!(parsed.ref_and_path, ["main", "skills", "review"]);
    }

    #[test]
    fn parses_a_bare_repository_url_with_an_empty_remainder() {
        let parsed = parse("https://github.com/example/repo");
        assert_eq!(parsed.repo_url, "https://github.com/example/repo");
        assert!(parsed.ref_and_path.is_empty());
        assert_eq!(
            parse("https://github.com/example/repo/"),
            parsed,
            "a trailing slash must not change the reading"
        );
    }

    #[test]
    fn accepts_a_trailing_slash_on_a_tree_url() {
        let parsed = parse("https://github.com/example/repo/tree/main/skills/review/");
        assert_eq!(parsed.ref_and_path, ["main", "skills", "review"]);
    }

    #[test]
    fn rejects_unsupported_url_forms() {
        let cases = [
            "https://github.com/example",
            "https://github.com/example/",
            "https://github.com/example/repo/commits/main/skills",
            "https://github.com/example/repo/tree",
            "https://github.com/example/repo/blob/SKILL.md",
            "https://github.com/example/repo/blob/main/skills/review/README.md",
            "https://github.com/example/repo/tree/main/skills/review?tab=readme",
            "https://github.com/example/repo/tree/main/skills/review#usage",
            "https://raw.githubusercontent.com/example/repo/main/skills/review/SKILL.md",
            "https://gist.github.com/example/2decf6c462d9b4418f2",
            "https://gitlab.com/example/repo/tree/main/skills/review",
            "http://github.com/example/repo/tree/main/skills/review",
            "git@github.com:example/repo.git",
            "example/repo",
        ];
        for url in cases {
            let diag = parse_github_skill_url(url).unwrap_err();
            assert_eq!(
                diag.code,
                DiagnosticCode::UnsupportedSourceReference,
                "must reject `{url}`"
            );
        }
    }

    #[test]
    fn resolves_a_plain_branch_boundary() {
        let parsed = parse("https://github.com/example/repo/tree/main/skills/review");
        let spec = resolve_boundary(&parsed, &refs(&["main"], &[])).unwrap();
        assert_eq!(spec.selector, GitSelector::Branch("main".to_owned()));
        assert_eq!(spec.path, "skills/review");
        assert_eq!(spec.url, "https://github.com/example/repo");
    }

    #[test]
    fn resolves_a_branch_containing_a_slash() {
        let parsed = parse("https://github.com/example/repo/tree/feature/review-x/skills/review");
        let spec = resolve_boundary(&parsed, &refs(&["feature/review-x", "main"], &[])).unwrap();
        assert_eq!(
            spec.selector,
            GitSelector::Branch("feature/review-x".to_owned())
        );
        assert_eq!(spec.path, "skills/review");
    }

    #[test]
    fn resolves_a_tag_boundary() {
        let parsed = parse("https://github.com/example/repo/tree/v1.2.0/skills/review");
        let spec = resolve_boundary(&parsed, &refs(&["main"], &["v1.2.0"])).unwrap();
        assert_eq!(spec.selector, GitSelector::Tag("v1.2.0".to_owned()));
        assert_eq!(spec.path, "skills/review");
    }

    #[test]
    fn resolves_a_full_commit_id_as_a_revision() {
        let sha = "468aac8caed5f0c3b859b8286968e2c78e2b8760";
        let parsed = parse(&format!(
            "https://github.com/example/repo/tree/{sha}/skills/review"
        ));
        let spec = resolve_boundary(&parsed, &refs(&["main"], &[])).unwrap();
        assert_eq!(
            spec.selector,
            GitSelector::Revision(CommitSha::parse(sha).unwrap())
        );
        assert_eq!(spec.path, "skills/review");
    }

    #[test]
    fn rejects_a_branch_and_tag_collision_as_ambiguous() {
        let parsed = parse("https://github.com/example/repo/tree/v1/skills/review");
        let diag = resolve_boundary(&parsed, &refs(&["v1"], &["v1"])).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::GitResolution);
        assert!(diag.message.contains("ambiguous"), "{}", diag.message);
    }

    #[test]
    fn rejects_two_branches_matching_different_boundaries_as_ambiguous() {
        let parsed = parse("https://github.com/example/repo/tree/a/b/skills/review");
        let diag = resolve_boundary(&parsed, &refs(&["a", "a/b"], &[])).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::GitResolution);
        assert!(diag.message.contains("ambiguous"), "{}", diag.message);
    }

    #[test]
    fn rejects_a_commit_id_that_also_names_a_branch_as_ambiguous() {
        let sha = "468aac8caed5f0c3b859b8286968e2c78e2b8760";
        let parsed = parse(&format!(
            "https://github.com/example/repo/tree/{sha}/skills/review"
        ));
        let diag = resolve_boundary(&parsed, &refs(&[sha], &[])).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::GitResolution);
        assert!(diag.message.contains("ambiguous"), "{}", diag.message);
    }

    #[test]
    fn rejects_a_backslash_anywhere_in_the_url() {
        let diag = parse_github_skill_url("https://github.com/example/repo/tree/main/..\\..\\x")
            .unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::UnsupportedSourceReference);
        assert!(diag.message.contains("backslash"), "{}", diag.message);
    }

    #[test]
    fn resolves_a_lone_commit_id_to_the_repository_root() {
        let sha = "468aac8caed5f0c3b859b8286968e2c78e2b8760";
        let parsed = parse(&format!("https://github.com/example/repo/tree/{sha}"));
        let spec = resolve_boundary(&parsed, &refs(&["main"], &[])).unwrap();
        assert_eq!(
            spec.selector,
            GitSelector::Revision(CommitSha::parse(sha).unwrap())
        );
        assert_eq!(spec.path, ".");
    }

    #[test]
    fn resolves_a_bare_repository_url_to_the_default_branch_root() {
        let parsed = parse("https://github.com/example/repo");
        let mut remote = refs(&["develop", "main"], &[]);
        remote.head_branch = Some("main".to_owned());
        let spec = resolve_boundary(&parsed, &remote).unwrap();
        assert_eq!(spec.selector, GitSelector::Branch("main".to_owned()));
        assert_eq!(spec.path, ".");
    }

    #[test]
    fn a_bare_repository_url_without_an_advertised_head_is_an_error() {
        let parsed = parse("https://github.com/example/repo/");
        let diag = resolve_boundary(&parsed, &refs(&["main"], &[])).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::GitResolution);
        assert!(diag.message.contains("default branch"), "{}", diag.message);
    }

    #[test]
    fn a_full_ref_and_a_shorter_ref_reading_are_ambiguous() {
        // `main/sub` reads as branch `main` with path `sub` and as branch `main/sub`
        // addressing the repository root; neither is preferred.
        let parsed = parse("https://github.com/example/repo/tree/main/sub");
        let diag = resolve_boundary(&parsed, &refs(&["main", "main/sub"], &[])).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::GitResolution);
        assert!(diag.message.contains("ambiguous"), "{}", diag.message);
    }

    #[test]
    fn rejects_a_remainder_matching_no_ref() {
        let parsed = parse("https://github.com/example/repo/tree/unknown/skills/review");
        let diag = resolve_boundary(&parsed, &refs(&["main"], &[])).unwrap_err();
        assert_eq!(diag.code, DiagnosticCode::GitResolution);
        assert!(diag.message.contains("cannot resolve"), "{}", diag.message);
    }

    #[test]
    fn resolves_a_full_ref_remainder_to_the_repository_root() {
        let parsed = parse("https://github.com/example/repo/tree/feature/x");
        // `feature/x` names a branch in full: the URL addresses the repository root.
        let spec = resolve_boundary(&parsed, &refs(&["feature/x"], &[])).unwrap();
        assert_eq!(spec.selector, GitSelector::Branch("feature/x".to_owned()));
        assert_eq!(spec.path, ".");
    }

    #[test]
    fn resolves_a_blob_url_for_a_root_skill_md() {
        let parsed = parse("https://github.com/example/repo/blob/main/SKILL.md");
        let spec = resolve_boundary(&parsed, &refs(&["main"], &[])).unwrap();
        assert_eq!(spec.selector, GitSelector::Branch("main".to_owned()));
        assert_eq!(spec.path, ".");
    }
}
