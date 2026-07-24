# Generated Output

Enozunu-managed target AI-native directories are generated output. For which target AIs are supported and exactly where each artifact is written, see [the supported targets page](support.md).

This guide is operational: what to commit, what regeneration does to your files, and how to inspect what was materialized. For why generated output is not treated as source of truth, see [the philosophy](../design/philosophy.md#generated-output-is-not-a-collaboration-surface). For the scope and policy behind it, see [the v0.0.x goal](../design/v0.0.x-goal.md) and [the v0.1.x goal](../design/v0.1.x-goal.md).

## Git Management

Recommended Git-managed files:

```text
enozunu.kdl
enozunu.lock.json
.enozunu/provenance.json
```

Committing `enozunu.lock.json` is what delivers the reproducibility guarantee: another machine materializes the same commits only if it sees the same lock.

Recommended Git-ignored files:

```text
.claude/
.agents/
.codex/
.enozunu/cache/
```

`enozunu init` generates `.enozunu/.gitignore` containing `cache/`, so the resolver cache under `.enozunu/cache/` is ignored without manual setup.
An existing `.enozunu/.gitignore` is left untouched, so a hand-edited file survives re-running `init`.

The target AI-native output lives at the repository root, outside `.enozunu`, so ignoring it remains a manual choice. Only ignore the directories for the targets your manifest materializes.

Example repository-root `.gitignore`:

```gitignore
# Generated target AI-native configuration
.claude/
.agents/
.codex/
```

If a project chooses to manually maintain a target AI-native directory,
that directory should be treated as ordinary project configuration,
not as Enozunu-generated output.

## What Regeneration Does to Your Files

Skill directory materialization uses replace semantics, not merge semantics.

When a Skill source is materialized to a target's native Skill path, such as:

```text
.claude/skills/<name>/
.agents/skills/<name>/
```

that target directory reflects the source directory after regeneration.

If a supporting file is removed from the source, it is also removed from the target on the next regeneration. This avoids stale files remaining in generated output. The reasoning is recorded in [the replace-semantics ADR](../design/adr/20260708T104205Z_generated-output-replace-semantics.md).

## Editing Generated Output by Hand

Enozunu does not preserve, detect, merge, or reconcile manual edits inside generated output. A hand edit inside a generated directory such as `.claude/`, `.agents/`, or `.codex/` is not source of truth, and it is lost on the next regeneration.

If an edit should be durable, use one of these approaches:

1. change the provider-side source that Enozunu materializes
2. stop treating the target directory as generated output, and Git-manage it as ordinary project configuration

## The Lock File

`enozunu.lock.json` records the resolved commit for every mutable Git selector — `branch` and `tag` — and is the resolution input for later runs:

```json
{
  "version": 1,
  "entries": [
    {
      "url": "https://github.com/example/repo",
      "selector": { "type": "branch", "value": "main" },
      "resolved_revision": "468aac8caed5f0c3b859b8286968e2c78e2b8760"
    }
  ]
}
```

Revision-pinned Git sources, Gist sources, and local sources have no lock entries: the manifest already pins the first two exactly, and a local source has no revision to freeze.

`enozunu summon` is lock-first by default:

- A source with a lock entry materializes the recorded commit, even if the branch or tag has moved upstream.
- A source without a lock entry resolves fresh and is added to the lock.
- A source removed from the manifest is pruned from the lock on the next run.

`enozunu summon --update` re-resolves every mutable selector and rewrites the lock; this is how a locked ref moves. `enozunu summon --frozen` resolves strictly from the lock, never writes it, and fails with `lock-out-of-date` when the lock is missing or lacks an entry for a mutable source; use it in CI. The lock file is rewritten only when its content changes, and the CLI prints a `created` or `updated` line only for a real file change.

A lock guarantees that the same commit is requested, not that the remote still serves it: a commit removed upstream fails resolution with a hint to run `enozunu summon --update`. The design is recorded in [the lockfile ADR](../design/adr/20260724T021001Z_lockfile-based-reproducibility.md).

## Inspecting Provenance

`.enozunu/provenance.json` records the previous materialization result.

Each entry includes information such as:

- source name
- artifact kind
- a typed `source` object
- target AI
- target path

Source-specific fields live under the typed `source` object rather than as top-level fields, so entries stay structurally consistent across source kinds.

The `target AI` is `claude` or `codex`. When one source is materialized to both targets in a run, provenance records one entry per target: the `source` object is identical, and the `target AI` and `target path` differ. The `source` object shape does not depend on the target AI.

For a Git source, the `source` object records:

- `type` (`"git"`)
- `url`
- `selector` (the declared selector: `{"type": "branch" | "tag" | "revision", "value": ...}`)
- `path`
- `resolved_revision`

The `selector` object records what the manifest declared, and `resolved_revision` records the commit that was materialized. For a branch or tag selector the two differ in kind, and `resolved_revision` is the only record of where the mutable ref pointed during that run. For a revision selector, `selector.value` equals `resolved_revision`, which records that the pin was honored.

For a local source, the `source` object records:

- `type` (`"local"`)
- `path` (as written in the manifest)
- `resolved_path` (the canonical filesystem path)

For a Gist source, the `source` object records:

- `type` (`"gist"`)
- `id`
- `revision`
- `file` (agent Gists only)

A Skill Gist materializes the root of the pinned revision, so its `source` object records no `file` key.

A Gist source is recorded as `type: "gist"`, never as `type: "git"`, even though Git transport materializes it. The recorded `revision` equals the pinned Gist revision.

`provenance.json` is not a lockfile, and it is never used as a resolution input. The resolution input is `enozunu.lock.json`; provenance stays the write-only record of what the last run materialized. Because the record is inspection-only output rather than a read-back contract, its shape may change without a version bump; the rationale is recorded in [the Git exact revision selector ADR](../design/adr/20260712T155345Z_git-exact-revision-selector.md#compatibility).

Branch and tag selectors are mutable refs, but a locked source materializes its recorded commit until `summon --update`; a moved ref becomes visible as a lock diff rather than a silent output change. Revision-selected Git sources and pinned Gist sources materialize the same commit on every run without a lock entry. The provenance record exists to make the previous result inspectable.
