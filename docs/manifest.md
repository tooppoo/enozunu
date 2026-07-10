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
gist
```

The `gist` block is accepted only under `provider.agents`. A `gist` block under `provider.skills` is rejected.

The following are invalid:

- no source reference block
- more than one source reference block in one declaration, in any combination
- unsupported source reference blocks
- a `gist` block under a Skill declaration

The rationale for the block structure is recorded in [the source reference blocks ADR](adr/20260709T070553Z_source-reference-blocks-and-local-sources.md). The rationale for `gist` as a distinct source kind is recorded in [the Gist source reference ADR](adr/20260710T220338Z_gist-first-class-source-reference.md).

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
- a local source path that equals, contains, or is contained by any target path materialized in the same run is rejected

Target paths are canonicalized through any existing symlinked ancestors before the overlap comparison, so a symlinked `.claude/skills` cannot hide an overlap.

### Gist Source Reference

A Gist source distributes a single agent file as a GitHub Gist, without requiring a full repository.

```kdl
agent "shell-script-reviewer" {
  gist {
    id "2decf6c462d9b4418f2"
    revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
    file "shell-script-reviewer.md"
  }
}
```

Required fields:

```text
id
revision
file
```

Semantics:

- `id` is the unique Gist identifier from the final path segment of a Gist URL, such as `2decf6c462d9b4418f2` in `https://gist.github.com/monalisa/2decf6c462d9b4418f2`.
- The manifest does not require the Gist owner: the resolver builds the Git remote `https://gist.github.com/<id>.git` from the id alone.
- `revision` pins the exact Gist commit to materialize.
- `file` is the agent artifact path relative to the root of the checked-out Gist revision.

`id` must be a non-empty lowercase ASCII hexadecimal string:

```regex
^[0-9a-f]+$
```

No fixed length is imposed. Percent-encoded and otherwise non-canonical id representations are rejected, because a validated id is interpolated directly into the Gist Git remote URL.

`revision` must be exactly 40 lowercase ASCII hexadecimal characters:

```regex
^[0-9a-f]{40}$
```

A latest, branch, tag, abbreviated, uppercase, whitespace-padded, or SHA-256 revision is rejected. After checkout, the resolver verifies that the checked-out `HEAD` equals the requested revision.

`file` uses the same safe relative path policy as a Git source path:

- it must be relative
- it must not contain empty or `..` segments
- it must resolve to a regular file inside the Gist checkout

A Gist source is materialized to `.claude/agents/<name>.md`, the same target as an agent Git source.

Gist resolution uses Git transport internally, but a Gist remains a distinct source kind: it is never recorded or reported as an ordinary Git source. Gist support is limited to agents in this version; Skill support from Gists is deferred.

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
Git revision selector
Git tag selector
Git latest selector
Git version range
absolute local paths
Gist Skill sources
Gist branch, tag, or abbreviated revision selectors
```

An exact revision is supported only for Gist sources, through the dedicated `gist` block, not for Git source references.

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
- `gist` blocks under `provider.skills`
- missing, duplicate, or unknown `gist` fields
- non-canonical Gist ids
- revisions that are not exactly 40 lowercase hexadecimal characters
- unsafe `gist` `file` paths

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

      agent "gist-reviewer" {
        gist {
          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          file "shell-script-reviewer.md"
        }
      }
    }
  }

  consumer {
    claude {
      use-skills "git-kura" "local-git-kura"
      use-agents "shell-script-reviewer" "gist-reviewer"
    }
  }
}
```
