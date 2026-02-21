# Changelog

All notable changes to this project will be documented in this file.

## [0.1.1-rc] - 2026-02-21

### Added
- **Provider dedupe and upsert safety**
  - Wizard and provider CLI now update in place instead of creating duplicate provider entries.
  - Duplicate target validation by `provider_type + base_url + model`.
- **z.ai endpoint switch in wizard**
  - Interactive choice between standard and coding endpoint.
- **Configurable fallback chain and vision provider**
  - Bot profile chain configuration from wizard.
  - New optional `vision_provider` per profile for media analysis routing.
- **Vision media analysis pipeline**
  - Telegram media metadata ingestion.
  - Optional dedicated vision endpoint call on inbound media.
  - Vision analysis is injected into main model context.
- **Cron tool exposure**
  - `cron` is now available as tool call, sharing runtime logic with `/cron`.
- **Android intent module/tool**
  - New `masix-intent` crate.
  - Built-in `intent` tool to dispatch Android intents through `am`.
- **WhatsApp read-only listener**
  - Rust adapter with schema version checks, sender allowlist, message size guard.
  - Optional shared-secret ingress verification.
  - Optional forwarding of summarized output to Telegram.
- **SMS runtime watcher**
  - Runtime ingestion from Termux SMS.
  - Optional forwarding of summarized output to Telegram.

### Changed
- `masix test provider` now respects `provider_type` and uses native Anthropic health checks when configured.
- Anthropic health check now probes `/v1/models` with proper headers.
- Project status moved from beta to `0.1.1-rc`.

### Fixed
- Cron due-job execution now warns on invalid non-numeric recipients instead of silent skip.
- Tool and profile validation tightened for startup safety.

## [0.1.0-beta.10] - 2026-02-20

### Added
- **Native Anthropic Provider**: Full support for Claude API with native format
  - Uses `/v1/messages` endpoint with `x-api-key` header
  - Proper message format conversion (system as separate param, content blocks)
  - Tool calling support with `tool_use` and `tool_result` blocks
- **Provider Type System**: Config now supports `provider_type` field
  - `openai` - OpenAI-compatible APIs (default)
  - `anthropic` - Native Anthropic/Claude API
- **Custom Provider Support**: Add any OpenAI-compatible or Anthropic API endpoint

### Changed
- Provider config now includes optional `provider_type` field
- `masix config providers list` shows provider type
- Anthropic provider correctly uses `claude-3-5-sonnet-latest` as default model

## [0.1.0-beta.9] - 2026-02-20

### Added
- **Provider Management CLI**: Full provider configuration from command line
  - `masix config providers list` - List all configured providers
  - `masix config providers add <name> --key <api-key>` - Add new provider
  - `masix config providers set-default <name>` - Set default provider
  - `masix config providers model <name> <model>` - Change model
  - `masix config providers remove <name>` - Remove provider
- **MCP Management CLI**: Manage MCP servers from command line
  - `masix config mcp list` - List MCP servers
  - `masix config mcp add <name> <command> [args...]` - Add MCP server
  - `masix config mcp remove <name>` - Remove MCP server
  - `masix config mcp enable/disable` - Toggle MCP
- **Chat Commands for Runtime Config**:
  - `/provider` - Show/change provider for current chat
  - `/model` - Show/change model for current chat
  - `/mcp` - Show MCP status
- **Extended Provider Support**: 14 LLM providers now supported
  - OpenAI, OpenRouter, z.ai (GLM), Chutes
  - xAI (Grok), Groq, Anthropic (Claude), Gemini
  - DeepSeek, Mistral, Together AI, Fireworks, Cohere
  - llama.cpp (local inference)
- **Config Wizard Updated**: Interactive setup now shows all 14 providers

### Changed
- Renamed "ollama" provider to "llama.cpp" for clarity
- Updated `/help` and `/` command list with new commands
- README expanded with provider table and CLI examples

## [0.1.0-beta.8] - 2026-02-20

### Added
- **Built-in Tools**: Tools now visible to LLM as function calls
  - `exec` - Execute shell commands (pwd, ls, df, etc.)
  - `termux` - Termux-specific commands (battery, location, wifi)
  - `read_file` - Read file contents from workdir
  - `write_file` - Write content to files
  - `list_dir` - List directory contents
  - `web_search` - Search the web using DuckDuckGo
  - `web_fetch` - Fetch and extract text from web pages
  - `device_info` - Get device info (battery, memory, storage, uptime)
- **WASM Support**: Added `masix-wasm-tools` crate for sandboxed WASM tool execution
- **Command List**: Type `/` to see available commands
- **Smart Provider Fallback**: 3 attempts per provider, then rotate to fallback
- **Typing Indicator**: Shows "typing..." while processing messages
- **Logging System**: Daily log rotation with 7-day retention

### Changed
- Menu system fully multilingual (en, es, zh, ru, it)
- `/new` command resets conversation session
- `/language` command changes language preference
- Removed `default_policy = "mention_only"` for better bot interaction

### Fixed
- Log files now properly written with auto-flush
- Menu inline keyboards now display correctly

## [0.1.0-beta.7] - 2026-02-19

### Added
- Initial release
- Telegram bot support
- MCP (Model Context Protocol) integration
- Multiple LLM providers (chutes, z.ai, openai, ollama)
- Cron-based reminders
- Termux integration
- WhatsApp and SMS support (basic)
