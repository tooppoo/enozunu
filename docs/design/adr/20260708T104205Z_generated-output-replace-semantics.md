# Treat Enozunu-Managed Output as Generated Output Without Hand-Edit Reconciliation

- Status: Accepted
- Created: 2026-07-08T10:42:05Z

## Context

Enozunu materializes the sources declared in `enozunu.consumer.kdl` into target AI-native paths.

A target AI-native directory such as `.claude/` is valid project configuration from the target AI's point of view.
In an Enozunu-managed project, however, it should be treated as generated output.

Preserving, detecting, or integrating hand edits inside generated output would require digests, ownership tracking, merge policies, and conflict resolution.
That falls outside the main v0.0.x goal of centralizing definitions.

## Decision

Enozunu does not aim to combine declarative management with hand editing.

Enozunu-managed output is treated as a product regenerated from `enozunu.consumer.kdl`.

Skill directory materialization must replace, not merge.

A supporting file removed from the source must also disappear from the target after re-materialization.

Enozunu must not preserve, detect, or integrate hand edits in generated output.

To keep hand edits, either manage the target AI-native directory explicitly in Git, or change the provider-side source.

Hand-edit detection by target digest and lockfiles are handled as separate issues when they become necessary.

## Consequences

- The v0.0.x update rules for generated output stay simple.
- It is clear that `enozunu.consumer.kdl` is the primary source of truth.
- Enozunu is not expected to protect or integrate a hand-edited `.claude/`.
- Replace semantics prevent stale supporting files from remaining.
- Hand-edit protection and conflict resolution can be split into future design work.

## Related

- Issue: [#8](https://github.com/tooppoo/enozunu/issues/8)
