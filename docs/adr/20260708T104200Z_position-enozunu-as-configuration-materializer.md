# Position Enozunu v0.0.x as a Target AI-Native Configuration Materializer

- Status: Accepted
- Created: 2026-07-08T10:42:00Z

## Context

Target AIs such as Claude and Codex each have official formats and locations for plugins, skills, and agents.

If Enozunu reimplemented their plugin managers, it would compete with the official mechanisms of each target AI.
Those specifications also change over time, so a design where Enozunu takes over runtime semantics would be expensive to maintain.

For v0.0.x, centralizing the definitions of AI-agent configuration and placing them into target AI-native paths matters more than guaranteeing reproducibility.

## Decision

Enozunu v0.0.x must not reimplement a target AI-native plugin manager.

Enozunu v0.0.x is a cross-provider configuration materializer.
It centralizes the definitions of AI-agent configuration and materializes them into target AI-native paths.

The only target AI in v0.0.x is Claude.

In an Enozunu-managed project, `.claude/` is treated as generated target AI-native output.

The source of truth is `enozunu.consumer.kdl`, not the target AI-native directory.

## Consequences

- Enozunu does not replace Claude's plugin, skill, or agent specifications.
- v0.0.x assumes regeneration from `enozunu.consumer.kdl` rather than direct edits to `.claude/`.
- A project that wants to manage `.claude/` by hand should not treat that directory as Enozunu-generated output.
- Adding another target AI such as Codex later starts as materialization into that target's native paths.

## Related

- Issue: [#8](https://github.com/tooppoo/enozunu/issues/8)
