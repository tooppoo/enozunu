# Generate Root Repository Instructions from Manifest-Declared Bases and Skill Usage Rules

- Status: Accepted
- Created: 2026-08-03T17:28:59Z

## Context

Enozunu materializes Skill and agent sources declared in `enozunu.kdl`, but when each Skill should be used is hand-written separately in root repository instruction files such as `CLAUDE.md`.

Three pieces of knowledge therefore live in different places:

- which Skills a project uses (`enozunu.kdl`)
- when each Skill should be used (hand-written instruction prose)
- the repository instructions a target AI reads at startup (`CLAUDE.md` / `AGENTS.md`)

Adding, removing, or renaming a Skill in the manifest does not update the instruction files, so the manifest and the repository instructions drift apart.

[The Claude and Codex materialization ADR](20260711T184657Z_materialize-claude-and-codex-without-semantic-conversion.md) deferred `AGENTS.md` as a separate artifact kind and listed the questions any later decision must answer: its scope over the repository tree, root versus nested precedence, merge versus replace semantics, its artifact kind, and a target-specific declaration.

This decision changes project responsibility boundaries and generated-output operation, so it is recorded as an ADR rather than only in [issue #38](https://github.com/tooppoo/enozunu/issues/38).

## Decision

### Repository instruction generation is an Enozunu responsibility

Enozunu must generate the root `CLAUDE.md` (Claude) and root `AGENTS.md` (Codex) when the manifest declares a corresponding `provider.instructions.<target>` base document source.

The generated file is deterministically composed from exactly two inputs:

- the target's base document, fetched from a `git` / `local` / `gist` source like any other artifact
- the Skill usage rules derived from the target consumer's `use-skills` selections and their `when` annotations

Generation is limited composition, not authoring: Enozunu concatenates a fixed generated marker, the base document, and a fixed-format Skill usage section. This stays inside the materializer responsibility recorded in [the positioning ADR](20260708T104200Z_position-enozunu-as-configuration-materializer.md) because every sentence is either user-authored content or a mechanical projection of manifest declarations.

### No template engine, no natural-language interpretation

Enozunu must not provide generic template syntax, variable expansion, or conditional logic in base documents or usage rules.

A `when` value is free text describing a usage situation. Enozunu must not evaluate it, parse it, or inspect its grammar; the value is inserted verbatim into the fixed sentence form:

```markdown
- When <when>, always use the `<skill>` skill.
```

A `use-skills` selection without `when` produces the fixed default form:

```markdown
- Always use the `<skill>` skill.
```

Interpreting instruction semantics belongs to the target AI, matching the "Declarations, Not Runtime Semantics" boundary in [the philosophy](../philosophy.md).

### Per-target base documents, no semantic conversion

Each target AI declares its own base document (`provider.instructions.claude`, `provider.instructions.codex`).

Enozunu must not convert one target's base document or instruction style into another's. This applies the no-semantic-conversion rule of [the Claude and Codex materialization ADR](20260711T184657Z_materialize-claude-and-codex-without-semantic-conversion.md) to instruction artifacts.

### Instruction sources are declared under `provider`, bound to a target

`provider.instructions.<target>` is a source declaration: it is resolved, locked, cached, and recorded in provenance exactly like Skill and agent sources, so it belongs on the provider side of the terminology recorded in [the manifest terminology ADR](20260708T104201Z_manifest-terminology-provider-consumer-target-ai.md).

Unlike the shared Skill and agent pools, an instruction source is target-bound, because the artifact itself is target-bound: a root instruction file exists once per target AI and has no cross-target selection to express. The consumer side keeps only what is selection-shaped — which Skills the target uses and the `when` annotations attached to those selections.

An instruction source without a corresponding declared consumer is not resolved, locked, or materialized, matching unselected Skill and agent sources.

### The generated root file is Enozunu-owned and replaced whole

Declaring `provider.instructions.<target>` is an explicit opt-in that transfers ownership of the corresponding root file to Enozunu.

- An existing `CLAUDE.md` / `AGENTS.md` regular file or symlink is replaced whole on summon.
- Manual edits are not preserved, detected, or merged, matching [the generated-output replace-semantics ADR](20260708T104205Z_generated-output-replace-semantics.md).
- If the target path is a directory, summon must fail with a diagnostic instead of deleting the directory tree.
- Adopting the feature on an existing repository means moving hand-written content into a base document first, then declaring the source.

### The generated root file is committed to Git

Root instruction files are read by target AIs — including remote agents — before `enozunu summon` can run, so the documented operation is to commit the generated `CLAUDE.md` / `AGENTS.md`, unlike the generated `.claude/` / `.agents/` / `.codex/` directories.

Synchronization between manifest, lock, base documents, and the committed files is verified in CI by running `enozunu summon --frozen` and checking that the Git diff is empty. No dedicated check command is added for this.

### Removing the declaration does not prune the generated file

Removing `provider.instructions.<target>` stops Enozunu writing that path; it must not delete the previously generated file. This matches the existing pipeline, which does not prune the targets of deselected Skills and agents. Stale outputs are removed explicitly by the user.

## Non-Goals

- nested (sub-directory) `CLAUDE.md` / `AGENTS.md`
- a generic template engine or variable expansion
- partial replacement of, or merging into, generated instruction files
- automatic pruning of previously generated files
- extracting usage conditions from Skill bodies or frontmatter
- runtime evaluation of `when` rules
- converting instruction content between target AIs
- instruction-specific check commands or i18n

## Alternatives Considered

### Keep repository instructions fully hand-written

Rejected because the drift between manifest-declared Skills and hand-written usage rules is the problem this decision exists to remove. Nothing keeps the instruction file mentioning exactly the Skills the manifest selects.

### Provide a template engine over the base document

Rejected because arbitrary substitution and conditionals would make Enozunu an instruction-authoring tool and make output correctness depend on template logic Enozunu would then own. Limited fixed-form composition keeps the output deterministic and reviewable.

### Evaluate `when` as a condition

Rejected because `when` values are natural language. Evaluating them would require interpreting runtime semantics, which [the philosophy](../philosophy.md) deliberately leaves to the target AI.

### One shared base document converted per target

Rejected because it would require the semantic conversion between target AIs that [the Claude and Codex materialization ADR](20260711T184657Z_materialize-claude-and-codex-without-semantic-conversion.md) already rejects for agents; the same reasoning applies to instruction prose.

### Declare instruction sources under `consumer`

Rejected because the declaration names where content comes from and must participate in resolution, locking, and provenance like every other source. Placing a source under `consumer` would break the provider/consumer split recorded in [the manifest terminology ADR](20260708T104201Z_manifest-terminology-provider-consumer-target-ai.md).

## Consequences

### Positive Consequences

- The manifest becomes the single declaration point for which Skills a project uses and when they are used, so Skill renames and removals cannot silently leave stale instruction prose behind.
- Generated instructions are deterministic, so manifest, lock, base documents, and committed root files can be checked for drift mechanically with `summon --frozen` plus a Git diff.
- Instruction base documents gain the same reuse, locking, and provenance properties as every other source.

### Negative Consequences

- A declared root instruction file is no longer hand-editable; manual edits are lost on the next summon. Adopters must migrate hand-written content into a base document.
- Committed generated files can drift from the manifest between summons; the guarantee is only as strong as the documented CI check.
- The fixed sentence forms limit expressiveness: usage rules that do not fit "When ..., always use ..." must live in the base document instead.

### Neutral Consequences

- `config-version` stays `1`; manifests without `provider.instructions` behave exactly as before, and no new root file is generated for them.
- This resolves the `AGENTS.md` deferral of [the Claude and Codex materialization ADR](20260711T184657Z_materialize-claude-and-codex-without-semantic-conversion.md) for the root file only: root scope, replace semantics, artifact kind `instruction`, and a target-bound provider declaration. Nested instruction files remain undecided.
- [The supported targets guide](../../guide/support.md) currently states that Codex `AGENTS.md` is out of scope for agent materialization, and [the philosophy](../philosophy.md) / [the generated output guide](../../guide/generated-output.md) describe target AI-native output as generated directories that are typically Git-ignored; they are updated by the pull requests that implement this decision, together with the committed-root-file exception.
