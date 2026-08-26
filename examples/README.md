# Enozunu Examples

## Contents

- [Getting started](#group-1-getting-started)
  - [Initialize a project](#file-1-1-initialize-a-project)
    - [init scaffolds a starter manifest](#case-1-1-1-init-scaffolds-a-starter-manifest)
    - [Re-running init is safe](#case-1-1-2-re-running-init-is-safe)
  - [Validate the manifest](#file-1-2-validate-the-manifest)
    - [validate accepts a freshly initialized manifest](#case-1-2-1-validate-accepts-a-freshly-initialized-manifest)
    - [Validation failures exit non-zero](#case-1-2-2-validation-failures-exit-non-zero)
- [Instructions](#group-2-instructions)
  - [Generate root instruction files](#file-2-1-generate-root-instruction-files)
    - [summon generates CLAUDE.md from a base document and the skill selections](#case-2-1-1-summon-generates-claude-md-from-a-base-document-and-the-skill-selections)
  - [Reuse one target's selections with use-same-*](#file-2-2-reuse-one-target-s-selections-with-use-same)
    - [codex follows claude's selections and instruction source](#case-2-2-1-codex-follows-claude-s-selections-and-instruction-source)
- [Materialize](#group-3-materialize)
  - [Materialize a local skill](#file-3-1-materialize-a-local-skill)
    - [summon materializes a local skill into the Claude-native path](#case-3-1-1-summon-materializes-a-local-skill-into-the-claude-native-path)
  - [Share one source pool across targets](#file-3-2-share-one-source-pool-across-targets)
    - [summon materializes one skill pool into both target-native paths](#case-3-2-1-summon-materializes-one-skill-pool-into-both-target-native-paths)
- [Reproducibility](#group-4-reproducibility)
  - [Resolve a git source and lock it](#file-4-1-resolve-a-git-source-and-lock-it)
    - [the first summon resolves the branch and creates the lock file](#case-4-1-1-the-first-summon-resolves-the-branch-and-creates-the-lock-file)
    - [The lock pins the commit until --update](#case-4-1-2-the-lock-pins-the-commit-until-update)
  - [Verify reproducibility in CI with --frozen](#file-4-2-verify-reproducibility-in-ci-with-frozen)
    - [summon --frozen fails when the lock file is missing](#case-4-2-1-summon-frozen-fails-when-the-lock-file-is-missing)
    - [The CI workflow](#case-4-2-2-the-ci-workflow)

<a id="group-1-getting-started"></a>
## Getting started

<a id="file-1-1-initialize-a-project"></a>
### Initialize a project

Source: examples/init.repor

`enozunu init` scaffolds a starter `enozunu.kdl` filled with placeholder values, plus `.enozunu/.gitignore` so the resolver cache under `.enozunu/cache/` stays out of version control.
Run it once per project and then edit the manifest to declare your real sources.

<a id="case-1-1-1-init-scaffolds-a-starter-manifest"></a>
#### init scaffolds a starter manifest

```reportage
case "init scaffolds a starter manifest" {
  $ enozunu init

  assert {
    exit 0
    stdout contains "created"
    file <"enozunu.kdl"> exists
    file <".enozunu/.gitignore"> exists
  }
}
```

<a id="case-1-1-2-re-running-init-is-safe"></a>
#### Re-running init is safe

A second `init` fails instead of clobbering the manifest you may have already edited.

```reportage
case "init refuses to overwrite an existing manifest" {
  $ enozunu init
  $ enozunu init

  assert {
    exit 1
    stderr contains "refusing to overwrite"
  }
}
```

<a id="file-1-2-validate-the-manifest"></a>
### Validate the manifest

Source: examples/validate.repor

`enozunu validate` checks that `enozunu.kdl` is a well-formed manifest without resolving sources or writing anything.
Use it as a fast pre-flight check before `enozunu summon`, for example in a pre-commit hook or a CI lint step.

<a id="case-1-2-1-validate-accepts-a-freshly-initialized-manifest"></a>
#### validate accepts a freshly initialized manifest

```reportage
case "validate accepts a freshly initialized manifest" {
  $ enozunu init
  $ enozunu validate

  assert {
    exit 0
    stdout contains "is valid"
  }
}
```

<a id="case-1-2-2-validation-failures-exit-non-zero"></a>
#### Validation failures exit non-zero

A missing manifest is reported on stderr with exit status 1, so a scripted check fails loudly instead of passing vacuously.

```reportage
case "validate reports a missing manifest" {
  $ enozunu validate

  assert {
    exit 1
    stderr contains "failed to read ./enozunu.kdl"
  }
}
```

<a id="group-2-instructions"></a>
## Instructions

<a id="file-2-1-generate-root-instruction-files"></a>
### Generate root instruction files

Source: examples/instructions.repor

`provider.instructions.<target>` declares a base document, and `enozunu summon` composes it with the target's `use-skills` selections into that target's root instruction file — `CLAUDE.md` for Claude, `AGENTS.md` for Codex.
Each selection becomes one fixed Skill usage rule, and an optional `when` annotation scopes the rule to a situation.
The generated file is owned by enozunu: it starts with a marker comment telling readers not to edit it directly.

<a id="case-2-1-1-summon-generates-claude-md-from-a-base-document-and-the-skill-selections"></a>
#### summon generates CLAUDE.md from a base document and the skill selections

````reportage
case "summon generates CLAUDE.md from a base document and the skill selections" {
  write <"skills/review/SKILL.md"> ```
    # review
    ```
  write <"skills/deploy/SKILL.md"> ```
    # deploy
    ```
  write <"base.md"> ```
    # Project instructions

    Follow the coding standards in this repository.
    ```
  write <"enozunu.kdl"> ```
    enozunu config-version=1 {
      provider {
        skills {
          skill "review" { local { path "skills/review" } }
          skill "deploy" { local { path "skills/deploy" } }
        }
        instructions {
          claude { local { path "base.md" } }
        }
      }
      consumer {
        claude {
          use-skills "review" {
            when "before completing a task"
          }
          use-skills "deploy"
        }
      }
    }
    ```

  $ enozunu summon

  assert {
    exit 0
    stdout contains "materialized instruction `claude` for claude -> CLAUDE.md"
    file <"CLAUDE.md"> text_equals ```
      <!-- Generated by enozunu. Do not edit directly. -->

      # Project instructions

      Follow the coding standards in this repository.

      ## Skill usage

      - When before completing a task, always use the `review` skill.
      - Always use the `deploy` skill.
      ```
  }
}
````

<a id="file-2-2-reuse-one-target-s-selections-with-use-same"></a>
### Reuse one target's selections with use-same-*

Source: examples/use-same.repor

`use-same-skills "claude"` expands Claude's effective `use-skills` selections at the declaration position, and `use-same-instruction "claude"` reuses Claude's instruction source.
Declare the shared selections once and let the second target follow them, adding its own selections before or after the expansion.
The reused values behave exactly like direct declarations, so both generated instruction files list the shared rules in the same order.

<a id="case-2-2-1-codex-follows-claude-s-selections-and-instruction-source"></a>
#### codex follows claude's selections and instruction source

````reportage
case "codex follows claude's selections and instruction source" {
  write <"skills/review/SKILL.md"> ```
    # review
    ```
  write <"skills/codex-only/SKILL.md"> ```
    # codex-only
    ```
  write <"base.md"> ```
    # Shared base document
    ```
  write <"enozunu.kdl"> ```
    enozunu config-version=1 {
      provider {
        skills {
          skill "review" { local { path "skills/review" } }
          skill "codex-only" { local { path "skills/codex-only" } }
        }
        instructions {
          claude { local { path "base.md" } }
          codex { use-same-instruction "claude" }
        }
      }
      consumer {
        claude {
          use-skills "review" {
            when "before completing a task"
          }
        }
        codex {
          use-same-skills "claude"
          use-skills "codex-only"
        }
      }
    }
    ```

  $ enozunu summon

  assert {
    exit 0
    file <"AGENTS.md"> text_equals ```
      <!-- Generated by enozunu. Do not edit directly. -->

      # Shared base document

      ## Skill usage

      - When before completing a task, always use the `review` skill.
      - Always use the `codex-only` skill.
      ```
    file <".agents/skills/review/SKILL.md"> exists
    file <".agents/skills/codex-only/SKILL.md"> exists
  }
}
````

<a id="group-3-materialize"></a>
## Materialize

<a id="file-3-1-materialize-a-local-skill"></a>
### Materialize a local skill

Source: examples/summon-local-skill.repor

The smallest useful manifest: one `local` skill source under `provider` and one `use-skills` selection under `consumer.claude`.
`enozunu summon` copies the source into the Claude-native path `.claude/skills/<name>/` and records what it wrote in `.enozunu/provenance.json`.
A `local` source lives inside the project, so nothing is fetched over the network.

<a id="case-3-1-1-summon-materializes-a-local-skill-into-the-claude-native-path"></a>
#### summon materializes a local skill into the Claude-native path

````reportage
case "summon materializes a local skill into the Claude-native path" {
  write <"sources/demo-skill/SKILL.md"> ```
    ---
    name: demo-skill
    description: demo skill materialized from a local source
    ---

    # Demo Skill
    ```
  write <"enozunu.kdl"> ```
    enozunu config-version=1 {
      provider {
        skills {
          skill "demo-skill" {
            local {
              path "sources/demo-skill"
            }
          }
        }
      }
      consumer {
        claude {
          use-skills "demo-skill"
        }
      }
    }
    ```

  $ enozunu summon

  assert {
    exit 0
    stdout contains "materialized"
    file <".claude/skills/demo-skill/SKILL.md"> contains "demo skill materialized from a local source"
    file <".enozunu/provenance.json"> contains "demo-skill"
  }
}
````

<a id="file-3-2-share-one-source-pool-across-targets"></a>
### Share one source pool across targets

Source: examples/summon-targets.repor

Claude and Codex select from the same `provider.skills` and `provider.agents` pool, and each selection lands in that target's native path.
Skills are target-neutral, so both targets may select the same skill.
Agent sources are target-native — a Claude agent is a Markdown file and a Codex custom agent is a TOML file — and enozunu never converts between the two.

<a id="case-3-2-1-summon-materializes-one-skill-pool-into-both-target-native-paths"></a>
#### summon materializes one skill pool into both target-native paths

````reportage
case "summon materializes one skill pool into both target-native paths" {
  write <"sources/demo-skill/SKILL.md"> ```
    ---
    name: demo-skill
    description: demo skill shared by both targets
    ---

    # Demo Skill
    ```
  write <"sources/claude-agent.md"> ```
    # Demo agent for Claude
    ```
  write <"sources/codex-agent.toml"> ```
    name = "demo agent for Codex"
    ```
  write <"enozunu.kdl"> ```
    enozunu config-version=1 {
      provider {
        skills {
          skill "demo-skill" {
            local {
              path "sources/demo-skill"
            }
          }
        }
        agents {
          agent "claude-agent" {
            local {
              path "sources/claude-agent.md"
            }
          }
          agent "codex-agent" {
            local {
              path "sources/codex-agent.toml"
            }
          }
        }
      }
      consumer {
        claude {
          use-skills "demo-skill"
          use-agents "claude-agent"
        }
        codex {
          use-skills "demo-skill"
          use-agents "codex-agent"
        }
      }
    }
    ```

  $ enozunu summon

  assert {
    exit 0
    file <".claude/skills/demo-skill/SKILL.md"> exists
    file <".agents/skills/demo-skill/SKILL.md"> exists
    file <".claude/agents/claude-agent.md"> contains "Demo agent for Claude"
    file <".codex/agents/codex-agent.toml"> contains "demo agent for Codex"
  }
}
````

<a id="group-4-reproducibility"></a>
## Reproducibility

<a id="file-4-1-resolve-a-git-source-and-lock-it"></a>
### Resolve a git source and lock it

Source: examples/git-source-lockfile.repor

A `git` source with a `branch` selector is mutable: the branch may move between runs.
The first `enozunu summon` records the resolved commit in `enozunu.lock.json`, and later runs materialize that recorded commit even after the branch advances.
Commit the lock file to make every machine materialize the same content; run `enozunu summon --update` when you want to follow the moved branch and refresh the lock.
Each case builds a git repository inside its own workspace and points the manifest at it with a `file://` URL, so the real resolver runs while the example stays offline.
The `SOURCE_URL` placeholder is substituted by a shell step because the workspace path is only known at run time, and the git steps run with `GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1` so ambient developer configuration cannot break the fixture.

<a id="case-4-1-1-the-first-summon-resolves-the-branch-and-creates-the-lock-file"></a>
#### the first summon resolves the branch and creates the lock file

```reportage
case "the first summon resolves the branch and creates the lock file" {
  $ GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git init --quiet --initial-branch main source-repo
  $ git -C source-repo config user.email "examples@example.com"
  $ git -C source-repo config user.name "examples"
  $ GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -C source-repo add --all
  $ GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -C source-repo commit --quiet -m initial
  $ sed -i "s|SOURCE_URL|file://$PWD/source-repo|" enozunu.kdl

  $ enozunu summon

  assert {
    exit 0
    stdout contains "created enozunu.lock.json"
    file <".claude/skills/demo-skill/SKILL.md"> contains "Demo Skill OLD"
    file <"enozunu.lock.json"> contains "\"type\": \"branch\""
    file <"enozunu.lock.json"> contains "\"value\": \"main\""
  }
}
```

<a id="case-4-1-2-the-lock-pins-the-commit-until-update"></a>
#### The lock pins the commit until --update

After the branch advances, a plain `summon` still materializes the locked commit, while `summon --update` follows the branch head and rewrites the lock.

```reportage
case "summon stays on the locked commit until --update follows the branch" {
  $ GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git init --quiet --initial-branch main source-repo
  $ git -C source-repo config user.email "examples@example.com"
  $ git -C source-repo config user.name "examples"
  $ GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -C source-repo add --all
  $ GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -C source-repo commit --quiet -m old
  $ sed -i "s|SOURCE_URL|file://$PWD/source-repo|" enozunu.kdl

  # First run locks the current branch head.
  $ enozunu summon
  assert {
    exit 0
    file <".claude/skills/demo-skill/SKILL.md"> contains "Demo Skill OLD"
  }

  # Advance the branch so its head no longer matches the locked commit.
  $ sed -i 's/Demo Skill OLD/Demo Skill NEW/' source-repo/skills/demo-skill/SKILL.md
  $ GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -C source-repo commit --quiet -am new

  $ enozunu summon
  assert {
    exit 0
    file <".claude/skills/demo-skill/SKILL.md"> contains "Demo Skill OLD"
    not {
      file <".claude/skills/demo-skill/SKILL.md"> contains "Demo Skill NEW"
    }
  }

  $ enozunu summon --update
  assert {
    exit 0
    stdout contains "updated enozunu.lock.json"
    file <".claude/skills/demo-skill/SKILL.md"> contains "Demo Skill NEW"
  }
}
```

<a id="file-4-2-verify-reproducibility-in-ci-with-frozen"></a>
### Verify reproducibility in CI with --frozen

Source: examples/summon-frozen.repor

`enozunu summon --frozen` materializes strictly from `enozunu.lock.json` and fails when the lock file is missing or lacks an entry for a mutable source.
Run it in CI so a manifest change that was committed without its refreshed lock fails the build instead of silently resolving something new.
A missing lock always means it was never created or committed, so frozen mode fails even for a manifest with no mutable sources.

<a id="case-4-2-1-summon-frozen-fails-when-the-lock-file-is-missing"></a>
#### summon --frozen fails when the lock file is missing

```reportage
case "summon --frozen fails when the lock file is missing" {
  $ enozunu summon --frozen

  assert {
    exit 1
    stderr contains "lock-out-of-date"
    not {
      dir <".claude"> exists
    }
    not {
      file <"enozunu.lock.json"> exists
    }
  }
}
```

<a id="case-4-2-2-the-ci-workflow"></a>
#### The CI workflow

A normal `summon` writes the lock once; committing it makes the later frozen run pass.

```reportage
case "summon --frozen passes once a normal summon has written the lock" {
  $ enozunu summon
  assert {
    exit 0
    file <"enozunu.lock.json"> exists
  }

  $ enozunu summon --frozen
  assert {
    exit 0
    file <".claude/skills/demo-skill/SKILL.md"> exists
  }
}
```
