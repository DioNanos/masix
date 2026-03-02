# discovery (free module)

`discovery` is the first free package bundled in the MIT distribution path.

## What It Is

- Package type: `mcp_binary`
- Visibility: `free`
- Distribution: local `.pkg` file install supported (`install-file`)

## Install Example

Termux:

```bash
masix plugin install-file \
  --file packages/free/discovery/0.2.3/discovery-android-aarch64-termux.pkg \
  --plugin discovery \
  --version 0.2.3 \
  --package-type mcp_binary
```

Linux:

```bash
masix plugin install-file \
  --file packages/free/discovery/0.2.3/discovery-linux-x86_64.pkg \
  --plugin discovery \
  --version 0.2.3 \
  --package-type mcp_binary
```

macOS (Apple Silicon):

```bash
masix plugin install-file \
  --file packages/free/discovery/0.2.3/discovery-macos-aarch64.pkg \
  --plugin discovery \
  --version 0.2.3 \
  --package-type mcp_binary
```

After install, run:

```bash
masix plugin enable discovery
masix restart
```
