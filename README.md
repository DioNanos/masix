# MasiX

[![Status: Beta](https://img.shields.io/badge/Status-Early%20Beta-blue.svg)](#project-status)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Target](https://img.shields.io/badge/Target-Termux%20%2F%20Linux-green.svg)](https://termux.dev)
[![npm](https://img.shields.io/npm/v/@mmmbuto/masix?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/masix)
[![ko-fi](https://img.shields.io/badge/☕_Support-Ko--fi-FF5E5B?style=flat-square&logo=ko-fi)](https://ko-fi.com/dionanos)

MasiX is a Rust-first automation runtime focused on Termux/Linux mobile workflows, inspired by OpenClaw.

## Project Status

- Early beta (study project)
- Extracted from a commercial product and simplified into a public testable core
- Primary focus: Telegram, MCP tool-calling, reminders, and stable Termux runtime

## Core Features

- Telegram long polling with persisted offsets and inline-menu callbacks
- Tool-calling pipeline: LLM -> MCP execution -> final LLM response
- OpenAI-compatible provider layer
- SQLite persistence for reminders/cron and offsets
- SOUL.md startup context support
- NPM package: `@mmmbuto/masix`

## Quick Install

```bash
npm install -g @mmmbuto/masix@latest
masix --help
masix config init
```

## Build From Source

```bash
git clone https://github.com/DioNanos/masix.git
cd MasiX

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

## Repository Layout

```text
crates/masix-core        runtime orchestration
crates/masix-telegram    telegram adapter + menus
crates/masix-storage     sqlite persistence + cron storage
crates/masix-providers   openai-compatible providers + tool calling
crates/masix-mcp         mcp client
crates/masix-cli         cli entrypoint
npm/masix-termux         termux package
docs/USER_GUIDE.md       detailed command and feature guide
```

## License

MIT License — Copyright (c) 2026 Davide A. Guglielmi
