---
name: subagent-review-loop
description: Use exactly one subagent to review the current changes or branch, fix every valid actionable finding, re-request review from the same reviewer until no actionable findings remain, then create a pull request and stop.
---

# Subagent review loop

Use this skill when the user asks for a workflow like: "start one subagent, ask it to review, fix findings, re-review, create a PR when clean."

## Hard constraints

- Use exactly one reviewer subagent role.
- Do not start parallel reviewers.
- Do not use agent teams, dynamic workflows, batch workflows, or multiple independent reviewer sessions.
- The parent agent owns implementation, commits, push, and PR creation.
- The reviewer subagent is read-only unless the user explicitly says otherwise.
- Reuse the same reviewer thread/session for re-review when the host supports it.
- If the host cannot reuse a reviewer thread, run at most one reviewer invocation at a time and treat it as the same reviewer role.
- Do not create a PR while any valid actionable reviewer finding remains unresolved.
- Do not continue after the PR is created. Report the PR URL and stop.

## Definitions

- "Finding" means a concrete, actionable review comment supported by code evidence.
- "Valid finding" means the parent agent confirms that the reported problem is real or plausibly risky enough to fix.
- "No findings" means the reviewer reports no valid actionable findings. Non-blocking notes may be carried into the PR body, but must not block PR creation.

## Preflight

1. Inspect repository guidance files if present, such as `AGENTS.md`, `CLAUDE.md`, `.github/pull_request_template.md`, project README, and package scripts.
2. Determine the review target:
   - Prefer the current branch diff against its upstream base.
   - If no upstream base is clear, compare against `main`.
   - If `main` does not exist, compare against `master`.
   - If no base can be determined, stop and report the blocker.
3. Inspect current git state.
4. If there are unrelated uncommitted changes, do not overwrite or discard them. Either avoid touching them or stop with a clear blocker.
5. Identify available verification commands from project guidance and manifests. Prefer repository-defined commands over guessed commands.

## Reviewer selection

Select exactly one reviewer subagent role.

Reviewer selection order:

1. If the user explicitly names a reviewer subagent for this task, use that reviewer unless it conflicts with repository guidance or the hard constraints of this skill.
2. If repository guidance files specify a reviewer subagent for the current repository, change type, or workflow, use that reviewer.
3. Otherwise, choose the nearest available read-only reviewer-capable subagent.

When choosing a fallback reviewer, prefer a subagent whose name or description indicates code review capability, such as:

- `reviewer`
- `code-reviewer`
- names containing review or reviewer
- `general-purpose`
- `explorer`
- the nearest host-supported equivalent

If multiple reviewer candidates are available, choose the single best match for the changed files and repository guidance.

Do not use more than one reviewer subagent.
Do not run specialized reviewers in parallel.
Do not switch reviewers during the loop unless the selected reviewer is unavailable or cannot perform read-only review at all.

## Initial reviewer delegation

Start exactly one reviewer subagent, preferably named `reviewer`, `code-reviewer`, `general-purpose`, `explorer`, or the nearest host-supported equivalent.

Send this prompt to the reviewer:

```text
Review the current branch diff against the selected base branch.

You are the only reviewer subagent for this task. Work read-only. Do not edit files, commit, push, or create a PR.

Focus on:
- correctness and behavior regressions
- error handling and failure modes
- security and unsafe command/file/network behavior
- test gaps
- maintainability and extension risks
- documentation or ADR omissions when they affect the change's long-term correctness

Return only one of these forms:

1. If there are actionable findings:

ACTIONABLE FINDINGS
- Severity: blocker | high | medium | low
  File/line or symbol:
  Evidence:
  Why it matters:
  Suggested fix:

2. If there are no actionable findings:

NO ACTIONABLE FINDINGS

Rules:
- Every finding must be specific and supported by evidence.
- Do not include stylistic preferences unless they affect correctness, safety, maintainability, or project consistency.
- Do not suggest creating a PR; the parent agent owns PR creation.
```

## Review / fix loop

Repeat until the reviewer returns `NO ACTIONABLE FINDINGS`.

1. Classify each finding:
   - Valid and must fix.
   - Valid but intentionally deferred only if deferral is safer or explicitly justified by repository scope.
   - Invalid, with a concrete reason.
2. Fix every valid finding that should be fixed in this PR.
3. Run the smallest meaningful verification command after each coherent fix set.
4. If verification fails, fix the failure before re-review.
5. Ask the same reviewer to re-review the new diff and the resolution notes.
6. Do not spawn a second reviewer.

Use this re-review prompt:

```text
Re-review the updated branch diff against the same base branch.

Use the same review criteria as before.

Resolution notes from the parent agent:
- Fixed:
  <list concrete fixes>
- Not fixed, with reasons:
  <list invalid/deferred findings and justification>

Confirm whether any valid actionable findings remain.

Return exactly:
- ACTIONABLE FINDINGS with evidence, or
- NO ACTIONABLE FINDINGS.
```

If the same finding repeats after a fix attempt, inspect the relevant code path directly. If the finding remains valid, fix it. If it is invalid, explain why in the next re-review request. If the loop cannot converge because of a real blocker, stop and report the blocker instead of creating a PR.

## Final verification

When the reviewer returns `NO ACTIONABLE FINDINGS`:

1. Run the repository's required verification commands.
2. Confirm the working tree contains only intended changes.
3. Commit the changes if needed, matching repository commit conventions.
4. Push the branch if needed.
5. Create a PR using the repository's standard tool, usually `gh pr create`.
6. Match the repository's PR language and style. If existing Issues and PRs are Japanese, write the title and body in Japanese.
7. Include:
   - Summary of changes.
   - Verification commands and results.
   - Reviewer loop result: one reviewer used; final result was no actionable findings.
   - Any intentionally deferred non-blocking notes.
8. After PR creation, report only:
   - PR URL.
   - Final verification status.
   - Any deferred non-blocking notes.
   Then stop.

## Safety rules

- Never delete user work to make the tree clean.
- Never force-push unless the user explicitly requested it.
- Never create a PR if tests or required verification fail.
- Never hide reviewer findings. If a finding is not fixed, state why.
- Never substitute multiple agents for one reviewer, even if multiple review dimensions exist.
