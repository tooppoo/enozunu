# Implement Enozunu in Rust

- Status: Accepted
- Created: 2026-07-08T07:57:13Z

## Context

Enozunu is a CLI-oriented tool for managing AI agent-facing configuration across projects and machines. The current v0.0.x direction treats Enozunu as a cross-provider configuration materializer rather than as a replacement for each target AI's native plugin or skill manager.

The primary source of truth for user-authored configuration is `enozunu.consumer.kdl`. Machine-generated materialization metadata is recorded as JSON, including `.enozunu/provenance.json`. Target AI-native directories, such as `.claude/`, are treated as generated outputs when a project is managed by Enozunu.

This direction makes the implementation language an architectural decision rather than a local implementation preference. The language affects CLI distribution, cross-platform behavior, parser ecosystem, filesystem safety, path validation, Git source resolution, diagnostic modeling, and long-term maintainability.

The implementation must support the following concerns:

- parsing and validating human-authored KDL manifests;
- generating machine-readable JSON records;
- resolving Git sources;
- copying or replacing materialized files and directories;
- preventing path traversal, unsafe symlink behavior, and writes outside the target root;
- producing stable diagnostics for invalid configuration and unsafe materialization plans;
- preserving a clear boundary between durable project policy and replaceable implementation details.

This decision is recorded as an ADR because it affects the build system, dependency policy, contribution model, binary distribution strategy, and the shape of the core domain model.

## Decision

Enozunu must be implemented in Rust.

The Rust implementation should be structured around explicit domain types for parsed configuration, validated configuration, source references, resolved sources, materialization plans, target paths, provenance records, and diagnostics. Stringly typed filesystem and manifest operations should be avoided at architectural boundaries.

The implementation must keep Git integration behind an internal abstraction. The initial implementation may use the external `git` command behind that abstraction. This ADR does not require adopting a pure Rust Git implementation in v0.0.x. A future implementation may replace the external command implementation with a Rust Git library if that becomes advantageous.

The implementation should prefer small internal crates or modules that separate at least the following concerns:

- CLI argument parsing and terminal output;
- KDL parsing and manifest validation;
- core domain model and materialization planning;
- Git source resolution;
- safe filesystem materialization;
- provenance JSON generation;
- diagnostic representation and rendering.

The Rust implementation must not imply that Enozunu performs provider semantic conversion. The language decision does not change the existing scope decision that v0.0.x focuses on Claude target materialization and does not guarantee semantic compatibility across target AIs.

## Alternatives Considered

### Go

Go was a viable alternative for a small cross-platform CLI. It offers fast implementation, straightforward subprocess handling, and simple cross-compilation.

Go was not selected because Enozunu's durable complexity is not limited to command dispatch. The project needs precise modeling of manifest state, validated paths, source resolution, materialization plans, and diagnostics. Rust gives stronger compile-time support for representing these distinctions.

The available KDL ecosystem was also a concern. Because Enozunu intentionally uses KDL for human-authored configuration, parser ecosystem quality and long-term fit are part of the architectural decision.

### TypeScript or Node.js

TypeScript was considered because it is convenient for JSON handling, CLI prototyping, and string-heavy workflows.

It was not selected because Enozunu is primarily a filesystem-materializing CLI rather than a web or Node ecosystem tool. A Node runtime dependency would make distribution and execution less self-contained than desired. TypeScript also gives weaker architectural pressure toward safe path and filesystem modeling than Rust.

### F# or .NET

F# was considered because it is strong for domain modeling and discriminated unions.

It was not selected because Enozunu is expected to be a lightweight cross-platform CLI. The Rust ecosystem is a better fit for small native binaries, low-level filesystem operations, and KDL-oriented tooling in this project.

### Zig

Zig was considered because it can produce small native binaries and gives low-level control.

It was not selected because Enozunu should not spend its early implementation budget on building or compensating for missing ecosystem pieces. The project benefits more from mature parser, CLI, JSON, diagnostic, and testing libraries than from lower-level control.

## Consequences

### Positive Consequences

- Enozunu can be distributed as a native CLI without requiring a language runtime from users.
- The core model can use Rust types to distinguish untrusted input, validated configuration, resolved sources, and safe target paths.
- Filesystem materialization safety can be designed around explicit invariants rather than ad hoc string checks.
- Diagnostics can be represented as stable structured values and rendered into human-readable or machine-readable output later.
- KDL parsing and JSON generation can use mature Rust libraries while keeping Enozunu's own manifest semantics in project-owned domain types.
- Future replacement of Git integration remains possible because Git operations are required to stay behind an abstraction.

### Negative Consequences

- Initial implementation may be slower than an equivalent Go implementation.
- Contributors must be comfortable with Rust's ownership model, error handling, and crate ecosystem.
- Over-modeling is a risk if the implementation introduces too many abstractions before v0.0.x requirements stabilize.
- Pure Rust Git integration is intentionally not decided here, so the first implementation may still depend on the external `git` command.

### Neutral Consequences

- The language decision does not decide the complete crate layout.
- The language decision does not require a plugin architecture.
- The language decision does not expand v0.0.x beyond Claude target materialization.
- The language decision does not require exact reproducibility, lockfile support, tag selectors, version ranges, or provider semantic conversion.
- If the project later changes its distribution or embedding model, this ADR may need to be superseded rather than edited in place.
