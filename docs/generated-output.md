# Generated Output Policy

Enozunu-managed target AI-native directories are generated output.

For v0.0.x, this primarily means `.claude/`.

## Source of Truth

The primary source of truth is:

```text
enozunu.kdl
```

The generated target AI-native output is not source of truth.

For v0.0.x, `enozunu.kdl` is human-authored KDL. `provenance.json` is machine-generated JSON.

```text
enozunu.kdl                 # human-authored configuration
.enozunu/provenance.json    # machine-generated derived record
```

## Git Management

Recommended Git-managed files:

```text
enozunu.kdl
.enozunu/provenance.json
```

Recommended Git-ignored files:

```text
.claude/
.enozunu/cache/
```

`enozunu init` generates `.enozunu/.gitignore` containing `cache/`, so the resolver cache under `.enozunu/cache/` is ignored without manual setup.
An existing `.enozunu/.gitignore` is left untouched, so a hand-edited file survives re-running `init`.

The `.claude/` generated output lives at the repository root, outside `.enozunu`, so ignoring it remains a manual choice.

Example repository-root `.gitignore`:

```gitignore
# Generated target AI-native configuration
.claude/
```

If a project chooses to manually maintain `.claude/`,
that directory should be treated as ordinary project configuration,
not as Enozunu-generated output.

## Replace Semantics

Skill directory materialization uses replace semantics,
not merge semantics.

When a Skill source is materialized to:

```text
.claude/skills/<name>/
```

that target directory should reflect the source directory.

If a supporting file is removed from the source, it should also be removed from the target after regeneration.

This avoids stale files remaining in generated output.

## Manual Edits

Enozunu does not aim to support both declarative management and manual edits inside generated output.

If generated output is edited by hand, Enozunu does not promise to:

- preserve that edit
- detect that edit
- merge that edit
- reconcile that edit with the provider source

Manual edits are not source of truth.

If an edit should be durable,
use one of these approaches:

1. change the provider-side source that Enozunu materializes
2. explicitly Git-manage the target AI-native directory instead of treating it as generated output

## Provenance

`.enozunu/provenance.json` records the previous materialization result.

Each entry should include information such as:

- source name
- artifact kind
- a typed `source` object
- target AI
- target path

Source-specific fields live under the typed `source` object rather than as top-level fields, so entries stay structurally consistent across source kinds.

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
- `file`

A Gist source is recorded as `type: "gist"`, never as `type: "git"`, even though Git transport materializes it. The recorded `revision` equals the pinned Gist revision.

`provenance.json` is not a lockfile.
It is not used as a resolution input in v0.0.x.

Because v0.0.x supports branch selectors, materializing the same manifest at different times may produce different results. The provenance record exists to make the previous result inspectable.

## Out of Scope

The following are outside v0.0.x:

- target digest based hand-edit detection
- generated output and manual edit reconciliation
- lockfile-based reproducibility
- frozen materialization
- exact revision selector support

These can be introduced later as separate design work.
