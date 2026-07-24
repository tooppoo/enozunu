# Why or Why Not Enozunu?

Enozunu is useful when Skills and agents should remain maintained at their source,
while each project declaratively selects and materializes the target AI-native configuration it consumes.

It separates three responsibilities:

- source authors maintain Skills and agents
- project manifests declare sources and target selections
- Enozunu resolves those declarations and writes generated files to target AI-native paths

The result is not merely less copying. The configuration becomes explicit,
regenerable from declarations, and traceable through materialization provenance.

## Main Use Cases

### Reuse Skills and Agents Without Copying Them

Without Enozunu, reusing a Skill or agent commonly means copying its files into each repository's `.claude/`, `.agents/`, or `.codex/` directories.
Those copies can drift from their source, and the source revision or intended configuration may become unclear.

With Enozunu, a project declares the source and target selection in `enozunu.kdl`, then runs `enozunu summon` to materialize the target-native files.
To reuse the same configuration elsewhere, reuse the relevant manifest declarations instead of copying generated directories.

This provides:

- a declarative record of which Skills and agents a project consumes
- one source that can be reused by multiple projects or supported target AIs
- regeneration of target-native configuration from the manifest
- provenance showing what source was materialized
- a lock file that keeps `branch` and `tag` selections on the same commit across runs and machines

A `branch` or `tag` selector stays on its locked commit until an explicit `enozunu summon --update`.
Use an exact `revision` when a Git source must stay pinned to one commit independently of the lock.

### Distribute Tool-Specific Skills and Agents Without a Custom Installer

A tool author may want to distribute a Skill or agent that explains how an AI agent should use, review, or operate that tool.
Without a shared materialization mechanism, each tool must document copy steps or implement its own setup script for target-specific configuration directories.

With Enozunu, the author can keep the Skill or agent in the tool repository or a Gist and provide an Enozunu-compatible manifest declaration.
Users add the declaration to their project's `enozunu.kdl` and materialize it through the same `enozunu summon` workflow used for other Skills and agents.

This removes the need for each tool to build a bespoke configuration installer.
It does not transfer all distribution responsibilities to Enozunu: the tool author still maintains the artifact, its target-native format, compatible revisions, and usage guidance.

For the declaration syntax, see [the manifest format guide](manifest.md).

## Use Enozunu When

Use Enozunu when you want to keep target AI-native directories out of the main source of truth.

Examples:

- you do not want to commit generated `.claude/`, `.agents/`, or `.codex/` directories in every repository
- you want to reuse the same Skill or agent source across multiple projects
- you want a project to declare which AI-agent configuration sources it consumes
- you want target AI-native configuration to be regenerated from a manifest
- you want to inspect what source revision was materialized
- you maintain a tool and want to distribute tool-specific Skills or agents without implementing a custom setup mechanism

Concretely, this means declaring sources in `enozunu.kdl` and materializing them into supported Claude or Codex project paths.

## Do Not Use Enozunu When

Do not use Enozunu when you want a target AI-native plugin or package manager.

Enozunu does not discover packages, solve dependencies, install runtime plugins, or execute them.
It resolves explicitly declared configuration sources and materializes configuration files.

Do not use Enozunu when you want runtime compatibility guarantees.

Enozunu does not guarantee that a Skill or agent created for Claude will work in Codex or another target AI.
A source may be selected for multiple targets where its format is compatible, but compatibility remains the source author's and user's responsibility.
Enozunu does not semantically convert agent definitions between target-native formats.

Do not use Enozunu when you want manual edits inside generated output to be preserved or merged.

Generated output is regenerated from declarations.
If a target AI-native directory needs to be hand-maintained, make it explicit Git-managed project configuration instead of treating it as Enozunu output.

## Not a Good Fit

Enozunu is not a good fit if you need:

- version ranges
- dependency solving
- registry or marketplace discovery
- importing or composing third-party manifest fragments
- source origin validation
- automatic hook or MCP configuration
- generated-output hand-edit reconciliation

Those may be future issues, but they are not part of the current goal.

## Good Fit

Enozunu is a good fit if you accept these constraints:

- Claude and Codex are the supported target AIs
- a `git` source selects its commit with exactly one selector: a mutable branch, a mutable tag, or an exact revision
- GitHub tree/blob URL shorthand is not supported
- Skill sources are directories containing `SKILL.md`
- agent sources are target-native files: Markdown for Claude and TOML for Codex
- `.claude/`, `.agents/`, and `.codex/` are generated output for the selected targets
- `enozunu.lock.json` is a machine-generated resolution record and must be committed for reproducibility
- `.enozunu/provenance.json` is a machine-generated derived record
- a lock guarantees the same commit is requested, not that the remote still serves it
