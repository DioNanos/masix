# MasiX Packages (MIT)

This folder contains MIT-distributable plugin packages that can be installed directly from local `.pkg` files.

## Current Scope

- Included now: `discovery` (free, MIT)
- Not bundled now: `codex-*` packages

`codex` remains available via server flow with owner-controlled registration/auth policy.

## Local Install (No Server)

```bash
masix plugin install-file \
  --file /absolute/path/to/discovery-<platform>.pkg \
  --plugin discovery \
  --version 0.2.3 \
  --package-type mcp_binary
```

Supported package layout in this repository:

- `packages/free/discovery/0.2.3/`
  - `discovery-android-aarch64-termux.pkg`
  - `discovery-linux-x86_64.pkg`
  - `manifest.json`
  - `SHA256SUMS`
