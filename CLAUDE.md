# rtk (Rust Token Killer)

High-performance CLI proxy that minimizes LLM token consumption. 60-90% savings through filtering, grouping, truncation, and deduplication. Fork with git argument fixes + modern JS/TS/Python/Go support.

## Quick Reference

```bash
cargo fmt --all && cargo clippy --all-targets && cargo test --all  # mandatory before commit
cargo build --release                                               # release build
bash scripts/test-all.sh                                            # smoke tests (69 assertions)
```

## Key Constraints

- **<10ms startup** -- no async, no tokio, lazy_static! regex, minimal allocations
- **60-90% token savings** -- verified with count_tokens() assertions in tests
- **No unwrap() in production** -- anyhow::Result + .context() everywhere
- **Graceful degradation** -- filter failure falls back to raw command execution
- **Pipe compatible** -- preserve stdout/stderr separation, respect exit codes

## Architecture

Command proxy: `main.rs (Clap) -> route to src/*_cmd.rs module -> tracking.rs (SQLite)`. See `.claude/rules/architecture.md` for module table and routing details.

## Detailed Rules

All detailed guidance lives in `.claude/rules/` for on-demand loading:

| Rule File | Contents |
|-----------|----------|
| `project-overview.md` | Description, name collision warning, dependencies |
| `architecture.md` | Components, routing, module responsibilities, proxy mode |
| `coding-standards.md` | Performance targets, error handling, pitfalls |
| `development-commands.md` | Build, test, lint, package commands |
| `cli-testing.md` | Snapshot tests, token accuracy, cross-platform, integration |
| `fork-features.md` | PR history, Python/Go support, filter checklist |
| `working-practices.md` | TDD, testing policy, plan execution, rabbit hole avoidance |
