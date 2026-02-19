# @mmmbuto/masix

Scoped Termux package for MasiX.

MasiX is a Rust-first messaging automation runtime inspired by OpenClaw, focused on stable mobile execution.

## Function Summary

- Telegram bot automation with interactive inline menus
- Real MCP tool-calling flow through OpenAI-compatible providers
- Natural-language reminder scheduling (cron persistence)
- Optional WhatsApp and SMS integrations
- SOUL.md startup memory context

## Install (Termux)

```bash
pkg update -y
pkg install -y rust nodejs-lts termux-api
npm install -g @mmmbuto/masix@latest
masix --help
```

## Quick Start

```bash
masix config init
# edit ~/.config/masix/config.toml
masix start
```

## Useful Commands

```bash
masix start
masix config show
masix cron add 'domani alle 9 "Daily check"'
masix cron list
masix cron cancel 1
masix stats
```

## Notes

- This package targets Android + arm64 (Termux environments)
- If no prebuilt binary is available, postinstall builds from source

## Full Documentation

- Repository README: https://github.com/DioNanos/masix
- Detailed guide: https://github.com/DioNanos/masix/blob/main/docs/USER_GUIDE.md
- Local llama.cpp endpoint guide: https://github.com/DioNanos/masix/blob/main/docs/TERMUX_LLAMA_CPP_LOCAL_ENDPOINT.md

## License

MIT - See `../../LICENSE`
