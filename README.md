# enozunu

Enozunu is a cross-provider configuration materializer for AI agent tooling.

It centralizes human-authored definitions of AI-agent configuration sources and materializes them into target AI-native configuration paths.

## Status

Enozunu is in early design.

The current v0.0.x goal is intentionally narrow:

- define AI-agent configuration sources in `enozunu.consumer.kdl`
- materialize selected sources into Claude-native project paths
- treat generated target AI-native directories as output
- record materialization provenance in `.enozunu/provenance.json`

v0.0.x focuses on centralized definitions, not exact reproducibility.

## What Enozunu Does

Enozunu manages where AI-agent configuration comes from and where it is materialized.

For v0.0.x, the target AI is Claude only.
A Skill source can be materialized into `.claude/skills/<name>/`.
An agent source file can be materialized into `.claude/agents/<name>.md`.

```kdl
enozunu config-version=1 {
  provider {
    skills {
      skill "git-kura" {
        git "https://github.com/tooppoo/reportage"
        branch "main"
        path ".claude/skills/git-kura"
      }
    }

    agents {
      agent "shell-script-reviewer" {
        git "https://github.com/tooppoo/installerer"
        branch "main"
        path ".claude/agents/shell-script-reviewer.md"
      }
    }
  }

  consumer {
    claude {
      use-skills "git-kura"
      use-agents "shell-script-reviewer"
    }
  }
}
```

## What Enozunu Does Not Do

Enozunu does not reimplement Claude, Codex, or any other target AI-native plugin manager.

Enozunu does not validate whether a source was originally created for Claude.
It validates artifact shape and materializes it.
Whether a reused Skill or agent behaves as expected in a target AI is outside Enozunu's guarantee.

Enozunu also does not try to reconcile generated output with manual edits. If a target AI-native directory needs to be hand-maintained, manage it directly instead of treating it as Enozunu-generated output.

## Usage

Build the CLI with Cargo:

```sh
cargo build --release
```

Validate the manifest of the current project:

```sh
enozunu validate
```

Resolve declared sources and materialize them into Claude project paths:

```sh
enozunu materialize
```

Both commands read `enozunu.consumer.kdl` in the project root by default.
Use `--manifest` and `--project-root` to override the defaults.

## File Format Policy

Human-authored configuration uses KDL.
Machine-generated records use JSON.

```text
enozunu.consumer.kdl        # human-authored configuration
.enozunu/provenance.json    # machine-generated derived record
```

## Documentation

- [Philosophy](docs/philosophy.md)
- [Why or why not Enozunu?](docs/why-or-why-not.md)
- [v0.0.x goal](docs/v0.0.x-goal.md)
- [Manifest format](docs/manifest.md)
- [Generated output policy](docs/generated-output.md)
- [Architecture Decision Records](docs/adr/README.md)
