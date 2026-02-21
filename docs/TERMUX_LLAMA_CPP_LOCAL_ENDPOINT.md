# Termux Local Endpoint with Optimized llama.cpp

This guide shows how to run a local OpenAI-compatible endpoint on Android Termux using the optimized npm packages:

- `@mmmbuto/llama-cpp-termux-snapdragon`
- `@mmmbuto/llama-cpp-termux-tensor`

## 1. Choose the Package

- Use `@mmmbuto/llama-cpp-termux-snapdragon` for Qualcomm Snapdragon devices (safe default).
- Use `@mmmbuto/llama-cpp-termux-tensor` for Google Pixel / Tensor devices.

## 2. Prerequisites (Termux)

```bash
pkg update -y
pkg upgrade -y
pkg install -y nodejs-lts openssl curl
```

## 3. Install One Build

### Snapdragon

```bash
npm install -g @mmmbuto/llama-cpp-termux-snapdragon
```

### Tensor / Pixel

```bash
npm install -g @mmmbuto/llama-cpp-termux-tensor
```

## 4. Verify Binaries

### Snapdragon

```bash
llama-cli-snapdragon --version
llama-server-snapdragon -h
```

### Tensor

```bash
llama-cli-tensor --version
llama-server-tensor -h
```

## 5. Start a Local Endpoint

Pick one command depending on installed package and your model path.

### Snapdragon

```bash
llama-server-snapdragon \
  -m /data/data/com.termux/files/home/models/model.gguf \
  --host 127.0.0.1 \
  --port 8080 \
  -c 4096
```

### Tensor

```bash
llama-server-tensor \
  -m /data/data/com.termux/files/home/models/model.gguf \
  --host 127.0.0.1 \
  --port 8080 \
  -c 4096
```

Endpoint base URL:

- `http://127.0.0.1:8080/v1`

## 6. Quick Endpoint Test

```bash
curl http://127.0.0.1:8080/v1/models
```

Chat test:

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "local-model",
    "messages": [{"role":"user","content":"Hello from Termux"}]
  }'
```

## 7. Connect MasiX to Local Endpoint

In your MasiX config (`~/.config/masix/config.toml`), add a local provider:

```toml
[providers]
default_provider = "llama_local"

[[providers.providers]]
name = "llama_local"
api_key = "not-needed"
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"
```

Then run:

```bash
masix start
```

## 8. Notes

- Keep the llama server process running while MasiX is active.
- Start with `-c 2048` or `-c 4096` to avoid memory pressure.
- Use Snapdragon build if you are unsure which CPU profile to use.

## 9. Recommended Fallback Chain in MasiX

You can keep local llama.cpp as primary and cloud fallback as backup:

```toml
[providers]
default_provider = "openrouter"

[[providers.providers]]
name = "llama_local"
api_key = "not-needed"
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"

[[providers.providers]]
name = "openrouter"
api_key = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
model = "openrouter/auto"

[bots]
strict_account_profile_mapping = false

[[bots.profiles]]
name = "termux_bot"
workdir = "~/.masix/bots/termux_bot"
memory_file = "~/.masix/bots/termux_bot/MEMORY.md"
provider_primary = "llama_local"
provider_fallback = ["openrouter"]
```

## 10. Optional Boot Automation (MasiX Runtime)

If you want MasiX to auto-start at Android boot:

```bash
masix termux boot enable
masix termux boot status
```

This writes `~/.termux/boot/masix` and requires the Termux:Boot app.
