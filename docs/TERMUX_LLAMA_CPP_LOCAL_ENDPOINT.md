# Termux Local AI Endpoint (llama.cpp)

MasiX supports local AI endpoints on smartphone through optimized Termux builds.

## Packages

- `@mmmbuto/llama-cpp-termux-snapdragon`
- `@mmmbuto/llama-cpp-termux-tensor`

NPM links:
- https://www.npmjs.com/package/@mmmbuto/llama-cpp-termux-snapdragon
- https://www.npmjs.com/package/@mmmbuto/llama-cpp-termux-tensor

## Install

```bash
# Snapdragon devices
npm install -g @mmmbuto/llama-cpp-termux-snapdragon

# Pixel/Tensor devices
npm install -g @mmmbuto/llama-cpp-termux-tensor
```

## Configure MasiX provider

```toml
[providers]
default_provider = "llama_local"

[[providers.providers]]
name = "llama_local"
api_key = "not-needed"
base_url = "http://127.0.0.1:8080/v1"
model = "local-model"
```
