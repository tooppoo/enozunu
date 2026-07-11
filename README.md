# enozunu

Enozunu is a cross-provider configuration materializer for AI agent tooling.

It centralizes human-authored definitions of AI-agent configuration sources and materializes them into target AI-native configuration paths.

## Quick Start

```sh
curl -fsSL https://raw.githubusercontent.com/tooppoo/reportage/refs/heads/main/install.sh | sh
```

## Status

Enozunu is in early design.

The current v0.0.x goal is intentionally narrow:

- define AI-agent configuration sources in `enozunu.kdl`
- materialize selected sources into Claude-native and Codex-native project paths
- treat generated target AI-native directories as output
- record materialization provenance in `.enozunu/provenance.json`

v0.0.x focuses on centralized definitions, not exact reproducibility.

## What Enozunu Does

Enozunu manages where AI-agent configuration comes from and where it is materialized.

The supported target AIs are Claude and Codex. Claude and Codex select from the same provider source pool, and each selection materializes into that target's native path:

```text
claude + skill -> .claude/skills/<name>/
claude + agent -> .claude/agents/<name>.md
codex  + skill -> .agents/skills/<name>/
codex  + agent -> .codex/agents/<name>.toml
```

A Skill source can be selected from both targets. Agent sources are target-native: a Claude agent is a Markdown file and a Codex custom agent is a TOML file, and Enozunu does not convert between the two.

```kdl
enozunu config-version=1 {
  provider {
    skills {
      skill "git-kura" {
        git {
          url "https://github.com/tooppoo/reportage"
          branch "main"
          path ".claude/skills/git-kura"
        }
      }
    }

    agents {
      agent "shell-script-reviewer-claude" {
        git {
          url "https://github.com/tooppoo/installerer"
          branch "main"
          path ".claude/agents/shell-script-reviewer.md"
        }
      }

      agent "shell-script-reviewer-codex" {
        git {
          url "https://github.com/tooppoo/installerer"
          branch "main"
          path ".codex/agents/shell-script-reviewer.toml"
        }
      }
    }
  }

  consumer {
    claude {
      use-skills "git-kura"
      use-agents "shell-script-reviewer-claude"
    }

    codex {
      use-skills "git-kura"
      use-agents "shell-script-reviewer-codex"
    }
  }
}
```

## What Enozunu Does Not Do

Enozunu does not reimplement Claude, Codex, or any other target AI-native plugin manager.

Enozunu does not validate which target AI a source was originally created for.
It validates artifact shape and materializes it.
It does not convert an agent between target formats, and it does not validate the target-native format.
Whether a reused Skill or agent behaves as expected in a target AI is outside Enozunu's guarantee.

Enozunu also does not try to reconcile generated output with manual edits. If a target AI-native directory needs to be hand-maintained, manage it directly instead of treating it as Enozunu-generated output.

## Usage

Create a starter manifest filled with placeholder values:

```sh
enozunu init
```

This also generates `.enozunu/.gitignore` so the resolver cache under `.enozunu/cache/` stays out of version control.

Validate the manifest of the current project:

```sh
enozunu validate
```

Resolve declared sources and materialize them into target AI project paths:

```sh
enozunu summon
```

All commands operate on `enozunu.kdl` in the project root by default.
Use `--manifest` and `--project-root` to override the defaults.

## File Format Policy

Human-authored configuration uses KDL.
Machine-generated records use JSON.

```text
enozunu.kdl                 # human-authored configuration
.enozunu/provenance.json    # machine-generated derived record
```

## Documentation

The [documentation](docs/README.md) is split by intent.

To use Enozunu, read [the guide](docs/guide/README.md):

- [Why or why not Enozunu?](docs/guide/why-or-why-not.md)
- [Manifest format](docs/guide/manifest.md)
- [Generated output](docs/guide/generated-output.md)

To understand how Enozunu works, read [the design docs](docs/design/README.md):

- [Philosophy](docs/design/philosophy.md)
- [v0.0.x goal](docs/design/v0.0.x-goal.md)
- [Architecture Decision Records](docs/design/adr/README.md)
