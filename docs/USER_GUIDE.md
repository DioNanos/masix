# MasiX User Guide

This guide covers setup, commands, runtime behavior, and operational workflows.

Last updated: 2026-02-24 (v0.2.5)

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
model = "gpt-5"
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
model = "gpt-5"

[[providers.providers]]
name = "openrouter"
api_key = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
model = "openrouter/auto"

[[providers.providers]]
name = "llama_local"
api_key = "not-needed"
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"

[[providers.providers]]
name = "zai"
api_key = "ZAI_API_KEY"
base_url = "https://api.z.ai/api/paas/v4"
model = "glm-5"

[[providers.providers]]
name = "chutes"
api_key = "CHUTES_API_KEY"
base_url = "https://llm.chutes.ai/v1"
model = "Qwen/Qwen3.5-397B-A17B-TEE"
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
bot_name = "OpsBot"
admins = [111111111]
users = [222222222, 333333333]
group_mode = "users_or_tag"
auto_register_users = false
bot_profile = "ops_bot"

[[telegram.accounts]]
bot_token = "BOT_B_TOKEN"
bot_name = "SalesBot"
admins = [444444444]
users = []
group_mode = "tag_only"
bot_profile = "sales_bot"

[bots]
strict_account_profile_mapping = true

[[bots.profiles]]
name = "ops_bot"
workdir = "~/.masix/bots/ops_bot"
memory_file = "~/.masix/bots/ops_bot/MEMORY.md"
provider_primary = "openrouter"
vision_provider = "gemini"
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
- `vision_provider` is optional and can point to a dedicated provider used only for inbound media analysis.
- `strict_account_profile_mapping = true` enforces profile mapping for every Telegram account.

### 2.3.1 Permission System

MasiX supports role-based access control:

**Roles:**
- **Admin**: Full access (tools, exec, user management)
- **User**: Basic chat access
- **Readonly**: Read-only access (future use)

**Group Modes:**
| Mode | Behavior |
|------|----------|
| `all` | Everyone can interact |
| `users_only` | Only listed users can interact |
| `tag_only` | Respond only when bot is tagged |
| `users_or_tag` | Listed users OR when tagged |
| `listen_only` | Listen only, respond when tagged by admin |

**Admin Commands:**
```
/admin list                    - Show merged static + dynamic ACL
/admin add <user_id>           - Add user immediately
/admin remove <user_id>        - Remove user immediately
/admin promote <user_id>       - Promote to admin immediately
/admin demote <user_id>        - Demote to user immediately
```

Wizard note:
- `masix config telegram` accepts numeric IDs and `@username` for admin/user lists.
- Username resolution uses Telegram `getChat`; if it fails, ask that user to write to the bot and then run `/whoiam`.

**Auto-Registration:**
When `group_mode = "all"` and `auto_register_users = true`, unknown users in private chats are auto-registered to `register_to_file` and permission is re-evaluated in the same message flow.

### 2.4 WhatsApp read-only listener

```toml
[whatsapp]
enabled = true
read_only = true
transport_path = "crates/masix-whatsapp/whatsapp-transport.js"
ingress_shared_secret = "CHANGE_ME"
allowed_senders = ["393471443005@c.us"]
max_message_chars = 4000
forward_to_telegram_chat_id = 111111111
forward_to_telegram_account_tag = "123456789"
forward_prefix = "WhatsApp Alert"
```

Notes:

- Only read-only mode is supported.
- Outbound replies on WhatsApp are intentionally disabled.
- Ingress schema is `whatsapp.v1`.
- If `ingress_shared_secret` is set, unsigned/invalid events are dropped.

### 2.5 SMS runtime watcher

```toml
[sms]
enabled = true
watch_interval_secs = 30
allowed_senders = ["+393471234567"] # optional; if omitted and users/admins empty => allow all
users = ["+393471111111"]            # optional
admins = ["+393470000000"]           # optional (required for runtime tool execution)
forward_to_telegram_chat_id = 111111111
forward_to_telegram_account_tag = "123456789"
forward_prefix = "SMS Alert"
```

Notes:

- Requires Termux + Termux:API permissions.
- Inbound SMS can be summarized by the LLM and forwarded to Telegram.
- Runtime tool execution from SMS channel is admin-only (via `sms.admins`).

### 2.6 Secret handling

- `masix config show` prints a redacted view (`***REDACTED***`)
- Do not commit real keys/tokens to git

### 2.7 Exec module (guarded command execution)

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

## 3.2 Diagnostics

```bash
masix verify              # Preflight checks (exit 0 = ok)
masix doctor              # Diagnostics with actionable hints
masix doctor --offline    # Skip network checks
masix verify --config /path/config.toml
```

- `verify`: Fast config and storage validation
- `doctor`: Full system check with suggestions
- Both commands validate/report the exact config path in use (including `--config`).

## 3.3 Telegram

```bash
masix telegram start
masix telegram test
```

- `start`: runs Telegram adapter loop
- `test`: reserved command path for connectivity checks

## 3.4 WhatsApp

```bash
masix whatsapp start
masix whatsapp login
```

- Current mode is **read-only listener**.
- Outbound replies on WhatsApp are intentionally disabled.
- Inbound events can be forwarded to Telegram (configured in `[whatsapp]`).
- Bridge input uses schema version `whatsapp.v1` and can be signed with a shared secret.

## 3.5 SMS

```bash
masix sms list --limit 20
masix sms send --to +391234567890 --text "Hello"
masix sms calls --limit 20
```

- Requires Termux + Termux:API
- Runtime watcher can ingest inbound SMS and forward LLM summaries to Telegram when configured in `[sms]`.

## 3.6 Cron / Reminders

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

## 3.7 Config

```bash
masix config init
masix config show
masix config validate
```

- `show`: redacted output for safe diagnostics
- `validate`: checks provider/profile mapping and config consistency

## 3.8 Stats / Version

```bash
masix stats
masix version
```

- `stats`: prints runtime metadata, provider counts, DB size, active cron jobs

## 3.9 Termux boot management

```bash
masix termux boot enable
masix termux boot disable
masix termux boot status
```

Notes:

- This manages `~/.termux/boot/masix`
- Requires Termux + Termux:Boot app
- `enable` writes an auto-start script for `masix start`

## 3.10 Cron scope behavior

- Reminder jobs are saved with `account_tag` scope.
- This prevents collisions when multiple Telegram bots share one DB.
- Runtime `/cron` commands are scoped to current bot + current chat.
- Legacy jobs without explicit scope are mapped to `__default__`.

## 3.11 Local STT wizard (auto model)

```bash
masix config stt
```

- Wizard can auto-detect RAM/CPU/arch and recommend a Whisper model size.
- It can auto-download the selected model into `~/.masix/models/whisper`.
- You can override both selected model and destination path.
- On Termux, install `ffmpeg` and provide/build `whisper-cli` separately.

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
- `/provider ...`, `/model ...`
- `/whoiam` (shows user/chat IDs, scope, and permission)

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

## 5.4 Built-in tool highlights

- `cron`: create/list/cancel reminders from tool calls.
- `vision`: returns media context and, if configured, vision analysis generated by `vision_provider`.
- `intent`: dispatch Android intents via `am` (`start`, `broadcast`, `service`).

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
