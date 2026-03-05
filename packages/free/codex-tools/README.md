# codex-tools (free module)

`codex-tools` exposes MCP tools for coding workflows and patch operations.

## What It Is

- Package type: `mcp_binary`
- Visibility: `free`
- Admin-only: `true`
- Distribution: local `.pkg` install supported (`install-file`)

## Install Example

```bash
masix plugin install-file \
  --file packages/free/codex-tools/0.1.3/codex-tools-linux-x86_64.pkg \
  --plugin codex-tools \
  --version 0.1.3 \
  --package-type mcp_binary
```

Then enable and restart:

```bash
masix plugin enable codex-tools
masix restart
```

## Notes

- For full coding flow, install `codex-backend` and `codex-tools` together.
- Tools are filtered by admin policy at runtime.
