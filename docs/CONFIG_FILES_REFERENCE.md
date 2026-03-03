# Config Files Reference (MIT Minimal)

This is the minimal file map for operating MasiX safely.

Power-mode sanitized reference:
- `docs/CONFIG_EXAMPLE_POWER.md`

## 1) Main Config

- Default: `~/.config/masix/config.toml`
- Override: `masix --config /absolute/path/config.toml ...`
- Bootstrap: `masix config init`

Core runtime sections (optional but supported):
- `[core.agent_loop]`
- `[core.tool_progress]`
- `[core.streaming]`
- `[core.cron]`

Streaming quick example (Telegram progressive output):

```toml
[core.streaming]
enabled = true
mode = "telegram_draft" # off | telegram_draft | telegram_chunked
flush_interval_ms = 900
max_message_edits = 20
finalize_timeout_secs = 10
```

Notes:
- `telegram_draft` uses Telegram Bot API `sendMessageDraft` for private chats.
- `telegram_chunked` is the portable fallback for non-private chats.
- `telegram_edit` remains accepted for backward compatibility, but is deprecated.

MCP server timeout controls:
- `timeout_secs`
- `startup_timeout_secs`
- `healthcheck_interval_secs`

## 2) Runtime Data Root

- Default: `~/.masix`
- Override in config: `[core].data_dir`

## 3) Runtime Files (under data_dir)

- `masix.pid` (daemon pid)
- `masix.db` (sqlite runtime storage)
- `logs/*.log` (runtime logs)
- `logs/cron_dead_letter.jsonl` (failed cron dispatch events)
- `logs/runtime_events.jsonl` (runtime response events)

## 4) Module Files (under data_dir/plugins)

- `auth.json` (device/module auth state)
- `installed.json` (installed modules registry)
- `packages/<plugin>/<version>/*.pkg` (local module artifacts)

## 5) Operational Rule

Prefer CLI commands to change state. Do not edit generated runtime files manually unless strictly required.
