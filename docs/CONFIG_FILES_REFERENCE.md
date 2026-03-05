# Config Files Reference (MIT Minimal)

This is the minimal file map for operating MasiX safely.

Power-mode sanitized reference:
- `docs/CONFIG_EXAMPLE_POWER.md`
- Docker Telegram standard:
- `docs/DOCKER_TELEGRAM_STANDARD.md`

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
mode = "telegram_chunked" # off | telegram_edit | telegram_chunked
flush_interval_ms = 900
max_message_edits = 20
finalize_timeout_secs = 10
```

Notes:
- `telegram_edit` updates a single message progressively (legacy-compatible mode).
- `telegram_chunked` emits chunked responses and is the safe default.
- Streaming is runtime-scoped (DM or tagged group, based on policy).

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

## 6) Telegram Registration Controls

For each `[[telegram.accounts]]`:
- `access_mode = "assistant_autoregister"` enables private-chat auto-registration flow.
- `dm_policy` and `group_policy` define baseline gate behavior.
- `group_require_mention` and `group_allow_known_untagged` refine group handling.
- `notify_admin_on_new_user` notifies admins on first auto-registration event.
- `new_user_welcome_message` sends a one-time welcome text to newly registered users.
- `start_welcome_admin` customizes `/start` response for admins.
- `start_welcome_user` customizes `/start` response for users/readonly.

CLI non-interactive update example:

```bash
masix config telegram \
  --account <account_tag> \
  --start-welcome-admin "Admin ready." \
  --start-welcome-user "Welcome."
```

## 7) MasiX Assistant Modules (Optional)

Recommended module wiring (via MCP servers + provider endpoint):
- `markai-assistant-kb-bridge` as MCP server for scoped memory/task/customer tools.
- `masix-assistant-orchestrator` as MCP server for playbook/state-machine actions.
- `masix-assistant-endpoint` as OpenAI-compatible HTTP provider endpoint.
- `masix-whatsapp-business` as channel adapter for WhatsApp Business Cloud.

Suggested env keys:
- `MASIX_ASSISTANT_KB_CORE_URL`
- `MASIX_ASSISTANT_KB_CORE_SHARED_KEY`
- `MASIX_ASSISTANT_KB_POLICY_DEFAULT`
- `MASIX_ASSISTANT_KB_POLICY_MAP`
- `MASIX_ASSISTANT_UPSTREAM_BASE_URL`
- `MASIX_ASSISTANT_UPSTREAM_API_KEY`
- `MASIX_ASSISTANT_ENDPOINT_TOKEN` or `MASIX_ASSISTANT_ENDPOINT_TOKENS`
