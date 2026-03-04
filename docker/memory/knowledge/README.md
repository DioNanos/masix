Drop extra `.md` knowledge files in:

`./docker/runtime/data/memory/custom/`

On first container bootstrap, files are appended to the account `MEMORY.md`.

After first run:
- edit `./docker/runtime/data/accounts/<bot_id>/MEMORY.md` directly for immediate effect,
- or keep maintaining files in `memory/custom` and merge manually when needed.

