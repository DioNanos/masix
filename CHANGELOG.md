# Changelog

## 0.3.4 - 2026-03-03

- Surgical MIT sync of stable core runtime improvements:
  - agent loop options (`core.agent_loop`) with controlled auto-continue behavior
  - tool progress controls (`core.tool_progress`)
  - streaming config surface (`core.streaming`)
  - cron delivery retry/backoff and dead-letter logging (`core.cron`)
  - MCP per-server timeout/startup/healthcheck controls
  - runtime context tools and group visibility commands (`/groups`, `/admin groups`, `chat_context`)
- Hardened MCP tool routing to resolve server/tool names by longest prefix.
- Updated minimal public docs for command/config parity:
  - `docs/COMMANDS_REFERENCE.md`
  - `docs/CONFIG_FILES_REFERENCE.md`
- Refreshed Termux Android prebuilt binary in npm package track.

## 0.3.3 - 2026-03-02

- MIT package track expanded with bundled free modules:
  - `codex-backend` `0.1.3` (android-aarch64-termux, linux-x86_64, macos-aarch64)
  - `codex-tools` `0.1.2` (android-aarch64-termux, linux-x86_64, macos-aarch64)
  - `discovery` `0.2.3` metadata updated with macOS artifact
- Added `docs/THIRD_PARTY_NOTICES.md` with upstream Codex Apache-2.0 attribution notes.
- Fixed plugin reinstall/update race (`text file busy`, ETXTBSY) using atomic package write/copy in CLI plugin manager.

## 0.3.2 - 2026-03-02

- Release alignment with modules-server security hardening:
  - global registration flow is now served by `@MasiX_Register_BOT`
  - per-endpoint API rate-limits enforced server-side
  - artifact checksum/size verification enforced server-side before download
  - catalog served in minimal mode with per-platform integrity metadata
- Package metadata normalized to canonical repository URL (`DioNanos/MasiX`).
- Expanded MIT package track under `packages/free/`:
  - added `codex-backend` package set (`0.1.3`, android/linux/macos)
  - added `codex-tools` package set (`0.1.2`, android/linux/macos)
  - updated `discovery` package set (`0.2.3`) with macOS artifact metadata
- Added `docs/THIRD_PARTY_NOTICES.md` with Codex Apache-2.0 attribution reference.

## 0.3.1 - 2026-03-02

- Added `masix plugin install-file` command to install plugin `.pkg` from local filesystem (offline, no server required).
- Added MIT package layout for free `discovery` module:
  - `packages/free/discovery/0.2.3/discovery-android-aarch64-termux.pkg`
  - `packages/free/discovery/0.2.3/discovery-linux-x86_64.pkg`
  - `packages/free/discovery/0.2.3/manifest.json`
  - `packages/free/discovery/0.2.3/SHA256SUMS`
- Kept non-MIT advanced modules out of bundled MIT package folder for now; they are distributed separately.

## 0.3.0 - 2026-02-26

- Clean restart of the public MIT repository as a core-only baseline.
- Core scope focuses on Telegram, MCP/tool-calling, cron/reminders, providers, and Termux lifecycle tooling.
- Local STT (`whisper.cpp`) remains optional but in-core.
- Optional modules are now treated as separate downloads via the wizard/plugin manager (details outside this repo).
