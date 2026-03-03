# Commands Reference (MIT Minimal)

Minimal command map for operators and AI workers.

## 1) CLI (terminal)

Runtime:

```bash
masix start
masix stop
masix status
masix restart
masix verify
masix doctor --offline
```

Config:

```bash
masix config init
masix config validate
```

AI automation:

```bash
masix ai contract --json
masix ai commands --json
masix ai status --json
```

Modules from server:

```bash
masix plugin list --server <url> --platform <id>
masix plugin install <plugin> --server <url>
masix plugin enable <plugin>
masix plugin disable <plugin>
```

Modules from local package (offline):

```bash
masix plugin install-file \
  --file /absolute/path/module.pkg \
  --plugin <plugin-id> \
  --version <version> \
  --package-type mcp_binary
```

## 2) Chat (`/...`)

Base:
- `/start`
- `/menu`
- `/new`
- `/help`
- `/whoiam`

Provider/model:
- `/provider`
- `/provider list`
- `/provider set <name>`
- `/model <name>`
- `/model reset`

Reminders:
- `/cron ...`
- `/cron list`
- `/cron cancel <id>`

Cron CLI subcommands (verified):

- `masix cron add "<schedule>" "<message>"`
- `masix cron list`
- `masix cron cancel <id>`

Admin/runtime:
- `/admin ...`
- `/groups`
- `/admin groups`
- `/admin groups refresh`
- `/plugin ...`
- `/mcp`
- `/tools`
- `/exec <allowlisted-command>`

AI/runtime context:
- `chat_context` (builtin tool; exposed to tool-calling runtime)
