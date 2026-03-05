# codex-backend (free module)

`codex-backend` provides the coding backend runtime used by codex tools.

## What It Is

- Package type: `library`
- Visibility: `free`
- Admin-only: `true`
- Distribution: local `.pkg` install supported (`install-file`)

## Install Example

```bash
masix plugin install-file \
  --file packages/free/codex-backend/0.1.4/codex-backend-linux-x86_64.pkg \
  --plugin codex-backend \
  --version 0.1.4 \
  --package-type library
```

Then enable and restart:

```bash
masix plugin enable codex-backend
masix restart
```

## Notes

- `codex-backend` is typically used together with `codex-tools`.
- Upstream attribution for Codex-derived behavior is documented in `docs/THIRD_PARTY_NOTICES.md`.
