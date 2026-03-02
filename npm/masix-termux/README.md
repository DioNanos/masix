# @mmmbuto/masix

Official Termux package for MasiX (MIT).

MasiX is an AI-friendly modular assistant runtime in Rust, optimized for smartphone (Termux/Android) and compatible with Linux/macOS.

## Function Summary

- Telegram bot automation with interactive inline menus
- Real MCP tool-calling flow through API-compatible endpoints
- Natural-language reminder scheduling (cron persistence)
- Cron scope isolation per bot/account (`account_tag`)
- Workdir isolation per Telegram account
- User memory isolation with per-user catalog (`meta.json`)
- Termux wake lock control (`masix termux wake on|off|status`)
- Guarded command execution (`/exec`, `/termux`) with allowlists
- Termux boot automation (`masix termux boot enable|disable|status`)
- Optional local STT via whisper.cpp (`masix config stt`)
- Optional modules can be installed from server catalog or local `.pkg`
- SOUL.md startup memory context
- Startup auto-update check/apply with configurable toggle in `config.toml` (`[updates]`)

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
masix config validate
masix start
masix status
```

## Useful Commands

```bash
masix start
masix config show
masix config validate
masix config stt
masix cron add 'domani alle 9 "Daily check"'
masix cron list
masix cron cancel 1
masix cron list --account-tag 123456789
masix termux boot status
masix stats
```

## Runtime Chat Commands

- `/cron ...`, `/cron list`, `/cron cancel <id>`
- `/exec <allowlisted-command>`
- `/termux ...`

## Notes

- This package targets Android + arm64 (Termux environments)
- If no prebuilt binary is available, postinstall builds from source
- `masix config stt` can auto-pick and auto-download a Whisper model based on device resources

## Full Documentation

- Repository README: https://github.com/DioNanos/MasiX
- Detailed guide: https://github.com/DioNanos/MasiX/blob/main/docs/USER_GUIDE.md
- Commands reference: https://github.com/DioNanos/MasiX/blob/main/docs/COMMANDS_REFERENCE.md
- Homebrew tap (Linux/macOS formula): https://github.com/DioNanos/homebrew-masix
- Local llama.cpp endpoint guide: https://github.com/DioNanos/MasiX/blob/main/docs/TERMUX_LLAMA_CPP_LOCAL_ENDPOINT.md

## Branding

<p>
Copyright (c) 2026 WellaNet.Dev<br>
Made in Italy 🇮🇹
</p>

## License

MIT - See `../../LICENSE`
