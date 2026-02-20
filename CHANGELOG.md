# Changelog

All notable changes to this project will be documented in this file.

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
