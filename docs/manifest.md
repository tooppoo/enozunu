# Manifest Format

`enozunu.kdl` is the human-authored configuration file for a project.

It declares source definitions under `provider`
and target materialization choices under `consumer`.

## Terminology

`provider` means the source definition side inside the Enozunu manifest. It does not mean Claude, Codex, or any other target AI.

`consumer` means the target materialization side inside the Enozunu manifest.

`target AI` means an AI agent tooling system that reads generated configuration. For v0.0.x, the only target AI is Claude.

`target AI-native` means the target AI's native format, path, or configuration layout.

## Root

The root node must be:

```kdl
enozunu config-version=1 {
  // ...
}
```

## Provider Block

The `provider` block defines available sources.

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
      agent "shell-script-reviewer" {
        git {
          url "https://github.com/tooppoo/installerer"
          branch "main"
          path ".claude/agents/shell-script-reviewer.md"
        }
      }
    }
  }
}
```

### Skill Source

A Skill source is declared under `provider.skills`.

```kdl
skill "git-kura" {
  git {
    url "https://github.com/tooppoo/reportage"
    branch "main"
    path ".claude/skills/git-kura"
  }
}
```

A Skill source must resolve to a directory containing `SKILL.md`.

### Agent Source

An agent source is declared under `provider.agents`.

```kdl
agent "shell-script-reviewer" {
  git {
    url "https://github.com/tooppoo/installerer"
    branch "main"
    path ".claude/agents/shell-script-reviewer.md"
  }
}
```

An agent source must resolve to a file. For v0.0.x, that file is materialized into `.claude/agents/<name>.md`.

## Source Reference Blocks

Each `skill` or `agent` declaration must contain exactly one source reference block.

Supported source reference blocks:

```text
git
local
```

The following are invalid:

- no source reference block
- both `git` and `local` in one declaration
- multiple `git` blocks
- multiple `local` blocks
- unsupported source reference blocks

The rationale for this structure is recorded in [the source reference blocks ADR](adr/20260709T070553Z_source-reference-blocks-and-local-sources.md).

### Git Source Reference

```kdl
git {
  url "https://github.com/example/repo"
  branch "main"
  path ".claude/skills/example"
}
```

Required fields:

```text
url
branch
path
```

Semantics:

- `url` is the Git repository URL.
- `branch` is the branch to resolve.
- `path` is the artifact path inside the resolved Git checkout.
- `path` must be relative and must not contain empty or `..` segments.

### Local Source Reference

```kdl
local {
  path "../some-repo/.claude/skills/example"
}
```

Required fields:

```text
path
```

Semantics:

- `path` is the local filesystem path to the artifact itself.
- Relative `path` values are resolved from the manifest file's containing directory, not from the process working directory.
- `path` may contain `..` so sibling repositories can be referenced.
- Absolute paths are rejected in v0.0.x.

Local sources use the same artifact-shape contract as Git sources: a Skill source is a directory containing `SKILL.md`, and an agent source is a file.

Local sources also keep the filesystem safety policy:

- a local source path that is a symlink is rejected
- symlinked Skill contents are rejected
- a local source path that equals, contains, or is contained by its materialization target path is rejected

## Consumer Block

The `consumer` block declares what to materialize for each target AI.

For v0.0.x, only `consumer.claude` is supported.

```kdl
consumer {
  claude {
    use-skills "git-kura"
    use-agents "shell-script-reviewer"
  }
}
```

### Claude Skills

`consumer.claude.use-skills` selects Skill sources by name.

```kdl
consumer {
  claude {
    use-skills "git-kura" "semantic-line-breaks"
  }
}
```

Each referenced name must exist under `provider.skills`.

Each selected Skill is materialized to:

```text
.claude/skills/<name>/
```

### Claude Agents

`consumer.claude.use-agents` selects agent sources by name.

```kdl
consumer {
  claude {
    use-agents "shell-script-reviewer"
  }
}
```

Each referenced name must exist under `provider.agents`.

Each selected agent source is materialized to:

```text
.claude/agents/<name>.md
```

## Unsupported in v0.0.x

The following are not supported in v0.0.x:

```text
consumer.codex
GitHub tree/blob URL shorthand
revision selector
tag selector
latest selector
version range
absolute local paths
```

For v0.0.x, Git source references must use the normalized form:

```text
git { url + branch + path }
```

## Validation Rules

v0.0.x should reject:

- duplicate source names within the same kind
- `use-skills` references that do not exist under `provider.skills`
- `use-agents` references that do not exist under `provider.agents`
- source declarations without exactly one source reference block
- unsupported source reference blocks
- Skill sources that do not contain `SKILL.md`
- source paths that cannot be resolved
- GitHub tree/blob URL shorthand
- `consumer.codex`
- path traversal or symlink writes outside the target root
- absolute `local` paths
- symlinked `local` source paths
- `local` source paths overlapping their materialization target paths
- multiple sources materializing to the same target path

v0.0.x should not reject a source merely because its URL or path is not under `.claude/`.

## Complete Example

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

      skill "local-git-kura" {
        local {
          path "../reportage/.claude/skills/git-kura"
        }
      }
    }

    agents {
      agent "shell-script-reviewer" {
        git {
          url "https://github.com/tooppoo/installerer"
          branch "main"
          path ".claude/agents/shell-script-reviewer.md"
        }
      }
    }
  }

  consumer {
    claude {
      use-skills "git-kura" "local-git-kura"
      use-agents "shell-script-reviewer"
    }
  }
}
```
