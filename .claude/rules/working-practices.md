# Working Practices

## Working Directory Confirmation
Always confirm working directory and branch before starting work: `pwd` and `git branch`.

## Avoiding Rabbit Holes
Stay focused on the task. If verification requires more than 3-4 exploratory commands, STOP and ask whether to continue or trust available info. Don't over-test regex patterns, deep-dive external docs, or over-verify cross-platform behavior.

## Plan Execution
When given a numbered plan: execute sequentially, commit after each logical step, never skip or reorder, track progress for plans with 3+ steps, validate file paths before starting.

## Testing Policy
Manual testing is REQUIRED for filter changes and new commands. Run `rtk <cmd>` and inspect output -- don't rely solely on automated tests. For hook changes, test in a real Claude Code session.

## TDD Workflow (Mandatory)
All code follows Red-Green-Refactor. Unit tests embedded in modules (`#[cfg(test)] mod tests`), smoke tests via `scripts/test-all.sh`. See `.claude/rules/cli-testing.md` for comprehensive testing strategy.
