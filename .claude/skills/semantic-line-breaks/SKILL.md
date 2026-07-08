---
name: semantic-line-breaks
description: Prevent hard-wrapped prose in documentation and source-code comments. Use when writing or editing Markdown docs, READMEs, ADRs, issue/PR text, inline comments, and doc comments; do not use for ordinary source-code formatting.
---

# Semantic Line Breaks

## Purpose

Prevent mechanical hard-wrapping of natural-language prose.

Line breaks in prose must express semantic structure, not visual column width.

## Core rule

Do not insert a newline inside a natural-language sentence merely because the line is long.

If a prose line feels too long, rewrite the prose. Do not mechanically wrap it.

Use one of these revisions instead:

1. shorten the sentence
2. split it into multiple sentences
3. convert parallel conditions into a list
4. move excessive detail from a source-code comment into documentation

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

If any answer indicates mechanical hard-wrapping, revise the prose before returning the result.
