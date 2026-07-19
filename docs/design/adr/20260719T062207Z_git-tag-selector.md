# Support Tag Selectors in Git Source Blocks

- Status: Accepted
- Created: 2026-07-19T06:22:07Z

## Context

[The branch-selector-first ADR](20260708T104203Z_branch-selector-first-reproducibility-deferred.md) planned tag selectors as the step after exact revision selectors.
[The Git exact revision selector ADR](20260712T155345Z_git-exact-revision-selector.md) then added `revision` and deferred tags again, recording that they should arrive as their own explicitly-designed feature rather than as an accepted revspec form.

Until now, a project wanting to track a released version of a Skill or agent had two unsatisfying options.
Following `branch` picks up unreleased work in progress.
Pinning `revision` requires a human to look up the commit a release tag points at and copy the SHA into the manifest, then repeat that on every release.

Adding a third selector raises three contract questions that outlive the implementation:

1. what reproducibility a tag selector does and does not promise, given that a Git tag is a mutable remote ref
2. which ref a tag selector resolves when a repository holds both a branch and a tag of the same name
3. how the selector set stays exclusive and reportable now that it has three members rather than two

These are manifest and machine-readable contract decisions, so they are recorded as an ADR.

## Decision

A `git` source block declares exactly one selector: `branch`, `tag`, or `revision`.

- Declaring none, or more than one, is a manifest error that names every selector actually declared.
- The domain model carries the selector as the existing `GitSelector` sum type, extended with a `Tag` variant, so invalid selector combinations cannot reach the resolution layer.

`tag` is a mutable selector, not a pinning one.

Enozunu resolves the tag on every run and materializes whatever commit it points at.
A Git tag can be moved or deleted on the remote, and Enozunu must not promise an immutability that Git itself does not enforce.
A source that must materialize the same commit on every run must use `revision`, which remains the only exact selector.
This places `tag` alongside `branch` in the reproducibility position recorded in [the branch-selector-first ADR](20260708T104203Z_branch-selector-first-reproducibility-deferred.md): the resolved commit is recorded for inspection, not frozen for replay.

A tag must resolve through the fully-qualified `refs/tags/` namespace.

The resolver fetches `refs/tags/<tag>` explicitly rather than passing the bare name to `git clone --branch`, which accepts branches and tags alike.
A repository holding both a branch and a tag named `release` therefore always resolves the tag for a `tag` selector, and the branch for a `branch` selector.
The fetch names a source ref with no destination, so the result lands in `FETCH_HEAD` and no local tag ref is written; a tag that moved on the remote needs no forced update.
An annotated tag is peeled to its commit, so the recorded revision is always a commit id and never a tag object id.

A tag value must not be empty, must not begin with `-`, and must not contain `:`.

The first two rules match the existing `branch` contract, where a leading `-` would let Git parse the value as an option.
The `:` rule is specific to tags because the value is interpolated into a `refs/tags/<tag>` refspec, where `:` separates source from destination.

Resolution and caching continue to key each Git source by `(url, selector kind, selector value)`, as established for revisions.
The selector kind is part of the key, so a branch and a tag of one name never share a resolution or a cache slot.

Provenance records a tag selector through the existing tagged selector shape:

```json
{
  "type": "git",
  "url": "https://github.com/example/repo",
  "selector": {
    "type": "tag",
    "value": "v1.2.0"
  },
  "path": ".claude/skills/example",
  "resolved_revision": "468aac8caed5f0c3b859b8286968e2c78e2b8760"
}
```

For a tag selector, `resolved_revision` is the only record of which commit the tag pointed at during that run.

## Alternatives Considered

### Treating a tag as an exact, pinning selector

Documenting `tag` as equivalent to `revision` would let a manifest express "this release" and claim reproducibility at the same time.
Git does not support that claim.
A tag is a ref, and `git tag --force` moves it; a deleted and recreated tag can point anywhere.
Promising immutability that the transport cannot enforce would make `provenance.json` misleading in exactly the case a reader cares about, so `tag` is documented as mutable and `revision` remains the only exact selector.

### Warning when a tag resolves to a different commit than the previous run

Detecting a moved tag requires comparing against the previous `resolved_revision` in `.enozunu/provenance.json`.
[The v0.0.x goal](../v0.0.x-goal.md) defines that record as inspection-only output that is never read back as a resolution input.
Adding this warning would make it a resolution input and quietly turn it into a lockfile, which is a v0.1.x decision rather than a side effect of adding a selector.

### Passing the tag name to `git clone --branch`

`git clone --branch` accepts a tag name, so the existing branch resolution path would have worked with no new fetch logic.
It resolves the name through Git's own ref precedence rules, which means a repository holding both a branch and a tag of one name gives the manifest no way to state which it meant.
An explicit `refs/tags/` refspec makes the manifest's intent the deciding factor.

### Fully qualifying `branch` as `refs/heads/` at the same time

Qualifying both selectors would make the ambiguity rules symmetric.
It also changes how every existing branch selector resolves, which is a behavior change to working configurations in service of a case that the tag-side qualification already resolves.
Branch resolution is therefore left unchanged.

## Consequences

### Positive Consequences

- A Skill or agent can be tracked by release tag, using a name that a human can read and verify, without copying commit ids by hand.
- A `tag` selector always resolves a tag, regardless of what branches the repository holds.
- The selector report names every selector a manifest actually declared, so a three-way mistake is diagnosed in one run.

### Negative Consequences

- A tag selector offers no more reproducibility than a branch selector, which may surprise readers who expect release tags to be immutable.
- A third selector adds another form that manifests, documentation, and provenance consumers must handle.

### Neutral Consequences

- Reproducibility guarantees such as lockfiles and frozen materialization remain future work, unchanged by this decision.
- Symbolic revspecs, version ranges, and latest selectors remain unsupported.
- Tag resolution uses a shallow clone, because a tag names a ref whose commit the remote can serve directly; only `revision` needs a full clone.

## Compatibility

The provenance version stays `1`.

This decision adds a `tag` variant to the existing tagged `selector` object rather than changing its shape, so a consumer that dispatches on `selector.type` as [the Git exact revision selector ADR](20260712T155345Z_git-exact-revision-selector.md) specified continues to work and sees one additional type.

The manifest contract is additive for existing manifests: every manifest valid before this change remains valid, because `tag` was previously rejected as an unknown `git` field.
The wording of the missing-selector and conflicting-selector diagnostics changed to enumerate all three selectors, which affects message text only and not diagnostic codes.

## Related

- [The branch-selector-first ADR](20260708T104203Z_branch-selector-first-reproducibility-deferred.md), which planned tag selectors after revision selectors
- [The Git exact revision selector ADR](20260712T155345Z_git-exact-revision-selector.md), which established the selector exclusivity rule, the tagged provenance selector, and the selector-kind cache key that this decision extends
