---
name: documentation-writing
description: Write and edit documentation-oriented prose with semantic line breaks, navigable file references, and clear structure. Use when writing or editing Markdown docs, READMEs, ADRs, design notes, issue/PR text, changelogs, inline comments, and doc comments; do not use for ordinary source-code formatting.
---

# Documentation Writing

## Purpose

Write documentation-oriented prose that is easy to review, navigate, and maintain.

This skill prevents mechanical hard-wrapping of prose and requires file references in Markdown prose to be written as links when practical.

Documentation should optimize for:

1. semantic structure
2. reviewability
3. navigability
4. stable diffs
5. formatter compatibility

Do not optimize prose layout for visual column width.

## Core rules

### Semantic line breaks

Do not insert a newline inside a natural-language sentence merely because the line is long.

Line breaks in prose must express semantic structure, not visual column width.

If a prose line feels too long, rewrite the prose. Do not mechanically wrap it.

Use one of these revisions instead:

1. shorten the sentence
2. split it into multiple sentences
3. convert parallel conditions into a list
4. move excessive detail from a source-code comment into documentation

### File references in Markdown prose

When referring to a file path in Markdown prose, write it as a Markdown link whenever practical.

Prefer:

```md
See [the runtime requirements document](docs/generated-installer-runtime.md).
```

Avoid bare file paths in prose:

```md
See docs/generated-installer-runtime.md.
```

Use a label that explains the role of the file, not only the path, when the surrounding sentence benefits from it.

Good:

```md
The runtime requirements are documented in [the generated installer runtime guide](docs/generated-installer-runtime.md).
```

Also acceptable when the exact path is the useful label:

```md
Update [docs/generated-installer-runtime.md](docs/generated-installer-runtime.md) when runtime requirements change.
```

Do not link file paths when doing so would change the meaning or reduce fidelity.

Do not apply this rule inside:

* code blocks
* terminal examples
* generated snapshots
* machine-readable config
* quoted source material
* inline code where the path is part of a command, value, or syntax example
* files whose format does not support Markdown links

Apply this rule to prose references, not to literal code or generated output.

## Scope

Apply this skill when generating or editing:

* Markdown documentation
* README files
* ADRs
* design notes
* issue descriptions
* PR descriptions
* changelog entries written as prose
* inline source-code comments
* block comments
* JSDoc / TSDoc
* Rustdoc
* Go doc comments
* other natural-language comments embedded in source code

Do not apply this skill to ordinary source-code formatting. Let the project formatter decide code layout.

## Markdown prose

Prefer one logical paragraph per physical line.

Bad:

```md
This command validates the workspace and reports diagnostics
for all configured providers before writing output.
```

Good:

```md
This command validates the workspace and reports diagnostics for all configured providers before writing output.
```

Also acceptable when the project intentionally uses sentence-per-line prose:

```md
This command validates the workspace.
It reports diagnostics for all configured providers before writing output.
```

Do not split a single sentence across lines merely to satisfy a visual line-width preference.

## Source-code comments

Do not split one comment sentence across multiple comment lines merely because of width.

Bad:

```ts
// The runner captures stdout and stderr separately so that
// callers can assert stream-specific behavior.
```

Good:

```ts
// The runner captures stdout and stderr separately so callers can assert stream-specific behavior.
```

Also good:

```ts
// The runner captures stdout and stderr separately.
// This lets callers assert stream-specific behavior.
```

If the comment remains too long, rewrite it into shorter statements or a list. Do not preserve the same sentence and wrap it mechanically.

## Documentation comments

For documentation comments, split at semantic boundaries.

Bad:

```ts
/**
 * Parses the workspace path and rejects absolute paths, empty paths,
 * parent-directory segments, and paths that cannot be normalized safely.
 */
```

Better:

```ts
/**
 * Parses the workspace path.
 *
 * Rejects absolute paths, empty paths, parent-directory segments, and paths that cannot be normalized safely.
 */
```

Better when the rejected cases matter as separate conditions:

```ts
/**
 * Parses the workspace path.
 *
 * Rejects:
 * - absolute paths
 * - empty paths
 * - parent-directory segments
 * - paths that cannot be normalized safely
 */
```

## File path link style

When linking to a repository file, prefer a relative Markdown link.

Good:

```md
See [the design rationale](docs/why-or-why-not.md).
```

Good when the path itself is the clearest label:

```md
See [docs/why-or-why-not.md](docs/why-or-why-not.md).
```

Avoid:

```md
See `docs/why-or-why-not.md`.
```

Use inline code for a path only when the path is being discussed as a literal value rather than as a navigational reference.

Examples:

```md
The default config path is `enozunu.consumer.kdl`.
```

```md
The consumer manifest format is described in [the consumer manifest documentation](docs/consumer-manifest.md).
```

Do not invent links to files that do not exist unless the task is explicitly proposing new files. When proposing new files, make it clear that the link target is proposed or pending.

## Allowed line breaks

A prose line break is allowed when it marks one of these boundaries:

* a new paragraph
* a heading
* a list item
* a table row
* a code block boundary
* a sentence boundary in a project that intentionally uses sentence-per-line prose
* a deliberate separation between distinct comment statements
* a format-required boundary in generated output or snapshots

## Exceptions

Preserve or introduce hard line breaks only when they are required by the surrounding format or by fidelity to source material.

Valid exceptions include:

* exact quotations where line breaks are meaningful
* poetry or verse
* tables
* code blocks
* generated snapshots
* terminal output
* formatter-controlled source code
* files whose existing project convention explicitly requires hard-wrapped prose

Apply exceptions narrowly. Do not generalize an exception to nearby prose.

## Review before final output

Before returning code or documentation, check:

* Did I insert a newline inside a sentence only because of width?
* Did I mechanically wrap Markdown prose?
* Did I split a single `//` comment sentence across multiple lines?
* Could a long comment be rewritten into shorter sentences?
* Would a list express the structure better than a wrapped sentence?
* Did I leave ordinary code formatting to the formatter?
* Did I write Markdown prose file references as links when practical?
* Did I avoid linkifying paths inside code blocks, commands, snapshots, config, and generated output?

If any answer indicates mechanical hard-wrapping or a missing navigational file link, revise the prose before returning the result.
