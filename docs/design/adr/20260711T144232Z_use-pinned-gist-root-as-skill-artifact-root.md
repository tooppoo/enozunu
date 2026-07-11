# Use the Pinned Gist Root as the Skill Artifact Root

- Status: Accepted
- Created: 2026-07-11T14:42:32Z

## Context

[The Gist source reference ADR](20260710T220338Z_gist-first-class-source-reference.md) introduced `gist` as a first-class source kind, limited to agent files. It deferred Skill support until the directory-artifact semantics were decided.

A Skill is a directory artifact containing `SKILL.md` and supporting files, while a Gist is a flat collection of files at one revision. Supporting Skills from Gists therefore requires two decisions: which part of a Gist revision is the Skill artifact, and how the resolver hands a directory artifact to materialization without exposing its Git cache internals.

Before this decision, materialization read artifacts directly out of the resolver's Git checkout. That was tolerable while every artifact was a selected path inside the checkout, but a Skill Gist materializes a whole tree, and a checkout root always contains `.git`. Copying the checkout root as-is would leak Git metadata into generated output; special-casing `.git` during materialization would spread resolver internals across the materialization layer.

These decisions outlive the feature: they fix the Skill Gist manifest contract and establish a resolver/materialization boundary, so they are recorded as an ADR. [The agent-only Gist ADR](20260710T220338Z_gist-first-class-source-reference.md) is kept unchanged as the historical record of the earlier decision.

## Decision

The Gist agent-only restriction is lifted. A Skill Gist is expressed by `id` and a pinned `revision` alone:

```kdl
skill "semantic-line-breaks" {
  gist {
    id "2decf6c462d9b4418f2"
    revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
  }
}
```

The root of the pinned Gist revision is the Skill artifact root:

- the root must contain a regular-file `SKILL.md`
- the whole tree below the root is materialized to `.claude/skills/<name>/` with the existing replace semantics
- the tree follows the existing Skill symlink policy
- no `path` field exists to select a nested Skill root
- `file` is rejected inside a Skill Gist

The agent Gist contract is unchanged: `id + revision + file`, with the existing path, shape, and symlink validation, materialized to `.claude/agents/<name>.md`.

The Skill/agent difference is expressed as a typed selector rather than an optional field:

```rust
pub enum GistArtifactSelector {
    Root,
    File { path: String },
}
```

The parser guarantees that `provider.skills` + `gist` produces `Root` and `provider.agents` + `gist` produces `File`, so an invalid combination cannot reach resolution or materialization.

The resolver-owned Git cache/checkout is separated from what materialization reads. The resolver returns an exported content tree: the clean working tree of the resolved commit, minus `.git` and any other Git repository metadata.

- The export preserves the current working-tree semantics: tracked content, file types (symlinks stay symlinks), and permissions such as the executable bit.
- Untracked files, ignored files, and stale files from a previous resolution are excluded, exactly as the current `reset --hard` + `clean -ffdx` checkout guarantees.
- No `git archive` semantics such as `export-ignore` or `export-subst` are applied, and `git archive` itself is not required.
- Materialization reads only the exported content root and the resolved commit identity; it never reads a resolver cache directory.

Gist resolution identity remains `(id, revision)`. The artifact kind and selector are not part of it, so a Skill and an agent referencing the same Gist revision in one run resolve once and share one exported content tree. Git sources keep their existing `(url, branch)` run-level de-duplication.

Provenance keeps the typed `gist` source object. A Skill Gist records `type`, `id`, and `revision`; `file` is recorded only for agent Gists. Runtime output keeps the existing `gist: <id>@<revision>` origin rendering. No user-visible contract outside Skill Gist support changes.

## Alternatives Considered

### Require a `file`-like `path` selector for Skill Gists

A `path` field selecting a nested directory would mirror the agent `file` selector, but a Gist is a flat file collection in practice, so a nested Skill root has no realistic use. The selector would add contract surface, extra validation, and a second way to express the common case. If a real need appears, `path` can be added later without breaking `id + revision` manifests.

### Keep `file: String` and treat an empty or absent value as "root"

Encoding the root selection as a degenerate `file` value would let an invalid state — a Skill Gist carrying a file selector — flow into resolution and materialization, and every consumer would need to re-check which combination is valid. A typed selector makes the invalid combination unrepresentable after parsing.

### Exclude `.git` during materialization instead of exporting a content tree

Skipping `.git` while copying would fix the immediate metadata leak but would keep materialization coupled to the resolver's checkout layout, and every future materialization path would need to remember the special case. Exporting a metadata-free content tree at the resolver boundary fixes the ownership: Git internals stay behind the resolver, and materialization semantics stay origin-independent.

### Export with `git archive`

`git archive` produces a metadata-free tree, but it applies `export-ignore` and `export-subst` attribute transformations, which would silently change exported content relative to the current clean working tree. A plain filesystem export preserves the existing working-tree semantics exactly.

## Consequences

### Positive Consequences

- A multi-file Skill can be distributed as a single Gist pinned to an exact revision.
- The Skill/agent Gist contracts are enforced by types, so invalid selector combinations cannot reach materialization.
- Materialization no longer reads resolver cache directories, so the resolver layout can change without touching materialization.
- Generated output can never contain `.git` leaked from a resolver checkout.

### Negative Consequences

- Every resolution now copies the working tree into an export, adding I/O proportional to source size.
- The resolver cache holds two copies of each resolved source: the checkout and its export.

### Neutral Consequences

- A Gist revision whose root lacks `SKILL.md` cannot be used as a Skill source; that is the same shape rule every other Skill source follows.
- The `gist` block is the only source reference whose field set depends on the artifact kind.

## Related

- Issue: [#28](https://github.com/tooppoo/enozunu/issues/28)
- ADR: [Gist as a First-Class Source Reference Kind](20260710T220338Z_gist-first-class-source-reference.md)
- ADR: [Source Reference Blocks and Local Sources](20260709T070553Z_source-reference-blocks-and-local-sources.md)
- ADR: [Generated Output Replace Semantics](20260708T104205Z_generated-output-replace-semantics.md)
