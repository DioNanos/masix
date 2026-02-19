//! Masix Configuration
//!
//! TOML configuration loading with environment variable support

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub core: CoreConfig,
    pub telegram: Option<TelegramConfig>,
    pub whatsapp: Option<WhatsappConfig>,
    pub sms: Option<SmsConfig>,
    pub mcp: Option<McpConfig>,
    pub providers: ProvidersConfig,
    pub policy: Option<PolicyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    pub data_dir: Option<String>,
    pub log_level: Option<String>,
    pub soul_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub poll_timeout_secs: Option<u64>,
    pub client_recreate_interval_secs: Option<u64>,
    pub default_policy: Option<String>,
    #[serde(default)]
    pub accounts: Vec<TelegramAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramAccount {
    pub bot_token: String,
    pub allowed_chats: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsappConfig {
    pub enabled: bool,
    pub transport_path: Option<String>,
    #[serde(default)]
    pub accounts: Vec<WhatsappAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsappAccount {
    pub session_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsConfig {
    pub enabled: bool,
    pub watch_interval_secs: Option<u64>,
    #[serde(default)]
    pub rules: Vec<SmsRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsRule {
    pub event_type: String,
    pub pattern_type: Option<String>,
    pub pattern_value: Option<String>,
    pub action_type: String,
    pub action_config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    pub enabled: bool,
    #[serde(default)]
    pub servers: Vec<McpServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    pub default_provider: String,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub allowlist: Option<Vec<String>>,
    pub denylist: Option<Vec<String>>,
    pub rate_limit: Option<RateLimitConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub messages_per_minute: u32,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn default_path() -> Option<std::path::PathBuf> {
        dirs::config_dir().map(|dir| dir.join("masix").join("config.toml"))
    }
}
