# MasiX - AI-Friendly Modular Assistant (Rust)

[![Status](https://img.shields.io/badge/Status-0.3.6-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Target](https://img.shields.io/badge/Target-Termux%20%2F%20Linux%20%2F%20macOS-green.svg)](https://termux.dev)
[![npm](https://img.shields.io/npm/v/@mmmbuto/masix?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/masix)

MasiX is a Rust runtime for AI-assisted automation with chat adapters, tool execution, and modular extension paths.

Core capabilities:
- Single runtime with deterministic CLI surfaces for operators and AI workers
- Chat integration with command routing and role-based permissions
- Tool runtime with MCP support and API-compatible endpoint routing
- Persistent state for reminders, runtime data, and plugin metadata
- Telegram progressive output mode (`telegram_draft`) for private chat streaming
- Dual module install path: server catalog or local `.pkg` artifacts

## Project Status

- Current line: `0.3.6`
- Core track is MIT and automation-first
- Optional capabilities are delivered as modules/plugins
- Free modules can be installed from server catalog or local `.pkg` files
- Optional/advanced modules may be distributed separately from the MIT core

## Quickstart

1. Install CLI (global)

```bash
npm install -g @mmmbuto/masix@latest
```

2. Initialize config

```bash
masix config init
```

3. Validate config

```bash
masix config validate
```

4. Start runtime

```bash
masix start
masix status
```

Configuration paths and file responsibilities:
- [Config Files Reference](docs/CONFIG_FILES_REFERENCE.md)

## Architecture (high-level)

- `Adapters`: chat/transport inputs (for example Telegram) are normalized into runtime events.
- `Router`: provider profile resolution selects the active API-compatible endpoint and model.
- `Tools`: built-in tools and MCP tools are exposed behind policy checks.
- `Storage`: runtime and plugin state are persisted under `data_dir` (`~/.masix` by default).
- `Execution`: command handlers, reminders, and plugin calls run in a deterministic CLI/runtime flow.
- `Output`: responses are emitted back to chat/CLI with structured status and diagnostics surfaces.

## Documentation

- [User Guide](docs/USER_GUIDE.md)
- [Commands Reference](docs/COMMANDS_REFERENCE.md)
- [Config Files Reference](docs/CONFIG_FILES_REFERENCE.md)
- [Power Config Example (Sanitized)](docs/CONFIG_EXAMPLE_POWER.md)
- [Homebrew Tap (Formula)](https://github.com/DioNanos/homebrew-masix)
- [Termux Local AI Endpoint](docs/TERMUX_LLAMA_CPP_LOCAL_ENDPOINT.md)
- [Third-Party Notices](docs/THIRD_PARTY_NOTICES.md)

## Module Distribution Policy

Optional modules may be distributed separately.
Local `.pkg` installation remains supported.
The MIT core remains self-contained.
MIT-bundled free packages currently include `discovery`, `codex-backend`, and `codex-tools`.

## Module Install Modes

1. Server catalog

```bash
masix plugin install <plugin> --server <url>
```

2. Local package (`.pkg`) without server

```bash
masix plugin install-file \
  --file /absolute/path/to/module.pkg \
  --plugin <plugin-id> \
  --version <version> \
  --package-type mcp_binary
```

## Future Roadmap

1. Broader free module catalog in MIT track with signed manifests/checksums
2. Optional module distribution flow separate from the MIT core
3. Stronger AI self-configuration surfaces and recovery actions
4. Expanded packaging/validation matrix across target environments
5. Hardened runtime/tool/module security boundaries

## Build

```bash
# Core-only (default)
cargo build -q

# Core + local STT
cargo build -q -p masix-cli --features stt
```

## Security Notes

- Keep `Admin` permissions only for trusted operators
- Treat MCP servers and local helpers as trusted code
- Protect config/data files with proper filesystem permissions
- Keep `/exec` disabled unless explicitly needed

## Repository Layout

```text
crates/masix-core        runtime orchestration
crates/masix-telegram    Telegram adapter and menus
crates/masix-storage     SQLite persistence and cron storage
crates/masix-providers   provider adapters and tool-calling
crates/masix-cli         CLI entrypoint
docs/                    operational/configuration guides
npm/masix-termux         Termux package
packages/                MIT free module package track (when present)
```

## License

MIT License
<p>
Copyright (c) 2026 WellaNet.Dev<br>
Made in Italy 🇮🇹
</p>
