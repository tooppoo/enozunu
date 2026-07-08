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
}
```

### Skill Source

A Skill source is declared under `provider.skills`.

```kdl
skill "git-kura" {
  git "https://github.com/tooppoo/reportage"
  branch "main"
  path ".claude/skills/git-kura"
}
```

Required fields:

```text
git
branch
path
```

A Skill source must resolve to a directory containing `SKILL.md`.

### Agent Source

An agent source is declared under `provider.agents`.

```kdl
agent "shell-script-reviewer" {
  git "https://github.com/tooppoo/installerer"
  branch "main"
  path ".claude/agents/shell-script-reviewer.md"
}
```

Required fields:

```text
git
branch
path
```

An agent source must resolve to a file. For v0.0.x, that file is materialized into `.claude/agents/<name>.md`.

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
```

For v0.0.x, source references must use the normalized form:

```text
git + branch + path
```

## Validation Rules

v0.0.x should reject:

- duplicate source names within the same kind
- `use-skills` references that do not exist under `provider.skills`
- `use-agents` references that do not exist under `provider.agents`
- Skill sources that do not contain `SKILL.md`
- source paths that cannot be resolved
- GitHub tree/blob URL shorthand
- `consumer.codex`
- path traversal or symlink writes outside the target root
- multiple sources materializing to the same target path

v0.0.x should not reject a source merely because its URL or path is not under `.claude/`.

## Complete Example

```kdl
enozunu config-version=1 {
  provider {
    skills {
      skill "git-kura" {
        git "https://github.com/tooppoo/reportage"
        branch "main"
        path ".claude/skills/git-kura"
      }

      skill "semantic-line-breaks" {
        git "https://github.com/tooppoo/reportage"
        branch "main"
        path ".claude/skills/semantic-line-breaks"
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
      use-skills "git-kura" "semantic-line-breaks"
      use-agents "shell-script-reviewer"
    }
  }
}
```
