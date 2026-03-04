#!/usr/bin/env sh
set -eu

log() {
  printf '%s\n' "$*"
}

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

toml_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

csv_ids_to_toml_array() {
  raw="${1:-}"
  cleaned="$(printf '%s' "$raw" | tr -d '[:space:]')"
  [ -z "$cleaned" ] && fail "MASIX_ADMIN_IDS is required (comma-separated numeric Telegram IDs)"
  out=""
  IFS=','
  for id in $cleaned; do
    [ -z "$id" ] && continue
    case "$id" in
      -*|[0-9]*)
        :
        ;;
      *)
        fail "Invalid Telegram ID '$id' in MASIX_ADMIN_IDS"
        ;;
    esac
    case "$id" in
      ''|*[!0-9-]*)
        fail "Invalid Telegram ID '$id' in MASIX_ADMIN_IDS"
        ;;
    esac
    if [ -z "$out" ]; then
      out="$id"
    else
      out="$out, $id"
    fi
  done
  unset IFS
  [ -z "$out" ] && fail "MASIX_ADMIN_IDS has no valid values"
  printf '[%s]' "$out"
}

append_provider() {
  provider_name="$1"
  key_var="$2"
  base_url="$3"
  default_model="$4"
  provider_type="$5"
  model_var="$6"

  eval "provider_key=\${$key_var:-}"
  [ -z "$provider_key" ] && return 0

  eval "model_override=\${$model_var:-}"
  if [ -n "$model_override" ]; then
    model="$model_override"
  else
    model="$default_model"
  fi

  key_escaped="$(toml_escape "$provider_key")"
  base_escaped="$(toml_escape "$base_url")"
  model_escaped="$(toml_escape "$model")"

  cat >> "$MASIX_CONFIG_FILE" <<EOF
[[providers.providers]]
name = "$provider_name"
api_key = "$key_escaped"
base_url = "$base_escaped"
model = "$model_escaped"
provider_type = "$provider_type"

EOF

  ENABLED_PROVIDERS="$ENABLED_PROVIDERS $provider_name"
}

ensure_memory_files() {
  mkdir -p "$MASIX_DATA_DIR" "$MASIX_DATA_DIR/memory/custom"
  if [ ! -f "$MASIX_DATA_DIR/SOUL.md" ]; then
    cp /opt/masix/memory/SOUL.md "$MASIX_DATA_DIR/SOUL.md"
  fi
  if [ ! -f "$MASIX_DATA_DIR/MEMORY.base.md" ]; then
    cp /opt/masix/memory/MEMORY.md "$MASIX_DATA_DIR/MEMORY.base.md"
  fi

  account_tag="$(printf '%s' "$MASIX_TELEGRAM_BOT_TOKEN" | cut -d: -f1)"
  account_memory_dir="$MASIX_DATA_DIR/accounts/$account_tag"
  account_memory_file="$account_memory_dir/MEMORY.md"
  mkdir -p "$account_memory_dir"

  if [ ! -f "$account_memory_file" ]; then
    cp "$MASIX_DATA_DIR/MEMORY.base.md" "$account_memory_file"
    for file in "$MASIX_DATA_DIR"/memory/custom/*.md; do
      [ -f "$file" ] || continue
      printf '\n\n# %s\n\n' "$(basename "$file")" >> "$account_memory_file"
      cat "$file" >> "$account_memory_file"
    done
  fi

  ln -snf "$account_memory_file" "$MASIX_DATA_DIR/MEMORY.current.md"
}

build_config() {
  : "${MASIX_TELEGRAM_BOT_TOKEN:?MASIX_TELEGRAM_BOT_TOKEN is required}"
  : "${MASIX_ADMIN_IDS:?MASIX_ADMIN_IDS is required}"
  : "${MASIX_ACTIVE_PROVIDER:?MASIX_ACTIVE_PROVIDER is required}"

  MASIX_BOT_NAME="${MASIX_BOT_NAME:-}"
  MASIX_LOG_LEVEL="${MASIX_LOG_LEVEL:-info}"
  MASIX_POLL_TIMEOUT_SECS="${MASIX_POLL_TIMEOUT_SECS:-60}"
  MASIX_AUTO_REGISTER_USERS="${MASIX_AUTO_REGISTER_USERS:-true}"
  MASIX_NOTIFY_ADMIN_ON_NEW_USER="${MASIX_NOTIFY_ADMIN_ON_NEW_USER:-true}"
  MASIX_GROUP_MODE="${MASIX_GROUP_MODE:-all}"
  MASIX_USER_TOOLS_MODE="${MASIX_USER_TOOLS_MODE:-none}"
  MASIX_USER_ALLOWED_TOOLS="${MASIX_USER_ALLOWED_TOOLS:-}"
  MASIX_REGISTER_TO_FILE="${MASIX_REGISTER_TO_FILE:-/data/register/telegram_users.json}"
  MASIX_NEW_USER_WELCOME_MESSAGE="${MASIX_NEW_USER_WELCOME_MESSAGE:-}"
  MASIX_ENABLE_STREAMING="${MASIX_ENABLE_STREAMING:-true}"
  MASIX_STREAMING_MODE="${MASIX_STREAMING_MODE:-telegram_draft}"

  admins_toml="$(csv_ids_to_toml_array "$MASIX_ADMIN_IDS")"

  mkdir -p "$(dirname "$MASIX_CONFIG_FILE")"
  cat > "$MASIX_CONFIG_FILE" <<EOF
[core]
data_dir = "$MASIX_DATA_DIR"
log_level = "$MASIX_LOG_LEVEL"
soul_file = "SOUL.md"

[core.streaming]
enabled = $MASIX_ENABLE_STREAMING
mode = "$MASIX_STREAMING_MODE"
flush_interval_ms = 900
max_message_edits = 20
finalize_timeout_secs = 10

[updates]
enabled = false
check_on_start = false
auto_apply = false
restart_after_update = false
channel = "latest"

[telegram]
poll_timeout_secs = $MASIX_POLL_TIMEOUT_SECS

[[telegram.accounts]]
bot_token = "$(toml_escape "$MASIX_TELEGRAM_BOT_TOKEN")"
EOF

  if [ -n "$MASIX_BOT_NAME" ]; then
    printf 'bot_name = "%s"\n' "$(toml_escape "$MASIX_BOT_NAME")" >> "$MASIX_CONFIG_FILE"
  fi

  cat >> "$MASIX_CONFIG_FILE" <<EOF
admins = $admins_toml
users = []
readonly = []
isolated = true
allow_self_memory_edit = true
group_mode = "$MASIX_GROUP_MODE"
auto_register_users = $MASIX_AUTO_REGISTER_USERS
notify_admin_on_new_user = $MASIX_NOTIFY_ADMIN_ON_NEW_USER
register_to_file = "$(toml_escape "$MASIX_REGISTER_TO_FILE")"
user_tools_mode = "$MASIX_USER_TOOLS_MODE"
# user_allowed_tools examples:
# user_allowed_tools = ["web_fetch", "plugin_discovery_web_search", "read_file"]
user_allowed_tools = []
EOF

  if [ -n "$MASIX_NEW_USER_WELCOME_MESSAGE" ]; then
    printf 'new_user_welcome_message = "%s"\n' "$(toml_escape "$MASIX_NEW_USER_WELCOME_MESSAGE")" >> "$MASIX_CONFIG_FILE"
  fi

  cat >> "$MASIX_CONFIG_FILE" <<EOF

[mcp]
enabled = false

[providers]
default_provider = "$(toml_escape "$MASIX_ACTIVE_PROVIDER")"

EOF

  ENABLED_PROVIDERS=""
  append_provider "openai" "MASIX_PROVIDER_OPENAI_KEY" "https://api.openai.com/v1" "gpt-5" "openai" "MASIX_PROVIDER_OPENAI_MODEL"
  append_provider "openrouter" "MASIX_PROVIDER_OPENROUTER_KEY" "https://openrouter.ai/api/v1" "openrouter/auto" "openai" "MASIX_PROVIDER_OPENROUTER_MODEL"
  append_provider "zai" "MASIX_PROVIDER_ZAI_KEY" "https://api.z.ai/api/paas/v4" "glm-5" "openai" "MASIX_PROVIDER_ZAI_MODEL"
  append_provider "zai-coding" "MASIX_PROVIDER_ZAI_CODING_KEY" "https://api.z.ai/api/coding/paas/v4" "glm-4.7" "openai" "MASIX_PROVIDER_ZAI_CODING_MODEL"
  append_provider "anthropic" "MASIX_PROVIDER_ANTHROPIC_KEY" "https://api.anthropic.com" "claude-sonnet-4-6" "anthropic" "MASIX_PROVIDER_ANTHROPIC_MODEL"
  append_provider "gemini" "MASIX_PROVIDER_GEMINI_KEY" "https://generativelanguage.googleapis.com/v1beta/openai" "gemini-2.5-pro" "openai" "MASIX_PROVIDER_GEMINI_MODEL"
  append_provider "xai" "MASIX_PROVIDER_XAI_KEY" "https://api.x.ai/v1" "grok-4-latest" "openai" "MASIX_PROVIDER_XAI_MODEL"
  append_provider "groq" "MASIX_PROVIDER_GROQ_KEY" "https://api.groq.com/openai/v1" "openai/gpt-oss-120b" "openai" "MASIX_PROVIDER_GROQ_MODEL"
  append_provider "deepseek" "MASIX_PROVIDER_DEEPSEEK_KEY" "https://api.deepseek.com/v1" "deepseek-reasoner" "openai" "MASIX_PROVIDER_DEEPSEEK_MODEL"
  append_provider "mistral" "MASIX_PROVIDER_MISTRAL_KEY" "https://api.mistral.ai/v1" "mistral-large-latest" "openai" "MASIX_PROVIDER_MISTRAL_MODEL"
  append_provider "together" "MASIX_PROVIDER_TOGETHER_KEY" "https://api.together.xyz/v1" "moonshotai/Kimi-K2.5" "openai" "MASIX_PROVIDER_TOGETHER_MODEL"
  append_provider "fireworks" "MASIX_PROVIDER_FIREWORKS_KEY" "https://api.fireworks.ai/inference/v1" "accounts/fireworks/models/llama-v3p1-70b-instruct" "openai" "MASIX_PROVIDER_FIREWORKS_MODEL"
  append_provider "cohere" "MASIX_PROVIDER_COHERE_KEY" "https://api.cohere.ai/v1" "command-a-03-2025" "openai" "MASIX_PROVIDER_COHERE_MODEL"
  append_provider "alibaba-model-studio" "MASIX_PROVIDER_ALIBABA_MODEL_STUDIO_KEY" "https://dashscope-intl.aliyuncs.com/compatible-mode/v1" "qwen-plus" "openai" "MASIX_PROVIDER_ALIBABA_MODEL_STUDIO_MODEL"
  append_provider "alibaba-coding-plan" "MASIX_PROVIDER_ALIBABA_CODING_PLAN_KEY" "https://coding-intl.dashscope.aliyuncs.com/v1" "qwen3.5-plus" "openai" "MASIX_PROVIDER_ALIBABA_CODING_PLAN_MODEL"
  append_provider "alibaba-anthropic" "MASIX_PROVIDER_ALIBABA_ANTHROPIC_KEY" "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic" "qwen3.5-plus" "anthropic" "MASIX_PROVIDER_ALIBABA_ANTHROPIC_MODEL"

  if [ -n "${MASIX_PROVIDER_CUSTOM_KEY:-}" ]; then
    : "${MASIX_PROVIDER_CUSTOM_BASE_URL:?MASIX_PROVIDER_CUSTOM_BASE_URL is required with MASIX_PROVIDER_CUSTOM_KEY}"
    custom_model="${MASIX_PROVIDER_CUSTOM_MODEL:-custom-model}"
    append_provider "custom" "MASIX_PROVIDER_CUSTOM_KEY" "$MASIX_PROVIDER_CUSTOM_BASE_URL" "$custom_model" "openai" "MASIX_PROVIDER_CUSTOM_MODEL"
  fi

  if [ -z "$ENABLED_PROVIDERS" ]; then
    fail "No provider key configured. Set at least one MASIX_PROVIDER_*_KEY"
  fi

  case " $ENABLED_PROVIDERS " in
    *" $MASIX_ACTIVE_PROVIDER "*) ;;
    *) fail "MASIX_ACTIVE_PROVIDER='$MASIX_ACTIVE_PROVIDER' not configured by provided keys" ;;
  esac

  cat >> "$MASIX_CONFIG_FILE" <<EOF
[exec]
enabled = true
allow_base = true
allow_termux = false
timeout_secs = 60
max_output_chars = 8000

[policy]
rate_limit = { messages_per_minute = 60 }
EOF
}

main() {
  build_config
  ensure_memory_files
  log "MasiX Telegram assistant config generated at $MASIX_CONFIG_FILE"
  exec masix start --foreground --config "$MASIX_CONFIG_FILE"
}

main "$@"

