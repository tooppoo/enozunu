# Adopt a Lockfile for Whole-Run Reproducibility

- Status: Accepted
- Created: 2026-07-24T02:10:01Z

## Context

[The branch-selector-first ADR](20260708T104203Z_branch-selector-first-reproducibility-deferred.md) deferred whole-run reproducibility to v0.1.x.
[The Git tag selector ADR](20260719T062207Z_git-tag-selector.md) then recorded that reading `.enozunu/provenance.json` back as a resolution input would quietly turn it into a lockfile, and that doing so must be an explicit v0.1.x decision rather than a side effect.

This is that decision.

Until now, a manifest declaring a `branch` or `tag` selector materialized whatever commit the ref pointed at on each run.
Two runs of `enozunu summon` from one unchanged manifest could produce different output, and nothing recorded an intent to keep them the same.
Per-source `revision` pinning exists, but it freezes one source by hand and pushes commit-id bookkeeping onto the manifest author.

The open questions are durable contract questions — what file freezes a run, what `summon` reads by default, and how a frozen record is refreshed — so they are recorded as an ADR.

## Decision

### A separate lock file, not a promoted provenance record

Reproducibility is provided by a new machine-generated file, `enozunu.lock.json`, written next to the manifest it locks and intended to be committed.

The three per-project files now have one role each:

- `enozunu.kdl` — human-authored intent: which sources, which selectors, which targets.
- `enozunu.lock.json` — machine-written resolution input: which commit each mutable ref resolved to.
- `.enozunu/provenance.json` — machine-written execution record: what the last run actually materialized, per target.

`provenance.json` keeps its existing contract unchanged: it is written after every run, never read back, and stays at version `1`.
The "not a lockfile" statements across the documentation remain true; the lockfile is the thing they said provenance is not.

The format is JSON, as [the KDL-for-human, JSON-for-machine ADR](20260708T104204Z_kdl-for-human-json-for-machine.md) already named it the default candidate for a machine-generated lock record.

### Only mutable selectors are locked

A lock entry exists per distinct `(url, selector type, selector value)` with a mutable selector — `branch` or `tag` — and records the resolved commit:

```json
{
  "version": 1,
  "entries": [
    {
      "url": "https://github.com/example/repo",
      "selector": { "type": "branch", "value": "main" },
      "resolved_revision": "468aac8caed5f0c3b859b8286968e2c78e2b8760"
    }
  ]
}
```

The lock records exactly the information the manifest cannot express: where a mutable ref pointed.

- A `revision` source is already exact in the manifest; a lock entry would be a second copy that can go stale against the declaration.
- A Gist source pins an immutable revision in the manifest.
- A local source has no revision to freeze; it stays a live filesystem reference.

The entry key matches the resolution key the pipeline already uses for Git sources, and the source `path` is not part of it: sources sharing `(url, selector)` share one resolution, so they share one lock entry.
Entries are sorted by `(url, selector type, selector value)` and written as pretty-printed JSON, so a version-control diff of the lock only ever shows real resolution changes.
A manifest with no mutable Git sources still writes a lock with empty `entries`, keeping behavior uniform and letting `--frozen` pass trivially.
Unknown fields are tolerated on read, so an additive version-1 change stays readable by older builds; a `version` other than `1` is rejected as `unsupported-lock-version`.

### `summon` is lock-first by default

A plain `enozunu summon` resolves a mutable selector from its lock entry when one exists, and resolves it fresh — then locks it — when none does.
Reproducibility is opt-out, not opt-in: once a revision is recorded, a default run never silently follows a moved branch or tag.

A locked entry is resolved through the existing exact-revision path, which verifies that the materialized commit equals the recorded one.
A locked revision is therefore checked, not trusted.

The lock is rebuilt from each run's actual resolutions rather than edited incrementally.
That one rule yields the whole behavior matrix: a new source is added, a re-resolved source is refreshed, and a source removed from the manifest is pruned, all without diffing logic.
The file is rewritten only when its serialized bytes change, and the CLI announces `created` or `updated` only for a real file change.

Two flags on `summon` complete the surface:

- `--update` ignores the recorded revisions, re-resolves every mutable selector, and rewrites the lock. This is the one way a locked ref moves.
- `--frozen` resolves strictly from the lock and never writes it, failing with `lock-out-of-date` when the lock file is missing or lacks an entry for a mutable source. All gaps are reported in one run, before any resolution work, so a frozen failure has no network side effects. This is the CI mode.

A corrupt or unsupported lock file fails every mode, including `--update`: silently rebuilding over a record the user may have meant to keep would destroy the pinned revisions it held.
When a locked revision no longer exists upstream — a force-push followed by pruning — the resolution error names the lock as the likely cause and points at `enozunu summon --update` as the way out.

## Alternatives Considered

### Promoting `provenance.json` to a lockfile

Provenance holds more information than the lock, which makes promotion look economical.
The extra information is exactly why it was rejected.

Per entry, provenance records the source name, artifact kind, target AI, and target path — all derivable from the manifest on every run — plus machine-specific output such as a local source's absolute resolved path.
Reading those back as resolution input would duplicate the manifest inside a second file and demand reconciliation rules for every field that can disagree after a manifest edit.
The only fields a lock needs are the ones the manifest cannot supply, and extracting them is this decision.

The two records also live on opposite sides of a run.
Provenance is a snapshot written after a successful run and rewritten wholly by the next one; a lock is an input read before resolution that must stay stable until an explicit update.
One file serving both roles would rewrite the resolution input as a side effect of every run, and revision changes would drown in target-path and rename noise in its diffs.

### Locking every source, including `revision`, Gist, and local sources

A uniform "everything is locked" rule reads simpler, but a lock entry for an already-pinned source duplicates the manifest and introduces a new staleness class — the manifest pin moves and the lock disagrees — while a local source has nothing lockable at all.
The lock stays restricted to what the manifest cannot pin.

### An `enozunu update` subcommand instead of `summon --update`

A dedicated subcommand mirrors `cargo update` and could later grow per-source arguments.
For this milestone `--update` is the minimal surface; a subcommand can wrap the same mode later without breaking anything, so it is deferred rather than rejected.

### Resolving a locked branch by fetching the branch and comparing commits

Fetching the declared branch and verifying the tip equals the locked commit would reuse the shallow branch path.
It cannot materialize a locked commit that is no longer the tip — the shallow fetch only contains the tip — which is precisely the case the lock exists for.
The exact-revision path handles the general case and brings its verification for free.

## Consequences

### Positive Consequences

- One unchanged manifest plus one unchanged lock materializes the same commits on every run, on every machine that can reach the sources.
- A branch or tag moving upstream becomes an explicit, reviewable lock diff instead of a silent output change.
- `--frozen` gives CI a mode that fails instead of resolving anything the lock does not cover.

### Negative Consequences

- The default behavior of `summon` changes: a mutable selector no longer follows its ref once locked. Users who relied on implicit tracking must now run `summon --update`.
- A locked branch or tag resolves through the exact-revision cache slot, which performs a full clone the first time even when a shallow branch slot already exists. A future optimization could probe existing slots first; it is out of scope here.
- One more machine-generated file exists at the project root, and it must be committed to deliver its guarantee.

### Neutral Consequences

- `url`, selector, and `resolved_revision` appear in both the lock and provenance. Both files are machine-rebuilt on every run, so nothing requires keeping them synchronized by hand.
- The frozen check tolerates stale extra entries in the lock; a strict equality mode in the spirit of `npm ci` remains future work.
- `summon` keeps the existing CLI output contract — text lines on stdout, `error[<code>]` diagnostics on stderr, a single failure exit code. Structured output formats, categorized exit codes, and a dry-run mode remain open CLI-wide decisions, unchanged by this ADR.
- Version ranges, dependency solving, and registry discovery remain out of scope, as recorded in [the v0.1.x goal](../v0.1.x-goal.md).

## Compatibility

The lock file starts at version `1` with the same forward policy provenance uses: the version marks the migration boundary for external consumers.

The manifest contract is unchanged; no new manifest syntax exists for the lock.
A project without a lock file behaves like a first run: everything resolves fresh and the lock is created.
The first `summon` after upgrading therefore changes no materialized output; behavior diverges from v0.0.x only on later runs, once recorded revisions exist.

The `run_materialize` library entry point gains a lock-mode parameter and returns a lock outcome alongside the materialized entries, which is a breaking change for direct library callers.

## Related

- [The branch-selector-first ADR](20260708T104203Z_branch-selector-first-reproducibility-deferred.md), which deferred this decision to v0.1.x
- [The Git tag selector ADR](20260719T062207Z_git-tag-selector.md), which required lockfile behavior to be an explicit decision rather than a side effect
- [The KDL-for-human, JSON-for-machine ADR](20260708T104204Z_kdl-for-human-json-for-machine.md), which named JSON as the lockfile format candidate
- [The Git exact revision selector ADR](20260712T155345Z_git-exact-revision-selector.md), whose resolution path and verification the locked resolution reuses
