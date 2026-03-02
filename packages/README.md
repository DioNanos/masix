# MasiX Packages (MIT)

This folder contains MIT-distributable plugin packages that can be installed directly from local `.pkg` files.

## Current Scope

- Included now (free, MIT):
  - `discovery` (`mcp_binary`)
  - `codex-backend` (`library`, admin-only)
  - `codex-tools` (`mcp_binary`, admin-only)

## Local Install (No Server)

```bash
masix plugin install-file \
  --file /absolute/path/to/<module>-<platform>.pkg \
  --plugin <plugin-id> \
  --version <version> \
  --package-type <mcp_binary|library>
```

Supported package layout in this repository:

- `packages/free/discovery/0.2.3/`
  - `discovery-android-aarch64-termux.pkg`
  - `discovery-linux-x86_64.pkg`
  - `discovery-macos-aarch64.pkg`
  - `manifest.json`
  - `SHA256SUMS`
- `packages/free/codex-backend/0.1.3/`
  - `codex-backend-android-aarch64-termux.pkg`
  - `codex-backend-linux-x86_64.pkg`
  - `codex-backend-macos-aarch64.pkg`
  - `manifest.json`
  - `SHA256SUMS`
- `packages/free/codex-tools/0.1.2/`
  - `codex-tools-android-aarch64-termux.pkg`
  - `codex-tools-linux-x86_64.pkg`
  - `codex-tools-macos-aarch64.pkg`
  - `manifest.json`
  - `SHA256SUMS`
