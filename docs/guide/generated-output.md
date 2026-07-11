# Generated Output

Enozunu-managed target AI-native directories are generated output. For which target AIs are supported and exactly where each artifact is written, see [the supported targets page](support.md).

This guide is operational: what to commit, what regeneration does to your files, and how to inspect what was materialized. For why generated output is not treated as source of truth, see [the philosophy](../design/philosophy.md#generated-output-is-not-a-collaboration-surface). For the scope and policy behind it, see [the v0.0.x goal](../design/v0.0.x-goal.md).

## Git Management

Recommended Git-managed files:

```text
enozunu.kdl
.enozunu/provenance.json
```

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
- `branch`
- `path`
- `resolved_revision`

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

`provenance.json` is not a lockfile. It is not used as a resolution input in v0.0.x.

Because v0.0.x supports branch selectors, materializing the same manifest at different times may produce different results. The provenance record exists to make the previous result inspectable.
