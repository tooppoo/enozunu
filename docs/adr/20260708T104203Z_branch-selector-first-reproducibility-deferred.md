# Support Branch Selectors First and Defer Reproducibility to v0.1.x

- Status: Accepted
- Created: 2026-07-08T10:42:03Z

## Context

During v0.0.x, both Enozunu itself and the referenced Skill and agent definitions are expected to change frequently.

Requiring exact revisions from the start would force users to update commit hashes by hand on every configuration change.
That is too heavy for the main v0.0.x goal of centralizing definitions.

Branch selectors, however, are mutable and therefore cannot guarantee exact reproducibility.

## Decision

v0.0.x supports branch selectors before exact revision selectors.

v0.0.x does not guarantee exact reproducibility.

At materialization time, the branch is resolved to a commit hash, and the resolved revision is recorded in `.enozunu/provenance.json`.

`.enozunu/provenance.json` is not a lockfile.
v0.0.x does not use it as a resolution input.

Exact revision selectors and reproducibility guarantees are adopted in v0.1.x.

Tag selectors are introduced after revision selectors.

## Consequences

- v0.0.x easily follows updates to branch heads.
- The same `enozunu.consumer.kdl` may materialize different results at different times.
- `.enozunu/provenance.json` makes it possible to trace which commit the previous materialization used.
- Reproducible install and frozen resolution become responsibilities of v0.1.x or later.

## Related

- Issue: [#8](https://github.com/tooppoo/enozunu/issues/8)
