# Changelog

All notable changes to this project will be documented in this file.

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
