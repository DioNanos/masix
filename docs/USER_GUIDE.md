# User Guide (MIT Minimal)

This guide is intentionally minimal and MIT-safe.

## 1) Setup

Initialize config:

```bash
masix config init
masix config validate
```

Start runtime:

```bash
masix start
masix status
```

## 2) Minimal Config Example

```toml
[core]
data_dir = "~/.masix"
log_level = "info"

[telegram]
poll_timeout_secs = 60

[[telegram.accounts]]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"

[providers]
default_provider = "default_endpoint"

[[providers.providers]]
name = "default_endpoint"
api_key = "YOUR_API_KEY"
base_url = "https://your-endpoint.example/v1"
model = "your-model"
```

Use API-compatible endpoint(s). Avoid hardcoding private keys in git-tracked files.

## 3) Modules

Install from server:

```bash
masix plugin list --server <url> --platform <id>
masix plugin install <plugin> --server <url>
```

Install from local `.pkg` (offline):

```bash
masix plugin install-file \
  --file /absolute/path/module.pkg \
  --plugin <plugin-id> \
  --version <version> \
  --package-type mcp_binary
```

## 4) Cron Commands (CLI)

```bash
masix cron add "<schedule>" "<message>"
masix cron list
masix cron cancel <id>
```

## 5) Chat Commands

- `/start`
- `/menu`
- `/new`
- `/help`
- `/whoiam`
- `/provider`
- `/model`
- `/cron ...`
- `/plugin ...`
- `/mcp`
- `/tools`
