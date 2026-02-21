# Coding Standards

## Performance Constraints

| Metric | Target | Verification |
|--------|--------|-------------|
| Startup time | <10ms | `hyperfine 'rtk git status' 'git status'` |
| Memory | <5MB resident | `/usr/bin/time -l rtk git status` |
| Token savings | 60-90% | `count_tokens()` assertions in tests |
| Binary size | <5MB stripped | `ls -lh target/release/rtk` |

Performance regressions are release blockers. RTK achieves <10ms through: zero async overhead (single-threaded, no tokio), lazy regex compilation (lazy_static!), minimal allocations (borrow over clone), no user config on startup (loaded on-demand).

## Error Handling

- **anyhow::Result** for CLI binary (application, not library)
- **ALWAYS** use `.context("description")` with `?` operator
- **NO unwrap()** in production code (tests only with `expect("explanation")`)
- **Graceful degradation**: if filter fails, fallback to raw command execution

## Common Pitfalls

- **Don't add async dependencies** -- kills startup time (+5-10ms overhead)
- **Don't recompile regex at runtime** -- use `lazy_static!` for all patterns
- **Don't panic on filter failure** -- always fallback to raw command
- **Don't assume command output format** -- test with real fixtures from multiple versions
- **Don't skip cross-platform testing** -- shell escaping, path separators, line endings all differ
- **Don't break pipe compatibility** -- `rtk cmd | grep x` must work, preserve stdout/stderr separation, respect exit codes

## Git Argument Handling (Critical)

src/git.rs uses `trailing_var_arg = true` + `allow_hyphen_values = true` for proper git flag handling. Auto-detects `--merges` to avoid conflicting with `--no-merges` injection.

## Language Detection

File extension-based with fallback heuristics. Supports Rust, Python, JS/TS, Java, Go, C/C++, etc. Tokenization rules vary by language.
