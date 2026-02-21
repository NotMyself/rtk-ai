# Development Commands

> Prefer `rtk <cmd>` over raw commands for token-optimized output when rtk is installed.

## Build & Run

```bash
cargo build                   # dev build
cargo build --release         # release build
cargo run -- <command>        # run directly
cargo install --path .        # install locally
```

## Testing

```bash
cargo test                    # all tests
cargo test <test_name>        # specific test
cargo test -- --nocapture     # with output
cargo test <module>::         # module-specific
bash scripts/test-all.sh      # smoke tests (69 assertions, requires installed binary)
```

## Linting & Quality

```bash
cargo check                   # check without building
cargo fmt                     # format code
cargo clippy --all-targets    # all clippy lints
```

## Build Verification (Mandatory)

After ANY Rust file edits, ALWAYS run before committing:
```bash
cargo fmt --all && cargo clippy --all-targets && cargo test --all
```

Never commit code that hasn't passed all 3 checks. Fix ALL clippy warnings (zero tolerance).

## Package Building

```bash
cargo install cargo-deb && cargo deb          # DEB (Linux)
cargo install cargo-generate-rpm && cargo build --release && cargo generate-rpm  # RPM
```
