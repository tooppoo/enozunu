# Manifest Format

`enozunu.kdl` is the human-authored configuration file for a project.

It declares source definitions under `provider`
and target materialization choices under `consumer`.

## Terminology

`provider` means the source definition side inside the Enozunu manifest. It does not mean Claude, Codex, or any other target AI.

`consumer` means the target materialization side inside the Enozunu manifest.

`target AI` means an AI agent tooling system that reads generated configuration. The supported target AIs are Claude and Codex.

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

An agent source must resolve to a file. It is materialized into the native agent path of each target AI that selects it: `.claude/agents/<name>.md` for Claude and `.codex/agents/<name>.toml` for Codex.

Enozunu does not convert between agent formats. A Claude agent is a Markdown file and a Codex custom agent is a TOML file, so the provider declares a target-native source for each. See the [Consumer Block](#consumer-block) section for how each target selects its agent, and [the Claude and Codex materialization ADR](../design/adr/20260711T184657Z_materialize-claude-and-codex-without-semantic-conversion.md) for the responsibility boundary.

## Source Reference Blocks

Each `skill` or `agent` declaration must contain exactly one source reference block.

Supported source reference blocks:

```text
git
local
gist
```

The following are invalid:

- no source reference block
- more than one source reference block in one declaration, in any combination
- unsupported source reference blocks

The rationale for the block structure is recorded in [the source reference blocks ADR](../design/adr/20260709T070553Z_source-reference-blocks-and-local-sources.md). The rationale for `gist` as a distinct source kind is recorded in [the Gist source reference ADR](../design/adr/20260710T220338Z_gist-first-class-source-reference.md).

### Git Source Reference

A `git` block selects its commit with exactly one selector: a mutable `branch`, a mutable `tag`, or an exact `revision`.

Branch selector:

```kdl
git {
  url "https://github.com/example/repo"
  branch "main"
  path ".claude/skills/example"
}
```

Tag selector:

```kdl
git {
  url "https://github.com/example/repo"
  tag "v1.2.0"
  path ".claude/skills/example"
}
```

Exact revision selector:

```kdl
git {
  url "https://github.com/example/repo"
  revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
  path ".claude/skills/example"
}
```

Required fields:

```text
url + exactly one of (branch, tag, revision) + path
```

Semantics:

- `url` is the Git repository URL.
- `branch` resolves the current head of the branch on each run.
- `tag` resolves the commit the tag currently points at, on each run.
- `revision` pins one exact Git commit.
- `path` is the artifact path inside the resolved Git checkout.
- `path` must be relative and must not contain empty or `..` segments.

Declaring more than one selector, or none, is rejected. Declaring any `git` field more than once is also rejected: repeated fields are a manifest error, not last-value-wins.

`tag` is a mutable selector, not a pinning one. A Git tag can be moved or deleted on the remote, so Enozunu re-resolves it on every run and offers no more reproducibility than `branch`. Use `revision` for a source that must materialize the same commit every time. The resolved commit is recorded in `.enozunu/provenance.json`, which is where a moved tag becomes visible after the fact.

A tag resolves through the fully-qualified `refs/tags/` namespace, so a repository holding both a branch and a tag of one name resolves the tag for a `tag` selector and the branch for a `branch` selector. An annotated tag is peeled to its commit, so a recorded revision is always a commit id and never a tag object id.

`tag` must not be empty, must not begin with `-`, and must not contain `:`. The value reaches Git inside a `refs/tags/<tag>` refspec, where a leading `-` would be parsed as an option and `:` would separate source from destination.

`revision` must be a canonical full SHA-1 commit id, exactly 40 lowercase ASCII hexadecimal characters:

```regex
^[0-9a-f]{40}$
```

`revision` identifies one exact commit; it is not an arbitrary Git revspec. Abbreviated, uppercase, and whitespace-padded object ids are rejected, as are tag names, `HEAD`, relative expressions such as `main~3`, and SHA-256 object ids. Select a tag with the `tag` field rather than by naming it in `revision`. After checkout, the resolver verifies that the resolved `HEAD` exactly equals the requested revision.

The SHA-1 restriction reflects the v0.0.x source-host scope: GitHub uses SHA-1 object ids for the supported workflow, and SHA-256 repositories are not supported. The object-id contract will be reconsidered when support expands to other Git hosting systems or repository formats, including GitLab. The selector and object-id decisions are recorded in [the Git exact revision selector ADR](../design/adr/20260712T155345Z_git-exact-revision-selector.md).

Each distinct `(url, selector kind, selector value)` combination is resolved once per run. The selector kind is part of that identity, so a branch whose name looks like a commit id never collides with an exact revision of the same text, and a branch never collides with a tag of the same name.

The tag selector contract is recorded in [the Git tag selector ADR](../design/adr/20260719T062207Z_git-tag-selector.md).

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

A Gist source distributes a Skill or an agent as a GitHub Gist, without requiring a full repository.

The `gist` block contract differs by artifact kind.

A Skill Gist declares `id` and `revision`; the root of the pinned Gist revision is the Skill artifact:

```kdl
skill "semantic-line-breaks" {
  gist {
    id "2decf6c462d9b4418f2"
    revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
  }
}
```

An agent Gist additionally requires `file`, which selects a single agent file:

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
skill: id + revision
agent: id + revision + file
```

Semantics:

- `id` is the unique Gist identifier from the final path segment of a Gist URL, such as `2decf6c462d9b4418f2` in `https://gist.github.com/monalisa/2decf6c462d9b4418f2`.
- The manifest does not require the Gist owner: the resolver builds the Git remote `https://gist.github.com/<id>.git` from the id alone.
- `revision` pins the exact Gist commit to materialize.
- For a Skill, the root of the pinned Gist revision is the Skill artifact root. It must contain a regular-file `SKILL.md`, and the whole tree is materialized to the selecting target AI's native Skill path.
- For an agent, `file` is the agent artifact path relative to the root of the checked-out Gist revision.
- A `file` field inside a Skill Gist is rejected, and no `path` field exists to select a nested Skill root.

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
- it must resolve to a regular file inside the resolved Gist content

An agent Gist source is materialized to the same target-native agent path as an agent Git source, determined by the selecting target AI.

Skill Gist trees follow the same validation policy as other Skill sources, including the symlink rejection policy.

Gist resolution uses Git transport internally, but a Gist remains a distinct source kind: it is never recorded or reported as an ordinary Git source. Skill and agent sources referencing the same `(id, revision)` in one run are resolved once and share the resolved content. The Skill artifact root decision is recorded in [the pinned Gist root ADR](../design/adr/20260711T144232Z_use-pinned-gist-root-as-skill-artifact-root.md).

## Consumer Block

The `consumer` block declares what to materialize for each target AI.

The supported target blocks are `consumer.claude` and `consumer.codex`. A manifest may declare either, both, or neither.

```kdl
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
```

### Shared Provider Pool

Claude and Codex select from the same `provider.skills` and `provider.agents` declarations. A source declaration is not bound to a target AI.

The same Skill source can be selected from both targets. Enozunu resolves it once per run and materializes it into each selecting target's native path.

Agent sources are target-native. A Claude agent is a Markdown file and a Codex custom agent is a TOML file, so the provider declares a separate source for each and each target selects the one written for it. Enozunu materializes the file verbatim; it does not convert a Claude agent into a Codex agent or the reverse.

### Skills

`use-skills` selects Skill sources by name. Each referenced name must exist under `provider.skills`.

```kdl
consumer {
  claude {
    use-skills "git-kura" "semantic-line-breaks"
  }
  codex {
    use-skills "git-kura"
  }
}
```

Each selected Skill is materialized to the selecting target AI's native Skill path:

```text
claude -> .claude/skills/<name>/
codex  -> .agents/skills/<name>/
```

### Agents

`use-agents` selects agent sources by name. Each referenced name must exist under `provider.agents`.

```kdl
consumer {
  claude {
    use-agents "shell-script-reviewer-claude"
  }
  codex {
    use-agents "shell-script-reviewer-codex"
  }
}
```

Each selected agent source is materialized to the selecting target AI's native agent path:

```text
claude -> .claude/agents/<name>.md
codex  -> .codex/agents/<name>.toml
```

The target filename suffix is fixed by the target AI. The source path itself is not required to carry a matching extension.

Enozunu does not guarantee that a source selected for a target AI is interpreted as intended by that target AI. It projects the source into the target's native path without validating the target-native format. The rationale is recorded in [the Claude and Codex materialization ADR](../design/adr/20260711T184657Z_materialize-claude-and-codex-without-semantic-conversion.md).

Codex `AGENTS.md` is repository instructions rather than a custom agent definition, so it is not part of agent materialization.

## Unsupported

The following are not supported in v0.0.x:

```text
consumer targets other than claude and codex
GitHub tree/blob URL shorthand
Git latest selector
Git version range
Git symbolic or relative revspecs
SHA-256 repositories
absolute local paths
Gist branch, tag, or abbreviated revision selectors
nested Skill root selection inside a Gist
```

Codex support is limited to Skill and custom agent materialization. `AGENTS.md`, `.codex/config.toml`, and Codex rules, MCP, hooks, and plugins are out of scope.

For v0.0.x, Git source references must use the normalized form:

```text
git { url + exactly one of (branch, tag, revision) + path }
```

## Validation Rules

v0.0.x should reject:

- duplicate source names within the same kind
- `use-skills` references that do not exist under `provider.skills`
- `use-agents` references that do not exist under `provider.agents`
- source declarations without exactly one source reference block
- unsupported source reference blocks
- `git` blocks without exactly one of `branch`, `tag`, and `revision`
- duplicate `git` fields
- unknown `git` fields
- `git` `revision` values that are not canonical full SHA-1 commit ids
- `git` `tag` values that are empty, begin with `-`, or contain `:`
- Skill sources that do not contain `SKILL.md`
- source paths that cannot be resolved
- GitHub tree/blob URL shorthand
- `consumer` targets other than `claude` and `codex`
- path traversal or symlink writes outside the target root
- absolute `local` paths
- symlinked `local` source paths
- `local` source paths overlapping their materialization target paths
- multiple sources materializing to the same target path
- missing, duplicate, or unknown `gist` fields
- `file` fields inside Skill `gist` blocks
- Skill Gist revisions whose root does not contain a regular-file `SKILL.md`
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

      skill "released-git-kura" {
        git {
          url "https://github.com/tooppoo/reportage"
          tag "v1.2.0"
          path ".claude/skills/git-kura"
        }
      }

      skill "pinned-git-kura" {
        git {
          url "https://github.com/tooppoo/reportage"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
          path ".claude/skills/git-kura"
        }
      }

      skill "local-git-kura" {
        local {
          path "../reportage/.claude/skills/git-kura"
        }
      }

      skill "semantic-line-breaks" {
        gist {
          id "2decf6c462d9b4418f2"
          revision "468aac8caed5f0c3b859b8286968e2c78e2b8760"
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

      agent "shell-script-reviewer-codex" {
        git {
          url "https://github.com/tooppoo/installerer"
          branch "main"
          path ".codex/agents/shell-script-reviewer.toml"
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
      use-skills "git-kura" "released-git-kura" "pinned-git-kura" "local-git-kura" "semantic-line-breaks"
      use-agents "shell-script-reviewer" "gist-reviewer"
    }

    codex {
      use-skills "git-kura" "semantic-line-breaks"
      use-agents "shell-script-reviewer-codex"
    }
  }
}
```
