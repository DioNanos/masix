# MasiX

[![Status: RC](https://img.shields.io/badge/Status-0.1.6--rc-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Target](https://img.shields.io/badge/Target-Termux%20%2F%20Linux-green.svg)](https://termux.dev)
[![npm](https://img.shields.io/npm/v/@mmmbuto/masix?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/masix)
[![ko-fi](https://img.shields.io/badge/☕_Support-Ko--fi-FF5E5B?style=flat-square&logo=ko-fi)](https://ko-fi.com/dionanos)

MasiX is a Rust-first automation runtime focused on Termux/Linux mobile workflows, inspired by OpenClaw.

## Project Status

- 0.1.6 release candidate
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
- WhatsApp read-only listener mode with optional Telegram forwarding
- SMS runtime watcher with optional Telegram forwarding
- Android intent dispatch tool for Termux (`intent`)
- Runtime command/tool inventory via `/tools`
- Torrent search tool (`torrent_search`) for links and optional magnet extraction
- Telegram command menu auto-sync (`setMyCommands`) at adapter startup
- Account-scoped bot workdir isolation (`.../accounts/<account_tag>/...`)
- User-scoped memory isolation and catalog (`memory/accounts/<account>/users/<user>/meta.json`)
- Per-user runtime provider/model selection state (scoped by bot account)

## Quick Install

```bash
npm install -g @mmmbuto/masix@latest
masix --help
masix config init
```

## Quick Ops

```bash
masix start
masix config validate
masix cron add 'domani alle 9 "Daily check"' --account-tag 123456789
masix termux boot status
```

## Wizard Coverage

`masix config init` now configures in one pass:
- Telegram account basics
- LLM provider + fallback chain + `vision_provider`
- MCP enablement
- WhatsApp read-only listener (secret, allowlist, forwarding)
- SMS watcher (interval, forwarding)

## Provider Management (CLI)

```bash
# List configured providers
masix config providers list

# Add a new provider
masix config providers add xai --key sk-xxx --default
masix config providers add groq --key gsk-xxx --model llama-3.3-70b-versatile

# Change default provider or model
masix config providers set-default openai
masix config providers model openai gpt-4o

# Remove a provider
masix config providers remove ollama
```

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
| OpenAI | gpt-4o-mini | Official API |
| OpenRouter | openai/gpt-4o-mini | Multi-provider gateway |
| z.ai (GLM) | glm-4.5 | Chinese LLM |
| Chutes | zai-org/GLM-5-TEE | Decentralized |
| xAI (Grok) | grok-2-latest | Elon Musk's AI |
| Groq | llama-3.3-70b-versatile | Fast inference |
| Anthropic | claude-3-5-sonnet-latest | Claude |
| Gemini | gemini-2.0-flash | Google |
| DeepSeek | deepseek-chat | Chinese reasoning |
| Mistral | mistral-large-latest | European |
| Together AI | meta-llama/Llama-3-70b | Multi-model |
| Fireworks | llama-v3-70b-instruct | Fast inference |
| Cohere | command-r | Enterprise |
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
