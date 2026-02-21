# rtk (Rust Token Killer) — Windows / .NET Usage Notes

> Personal reference notes. Last updated: 2026-02-21.
> Repo: https://github.com/rtk-ai/rtk (v0.22.0 current at time of writing)

---

## What It Is

rtk is a single Rust binary CLI proxy that compresses verbose command output before it reaches Claude Code's context window. It intercepts commands like `git status`, `dotnet test`, `cat`, etc. and strips boilerplate, aggregates noise, and returns only what's actionable.

Claimed savings: **60–90% token reduction per session**. A typical 30-minute Claude Code session drops from ~150K tokens to ~45K. The methodology is straightforward enough to be credible — most of the savings come from git operations and test output, which dump enormous boilerplate by default.

**Important naming collision:** There are two completely different projects named `rtk` on crates.io. The one you want is `rtk-ai/rtk`. Always verify after install:

```powershell
rtk gain   # must show token savings stats, not "command not found"
```

If `rtk gain` fails, you installed the wrong one (Rust Type Kit from `reachingforthejack/rtk`).

---

## Installation on Windows 11

No Homebrew, no install script for Windows. Two options:

### Option 1 — Pre-built MSVC binary (recommended)

Download `rtk-x86_64-pc-windows-msvc.zip` from the [releases page](https://github.com/rtk-ai/rtk/releases), extract, and add to PATH.

```powershell
# Add to PATH permanently (adjust path as needed)
$env:PATH += ";C:\tools\rtk"
[Environment]::SetEnvironmentVariable("PATH", $env:PATH, "User")
```

### Option 2 — Build from source via cargo

```powershell
cargo install --git https://github.com/rtk-ai/rtk
```

⚠️ Do **not** use `cargo install rtk` — that may install the wrong package from crates.io.

### Verify

```powershell
rtk --version   # should show "rtk 0.x.x"
rtk gain        # must show token savings dashboard
```

---

## Windows Hook Limitation (Critical)

`rtk init -g` installs a **bash shell hook** (`~/.claude/hooks/rtk-rewrite.sh`) that transparently rewrites commands before Claude Code executes them (e.g., `git status` → `rtk git status`). This is the recommended setup and provides 100% automatic rtk adoption.

**This hook does not work on Windows natively.** The `.sh` script requires bash.

Two open PRs are working on this:

- [PR #150](https://github.com/rtk-ai/rtk/pull/150) — Native cross-platform hook rewrite in Rust
- [PR #141](https://github.com/rtk-ai/rtk/pull/141) — Node.js/Bun hook for Windows (awaiting-changes)

**Status as of 2026-02-21: neither is merged.** Watch these PRs.

### Workaround: CLAUDE.md injection mode

This mode injects rtk instructions into your global CLAUDE.md instead of using a shell hook. No bash dependency. Claude Code reads the instructions and applies them — ~70–85% adoption rate (vs. 100% with the hook).

```powershell
rtk init -g --claude-md
```

**One caveat:** if Claude Code on your machine is actually routing commands through Git Bash (some Windows setups do this depending on how Claude Code resolves the shell), the existing `.sh` hook may actually work. Worth testing before assuming it won't.

---

## .NET Support

**No first-class `dotnet` subcommand exists in the current release.** There is one open PR:

- [PR #172](https://github.com/rtk-ai/rtk/pull/172) by `danielmarbach` — adds structured `rtk dotnet build/test/restore` with binlog/TRX parsing, locale-stable fallback behavior, and proper argument forwarding. Open, not merged.

### What works today (generic wrappers)

```powershell
rtk err dotnet build       # errors and warnings only — strips restore noise, progress bars
rtk test dotnet test       # failures only — ~90% token reduction on verbose test runs
rtk err dotnet restore     # restore errors only
```

These work but without structured TRX parsing. For large test suites that produce megabytes of output, `rtk test dotnet test` is still a meaningful win.

---

## Useful Commands for Daily Use

### Files

```powershell
rtk ls .                       # compact directory tree
rtk read .\src\Foo.cs          # smart file reading (strips noise)
rtk read .\src\Foo.cs -l aggressive  # signatures only, strips bodies
rtk grep "pattern" .           # grouped search results
```

### Git

```powershell
rtk git status
rtk git diff
rtk git log -n 10
rtk git add .
rtk git commit -m "msg"        # outputs: ok ✓ abc1234
rtk git push                   # outputs: ok ✓ main
```

### .NET (current workarounds)

```powershell
rtk err dotnet build
rtk test dotnet test
rtk err dotnet restore
rtk summary dotnet <anything>  # heuristic summary for unhandled subcommands
```

### Docker / Azure

```powershell
rtk docker ps
rtk docker images
rtk docker logs <container>
```

### Analytics

```powershell
rtk gain                       # token savings summary
rtk gain --graph               # with ASCII graph (last 30 days)
rtk gain --history             # with recent command history
rtk discover --all             # scan Claude Code session history for missed savings
```

---

## Configuration

Config file: `~/.config/rtk/config.toml`

```toml
[tee]
enabled = true
mode = "failures"    # "failures" | "always" | "never"
max_files = 20

[tracking]
# database_path = "C:/custom/path/history.db"  # override default
```

Default DB location: `~/.local/share/rtk/history.db`

The **tee feature** is useful: on command failure, rtk writes the full unfiltered output to `~/.local/share/rtk/tee/` and prints a one-liner hint so Claude Code can read the full log instead of re-running the command. Saves tokens on retry cycles.

---

## Things to Watch

| Item | Status |
|------|--------|
| Windows native hook (PR #150) | Open — check before next Claude Code session setup |
| `rtk dotnet` subcommand (PR #172) | Open — will add TRX parsing, binlog support |
| Node.js hook for Windows (PR #141) | Open, awaiting-changes |

---

## Bottom Line

Worth installing and using manually now. The token savings on `git diff`, `git log`, and `dotnet test` output are real and require zero configuration beyond installing the binary. Full transparent-hook automation is blocked on the Windows PR landing.

Re-evaluate hook setup when PR #150 merges. Re-evaluate dotnet integration when PR #172 merges.
