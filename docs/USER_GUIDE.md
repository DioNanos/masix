# MasiX User Guide

This guide covers setup, commands, runtime behavior, and operational workflows.

Last updated: 2026-02-19

## 1. Installation

### 1.1 Termux (Android)

```bash
pkg update -y
pkg install -y rust nodejs-lts termux-api git

git clone https://github.com/DioNanos/masix.git
cd masix

./scripts/build_termux.sh
masix --help
```

### 1.2 Linux (Development)

```bash
git clone https://github.com/DioNanos/masix.git
cd masix

cargo build --release
./target/release/masix --help
```

### 1.3 NPM Package (Termux)

Build package from repo:

```bash
cd npm/masix-termux
npm pack
```

Install on Termux device:

```bash
npm install -g @mmmbuto/masix
```

Install from local packed tarball:

```bash
npm install -g mmmbuto-masix-<version>.tgz
masix --help
```

## 2. Configuration

Initialize a starter config:

```bash
masix config init
```

Default location:

- `~/.config/masix/config.toml`

Reference example:

- `config/config.example.toml`

### 2.1 Minimal config example

```toml
[core]
data_dir = "~/.masix"
log_level = "info"
soul_file = "~/.masix/SOUL.md"

[telegram]
poll_timeout_secs = 60
client_recreate_interval_secs = 60

[[telegram.accounts]]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"
# bot_profile = "ops_bot"

[mcp]
enabled = true

[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/data/data/com.termux/files/home"]

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "YOUR_API_KEY"
model = "gpt-4o-mini"
```

### 2.2 Multiple endpoints and providers

MasiX supports multiple OpenAI-compatible providers at the same time.

- Define one entry per provider in `[[providers.providers]]`
- Set `base_url` for non-default OpenAI endpoints
- Select active provider with `[providers].default_provider`
- Restart runtime after config changes

Example with OpenAI, OpenRouter, llama.cpp, z.ai, and chutes.ai:

```toml
[providers]
default_provider = "openrouter"

[[providers.providers]]
name = "openai"
api_key = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[[providers.providers]]
name = "openrouter"
api_key = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
model = "openai/gpt-4o-mini"

[[providers.providers]]
name = "llama_local"
api_key = "not-needed"
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"

[[providers.providers]]
name = "zai"
api_key = "ZAI_API_KEY"
base_url = "https://api.z.ai/api/paas/v4"
model = "glm-4.5"

[[providers.providers]]
name = "chutes"
api_key = "CHUTES_API_KEY"
base_url = "https://llm.chutes.ai/v1"
model = "zai-org/GLM-5-TEE"
```

Switch active endpoint/provider:

```toml
[providers]
default_provider = "llama_local"
```

Notes:

- Runtime selection rules:
  - If a Telegram account is mapped to `bots.profiles`, runtime uses that profile chain (`provider_primary` + `provider_fallback`).
  - If no profile is mapped, runtime falls back to `[providers].default_provider`.
- If your z.ai account/model requires coding endpoint, use:
  - `https://api.z.ai/api/coding/paas/v4`
- For chutes.ai, verify your account docs if your tenant uses a different base URL.
- Provider docs:
  - OpenAI: `https://platform.openai.com/docs/api-reference`
  - OpenRouter: `https://openrouter.ai/docs`
  - z.ai: `https://docs.z.ai/`
  - chutes.ai: `https://docs.chutes.ai/`

### 2.3 Per-bot model selection and fallback

Use `bots.profiles` to assign model chains per bot and isolate workdir/memory.

Example:

```toml
[telegram]
poll_timeout_secs = 60
client_recreate_interval_secs = 60

[[telegram.accounts]]
bot_token = "BOT_A_TOKEN"
allowed_chats = [111111111]
bot_profile = "ops_bot"

[[telegram.accounts]]
bot_token = "BOT_B_TOKEN"
allowed_chats = [222222222]
bot_profile = "sales_bot"

[bots]
strict_account_profile_mapping = true

[[bots.profiles]]
name = "ops_bot"
workdir = "~/.masix/bots/ops_bot"
memory_file = "~/.masix/bots/ops_bot/MEMORY.md"
provider_primary = "openrouter"
provider_fallback = ["zai", "openai", "llama_local"]

[bots.profiles.retry]
window_secs = 600
initial_delay_secs = 2
backoff_factor = 2
max_delay_secs = 30

[[bots.profiles]]
name = "sales_bot"
workdir = "~/.masix/bots/sales_bot"
memory_file = "~/.masix/bots/sales_bot/MEMORY.md"
provider_primary = "openai"
provider_fallback = ["openrouter"]
```

Notes:

- Each `telegram.accounts[].bot_profile` must exist in `bots.profiles`.
- `provider_primary` and each fallback provider must exist in `providers.providers`.
- `strict_account_profile_mapping = true` enforces profile mapping for every Telegram account.

### 2.4 Secret handling

- `masix config show` prints a redacted view (`***REDACTED***`)
- Do not commit real keys/tokens to git

### 2.5 Exec module (guarded command execution)

`exec` is disabled by default and must be explicitly enabled.

```toml
[exec]
enabled = true
allow_base = true
allow_termux = true
timeout_secs = 15
max_output_chars = 3500
base_allowlist = ["pwd", "ls", "date", "uname", "df", "free"]
termux_allowlist = ["termux-info", "termux-battery-status", "termux-location"]
```

Notes:

- Commands run in the bot profile `workdir`
- Only allowlisted binaries can run
- Absolute paths, `..`, and unsafe shell characters are blocked

## 3. CLI Commands

## 3.1 Runtime

```bash
masix start
```

Starts the full runtime (adapters, inbound processing, provider routing, tool-calling, outbound responses).

When `bots.profiles` are configured:

- runtime resolves bot context from Telegram account (`account_tag`)
- each bot uses primary provider plus configured fallback chain
- each bot stores memory files in its own workdir

## 3.2 Telegram

```bash
masix telegram start
masix telegram test
```

- `start`: runs Telegram adapter loop
- `test`: reserved command path for connectivity checks

## 3.3 WhatsApp

```bash
masix whatsapp start
masix whatsapp login
```

- Transport depends on Node-based bridge (`whatsapp-web.js` workflow)

## 3.4 SMS

```bash
masix sms list --limit 20
masix sms send --to +391234567890 --text "Hello"
masix sms calls --limit 20
```

- Requires Termux + Termux:API

## 3.5 Cron / Reminders

```bash
masix cron add 'domani alle 9 "Team sync"'
masix cron list
masix cron cancel 1
```

- `add`: parses natural language and stores the schedule
- `list`: shows enabled jobs
- `cancel`: disables a job by ID
- Jobs now carry an `account_tag` scope to prevent cross-bot execution

Scoped examples:

```bash
masix cron add 'domani alle 9 "Ops check"' --account-tag 123456789
masix cron list --account-tag 123456789
masix cron list --account-tag 123456789 --recipient 111111111
masix cron cancel 42 --account-tag 123456789
```

## 3.6 Config

```bash
masix config init
masix config show
masix config validate
```

- `show`: redacted output for safe diagnostics
- `validate`: checks provider/profile mapping and config consistency

## 3.7 Stats / Version

```bash
masix stats
masix version
```

- `stats`: prints runtime metadata, provider counts, DB size, active cron jobs

## 3.8 Termux boot management

```bash
masix termux boot enable
masix termux boot disable
masix termux boot status
```

Notes:

- This manages `~/.termux/boot/masix`
- Requires Termux + Termux:Boot app
- `enable` writes an auto-start script for `masix start`

## 3.9 Cron scope behavior

- Reminder jobs are saved with `account_tag` scope.
- This prevents collisions when multiple Telegram bots share one DB.
- Runtime `/cron` commands are scoped to current bot + current chat.
- Legacy jobs without explicit scope are mapped to `__default__`.

## 4. Telegram Runtime Behavior

## 4.1 Interactive menus

Telegram chat commands:

- `/start`
- `/menu`

Menu sections:

- Home
- Reminder
- Utility
- Settings

Navigation uses callback queries and message editing when possible.

Runtime commands available in chat:

- `/cron ...`, `/cron list`, `/cron cancel <id>`
- `/exec <allowlisted-command>`
- `/termux info|battery|cmd <termux-command>|boot on|off|status`

## 4.2 Message handling flow

1. Adapter polls updates
2. Message/callback published to event bus
3. Core processes input
4. Core calls provider (with tools when MCP is available)
5. Outbound response is published
6. Telegram adapter sends or edits messages

## 4.3 Long message handling

- Automatic chunking for Telegram size limits
- Markdown first, plain-text fallback on formatting errors

## 5. Tool Calling and MCP

## 5.1 What happens during tool calls

1. Core fetches available MCP tools
2. Tools are passed to the LLM
3. LLM returns `tool_calls`
4. Core executes MCP tools
5. Tool outputs are fed back to the LLM
6. Final user response is sent

## 5.2 Tool naming convention

- Exposed as `server_tool` (example: `filesystem_read_file`)

## 5.3 Safety limit

- Max tool loop iterations: `5`

## 6. SOUL.md Memory

Set in config:

```toml
[core]
soul_file = "~/.masix/SOUL.md"
```

Runtime loads SOUL.md at startup and uses it in system context.

## 7. Data and Persistence

- Data directory default: `~/.masix`
- Database: `~/.masix/masix.db`
- Telegram offsets are persisted by bot/account
- Cron jobs persist across restarts

## 8. Build and Packaging

## 8.1 Rust release build

```bash
cargo build --release
```

Binary:

- `target/release/masix`

## 8.2 Termux script

```bash
./scripts/build_termux.sh
```

Script builds and installs `masix` into `$PREFIX/bin`.

## 8.3 NPM package checks

```bash
cd npm/masix-termux
npm pack
```

Package contains:

- `README.md`
- `install.js`
- `package.json`
- `prebuilt/masix`

Generated tarball pattern:

- `mmmbuto-masix-<version>.tgz`

## 9. Troubleshooting

## 9.1 Permission denied on data dir

- Ensure `data_dir` is writable
- Prefer `~/.masix` on user-owned environments

## 9.2 No answers in Telegram

- Verify bot token
- Verify provider API config
- Verify runtime started with `masix start`
- Verify MCP server commands are available if tool calls are expected

## 9.3 Tool calling not executed

- Check provider supports tool-calling in OpenAI-compatible API
- Check MCP servers are enabled and started
- Check runtime logs for `Executing tool: ...`
- If the model prints `### TOOL_CALL` text but no real `tool_calls` payload, verify provider compatibility and response format.

## 9.4 Cron jobs do not fire

- Confirm schedule parsing at creation time
- Confirm `masix start` process is running
- Check active jobs with `masix cron list`
- If running multiple bots, verify `account_tag` scope (`masix cron list --account-tag <tag>`).

## 9.5 `/exec` or `/termux` rejected

- Verify `[exec]` section is enabled in config.
- Verify command is in allowlist.
- Remember: absolute paths, `..`, and unsafe shell chars are blocked by design.

## 9.6 Termux boot not starting

- Verify Termux:Boot app is installed.
- Verify script exists at `~/.termux/boot/masix`.
- Check boot logs at `~/.masix/logs/boot.log`.

## 10. Operational Notes

- This project targets stable mobile operation first
- Keep runtime logs enabled during test phase
- Prefer small iterative config changes and validate with `masix stats`

## 11. Local Llama.cpp Endpoint (Termux)

For a dedicated setup guide with optimized Termux binaries, see:

- `docs/TERMUX_LLAMA_CPP_LOCAL_ENDPOINT.md`
