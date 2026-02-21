# Project Overview

**rtk (Rust Token Killer)** is a high-performance CLI proxy that minimizes LLM token consumption by filtering and compressing command outputs. It achieves 60-90% token savings on common development operations through smart filtering, grouping, truncation, and deduplication.

This is a fork with critical fixes for git argument parsing and modern JavaScript stack support (pnpm, vitest, Next.js, TypeScript, Playwright, Prisma).

## Name Collision Warning

**Two different "rtk" projects exist:**
- This project: Rust Token Killer (rtk-ai/rtk)
- reachingforthejack/rtk: Rust Type Kit (DIFFERENT - generates Rust types)

Verify correct installation: `rtk --version` should show "rtk 0.22.2" (or newer), and `rtk gain` should show token savings stats.

## Core Dependencies

clap (CLI parsing), anyhow (error handling), rusqlite (SQLite tracking), regex (filtering), ignore (gitignore-aware traversal), colored (terminal output), serde/serde_json (config and JSON parsing).

## Build Optimizations

Release profile: opt-level 3, LTO enabled, single codegen unit, stripped symbols, panic=abort.
