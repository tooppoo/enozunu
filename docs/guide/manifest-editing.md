# Editing the Manifest from the CLI

`enozunu add-skill` and `enozunu use-skill` add Skill declarations to `enozunu.kdl` without hand editing.

Both commands follow one editing discipline.
They edit only the root manifest they are given: `use-same-*` references and their referenced targets are never rewritten, and unrelated nodes, comments, and formatting are preserved.
The decision whether anything needs to change uses the effective value, including `use-same-*` expansion.
The changed manifest is re-validated with the ordinary rules and replaces the file atomically, so every error, no-op, and declined-confirmation path leaves the manifest untouched.

Both commands accept `--manifest` and `--project-root` with the same defaults as every other command.
For the declaration formats these commands write, see [the manifest format guide](manifest.md).

## `add-skill`

```sh
enozunu add-skill <skill-id> <source>
```

Adds a Skill source declaration under `provider.skills`, creating the `provider` and `skills` blocks when missing.
`<skill-id>` follows the same name rules as a hand-written `skill` declaration.
`<source>` is either a local path or a GitHub URL.

If the effective `provider.skills` already declares `<skill-id>` with the same source, the command is a no-op.
If it declares `<skill-id>` with a different source, the command fails; it never replaces an existing declaration.

### Local path

```sh
enozunu add-skill review ../catalog/skills/review
```

The path names a Skill directory: a directory containing `SKILL.md`, with no symlinks anywhere in its tree.
This is checked before the manifest changes, so a source `summon` would later reject is rejected now.

A relative path is interpreted from the current directory, and the manifest records it relative to the manifest's own directory — the same base `summon` resolves `local` sources from.
An absolute path is accepted as input, but the recorded path is always relative.

### GitHub URL

```sh
enozunu add-skill review "https://github.com/example/repo/tree/main/skills/review"
```

The accepted forms are:

- `https://github.com/<owner>/<repo>/tree/<ref>/<skill-path>` — a tree URL naming the Skill directory
- `https://github.com/<owner>/<repo>/blob/<ref>/<skill-path>/SKILL.md` — a blob URL naming the Skill's `SKILL.md`
- `https://github.com/<owner>/<repo>` — a repository whose root is itself the Skill directory, at the default branch

Raw URLs, Gist URLs, other hosting services, and Git remote shorthand such as `git@github.com:...` are not accepted.
For a Gist, declare a `gist` source in the manifest directly.

The `<ref>` / `<skill-path>` boundary is never guessed from the URL text alone.
The remote's advertised branches and tags decide it, so a branch or tag containing `/` resolves correctly, and a first segment that is a full 40-character commit id resolves as a `revision`.
A URL that reads as more than one selector and path split — for example a branch and a tag sharing a name — fails as ambiguous instead of picking one reading.
A ref that consumes the whole remainder addresses the repository root and is recorded as `path "."`.

The resolved source must satisfy the Skill source contract (`SKILL.md` in the resolved directory) before anything else happens.
The command then shows exactly the values it would record and asks for confirmation:

```text
skill-id: review
url: https://github.com/example/repo
branch: main
path: skills/review
add this Skill source to enozunu.kdl? [y/N]
```

Only an explicit `y` or `yes` writes the manifest; anything else leaves it untouched.

URL resolution only determines the recorded source reference.
The lock file is neither read nor written; a later `enozunu summon` records the resolved commit as usual.
See [the generated output guide](generated-output.md#the-lock-file) for how locking works.

## `use-skill`

```sh
enozunu use-skill <consumer> <skill-id> [--when <condition>]...
```

Selects a declared Skill for a consumer target by appending one direct `use-skills` declaration at the end of the target block, creating the block when missing.
`<consumer>` is a supported target AI (`claude` or `codex`), and `<skill-id>` must be declared under the effective `provider.skills` — add it with `add-skill` first.

Existing declarations are never edited.
The command relies on the manifest's repeatable selection semantics: several `use-skills` declarations for one Skill are legal, and selections aggregate in declaration order (see [selection node aggregation](manifest.md#selection-node-aggregation)).

### Without `--when`

If the target's effective selection already contains the Skill — directly, through `use-same-skills`, or as a conditional selection — the command is a no-op.
Otherwise it appends a blockless declaration:

```kdl
use-skills "review"
```

### With `--when`

```sh
enozunu use-skill claude review \
  --when "reviewing code" \
  --when "before completion"
```

`--when` is repeatable and each value follows the manifest's `when` contract; duplicate values in one command line count once.
A `when` rule only exists in the generated instruction file, so the target needs an effective instruction source (`provider.instructions.<consumer>`, possibly via `use-same-instruction`); without one the command fails and changes nothing.

Each requested rule already present on the Skill in the target's effective declarations is skipped.
If every rule is already present the command is a no-op.
Otherwise it appends one new declaration carrying only the missing rules, in command-line order:

```kdl
use-skills "review" {
  when "reviewing code"
  when "before completion"
}
```

### Relationship with `use-same-*`

`use-skill` reads through `use-same-skills` when deciding what is already selected, but it never modifies the reference or the referenced target.
When a target follows another via `use-same-skills` and needs one more Skill or rule of its own, the appended direct declaration simply joins the same aggregation:

```kdl
codex {
  use-same-skills "claude"
  use-skills "review" {
    when "before completion"
  }
}
```
