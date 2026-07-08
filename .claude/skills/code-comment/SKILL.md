---
name: code-comment
description: Reviews, writes, revises, and removes code comments by judging whether a comment is needed and ensuring comments explain intent, constraints, invariants, responsibility boundaries, external requirements, rejected alternatives, or deferred decisions rather than translating code into natural language.
---

# Code Comment Skill

## Purpose

This skill helps AI agents and human maintainers review, write, revise, and remove code comments.

The goal is not to increase the number of comments.
The goal is to preserve knowledge that is necessary for safe maintenance and cannot be read directly from the code.

A good comment explains intent, constraints, invariants, responsibility boundaries, external requirements, rejected alternatives, or non-obvious semantic behavior.

A bad comment merely translates code into natural language, compensates for poor naming, records raw history without current relevance, or hides design problems.

## Core Principle

Prefer self-explanatory code over explanatory comments.

Use comments only when the code alone cannot clearly communicate the relevant meaning, reason, constraint, risk, or responsibility boundary.

A comment is justified when it answers at least one of these questions:

* Why does this code exist?
* Why is an apparently simpler alternative not used?
* What semantic behavior must be preserved?
* What invariant must not be broken?
* What role does this module, type, function, or block play in the larger design?
* What external constraint affects this implementation?
* What should future maintainers or AI agents avoid changing accidentally?

Avoid comments that merely restate what the code does line by line.

## Comment Decision Rule

Use a comment when at least one of the following is true:

* the reason is not obvious from the code
* an apparently simpler implementation would be wrong
* an invariant must be preserved
* a module or layer boundary must be protected
* external behavior constrains the implementation
* error handling has non-obvious semantics
* deterministic, compatibility, security, recovery, or user-facing behavior depends on the current shape
* a long or non-obvious block has a semantic purpose that is not clear locally

Do not use a comment when:

* it merely repeats the code
* it explains syntax
* it compensates for poor naming
* it records change history without current relevance
* it states an obvious fact
* it makes complex code look acceptable instead of improving the code

If better naming, typing, or function extraction can make the comment unnecessary, improve the code first.

## Good Comment Categories

### 1. Explain “why”, not just “what”

Bad:

```rust
// Return an error if user_id is missing.
if user_id.is_none() {
    return Err(Error::MissingUserId);
}
```

This only translates the code.

Good:

```rust
// Reject missing user IDs here because audit records must be attributable.
// Continuing would create domain events that cannot be traced to an actor.
if user_id.is_none() {
    return Err(Error::MissingUserId);
}
```

This explains the reason and the consequence.

### 2. Explain why an apparently simpler alternative is not used

Use this when a future maintainer or AI agent is likely to choose an apparently cleaner implementation that would break a hidden requirement.

Bad:

```rust
// Do not canonicalize the path.
```

Good:

```rust
// Do not canonicalize here.
// Diagnostics must point to the user-written project-relative path, not to
// the host filesystem path after resolution.
```

If an ADR, design document, specification, or issue already explains the rejected alternative, do not duplicate the full reasoning in the comment. Keep a short local rule and reference the record.

Good:

```rust
// Do not canonicalize here; diagnostics must preserve user-written paths.
// See ADR-0004 for the path normalization decision.
```

### 3. Describe semantic behavior, not implementation steps

Bad:

```go
// Read the config, then read entries, then build diagnostics.
```

This repeats the procedure.

Good:

```go
// Validation is evidence-collecting rather than fail-fast.
// A single run should report as many user-fixable errors as possible.
```

This describes the intended behavior of the block.

### 4. Explain module responsibility and boundaries

Module comments should explain:

* what this module owns
* what this module does not own
* what abstraction level it works at
* which neighboring module should handle adjacent responsibilities

Good:

```rust
//! Parses user-authored configuration into the domain model.
//!
//! This module handles syntax-level and schema-level validation.
//! It does not resolve filesystem paths or inspect repository state;
//! those checks belong to the validation layer.
```

Responsibility boundaries are especially useful for AI agents because they reduce the risk of placing new logic in the wrong layer.

If an existing architecture document, ADR, or issue defines the boundary, reference it instead of restating the whole design.

Good:

```rust
//! Parses user-authored configuration into the domain model.
//!
//! This module stops at syntax and schema validation.
//! Repository-state validation belongs to the validation layer;
//! see docs/architecture.md for the layer boundary.
```

### 5. State invariants explicitly

Use invariant comments for state transitions, filesystem operations, cache updates, database writes, concurrency, and recovery-sensitive code.

Good:

```rust
// Invariant: pending entries are never deleted unless the consumed record
// has already been written successfully.
```

If the invariant is part of a documented design decision, keep the local invariant visible and reference the durable record.

Good:

```rust
// Invariant: pending entries are never deleted before the consumed record is persisted.
// See ADR-0007 for the consume/ready failure-safety model.
```

### 6. Explain external constraints

Use comments when platform behavior, language behavior, library behavior, file format semantics, compatibility requirements, CLI contracts, security requirements, or user-facing error semantics constrain the implementation.

Good:

```ts
// GitHub Actions treats non-zero exit codes as step failure.
// This branch must exit zero and report the condition as a warning on stderr.
```

Good:

```rust
// KDL accepts this syntax, but config v1 rejects it so that path matching
// remains independent from host-specific normalization behavior.
```

If an existing document explains the external constraint, reference it.

Good:

```ts
// Exit zero here because GitHub Actions treats non-zero exits as step failure.
// See docs/github-actions.md for warning-mode behavior.
```

### 7. Summarize long or non-obvious blocks

A block comment should explain what property the block preserves, not merely name the operation.

Bad:

```go
// Process entries.
```

Good:

```go
// Build the release note in deterministic order.
// This keeps generated diffs stable even when filesystem iteration order differs.
```

### 8. Explain error semantics

Error handling often encodes product or UX decisions that are not obvious from control flow.

Good:

```go
// This is an error rather than a warning because continuing would make the
// generated release note appear complete while pending entries remain unconsumed.
```

Use comments for non-obvious distinctions such as:

* error vs warning
* fail-fast vs collect-errors
* retry vs no retry
* exit non-zero vs exit zero with stderr warning
* user-fixable error vs internal error
* recoverable state vs corrupted state

## Referencing Existing Design Records

Some comments explain decisions that may already be documented elsewhere.

When an ADR, design document, specification, issue, or `TBD.md` entry already explains the relevant background, do not duplicate the full explanation in the code comment. Instead, write a short local summary and reference the document.

This applies especially to comments about:

* why an apparently simpler alternative is not used
* module responsibility and boundaries
* state invariants
* external constraints
* historical background that affects the current implementation
* deferred design questions or intentionally unresolved behavior

The code comment should still be useful at the point of reading.
A bare reference is usually not enough.

Bad:

```rust
// See ADR-0004.
```

Good:

```rust
// Do not canonicalize here; diagnostics must preserve user-written paths.
// See ADR-0004 for the path normalization decision.
```

Bad:

```rust
// See TBD.md.
```

Good:

```rust
// Timeout behavior is intentionally unspecified in v0.
// See TBD.md#timeout-policy before adding timeout handling here.
```

## Historical Background

Do not use code comments as a substitute for historical records.

If historical background is important enough to affect future maintenance, write or update an ADR, design document, or issue first. Then keep the code comment short and point to that record.

Bad:

```rust
// This used to canonicalize paths, but it caused problems in CI on Windows,
// so we changed it after issue #17. The old implementation also made
// diagnostics harder to understand.
```

Good:

```rust
// Keep paths project-relative so diagnostics match user-authored config.
// See ADR-0004 for rejected canonicalization behavior.
```

Historical information belongs in code comments only when it describes a current constraint directly needed to understand the nearby code.

Prefer:

* ADRs for durable design decisions and rejected alternatives
* design documents or specifications for current intended behavior
* issues for active work, migration context, or short-term implementation scope
* `TBD.md` for intentionally unresolved or deferred decisions
* Git history only for investigation, not as the primary explanation in comments

## Deferred Decisions and TBD.md

If a behavior is intentionally deferred, unresolved, or outside the current version scope, record the pending decision in `TBD.md`.

The code comment should not fully explain the unresolved design space. It should identify the local constraint and reference the relevant `TBD.md` entry.

Bad:

```rust
// TODO: add timeout later.
```

Good:

```rust
// Do not introduce timeout behavior in this layer yet.
// Timeout policy is deferred; see TBD.md#timeout-policy.
```

Bad:

```rust
// TODO: decide whether IDs should be UUIDs, numbers, or something else.
```

Good:

```rust
// Target action IDs are internal-only in the first slice.
// Public ID semantics are deferred; see TBD.md#target-action-id-policy.
```

A `TBD.md` reference is appropriate when:

* the behavior is intentionally not decided yet
* the current implementation must avoid accidentally freezing the design
* future work needs to revisit the decision
* the comment would otherwise become a long explanation of open alternatives

A `TBD.md` reference is not appropriate when the decision has already been made.

Once decided, move the rationale to an ADR or design document and update the code comment to reference that durable record instead.

## TODO Comments

Avoid vague TODO comments.

Bad:

```rust
// TODO: improve this
```

Good:

```rust
// TODO: Replace this linear scan if entry lookup becomes observable in large repositories.
// Current expected entry count is small enough that an index would add complexity
// without measurable benefit.
```

A TODO should include at least one of:

* the reason it exists
* the trigger condition for revisiting it
* the expected direction
* the relevant issue, ADR, or `TBD.md` entry

If the TODO represents an unresolved design decision, prefer adding or updating `TBD.md` and referencing it from the comment.

Good:

```rust
// TODO: Keep this parser permissive until config strictness is decided.
// See TBD.md#config-strictness.
```

## Bad Comment Patterns

### 1. Code translation comments

Avoid:

```ts
// Increment count by one.
count += 1;
```

The code is already clearer than the comment.

### 2. Comments that compensate for poor naming

Avoid:

```rust
// x is the release identifier.
let x = parse_arg();
```

Prefer:

```rust
let release_id = parse_arg();
```

### 3. Raw historical notes

Avoid:

```rust
// Changed this on 2026-06-01 because the old implementation was broken.
```

Prefer a comment only when the history explains a current constraint:

```rust
// Keep accepting the legacy field until config v2 is released.
// Older projects generated by v0.1.x still contain this key.
```

If the historical reason is substantial, write an ADR and reference it:

```rust
// Keep accepting the legacy field until config v2 is released.
// See ADR-0012 for the config migration policy.
```

### 4. Comments that hide design problems

Avoid long comments that explain how to mentally execute complicated code.

If a comment needs to explain control flow in detail, first consider:

* renaming variables
* extracting functions
* introducing domain types
* reducing nesting
* splitting responsibilities
* replacing boolean flags with explicit variants

Use comments to preserve intent and constraints, not to make tangled code acceptable.

## Comment Shape with References

Use this shape when referencing external records:

```text
// <short local rule or constraint>.
// See <record> for <reason / decision / deferred topic>.
```

Examples:

```rust
// Pending entries must not be deleted before the consumed record is persisted.
// See ADR-0007 for the consume/ready failure-safety model.
```

```rust
// This module only validates config syntax and schema shape.
// Repository-state validation belongs to the validation layer; see docs/architecture.md.
```

```ts
// Exit zero here because GitHub Actions treats non-zero exits as step failure.
// See docs/github-actions.md for warning-mode behavior.
```

```rust
// Dot segments are rejected rather than normalized.
// See ADR-0005 for the path matching model.
```

```rust
// Target action IDs are internal-only in the first slice.
// Public ID semantics are deferred; see TBD.md#target-action-id-policy.
```

## Review Checklist

When reviewing comments, ask:

1. Does this comment explain something the code does not already say?
2. Does it explain intent, constraint, invariant, responsibility, or semantic behavior?
3. Would the code become misleading or risky without this comment?
4. Is the comment close to the code it explains?
5. Could better naming, typing, or function extraction remove the need for the comment?
6. Is the comment likely to become stale when the code changes?
7. Does the comment prevent a plausible future mistake?
8. Should this explanation live in an ADR, design document, issue, or `TBD.md` instead?
9. If it references another record, does it still include enough local context to be useful?
10. If it describes a deferred decision, is that decision recorded in `TBD.md`?

## AI Agent Behavior

When asked to add, review, or revise comments:

1. First decide whether a comment is needed at all.
2. Prefer code improvement over comment addition when naming or structure is the real problem.
3. Add comments only where they explain intent, constraints, invariants, boundaries, rejected alternatives, external requirements, non-obvious error semantics, or deferred decisions.
4. Remove or rewrite comments that merely translate code.
5. Treat stale or misleading comments as defects.
6. Do not over-comment straightforward code.
7. If a design decision is too large for a code comment, suggest or create an ADR or design document and keep only a short local pointer in the code.
8. If a question is intentionally unresolved, ensure it is recorded in `TBD.md` and reference that entry from the code comment.
9. If an existing ADR, document, issue, or `TBD.md` entry already explains the background, reference it instead of duplicating the explanation.
10. Never use a bare reference when a short local summary is needed to prevent misreading.

## Compact Rule

A comment is justified when it preserves knowledge necessary for safe maintenance that cannot be read directly from the code.

A good comment says:

* why this shape exists
* what must remain true
* what boundary must not be crossed
* what external rule constrains the implementation
* what deferred decision must not be accidentally settled
* where to find the durable record when the full reasoning belongs elsewhere

A bad comment says only what the code already says.
