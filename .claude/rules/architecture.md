# Architecture

## Core Design Pattern

rtk uses a **command proxy architecture** with specialized modules:

```
main.rs (CLI entry) -> Clap command parsing -> Route to specialized modules -> tracking.rs (SQLite) records token savings
```

## Key Components

1. **Command Modules** (src/*_cmd.rs, src/git.rs, src/container.rs) -- each handles a specific command type, executes underlying commands and transforms output
2. **Core Filtering** (src/filter.rs) -- language-aware code filtering (Rust, Python, JS, etc.), filter levels: none/minimal/aggressive
3. **Token Tracking** (src/tracking.rs) -- SQLite at ~/.local/share/rtk/tracking.db, 90-day retention, configurable via RTK_DB_PATH env var or config.toml
4. **Configuration** (src/config.rs, src/init.rs) -- reads ~/.config/rtk/config.toml, `rtk init` bootstraps LLM integration
5. **Tee Output Recovery** (src/tee.rs) -- saves raw output to ~/.local/share/rtk/tee/ on failure, prints hint for LLM re-read
6. **Shared Utilities** (src/utils.rs) -- common functions, package manager auto-detection (pnpm/yarn/npm/npx)
7. **Discovery** (src/discover/) -- Claude Code history analysis, scan JSONL sessions, classify commands, report missed savings

## Command Routing

```rust
main.rs:Commands enum -> match routes to module -> module::run() -> tracking::track_command() -> Result<()>
```

## Module Responsibilities

| Module | Purpose | Token Savings |
|--------|---------|---------------|
| git.rs | Git operations | Stat summaries + compact diffs |
| grep_cmd.rs | Code search | Group by file, truncate lines |
| ls.rs | Directory listing | Tree format, aggregate counts |
| read.rs | File reading | Filter-level stripping |
| runner.rs | Command execution | Stderr/failures only |
| log_cmd.rs | Log parsing | Deduplication with counts |
| json_cmd.rs | JSON inspection | Structure without values |
| lint_cmd.rs | ESLint/Biome | 84% reduction |
| tsc_cmd.rs | TypeScript compiler | 83% reduction |
| next_cmd.rs | Next.js build/dev | 87% reduction |
| prettier_cmd.rs | Format checking | 70% reduction |
| playwright_cmd.rs | E2E tests | 94% reduction |
| prisma_cmd.rs | Prisma CLI | 88% reduction |
| gh_cmd.rs | GitHub CLI | 26-87% reduction |
| vitest_cmd.rs | Vitest runner | 99.5% reduction |
| pnpm_cmd.rs | pnpm | 70-90% reduction |
| ruff_cmd.rs | Ruff linter/formatter | 80%+ reduction |
| pytest_cmd.rs | Pytest runner | 90%+ reduction |
| pip_cmd.rs | pip/uv package mgr | 70-85% reduction |
| go_cmd.rs | Go commands | 80-90% reduction |
| golangci_cmd.rs | golangci-lint | 85% reduction |

## Proxy Mode

`rtk proxy <command>` -- execute without filtering but track usage for metrics. All proxy commands show 0% savings in `rtk gain --history`.
