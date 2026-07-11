# Use KDL for Human-Authored Configuration and JSON for Machine-Generated Records

- Status: Accepted
- Created: 2026-07-08T10:42:04Z

## Context

Enozunu handles both declarative configuration edited by humans and derived records generated and updated by Enozunu.

A manifest that humans read and write benefits from a format with readable structure, comments, and hierarchy.

A record that machines generate and update only needs a stable machine-readable format and is not meant to be edited by hand.

## Decision

Human-authored configuration must be KDL.

Machine-generated records must be JSON.

v0.0.x adopts the following layout.

```text
enozunu.consumer.kdl        # human-authored configuration
.enozunu/provenance.json    # machine-generated derived record
```

`.enozunu/provenance.json` is a Git-managed machine-generated derived record.

## Consequences

- The manifest stays easy to read and write as KDL.
- Provenance is easy to implement and process as JSON.
- `.enozunu/provenance.json` is not something humans edit by hand.
- If a lockfile is introduced later, JSON is the default candidate for that machine-generated record as well.

## Related

- Issue: [#8](https://github.com/tooppoo/enozunu/issues/8)
