
## File changes

- When making file changes, always use the `git-kura` skill unless the user explicitly instructs otherwise.
- Follow the workflow defined by the `git-kura` skill. Do not duplicate or reinterpret that workflow in this file.
- After completing file edits, always use the `subagent-review-loop` skill to review and revise the changes before reporting completion, unless the user explicitly instructs otherwise.
- When updating documentation, always use the `semantic-line-breaks` skill.
- When writing or modifying code comments, always use both the `semantic-line-breaks` skill and the `code-comment` skill.

## Subagent review loop

When using `subagent-review-loop` in this repository, use `reportage-reviewer` as the single reviewer subagent.

Do not add additional reviewer subagents during the loop unless the user explicitly overrides this rule.
