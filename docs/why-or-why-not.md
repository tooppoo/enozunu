# Why or Why Not Enozunu?

Enozunu is useful when AI-agent configuration should be declared once
and materialized into target AI-native paths.

It is not useful when the desired workflow is to manually maintain generated target directories.

## Use Enozunu When

Use Enozunu when you want to keep target AI-native directories out of the main source of truth.

Examples:

- you do not want to commit generated `.claude/` directories in every repository
- you want to reuse the same Skill or agent source across multiple projects
- you want a project to declare which AI-agent configuration sources it consumes
- you want target AI-native configuration to be regenerated from a manifest
- you want to track what source revision was materialized

For v0.0.x,
this means declaring sources in `enozunu.consumer.kdl`
and materializing them into Claude project paths.

## Do Not Use Enozunu When

Do not use Enozunu when you want a target AI-native plugin manager.

Enozunu does not install,
resolve,
or execute target AI plugins as a runtime.
It materializes configuration files.

Do not use Enozunu when you want runtime compatibility guarantees.

Enozunu does not guarantee that a Skill or agent created for Claude
will work in Codex or another target AI.
Future cross-target reuse is allowed,
but compatibility is the user's responsibility.

Do not use Enozunu when you want manual edits inside generated output
to be preserved or merged.

Generated output is regenerated from declarations.
If `.claude/` needs to be hand-maintained,
make it explicit Git-managed project configuration instead of treating it as Enozunu output.

## Not a Good Fit for v0.0.x

v0.0.x is not a good fit if you need:

- Codex target materialization
- `.agents/` materialization
- exact revision selectors
- lockfile-based reproducibility
- tag selectors
- version ranges
- dependency solving
- registry or marketplace discovery
- source origin validation
- automatic hook or MCP configuration
- generated-output hand-edit reconciliation

Those may be future issues,
but they are not part of the v0.0.x goal.

## Good Fit for v0.0.x

v0.0.x is a good fit if you accept these constraints:

- Claude is the only target AI
- `git` + `branch` + `path` is the only supported source reference shape
- GitHub tree/blob URL shorthand is not supported
- Skill sources must be directories containing `SKILL.md`
- agent sources are files materialized into `.claude/agents/<name>.md`
- `.claude/` is generated output
- `.enozunu/provenance.json` is a machine-generated derived record
- exact reproducibility is not guaranteed
