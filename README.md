# enozunu

Enozunu(`役小角`) is a cross-provider configuration materializer for AI agent tooling.

It centralizes human-authored definitions of AI-agent configuration sources and materializes them into target AI-native configuration paths.

## Why Enozunu?

Enozunu separates Skill and agent sources from the target-native files generated in each project.

- **Reuse Skills and agents declaratively.** Declare sources and selections in `enozunu.kdl` instead of copying configuration files between projects.
- **Distribute tool-specific Skills and agents without a custom installer.** Tool authors can publish the artifacts and an Enozunu-compatible manifest declaration instead of implementing their own setup mechanism.

See [Why or Why Not Enozunu?](docs/guide/why-or-why-not.md) for the detailed use cases, responsibility boundaries, and current limitations.

## Quick Start

```sh
curl -fsSL https://raw.githubusercontent.com/tooppoo/enozunu/refs/heads/main/install.sh | sh
```

## Overview

Enozunu manages where AI-agent configuration comes from and where it is materialized. You declare sources once in `enozunu.kdl`, and Enozunu resolves them and writes them into each target AI's native paths.

The supported target AIs are Claude and Codex. Both select from the same source pool, and each selection is materialized into that target's native path. For the exact placement of each artifact, see [the supported targets guide](docs/guide/support.md).

Enozunu is in early design and its goal is intentionally narrow. Mutable Git selections are kept reproducible through `enozunu.lock.json`; for the current scope and non-goals, see [the v0.1.x goal](docs/design/v0.1.x-goal.md), and for the initial phase, [the v0.0.x goal](docs/design/v0.0.x-goal.md).

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

The first run records the resolved commit of every `branch` and `tag` selection in `enozunu.lock.json`; later runs materialize those recorded commits. Commit the lock file to make runs reproducible across machines. Use `enozunu summon --update` to follow moved refs and refresh the lock, and `enozunu summon --frozen` in CI to fail instead of resolving anything the lock does not cover. See [the generated output guide](docs/guide/generated-output.md#the-lock-file) for details.

All commands operate on `enozunu.kdl` in the project root by default. Use `--manifest` and `--project-root` to override the defaults.

A minimal manifest declares sources under `provider` and target selections under `consumer`:

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
  }

  consumer {
    claude {
      use-skills "git-kura"
    }
  }
}
```

For the full manifest contract, including `git`, `local`, and `gist` source references, see [the manifest format guide](docs/guide/manifest.md).

## Documentation

The [documentation](docs/README.md) is split by intent.

To use Enozunu, read [the guide](docs/guide/README.md):

- [Why or why not Enozunu?](docs/guide/why-or-why-not.md)
- [Supported targets](docs/guide/support.md)
- [Manifest format](docs/guide/manifest.md)
- [Generated output](docs/guide/generated-output.md)

To understand how Enozunu works, read [the design docs](docs/design/README.md):

- [Philosophy](docs/design/philosophy.md)
- [v0.0.x goal](docs/design/v0.0.x-goal.md)
- [v0.1.x goal](docs/design/v0.1.x-goal.md)
- [Architecture Decision Records](docs/design/adr/README.md)
