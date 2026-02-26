# MasiX

[![Status](https://img.shields.io/badge/Status-0.3.0-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Target](https://img.shields.io/badge/Target-Termux%20%2F%20Linux%20%2F%20macOS-green.svg)](https://termux.dev)
[![npm](https://img.shields.io/npm/v/@mmmbuto/masix?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/masix)

MasiX is a Rust-first automation runtime built for Termux/Android first, with Linux and macOS support.

## Project Status

- `0.3.0` core-only MIT baseline
- Focus: Telegram, MCP tool-calling, reminders, stable Termux runtime
- Optional modules are installed later via the wizard / plugin manager

## Core Features (MIT)

- Termux-first runtime with Linux/macOS support and stable CLI workflows
- Telegram adapter with persistent offsets, chat commands, and inline menu support
- Role-based permissions (`Admin`, `User`, `Readonly`) with channel-aware checks
- Multi-bot isolation (account-scoped workdirs + per-bot/user runtime state)
- SQLite-backed persistence for reminders, offsets, and runtime metadata
- Tool-calling runtime with optional MCP servers and admin-gated execution paths
- OpenAI-compatible providers (per-bot and per-user overrides)
- Local STT (`whisper.cpp`) remains in-core and optional (`stt` feature)
- Termux lifecycle helpers and diagnostics (`verify`, `doctor`, `boot`, `wake`)

## Quickstart

```bash
# Termux / Android
npm install -g @mmmbuto/masix@latest

# macOS (Homebrew tap)
brew tap DioNanos/masix
brew install masix

# Linux/macOS source build
cargo build -q -p masix-cli
```

```bash
masix --help
masix config init
masix config validate
masix start
```

## Build

```bash
# Core-only (default)
cargo build -q

# Core + local STT (still core)
cargo build -q -p masix-cli --features stt
```

## Local STT (Optional, In-Core)

```bash
masix config stt
```

- STT is optional and disabled by default.
- Wizard can download/install or help build `whisper.cpp` CLI.
- Keep it disabled if audio transcription is not needed.

## Security Notes

- Use least privilege and keep `Admin` only for trusted operators.
- Keep `/exec` disabled unless strictly needed.
- Treat MCP servers and local helpers as trusted code only.
- Protect config/data (`~/.masix`) with proper filesystem permissions.

## Common Commands

```bash
masix start
masix stop
masix status
masix verify
masix doctor
masix cron list
masix termux boot status
masix termux wake status
```

## Repository Layout

```text
crates/masix-core        runtime orchestration
crates/masix-telegram    Telegram adapter + menus
crates/masix-storage     SQLite persistence and cron storage
crates/masix-providers   provider adapters and tool-calling
crates/masix-cli         CLI entrypoint
npm/masix-termux         Termux package
```

## License

MIT License
Copyright (c) 2026 Davide A. Guglielmi
Made in Italy 🇮🇹
