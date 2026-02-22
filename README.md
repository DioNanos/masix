# MasiX

[![Status](https://img.shields.io/badge/Status-0.2.1-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Target](https://img.shields.io/badge/Target-Termux%20%2F%20Linux%20%2F%20macOS-green.svg)](https://termux.dev)
[![npm](https://img.shields.io/npm/v/@mmmbuto/masix?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/masix)
[![ko-fi](https://img.shields.io/badge/☕_Support-Ko--fi-FF5E5B?style=flat-square&logo=ko-fi)](https://ko-fi.com/dionanos)

MasiX is a Rust-first automation runtime for Termux, Linux, and macOS workflows, inspired by OpenClaw.

## Project Status

- 0.2.1 stable release
- Extracted from a commercial product and simplified into a public testable core
- Primary focus: Telegram, MCP tool-calling, reminders, and stable Termux runtime

## Core Features

- Telegram long polling with persisted offsets and inline-menu callbacks
- Tool-calling pipeline: LLM -> MCP execution -> final LLM response
- OpenAI-compatible provider layer with 14+ providers (OpenAI, xAI/Grok, Groq, Anthropic, Gemini, DeepSeek, Mistral, etc.)
- Per-bot profiles (workdir, memory, primary model + fallback chain)
- SQLite persistence for reminders/cron and offsets
- Cron reminders scoped by Telegram bot/account to avoid cross-bot overlap
- Guarded exec module for base and Termux commands in bot workdir
- Termux boot management (`masix termux boot enable|disable|status`)
- Termux wake lock management (`masix termux wake on|off|status`)
- SOUL.md startup context support
- NPM package: `@mmmbuto/masix`
- Runtime provider/model switching via chat commands
- MCP server management via CLI
- Dedicated media vision provider per bot profile (`vision_provider`)
- Optional local STT (whisper.cpp) for Telegram voice/audio
- WhatsApp read-only listener mode with optional Telegram forwarding
- SMS runtime watcher with optional Telegram forwarding
- Android intent dispatch tool for Termux (`intent`)
- Runtime command/tool inventory via `/tools`
- Torrent tools split: `torrent_search` (links) + `torrent_extract_magnet` (on-demand magnet extraction)
- Telegram command menu auto-sync (`setMyCommands`) at adapter startup
- Account-scoped bot workdir isolation (`.../accounts/<account_tag>/...`)
- User-scoped memory isolation and catalog (`memory/accounts/<account>/users/<user>/meta.json`)
- Per-user runtime provider/model selection state (scoped by bot account)
- Startup auto-update check/apply with automatic process restart (configurable)

## Quick Install

```bash
# Termux (npm package)
npm install -g @mmmbuto/masix@latest

# Linux/macOS (Homebrew tap)
brew tap DioNanos/masix
brew install masix

masix --help
masix config init
```

## Platform Behavior

- Termux/Android: full feature set (Termux boot/wake, SMS watcher, intent tool).
- Linux/macOS: mobile-only features are automatically disabled. `masix termux boot enable|disable|status` auto-detects the platform and applies the best boot strategy:
  - Linux: systemd system service (no login) -> systemd user service + linger -> `~/.config/autostart` fallback
  - macOS: LaunchDaemon (no login) -> LaunchAgent fallback
- Update hint is platform-aware:
  - Termux: `npm install -g @mmmbuto/masix@latest`
  - Homebrew: `brew upgrade masix`

## Quick Ops

```bash
masix start
masix config validate
masix cron add 'domani alle 9 "Daily check"' --account-tag 123456789
masix termux boot status
```

## Startup Auto-Update

By default, `masix start` checks npm updates at startup, applies the update, and restarts the process.

Disable it in config:

```toml
[updates]
enabled = false
```

Fine-grained controls:

```toml
[updates]
enabled = true
check_on_start = true
auto_apply = true
restart_after_update = true
channel = "latest"
```

## Wizard Coverage

`masix config init` now configures in one pass:
- Telegram account basics
- LLM provider + fallback chain + `vision_provider`
- MCP enablement
- WhatsApp read-only listener (secret, allowlist, forwarding)
- SMS watcher (interval, forwarding)
- Local STT profile (`[stt]`: model path, binary, threads, language)

## Provider Management (CLI)

```bash
# List configured providers
masix config providers list

# Add a new provider
masix config providers add xai --key sk-xxx --default
masix config providers add groq --key gsk-xxx --model openai/gpt-oss-120b

# Change default provider or model
masix config providers set-default openai
masix config providers model openai gpt-5

# Remove a provider
masix config providers remove ollama

# Set dedicated vision provider (or auto fallback)
masix config providers vision gemini
masix config providers vision auto
```

## Local STT (whisper.cpp)

Configure local STT:

```bash
masix config stt
```

Required runtime dependencies:
- `whisper-cli` (whisper.cpp CLI)
- `ffmpeg` (required for Telegram voice/ogg-opus conversion)

When `[stt].enabled = true`, Telegram `voice`/`audio` media is transcribed locally and transcript context is appended to the LLM prompt.

## MCP Management (CLI)

```bash
# List MCP servers
masix config mcp list

# Add/remove MCP servers
masix config mcp add brave npx -y @modelcontextprotocol/server-brave-search
masix config mcp remove memory

# Enable/disable MCP
masix config mcp enable
masix config mcp disable
```

## Chat Commands

| Command | Description |
|---------|-------------|
| `/start` `/menu` | Show main menu |
| `/new` | Reset conversation session |
| `/help` | Show help |
| `/language` | Change language (EN/ES/ZH/RU/IT) |
| `/provider` | Show current provider |
| `/provider list` | List all providers |
| `/provider set <name>` | Switch provider for this user |
| `/model` | Show current model |
| `/model <name>` | Change model for this user |
| `/mcp` | Show MCP status |
| `/tools` | List runtime exposed tools (built-in + MCP) |
| `/cron` | Manage reminders |
| `/exec` | Run shell commands |
| `/termux` | Termux tools (`info`, `battery`, `boot`, `wake`) |

## Supported Providers

| Provider | Default Model | Notes |
|----------|---------------|-------|
| OpenAI | gpt-5 | Official API |
| OpenRouter | openrouter/auto | Multi-provider gateway |
| z.ai (GLM) | glm-5 | Chinese LLM |
| Chutes | Qwen/Qwen3.5-397B-A17B-TEE | Decentralized |
| xAI (Grok) | grok-4-latest | Elon Musk's AI |
| Groq | openai/gpt-oss-120b | Fast inference |
| Anthropic | claude-sonnet-4-6 | Claude |
| Gemini | gemini-2.5-pro | Google |
| DeepSeek | deepseek-reasoner | Chinese reasoning |
| Mistral | mistral-large-latest | European |
| Together AI | moonshotai/Kimi-K2.5 | Multi-model |
| Fireworks | llama-v3p1-70b-instruct | Fast inference |
| Cohere | command-a-03-2025 | Enterprise |
| llama.cpp | local-model | Local inference |

## Build From Source

```bash
git clone https://github.com/DioNanos/masix.git
cd masix

cargo build --release
./target/release/masix --help
```

## Local llama.cpp Packages (Termux)

```bash
npm install @mmmbuto/llama-cpp-termux-tensor
npm install @mmmbuto/llama-cpp-termux-snapdragon
```

- [@mmmbuto/llama-cpp-termux-tensor](https://www.npmjs.com/package/@mmmbuto/llama-cpp-termux-tensor)
- [@mmmbuto/llama-cpp-termux-snapdragon](https://www.npmjs.com/package/@mmmbuto/llama-cpp-termux-snapdragon)

For local endpoint setup:
- [docs/TERMUX_LLAMA_CPP_LOCAL_ENDPOINT.md](docs/TERMUX_LLAMA_CPP_LOCAL_ENDPOINT.md)

## Documentation

- [Main usage guide](docs/USER_GUIDE.md)
- [NPM package docs](npm/masix-termux/README.md)
- [Homebrew distribution](docs/HOMEBREW.md)
- [Example config](config/config.example.toml)
- [Termux llama.cpp local endpoint guide](docs/TERMUX_LLAMA_CPP_LOCAL_ENDPOINT.md)

## Repository Layout

```text
crates/masix-core        runtime orchestration
crates/masix-telegram    telegram adapter + menus
crates/masix-whatsapp    read-only WhatsApp listener adapter
crates/masix-sms         Termux SMS/call integration
crates/masix-intent      Android intent module (`am` wrapper)
crates/masix-storage     sqlite persistence + cron storage
crates/masix-providers   openai-compatible providers + tool calling
crates/masix-mcp         mcp client
crates/masix-cli         cli entrypoint
npm/masix-termux         termux package
docs/USER_GUIDE.md       detailed command and feature guide
```

## License

MIT License — Copyright (c) 2026 Davide A. Guglielmi
