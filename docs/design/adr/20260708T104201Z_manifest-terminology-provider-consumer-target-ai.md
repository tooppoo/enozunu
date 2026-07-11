# Fix Manifest Terminology as Provider / Consumer / Target AI

- Status: Accepted
- Created: 2026-07-08T10:42:01Z

## Context

Enozunu handles both the source definition side and the target materialization side in the same manifest.

Calling AI agent tooling such as Claude or Codex a "provider", as is common elsewhere, would collide with the `provider` block inside the manifest.

Keeping room for manifest extensions requires separate namespaces for the source definition side and the target materialization side.

## Decision

`enozunu.consumer.kdl` uses the following terminology.

- `provider`: the source definition side inside the Enozunu manifest
- `consumer`: the target materialization side inside the Enozunu manifest
- `target AI`: AI agent tooling, such as Claude or Codex, that reads the generated configuration
- `target AI-native`: the native format, path, or configuration that a target AI reads

`provider` does not mean a target AI such as Claude or Codex.

The basic manifest structure is:

```kdl
enozunu config-version=1 {
  provider {
    skills {
      skill "example" {
        git "https://github.com/example/repo"
        branch "main"
        path "path/to/skill"
      }
    }
  }

  consumer {
    claude {
      use-skills "example"
    }
  }
}
```

## Consequences

- The source catalog and the target materialization policy are clearly separated.
- Future additions such as `provider.plugins`, `provider.mcp`, or `consumer.codex` are unlikely to collide.
- Documentation must call Claude and Codex target AIs, not providers.

## Related

- Issue: [#8](https://github.com/tooppoo/enozunu/issues/8)
