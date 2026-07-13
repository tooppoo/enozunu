# Support Exact Revision Selectors in Git Source Blocks

- Status: Accepted
- Created: 2026-07-12T15:53:45Z

## Context

[The branch-selector-first ADR](20260708T104203Z_branch-selector-first-reproducibility-deferred.md) deferred exact revision selection so early dogfooding could follow fast-moving branch heads.
Since then, Gist sources introduced revision pinning (the `gist` block requires a `revision`), and the resolver already supports exact-revision resolution behind `GitSelector::Revision`.
An ordinary `git` source, however, could still only follow a branch, so a user could not pin a repository-hosted Skill or agent to one exact commit.

Allowing a second selector raises three contract questions that outlive the implementation:

1. how a manifest declares which selector applies, without ambiguous or silently-overriding combinations
2. what `revision` accepts, given that Git revspecs range from full object ids to relative expressions such as `main~3`
3. how provenance records the declared selector without breaking its typed `source` object shape

These are manifest and machine-readable contract decisions, so they are recorded as an ADR.

## Decision

A `git` source block declares exactly one selector: `branch` or `revision`.

- Declaring both, or neither, is a manifest error.
- Repeated `git` fields are manifest errors, not last-value-wins.
- The domain model carries the selector as a sum type (`GitSelector::Branch` / `GitSelector::Revision`), so the invalid states "both selectors" and "no selector" cannot reach the resolution layer.

`revision` means one canonical full SHA-1 commit id, not an arbitrary Git revspec:

```regex
^[0-9a-f]{40}$
```

Abbreviated, uppercase, and whitespace-padded object ids are rejected, as are tags, `HEAD`, relative expressions such as `main~3`, and SHA-256 object ids.
After checkout, the resolver must verify that the resolved `HEAD` exactly equals the requested revision.

The SHA-1-only contract is tied to the current source-host scope: GitHub uses SHA-1 object ids for the supported workflow, and Enozunu does not support SHA-256 repositories.
The object-id contract must be reconsidered when support expands to other Git hosting systems or repository formats, including GitLab.

Resolution and caching key each Git source by `(url, selector kind, selector value)`.
The selector kind is part of the key, so a branch whose name looks like a commit id can never share a resolution or cache slot with an exact revision of the same text.

Provenance records both the declared selector and the materialized commit, using one tagged selector shape for branch and revision sources:

```json
{
  "type": "git",
  "url": "https://github.com/example/repo",
  "selector": {
    "type": "branch",
    "value": "main"
  },
  "path": ".claude/skills/example",
  "resolved_revision": "468aac8caed5f0c3b859b8286968e2c78e2b8760"
}
```

For a revision selector, `selector.value` equals `resolved_revision`, which records that the pin was honored.

## Alternatives Considered

### Optional `branch` and `revision` fields with precedence rules

Keeping both as optional fields and defining precedence (for example, `revision` wins) would accept manifests that do not say what they mean.
A manifest declaring both selectors most likely reflects an editing mistake, and silently preferring one hides that mistake.
Exclusivity keeps the manifest a statement of intent.

### Accepting arbitrary Git revspecs as `revision`

Accepting tags, abbreviated ids, or expressions such as `main~3` would make `revision` mean "whatever Git resolves this to at materialization time".
Most revspecs are mutable or context-dependent, which contradicts the purpose of an exact selector.
Tag selectors remain future work as their own explicitly-designed feature, per [the branch-selector-first ADR](20260708T104203Z_branch-selector-first-reproducibility-deferred.md).

### Recording provenance as optional `branch` / `revision` fields

Adding an optional `revision` next to the existing `branch` field would force consumers to probe which field is present and would misrepresent the manifest contract, which allows exactly one selector.
A single tagged `selector` object mirrors the sum type in the domain model, the same approach the typed `source` object took in [the source reference blocks ADR](20260709T070553Z_source-reference-blocks-and-local-sources.md).

## Consequences

### Positive Consequences

- A repository-hosted Skill or agent can be pinned to one exact commit, matching the pinning capability Gist sources already had.
- Invalid selector combinations are rejected at parse time and cannot reach resolution.
- Branch and revision selectors with similar text cannot collide in resolution or cache keys.

### Negative Consequences

- Provenance consumers reading `source.branch` for Git entries must move to `source.selector`.
- Pinned revisions require a full clone rather than a shallow one, because the pinned commit may not be a branch tip.

### Neutral Consequences

- Reproducibility guarantees (lockfiles, frozen materialization) remain future work; an exact selector pins one source but does not freeze a whole run.
- Tag selectors, symbolic revspecs, version ranges, and latest selectors remain unsupported.

## Compatibility

The provenance version stays `1` even though the Git `source` object shape changes (`branch` is replaced by `selector`).

[The v0.0.x goal](../v0.0.x-goal.md) defines `provenance.json` as an inspection record: it is not a lockfile and is never read back as a resolution input, so no Enozunu component depends on the old shape.
Within the v0.0.x series the record shape may change without a version bump, following the precedent of [the source reference blocks ADR](20260709T070553Z_source-reference-blocks-and-local-sources.md), which restructured the same record while keeping version `1`.
The version field exists to mark a future migration boundary once external consumers are expected to parse the record, not to version every documented shape change during v0.0.x.

## Related

- Issue: [#32](https://github.com/tooppoo/enozunu/issues/32)
- [The branch-selector-first ADR](20260708T104203Z_branch-selector-first-reproducibility-deferred.md), whose deferral of exact revision selectors this ADR ends ahead of the v0.1.x plan
- [The Gist source reference ADR](20260710T220338Z_gist-first-class-source-reference.md), which introduced revision pinning for Gist sources
