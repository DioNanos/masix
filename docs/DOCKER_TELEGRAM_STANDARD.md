# Docker Standard: Telegram Assistant

This setup runs MasiX as a Telegram-only assistant with:
- admin power,
- user soft policy (no runtime tools by default),
- auto-registration for new private users,
- admin notification on first contact,
- persistent per-user memory.

## 1) Prepare environment

```bash
cp docker/env/.env.example docker/env/.env
```

Edit `docker/env/.env`:
- set `MASIX_TELEGRAM_BOT_TOKEN`,
- set `MASIX_ADMIN_IDS`,
- set at least one `MASIX_PROVIDER_*_KEY`,
- set `MASIX_ACTIVE_PROVIDER` to one configured provider.

## 2) Start

```bash
docker compose -f docker-compose.standard.yml up -d --build
```

## 3) Verify

```bash
docker compose -f docker-compose.standard.yml ps
docker compose -f docker-compose.standard.yml logs -f --tail=100
```

## 4) Memory files

Editable files:
- `./docker/runtime/data/SOUL.md`
- `./docker/runtime/data/accounts/<bot_id>/MEMORY.md`
- `./docker/runtime/data/accounts/<bot_id>/memory/accounts/...` (per-user conversation state)

Registration state:
- `./docker/runtime/data/register/telegram_users.json`

## 5) Policy defaults

- `group_mode = "all"`
- `auto_register_users = true`
- `notify_admin_on_new_user = true`
- `user_tools_mode = "none"`
- `user_allowed_tools = []`

Admins can later grant user tools via Telegram:
- `/admin tools user mode selected`
- `/admin tools user allow <tool_name>`

## 6) Security defaults

- non-root container user,
- read-only root filesystem,
- no Linux capabilities,
- no-new-privileges,
- writable only `/data` and `/tmp`.

