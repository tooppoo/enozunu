# Supported Targets

This page summarizes which target AIs Enozunu materializes for and where each artifact is written.

For the full manifest contract, see [the manifest format guide](manifest.md). For how to manage the generated directories, see [the generated output guide](generated-output.md).

## Supported AI Agents

| Target AI | Consumer block | Skills | Custom agents | Root instructions |
|-----------|----------------|--------|---------------|-------------------|
| Claude    | `consumer.claude` | supported | supported (Markdown) | generated (`CLAUDE.md`) |
| Codex     | `consumer.codex`  | supported | supported (TOML) | generated (`AGENTS.md`) |

Claude and Codex select from the same `provider.skills` and `provider.agents` pool. The same Skill source can be selected from both targets. Agent sources are target-native: a Claude agent is a Markdown file and a Codex custom agent is a TOML file, and Enozunu does not convert between the two.

Codex `AGENTS.md` is repository instructions rather than a custom agent definition, so it is not an agent artifact. The root `CLAUDE.md` / `AGENTS.md` are instead generated as instruction artifacts when the manifest declares `provider.instructions.<target>`; see [the manifest format guide](manifest.md#root-instructions).

## Materialized File Placement

| Target AI | Artifact | Source shape | Materialized path |
|-----------|----------|--------------|-------------------|
| Claude    | Skill    | directory with `SKILL.md` | `.claude/skills/<name>/` |
| Claude    | Agent    | Markdown file | `.claude/agents/<name>.md` |
| Codex     | Skill    | directory with `SKILL.md` | `.agents/skills/<name>/` |
| Codex     | Agent    | TOML file | `.codex/agents/<name>.toml` |
| Claude    | Instruction | UTF-8 text file (base document) | `CLAUDE.md` |
| Codex     | Instruction | UTF-8 text file (base document) | `AGENTS.md` |

`<name>` is the source declaration name selected by `use-skills` / `use-agents`. The target filename suffix is fixed by the target AI and is not required to match the source path extension.

Unlike Skills and agents, an instruction target is not a verbatim copy: the file is composed from the base document and the target's Skill usage rules, and only materializes when both `provider.instructions.<target>` and the target's `consumer` block are declared. The composition contract is described in [the manifest format guide](manifest.md#root-instructions).

The responsibility boundary behind these placements is recorded in [the Claude and Codex materialization ADR](../design/adr/20260711T184657Z_materialize-claude-and-codex-without-semantic-conversion.md) and, for root instruction files, in [the root repository instructions ADR](../design/adr/20260803T172859Z_generate-root-repository-instructions-from-manifest.md).
