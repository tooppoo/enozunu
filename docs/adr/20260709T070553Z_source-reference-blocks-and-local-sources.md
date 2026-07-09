# Source Reference Blocks and Local Sources

- Status: Accepted
- Created: 2026-07-09T07:05:53Z

## Context

Until now, a provider source declaration carried Git-only fields (`git`, `branch`, `path`) directly under the `skill` / `agent` node, and the Rust model represented a source reference as a Git-only struct.

During local development, it should be possible to materialize a Skill or agent from a local filesystem path without first publishing it through a remote Git repository, for example when iterating against a sibling repository.

Adding a second source kind raises structural questions that outlive this feature: how the manifest distinguishes source kinds, how many source references one declaration may carry, and how provenance output stays consistent when entries no longer share Git-specific fields.

## Decision

Provider source references are represented as tagged source reference blocks.

Each `skill` or `agent` declaration must contain exactly one source reference block. Declarations with no source reference block, both `git` and `local`, multiple blocks of the same kind, or unsupported block kinds must be rejected.

`git` and `local` are the initial supported source reference kinds:

```kdl
skill "git-kura" {
  git {
    url "https://github.com/tooppoo/reportage"
    branch "main"
    path ".claude/skills/git-kura"
  }
}

skill "local-git-kura" {
  local {
    path "../reportage/.claude/skills/git-kura"
  }
}
```

The Rust model mirrors this as a tagged union:

```rust
pub enum SourceReference {
    Git { url: String, branch: String, path: String },
    Local { path: String },
}
```

Provenance JSON preserves the same structure: source-specific fields live under a typed `source` object with a `type` discriminator, instead of Git-specific top-level fields such as `source_url` or `resolved_revision`.

Local source semantics:

- Relative `local.path` values are resolved from the manifest file's containing directory, not from the process working directory.
- `local.path` may contain `..` so sibling repositories can be referenced.
- Absolute `local.path` values are rejected in v0.0.x. Absolute path support may be reconsidered later, for example for a git-ignored user-local override file, but accepting it in the shared manifest now would weaken portability.

Local source artifact validation is still shape-based, not origin-based, consistent with [the decision not to validate source origin](20260708T104202Z_no-source-origin-validation.md): a Skill source must be a directory containing `SKILL.md`, and an agent source must be a file.

Local sources must keep the filesystem safety policy at least as strict as Git sources:

- a `local.path` that resolves to a symlink is rejected
- symlinked Skill contents are rejected
- a resolved local source path that equals, contains, or is contained by its materialization target path is rejected, because Git sources are copied from cache checkouts while local sources can point back into the target project

## Alternatives Considered

### Keep flat fields and add a mutually exclusive `local-path` field

A flat `skill { git ...; branch ...; path ...; local-path ... }` shape avoids one nesting level, but the mutual-exclusion rule becomes implicit: nothing in the syntax shows which fields belong to which source kind, and each new source kind multiplies the invalid field combinations to document and reject. The block form makes the source kind and its field set explicit in the syntax itself.

### Model local sources as `file://` Git URLs

Reusing the Git reference with a `file://` URL would avoid a new reference kind, but it forces local paths through Git resolution semantics (`branch`, resolved revision) that do not apply to a plain directory, and it cannot express "the artifact itself" without a checkout-relative `path`. The mismatch would surface as artificial required fields.

### Keep Git-specific top-level provenance fields and add local ones beside them

Adding `local_path` next to `source_url` and `resolved_revision` keeps the JSON flat, but every consumer must then probe which field set is present. A typed `source` object with a `type` discriminator lets consumers dispatch on one field and keeps future source kinds additive.

## Consequences

### Positive Consequences

- Skills and agents can be materialized from local paths during development, before they are distributed through a remote Git repository.
- The manifest syntax, the Rust model, and the provenance JSON share one tagged-union structure, so adding a future source kind is an additive change in all three places.
- The exactly-one-block rule gives declarative, syntax-visible mutual exclusion between source kinds.

### Negative Consequences

- This is a breaking change to the manifest format: existing manifests using the flat `git` + `branch` + `path` fields must be migrated to the `git { url ... }` block form.
- The provenance JSON shape changes: consumers reading `source_url`, `branch`, `resolved_revision`, or `source_path` at the top level must move to the typed `source` object.
- Local sources make materialization host-dependent: a manifest with a `local` reference only works where that path exists, so shared manifests should prefer Git sources.

### Neutral Consequences

- Local sources have no resolved revision; provenance records the canonical resolved path instead, so entries are inspectable but not reproducible references.
- Git source de-duplication stays keyed by `(url, branch)`; local sources are resolved per manifest-relative path without a resolver cache.

## Related

- Issue: [#14](https://github.com/tooppoo/enozunu/issues/14)
- ADR: [Do Not Validate Source Origin](20260708T104202Z_no-source-origin-validation.md)
