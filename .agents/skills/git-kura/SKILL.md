---
name: git-kura
description: Use git-kura to manage task worktrees, path seals, and tool components safely. Activate for review work or any repository file-changing task — implementing features, updating docs, adding tests, refactoring — in a git-kura-managed repository, even when the user does not mention git-kura explicitly.
---

# Activation

Use this skill before any task that involves reviewing or changing repository files in a git-kura-managed repository, even when the user does not write "git-kura".

**Activate for prompts such as:**

- Review: `review #123`, `PR #123 を確認`, `#123 をレビュー`, `このPRを見て`
- Task start: `作業開始`, `worktree を切って`, `branch を切って`, `このIssueを進める`
- File changes: `README を更新`, `docs を更新`, `テストを追加`, `このファイルを修正`, `実装して`, `修正して`, `リファクタして`
- Bare issue or PR reference implying review or changes: `#115 を実装`, `#115`

**Do not activate solely for pure remote GitHub operations with no repository file changes:**

Editing an issue body, adding a comment, creating subissues, or converting an issue into a tracking issue — unless these are part of a repository file-changing workflow.

git-kura is the source of truth for resolving task keys, branches, and worktree paths. Do not create ad-hoc worktrees or branches when git-kura should manage them.

# Key Selection

Choose a stable, shell-safe key before changing files.

- Use the GitHub issue number for issue work, without `#`.
- Use an explicitly provided key when the user gives one.
- For non-issue work, derive a short slug from the task, such as `installer-script` or `json-output`.
- For review work, use the implementation target's key.

# Implementation Workflow

For implementation, docs, tests, or refactors, always follow these steps in order. Never start editing before claiming the files you intend to change.

1. Open the worktree at the start of the task:

   ```sh
   git kura open <key>
   ```

   If `git kura open <key>` reports that the branch or worktree already exists, reuse the existing one.

2. Move into the worktree:

   ```sh
   cd "$(git kura get <key>)"
   ```

3. Check the worktree state before starting work:

   ```sh
   git status --short
   ```

   Review uncommitted changes and the index. If the worktree has unexpected changes from a prior session, clarify with the user before proceeding.

4. List the files you plan to change, then claim all of them:

   ```sh
   git kura seal claim <files...>
   ```

   Claims are repository-root relative and require each path to be an existing file (not a directory). For files you will create, create them first (for example with `touch`) and then claim them.

5. If the claim conflicts, stop and report. A cross-key conflict makes `seal claim` exit with code 6 and print `seal-conflict:` along with the key that already claims each path. Report the conflicting files and the owning key, then pause the task. Do not unclaim another key's paths or edit around the conflict.

6. If there is no conflict, make the actual changes. Edit only claimed paths. If the set of files to change grows, claim the new files before editing them.

7. Before committing, verify staged files pass the seal check:

   ```sh
   git kura seal test <staged-files...>
   ```

   Or rely on the pre-commit hook if `pre-commit` is installed via `git kura tools install pre-commit`.

8. Check the worktree state before leaving:

   ```sh
   git status --short
   ```

   Confirm there are no unexpected uncommitted changes before tearing down.

9. After review, create the PR only when you are told to. Do not push or open a PR before review.

10. When told to merge, release every path you claimed:

    ```sh
    git kura seal unclaim <files...>
    ```

    Use `git kura seal ls <key>` to confirm which paths the key still claims.

11. Tear down the worktree:

    ```sh
    cd "$(git kura get <key> --root)"   # back to the repository root
    git kura close <key>                 # delete worktree and branch
    git pull                             # update main
    ```

# Review Workflow

For reviews:

1. Identify the target key. If uncertain, run:

   ```sh
   git kura ls
   ```

2. Resolve and enter the target worktree:

   ```sh
   cd "$(git kura get <key>)"
   ```

3. Confirm the target:

   ```sh
   git status --short
   git branch --show-current
   git log --oneline --decorate -n 10
   ```

4. Review from inside that worktree.
5. In review mode, do not edit files unless the user explicitly asks for fixes. If fixes are requested, run `git status --short` and claim the files before making changes, following the implementation workflow's seal rules.

# Safety Rules

- Do not implement directly in the repository root when a worktree can be used.
- Do not guess worktree paths manually.
- Do not edit a file before claiming it with `git kura seal claim`.
- Do not unclaim or override another key's seal to work around a conflict; report the conflict and stop instead.
- Do not run `git merge`, `git rebase`, `git push`, or destructive operations unless explicitly instructed.
- Do not run `git kura close <key>` unless the user asks for cleanup.
- Do not close a dirty worktree without explicit user confirmation.
