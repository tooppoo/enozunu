# Enozunu Documentation

This directory is split by what you are trying to do, so the two audiences do not have to read each other's material.

## I want to use Enozunu

Read [the guide](guide/README.md). It covers deciding whether Enozunu fits, writing `enozunu.kdl`, and living with generated output.

- [Why or why not Enozunu?](guide/why-or-why-not.md) — whether Enozunu fits your workflow
- [Supported targets](guide/support.md) — supported AI agents and where each artifact is written
- [Manifest format](guide/manifest.md) — how to write `enozunu.kdl`
- [Generated output](guide/generated-output.md) — what Enozunu writes and how to manage it in Git

## I want to understand how Enozunu works

Read [the design docs](design/README.md). They explain the reasoning and boundaries behind the tool rather than how to operate it.

- [Philosophy](design/philosophy.md) — what Enozunu is and is not
- [v0.0.x goal](design/v0.0.x-goal.md) — the initial scope and non-goals
- [v0.1.x goal](design/v0.1.x-goal.md) — whole-run reproducibility through the lock file
- [Architecture Decision Records](design/adr/README.md) — recorded design decisions and their trade-offs
