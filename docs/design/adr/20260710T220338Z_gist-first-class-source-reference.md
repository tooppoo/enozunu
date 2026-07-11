# Gist as a First-Class Source Reference Kind

- Status: Accepted
- Created: 2026-07-10T22:03:38Z

## Context

Small agent definitions are often distributed as GitHub Gists. Requiring a full repository for a single agent file adds unnecessary distribution overhead.

A Gist could be resolved through several transports: the GitHub Gist REST API, a manually constructed raw-content URL, or Git. The REST API imposes rate limits and can truncate large files, and raw-content URL patterns are undocumented and unstable. Git transport avoids both, because a Gist is itself a Git repository at `https://gist.github.com/<id>.git`.

Reusing Git transport raises a modeling question that outlives this feature. A Gist is not a Git source in the manifest sense: it has no user-facing `url`, `branch`, or checkout-relative `path`, and it pins an immutable revision rather than following a branch. Collapsing a Gist into the existing `git { url branch path }` reference would force Gist concepts into fields that do not fit and would erase the Gist identity in diagnostics, runtime output, and provenance.

This decision needs to be recorded as an ADR, not only in an issue, because it establishes a lasting boundary between source identity and transport implementation, and because it deliberately does not amend [the source reference blocks ADR](20260709T070553Z_source-reference-blocks-and-local-sources.md) as if Gist had been part of that original decision.

## Decision

`gist` is a first-class source reference kind alongside `git` and `local`.

A Gist source is expressed by three required fields:

```kdl
agent "shell-script-reviewer" {
  gist {
    id "2decf6c462d9b4418f2"
    revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
    file "shell-script-reviewer.md"
  }
}
```

Gist identity is expressed by `id`, a pinned `revision`, and a selected `file`. The owner is not part of the manifest contract: the resolver builds the Git remote `https://gist.github.com/<id>.git` from the id alone.

The Rust model extends the existing tagged union with a distinct variant, so a Gist is never collapsed into the `Git` variant:

```rust
pub enum SourceReference {
    Git { url: String, branch: String, path: String },
    Local { path: String },
    Gist { id: GistId, revision: CommitSha, file: String },
}
```

Validated domain types enforce the accepted-input contract:

- `GistId` accepts a non-empty lowercase ASCII hexadecimal string with no fixed length. This is a conservative accepted-input contract, not a claim that GitHub guarantees a specific Gist id length or representation. Percent-encoded and otherwise non-canonical forms are rejected, because a validated id is interpolated directly into the Gist Git remote URL.
- `CommitSha` accepts exactly 40 lowercase ASCII hexadecimal characters. Latest, branch, tag, abbreviated, uppercase, whitespace-padded, and SHA-256 forms are rejected. SHA-256 object id support would require a future contract change.

Gist resolution uses Git transport internally, but source identity and transport implementation stay distinct concerns:

- Branch resolution and exact-revision resolution are distinct resolver operations, expressed as a typed `GitSelector::Branch` or `GitSelector::Revision`. A revision is never passed through an argument whose contract is "branch".
- After checkout, the resolver verifies that `HEAD` equals the requested revision.
- Gist checkouts are de-duplicated by `(id, revision)`; the selected file is not part of the checkout identity, so multiple agents selecting different files from one revision share a single checkout.
- The runtime origin, the CLI output, and the provenance record all preserve Gist identity. Provenance records `source.type = "gist"` with `id`, `revision`, and `file`, never `type: "git"`.

Diagnostic classification depends only on a `DiagnosticCode`, not on message text. Gist transport failures are classified as `GistFetch` and `GistRevisionNotFound`, not as `GitResolution`, so classification reflects the source kind rather than the transport.

The GitHub REST API and raw-content URLs are not the required materialization path. The remote-access boundary is a replaceable `GitResolver`, so required automated tests do not depend on live GitHub access: exact-revision resolution is integration-tested against a local Git repository, and the full pipeline is tested through a deterministic fake transport. A live Gist smoke test may exist for manual use, but it is not part of the required suite.

Gist support is initially limited to agent files. A `gist` block under `provider.skills` is rejected. Skill support from Gists is deferred until the directory-artifact semantics are decided.

## Alternatives Considered

### Model a Gist as a Git source with a `gist.github.com` URL

Reusing the `Git` variant with a `https://gist.github.com/<id>.git` URL would avoid a new source kind, but it forces Gist concepts into fields that do not fit. A Gist has no user-facing branch, and pinning an immutable revision through a field whose contract is "branch" would be a category error. It would also erase the Gist identity from diagnostics, runtime output, and provenance, because every entry would read as an ordinary Git source. The mismatch would surface as artificial required fields and as lost provenance fidelity.

### Resolve Gists through the GitHub REST API or a raw-content URL

The REST API is subject to rate limits and truncates large files, and manually constructed `gist.githubusercontent.com` URLs depend on an undocumented, unstable pattern. Git transport avoids both and reuses the existing checkout and cache infrastructure. Git is therefore an internal transport detail, not part of the user-facing source contract.

### Amend the existing source reference ADR

Editing [the source reference blocks ADR](20260709T070553Z_source-reference-blocks-and-local-sources.md) to include Gist would misrepresent the history, as if Gist had been part of the original two-kind decision. Recording a separate ADR keeps the decision boundary and its rationale traceable.

## Consequences

### Positive Consequences

- A single agent file can be distributed as a Gist without publishing a full repository.
- The manifest syntax, the Rust model, and the provenance JSON share one tagged-union structure, so the Gist kind is additive in all three places.
- Source identity is preserved end to end: a Gist is never mistaken for a Git source in output or provenance.
- Required tests are deterministic and network-independent, because the remote-access boundary is replaceable.

### Negative Consequences

- Git transport is now driven by two distinct selectors, so the resolver contract is larger than a single branch-resolution call.
- Gist support is asymmetric across artifact kinds in this version: agents may use Gists, Skills may not.

### Neutral Consequences

- A Gist pins an immutable revision, so re-materializing the same Gist reference always produces the same result, unlike a branch-following Git source.
- The accepted-input contract for Gist ids is intentionally conservative and may be revised if GitHub's id representation changes.

## Related

- Issue: [#24](https://github.com/tooppoo/enozunu/issues/24)
- ADR: [Source Reference Blocks and Local Sources](20260709T070553Z_source-reference-blocks-and-local-sources.md)
- ADR: [Branch Selector First, Reproducibility Deferred](20260708T104203Z_branch-selector-first-reproducibility-deferred.md)
