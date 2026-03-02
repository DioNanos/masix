# Changelog

## 0.3.2 - 2026-03-02

- Release alignment with modules-server security hardening:
  - global registration flow is now served by `@MasiX_Register_BOT`
  - per-endpoint API rate-limits enforced server-side
  - artifact checksum/size verification enforced server-side before download
  - catalog served in minimal mode with per-platform integrity metadata
- Package metadata normalized to canonical repository URL (`DioNanos/MasiX`).

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
