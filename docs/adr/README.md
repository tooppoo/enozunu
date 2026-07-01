# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs).

An ADR records a decision that affects the architecture, public contract, operational model, or long-term maintainability of the project. It should explain not only what was decided, but why the decision was made and what trade-offs were accepted.

## When to Write an ADR

Write an ADR when a decision affects one or more of the following:

* public CLI behavior
* output formats or machine-readable contracts
* persistent metadata or schema design
* worktree, branch, or state-location rules
* runtime or build-time dependency policy
* cross-platform behavior
* destructive-operation safety policy
* project scope, non-goals, or architectural boundaries
* a decision that future contributors are likely to question or revisit

Do not write an ADR for ordinary implementation details, small refactorings, typo fixes, or issue-local discussion unless the decision creates a durable project rule.

## File Naming

ADR files must be placed directly under `docs/adr/`.

Use the following filename format:

```txt
YYYYMMDDTHHMMSSZ_short-kebab-case-title.md
```

Examples:

```txt
20260609T005136Z_json-schema-for-get-output.md
20260609T120000Z_store-kura-state-in-git-common-dir.md
```

Rules:

* The timestamp must be UTC.
* The timestamp in the filename and the `Created` field inside the ADR must represent the same instant.
* Use a short, descriptive, lowercase kebab-case slug after the timestamp.
* Do not rename an ADR after it has been merged, because other ADRs, issues, or pull requests may link to it.

Useful commands:

```sh
date -u +"%Y%m%dT%H%M%SZ"
```

## Status Values

Use one of the following status forms:

```md
- Status: Proposed
- Status: Accepted
- Status: Rejected
- Status: Superseded by [YYYYMMDDTHHMMSSZ_new-decision.md](YYYYMMDDTHHMMSSZ_new-decision.md)
```

Status meanings:

* `Proposed`: the decision is being discussed and has not yet been accepted.
* `Accepted`: the decision is current project policy.
* `Rejected`: the option was considered and explicitly not adopted.
* `Superseded by ...`: the decision was replaced by a later ADR.

Do not delete obsolete ADRs. If a decision changes, create a new ADR and update the old ADR's status to `Superseded by ...`.

## Required Structure

Use this structure for ordinary ADRs:

```md
# Title

- Status: Proposed
- Created: YYYY-MM-DDTHH:MM:SSZ

## Context

Describe the problem, constraints, requirements, and relevant background.

## Decision

State the decision clearly.

Use normative language where appropriate:

- `must` for requirements
- `should` for strong recommendations
- `may` for explicitly allowed options
- `must not` for prohibited behavior

## Alternatives Considered

Describe serious alternatives and why they were not selected.

## Consequences

Describe the effects of the decision.

### Positive Consequences

- ...

### Negative Consequences

- ...

### Neutral Consequences

- ...
```

For small follow-up ADRs, `Alternatives Considered` and the positive/negative/neutral subsections may be omitted when they would add noise. However, every ADR must contain at least:

* `Context`
* `Decision`
* `Consequences`

## Optional Sections

Use additional sections when they clarify the decision.

Common optional sections:

```md
## Non-Goals
```

Use this when the decision could otherwise be misread as expanding the project scope.

```md
## Summary
```

Use this for large ADRs where a final short restatement helps future readers.

```md
## Safety Policy
```

Use this when the decision affects destructive operations, user data, worktree removal, branch deletion, or other potentially unsafe behavior.

```md
## Output Contract
```

Use this when the decision affects script-facing or agent-facing output.

```md
## Compatibility
```

Use this when the decision affects platform support, existing users, schema versions, or future migration paths.

## Writing Rules

ADRs should be written in English.

Each ADR should:

* state the actual decision directly
* separate context from decision
* explain why selected alternatives were rejected
* describe known trade-offs rather than hiding them
* avoid vague claims such as “better”, “cleaner”, or “simpler” without explaining the criterion
* distinguish durable project policy from temporary implementation choices
* link to related ADRs, issues, or pull requests when they are relevant
* include examples when they clarify CLI behavior, file layout, schema shape, or safety behavior

An ADR should not be a general design document. It should record a decision. If a document mostly describes how to implement a known decision, put it elsewhere under `docs/`.

## Updating ADRs

After an ADR is accepted, avoid rewriting its decision.

Allowed updates:

* typo fixes
* formatting fixes
* broken-link fixes
* adding links to later ADRs
* changing `Status` to `Superseded by ...`

Not allowed:

* silently changing the decision
* rewriting the rationale to match later implementation choices
* deleting alternatives that were actually considered
* removing known negative consequences

If the project changes direction, create a new ADR instead.

## Template

Use [TEMPLATE.md](./TEMPLATE.md)
