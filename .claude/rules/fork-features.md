# Fork-Specific Features

## PR #5: Git Argument Parsing Fix (Critical)
Fixed Clap parsing with proper trailing_var_arg configuration so git flags like `--oneline`, `--cached` are accepted.

## PR #6: pnpm Support
New commands: `rtk pnpm list/outdated/install`. 70-90% token reduction. Package name validation prevents command injection.

## PR #9: Modern JavaScript/TypeScript Tooling
Six commands for T3 Stack: lint (ESLint/Biome 84%), tsc (83%), next (87%), prettier (70%), playwright (94%), prisma (88%). Shared utils.rs for package manager auto-detection.

## Python & Go Support
Python: ruff check/format (80%+), pytest (90%+), pip list/outdated/install with uv auto-detect (70-85%). Go: go test NDJSON (90%+), go build (80%), go vet (75%), golangci-lint JSON (85%).

## Filter Development Checklist

When adding a new filter (`rtk newcmd`):

**Implementation**: create src/<cmd>_cmd.rs, add lazy_static! regex, implement fallback to raw command on error, preserve exit codes.

**Testing**: snapshot test with real fixture, verify >=60% token savings, test cross-platform escaping, edge cases (empty, errors, unicode, ANSI).

**Integration**: register in main.rs Commands enum, update README.md and CHANGELOG.md.

**Quality gates**: `cargo fmt --all && cargo clippy --all-targets && cargo test`, benchmark with hyperfine (<10ms), manual test with `rtk <cmd>`, verify fallback works.
