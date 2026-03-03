# Config Example — Power Mode (Sanitized)

Reference configuration for advanced operators.
This file is intentionally sanitized: only placeholders, no real IDs, no real keys, no host-specific paths.

```toml
[core]
data_dir = "~/.masix"
log_level = "info"
soul_file = "~/.masix/bots/YOUR_BOT_NAME/memory/SOUL.md"
global_memory_file = "~/.masix/bots/YOUR_BOT_NAME/memory/MEMORY.md"

[core.agent_loop]
auto_continue_enabled = true
auto_continue_max = 3
continuation_detection = "heuristic_v1"
# max_tool_iterations = 25

[core.tool_progress]
enabled = true
mode = "first_round" # first_round | periodic | milestones
interval_secs = 8
max_updates = 5
include_tool_names = true

[core.streaming]
enabled = false
mode = "off" # off | telegram_edit | telegram_chunked
flush_interval_ms = 900
max_message_edits = 20
finalize_timeout_secs = 10

[core.cron]
dispatch_concurrency = 2
delivery_retry_count = 2
delivery_retry_backoff_secs = 10
dead_letter_enabled = true

[updates]
enabled = true
check_on_start = true
auto_apply = true
restart_after_update = true
channel = "latest"

[telegram]
poll_timeout_secs = 60
client_recreate_interval_secs = 60

[[telegram.accounts]]
bot_token = "YOUR_TELEGRAM_BOT_TOKEN"
bot_name = "YOUR_BOT_NAME"
bot_profile = "default"
admins = [123456789]
users = []
readonly = []
isolated = true
shared_memory_with = []
allow_self_memory_edit = true
group_mode = "tag_only" # all | users_only | tag_only | users_or_tag | listen_only
auto_register_users = true
user_tools_mode = "selected" # none | selected
user_allowed_tools = [
  "read_file",
  "write_file",
  "list_dir",
  "web_fetch",
  "memory_read",
  "memory_write",
  "cron",
  "discovery_web_search",
  "discovery_torrent_search"
]

[stt]
enabled = true
engine = "local_whisper_cpp"
local_model_path = "~/.masix/models/whisper/ggml-small.bin"
local_bin = "whisper-cli"
local_threads = 2
local_language = "it"

[mcp]
enabled = true

[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "~"]
timeout_secs = 30
startup_timeout_secs = 20
healthcheck_interval_secs = 60

[[mcp.servers]]
name = "memory"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-memory"]
timeout_secs = 30
startup_timeout_secs = 20
healthcheck_interval_secs = 60

# Optional codex-tools module server (example)
# [[mcp.servers]]
# name = "plugin_codex_tools"
# command = "/absolute/path/to/codex-tools.pkg"
# args = ["serve-mcp"]
# env = {
#   CODEX_PROVIDER = "openai",
#   CODEX_MODEL = "glm-5",
#   CODEX_API_KEY = "YOUR_CODEX_API_KEY",
#   CODEX_BASE_URL = "https://coding-intl.dashscope.aliyuncs.com/v1",
#   CODEX_TIMEOUT_SECS = "1800"
# }
# timeout_secs = 1800
# startup_timeout_secs = 30
# healthcheck_interval_secs = 120

[providers]
default_provider = "alibaba-coding-glm5"

[[providers.providers]]
name = "alibaba-coding-glm5"
api_key = "YOUR_ALIBABA_API_KEY"
base_url = "https://coding-intl.dashscope.aliyuncs.com/v1"
model = "glm-5"
provider_type = "openai"

[[providers.providers]]
name = "alibaba-coding-qwen35"
api_key = "YOUR_ALIBABA_API_KEY"
base_url = "https://coding-intl.dashscope.aliyuncs.com/v1"
model = "qwen3.5-plus"
provider_type = "openai"

[[providers.providers]]
name = "zai-coding"
api_key = "YOUR_ZAI_API_KEY"
base_url = "https://api.z.ai/api/coding/paas/v4"
model = "glm-4.7"
provider_type = "openai"

[[providers.providers]]
name = "anthropic"
api_key = "YOUR_ANTHROPIC_API_KEY"
base_url = "https://api.anthropic.com"
model = "claude-3-5-sonnet-latest"
provider_type = "anthropic"

[bots]
strict_account_profile_mapping = true

[[bots.profiles]]
name = "default"
workdir = "~/.masix/bots/YOUR_BOT_NAME/workdir"
memory_file = "~/.masix/bots/YOUR_BOT_NAME/memory/MEMORY.md"
soul_file = "~/.masix/bots/YOUR_BOT_NAME/memory/SOUL.md"
use_global_soul = false
use_global_memory = false
provider_primary = "alibaba-coding-glm5"
vision_provider = "alibaba-coding-qwen35"
provider_fallback = ["zai-coding"]
vision_fallback = ["zai-coding"]

[bots.profiles.retry]
window_secs = 60
initial_delay_secs = 2
backoff_factor = 2
max_delay_secs = 20

[exec]
enabled = true
allow_base = true
allow_termux = true
timeout_secs = 120
max_output_chars = 100000
base_allowlist = [
  "pwd", "ls", "whoami", "date", "uname", "uptime", "df", "du", "free", "head", "tail", "wc",
  "git", "gh",
  "cargo", "rustc", "rustup", "rustfmt", "clippy",
  "node", "npm", "npx", "yarn", "pnpm",
  "python", "python3", "pip", "pip3",
  "go",
  "cat", "echo", "mkdir", "rm", "cp", "mv", "touch", "chmod",
  "grep", "sed", "awk", "find", "sort", "uniq", "diff", "tree", "rg",
  "tar", "unzip", "zip", "gzip",
  "curl", "wget", "ssh", "scp", "rsync",
  "ps", "top", "htop", "kill", "pkill", "pgrep",
  "jq"
]
termux_allowlist = [
  "termux-info",
  "termux-battery-status",
  "termux-location",
  "termux-wifi-connectioninfo",
  "termux-telephony-deviceinfo",
  "termux-clipboard-get",
  "termux-clipboard-set",
  "termux-notification",
  "termux-storage-get",
  "termux-wake-lock",
  "termux-wake-unlock"
]

[policy]
rate_limit = { messages_per_minute = 60 }
```

## Security Notes

- Keep all API keys and bot tokens out of repository files.
- Use placeholders in documentation and local secret injection at runtime.
- `exec` and MCP tool surfaces are privileged: enable only what you need.
- For public/shared deployments, reduce allowlists and disable non-essential commands.
