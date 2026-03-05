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
    #[serde(default)]
    pub updates: UpdatesConfig,
    pub telegram: Option<TelegramConfig>,
    pub sms: Option<SmsConfig>,
    pub stt: Option<SttConfig>,
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
    #[serde(default)]
    pub global_memory_file: Option<String>,
    /// Legacy max tool iterations in the agent loop. Default: 25.
    #[serde(default)]
    pub max_tool_iterations: Option<usize>,
    #[serde(default)]
    pub agent_loop: CoreAgentLoopConfig,
    #[serde(default)]
    pub tool_progress: CoreToolProgressConfig,
    #[serde(default)]
    pub streaming: CoreStreamingConfig,
    #[serde(default)]
    pub cron: CoreCronConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentLoopContinuationDetection {
    #[default]
    HeuristicV1,
    StrictV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreAgentLoopConfig {
    #[serde(default = "default_true")]
    pub auto_continue_enabled: bool,
    #[serde(default = "default_agent_loop_auto_continue_max")]
    pub auto_continue_max: u8,
    #[serde(default)]
    pub continuation_detection: AgentLoopContinuationDetection,
    #[serde(default)]
    pub max_tool_iterations: Option<usize>,
}

impl Default for CoreAgentLoopConfig {
    fn default() -> Self {
        Self {
            auto_continue_enabled: true,
            auto_continue_max: default_agent_loop_auto_continue_max(),
            continuation_detection: AgentLoopContinuationDetection::HeuristicV1,
            max_tool_iterations: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolProgressMode {
    #[default]
    FirstRound,
    Periodic,
    Milestones,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreToolProgressConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: ToolProgressMode,
    #[serde(default = "default_tool_progress_interval_secs")]
    pub interval_secs: u64,
    #[serde(default = "default_tool_progress_max_updates")]
    pub max_updates: u8,
    #[serde(default = "default_true")]
    pub include_tool_names: bool,
}

impl Default for CoreToolProgressConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: ToolProgressMode::FirstRound,
            interval_secs: default_tool_progress_interval_secs(),
            max_updates: default_tool_progress_max_updates(),
            include_tool_names: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StreamingMode {
    #[default]
    Off,
    TelegramEdit,
    TelegramChunked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreStreamingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: StreamingMode,
    #[serde(default = "default_streaming_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default = "default_streaming_max_message_edits")]
    pub max_message_edits: u16,
    #[serde(default = "default_streaming_finalize_timeout_secs")]
    pub finalize_timeout_secs: u64,
}

impl Default for CoreStreamingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: StreamingMode::Off,
            flush_interval_ms: default_streaming_flush_interval_ms(),
            max_message_edits: default_streaming_max_message_edits(),
            finalize_timeout_secs: default_streaming_finalize_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreCronConfig {
    #[serde(default = "default_cron_dispatch_concurrency")]
    pub dispatch_concurrency: u8,
    #[serde(default = "default_cron_delivery_retry_count")]
    pub delivery_retry_count: u8,
    #[serde(default = "default_cron_delivery_retry_backoff_secs")]
    pub delivery_retry_backoff_secs: u64,
    #[serde(default = "default_true")]
    pub dead_letter_enabled: bool,
}

impl Default for CoreCronConfig {
    fn default() -> Self {
        Self {
            dispatch_concurrency: default_cron_dispatch_concurrency(),
            delivery_retry_count: default_cron_delivery_retry_count(),
            delivery_retry_backoff_secs: default_cron_delivery_retry_backoff_secs(),
            dead_letter_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatesConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub check_on_start: bool,
    #[serde(default = "default_true")]
    pub auto_apply: bool,
    #[serde(default = "default_true")]
    pub restart_after_update: bool,
    #[serde(default = "default_update_channel")]
    pub channel: String,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_on_start: true,
            auto_apply: true,
            restart_after_update: true,
            channel: default_update_channel(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DmPolicy {
    #[default]
    Pairing,
    Allowlist,
    Open,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GroupPolicy {
    #[default]
    Allowlist,
    Open,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    AdminOnlyRegistered,
    AssistantAutoregister,
    GroupTagResponse,
    GroupProfiler,
    GroupAllResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UserToolsMode {
    #[default]
    None,
    Selected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramPairingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_pairing_pending_ttl_secs")]
    pub pending_ttl_secs: u64,
    #[serde(default = "default_pairing_pending_max")]
    pub pending_max: usize,
}

impl Default for TelegramPairingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pending_ttl_secs: default_pairing_pending_ttl_secs(),
            pending_max: default_pairing_pending_max(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramGroupAccess {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub require_mention: bool,
    #[serde(default)]
    pub group_policy: Option<GroupPolicy>,
    #[serde(default)]
    pub allow_from: Vec<i64>,
}

impl Default for TelegramGroupAccess {
    fn default() -> Self {
        Self {
            enabled: true,
            require_mention: true,
            group_policy: None,
            allow_from: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramConfig {
    pub poll_timeout_secs: Option<u64>,
    pub client_recreate_interval_secs: Option<u64>,
    pub default_policy: Option<String>,
    #[serde(default)]
    pub accounts: Vec<TelegramAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramAccount {
    pub bot_token: String,
    #[serde(default)]
    pub bot_name: Option<String>,
    pub bot_profile: Option<String>,
    #[serde(default)]
    pub allowed_chats: Option<Vec<i64>>,
    #[serde(default)]
    pub admins: Vec<i64>,
    #[serde(default)]
    pub users: Vec<i64>,
    #[serde(default)]
    pub readonly: Vec<i64>,
    #[serde(default = "default_true")]
    pub isolated: bool,
    #[serde(default)]
    pub shared_memory_with: Vec<String>,
    #[serde(default = "default_true")]
    pub allow_self_memory_edit: bool,
    #[serde(default)]
    pub dm_policy: DmPolicy,
    #[serde(default)]
    pub dm_allow_from: Vec<i64>,
    #[serde(default)]
    pub access_mode: Option<AccessMode>,
    #[serde(default)]
    pub group_policy: GroupPolicy,
    #[serde(default = "default_true")]
    pub group_require_mention: bool,
    #[serde(default)]
    pub group_allow_known_untagged: bool,
    #[serde(default)]
    pub group_allow_from: Vec<i64>,
    #[serde(default)]
    pub groups: std::collections::BTreeMap<String, TelegramGroupAccess>,
    #[serde(default)]
    pub pairing: TelegramPairingConfig,
    #[serde(default)]
    pub register_to_file: Option<String>,
    #[serde(default = "default_true")]
    pub notify_admin_on_new_user: bool,
    #[serde(default)]
    pub new_user_welcome_message: Option<String>,
    #[serde(default)]
    pub start_welcome_admin: Option<String>,
    #[serde(default)]
    pub start_welcome_user: Option<String>,
    #[serde(default)]
    pub user_tools_mode: UserToolsMode,
    #[serde(default)]
    pub user_allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsConfig {
    pub enabled: bool,
    pub watch_interval_secs: Option<u64>,
    pub forward_to_telegram_chat_id: Option<i64>,
    pub forward_to_telegram_account_tag: Option<String>,
    pub forward_prefix: Option<String>,
    #[serde(default)]
    pub allowed_senders: Vec<String>,
    #[serde(default)]
    pub admins: Vec<String>,
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub rules: Vec<SmsRule>,
}

impl SmsConfig {
    pub fn get_permission_level(&self, sender: &str) -> PermissionLevel {
        let sender = sender.trim();
        if sender.is_empty() {
            return PermissionLevel::None;
        }

        if self.admins.iter().any(|a| a == sender) {
            return PermissionLevel::Admin;
        }
        if self.users.iter().any(|u| u == sender)
            || self.allowed_senders.iter().any(|s| s == sender)
        {
            return PermissionLevel::User;
        }

        // Backward-compatible default for existing configs that did not declare sender ACL.
        if self.admins.is_empty() && self.users.is_empty() && self.allowed_senders.is_empty() {
            PermissionLevel::User
        } else {
            PermissionLevel::None
        }
    }

    pub fn is_authorized(&self, sender: &str) -> bool {
        self.get_permission_level(sender) != PermissionLevel::None
    }

    pub fn is_admin(&self, sender: &str) -> bool {
        self.get_permission_level(sender) == PermissionLevel::Admin
    }

    pub fn can_use_tools(&self, sender: &str) -> bool {
        self.is_admin(sender)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_stt_engine")]
    pub engine: String,
    pub local_model_path: Option<String>,
    pub local_bin: Option<String>,
    pub local_threads: Option<u32>,
    pub local_language: Option<String>,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: default_stt_engine(),
            local_model_path: None,
            local_bin: None,
            local_threads: Some(2),
            local_language: Some("it".to_string()),
        }
    }
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
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default = "default_mcp_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_mcp_startup_timeout")]
    pub startup_timeout_secs: u64,
    #[serde(default = "default_mcp_healthcheck_interval")]
    pub healthcheck_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersConfig {
    #[serde(default)]
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
    #[serde(default)]
    pub soul_file: Option<String>,
    #[serde(default)]
    pub use_global_soul: bool,
    #[serde(default)]
    pub use_global_memory: bool,
    pub provider_primary: String,
    #[serde(default)]
    pub vision_provider: Option<String>,
    #[serde(default)]
    pub provider_fallback: Vec<String>,
    #[serde(default)]
    pub vision_fallback: Vec<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionLevel {
    Admin,
    User,
    Readonly,
    None,
}

impl TelegramAccount {
    fn permission_for_mode(
        &self,
        mode: AccessMode,
        user_id: i64,
        chat_id: i64,
        is_bot_tagged: bool,
    ) -> PermissionLevel {
        let user_perm = self.get_permission_level(user_id);
        let is_private = user_id == chat_id;
        let is_allowlisted = self.dm_allow_from.contains(&user_id);

        if is_private {
            return match mode {
                AccessMode::AssistantAutoregister => user_perm,
                _ => {
                    if user_perm != PermissionLevel::None {
                        user_perm
                    } else if is_allowlisted {
                        PermissionLevel::User
                    } else {
                        PermissionLevel::None
                    }
                }
            };
        }

        match mode {
            AccessMode::AdminOnlyRegistered => {
                if user_perm != PermissionLevel::None || self.group_allow_from.contains(&user_id) {
                    if user_perm == PermissionLevel::None {
                        PermissionLevel::User
                    } else {
                        user_perm
                    }
                } else {
                    PermissionLevel::None
                }
            }
            AccessMode::AssistantAutoregister => {
                if user_perm != PermissionLevel::None {
                    user_perm
                } else if is_bot_tagged {
                    PermissionLevel::User
                } else {
                    PermissionLevel::None
                }
            }
            AccessMode::GroupTagResponse => {
                if is_bot_tagged {
                    if user_perm == PermissionLevel::None {
                        PermissionLevel::User
                    } else {
                        user_perm
                    }
                } else {
                    PermissionLevel::None
                }
            }
            AccessMode::GroupProfiler => {
                if is_bot_tagged && user_perm == PermissionLevel::Admin {
                    PermissionLevel::Admin
                } else {
                    PermissionLevel::None
                }
            }
            AccessMode::GroupAllResponse => {
                if user_perm == PermissionLevel::Admin {
                    PermissionLevel::Admin
                } else if user_perm == PermissionLevel::Readonly {
                    PermissionLevel::Readonly
                } else {
                    PermissionLevel::User
                }
            }
        }
    }

    pub fn get_permission_level(&self, user_id: i64) -> PermissionLevel {
        if self.admins.contains(&user_id) {
            PermissionLevel::Admin
        } else if self.users.contains(&user_id) {
            PermissionLevel::User
        } else if self.readonly.contains(&user_id) {
            PermissionLevel::Readonly
        } else if let Some(allowed) = &self.allowed_chats {
            if allowed.contains(&user_id) {
                PermissionLevel::User
            } else {
                PermissionLevel::None
            }
        } else {
            PermissionLevel::None
        }
    }

    pub fn get_permission_for_group(
        &self,
        user_id: i64,
        chat_id: i64,
        is_bot_tagged: bool,
    ) -> PermissionLevel {
        if let Some(mode) = self.access_mode {
            return self.permission_for_mode(mode, user_id, chat_id, is_bot_tagged);
        }

        let is_private = user_id == chat_id;

        if is_private {
            let base = self.get_permission_level(user_id);
            let is_allowed = base != PermissionLevel::None || self.dm_allow_from.contains(&user_id);
            return match self.dm_policy {
                DmPolicy::Disabled => PermissionLevel::None,
                DmPolicy::Open => {
                    if base == PermissionLevel::None {
                        PermissionLevel::User
                    } else {
                        base
                    }
                }
                DmPolicy::Allowlist | DmPolicy::Pairing => {
                    if is_allowed {
                        if base == PermissionLevel::None {
                            PermissionLevel::User
                        } else {
                            base
                        }
                    } else {
                        PermissionLevel::None
                    }
                }
            };
        }

        let group_key = chat_id.to_string();
        let group_cfg = self.groups.get(&group_key);
        if matches!(group_cfg, Some(cfg) if !cfg.enabled) {
            return PermissionLevel::None;
        }

        let effective_group_policy = group_cfg
            .and_then(|cfg| cfg.group_policy)
            .unwrap_or(self.group_policy);
        let require_mention = group_cfg
            .map(|cfg| cfg.require_mention)
            .unwrap_or(self.group_require_mention);
        let sender_allow = if let Some(cfg) = group_cfg {
            if cfg.allow_from.is_empty() {
                if self.group_allow_from.is_empty() {
                    &self.dm_allow_from
                } else {
                    &self.group_allow_from
                }
            } else {
                &cfg.allow_from
            }
        } else if self.group_allow_from.is_empty() {
            &self.dm_allow_from
        } else {
            &self.group_allow_from
        };

        let user_perm = self.get_permission_level(user_id);
        if require_mention && !is_bot_tagged && user_perm != PermissionLevel::Admin {
            if !(self.group_allow_known_untagged && user_perm != PermissionLevel::None) {
                return PermissionLevel::None;
            }
        }

        match effective_group_policy {
            GroupPolicy::Open => {
                if user_perm == PermissionLevel::Admin {
                    PermissionLevel::Admin
                } else if user_perm == PermissionLevel::Readonly {
                    PermissionLevel::Readonly
                } else {
                    PermissionLevel::User
                }
            }
            GroupPolicy::Disabled => PermissionLevel::None,
            GroupPolicy::Allowlist => {
                if user_perm != PermissionLevel::None || sender_allow.contains(&user_id) {
                    if user_perm == PermissionLevel::None {
                        PermissionLevel::User
                    } else {
                        user_perm
                    }
                } else {
                    PermissionLevel::None
                }
            }
        }
    }

    pub fn should_respond(&self, user_id: i64, chat_id: i64, is_bot_tagged: bool) -> bool {
        self.get_permission_for_group(user_id, chat_id, is_bot_tagged) != PermissionLevel::None
    }

    pub fn is_authorized(&self, user_id: i64) -> bool {
        self.get_permission_level(user_id) != PermissionLevel::None
    }

    pub fn is_admin(&self, user_id: i64) -> bool {
        self.get_permission_level(user_id) == PermissionLevel::Admin
    }

    pub fn can_use_tools(&self, user_id: i64, chat_id: i64, is_bot_tagged: bool) -> bool {
        matches!(
            self.get_permission_for_group(user_id, chat_id, is_bot_tagged),
            PermissionLevel::Admin
        )
    }

    pub fn bot_username(&self) -> Option<String> {
        self.bot_name
            .as_deref()
            .map(str::trim)
            .map(|value| value.trim_start_matches('@').to_lowercase())
            .filter(|value| !value.is_empty())
    }

    pub fn get_bot_name(&self) -> String {
        self.bot_username().unwrap_or_else(|| "unknown".to_string())
    }

    pub fn uses_dm_pairing(&self) -> bool {
        if matches!(self.access_mode, Some(AccessMode::AssistantAutoregister)) {
            return self.pairing.enabled;
        }
        self.pairing.enabled && self.dm_policy == DmPolicy::Pairing
    }
}

pub fn telegram_account_tag(bot_token: &str) -> String {
    let token = bot_token.trim();
    token.split(':').next().unwrap_or(token).trim().to_string()
}

fn default_true() -> bool {
    true
}

fn default_update_channel() -> String {
    "latest".to_string()
}

fn default_stt_engine() -> String {
    "local_whisper_cpp".to_string()
}

fn default_mcp_timeout() -> u64 {
    30
}

fn default_mcp_startup_timeout() -> u64 {
    20
}

fn default_mcp_healthcheck_interval() -> u64 {
    60
}

fn default_pairing_pending_ttl_secs() -> u64 {
    3600
}

fn default_pairing_pending_max() -> usize {
    3
}

fn default_agent_loop_auto_continue_max() -> u8 {
    3
}

fn default_tool_progress_interval_secs() -> u64 {
    8
}

fn default_tool_progress_max_updates() -> u8 {
    5
}

fn default_streaming_flush_interval_ms() -> u64 {
    900
}

fn default_streaming_max_message_edits() -> u16 {
    20
}

fn default_streaming_finalize_timeout_secs() -> u64 {
    10
}

fn default_cron_dispatch_concurrency() -> u8 {
    2
}

fn default_cron_delivery_retry_count() -> u8 {
    2
}

fn default_cron_delivery_retry_backoff_secs() -> u64 {
    10
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let value: toml::Value = toml::from_str(&content)?;
        if value
            .as_table()
            .map(|table| table.contains_key("whatsapp"))
            .unwrap_or(false)
        {
            anyhow::bail!(
                "The [whatsapp] section is deprecated in masix-core. Use the separate module 'masix-whatsapp-business'."
            );
        }
        if let Some(telegram) = value.get("telegram").and_then(|v| v.as_table()) {
            if let Some(accounts) = telegram.get("accounts").and_then(|v| v.as_array()) {
                for (index, account) in accounts.iter().enumerate() {
                    let Some(tbl) = account.as_table() else {
                        continue;
                    };
                    if tbl.contains_key("group_mode") || tbl.contains_key("auto_register_users") {
                        anyhow::bail!(
                            "telegram.accounts[{}] uses legacy keys (group_mode/auto_register_users). Please migrate to dm_policy/group_policy/group_require_mention/group_allow_known_untagged.",
                            index
                        );
                    }
                }
            }
        }
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

        if !self.providers.default_provider.is_empty()
            && !provider_names.contains(&self.providers.default_provider)
        {
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
                    if f == profile.provider_primary.trim() {
                        anyhow::bail!(
                            "Bot profile '{}' fallback provider '{}' cannot match primary provider",
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
            let mut telegram_account_tags = HashSet::new();
            for account in &telegram.accounts {
                let token = account.bot_token.trim();
                if token.is_empty() {
                    anyhow::bail!("Telegram account bot_token cannot be empty");
                }
                let account_tag = token.split(':').next().unwrap_or(token).trim();
                if account_tag.is_empty() {
                    anyhow::bail!("Telegram account bot_token has invalid account tag");
                }
                if !telegram_account_tags.insert(account_tag.to_string()) {
                    anyhow::bail!(
                        "Duplicate Telegram account token/account tag '{}'",
                        account_tag
                    );
                }

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

        if let Some(max_iters) = self.core.max_tool_iterations {
            if max_iters == 0 {
                anyhow::bail!("core.max_tool_iterations must be > 0");
            }
        }

        if let Some(max_iters) = self.core.agent_loop.max_tool_iterations {
            if max_iters == 0 {
                anyhow::bail!("core.agent_loop.max_tool_iterations must be > 0");
            }
        }
        if self.core.agent_loop.auto_continue_max == 0 {
            anyhow::bail!("core.agent_loop.auto_continue_max must be > 0");
        }

        if self.core.tool_progress.interval_secs == 0 {
            anyhow::bail!("core.tool_progress.interval_secs must be > 0");
        }
        if self.core.tool_progress.max_updates == 0 {
            anyhow::bail!("core.tool_progress.max_updates must be > 0");
        }

        if self.core.streaming.enabled && self.core.streaming.mode != StreamingMode::Off {
            if self.core.streaming.flush_interval_ms < 300 {
                anyhow::bail!("core.streaming.flush_interval_ms must be >= 300");
            }
            if self.core.streaming.max_message_edits == 0 {
                anyhow::bail!("core.streaming.max_message_edits must be > 0");
            }
            if self.core.streaming.finalize_timeout_secs == 0 {
                anyhow::bail!("core.streaming.finalize_timeout_secs must be > 0");
            }
        }

        if self.core.cron.dispatch_concurrency == 0 {
            anyhow::bail!("core.cron.dispatch_concurrency must be > 0");
        }
        if self.core.cron.delivery_retry_backoff_secs == 0 {
            anyhow::bail!("core.cron.delivery_retry_backoff_secs must be > 0");
        }

        if let Some(mcp) = &self.mcp {
            for server in &mcp.servers {
                if server.timeout_secs == 0 {
                    anyhow::bail!("mcp.servers['{}'].timeout_secs must be > 0", server.name);
                }
                if server.startup_timeout_secs == 0 {
                    anyhow::bail!(
                        "mcp.servers['{}'].startup_timeout_secs must be > 0",
                        server.name
                    );
                }
                if server.healthcheck_interval_secs == 0 {
                    anyhow::bail!(
                        "mcp.servers['{}'].healthcheck_interval_secs must be > 0",
                        server.name
                    );
                }
            }
        }

        if self.updates.channel.trim().is_empty() {
            anyhow::bail!("updates.channel cannot be empty");
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

        if let Some(stt) = &self.stt {
            let engine = stt.engine.trim();
            if engine.is_empty() {
                anyhow::bail!("stt.engine cannot be empty");
            }
            if engine != "local_whisper_cpp" {
                anyhow::bail!(
                    "stt.engine '{}' is unsupported (supported: local_whisper_cpp)",
                    engine
                );
            }
            if stt.enabled {
                let model_path = stt
                    .local_model_path
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default();
                if model_path.is_empty() {
                    anyhow::bail!("stt.local_model_path is required when stt.enabled=true");
                }
                if let Some(threads) = stt.local_threads {
                    if threads == 0 || threads > 32 {
                        anyhow::bail!("stt.local_threads must be in range 1..=32");
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AccessMode, Config, DmPolicy, GroupPolicy, PermissionLevel, TelegramAccount};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn parse_config(input: &str) -> Config {
        let cfg: Config = toml::from_str(input).expect("valid TOML");
        cfg
    }

    fn write_temp_config(input: &str) -> std::path::PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("masix-config-test-{}.toml", ts));
        std::fs::write(&path, input).expect("write temp config");
        path
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
    fn load_rejects_legacy_group_mode_key() {
        let path = write_temp_config(
            r#"
[core]

[telegram]
[[telegram.accounts]]
bot_token = "123:abc"
group_mode = "all"

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        let err = Config::load(&path).expect_err("legacy key must be rejected");
        assert!(
            err.to_string().contains("legacy keys"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn load_rejects_legacy_auto_register_key() {
        let path = write_temp_config(
            r#"
[core]

[telegram]
[[telegram.accounts]]
bot_token = "123:abc"
auto_register_users = true

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        let err = Config::load(&path).expect_err("legacy key must be rejected");
        assert!(
            err.to_string().contains("legacy keys"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_file(path);
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

    fn make_policy_account() -> TelegramAccount {
        TelegramAccount {
            bot_token: "123:abc".to_string(),
            bot_name: None,
            bot_profile: None,
            allowed_chats: None,
            admins: vec![1],
            users: vec![2],
            readonly: vec![3],
            isolated: true,
            shared_memory_with: Vec::new(),
            allow_self_memory_edit: true,
            dm_policy: DmPolicy::Allowlist,
            dm_allow_from: vec![],
            access_mode: None,
            group_policy: GroupPolicy::Allowlist,
            group_require_mention: true,
            group_allow_known_untagged: false,
            group_allow_from: vec![],
            groups: std::collections::BTreeMap::new(),
            pairing: Default::default(),
            register_to_file: None,
            notify_admin_on_new_user: true,
            new_user_welcome_message: None,
            start_welcome_admin: None,
            start_welcome_user: None,
            user_tools_mode: Default::default(),
            user_allowed_tools: Vec::new(),
        }
    }

    #[test]
    fn access_mode_admin_only_registered_denies_unknown_group_user() {
        let mut account = make_policy_account();
        account.access_mode = Some(AccessMode::AdminOnlyRegistered);
        assert_eq!(
            account.get_permission_for_group(999, -100, true),
            PermissionLevel::None
        );
        assert_eq!(
            account.get_permission_for_group(2, -100, false),
            PermissionLevel::User
        );
    }

    #[test]
    fn access_mode_group_all_response_allows_unknown_group_user() {
        let mut account = make_policy_account();
        account.access_mode = Some(AccessMode::GroupAllResponse);
        assert_eq!(
            account.get_permission_for_group(999, -100, false),
            PermissionLevel::User
        );
    }

    #[test]
    fn uses_dm_pairing_is_true_for_assistant_mode() {
        let mut account = make_policy_account();
        account.pairing.enabled = true;
        account.access_mode = Some(AccessMode::AssistantAutoregister);
        account.dm_policy = DmPolicy::Allowlist;
        assert!(account.uses_dm_pairing());
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
    fn load_rejects_deprecated_whatsapp_section() {
        let path = std::env::temp_dir().join(format!(
            "masix-config-whatsapp-deprecated-{}.toml",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::write(
            &path,
            r#"
[core]

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[whatsapp]
enabled = true
"#,
        )
        .expect("write temp config");

        let result = Config::load(&path);
        std::fs::remove_file(path).ok();
        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_duplicate_telegram_account_tags() {
        let cfg = parse_config(
            r#"
[core]

[telegram]
[[telegram.accounts]]
bot_token = "123:abc"
[[telegram.accounts]]
bot_token = "123:def"

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_primary_provider_in_fallback_chain() {
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
[[bots.profiles]]
name = "ops"
workdir = "~/.masix/bots/ops"
memory_file = "~/.masix/bots/ops/MEMORY.md"
provider_primary = "openai"
provider_fallback = ["openai", "openrouter"]
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_updates_channel() {
        let cfg = parse_config(
            r#"
[core]

[updates]
channel = ""

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_accepts_local_stt_configuration() {
        let cfg = parse_config(
            r#"
[core]

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[stt]
enabled = true
engine = "local_whisper_cpp"
local_model_path = "~/.masix/models/whisper/whisper_base.bin"
local_threads = 2
local_language = "it"
"#,
        );
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_stt_enabled_without_model_path() {
        let cfg = parse_config(
            r#"
[core]

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[stt]
enabled = true
engine = "local_whisper_cpp"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_max_tool_iterations() {
        let cfg = parse_config(
            r#"
[core]
max_tool_iterations = 0

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_mcp_server_timeout() {
        let cfg = parse_config(
            r#"
[core]

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[mcp]
[[mcp.servers]]
name = "test"
command = "echo"
args = []
timeout_secs = 0
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_agent_loop_auto_continue_max() {
        let cfg = parse_config(
            r#"
[core]
[core.agent_loop]
auto_continue_max = 0

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_tool_progress_max_updates() {
        let cfg = parse_config(
            r#"
[core]
[core.tool_progress]
max_updates = 0

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_streaming_too_fast_flush() {
        let cfg = parse_config(
            r#"
[core]
[core.streaming]
enabled = true
mode = "telegram_edit"
flush_interval_ms = 100

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_mcp_startup_timeout() {
        let cfg = parse_config(
            r#"
[core]

[providers]
default_provider = "openai"

[[providers.providers]]
name = "openai"
api_key = "k"

[mcp]
[[mcp.servers]]
name = "test"
command = "echo"
args = []
timeout_secs = 30
startup_timeout_secs = 0
healthcheck_interval_secs = 60
"#,
        );
        assert!(cfg.validate().is_err());
    }
}
