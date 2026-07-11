# Materialize Claude and Codex Configurations Without Semantic Conversion

- Status: Accepted
- Created: 2026-07-11T18:46:57Z

## Context

Enozunu is a configuration materializer that centralizes configuration sources for AI agent tooling in a manifest and materializes them into the native paths each target AI reads.

Before this decision, Claude was the only target AI. Enozunu materialized to the following paths.

```text
skill -> .claude/skills/<name>/
agent -> .claude/agents/<name>.md
```

Codex also loads Skills and custom agents at project scope, but its native paths and agent format differ from Claude's.

```text
skill -> .agents/skills/<name>/
agent -> .codex/agents/<name>.toml
```

Both target AIs treat a Skill as a directory with `SKILL.md` at its root. A Claude agent is a Markdown file and a Codex custom agent is a TOML file, so their formats and meaning contracts are not identical.

If Enozunu converted between a Claude agent and a Codex custom agent, it would take on more than a projection into a target-native path. It would own cross-target semantic conversion, field mapping, default population, and compatibility guarantees. That expands Enozunu's responsibility as a configuration materializer and makes Enozunu answerable for meaning lost or misread during conversion.

Codex `AGENTS.md` is repository instructions applied to the repository tree, not a custom agent definition. Treating it as the same artifact kind as a custom agent would conflate different lifetimes, scopes, and resolution semantics.

This decision is recorded as an ADR because it establishes a lasting boundary: Enozunu projects sources into target-native paths but does not convert, validate, or guarantee target-native semantics. It also updates the earlier "Claude-only target AI" scope.

## Decision

### Add Codex as a target AI

`consumer.codex` is added to the manifest contract with the same source selection, resolution, artifact validation, materialization, and provenance pipeline as Claude.

The manifest `config-version` stays `1`. This change does not invalidate existing Claude manifests; it is an additive change that adds `consumer.codex` to the existing structure.

### Materialize to target-native paths

Materialization targets are determined by the combination of target AI and artifact kind.

```text
claude + skill -> .claude/skills/<name>/
claude + agent -> .claude/agents/<name>.md
codex  + skill -> .agents/skills/<name>/
codex  + agent -> .codex/agents/<name>.toml
```

The target path decision is the responsibility of the target-aware planner. The materialization executor is not duplicated per target AI.

### Share the provider source pool across targets

`provider.skills` and `provider.agents` are a single source declaration pool shared by Claude and Codex.

The same Skill source can be selected from both `consumer.claude` and `consumer.codex`.

Source resolution identity does not include the target AI. Git, local, and Gist sources keep their existing identity and de-duplication semantics, so materializing one source to multiple targets does not resolve it more than once in a single run.

### Share the Skill artifact shape

A Claude Skill and a Codex Skill are the same artifact shape inside Enozunu.

- it is a directory
- it has a regular-file `SKILL.md` at its root
- the whole supporting-file tree is materialized
- the symlink policy and replace semantics follow the existing contract

Enozunu does not guarantee that the same Skill has the same meaning, capability, or safety under Claude and Codex.

### Place target-native agent sources verbatim

A Claude agent and a Codex custom agent are supplied as target-native files by the provider.

```text
claude agent source -> Markdown file
codex agent source  -> TOML file
```

Enozunu does not do any of the following.

- convert a Claude Markdown agent into a Codex TOML agent
- convert a Codex TOML agent into a Claude Markdown agent
- inject, remove, or rewrite fields inside a target-native file
- guarantee that a source declaration name matches a name field inside the file
- semantically validate a Claude agent Markdown
- semantically validate a Codex custom agent TOML
- automatically determine target compatibility

For an agent source, Enozunu validates only that the source is a regular file, as it does today.

### Represent the target AI as a domain type

The target AI is a closed domain type rather than an unvalidated string.

```rust
pub enum TargetAi {
    Claude,
    Codex,
}
```

Consumer selection and planned materialization carry a `TargetAi`. The target-native path, the provenance `target_ai`, and the CLI target rendering are all derived from this domain type.

### Record provenance per target

When one source is materialized to both Claude and Codex, provenance records one entry per target.

```json
{
  "source_name": "git-kura",
  "kind": "skill",
  "target_ai": "codex",
  "target_path": ".agents/skills/git-kura"
}
```

Source identity and source-specific provenance fields do not change by target AI.

The provenance version stays `1`, because the existing entry shape already holds `target_ai` and `target_path` as open string fields and only needs a new value.

Provenance remains a derived record. It is not used as a lockfile or a resolution input.

### Defer `AGENTS.md` as a separate artifact kind

Codex `AGENTS.md` is not a custom agent definition, so it is out of scope for agent materialization in this decision.

If `AGENTS.md` is handled later, the following must be considered separately.

- its scope over the repository tree
- root versus nested file precedence
- merge versus replace semantics
- its artifact kind in the provider model
- a target-specific declaration for a concept Claude has no counterpart for

### Update the scope of the earlier Claude-only decision

Earlier ADRs and design documents state that "the v0.0.x target AI is Claude only". That statement is kept as historical record.

This ADR updates the part of that judgment that limited the target AI, so the covered target AIs are Claude and Codex. The following judgments are kept.

- Enozunu does not replace a target AI-native format
- Enozunu does not perform origin validation
- Enozunu does not perform cross-provider semantic conversion
- Enozunu does not treat generated output as source of truth
- Enozunu does not preserve, detect, or integrate hand edits
- Enozunu keeps a shared source resolution and materialization pipeline

## Alternatives Considered

### Automatically convert a Claude agent into a Codex agent

Not adopted.

It would require defining not only format conversion but also field mapping, defaults, unsupported capabilities, and meaning preservation for instructions. It would also require guaranteeing that the converted result behaves as expected in the target AI, which exceeds the responsibility of a configuration materializer.

### Split provider declarations into target-specific pools

For example:

```text
provider.claude.agents
provider.codex.agents
```

Not adopted for the initial support.

It makes a Skill source harder to share across targets and breaks the separation between source management and target projection. An agent source can be declared under a distinct name and selected per consumer instead.

If representing target compatibility in the type system becomes concretely necessary, it should be considered as artifact capability metadata or a target constraint rather than splitting the whole provider declaration.

### Support only the Codex Skill first

Not adopted.

It would not satisfy "Codex support on par with Claude" and would force the consumer and planning models to change again within a short period. Both Skill and custom agent are covered from the start.

### Parse and validate the Codex agent TOML

Not adopted for the initial support.

Holding a target-native semantic validator inside Enozunu would make Enozunu responsible for tracking Codex format changes and version compatibility. The initial support validates only the regular-file shape.

### Treat `AGENTS.md` as an agent source

Not adopted.

`AGENTS.md` differs from a custom agent definition in role, scope, and placement semantics. Treating it under the same `provider.agents` would conflate distinct concepts.

### Bump the provenance version

Not adopted.

The existing entry shape already has `target_ai` and `target_path`, so it can represent the change by adding the new value `codex`. A consumer that assumed `target_ai` was a closed enum of only `claude` needs to update its own contract, but the JSON document shape itself does not change.

## Consequences

### Positive Consequences

- one manifest can generate Claude and Codex project configuration
- the same Skill source can be reused across multiple targets
- source resolution, artifact validation, filesystem safety, and replace semantics stay shared
- adding a target AI avoids duplicating the resolver and executor
- because agent conversion is not performed, Enozunu does not have to guarantee correct conversion of target-specific semantics
- provenance makes the source-to-target correspondence explicit

### Negative Consequences

- using an agent on both Claude and Codex requires managing a target-native source file for each
- placing the same Skill source on both targets does not guarantee runtime compatibility or meaning preservation
- a Codex custom agent TOML syntax error is not detected during materialization and may first fail on the Codex side
- the provider model alone does not express, at the type level, which target an agent source is meant for
- in addition to `.claude/`, `.agents/` and `.codex/` become generated output, so gitignore and operational guidance grow

### Neutral Consequences

- the provenance version stays `1`
- the manifest `config-version` stays `1`

## Related

- [the Claude-only v0.0.x goal](../v0.0.x-goal.md), whose target-AI limitation this ADR updates
- [the manifest terminology ADR](20260708T104201Z_manifest-terminology-provider-consumer-target-ai.md), which defines provider, consumer, and target AI
- [the no source origin validation ADR](20260708T104202Z_no-source-origin-validation.md), which this ADR extends across target AIs
- [the generated output replace-semantics ADR](20260708T104205Z_generated-output-replace-semantics.md), whose policy is kept
- [the Gist source reference ADR](20260710T220338Z_gist-first-class-source-reference.md), whose source support is shared by the Codex target
