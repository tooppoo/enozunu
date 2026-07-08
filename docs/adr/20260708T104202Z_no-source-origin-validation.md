# Do Not Validate Source Origin

- Status: Accepted
- Created: 2026-07-08T10:42:02Z

## Context

Enozunu fetches Skill and agent definitions from sources and places them into target AI-native paths.

A source is not necessarily located under `.claude/`.
Users may also want to reuse a Skill or agent created for Claude with other target AIs.

If Enozunu started validating from a source URL or source path whether something "is for Claude", it would narrow reusability.
Such validation would also require interpreting target AI specifications, which expands Enozunu's responsibility too far.

## Decision

Enozunu must not validate whether a source was originally created for Claude.

A source URL or source path is not required to be under `.claude/`.

A Skill source only needs the artifact shape required for placement into the Claude target: a directory containing `SKILL.md`.

An agent source is treated as a file to place into the Claude target.
v0.0.x does not validate its meaning as a Claude agent.

Even if a Skill or agent distributed for Claude can later be placed into another target AI's path, whether it works as expected is the user's responsibility and outside Enozunu's guarantee.

## Consequences

- Enozunu looks at artifact shape, not source origin.
- Enozunu does not guarantee semantic compatibility across target AIs.
- Sources are usable even when their path is not under `.claude/`.
- The possibility of placing an unsuitable artifact remains; diagnostics and documentation address it.

## Related

- Issue: [#8](https://github.com/tooppoo/enozunu/issues/8)
