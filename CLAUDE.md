
## File changes

- When making file changes, always use the `git-kura` skill unless the user explicitly instructs otherwise.
- Follow the workflow defined by the `git-kura` skill. Do not duplicate or reinterpret that workflow in this file.
- When design or implemnts CLI output, always use the `cli-output-design` skill to design, implements and self review.
- After completing file edits, always use the `subagent-review-loop` skill to review and revise the changes before reporting completion, unless the user explicitly instructs otherwise.
- When updating documentation, always use the `documentation-writing` skill.
- When writing or modifying code comments, always use both the `documentation-writing` skill and the `code-comment` skill.
- When implments e2e test, always use `reportage` skill to write test script.

## Pull Request

- Use `.github/pull_request_template.md` as template.
