//! Masix Configuration
//!
//! TOML configuration loading with environment variable support

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub core: CoreConfig,
    pub telegram: Option<TelegramConfig>,
    pub whatsapp: Option<WhatsappConfig>,
    pub sms: Option<SmsConfig>,
    pub mcp: Option<McpConfig>,
    #[serde(default)]
    pub providers: ProvidersConfig,
    pub bots: Option<BotsConfig>,
    pub exec: Option<ExecConfig>,
    pub policy: Option<PolicyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoreConfig {
    pub data_dir: Option<String>,
    pub log_level: Option<String>,
    pub soul_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    pub bot_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsappConfig {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub read_only: bool,
    pub transport_path: Option<String>,
    pub ingress_shared_secret: Option<String>,
    pub max_message_chars: Option<usize>,
    #[serde(default)]
    pub allowed_senders: Vec<String>,
    pub forward_to_telegram_chat_id: Option<i64>,
    pub forward_to_telegram_account_tag: Option<String>,
    pub forward_prefix: Option<String>,
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
    pub forward_to_telegram_chat_id: Option<i64>,
    pub forward_to_telegram_account_tag: Option<String>,
    pub forward_prefix: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
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
    #[serde(default)]
    pub default_provider: String,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            default_provider: String::new(),
            providers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub provider_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotsConfig {
    #[serde(default)]
    pub strict_account_profile_mapping: Option<bool>,
    #[serde(default)]
    pub profiles: Vec<BotProfileConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotProfileConfig {
    pub name: String,
    pub workdir: String,
    pub memory_file: String,
    pub provider_primary: String,
    #[serde(default)]
    pub vision_provider: Option<String>,
    #[serde(default)]
    pub provider_fallback: Vec<String>,
    pub retry: Option<RetryPolicyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicyConfig {
    pub window_secs: Option<u64>,
    pub initial_delay_secs: Option<u64>,
    pub backoff_factor: Option<u32>,
    pub max_delay_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecConfig {
    pub enabled: Option<bool>,
    pub allow_base: Option<bool>,
    pub allow_termux: Option<bool>,
    pub timeout_secs: Option<u64>,
    pub max_output_chars: Option<usize>,
    #[serde(default)]
    pub base_allowlist: Vec<String>,
    #[serde(default)]
    pub termux_allowlist: Vec<String>,
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

fn default_true() -> bool {
    true
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn default_path() -> Option<std::path::PathBuf> {
        dirs::config_dir().map(|dir| dir.join("masix").join("config.toml"))
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let mut provider_names = HashSet::new();
        let mut provider_targets: HashMap<String, String> = HashMap::new();
        for provider in &self.providers.providers {
            let name = provider.name.trim();
            if name.is_empty() {
                anyhow::bail!("Provider name cannot be empty");
            }
            if !provider_names.insert(name.to_string()) {
                anyhow::bail!("Duplicate provider name '{}'", name);
            }

            let target_key = provider
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .and_then(|base_url| {
                    provider
                        .model
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(|model| {
                            let provider_type = provider
                                .provider_type
                                .as_deref()
                                .unwrap_or("openai")
                                .trim()
                                .to_lowercase();
                            format!(
                                "{}|{}|{}",
                                provider_type,
                                base_url.to_lowercase(),
                                model.to_lowercase()
                            )
                        })
                });

            if let Some(target_key) = target_key {
                if let Some(existing_name) =
                    provider_targets.insert(target_key.clone(), name.to_string())
                {
                    anyhow::bail!(
                        "Duplicate provider target endpoint+model between '{}' and '{}': {}",
                        existing_name,
                        name,
                        target_key
                    );
                }
            }
        }

        if !provider_names.contains(&self.providers.default_provider) {
            anyhow::bail!(
                "default_provider '{}' is not defined in providers.providers",
                self.providers.default_provider
            );
        }

        let mut profile_names = HashSet::new();
        let mut has_profiles = false;
        let mut strict_mapping = false;

        if let Some(bots) = &self.bots {
            has_profiles = !bots.profiles.is_empty();
            strict_mapping = bots.strict_account_profile_mapping.unwrap_or(false);

            for profile in &bots.profiles {
                let profile_name = profile.name.trim();
                if profile_name.is_empty() {
                    anyhow::bail!("Bot profile name cannot be empty");
                }
                if !profile_names.insert(profile_name.to_string()) {
                    anyhow::bail!("Duplicate bot profile '{}'", profile_name);
                }

                if profile.workdir.trim().is_empty() {
                    anyhow::bail!("Bot profile '{}' has empty workdir", profile_name);
                }
                if profile.memory_file.trim().is_empty() {
                    anyhow::bail!("Bot profile '{}' has empty memory_file", profile_name);
                }

                if !provider_names.contains(profile.provider_primary.trim()) {
                    anyhow::bail!(
                        "Bot profile '{}' primary provider '{}' is not defined",
                        profile_name,
                        profile.provider_primary
                    );
                }

                if let Some(vision_provider) = &profile.vision_provider {
                    let vision_name = vision_provider.trim();
                    if vision_name.is_empty() {
                        anyhow::bail!("Bot profile '{}' has empty vision_provider", profile_name);
                    }
                    if !provider_names.contains(vision_name) {
                        anyhow::bail!(
                            "Bot profile '{}' vision provider '{}' is not defined",
                            profile_name,
                            vision_name
                        );
                    }
                }

                let mut seen_fallbacks = HashSet::new();
                for fallback in &profile.provider_fallback {
                    let f = fallback.trim();
                    if f.is_empty() {
                        anyhow::bail!(
                            "Bot profile '{}' contains empty fallback provider entry",
                            profile_name
                        );
                    }
                    if !provider_names.contains(f) {
                        anyhow::bail!(
                            "Bot profile '{}' fallback provider '{}' is not defined",
                            profile_name,
                            f
                        );
                    }
                    if !seen_fallbacks.insert(f.to_string()) {
                        anyhow::bail!(
                            "Bot profile '{}' contains duplicate fallback provider '{}'",
                            profile_name,
                            f
                        );
                    }
                }

                if let Some(retry) = &profile.retry {
                    if let Some(window) = retry.window_secs {
                        if window == 0 {
                            anyhow::bail!(
                                "Bot profile '{}' retry.window_secs must be > 0",
                                profile_name
                            );
                        }
                    }
                    if let Some(initial) = retry.initial_delay_secs {
                        if initial == 0 {
                            anyhow::bail!(
                                "Bot profile '{}' retry.initial_delay_secs must be > 0",
                                profile_name
                            );
                        }
                    }
                    if let Some(factor) = retry.backoff_factor {
                        if factor < 1 {
                            anyhow::bail!(
                                "Bot profile '{}' retry.backoff_factor must be >= 1",
                                profile_name
                            );
                        }
                    }
                    if let Some(max_delay) = retry.max_delay_secs {
                        if max_delay == 0 {
                            anyhow::bail!(
                                "Bot profile '{}' retry.max_delay_secs must be > 0",
                                profile_name
                            );
                        }
                    }
                }
            }
        }

        if let Some(telegram) = &self.telegram {
            for account in &telegram.accounts {
                if let Some(profile_name) = &account.bot_profile {
                    if !has_profiles {
                        anyhow::bail!(
                            "Telegram account references bot_profile '{}' but no bots.profiles are defined",
                            profile_name
                        );
                    }
                    if !profile_names.contains(profile_name) {
                        anyhow::bail!(
                            "Telegram account references unknown bot_profile '{}'",
                            profile_name
                        );
                    }
                } else if strict_mapping && has_profiles {
                    anyhow::bail!(
                        "strict_account_profile_mapping is enabled but a telegram account has no bot_profile"
                    );
                }
            }
        }

        if let Some(exec) = &self.exec {
            if let Some(timeout) = exec.timeout_secs {
                if timeout == 0 {
                    anyhow::bail!("exec.timeout_secs must be > 0");
                }
            }
            if let Some(max_output) = exec.max_output_chars {
                if max_output < 128 {
                    anyhow::bail!("exec.max_output_chars must be >= 128");
                }
            }
            for item in &exec.base_allowlist {
                if item.trim().is_empty() {
                    anyhow::bail!("exec.base_allowlist contains an empty command");
                }
            }
            for item in &exec.termux_allowlist {
                if item.trim().is_empty() {
                    anyhow::bail!("exec.termux_allowlist contains an empty command");
                }
            }
        }

        if let Some(whatsapp) = &self.whatsapp {
            if whatsapp.enabled {
                if !whatsapp.read_only {
                    anyhow::bail!("whatsapp.read_only=false is not supported");
                }
                if let Some(max_chars) = whatsapp.max_message_chars {
                    if max_chars == 0 || max_chars > 20000 {
                        anyhow::bail!("whatsapp.max_message_chars must be in range 1..=20000");
                    }
                }
                for sender in &whatsapp.allowed_senders {
                    if sender.trim().is_empty() {
                        anyhow::bail!("whatsapp.allowed_senders contains an empty entry");
                    }
                }
                if whatsapp.forward_to_telegram_chat_id.is_some() {
                    let has_telegram = self
                        .telegram
                        .as_ref()
                        .map(|tg| !tg.accounts.is_empty())
                        .unwrap_or(false);
                    if !has_telegram {
                        anyhow::bail!(
                            "whatsapp.forward_to_telegram_chat_id requires at least one telegram account"
                        );
                    }
                }
            }
        }

        if let Some(sms) = &self.sms {
            if sms.enabled {
                if let Some(interval) = sms.watch_interval_secs {
                    if interval == 0 || interval > 3600 {
                        anyhow::bail!("sms.watch_interval_secs must be in range 1..=3600");
                    }
                }
                if sms.forward_to_telegram_chat_id.is_some() {
                    let has_telegram = self
                        .telegram
                        .as_ref()
                        .map(|tg| !tg.accounts.is_empty())
                        .unwrap_or(false);
                    if !has_telegram {
                        anyhow::bail!(
                            "sms.forward_to_telegram_chat_id requires at least one telegram account"
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    fn parse_config(input: &str) -> Config {
        let cfg: Config = toml::from_str(input).expect("valid TOML");
        cfg
    }

    #[test]
    fn validate_accepts_legacy_config_without_bots() {
        let cfg = parse_config(
            r#"
[core]

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_unknown_default_provider() {
        let cfg = parse_config(
            r#"
[core]

[providers]
default_provider = "missing"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_unknown_bot_profile_in_telegram_account() {
        let cfg = parse_config(
            r#"
[core]

[telegram]
[[telegram.accounts]]
bot_token = "123:abc"
bot_profile = "missing"

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[bots]
[[bots.profiles]]
name = "ops"
workdir = "~/.masix/bots/ops"
memory_file = "~/.masix/bots/ops/MEMORY.md"
provider_primary = "openai"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_accepts_bot_profiles_with_valid_provider_chain() {
        let cfg = parse_config(
            r#"
[core]

[telegram]
[[telegram.accounts]]
bot_token = "123:abc"
bot_profile = "ops"

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[[providers.providers]]
name = "openrouter"
api_key = "k"

[bots]
strict_account_profile_mapping = true
[[bots.profiles]]
name = "ops"
workdir = "~/.masix/bots/ops"
memory_file = "~/.masix/bots/ops/MEMORY.md"
provider_primary = "openai"
vision_provider = "openai"
provider_fallback = ["openrouter"]
[bots.profiles.retry]
window_secs = 600
initial_delay_secs = 2
backoff_factor = 2
max_delay_secs = 30
"#,
        );
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_duplicate_provider_target_endpoint_and_model() {
        let cfg = parse_config(
            r#"
[core]

[providers]
default_provider = "zai_main"

[[providers.providers]]
name = "zai_main"
api_key = "k1"
base_url = "https://api.z.ai/api/paas/v4"
model = "glm-4.5"
provider_type = "openai"

[[providers.providers]]
name = "zai_alias"
api_key = "k2"
base_url = "https://api.z.ai/api/paas/v4"
model = "glm-4.5"
provider_type = "openai"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_unknown_vision_provider_in_profile() {
        let cfg = parse_config(
            r#"
[core]

[telegram]
[[telegram.accounts]]
bot_token = "123:abc"
bot_profile = "ops"

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[bots]
[[bots.profiles]]
name = "ops"
workdir = "~/.masix/bots/ops"
memory_file = "~/.masix/bots/ops/MEMORY.md"
provider_primary = "openai"
vision_provider = "missing"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_whatsapp_non_read_only_mode() {
        let cfg = parse_config(
            r#"
[core]

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[whatsapp]
enabled = true
read_only = false
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_accepts_whatsapp_read_only_with_telegram_forward() {
        let cfg = parse_config(
            r#"
[core]

[telegram]
[[telegram.accounts]]
bot_token = "123:abc"

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[whatsapp]
enabled = true
read_only = true
forward_to_telegram_chat_id = 111111111
"#,
        );
        assert!(cfg.validate().is_ok());
    }
}
