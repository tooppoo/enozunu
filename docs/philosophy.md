# Philosophy

Enozunu centralizes declarations.
It does not own runtime meaning.

The project exists to make AI-agent configuration sources explicit, shareable, and materializable across projects without committing generated target AI-native directories everywhere.

## Configuration Materialization, Not Plugin Management

Enozunu does not reimplement target AI-native plugin managers.

Claude, Codex, and other target AIs may have their own official plugin, skill, agent, or configuration systems.
Enozunu treats those systems as target formats.
It does not replace them.

The job of Enozunu is narrower:

```text
declare source -> resolve source -> materialize to target AI-native path
```

For v0.0.x, the only target AI is Claude.

## Declarations, Not Runtime Semantics

Enozunu manages where configuration comes from
and where it is materialized.

It does not certify what the target AI will do with the generated files.
It does not promise that a Skill or agent reused across target AIs will behave the same way.

This boundary is deliberate.
Runtime semantics belong to the target AI.
Materialization belongs to Enozunu.

## No Source Origin Validation

Enozunu does not validate whether a source was originally created for Claude.

A source URL or source path does not need to be under `.claude/`.
A Skill source only needs to have the artifact shape Enozunu requires for the target operation.
For v0.0.x, that means a Skill source is a directory containing `SKILL.md`.

This preserves reuse.
A user may reuse a Claude-distributed Skill elsewhere in the future,
but whether it works as expected in another target AI is the user's responsibility.

## Generated Output Is Not a Collaboration Surface

Enozunu-managed target AI-native directories are generated output.

For v0.0.x, `.claude/` may be generated from `enozunu.consumer.kdl`. Manual edits inside generated output are not treated as source of truth. Enozunu does not try to preserve, detect, or merge manual edits in generated output.

If manual editing is required,
manage the target AI-native directory directly,
or change the provider-side source that Enozunu materializes.

## Human and Machine File Formats

Human-authored configuration should be easy to read and edit.
Machine-generated records should be stable for tools to process.

Therefore:

```text
KDL   -> human-authored configuration
JSON  -> machine-generated records
```

In v0.0.x:

```text
enozunu.consumer.kdl        # human-authored configuration
.enozunu/provenance.json    # machine-generated derived record
```

## Reproducibility Is Deferred

v0.0.x supports branch selectors first.
This makes early dogfooding easier while Skill and agent sources change frequently.

Branch selectors are mutable.
Therefore v0.0.x does not guarantee exact reproducibility.

At materialization time, Enozunu records the resolved commit in `.enozunu/provenance.json`. That record is provenance, not a lockfile. Exact revision selectors and reproducibility guarantees are future work.
