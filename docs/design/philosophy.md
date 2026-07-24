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

The supported target AIs are Claude and Codex. Enozunu materializes into each target's native path without converting the artifact between target formats. It does not convert a Claude Markdown agent into a Codex TOML agent, or the reverse. See [the Claude and Codex materialization ADR](adr/20260711T184657Z_materialize-claude-and-codex-without-semantic-conversion.md).

## Declarations, Not Runtime Semantics

Enozunu manages where configuration comes from and where it is materialized.

It does not certify what the target AI will do with the generated files.
It does not promise that a Skill or agent reused across target AIs will behave the same way.

This boundary is deliberate.
Runtime semantics belong to the target AI.
Materialization belongs to Enozunu.

## No Source Origin Validation

Enozunu does not validate which target AI a source was originally created for.

A source URL or source path does not need to be under `.claude/` or any other target-specific location.
A Skill source only needs to have the artifact shape Enozunu requires for the target operation.
For v0.0.x, that means a Skill source is a directory containing `SKILL.md`.

This preserves reuse.
A user may reuse a Claude-distributed Skill from Codex, or the reverse,
but whether it works as expected in the selecting target AI is the user's responsibility.

## Generated Output Is Not a Collaboration Surface

Enozunu-managed target AI-native directories are generated output.

Target AI-native directories such as `.claude/`, `.agents/`, and `.codex/` may be generated from `enozunu.kdl`. Manual edits inside generated output are not treated as source of truth. Enozunu does not try to preserve, detect, or merge manual edits in generated output.

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

The files:

```text
enozunu.kdl                 # human-authored configuration
enozunu.lock.json           # machine-generated resolution input
.enozunu/provenance.json    # machine-generated derived record
```

## Reproducibility Through the Lock File

Branch and tag selectors are mutable refs, and early versions simply followed them: v0.0.x supported branch selectors first to make dogfooding easy while sources changed frequently, and deliberately did not guarantee exact reproducibility.

Since v0.1.x, `enozunu.lock.json` freezes what each mutable selector resolved to.
A default `enozunu summon` materializes the recorded commits; `summon --update` is the explicit step that follows moved refs and rewrites the lock.
An exact `revision` selector still pins a single source in the manifest itself, without a lock entry.

Each file keeps one role.
`enozunu.kdl` is human-authored intent.
`enozunu.lock.json` is the machine-written resolution input.
`.enozunu/provenance.json` records what the last run actually materialized; it is provenance, not a lockfile, and it is never read back as a resolution input.
The decision and its alternatives are recorded in [the lockfile ADR](adr/20260724T021001Z_lockfile-based-reproducibility.md).
