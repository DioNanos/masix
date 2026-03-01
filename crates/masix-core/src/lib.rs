//! Masix Core
//!
//! Main runtime orchestration with MCP + Cron + LLM support

mod builtin_tools;

use anyhow::{anyhow, Result};
use base64::Engine;
use builtin_tools::{execute_builtin_tool, get_builtin_tool_definitions, is_builtin_tool};
#[cfg(feature = "stt")]
use masix_config::SttConfig;
use masix_config::{Config, GroupMode, PermissionLevel, RetryPolicyConfig, UserToolsMode};
use masix_exec::{
    is_termux_environment, manage_termux_boot, manage_termux_wake_lock, run_command, BootAction,
    ExecMode, ExecPolicy, WakeLockAction,
};
use masix_ipc::{Envelope, EventBus, MessageKind, OutboundMessage};
use masix_mcp::McpClient;
use masix_policy::PolicyEngine;
use masix_providers::{
    AnthropicProvider, ChatMessage, OpenAICompatibleProvider, Provider, ProviderRouter,
    RetryPolicy, ToolCall, ToolDefinition,
};
use masix_storage::Storage;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
#[cfg(feature = "stt")]
use tokio::process::Command as TokioCommand;
use tokio::sync::{broadcast, Mutex, Semaphore};
use tracing::{debug, error, info, warn};

use masix_telegram::menu::Language;

#[cfg(feature = "sms")]
use std::hash::{Hash, Hasher};

const MAX_TOOL_ITERATIONS: usize = 5;
const MEMORY_MAX_CONTEXT_ENTRIES: usize = 12;
const MAX_INBOUND_CONCURRENCY: usize = 8;

/// Check if a tool belongs to an admin-only MCP server.
/// Tool names are formatted as `{server_name}_{tool_name}`.
/// This function checks if the server_name matches a known admin-only module.
fn is_admin_only_tool(tool_name: &str, admin_only_modules: &HashSet<String>) -> bool {
    // Builtin tools are never admin-only (they have their own permission model)
    if is_builtin_tool(tool_name) {
        return false;
    }

    // MCP tools are prefixed with server_name_
    let parts: Vec<&str> = tool_name.splitn(2, '_').collect();
    if parts.len() == 2 {
        let server_name = parts[0];
        // Check both hyphen and underscore variants
        let with_hyphens = server_name.replace('_', "-");
        let with_underscores = server_name.replace('-', "_");
        return admin_only_modules.contains(server_name)
            || admin_only_modules.contains(&with_hyphens)
            || admin_only_modules.contains(&with_underscores);
    }

    false
}

/// Load admin-only plugin IDs from the installed plugin registry.
fn load_admin_only_modules(data_dir: &Path) -> HashSet<String> {
    let registry_path = data_dir.join("plugins").join("installed.json");
    if !registry_path.exists() {
        return HashSet::new();
    }

    #[derive(serde::Deserialize, Default)]
    struct PluginRegistry {
        #[serde(default)]
        plugins: Vec<InstalledPluginInfo>,
    }

    #[derive(serde::Deserialize)]
    struct InstalledPluginInfo {
        plugin_id: String,
        #[serde(default)]
        enabled: bool,
        #[serde(default)]
        admin_only: bool,
    }

    std::fs::read_to_string(&registry_path)
        .ok()
        .and_then(|content| serde_json::from_str::<PluginRegistry>(&content).ok())
        .map(|registry| {
            registry
                .plugins
                .into_iter()
                .filter(|p| p.enabled && p.admin_only)
                .map(|p| p.plugin_id)
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait::async_trait]
pub trait ToolProvider: Send + Sync {
    fn namespace(&self) -> &str;
    async fn list_tools(&self) -> Result<Vec<ToolDefinition>>;
    async fn call_tool(&self, tool_name: &str, arguments: serde_json::Value) -> Result<String>;
}

#[async_trait::async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn channel_name(&self) -> &str;
    async fn start(&self, _event_bus: EventBus) -> Result<()>;
}

#[derive(Debug, Clone, Default)]
struct DynamicAcl {
    admins: HashSet<i64>,
    users: HashSet<i64>,
    readonly: HashSet<i64>,
    user_tools_mode: Option<UserToolsMode>,
    user_allowed_tools: Option<HashSet<String>>,
}

#[derive(Debug, Clone)]
enum RuntimeToolAccess {
    None,
    All,
    Selected(HashSet<String>),
}

struct TypingHeartbeat {
    stop: Arc<AtomicBool>,
}

impl Drop for TypingHeartbeat {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl RuntimeToolAccess {
    fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }

    fn allows_tool(&self, tool_name: &str) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Selected(allowed) => {
                let normalized = tool_name.trim().to_lowercase();
                allowed.contains(&normalized)
            }
        }
    }

    fn filter_tools(&self, tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
        match self {
            Self::None => Vec::new(),
            Self::All => tools,
            Self::Selected(allowed) => tools
                .into_iter()
                .filter(|tool| allowed.contains(&tool.function.name.trim().to_lowercase()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_admin_only_tool, load_admin_only_modules, MasixRuntime};
    use masix_config::{Config, GroupMode, TelegramAccount, TelegramConfig};
    use masix_ipc::{Envelope, MessageKind};
    use masix_providers::ChatMessage;
    use masix_storage::Storage;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::{broadcast, Mutex};

    fn make_account(token: &str) -> TelegramAccount {
        TelegramAccount {
            bot_token: token.to_string(),
            bot_name: None,
            allowed_chats: None,
            bot_profile: None,
            admins: vec![],
            users: vec![],
            readonly: vec![],
            isolated: true,
            shared_memory_with: vec![],
            allow_self_memory_edit: true,
            group_mode: GroupMode::All,
            auto_register_users: false,
            register_to_file: None,
            user_tools_mode: masix_config::UserToolsMode::None,
            user_allowed_tools: vec![],
        }
    }

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("masix-core-{}-{}.db", name, ts))
    }

    #[test]
    fn get_telegram_account_routes_by_account_tag() {
        let config = Config {
            telegram: Some(TelegramConfig {
                poll_timeout_secs: Some(60),
                client_recreate_interval_secs: Some(60),
                default_policy: None,
                accounts: vec![make_account("111:AAA"), make_account("222:BBB")],
            }),
            ..Config::default()
        };

        let first = MasixRuntime::get_telegram_account(&config, Some("111"))
            .expect("account 111 should resolve");
        let second = MasixRuntime::get_telegram_account(&config, Some("222"))
            .expect("account 222 should resolve");

        assert_eq!(first.bot_token, "111:AAA");
        assert_eq!(second.bot_token, "222:BBB");
    }

    #[test]
    fn user_state_key_isolated_by_account_tag() {
        let key_a = MasixRuntime::user_state_key(Some("111"), Some("555"), Some(100));
        let key_b = MasixRuntime::user_state_key(Some("222"), Some("555"), Some(100));
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn tag_only_untagged_group_denial_is_silent_ignore() {
        let mut account = make_account("111:AAA");
        account.bot_name = Some("MyBot".to_string());
        account.group_mode = GroupMode::TagOnly;
        let config = Config {
            telegram: Some(TelegramConfig {
                poll_timeout_secs: Some(60),
                client_recreate_interval_secs: Some(60),
                default_policy: None,
                accounts: vec![account],
            }),
            ..Config::default()
        };

        assert!(
            MasixRuntime::should_silently_ignore_telegram_permission_denial(
                &config,
                Some("111"),
                123,
                -999,
                ""
            )
        );
    }

    #[test]
    fn admin_only_tool_detection() {
        let mut admin_modules = HashSet::new();
        admin_modules.insert("codex-backend".to_string());

        // MCP tools prefixed with server_name_ (hyphens preserved in server name)
        assert!(is_admin_only_tool("codex-backend_run", &admin_modules));
        assert!(is_admin_only_tool("codex-backend_list", &admin_modules));

        // Non-admin tools
        assert!(!is_admin_only_tool("discovery_web-search", &admin_modules));
        assert!(!is_admin_only_tool("other_server_tool", &admin_modules));

        // Builtin tools are never admin-only
        assert!(!is_admin_only_tool("shell", &admin_modules));
        assert!(!is_admin_only_tool("web", &admin_modules));
    }

    #[test]
    fn load_admin_only_modules_from_registry() {
        use std::io::Write;
        let temp_dir = std::env::temp_dir().join(format!(
            "masix-test-admin-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(temp_dir.join("plugins")).unwrap();

        let registry_content = r#"{
            "plugins": [
                {"plugin_id": "codex-backend", "version": "0.1.2", "enabled": true, "admin_only": true, "visibility": "free", "platform": "linux-x86_64", "source_server": "https://test", "install_path": "/tmp", "installed_at": 0},
                {"plugin_id": "discovery", "version": "0.2.0", "enabled": true, "admin_only": false, "visibility": "free", "platform": "linux-x86_64", "source_server": "https://test", "install_path": "/tmp", "installed_at": 0},
                {"plugin_id": "admin-module-disabled", "version": "1.0.0", "enabled": false, "admin_only": true, "visibility": "free", "platform": "linux-x86_64", "source_server": "https://test", "install_path": "/tmp", "installed_at": 0}
            ]
        }"#;
        let mut file =
            std::fs::File::create(temp_dir.join("plugins").join("installed.json")).unwrap();
        file.write_all(registry_content.as_bytes()).unwrap();

        let modules = load_admin_only_modules(&temp_dir);
        assert!(modules.contains("codex-backend"));
        assert!(!modules.contains("discovery"));
        assert!(!modules.contains("admin-module-disabled")); // Not enabled

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn users_only_group_denial_keeps_unauthorized_response() {
        let mut account = make_account("111:AAA");
        account.bot_name = Some("MyBot".to_string());
        account.group_mode = GroupMode::UsersOnly;
        let config = Config {
            telegram: Some(TelegramConfig {
                poll_timeout_secs: Some(60),
                client_recreate_interval_secs: Some(60),
                default_policy: None,
                accounts: vec![account],
            }),
            ..Config::default()
        };

        assert!(
            !MasixRuntime::should_silently_ignore_telegram_permission_denial(
                &config,
                Some("111"),
                123,
                -999,
                "hello group"
            )
        );
    }

    #[test]
    fn effective_register_file_defaults_by_account_tag() {
        let account = make_account("111:AAA");
        let register_file = MasixRuntime::effective_register_file(&account);
        assert_eq!(
            register_file,
            "~/.masix/accounts/telegram.111.register.json"
        );
    }

    #[test]
    fn effective_register_file_prefers_configured_value() {
        let mut account = make_account("111:AAA");
        account.register_to_file = Some("~/custom/register.json".to_string());
        let register_file = MasixRuntime::effective_register_file(&account);
        assert_eq!(register_file, "~/custom/register.json");
    }

    #[test]
    fn inbound_processing_scope_key_uses_chat_scope() {
        let envelope = Envelope::new(
            "telegram",
            MessageKind::Message {
                from: "123".to_string(),
                text: "ciao".to_string(),
            },
        )
        .with_chat_id(-100)
        .with_payload(serde_json::json!({
            "account_tag": "1234567890"
        }));

        let key = MasixRuntime::inbound_processing_scope_key(&envelope);
        assert_eq!(key, "telegram:1234567890:chat:-100");
    }

    #[test]
    fn detects_rate_limit_error_strings() {
        let err = anyhow::anyhow!("HTTP 429 Too Many Requests (Retry-After: 2)");
        assert!(MasixRuntime::is_rate_limit_error(&err));
        let non_rate = anyhow::anyhow!("HTTP 401 Unauthorized");
        assert!(!MasixRuntime::is_rate_limit_error(&non_rate));
    }

    #[tokio::test]
    async fn cron_check_dispatches_due_job_and_disables_one_shot() {
        let path = temp_db_path("cron-dispatch");
        let storage = Storage::new(&path).expect("storage");
        let due = (chrono::Utc::now() - chrono::Duration::seconds(2)).to_rfc3339();
        storage
            .create_cron_job(
                "test",
                &due,
                "telegram",
                "12345",
                None,
                "cron ping",
                "+00:00",
                false,
            )
            .expect("create cron job");

        let storage = Arc::new(Mutex::new(storage));
        let (tx, mut rx) = broadcast::channel(8);

        MasixRuntime::check_cron_jobs(&storage, &tx, Some("bot_default"))
            .await
            .expect("cron check");

        let out = rx.recv().await.expect("outbound");
        assert_eq!(out.channel, "telegram");
        assert_eq!(out.chat_id, 12345);
        assert_eq!(out.text, "cron ping");
        assert_eq!(out.account_tag.as_deref(), Some("bot_default"));

        let remaining = storage
            .lock()
            .await
            .list_enabled_cron_jobs()
            .expect("list jobs");
        assert!(remaining.is_empty());

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn cron_check_skips_non_numeric_recipient_and_disables_job() {
        let path = temp_db_path("cron-invalid-recipient");
        let storage = Storage::new(&path).expect("storage");
        let due = (chrono::Utc::now() - chrono::Duration::seconds(2)).to_rfc3339();
        storage
            .create_cron_job(
                "test",
                &due,
                "telegram",
                "@not_numeric",
                Some("bot_a"),
                "should not send",
                "+00:00",
                false,
            )
            .expect("create cron job");

        let storage = Arc::new(Mutex::new(storage));
        let (tx, mut rx) = broadcast::channel(8);

        MasixRuntime::check_cron_jobs(&storage, &tx, None)
            .await
            .expect("cron check");

        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));

        let remaining = storage
            .lock()
            .await
            .list_enabled_cron_jobs()
            .expect("list jobs");
        assert!(remaining.is_empty());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn discovery_payload_count_detects_json_and_numbered_lists() {
        let json_payload = r#"[{"title":"A"},{"title":"B"}]"#;
        assert_eq!(
            MasixRuntime::count_search_results_from_tool_payload(json_payload),
            2
        );

        let list_payload = "1. First result\n2. Second result\n3. Third result";
        assert_eq!(
            MasixRuntime::count_search_results_from_tool_payload(list_payload),
            3
        );

        let error_payload = "Tool error: Server not found";
        assert_eq!(
            MasixRuntime::count_search_results_from_tool_payload(error_payload),
            0
        );
    }

    #[test]
    fn sanitize_false_search_unavailable_claims_removes_wrong_outage_lines() {
        let response = "## Ricerca\n⚠️ Ricerca web non disponibile (server non trovato).\nEcco i risultati trovati.";
        let (sanitized, changed) = MasixRuntime::sanitize_false_search_unavailable_claims(response);

        assert!(changed);
        assert!(!sanitized.to_lowercase().contains("server non trovato"));
        assert!(sanitized.contains("Ecco i risultati trovati."));
        assert!(sanitized.contains("Web search succeeded in this turn"));
    }

    #[test]
    fn synthesize_from_tool_messages_extracts_non_error_payloads() {
        let messages = vec![
            ChatMessage {
                role: "tool".to_string(),
                content: Some("Error: timeout".to_string()),
                tool_calls: None,
                tool_call_id: Some("1".to_string()),
                name: Some("plugin_discovery_web_search".to_string()),
            },
            ChatMessage {
                role: "tool".to_string(),
                content: Some("1. Result A\n2. Result B".to_string()),
                tool_calls: None,
                tool_call_id: Some("2".to_string()),
                name: Some("plugin_discovery_web_search".to_string()),
            },
        ];

        let out = MasixRuntime::synthesize_from_tool_messages(&messages).unwrap();
        assert!(out.contains("Raw findings"));
        assert!(out.contains("Result A"));
    }
}

#[derive(Debug, Clone)]
struct BotContext {
    profile_name: String,
    workdir: PathBuf,
    memory_dir: PathBuf,
    memory_file: PathBuf,
    provider_chain: Vec<String>,
    vision_provider: Option<String>,
    retry_policy: RetryPolicy,
    exec_policy: ExecPolicy,
}

/// Result of the LLM tool execution loop
struct LlmLoopResult {
    final_response: String,
    used_tools: Vec<String>,
    successful_discovery_search_calls: usize,
}

/// Context for LLM message building
struct LlmMessagesResult {
    messages: Vec<ChatMessage>,
    user_message: String,
    vision_analysis: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ChatMemoryEntry {
    role: String,
    content: String,
    ts: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct UserMemoryMeta {
    account_tag: String,
    user_id: String,
    first_seen: String,
    last_seen: String,
    channels: Vec<String>,
    chat_ids: Vec<i64>,
}

#[derive(Debug, serde::Deserialize)]
struct TelegramGetFileResponse {
    ok: bool,
    result: Option<TelegramGetFileResult>,
    description: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct TelegramGetFileResult {
    file_path: String,
}

#[derive(Debug, Clone)]
struct MediaFileReference {
    file_id: String,
    mime_type: String,
    #[cfg_attr(not(feature = "stt"), allow(dead_code))]
    file_name: Option<String>,
    caption: Option<String>,
}

pub struct MasixRuntime {
    config: Config,
    storage: Arc<Mutex<Storage>>,
    policy: PolicyEngine,
    provider_router: Arc<ProviderRouter>,
    mcp_client: Option<Arc<Mutex<McpClient>>>,
    event_bus: EventBus,
    system_prompt: String,
    user_languages: Arc<Mutex<HashMap<String, Language>>>,
    user_providers: Arc<Mutex<HashMap<String, String>>>,
    user_models: Arc<Mutex<HashMap<String, String>>>,
}

impl MasixRuntime {
    pub fn new(config: Config, storage: Storage) -> Result<Self> {
        let policy = PolicyEngine::new(config.policy.as_ref());

        let mut provider_router = ProviderRouter::new(config.providers.default_provider.clone());

        for provider_config in &config.providers.providers {
            let provider_type = provider_config.provider_type.as_deref().unwrap_or("openai");

            let provider: Box<dyn Provider> = match provider_type {
                "anthropic" => Box::new(AnthropicProvider::new(
                    provider_config.name.clone(),
                    provider_config.api_key.clone(),
                    provider_config.base_url.clone(),
                    provider_config.model.clone(),
                )),
                _ => Box::new(OpenAICompatibleProvider::new(
                    provider_config.name.clone(),
                    provider_config.api_key.clone(),
                    provider_config.base_url.clone(),
                    provider_config.model.clone(),
                )),
            };
            provider_router.add_provider(provider);
        }

        let mcp_client = if let Some(mcp_config) = &config.mcp {
            if mcp_config.enabled {
                Some(Arc::new(Mutex::new(McpClient::new())))
            } else {
                None
            }
        } else {
            None
        };

        let system_prompt = Self::load_soul(&config).unwrap_or_else(|| {
            "Sei un assistente utile. Rispondi in italiano quando possibile.".to_string()
        });

        Ok(Self {
            config,
            storage: Arc::new(Mutex::new(storage)),
            policy,
            provider_router: Arc::new(provider_router),
            mcp_client,
            event_bus: EventBus::new(),
            system_prompt,
            user_languages: Arc::new(Mutex::new(HashMap::new())),
            user_providers: Arc::new(Mutex::new(HashMap::new())),
            user_models: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn load_soul(config: &Config) -> Option<String> {
        let soul_path = config.core.soul_file.as_ref()?;

        if let Ok(content) = std::fs::read_to_string(soul_path) {
            info!("Loaded SOUL.md from {}", soul_path);
            Some(format!(
                "Sei un assistente AI. Ecco la tua identità e valori:\n\n{}\n\nRispondi in italiano quando possibile.",
                content
            ))
        } else {
            warn!("Failed to load SOUL.md from {}", soul_path);
            None
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Masix runtime starting...");

        self.init_mcp_servers().await;

        let outbound_sender = self.event_bus.outbound_sender();
        let base_data_dir = self.get_data_dir()?;
        let bot_contexts = Arc::new(self.build_bot_contexts(&base_data_dir)?);

        // Load admin-only module IDs from plugin registry
        let admin_only_modules = Arc::new(load_admin_only_modules(&base_data_dir));
        if !admin_only_modules.is_empty() {
            info!(
                "Admin-only modules loaded: {:?}",
                admin_only_modules.iter().collect::<Vec<_>>()
            );
        }

        self.start_telegram_adapters(Arc::clone(&bot_contexts))
            .await?;
        self.start_whatsapp_adapter().await?;
        self.start_sms_adapter().await?;

        let mut inbound_rx = self.event_bus.subscribe();
        let outbound_for_processor = outbound_sender.clone();
        let provider_router = Arc::clone(&self.provider_router);
        let storage_for_processor = Arc::clone(&self.storage);
        let mcp_client = self.mcp_client.clone();
        let system_prompt = self.system_prompt.clone();
        let policy = self.policy.clone();
        let rate_state: Arc<Mutex<HashMap<String, (i64, u32)>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let inbound_scope_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let inbound_semaphore = Arc::new(Semaphore::new(MAX_INBOUND_CONCURRENCY));
        let bot_contexts_for_processor = Arc::clone(&bot_contexts);
        let default_cron_account_tag = self.default_telegram_account_tag();
        let user_languages_for_processor = Arc::clone(&self.user_languages);
        let user_providers_for_processor = Arc::clone(&self.user_providers);
        let user_models_for_processor = Arc::clone(&self.user_models);
        let config_for_processor = self.config.clone();
        let admin_only_modules_for_processor = Arc::clone(&admin_only_modules);

        tokio::spawn(async move {
            let mut cron_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

            loop {
                tokio::select! {
                    result = inbound_rx.recv() => {
                        match result {
                            Ok(envelope) => {
                                let outbound = outbound_for_processor.clone();
                                let provider_router = Arc::clone(&provider_router);
                                let storage = Arc::clone(&storage_for_processor);
                                let mcp_client = mcp_client.clone();
                                let system_prompt = system_prompt.clone();
                                let policy = policy.clone();
                                let rate_state = Arc::clone(&rate_state);
                                let bot_contexts = Arc::clone(&bot_contexts_for_processor);
                                let user_languages = Arc::clone(&user_languages_for_processor);
                                let user_providers = Arc::clone(&user_providers_for_processor);
                                let user_models = Arc::clone(&user_models_for_processor);
                                let config = config_for_processor.clone();
                                let admin_only_modules = Arc::clone(&admin_only_modules_for_processor);
                                let semaphore = Arc::clone(&inbound_semaphore);
                                let scope_locks = Arc::clone(&inbound_scope_locks);
                                let scope_key = Self::inbound_processing_scope_key(&envelope);
                                let trace_id = envelope.trace_id.clone();

                                tokio::spawn(async move {
                                    let _permit = match semaphore.acquire_owned().await {
                                        Ok(permit) => permit,
                                        Err(err) => {
                                            error!(
                                                "Inbound worker semaphore closed (trace_id={}): {}",
                                                trace_id, err
                                            );
                                            return;
                                        }
                                    };

                                    let scope_lock = Self::get_or_create_inbound_scope_lock(
                                        &scope_locks,
                                        &scope_key,
                                    )
                                    .await;
                                    let _scope_guard = scope_lock.lock_owned().await;

                                    if let Err(e) = Self::process_inbound_message(
                                        envelope,
                                        outbound,
                                        provider_router.as_ref(),
                                        &storage,
                                        &mcp_client,
                                        &system_prompt,
                                        &policy,
                                        &rate_state,
                                        &bot_contexts,
                                        &user_languages,
                                        &user_providers,
                                        &user_models,
                                        &config,
                                        &admin_only_modules,
                                    )
                                    .await
                                    {
                                        error!(
                                            "Error processing inbound message (trace_id={}): {}",
                                            trace_id, e
                                        );
                                    }
                                });
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                info!("Event bus closed, stopping message processor");
                                break;
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!("Event bus lagged by {} messages", n);
                            }
                        }
                    }
                    _ = cron_interval.tick() => {
                        if let Err(e) = Self::check_cron_jobs(
                            &storage_for_processor,
                            &outbound_for_processor,
                            default_cron_account_tag.as_deref(),
                        ).await {
                            error!("Error checking cron jobs: {}", e);
                        }
                    }
                }
            }
        });

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            info!("Masix runtime heartbeat");
        }
    }

    async fn check_cron_jobs(
        storage: &Arc<Mutex<Storage>>,
        outbound_sender: &broadcast::Sender<OutboundMessage>,
        default_account_tag: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();

        let storage_guard = storage.lock().await;
        let jobs = storage_guard.get_due_cron_jobs(&now)?;
        drop(storage_guard);

        for job in jobs {
            info!("Executing cron job {}: {}", job.id, job.message);

            let channel = job.channel.clone();
            let recipient = job.recipient.clone();
            let message = job.message.clone();
            let account_tag = if job.account_tag == "__default__" || job.account_tag.is_empty() {
                default_account_tag.map(|tag| tag.to_string())
            } else {
                Some(job.account_tag.clone())
            };

            if let Ok(chat_id) = recipient.parse::<i64>() {
                let msg = OutboundMessage {
                    channel,
                    account_tag,
                    chat_id,
                    text: message,
                    reply_to: None,
                    edit_message_id: None,
                    inline_keyboard: None,
                    chat_action: None,
                };

                let _ = outbound_sender.send(msg);
            } else {
                warn!(
                    "Skipping cron job {}: non-numeric recipient '{}' for channel '{}'",
                    job.id, recipient, channel
                );
            }

            let storage_guard = storage.lock().await;
            if job.recurring {
                storage_guard.update_cron_next_run(job.id, &job.schedule, &job.timezone)?;
            } else {
                storage_guard.disable_cron_job(job.id)?;
            }
        }

        Ok(())
    }

    async fn init_mcp_servers(&self) {
        if let Some(mcp_client) = &self.mcp_client {
            if let Some(mcp_config) = &self.config.mcp {
                if mcp_config.enabled {
                    let mut client = mcp_client.lock().await;
                    for server in &mcp_config.servers {
                        match client
                            .add_server(
                                server.name.clone(),
                                server.command.clone(),
                                server.args.clone(),
                            )
                            .await
                        {
                            Ok(_) => info!("MCP server '{}' started", server.name),
                            Err(e) => error!("Failed to start MCP server '{}': {}", server.name, e),
                        }
                    }
                }
            }
        }
    }

    async fn start_telegram_adapters(
        &self,
        bot_contexts: Arc<HashMap<String, BotContext>>,
    ) -> Result<()> {
        if let Some(telegram_config) = &self.config.telegram {
            let data_dir = self.get_data_dir()?;
            let poll_timeout = telegram_config.poll_timeout_secs;
            let recreate_interval = telegram_config.client_recreate_interval_secs;
            let mut seen_account_tags: HashSet<String> = HashSet::new();

            for (idx, account) in telegram_config.accounts.iter().enumerate() {
                let account_tag = Self::account_tag_from_token(&account.bot_token);
                if !seen_account_tags.insert(account_tag.clone()) {
                    warn!(
                        "Skipping duplicate Telegram account entry for account tag '{}'",
                        account_tag
                    );
                    continue;
                }
                info!("Telegram adapter enabled for account #{}", idx + 1);

                let account_clone = account.clone();
                let data_dir_clone = bot_contexts
                    .get(&account_tag)
                    .map(|ctx| ctx.workdir.clone())
                    .unwrap_or_else(|| data_dir.clone());
                let event_bus = self.event_bus.clone();

                tokio::spawn(async move {
                    let outbound_rx = event_bus.outbound_subscribe();
                    let adapter = masix_telegram::TelegramAdapter::new(
                        &account_clone,
                        data_dir_clone.clone(),
                        poll_timeout,
                        recreate_interval,
                    )
                    .with_event_bus(event_bus);

                    let adapter_for_outbound = masix_telegram::TelegramAdapter::new(
                        &account_clone,
                        data_dir_clone,
                        poll_timeout,
                        recreate_interval,
                    );

                    tokio::spawn(async move {
                        adapter_for_outbound.run_outbound_handler(outbound_rx).await;
                    });

                    if let Err(e) = adapter.poll().await {
                        error!("Telegram adapter failed: {}", e);
                    }
                });
            }
        }
        Ok(())
    }

    #[cfg(feature = "whatsapp")]
    async fn start_whatsapp_adapter(&self) -> Result<()> {
        if let Some(whatsapp_config) = &self.config.whatsapp {
            if whatsapp_config.enabled {
                info!("WhatsApp adapter enabled (read-only)");
                let adapter = masix_whatsapp::WhatsAppAdapter::from_config(whatsapp_config)
                    .with_event_bus(self.event_bus.clone());
                tokio::spawn(async move {
                    if let Err(err) = adapter.start().await {
                        error!("WhatsApp adapter failed: {}", err);
                    }
                });
            }
        }
        Ok(())
    }

    #[cfg(not(feature = "whatsapp"))]
    async fn start_whatsapp_adapter(&self) -> Result<()> {
        if self
            .config
            .whatsapp
            .as_ref()
            .map(|cfg| cfg.enabled)
            .unwrap_or(false)
        {
            warn!("WhatsApp support not compiled in. Rebuild with --features whatsapp");
        }
        Ok(())
    }

    #[cfg(feature = "sms")]
    async fn start_sms_adapter(&self) -> Result<()> {
        if let Some(sms_config) = &self.config.sms {
            if sms_config.enabled {
                if !is_termux_environment() {
                    warn!(
                        "SMS adapter is enabled in config but current platform is not Termux; skipping SMS watcher."
                    );
                    return Ok(());
                }

                let watch_interval = sms_config.watch_interval_secs.unwrap_or(30).max(1);
                let event_bus = self.event_bus.clone();
                info!("SMS adapter enabled (watch interval: {}s)", watch_interval);

                tokio::spawn(async move {
                    let adapter = masix_sms::SmsAdapter::new(Some(watch_interval));
                    let mut last_seen_ts: i64 = 0;

                    loop {
                        match adapter.list_sms(20).await {
                            Ok(mut messages) => {
                                messages.sort_by_key(|msg| msg.date);
                                for msg in messages {
                                    if msg.date <= last_seen_ts {
                                        continue;
                                    }
                                    last_seen_ts = msg.date;

                                    let sender = msg.address.trim().to_string();
                                    if sender.is_empty() {
                                        continue;
                                    }

                                    let envelope = Envelope::new(
                                        "sms",
                                        MessageKind::Message {
                                            from: sender.clone(),
                                            text: msg.body.clone(),
                                        },
                                    )
                                    .with_chat_id(Self::virtual_chat_id_from_sender(&sender))
                                    .with_payload(serde_json::json!({
                                        "source": "termux-sms",
                                        "ts": msg.date,
                                        "read": msg.read,
                                    }));

                                    if let Err(err) = event_bus.publish(envelope) {
                                        warn!("Failed to publish SMS event: {}", err);
                                    }
                                }
                            }
                            Err(err) => warn!("SMS watcher poll failed: {}", err),
                        }

                        tokio::time::sleep(tokio::time::Duration::from_secs(watch_interval)).await;
                    }
                });
            }
        }
        Ok(())
    }

    #[cfg(not(feature = "sms"))]
    async fn start_sms_adapter(&self) -> Result<()> {
        if self
            .config
            .sms
            .as_ref()
            .map(|cfg| cfg.enabled)
            .unwrap_or(false)
        {
            warn!("SMS support not compiled in. Rebuild with --features sms");
        }
        Ok(())
    }

    fn build_bot_contexts(&self, base_data_dir: &Path) -> Result<HashMap<String, BotContext>> {
        let mut contexts = HashMap::new();
        let default_context = self.default_bot_context(base_data_dir)?;
        contexts.insert("__default__".to_string(), default_context.clone());

        let mut profile_map: HashMap<String, &masix_config::BotProfileConfig> = HashMap::new();
        if let Some(bots) = &self.config.bots {
            for profile in &bots.profiles {
                profile_map.insert(profile.name.clone(), profile);
            }
        }

        if let Some(telegram) = &self.config.telegram {
            for account in &telegram.accounts {
                let account_tag = Self::account_tag_from_token(&account.bot_token);
                if contexts.contains_key(&account_tag) {
                    warn!(
                        "Duplicate Telegram account tag '{}' detected in config; reusing first context",
                        account_tag
                    );
                    continue;
                }
                let context = if let Some(profile_name) = &account.bot_profile {
                    if let Some(profile) = profile_map.get(profile_name) {
                        let profile_root =
                            Self::resolve_path_with_base(&profile.workdir, base_data_dir)?;
                        let workdir = Self::scoped_account_workdir(&profile_root, &account_tag);
                        let memory_file =
                            Self::scoped_account_memory_file(&profile.memory_file, &workdir);
                        let mut provider_chain = vec![profile.provider_primary.clone()];
                        provider_chain.extend(profile.provider_fallback.clone());

                        BotContext {
                            profile_name: format!("{}/{}", profile.name, account_tag),
                            memory_dir: workdir.join("memory"),
                            workdir,
                            memory_file,
                            provider_chain,
                            vision_provider: profile.vision_provider.clone(),
                            retry_policy: Self::retry_policy_from_config(profile.retry.as_ref()),
                            exec_policy: Self::exec_policy_from_config(self.config.exec.as_ref()),
                        }
                    } else {
                        self.default_account_bot_context(base_data_dir, &account_tag)?
                    }
                } else {
                    self.default_account_bot_context(base_data_dir, &account_tag)?
                };

                Self::ensure_bot_context_dirs(&context)?;
                contexts.insert(account_tag, context);
            }
        }

        Ok(contexts)
    }

    fn default_bot_context(&self, base_data_dir: &Path) -> Result<BotContext> {
        let workdir = base_data_dir.to_path_buf();
        let memory_file = if let Some(path) = &self.config.core.global_memory_file {
            Self::resolve_path_with_base(path, base_data_dir)?
        } else {
            workdir.join("MEMORY.md")
        };
        let context = BotContext {
            profile_name: "default".to_string(),
            workdir: workdir.clone(),
            memory_dir: workdir.join("memory/default"),
            memory_file,
            provider_chain: vec![self.config.providers.default_provider.clone()],
            vision_provider: None,
            retry_policy: RetryPolicy::default(),
            exec_policy: Self::exec_policy_from_config(self.config.exec.as_ref()),
        };
        Self::ensure_bot_context_dirs(&context)?;
        Ok(context)
    }

    fn default_account_bot_context(
        &self,
        base_data_dir: &Path,
        account_tag: &str,
    ) -> Result<BotContext> {
        let workdir = Self::scoped_account_workdir(base_data_dir, account_tag);
        let context = BotContext {
            profile_name: format!("default/{}", account_tag),
            workdir: workdir.clone(),
            memory_dir: workdir.join("memory"),
            memory_file: workdir.join("MEMORY.md"),
            provider_chain: vec![self.config.providers.default_provider.clone()],
            vision_provider: None,
            retry_policy: RetryPolicy::default(),
            exec_policy: Self::exec_policy_from_config(self.config.exec.as_ref()),
        };
        Self::ensure_bot_context_dirs(&context)?;
        Ok(context)
    }

    fn scoped_account_workdir(base: &Path, account_tag: &str) -> PathBuf {
        base.join("accounts")
            .join(Self::sanitize_scope_component(account_tag))
    }

    fn scoped_account_memory_file(memory_file: &str, account_workdir: &Path) -> PathBuf {
        let template = PathBuf::from(memory_file);
        if memory_file == "~" || memory_file.starts_with("~/") || template.is_absolute() {
            let fallback = "MEMORY.md".to_string();
            let filename = template
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(fallback.as_str());
            account_workdir.join(filename)
        } else {
            account_workdir.join(template)
        }
    }

    fn ensure_bot_context_dirs(context: &BotContext) -> Result<()> {
        std::fs::create_dir_all(&context.workdir)?;
        std::fs::create_dir_all(&context.memory_dir)?;
        if let Some(parent) = context.memory_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !context.memory_file.exists() {
            std::fs::write(&context.memory_file, "")?;
        }
        Ok(())
    }

    fn resolve_path_with_base(path: &str, base: &Path) -> Result<PathBuf> {
        if path == "~" || path.starts_with("~/") {
            let home =
                dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
            if path == "~" {
                Ok(home)
            } else {
                Ok(home.join(path.trim_start_matches("~/")))
            }
        } else {
            let p = PathBuf::from(path);
            if p.is_absolute() {
                Ok(p)
            } else {
                Ok(base.join(p))
            }
        }
    }

    fn retry_policy_from_config(config: Option<&RetryPolicyConfig>) -> RetryPolicy {
        let default = RetryPolicy::default();
        if let Some(cfg) = config {
            RetryPolicy {
                window_secs: cfg.window_secs.unwrap_or(default.window_secs),
                initial_delay_secs: cfg.initial_delay_secs.unwrap_or(default.initial_delay_secs),
                backoff_factor: cfg.backoff_factor.unwrap_or(default.backoff_factor),
                max_delay_secs: cfg.max_delay_secs.unwrap_or(default.max_delay_secs),
            }
        } else {
            default
        }
    }

    fn exec_policy_from_config(config: Option<&masix_config::ExecConfig>) -> ExecPolicy {
        let mut policy = ExecPolicy::default();
        if let Some(cfg) = config {
            policy.enabled = cfg.enabled.unwrap_or(policy.enabled);
            policy.allow_base = cfg.allow_base.unwrap_or(policy.allow_base);
            policy.allow_termux = cfg.allow_termux.unwrap_or(policy.allow_termux);
            policy.timeout_secs = cfg.timeout_secs.unwrap_or(policy.timeout_secs);
            policy.max_output_chars = cfg.max_output_chars.unwrap_or(policy.max_output_chars);
            if !cfg.base_allowlist.is_empty() {
                policy.base_allowlist = cfg.base_allowlist.clone();
            }
            if !cfg.termux_allowlist.is_empty() {
                policy.termux_allowlist = cfg.termux_allowlist.clone();
            }
        }
        policy
    }

    fn account_tag_from_token(token: &str) -> String {
        token.split(':').next().unwrap_or("default").to_string()
    }

    fn inbound_processing_scope_key(envelope: &Envelope) -> String {
        let account_tag = envelope
            .payload
            .get("account_tag")
            .and_then(|v| v.as_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("__default__");
        let scope = if let Some(chat_id) = envelope.chat_id {
            format!("chat:{}", chat_id)
        } else {
            match &envelope.kind {
                MessageKind::Message { from, .. } => format!("sender:{}", from.trim()),
                MessageKind::Callback { query_id, .. } => format!("callback:{}", query_id.trim()),
                MessageKind::Command { name, .. } => format!("command:{}", name.trim()),
                MessageKind::Reply { to, .. } => format!("reply:{}", to.trim()),
                MessageKind::Error { code, .. } => format!("error:{}", code),
            }
        };
        format!("{}:{}:{}", envelope.channel, account_tag, scope)
    }

    async fn get_or_create_inbound_scope_lock(
        scope_locks: &Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
        scope_key: &str,
    ) -> Arc<Mutex<()>> {
        let mut locks = scope_locks.lock().await;
        locks
            .entry(scope_key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn default_telegram_account_tag(&self) -> Option<String> {
        self.config.telegram.as_ref().and_then(|telegram| {
            telegram
                .accounts
                .first()
                .map(|account| Self::account_tag_from_token(&account.bot_token))
        })
    }

    fn default_telegram_account_tag_from_config(config: &Config) -> Option<String> {
        config.telegram.as_ref().and_then(|telegram| {
            telegram
                .accounts
                .first()
                .map(|account| Self::account_tag_from_token(&account.bot_token))
        })
    }

    #[cfg(feature = "sms")]
    fn virtual_chat_id_from_sender(sender: &str) -> i64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        sender.hash(&mut hasher);
        let raw = hasher.finish() & 0x7FFF_FFFF_FFFF_FFFF;
        -((raw.max(1)) as i64)
    }

    fn resolve_bot_context(
        bot_contexts: &Arc<HashMap<String, BotContext>>,
        account_tag: Option<&str>,
    ) -> BotContext {
        if let Some(tag) = account_tag {
            if let Some(ctx) = bot_contexts.get(tag) {
                return ctx.clone();
            }
        }
        bot_contexts
            .get("__default__")
            .cloned()
            .unwrap_or(BotContext {
                profile_name: "default".to_string(),
                workdir: PathBuf::from("."),
                memory_dir: PathBuf::from("./memory/default"),
                memory_file: PathBuf::from("./MEMORY.md"),
                provider_chain: vec!["openai".to_string()],
                vision_provider: None,
                retry_policy: RetryPolicy::default(),
                exec_policy: ExecPolicy::default(),
            })
    }

    /// Build LLM messages including system context, memory, media analysis, and history.
    #[allow(clippy::too_many_arguments)]
    async fn build_llm_messages(
        system_prompt: &str,
        bot_context: &BotContext,
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
        text: &str,
        payload: &serde_json::Value,
        tools: &[ToolDefinition],
        has_tools: bool,
        config: &Config,
    ) -> Result<LlmMessagesResult> {
        // Build system context
        let mut system_context = system_prompt.to_string();
        if let Some(memory) = Self::load_bot_memory_file(bot_context).await {
            if !memory.trim().is_empty() {
                system_context.push_str("\n\n# Bot Memory\n");
                system_context.push_str(&memory);
            }
        }
        if has_tools {
            system_context.push_str(&Self::build_tool_call_guidance(tools));
        }

        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: Some(system_context),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }];

        // Build user message with media enrichment
        let mut user_message = Self::enrich_user_message_with_media(text, payload);

        // STT transcription
        let stt_transcript =
            match Self::transcribe_media_with_local_stt(config, account_tag, payload).await {
                Ok(result) => result,
                Err(e) => {
                    warn!(
                        "Local STT failed for profile '{}': {}",
                        bot_context.profile_name, e
                    );
                    None
                }
            };
        if let Some(transcript) = &stt_transcript {
            user_message.push_str("\n\n[STT Transcript]\n");
            user_message.push_str(transcript);
        }

        // Vision analysis
        let vision_analysis = match Self::analyze_media_with_vision_provider(
            config,
            bot_context,
            account_tag,
            payload,
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                warn!(
                    "Vision analysis failed for profile '{}': {}",
                    bot_context.profile_name, e
                );
                None
            }
        };
        if let Some(analysis) = &vision_analysis {
            user_message.push_str("\n\n[Vision Analysis]\n");
            user_message.push_str(analysis);
        }

        // Load history
        let history = Self::load_chat_memory_history(
            bot_context,
            account_tag,
            user_scope_id,
            chat_id,
            MEMORY_MAX_CONTEXT_ENTRIES,
        )
        .await;
        messages.extend(history);

        // Add user message
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: Some(user_message.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });

        Ok(LlmMessagesResult {
            messages,
            user_message,
            vision_analysis,
        })
    }

    async fn get_mcp_tools(mcp_client: &Option<Arc<Mutex<McpClient>>>) -> Vec<ToolDefinition> {
        let mut tools = get_builtin_tool_definitions();

        if let Some(client) = mcp_client {
            let mcp = client.lock().await;
            let mcp_tools = mcp.list_all_tools().await;

            for (server_name, tool) in mcp_tools {
                let tool_def = ToolDefinition {
                    tool_type: "function".to_string(),
                    function: masix_providers::FunctionDefinition {
                        name: format!("{}_{}", server_name, tool.name),
                        description: tool.description,
                        parameters: tool.input_schema,
                    },
                };
                tools.push(tool_def);
            }
        }

        tools
    }

    fn build_tool_call_guidance(tools: &[ToolDefinition]) -> String {
        let mut builtin_names: Vec<String> = tools
            .iter()
            .map(|t| t.function.name.clone())
            .filter(|name| is_builtin_tool(name))
            .collect();
        builtin_names.sort();
        builtin_names.dedup();

        let mut extra_names: Vec<String> = tools
            .iter()
            .map(|t| t.function.name.clone())
            .filter(|name| !is_builtin_tool(name))
            .collect();
        extra_names.sort();
        extra_names.dedup();

        const MAX_LISTED_EXTRA_TOOLS: usize = 40;
        let extra_preview = if extra_names.is_empty() {
            "(none)".to_string()
        } else if extra_names.len() > MAX_LISTED_EXTRA_TOOLS {
            format!(
                "{} (+{} more)",
                extra_names[..MAX_LISTED_EXTRA_TOOLS].join(", "),
                extra_names.len() - MAX_LISTED_EXTRA_TOOLS
            )
        } else {
            extra_names.join(", ")
        };

        format!(
            "\n\n# Tool Calling Protocol\nHai accesso a tool runtime.\nQuando una richiesta richiede azioni su shell/file/web/device/termux, usa un tool-call e non limitarti a descrivere i tool.\nPreferisci sempre il tool-calling nativo del provider.\nSe il provider non supporta tool-calling nativo, usa questo formato esatto:\n### TOOL_CALL\ncall <tool_name>\n{{\"arg\":\"value\"}}\n### TOOL_CALL\nIf a search tool returns results, do not claim search is unavailable or 'Server not found'.\nBuilt-in tools sempre disponibili: {}\nMCP/extra tools disponibili: {}\nTotale tools disponibili: {}",
            builtin_names.join(", "),
            extra_preview,
            builtin_names.len() + extra_names.len()
        )
    }

    fn tool_call_signature(tool_call: &ToolCall) -> String {
        let canonical_args =
            serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments)
                .map(|value| value.to_string())
                .unwrap_or_else(|_| tool_call.function.arguments.trim().to_string());
        format!("{}::{}", tool_call.function.name, canonical_args)
    }

    fn build_relaxed_web_search_args(arguments: &serde_json::Value) -> Option<serde_json::Value> {
        let query = arguments.get("query").and_then(|v| v.as_str())?;
        let relaxed = Self::relax_search_query(query)?;
        if relaxed == query {
            return None;
        }

        let mut out = arguments.clone();
        if let serde_json::Value::Object(map) = &mut out {
            map.insert("query".to_string(), serde_json::Value::String(relaxed));
            map.remove("endpoint");
        } else {
            out = serde_json::json!({ "query": relaxed });
        }
        Some(out)
    }

    fn relax_search_query(query: &str) -> Option<String> {
        let stopwords: HashSet<&'static str> = [
            "fai",
            "ricerca",
            "ricercare",
            "cerca",
            "cercare",
            "correlate",
            "correlato",
            "analizza",
            "analisi",
            "eventuali",
            "possibili",
            "breve",
            "riassunto",
            "sintesi",
            "su",
            "sul",
            "sulla",
            "sulle",
            "con",
            "per",
            "tra",
            "fra",
            "del",
            "della",
            "delle",
            "degli",
            "dei",
            "e",
            "ed",
            "in",
            "di",
            "da",
            "a",
            "il",
            "lo",
            "la",
            "gli",
            "le",
            "the",
            "and",
            "for",
            "with",
            "from",
            "into",
        ]
        .into_iter()
        .collect();

        let mut keywords = Vec::new();
        for raw in query.split_whitespace() {
            let cleaned = raw
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                .to_lowercase();
            if cleaned.len() < 3 || stopwords.contains(cleaned.as_str()) {
                continue;
            }
            if !keywords.iter().any(|k| k == &cleaned) {
                keywords.push(cleaned);
            }
            if keywords.len() >= 10 {
                break;
            }
        }

        if keywords.is_empty() {
            None
        } else {
            Some(keywords.join(" "))
        }
    }

    fn count_search_results_from_tool_payload(payload: &str) -> usize {
        let trimmed = payload.trim();
        if trimmed.is_empty() || trimmed.starts_with("Tool error:") || trimmed.starts_with("Error:")
        {
            return 0;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(arr) = value.as_array() {
                return arr.len();
            }
            if let Some(arr) = value.get("results").and_then(|v| v.as_array()) {
                return arr.len();
            }
        }

        trimmed
            .lines()
            .filter(|line| {
                let l = line.trim_start();
                let mut chars = l.chars().peekable();
                let mut saw_digit = false;
                while matches!(chars.peek(), Some(c) if c.is_ascii_digit()) {
                    saw_digit = true;
                    let _ = chars.next();
                }
                saw_digit && matches!(chars.next(), Some('.'))
            })
            .count()
    }

    fn sanitize_false_search_unavailable_claims(response: &str) -> (String, bool) {
        let mut changed = false;
        let mut kept = Vec::new();

        for line in response.lines() {
            let low = line.to_lowercase();
            let says_server_not_found =
                low.contains("server not found") || low.contains("server non trovato");
            let says_unavailable = (low.contains("non disponibile")
                || low.contains("unavailable")
                || low.contains("not available"))
                && (low.contains("ricerca")
                    || low.contains("search")
                    || low.contains("plugin_discovery")
                    || low.contains("tool di ricerca web")
                    || low.contains("web tool"));

            if says_server_not_found || says_unavailable {
                changed = true;
                continue;
            }

            kept.push(line);
        }

        if !changed {
            return (response.to_string(), false);
        }

        let mut sanitized = kept.join("\n").trim().to_string();
        let note = "✅ Web search succeeded in this turn via discovery tools.";
        if sanitized.is_empty() {
            sanitized = note.to_string();
        } else {
            sanitized.push_str("\n\n");
            sanitized.push_str(note);
        }
        (sanitized, true)
    }

    fn synthesize_from_tool_messages(messages: &[ChatMessage]) -> Option<String> {
        let mut snippets: Vec<String> = Vec::new();

        for msg in messages.iter().rev() {
            if msg.role != "tool" {
                continue;
            }
            let Some(content) = &msg.content else {
                continue;
            };
            let trimmed = content.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("Error:")
                || trimmed.starts_with("Tool error:")
            {
                continue;
            }

            let preview = trimmed
                .lines()
                .take(8)
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            if preview.is_empty() {
                continue;
            }

            snippets.push(preview);
            if snippets.len() >= 2 {
                break;
            }
        }

        if snippets.is_empty() {
            return None;
        }

        snippets.reverse();
        Some(format!(
            "Tool execution completed but the model did not finalize in time.\n\nRaw findings:\n\n{}",
            snippets.join("\n\n---\n\n")
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_call(
        mcp_client: &Option<Arc<Mutex<McpClient>>>,
        tool_call: &ToolCall,
        exec_policy: &ExecPolicy,
        workdir: &Path,
        storage: &Arc<Mutex<Storage>>,
        envelope: &Envelope,
        account_tag: Option<&str>,
        vision_analysis: Option<&str>,
    ) -> Result<String> {
        let tool_name = &tool_call.function.name;
        let arguments: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
            .unwrap_or_else(|_| serde_json::json!({}));

        if tool_name == "cron" {
            return Self::execute_cron_tool(arguments, storage, envelope, account_tag).await;
        }
        if tool_name == "vision" {
            return Self::execute_vision_tool(arguments, envelope, vision_analysis);
        }

        // Check if it's a builtin tool
        if is_builtin_tool(tool_name) {
            return execute_builtin_tool(tool_name, arguments, exec_policy, workdir).await;
        }

        // Otherwise, it's an MCP tool
        let parts: Vec<&str> = tool_name.splitn(2, '_').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid tool name format: {}",
                tool_call.function.name
            ));
        }

        let server_name = parts[0];
        let mcp_tool_name = parts[1];

        if let Some(client) = mcp_client {
            let mcp = client.lock().await;
            let mut result = match mcp
                .call_tool(server_name, mcp_tool_name, arguments.clone())
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    if mcp_tool_name == "web_search" {
                        if let Some(relaxed_args) = Self::build_relaxed_web_search_args(&arguments)
                        {
                            warn!(
                                    "MCP web_search failed on first attempt (server='{}'): {}. Retrying with relaxed query.",
                                    server_name, e
                                );
                            mcp.call_tool(server_name, mcp_tool_name, relaxed_args)
                                .await?
                        } else {
                            return Err(e.into());
                        }
                    } else {
                        return Err(e.into());
                    }
                }
            };

            if result.is_error && mcp_tool_name == "web_search" {
                if let Some(relaxed_args) = Self::build_relaxed_web_search_args(&arguments) {
                    warn!(
                        "MCP web_search returned error payload (server='{}'), retrying with relaxed query.",
                        server_name
                    );
                    if let Ok(retry_result) = mcp
                        .call_tool(server_name, mcp_tool_name, relaxed_args)
                        .await
                    {
                        if !retry_result.is_error {
                            result = retry_result;
                        }
                    }
                }
            }

            let mut content_parts = Vec::new();
            for item in &result.content {
                match item {
                    masix_mcp::ToolContent::Text { text } => content_parts.push(text.clone()),
                    masix_mcp::ToolContent::Image { mime_type, .. } => {
                        content_parts.push(format!("[image content: {}]", mime_type));
                    }
                }
            }

            if content_parts.is_empty() {
                return Ok(serde_json::to_string(&result)?);
            }

            let joined = content_parts.join("\n");
            if result.is_error {
                warn!(
                    "MCP tool '{}' error payload (first 320 chars): {}",
                    tool_call.function.name,
                    joined.chars().take(320).collect::<String>()
                );
                Ok(format!("Tool error: {}", joined))
            } else {
                Ok(joined)
            }
        } else {
            Err(anyhow::anyhow!("No MCP client available"))
        }
    }

    /// Execute LLM chat loop with optional tool calling.
    /// Handles tool call deduplication, policy gates, and iteration limits.
    #[allow(clippy::too_many_arguments)]
    async fn run_llm_tool_loop(
        provider_router: &ProviderRouter,
        messages: &mut Vec<ChatMessage>,
        tools: Option<Vec<ToolDefinition>>,
        provider_chain: &[String],
        preferred_provider: Option<String>,
        preferred_model: Option<String>,
        retry_policy: &RetryPolicy,
        profile_name: &str,
        runtime_tool_access: &RuntimeToolAccess,
        sender_id: &str,
        mcp_client: &Option<Arc<Mutex<McpClient>>>,
        exec_policy: &ExecPolicy,
        workdir: &Path,
        storage: &Arc<Mutex<Storage>>,
        envelope: &Envelope,
        account_tag: Option<&str>,
        vision_analysis: Option<&str>,
        admin_only_modules: &HashSet<String>,
        permission: PermissionLevel,
    ) -> Result<LlmLoopResult> {
        let mut final_response = String::new();
        let mut iterations = 0;
        let mut selected_provider = preferred_provider;
        let mut used_tools: Vec<String> = Vec::new();
        let mut used_tool_signatures: HashSet<String> = HashSet::new();
        let mut successful_discovery_search_calls = 0usize;
        let mut hit_tool_iteration_limit = false;

        loop {
            iterations += 1;
            if iterations > MAX_TOOL_ITERATIONS {
                warn!("Max tool iterations reached");
                hit_tool_iteration_limit = true;
                break;
            }

            let (response, provider_used) = Self::chat_with_fallback_chain(
                provider_router,
                messages.clone(),
                tools.clone(),
                provider_chain,
                selected_provider.as_deref(),
                preferred_model.as_deref(),
                retry_policy,
                profile_name,
            )
            .await?;
            selected_provider = Some(provider_used);

            if let Some(content) = &response.content {
                if !content.is_empty() {
                    final_response = content.clone();
                }
            }

            if let Some(tool_calls) = &response.tool_calls {
                if tool_calls.is_empty() {
                    break;
                }

                let assistant_message = ChatMessage {
                    role: "assistant".to_string(),
                    content: response.content.clone(),
                    tool_calls: Some(tool_calls.clone()),
                    tool_call_id: None,
                    name: None,
                };
                messages.push(assistant_message);

                for tool_call in tool_calls {
                    info!("Executing tool: {}", tool_call.function.name);
                    if !used_tools
                        .iter()
                        .any(|name| name == &tool_call.function.name)
                    {
                        used_tools.push(tool_call.function.name.clone());
                    }

                    let signature = Self::tool_call_signature(tool_call);
                    if !used_tool_signatures.insert(signature) {
                        warn!(
                            "Skipping duplicate tool call within same turn: {}",
                            tool_call.function.name
                        );
                        messages.push(ChatMessage {
                            role: "tool".to_string(),
                            content: Some(
                                "Skipped duplicate tool call in same turn to prevent loops."
                                    .to_string(),
                            ),
                            tool_calls: None,
                            tool_call_id: Some(tool_call.id.clone()),
                            name: Some(tool_call.function.name.clone()),
                        });
                        continue;
                    }

                    if !runtime_tool_access.allows_tool(&tool_call.function.name) {
                        warn!(
                            "Tool execution denied for sender '{}' (tool='{}')",
                            sender_id, tool_call.function.name
                        );
                        messages.push(ChatMessage {
                            role: "tool".to_string(),
                            content: Some("Tool execution denied by role/tool policy.".to_string()),
                            tool_calls: None,
                            tool_call_id: Some(tool_call.id.clone()),
                            name: Some(tool_call.function.name.clone()),
                        });
                        continue;
                    }

                    // Check if tool belongs to an admin-only module
                    if is_admin_only_tool(&tool_call.function.name, admin_only_modules) {
                        if permission != PermissionLevel::Admin {
                            warn!(
                                "Admin-only tool '{}' denied for non-admin sender '{}'",
                                tool_call.function.name, sender_id
                            );
                            messages.push(ChatMessage {
                                role: "tool".to_string(),
                                content: Some(
                                    "Tool execution denied: this tool requires admin privileges."
                                        .to_string(),
                                ),
                                tool_calls: None,
                                tool_call_id: Some(tool_call.id.clone()),
                                name: Some(tool_call.function.name.clone()),
                            });
                            continue;
                        }
                    }

                    let tool_result = match Self::execute_tool_call(
                        mcp_client,
                        tool_call,
                        exec_policy,
                        workdir,
                        storage,
                        envelope,
                        account_tag,
                        vision_analysis,
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(e) => format!("Error: {}", e),
                    };

                    if tool_call.function.name == "plugin_discovery_web_search"
                        && Self::count_search_results_from_tool_payload(&tool_result) > 0
                    {
                        successful_discovery_search_calls += 1;
                    }

                    let tool_message = ChatMessage {
                        role: "tool".to_string(),
                        content: Some(tool_result),
                        tool_calls: None,
                        tool_call_id: Some(tool_call.id.clone()),
                        name: Some(tool_call.function.name.clone()),
                    };
                    messages.push(tool_message);
                }
            } else {
                break;
            }
        }

        if final_response.trim().is_empty() && !used_tools.is_empty() {
            let mut finalize_messages = messages.clone();
            finalize_messages.push(ChatMessage {
                role: "user".to_string(),
                content: Some(
                    "Finalize now using only gathered tool outputs. Do not call tools. Return the final user-facing answer."
                        .to_string(),
                ),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });

            match Self::chat_with_fallback_chain(
                provider_router,
                finalize_messages,
                None,
                provider_chain,
                selected_provider.as_deref(),
                preferred_model.as_deref(),
                retry_policy,
                profile_name,
            )
            .await
            {
                Ok((resp, _provider_used)) => {
                    if let Some(content) = resp.content {
                        if !content.trim().is_empty() {
                            final_response = content;
                        }
                    }
                }
                Err(e) => {
                    warn!("Finalization pass after tool loop failed: {}", e);
                }
            }
        }

        if final_response.trim().is_empty() && !used_tools.is_empty() {
            if let Some(synthesized) = Self::synthesize_from_tool_messages(&messages) {
                final_response = synthesized;
            }
        }

        if hit_tool_iteration_limit && final_response.trim().is_empty() {
            final_response = "Tool execution reached iteration limit before completion. Please retry with a narrower request.".to_string();
        }

        Ok(LlmLoopResult {
            final_response,
            used_tools,
            successful_discovery_search_calls,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_inbound_message(
        envelope: Envelope,
        outbound_sender: broadcast::Sender<OutboundMessage>,
        provider_router: &ProviderRouter,
        storage: &Arc<Mutex<Storage>>,
        mcp_client: &Option<Arc<Mutex<McpClient>>>,
        system_prompt: &str,
        policy: &PolicyEngine,
        rate_state: &Arc<Mutex<HashMap<String, (i64, u32)>>>,
        bot_contexts: &Arc<HashMap<String, BotContext>>,
        user_languages: &Arc<Mutex<HashMap<String, masix_telegram::menu::Language>>>,
        user_providers: &Arc<Mutex<HashMap<String, String>>>,
        user_models: &Arc<Mutex<HashMap<String, String>>>,
        config: &Config,
        admin_only_modules: &HashSet<String>,
    ) -> Result<()> {
        let account_tag = envelope
            .payload
            .get("account_tag")
            .and_then(|v| v.as_str())
            .map(|value| value.to_string());
        let bot_context = Self::resolve_bot_context(bot_contexts, account_tag.as_deref());
        let user_scope_id = Self::resolve_user_scope_id(&envelope);
        let user_state_key = Self::user_state_key(
            account_tag.as_deref(),
            user_scope_id.as_deref(),
            envelope.chat_id,
        );

        match &envelope.kind {
            MessageKind::Message { from, text } => {
                info!("Processing message from {}: {}", from, text);

                let sender_id = envelope
                    .chat_id
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| from.clone());
                let scoped_sender_id = Self::scoped_value(account_tag.as_deref(), &sender_id);

                if !policy.is_allowed(&sender_id) {
                    warn!("Blocked message by policy from {}", sender_id);
                    if let Some(chat_id) = envelope.chat_id {
                        Self::send_outbound_text(
                            &outbound_sender,
                            &envelope.channel,
                            account_tag.clone(),
                            chat_id,
                            "Access denied by policy.",
                            None,
                        );
                    }
                    return Ok(());
                }

                if !Self::check_and_update_rate_limit(policy, rate_state, &scoped_sender_id).await {
                    warn!("Rate limit exceeded for {}", sender_id);
                    if let Some(chat_id) = envelope.chat_id {
                        Self::send_outbound_text(
                            &outbound_sender,
                            &envelope.channel,
                            account_tag.clone(),
                            chat_id,
                            "Rate limit exceeded. Please retry shortly.",
                            envelope.message_id,
                        );
                    }
                    return Ok(());
                }

                let from_user_id = Self::resolve_sender_user_id(&envelope, from);
                let chat_id = envelope.chat_id.unwrap_or(0);
                let mut permission = Self::get_permission_level(
                    config,
                    account_tag.as_deref(),
                    &envelope.channel,
                    from,
                    from_user_id,
                    chat_id,
                    text,
                );

                if permission == PermissionLevel::None
                    && Self::should_auto_register_user(
                        config,
                        account_tag.as_deref(),
                        &envelope.channel,
                        from_user_id,
                        chat_id,
                    )
                {
                    if let Err(e) =
                        Self::register_user(config, account_tag.as_deref(), from_user_id).await
                    {
                        warn!("Failed to auto-register user {}: {}", from_user_id, e);
                    } else {
                        permission = Self::get_permission_level(
                            config,
                            account_tag.as_deref(),
                            &envelope.channel,
                            from,
                            from_user_id,
                            chat_id,
                            text,
                        );
                    }
                }

                if permission == PermissionLevel::None {
                    if envelope.channel == "telegram"
                        && Self::should_silently_ignore_telegram_permission_denial(
                            config,
                            account_tag.as_deref(),
                            from_user_id,
                            chat_id,
                            text,
                        )
                    {
                        debug!(
                            "Ignoring untagged Telegram group message (tag-based mode) from user {} in chat {}",
                            from_user_id,
                            envelope.chat_id.unwrap_or(0)
                        );
                        return Ok(());
                    }
                    warn!(
                        "Unauthorized access attempt from user {} in chat {}",
                        from_user_id,
                        envelope.chat_id.unwrap_or(0)
                    );
                    if let Some(chat_id) = envelope.chat_id {
                        Self::send_outbound_text(
                            &outbound_sender,
                            &envelope.channel,
                            account_tag.clone(),
                            chat_id,
                            "Unauthorized. Contact admin for access.",
                            None,
                        );
                    }
                    return Ok(());
                }

                let runtime_tool_access = Self::runtime_tool_access_for_message(
                    config,
                    account_tag.as_deref(),
                    &envelope.channel,
                    permission,
                );
                let allow_runtime_tools = runtime_tool_access.is_enabled();

                let _ = Self::record_user_catalog(
                    &bot_context,
                    account_tag.as_deref(),
                    user_scope_id.as_deref(),
                    envelope.chat_id,
                    &envelope.channel,
                )
                .await;

                if Self::handle_chat_commands(
                    text,
                    &envelope,
                    &outbound_sender,
                    account_tag.as_deref(),
                    &user_state_key,
                    user_languages,
                    user_providers,
                    user_models,
                    &bot_context,
                    user_scope_id.as_deref(),
                    config,
                    from,
                    from_user_id,
                    permission,
                    mcp_client,
                )
                .await?
                {
                    return Ok(());
                }

                if Self::handle_cron_command(
                    text,
                    &envelope,
                    &outbound_sender,
                    storage,
                    account_tag.clone(),
                )
                .await?
                {
                    return Ok(());
                }

                if Self::handle_exec_command(
                    text,
                    &envelope,
                    &outbound_sender,
                    &bot_context,
                    account_tag.clone(),
                    permission,
                )
                .await?
                {
                    return Ok(());
                }

                let tools = if allow_runtime_tools {
                    let all_tools = Self::get_mcp_tools(mcp_client).await;
                    runtime_tool_access.filter_tools(all_tools)
                } else {
                    Vec::new()
                };
                let has_tools = !tools.is_empty();
                let builtin_tools_count = tools
                    .iter()
                    .filter(|tool| is_builtin_tool(&tool.function.name))
                    .count();
                if !allow_runtime_tools {
                    debug!(
                        "Runtime tools disabled for sender '{}' (channel={}, permission={:?})",
                        from, envelope.channel, permission
                    );
                }
                if has_tools {
                    info!(
                        "Tool exposure for '{}' profile: total={} builtins={} mcp={}",
                        bot_context.profile_name,
                        tools.len(),
                        builtin_tools_count,
                        tools.len().saturating_sub(builtin_tools_count)
                    );
                    debug!(
                        "Tool names: {}",
                        tools
                            .iter()
                            .map(|tool| tool.function.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }

                let _typing_heartbeat = Self::start_typing_heartbeat(
                    &outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    envelope.chat_id,
                );

                // Build LLM messages (system, memory, media, history)
                let llm_msgs = Self::build_llm_messages(
                    system_prompt,
                    &bot_context,
                    account_tag.as_deref(),
                    user_scope_id.as_deref(),
                    envelope.chat_id,
                    text,
                    &envelope.payload,
                    &tools,
                    has_tools,
                    config,
                )
                .await?;

                let mut messages = llm_msgs.messages;
                let user_message = llm_msgs.user_message;
                let vision_analysis = llm_msgs.vision_analysis;

                let preferred_provider = user_providers.lock().await.get(&user_state_key).cloned();
                let preferred_model = user_models.lock().await.get(&user_state_key).cloned();

                // Execute LLM loop with tool calling
                let llm_result = Self::run_llm_tool_loop(
                    provider_router,
                    &mut messages,
                    if has_tools { Some(tools.clone()) } else { None },
                    &bot_context.provider_chain,
                    preferred_provider,
                    preferred_model,
                    &bot_context.retry_policy,
                    &bot_context.profile_name,
                    &runtime_tool_access,
                    from,
                    mcp_client,
                    &bot_context.exec_policy,
                    &bot_context.workdir,
                    storage,
                    &envelope,
                    account_tag.as_deref(),
                    vision_analysis.as_deref(),
                    admin_only_modules,
                    permission,
                )
                .await?;

                let mut final_response = llm_result.final_response;
                if final_response.is_empty() {
                    final_response = "Non ho potuto generare una risposta.".to_string();
                }
                if llm_result.successful_discovery_search_calls > 0 {
                    let (sanitized, changed) =
                        Self::sanitize_false_search_unavailable_claims(&final_response);
                    if changed {
                        warn!(
                            "Sanitized false search-unavailable claim from LLM response (successful discovery searches: {}).",
                            llm_result.successful_discovery_search_calls
                        );
                    }
                    final_response = sanitized;
                }
                if !llm_result.used_tools.is_empty() {
                    final_response.push_str("\n\n🧰 Tool usati: ");
                    final_response.push_str(&llm_result.used_tools.join(", "));
                }

                let _ = Self::append_chat_memory(
                    &bot_context,
                    account_tag.as_deref(),
                    user_scope_id.as_deref(),
                    envelope.chat_id,
                    "user",
                    &user_message,
                )
                .await;
                let _ = Self::append_chat_memory(
                    &bot_context,
                    account_tag.as_deref(),
                    user_scope_id.as_deref(),
                    envelope.chat_id,
                    "assistant",
                    &final_response,
                )
                .await;
                let _ = Self::update_summary_snapshot(
                    &bot_context,
                    account_tag.as_deref(),
                    user_scope_id.as_deref(),
                    envelope.chat_id,
                )
                .await;

                Self::dispatch_final_response(
                    &outbound_sender,
                    &envelope,
                    account_tag.clone(),
                    &final_response,
                    config,
                );
            }
            MessageKind::Callback { query_id, data } => {
                info!("Processing callback {}: {}", query_id, data);

                if let Some(chat_id) = envelope.chat_id {
                    if !policy.is_allowed(&chat_id.to_string()) {
                        warn!("Blocked callback by policy from chat {}", chat_id);
                        return Ok(());
                    }

                    // Handle language change
                    if data.starts_with("lang:") {
                        let lang_code = data.strip_prefix("lang:").unwrap_or("en");
                        if let Ok(new_lang) = lang_code.parse::<Language>() {
                            user_languages
                                .lock()
                                .await
                                .insert(user_state_key.clone(), new_lang);
                            let (_text, keyboard) = masix_telegram::menu::settings_menu(new_lang);
                            let msg = OutboundMessage {
                                channel: envelope.channel.clone(),
                                account_tag: account_tag.clone(),
                                chat_id,
                                text: masix_telegram::menu::language_changed_text(new_lang),
                                reply_to: None,
                                edit_message_id: envelope.message_id,
                                inline_keyboard: Some(keyboard),
                                chat_action: None,
                            };
                            let _ = outbound_sender.send(msg);
                            return Ok(());
                        }
                    }

                    let lang = user_languages
                        .lock()
                        .await
                        .get(&user_state_key)
                        .copied()
                        .unwrap_or_default();
                    let callback_user_id = envelope
                        .payload
                        .get("from_user_id")
                        .and_then(|value| value.as_i64())
                        .unwrap_or_default();
                    let callback_is_admin = if envelope.channel == "telegram" {
                        if let Some(account) =
                            Self::get_telegram_account(config, account_tag.as_deref())
                        {
                            let dynamic_acl = Self::load_dynamic_acl_for_account(account);
                            Self::telegram_user_permission(account, &dynamic_acl, callback_user_id)
                                == PermissionLevel::Admin
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if let Some(msg) = masix_telegram::menu::handle_callback(
                        data,
                        chat_id,
                        envelope.message_id,
                        account_tag.clone(),
                        lang,
                        callback_is_admin,
                    ) {
                        let _ = outbound_sender.send(msg);
                    }
                }
            }
            MessageKind::Command { name, args } => {
                info!("Processing command {} with args {:?}", name, args);
            }
            MessageKind::Reply { to, text } => {
                info!("Processing reply to {}: {}", to, text);
            }
            MessageKind::Error { code, message } => {
                error!("Error {} received: {}", code, message);
            }
        }

        Ok(())
    }

    fn dispatch_final_response(
        outbound_sender: &broadcast::Sender<OutboundMessage>,
        envelope: &Envelope,
        account_tag: Option<String>,
        response: &str,
        config: &Config,
    ) {
        if envelope.channel == "telegram" {
            if let Some(chat_id) = envelope.chat_id {
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag,
                    chat_id,
                    response,
                    envelope.message_id,
                );
            }
            return;
        }

        if envelope.channel == "whatsapp" {
            if let Some(msg) = Self::build_whatsapp_forward_message(config, envelope, response) {
                let _ = outbound_sender.send(msg);
            }
            return;
        }

        if envelope.channel == "sms" {
            if let Some(msg) = Self::build_sms_forward_message(config, envelope, response) {
                let _ = outbound_sender.send(msg);
            }
        }
    }

    fn message_sender_id(envelope: &Envelope) -> &str {
        match &envelope.kind {
            MessageKind::Message { from, .. } => from.as_str(),
            _ => "unknown",
        }
    }

    fn build_whatsapp_forward_message(
        config: &Config,
        envelope: &Envelope,
        response: &str,
    ) -> Option<OutboundMessage> {
        let whatsapp = config.whatsapp.as_ref()?;
        let chat_id = whatsapp.forward_to_telegram_chat_id?;
        let account_tag = whatsapp
            .forward_to_telegram_account_tag
            .clone()
            .or_else(|| Self::default_telegram_account_tag_from_config(config));
        let prefix = whatsapp
            .forward_prefix
            .as_deref()
            .unwrap_or("WhatsApp listener");
        let sender = Self::message_sender_id(envelope);
        let text = format!("{} [{}]\n{}", prefix, sender, response);

        Some(OutboundMessage {
            channel: "telegram".to_string(),
            account_tag,
            chat_id,
            text,
            reply_to: None,
            edit_message_id: None,
            inline_keyboard: None,
            chat_action: None,
        })
    }

    fn build_sms_forward_message(
        config: &Config,
        envelope: &Envelope,
        response: &str,
    ) -> Option<OutboundMessage> {
        let sms = config.sms.as_ref()?;
        let chat_id = sms.forward_to_telegram_chat_id?;
        let account_tag = sms
            .forward_to_telegram_account_tag
            .clone()
            .or_else(|| Self::default_telegram_account_tag_from_config(config));
        let prefix = sms.forward_prefix.as_deref().unwrap_or("SMS listener");
        let sender = Self::message_sender_id(envelope);
        let text = format!("{} [{}]\n{}", prefix, sender, response);

        Some(OutboundMessage {
            channel: "telegram".to_string(),
            account_tag,
            chat_id,
            text,
            reply_to: None,
            edit_message_id: None,
            inline_keyboard: None,
            chat_action: None,
        })
    }

    fn enrich_user_message_with_media(text: &str, payload: &serde_json::Value) -> String {
        let Some(summary) = Self::media_summary_from_payload(payload) else {
            return text.to_string();
        };
        if text.trim().is_empty() {
            format!("[Media message]\n{}", summary)
        } else {
            format!("{}\n\n[Media Context]\n{}", text, summary)
        }
    }

    fn media_summary_from_payload(payload: &serde_json::Value) -> Option<String> {
        let media = payload.get("media")?;
        let kind = media
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let file_id = media
            .get("file_id")
            .and_then(|v| v.as_str())
            .unwrap_or("n/a");
        let caption = media
            .get("caption")
            .and_then(|v| v.as_str())
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .unwrap_or("(none)");
        let mime_type = media
            .get("mime_type")
            .and_then(|v| v.as_str())
            .unwrap_or("n/a");
        let width = media
            .get("width")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string());
        let height = media
            .get("height")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string());
        let file_size = media
            .get("file_size")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string());
        let duration = media
            .get("duration")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string());
        let file_name = media
            .get("file_name")
            .and_then(|v| v.as_str())
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .unwrap_or("n/a");
        let title = media
            .get("title")
            .and_then(|v| v.as_str())
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .unwrap_or("n/a");
        let performer = media
            .get("performer")
            .and_then(|v| v.as_str())
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .unwrap_or("n/a");

        let mut lines = vec![
            format!("kind: {}", kind),
            format!("file_id: {}", file_id),
            format!("mime_type: {}", mime_type),
            format!("size: {} bytes", file_size),
        ];

        if width != "n/a" || height != "n/a" {
            lines.push(format!("resolution: {}x{}", width, height));
        }
        if let Some(duration) = duration {
            lines.push(format!("duration: {}s", duration));
        }
        if file_name != "n/a" {
            lines.push(format!("file_name: {}", file_name));
        }
        if title != "n/a" {
            lines.push(format!("title: {}", title));
        }
        if performer != "n/a" {
            lines.push(format!("performer: {}", performer));
        }
        lines.push(format!("caption: {}", caption));

        Some(lines.join("\n"))
    }

    fn media_file_reference_from_payload(
        payload: &serde_json::Value,
    ) -> Option<MediaFileReference> {
        let media = payload.get("media")?;
        let file_id = media
            .get("file_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())?
            .to_string();
        let kind = media
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let file_name = media
            .get("file_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string());

        let mut mime_type = media
            .get("mime_type")
            .and_then(|v| v.as_str())
            .map(|v| v.trim().to_string())
            .or_else(|| {
                file_name
                    .as_deref()
                    .and_then(Self::guess_mime_from_file_name)
            })
            .unwrap_or_else(|| match kind {
                "photo" | "image_document" => "image/jpeg".to_string(),
                "voice" => "audio/ogg".to_string(),
                "audio" => "audio/mpeg".to_string(),
                _ => "application/octet-stream".to_string(),
            });

        if mime_type == "application/octet-stream" {
            if let Some(name) = file_name.as_deref() {
                if let Some(guess) = Self::guess_mime_from_file_name(name) {
                    mime_type = guess;
                }
            }

            if mime_type == "application/octet-stream" && kind == "image_document" {
                mime_type = "image/jpeg".to_string();
            }
            if mime_type == "application/octet-stream" && kind == "voice" {
                mime_type = "audio/ogg".to_string();
            }
        }

        let caption = media
            .get("caption")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string());

        Some(MediaFileReference {
            file_id,
            mime_type,
            file_name,
            caption,
        })
    }

    #[cfg(feature = "stt")]
    fn audio_media_file_reference_from_payload(
        payload: &serde_json::Value,
    ) -> Option<MediaFileReference> {
        let media = payload.get("media")?;
        let kind = media
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        if kind != "voice" && kind != "audio" {
            return None;
        }

        let media_ref = Self::media_file_reference_from_payload(payload)?;
        if media_ref.mime_type.starts_with("audio/") || kind == "voice" || kind == "audio" {
            Some(media_ref)
        } else {
            None
        }
    }

    fn guess_mime_from_file_name(file_name: &str) -> Option<String> {
        let ext = std::path::Path::new(file_name)
            .extension()
            .and_then(|value| value.to_str())?
            .to_ascii_lowercase();
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "ogg" => "audio/ogg",
            "oga" => "audio/ogg",
            "opus" => "audio/opus",
            "m4a" => "audio/mp4",
            "mp4" => "video/mp4",
            _ => return None,
        };
        Some(mime.to_string())
    }

    fn resolve_telegram_bot_token(config: &Config, account_tag: Option<&str>) -> Option<String> {
        let telegram = config.telegram.as_ref()?;
        if let Some(tag) = account_tag {
            if let Some(account) = telegram
                .accounts
                .iter()
                .find(|account| Self::account_tag_from_token(&account.bot_token) == tag)
            {
                return Some(account.bot_token.clone());
            }
        }
        telegram
            .accounts
            .first()
            .map(|account| account.bot_token.clone())
    }

    async fn fetch_telegram_media_bytes(
        config: &Config,
        account_tag: Option<&str>,
        file_id: &str,
    ) -> Result<(Vec<u8>, Option<String>)> {
        let bot_token = Self::resolve_telegram_bot_token(config, account_tag)
            .ok_or_else(|| anyhow!("Telegram bot token not found for media download"))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .build()?;
        let get_file_url = format!("https://api.telegram.org/bot{}/getFile", bot_token);
        let get_file_resp = client
            .post(&get_file_url)
            .json(&serde_json::json!({
                "file_id": file_id
            }))
            .send()
            .await?;
        let get_file_status = get_file_resp.status();
        let get_file_body = get_file_resp.text().await.unwrap_or_default();
        if !get_file_status.is_success() {
            anyhow::bail!(
                "telegram getFile failed with HTTP {}",
                get_file_status.as_u16()
            );
        }

        let parsed: TelegramGetFileResponse =
            serde_json::from_str(&get_file_body).map_err(|e| {
                anyhow!(
                    "telegram getFile decode failed: {} | body={}",
                    e,
                    get_file_body.chars().take(400).collect::<String>()
                )
            })?;
        if !parsed.ok {
            let description = parsed
                .description
                .unwrap_or_else(|| "unknown getFile error".to_string());
            anyhow::bail!("telegram getFile returned ok=false: {}", description);
        }
        let file_path = parsed
            .result
            .ok_or_else(|| anyhow!("telegram getFile missing result"))?
            .file_path;

        let download_url = format!(
            "https://api.telegram.org/file/bot{}/{}",
            bot_token, file_path
        );
        let download_resp = client.get(&download_url).send().await?;
        let download_status = download_resp.status();
        if !download_status.is_success() {
            anyhow::bail!(
                "telegram media download failed with HTTP {}",
                download_status.as_u16()
            );
        }
        let detected_mime = download_resp
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_string());
        let bytes = download_resp.bytes().await?.to_vec();
        Ok((bytes, detected_mime))
    }

    fn parse_openai_compatible_response_text(value: &serde_json::Value) -> Option<String> {
        let message = value
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))?;

        if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        if let Some(content_items) = message.get("content").and_then(|v| v.as_array()) {
            let mut parts = Vec::new();
            for item in content_items {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                }
            }
            if !parts.is_empty() {
                return Some(parts.join("\n"));
            }
        }

        None
    }

    async fn call_openai_compatible_vision_provider(
        provider: &masix_config::ProviderConfig,
        image_bytes: &[u8],
        mime_type: &str,
        caption: Option<&str>,
    ) -> Result<String> {
        let base_url = provider
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let model = provider
            .model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".to_string());
        let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        let mut prompt = "Analizza questa immagine e restituisci solo informazioni utili al modello principale: oggetti, testo leggibile, contesto, eventuali warning."
            .to_string();
        if let Some(value) = caption {
            let caption_trimmed = value.trim();
            if !caption_trimmed.is_empty() {
                prompt.push_str("\nCaption utente: ");
                prompt.push_str(caption_trimmed);
            }
        }

        let image_data = format!(
            "data:{};base64,{}",
            mime_type,
            base64::engine::general_purpose::STANDARD.encode(image_bytes)
        );
        let body = serde_json::json!({
            "model": model,
            "messages": [
                {
                    "role": "system",
                    "content": "Sei un modulo vision. Rispondi in italiano, sintetico, senza inventare dettagli non osservabili."
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": prompt
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": image_data
                            }
                        }
                    ]
                }
            ],
            "temperature": 0.1
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        let response = client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {}", provider.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let raw = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "Vision provider '{}' returned HTTP {}: {}",
                provider.name,
                status.as_u16(),
                raw.chars().take(400).collect::<String>()
            );
        }

        let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
            anyhow!(
                "Vision provider '{}' decode failed: {} | body={}",
                provider.name,
                e,
                raw.chars().take(400).collect::<String>()
            )
        })?;
        let text = Self::parse_openai_compatible_response_text(&parsed).ok_or_else(|| {
            anyhow!(
                "Vision provider '{}' returned response without text content",
                provider.name
            )
        })?;
        Ok(text)
    }

    async fn analyze_media_with_vision_provider(
        config: &Config,
        bot_context: &BotContext,
        account_tag: Option<&str>,
        payload: &serde_json::Value,
    ) -> Result<Option<String>> {
        let Some(vision_provider_name) = bot_context.vision_provider.as_deref() else {
            return Ok(None);
        };
        let Some(media_ref) = Self::media_file_reference_from_payload(payload) else {
            return Ok(None);
        };
        if !media_ref.mime_type.starts_with("image/") {
            return Ok(None);
        }

        let provider = config
            .providers
            .providers
            .iter()
            .find(|candidate| candidate.name == vision_provider_name)
            .ok_or_else(|| anyhow!("Vision provider '{}' not found", vision_provider_name))?;
        let provider_type = provider.provider_type.as_deref().unwrap_or("openai");
        if provider_type != "openai" {
            anyhow::bail!(
                "Vision provider '{}' must be openai-compatible, found '{}'",
                vision_provider_name,
                provider_type
            );
        }

        let (media_bytes, detected_mime) =
            Self::fetch_telegram_media_bytes(config, account_tag, &media_ref.file_id).await?;
        if media_bytes.is_empty() {
            return Ok(None);
        }
        if media_bytes.len() > 15 * 1024 * 1024 {
            anyhow::bail!(
                "Media too large for vision analysis ({} bytes)",
                media_bytes.len()
            );
        }

        let mime_type = if media_ref.mime_type.starts_with("image/") {
            media_ref.mime_type
        } else {
            detected_mime.unwrap_or_else(|| "image/jpeg".to_string())
        };
        if !mime_type.starts_with("image/") {
            return Ok(None);
        }

        let mut analysis = Self::call_openai_compatible_vision_provider(
            provider,
            &media_bytes,
            &mime_type,
            media_ref.caption.as_deref(),
        )
        .await?;
        if analysis.chars().count() > 4000 {
            analysis = format!("{}...", analysis.chars().take(4000).collect::<String>());
        }
        Ok(Some(analysis))
    }

    #[cfg(feature = "stt")]
    async fn transcribe_media_with_local_stt(
        config: &Config,
        account_tag: Option<&str>,
        payload: &serde_json::Value,
    ) -> Result<Option<String>> {
        let Some(stt) = config.stt.as_ref() else {
            return Ok(None);
        };
        if !stt.enabled {
            return Ok(None);
        }
        if stt.engine.trim() != "local_whisper_cpp" {
            return Ok(None);
        }

        let Some(media_ref) = Self::audio_media_file_reference_from_payload(payload) else {
            return Ok(None);
        };

        let (media_bytes, detected_mime) =
            Self::fetch_telegram_media_bytes(config, account_tag, &media_ref.file_id).await?;
        if media_bytes.is_empty() {
            return Ok(None);
        }
        if media_bytes.len() > 25 * 1024 * 1024 {
            anyhow::bail!(
                "Media too large for local STT ({} bytes)",
                media_bytes.len()
            );
        }

        let mime_type = if media_ref.mime_type == "application/octet-stream" {
            detected_mime.unwrap_or(media_ref.mime_type)
        } else {
            media_ref.mime_type
        };

        let mut transcript = Self::run_local_whisper_cpp(
            config,
            stt,
            &media_bytes,
            &mime_type,
            media_ref.file_name.as_deref(),
        )
        .await?;

        transcript = transcript.trim().to_string();
        if transcript.is_empty() {
            return Ok(None);
        }
        if transcript.chars().count() > 4000 {
            transcript = format!("{}...", transcript.chars().take(4000).collect::<String>());
        }
        Ok(Some(transcript))
    }

    #[cfg(not(feature = "stt"))]
    async fn transcribe_media_with_local_stt(
        _config: &Config,
        _account_tag: Option<&str>,
        _payload: &serde_json::Value,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    #[cfg(feature = "stt")]
    async fn run_local_whisper_cpp(
        config: &Config,
        stt: &SttConfig,
        audio_bytes: &[u8],
        mime_type: &str,
        file_name: Option<&str>,
    ) -> Result<String> {
        let model_raw = stt
            .local_model_path
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if model_raw.is_empty() {
            anyhow::bail!("stt.local_model_path is not configured");
        }

        let data_dir = Self::data_dir_from_config(config)?;
        let model_path = Self::resolve_path_with_base(model_raw, &data_dir)?;
        if !model_path.exists() {
            anyhow::bail!("STT model file not found: {}", model_path.display());
        }

        let bin_config = stt
            .local_bin
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let whisper_bin = if let Some(value) = bin_config {
            PathBuf::from(value)
        } else if Self::is_command_available("whisper-cli") {
            PathBuf::from("whisper-cli")
        } else if Self::is_command_available("whisper-cpp") {
            PathBuf::from("whisper-cpp")
        } else {
            let bundled_candidates = [
                data_dir.join("bin").join("whisper-cli"),
                data_dir.join("bin").join("whisper-cpp"),
                data_dir.join("bin").join("main"),
            ];
            if let Some(found) = bundled_candidates.into_iter().find(|path| path.exists()) {
                found
            } else {
                anyhow::bail!(
                    "whisper-cli/whisper-cpp not found. Run `masix config stt` to install/download a local STT binary or set stt.local_bin."
                );
            }
        };
        if !Self::is_command_available_path(&whisper_bin) {
            anyhow::bail!("STT binary not found/executable: {}", whisper_bin.display());
        }

        let threads = stt.local_threads.unwrap_or(2).clamp(1, 32);
        let language = stt
            .local_language
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let work_dir = std::env::temp_dir().join(format!(
            "masix-stt-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        fs::create_dir_all(&work_dir).await?;

        let result: Result<String> = async {
            let ext = Self::audio_extension_for_input(mime_type, file_name);
            let input_path = work_dir.join(format!("input.{}", ext));
            fs::write(&input_path, audio_bytes).await?;

            let mut whisper_input_path = input_path.clone();
            if Self::mime_requires_ffmpeg_conversion(mime_type, file_name) {
                if !Self::is_command_available("ffmpeg") {
                    anyhow::bail!(
                        "ffmpeg is required for '{}' audio (voice/ogg-opus). Install ffmpeg.",
                        mime_type
                    );
                }

                let converted = work_dir.join("input.wav");
                let ffmpeg_output = TokioCommand::new("ffmpeg")
                    .arg("-y")
                    .arg("-i")
                    .arg(&input_path)
                    .arg("-ar")
                    .arg("16000")
                    .arg("-ac")
                    .arg("1")
                    .arg(&converted)
                    .output()
                    .await?;
                if !ffmpeg_output.status.success() {
                    anyhow::bail!(
                        "ffmpeg conversion failed: {}",
                        String::from_utf8_lossy(&ffmpeg_output.stderr)
                            .trim()
                            .chars()
                            .take(240)
                            .collect::<String>()
                    );
                }
                whisper_input_path = converted;
            }

            let out_prefix = work_dir.join("transcript");
            let mut cmd = TokioCommand::new(&whisper_bin);
            cmd.arg("-m")
                .arg(&model_path)
                .arg("-f")
                .arg(&whisper_input_path)
                .arg("-of")
                .arg(&out_prefix)
                .arg("-otxt")
                .arg("-t")
                .arg(threads.to_string());
            if let Some(language) = language {
                cmd.arg("-l").arg(language);
            }

            let output =
                tokio::time::timeout(std::time::Duration::from_secs(120), cmd.output()).await;
            let output = match output {
                Ok(inner) => inner?,
                Err(_) => anyhow::bail!("whisper-cli timed out after 120 seconds"),
            };
            if !output.status.success() {
                anyhow::bail!(
                    "whisper-cli failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                        .trim()
                        .chars()
                        .take(320)
                        .collect::<String>()
                );
            }

            Self::discover_whisper_transcript(
                &work_dir,
                &out_prefix,
                &whisper_input_path,
                &output.stdout,
            )
            .await
        }
        .await;

        if let Err(err) = fs::remove_dir_all(&work_dir).await {
            debug!(
                "Failed to cleanup temporary STT directory '{}': {}",
                work_dir.display(),
                err
            );
        }

        result
    }

    #[cfg(feature = "stt")]
    async fn discover_whisper_transcript(
        work_dir: &Path,
        out_prefix: &Path,
        whisper_input_path: &Path,
        stdout: &[u8],
    ) -> Result<String> {
        let candidate_paths = vec![
            PathBuf::from(format!("{}.txt", out_prefix.display())),
            PathBuf::from(format!("{}.txt", whisper_input_path.display())),
            whisper_input_path.with_extension("txt"),
            work_dir.join("transcript.txt"),
        ];

        let mut seen = HashSet::new();
        for candidate in candidate_paths {
            if !seen.insert(candidate.clone()) {
                continue;
            }
            if !candidate.exists() {
                continue;
            }
            let raw = fs::read_to_string(&candidate).await.unwrap_or_default();
            if let Some(text) = Self::normalize_whisper_transcript(&raw) {
                return Ok(text);
            }
        }

        let mut newest_txt: Option<(std::time::SystemTime, PathBuf)> = None;
        let mut dir = fs::read_dir(work_dir).await?;
        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            let is_txt = path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));
            if !is_txt {
                continue;
            }
            let modified = entry
                .metadata()
                .await
                .ok()
                .and_then(|meta| meta.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            match &newest_txt {
                Some((current, _)) if modified <= *current => {}
                _ => newest_txt = Some((modified, path)),
            }
        }
        if let Some((_, candidate)) = newest_txt {
            let raw = fs::read_to_string(candidate).await.unwrap_or_default();
            if let Some(text) = Self::normalize_whisper_transcript(&raw) {
                return Ok(text);
            }
        }

        let stdout_text = String::from_utf8_lossy(stdout);
        if let Some(text) = Self::normalize_whisper_transcript(&stdout_text) {
            return Ok(text);
        }

        anyhow::bail!("whisper-cli completed but transcript output was not found");
    }

    #[cfg(feature = "stt")]
    fn normalize_whisper_transcript(raw: &str) -> Option<String> {
        let mut parts = Vec::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("main:")
                || lower.starts_with("system_info:")
                || lower.starts_with("loading model")
                || lower.starts_with("processing")
                || lower.starts_with("output")
                || lower.starts_with("whisper")
                || lower.starts_with("usage:")
            {
                continue;
            }

            let mut cleaned = trimmed;
            if trimmed.starts_with('[') {
                if let Some(end) = trimmed.rfind(']') {
                    if end + 1 < trimmed.len() {
                        cleaned = trimmed[end + 1..].trim();
                    }
                }
            }
            if cleaned.is_empty() {
                continue;
            }
            parts.push(cleaned.to_string());
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }

    #[cfg(feature = "stt")]
    fn audio_extension_for_input(mime_type: &str, file_name: Option<&str>) -> String {
        if let Some(file_name) = file_name {
            if let Some(ext) = std::path::Path::new(file_name)
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase())
            {
                if !ext.is_empty() {
                    return ext;
                }
            }
        }

        let mime = mime_type.to_ascii_lowercase();
        if mime.contains("wav") {
            "wav".to_string()
        } else if mime.contains("mpeg") || mime.contains("mp3") {
            "mp3".to_string()
        } else if mime.contains("mp4") || mime.contains("m4a") {
            "m4a".to_string()
        } else if mime.contains("ogg") || mime.contains("opus") {
            "ogg".to_string()
        } else {
            "bin".to_string()
        }
    }

    #[cfg(feature = "stt")]
    fn mime_requires_ffmpeg_conversion(mime_type: &str, file_name: Option<&str>) -> bool {
        let mime = mime_type.to_ascii_lowercase();
        if mime.contains("opus") {
            return true;
        }
        if mime.contains("ogg") && !mime.contains("vorbis") {
            return true;
        }

        if let Some(file_name) = file_name {
            let ext = std::path::Path::new(file_name)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            return ext == "opus" || ext == "ogg";
        }

        false
    }

    #[cfg(feature = "stt")]
    fn is_command_available(binary: &str) -> bool {
        let path = PathBuf::from(binary);
        if path.components().count() > 1 {
            return path.exists();
        }
        std::env::var_os("PATH")
            .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).exists()))
    }

    #[cfg(feature = "stt")]
    fn is_command_available_path(path: &Path) -> bool {
        if path.components().count() > 1 {
            return path.exists();
        }
        path.to_str().is_some_and(Self::is_command_available)
    }

    #[cfg_attr(not(feature = "stt"), allow(dead_code))]
    fn data_dir_from_config(config: &Config) -> Result<PathBuf> {
        if let Some(data_dir) = &config.core.data_dir {
            if data_dir == "~" || data_dir.starts_with("~/") {
                let home =
                    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
                if data_dir == "~" {
                    Ok(home)
                } else {
                    Ok(home.join(data_dir.trim_start_matches("~/")))
                }
            } else {
                Ok(PathBuf::from(data_dir))
            }
        } else {
            let home =
                dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
            Ok(home.join(".masix"))
        }
    }

    async fn execute_cron_tool(
        arguments: serde_json::Value,
        storage: &Arc<Mutex<Storage>>,
        envelope: &Envelope,
        account_tag: Option<&str>,
    ) -> Result<String> {
        let Some(chat_id) = envelope.chat_id else {
            return Ok("Cron tool unavailable: missing chat context.".to_string());
        };

        let command = arguments
            .get("command")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or("help");
        let scoped_account_tag = account_tag
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("__default__");
        let recipient = chat_id.to_string();

        Self::execute_cron_instruction(
            command,
            &envelope.channel,
            &recipient,
            scoped_account_tag,
            storage,
        )
        .await
    }

    fn execute_vision_tool(
        _arguments: serde_json::Value,
        envelope: &Envelope,
        vision_analysis: Option<&str>,
    ) -> Result<String> {
        if let Some(analysis) = vision_analysis {
            return Ok(format!("Vision analysis:\n{}", analysis));
        }

        if let Some(summary) = Self::media_summary_from_payload(&envelope.payload) {
            Ok(format!(
                "Media metadata available:\n{}\n\nNote: OCR/image understanding is not enabled yet; this tool currently exposes media context only.",
                summary
            ))
        } else {
            Ok("No media metadata available in this message context.".to_string())
        }
    }

    async fn execute_cron_instruction(
        command: &str,
        channel: &str,
        recipient: &str,
        scoped_account_tag: &str,
        storage: &Arc<Mutex<Storage>>,
    ) -> Result<String> {
        let rest = command.trim();

        if rest.is_empty() || rest.eq_ignore_ascii_case("help") {
            return Ok(
                "Reminder commands:\n- `domani alle 9 \"Meeting\"`\n- `list`\n- `cancel <id>`"
                    .to_string(),
            );
        }

        if rest.eq_ignore_ascii_case("list") {
            let storage_guard = storage.lock().await;
            let jobs = storage_guard
                .list_enabled_cron_jobs_for_account_recipient(scoped_account_tag, recipient)?;
            drop(storage_guard);

            if jobs.is_empty() {
                return Ok("Nessun reminder attivo per questa chat.".to_string());
            }

            let mut lines = vec!["Reminder attivi:".to_string()];
            for job in jobs {
                lines.push(format!(
                    "- ID {} | {} | recurring: {}\n  {}",
                    job.id, job.schedule, job.recurring, job.message
                ));
            }
            return Ok(lines.join("\n"));
        }

        let lower = rest.to_lowercase();
        if lower.starts_with("cancel ") || lower.starts_with("delete ") {
            let id_part = rest.split_whitespace().nth(1).unwrap_or_default();
            let id = id_part
                .parse::<i64>()
                .map_err(|_| anyhow!("Invalid cron id '{}'", id_part))?;
            let storage_guard = storage.lock().await;
            let changed = storage_guard.disable_cron_job_for_account(id, scoped_account_tag)?;
            drop(storage_guard);

            if changed {
                return Ok(format!("Reminder {} eliminato.", id));
            }
            return Ok(format!(
                "Reminder {} non trovato per questo bot/chat (scope account: {}).",
                id, scoped_account_tag
            ));
        }

        let parser = masix_cron::CronParser::new();
        let parsed = parser.parse(rest, channel, recipient)?;
        let storage_guard = storage.lock().await;
        let id = storage_guard.create_cron_job(
            channel,
            &parsed.schedule,
            &parsed.channel,
            &parsed.recipient,
            Some(scoped_account_tag),
            &parsed.message,
            &parsed.timezone,
            parsed.recurring,
        )?;
        drop(storage_guard);

        Ok(format!(
            "Reminder creato.\nID: {}\nSchedule: {}\nRecurring: {}\nMessage: {}",
            id, parsed.schedule, parsed.recurring, parsed.message
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_chat_commands(
        text: &str,
        envelope: &Envelope,
        outbound_sender: &broadcast::Sender<OutboundMessage>,
        account_tag: Option<&str>,
        user_state_key: &str,
        user_languages: &Arc<Mutex<HashMap<String, Language>>>,
        user_providers: &Arc<Mutex<HashMap<String, String>>>,
        user_models: &Arc<Mutex<HashMap<String, String>>>,
        bot_context: &BotContext,
        user_scope_id: Option<&str>,
        config: &Config,
        from: &str,
        from_user_id: i64,
        permission: PermissionLevel,
        mcp_client: &Option<Arc<Mutex<McpClient>>>,
    ) -> Result<bool> {
        let Some(chat_id) = envelope.chat_id else {
            return Ok(false);
        };
        let account_tag_owned = account_tag.map(|value| value.to_string());

        if text == "/" {
            let lang = user_languages
                .lock()
                .await
                .get(user_state_key)
                .copied()
                .unwrap_or_default();
            let cmd_list =
                masix_telegram::menu::command_list(lang, permission == PermissionLevel::Admin);
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag_owned.clone(),
                chat_id,
                &cmd_list,
                None,
            );
            return Ok(true);
        }

        if text.starts_with("/start") || text.starts_with("/menu") {
            info!("Processing menu command");
            let lang = user_languages
                .lock()
                .await
                .get(user_state_key)
                .copied()
                .unwrap_or_default();
            let (menu_text, keyboard) =
                masix_telegram::menu::home_menu(lang, permission == PermissionLevel::Admin);
            let msg = OutboundMessage {
                channel: envelope.channel.clone(),
                account_tag: account_tag_owned.clone(),
                chat_id,
                text: menu_text,
                reply_to: None,
                edit_message_id: None,
                inline_keyboard: Some(keyboard),
                chat_action: None,
            };
            if let Err(e) = outbound_sender.send(msg) {
                error!("Failed to send menu: {}", e);
            }
            return Ok(true);
        }

        if text.starts_with("/new") {
            info!("Processing /new - session reset");
            let lang = user_languages
                .lock()
                .await
                .get(user_state_key)
                .copied()
                .unwrap_or_default();
            let reset_text = masix_telegram::menu::session_reset_text(lang);
            Self::clear_chat_memory(bot_context, account_tag, user_scope_id, Some(chat_id)).await;
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag_owned.clone(),
                chat_id,
                &reset_text,
                None,
            );
            return Ok(true);
        }

        if text.starts_with("/help") {
            info!("Processing /help");
            let lang = user_languages
                .lock()
                .await
                .get(user_state_key)
                .copied()
                .unwrap_or_default();
            let help_text =
                masix_telegram::menu::help_text(lang, permission == PermissionLevel::Admin);
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag_owned.clone(),
                chat_id,
                &help_text,
                None,
            );
            return Ok(true);
        }

        if text.starts_with("/whoiam") {
            info!("Processing /whoiam");
            let chat_type = envelope
                .payload
                .get("chat_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let scope = if envelope.channel == "telegram" {
                if from_user_id == chat_id {
                    "private"
                } else {
                    "group_or_channel"
                }
            } else {
                "n/a"
            };
            let message = format!(
                "👤 Identity\nChannel: {}\nAccount tag: {}\nSender: {}\nUser ID: {}\nChat ID: {}\nChat type: {}\nScope: {}\nPermission: {:?}",
                envelope.channel,
                account_tag.unwrap_or("__default__"),
                from,
                from_user_id,
                chat_id,
                chat_type,
                scope,
                permission
            );
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag_owned.clone(),
                chat_id,
                &message,
                envelope.message_id,
            );
            return Ok(true);
        }

        if text.starts_with("/language") {
            info!("Processing /language");
            let lang = user_languages
                .lock()
                .await
                .get(user_state_key)
                .copied()
                .unwrap_or_default();
            let (menu_text, keyboard) = masix_telegram::menu::language_menu(lang);
            let msg = OutboundMessage {
                channel: envelope.channel.clone(),
                account_tag: account_tag_owned.clone(),
                chat_id,
                text: menu_text,
                reply_to: None,
                edit_message_id: None,
                inline_keyboard: Some(keyboard),
                chat_action: None,
            };
            let _ = outbound_sender.send(msg);
            return Ok(true);
        }

        if text.starts_with("/provider") {
            info!("Processing /provider");
            let response =
                Self::handle_provider_chat_command(text, user_state_key, config, user_providers)
                    .await;
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag_owned.clone(),
                chat_id,
                &response,
                None,
            );
            return Ok(true);
        }

        if text.starts_with("/model") {
            info!("Processing /model");
            let response = Self::handle_model_chat_command(
                text,
                user_state_key,
                config,
                user_providers,
                user_models,
            )
            .await;
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag_owned.clone(),
                chat_id,
                &response,
                None,
            );
            return Ok(true);
        }

        if text.starts_with("/admin") {
            info!("Processing /admin");
            let response =
                Self::handle_admin_command(text, config, account_tag, from_user_id, permission)
                    .await;
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag_owned.clone(),
                chat_id,
                &response,
                None,
            );
            return Ok(true);
        }

        if text.starts_with("/mcp") {
            if permission != PermissionLevel::Admin {
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag_owned.clone(),
                    chat_id,
                    "Admin only command.",
                    None,
                );
                return Ok(true);
            }
            info!("Processing /mcp");
            let response = Self::handle_mcp_chat_command(text, config).await;
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag_owned.clone(),
                chat_id,
                &response,
                None,
            );
            return Ok(true);
        }

        if text.starts_with("/tools") {
            if permission != PermissionLevel::Admin {
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag_owned.clone(),
                    chat_id,
                    "Admin only command.",
                    None,
                );
                return Ok(true);
            }
            info!("Processing /tools");
            let response = Self::handle_tools_chat_command(mcp_client).await;
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag_owned.clone(),
                chat_id,
                &response,
                None,
            );
            return Ok(true);
        }

        Ok(false)
    }

    async fn handle_cron_command(
        text: &str,
        envelope: &Envelope,
        outbound_sender: &broadcast::Sender<OutboundMessage>,
        storage: &Arc<Mutex<Storage>>,
        account_tag: Option<String>,
    ) -> Result<bool> {
        let trimmed = text.trim();
        if !(trimmed == "/cron" || trimmed.starts_with("/cron ")) {
            return Ok(false);
        }

        let Some(chat_id) = envelope.chat_id else {
            return Ok(true);
        };

        let rest = trimmed.strip_prefix("/cron").unwrap_or("").trim();
        let scoped_account_tag = account_tag
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("__default__");
        let recipient = chat_id.to_string();
        let response = Self::execute_cron_instruction(
            rest,
            &envelope.channel,
            &recipient,
            scoped_account_tag,
            storage,
        )
        .await?;

        Self::send_outbound_text(
            outbound_sender,
            &envelope.channel,
            account_tag.clone(),
            chat_id,
            &response,
            envelope.message_id,
        );

        Ok(true)
    }

    async fn handle_exec_command(
        text: &str,
        envelope: &Envelope,
        outbound_sender: &broadcast::Sender<OutboundMessage>,
        bot_context: &BotContext,
        account_tag: Option<String>,
        permission: PermissionLevel,
    ) -> Result<bool> {
        let trimmed = text.trim();
        let Some(chat_id) = envelope.chat_id else {
            return Ok(false);
        };

        if trimmed == "/exec" || trimmed.starts_with("/exec ") {
            if permission != PermissionLevel::Admin {
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    "Admin only command.",
                    envelope.message_id,
                );
                return Ok(true);
            }

            let rest = trimmed.strip_prefix("/exec").unwrap_or("");
            let command = rest.trim();
            if command.is_empty() || command.eq_ignore_ascii_case("help") {
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    "Usage: `/exec <command>`\nExample: `/exec ls -la`\nExecutes only allowlisted commands in bot workdir.",
                    envelope.message_id,
                );
                return Ok(true);
            }

            let response = match run_command(
                &bot_context.exec_policy,
                ExecMode::Base,
                command,
                &bot_context.workdir,
            )
            .await
            {
                Ok(result) => result.format_for_chat(),
                Err(e) => format!("Exec error: {}", e),
            };

            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag.clone(),
                chat_id,
                &response,
                envelope.message_id,
            );
            return Ok(true);
        }

        if trimmed == "/termux" || trimmed.starts_with("/termux ") {
            let rest = trimmed.strip_prefix("/termux").unwrap_or("");
            let command = rest.trim();
            let is_termux = is_termux_environment();

            if command.is_empty() || command.eq_ignore_ascii_case("help") {
                let help_text = if is_termux {
                    "Uso:\n- `/termux info`\n- `/termux battery`\n- `/termux cmd <termux-command>`\n- `/termux boot on|off|status`\n- `/termux wake on|off|status`"
                } else {
                    "Uso desktop:\n- `/termux boot on|off|status`\n\nNote:\n- i comandi `info/battery/cmd` e `wake` richiedono Android Termux."
                };
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    help_text,
                    envelope.message_id,
                );
                return Ok(true);
            }

            if let Some(wake_value) = command.strip_prefix("wake ").map(str::trim) {
                if !is_termux {
                    Self::send_outbound_text(
                        outbound_sender,
                        &envelope.channel,
                        account_tag.clone(),
                        chat_id,
                        "Wake lock disponibile solo su Android Termux.",
                        envelope.message_id,
                    );
                    return Ok(true);
                }

                let action = match wake_value.to_lowercase().as_str() {
                    "on" | "enable" => WakeLockAction::Enable,
                    "off" | "disable" => WakeLockAction::Disable,
                    "status" => WakeLockAction::Status,
                    _ => {
                        Self::send_outbound_text(
                            outbound_sender,
                            &envelope.channel,
                            account_tag.clone(),
                            chat_id,
                            "Valore non valido. Usa `/termux wake on|off|status`.",
                            envelope.message_id,
                        );
                        return Ok(true);
                    }
                };

                let out = match manage_termux_wake_lock(action, None).await {
                    Ok(status) => {
                        if action == WakeLockAction::Status {
                            format!(
                                "Termux wake lock:\nSupported: {}\nEnabled: {}\nState: `{}`",
                                status.supported,
                                status.enabled,
                                status.state_path.display()
                            )
                        } else {
                            format!(
                                "Termux wake lock aggiornato.\nSupported: {}\nEnabled: {}\nState: `{}`",
                                status.supported,
                                status.enabled,
                                status.state_path.display()
                            )
                        }
                    }
                    Err(e) => format!("Termux wake lock error: {}", e),
                };

                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    &out,
                    envelope.message_id,
                );
                return Ok(true);
            }

            if let Some(boot_value) = command.strip_prefix("boot ").map(str::trim) {
                let action = match boot_value.to_lowercase().as_str() {
                    "on" | "enable" => BootAction::Enable,
                    "off" | "disable" => BootAction::Disable,
                    "status" => BootAction::Status,
                    _ => {
                        Self::send_outbound_text(
                            outbound_sender,
                            &envelope.channel,
                            account_tag.clone(),
                            chat_id,
                            "Valore non valido. Usa `/termux boot on|off|status`.",
                            envelope.message_id,
                        );
                        return Ok(true);
                    }
                };

                let masix_bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("masix"));
                let out = match manage_termux_boot(action, &masix_bin, None).await {
                    Ok(status) => {
                        if action == BootAction::Status {
                            format!(
                                "Termux boot script: `{}`\nMethod: `{}`\nEnabled: {}",
                                status.script_path.display(),
                                status.method,
                                status.enabled
                            )
                        } else {
                            format!(
                                "Termux boot aggiornato.\nScript: `{}`\nMethod: `{}`\nEnabled: {}",
                                status.script_path.display(),
                                status.method,
                                status.enabled
                            )
                        }
                    }
                    Err(e) => format!("Termux boot error: {}", e),
                };
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    &out,
                    envelope.message_id,
                );
                return Ok(true);
            }

            if !is_termux {
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    "Comando non disponibile su questa piattaforma. Usa `/termux boot on|off|status`.",
                    envelope.message_id,
                );
                return Ok(true);
            }

            let mapped_command = if let Some(cmd) = command.strip_prefix("cmd ").map(str::trim) {
                cmd.to_string()
            } else {
                match command.to_lowercase().as_str() {
                    "info" => "termux-info".to_string(),
                    "battery" => "termux-battery-status".to_string(),
                    "location" => "termux-location".to_string(),
                    "wifi" => "termux-wifi-connectioninfo".to_string(),
                    "device" => "termux-telephony-deviceinfo".to_string(),
                    "clipboard" => "termux-clipboard-get".to_string(),
                    _ => {
                        Self::send_outbound_text(
                            outbound_sender,
                            &envelope.channel,
                            account_tag.clone(),
                            chat_id,
                            "Comando non riconosciuto. Usa `/termux help`.",
                            envelope.message_id,
                        );
                        return Ok(true);
                    }
                }
            };

            let response = match run_command(
                &bot_context.exec_policy,
                ExecMode::Termux,
                &mapped_command,
                &bot_context.workdir,
            )
            .await
            {
                Ok(result) => result.format_for_chat(),
                Err(e) => format!("Termux exec error: {}", e),
            };

            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag.clone(),
                chat_id,
                &response,
                envelope.message_id,
            );
            return Ok(true);
        }

        Ok(false)
    }

    async fn handle_admin_command(
        text: &str,
        config: &Config,
        account_tag: Option<&str>,
        _from_user_id: i64,
        permission: PermissionLevel,
    ) -> String {
        if permission != PermissionLevel::Admin {
            return "Admin only command.".to_string();
        }

        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() < 2 {
            return "🛡️ Admin commands\n\
/admin list\n\
/admin add <user_id>\n\
/admin remove <user_id>\n\
/admin promote <user_id>\n\
/admin demote <user_id>\n\
/admin tools user list\n\
/admin tools user available\n\
/admin tools user mode <none|selected>\n\
/admin tools user allow <tool_name>\n\
/admin tools user deny <tool_name>\n\
/admin tools user clear"
                .to_string();
        }

        let account = match Self::get_telegram_account(config, account_tag) {
            Some(a) => a,
            None => return "No account found.".to_string(),
        };

        let path = Self::effective_register_file_path(account);
        let register_source = if account
            .register_to_file
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
        {
            "configured"
        } else {
            "default"
        };

        let mut payload = if path.exists() {
            match fs::read_to_string(&path).await {
                Ok(content) => serde_json::from_str::<serde_json::Value>(&content)
                    .unwrap_or_else(|_| serde_json::json!({})),
                Err(_) => serde_json::json!({}),
            }
        } else {
            serde_json::json!({})
        };
        if !payload.is_object() {
            payload = serde_json::json!({});
        }
        let mut acl = Self::parse_dynamic_acl(&payload);

        match parts[1].to_lowercase().as_str() {
            "tools" => {
                Self::handle_admin_tools_subcommand(&parts, account, &path, &mut payload, &mut acl)
                    .await
            }
            "list" => {
                let mut admins: HashSet<i64> = account.admins.iter().copied().collect();
                admins.extend(acl.admins.iter().copied());

                let mut users: HashSet<i64> = account.users.iter().copied().collect();
                users.extend(acl.users.iter().copied());

                let mut readonly: HashSet<i64> = account.readonly.iter().copied().collect();
                readonly.extend(acl.readonly.iter().copied());

                let admins = Self::sorted_csv(&admins);
                let users = Self::sorted_csv(&users);
                let readonly = Self::sorted_csv(&readonly);
                format!(
                    "Admins: {}\nUsers: {}\nReadonly: {}\nRegister file: {}\nRegister file source: {}",
                    admins,
                    users,
                    readonly,
                    path.display(),
                    register_source
                )
            }
            "add" => {
                if parts.len() < 3 {
                    return "Usage: /admin add <user_id>".to_string();
                }
                match parts[2].parse::<i64>() {
                    Ok(user_id) => {
                        acl.users.insert(user_id);
                        acl.admins.remove(&user_id);
                        acl.readonly.remove(&user_id);
                        if let Err(err) =
                            Self::persist_dynamic_acl_payload(&path, &mut payload, &acl).await
                        {
                            return format!("Failed to persist ACL: {}", err);
                        }
                        format!("User {} added and active immediately.", user_id)
                    }
                    Err(_) => "Invalid user ID. Use numeric ID.".to_string(),
                }
            }
            "remove" => {
                if parts.len() < 3 {
                    return "Usage: /admin remove <user_id>".to_string();
                }
                match parts[2].parse::<i64>() {
                    Ok(user_id) => {
                        acl.admins.remove(&user_id);
                        acl.users.remove(&user_id);
                        acl.readonly.remove(&user_id);
                        if let Some(obj) = payload.as_object_mut() {
                            obj.remove(&user_id.to_string());
                        }
                        if let Err(err) =
                            Self::persist_dynamic_acl_payload(&path, &mut payload, &acl).await
                        {
                            return format!("Failed to persist ACL: {}", err);
                        }
                        format!("User {} removed and deauthorized immediately.", user_id)
                    }
                    Err(_) => "Invalid user ID.".to_string(),
                }
            }
            "promote" => {
                if parts.len() < 3 {
                    return "Usage: /admin promote <user_id>".to_string();
                }
                match parts[2].parse::<i64>() {
                    Ok(user_id) => {
                        acl.admins.insert(user_id);
                        acl.users.remove(&user_id);
                        acl.readonly.remove(&user_id);
                        if let Err(err) =
                            Self::persist_dynamic_acl_payload(&path, &mut payload, &acl).await
                        {
                            return format!("Failed to persist ACL: {}", err);
                        }
                        format!("User {} promoted to admin and active immediately.", user_id)
                    }
                    Err(_) => "Invalid user ID.".to_string(),
                }
            }
            "demote" => {
                if parts.len() < 3 {
                    return "Usage: /admin demote <user_id>".to_string();
                }
                match parts[2].parse::<i64>() {
                    Ok(user_id) => {
                        acl.admins.remove(&user_id);
                        acl.users.insert(user_id);
                        acl.readonly.remove(&user_id);
                        if let Err(err) =
                            Self::persist_dynamic_acl_payload(&path, &mut payload, &acl).await
                        {
                            return format!("Failed to persist ACL: {}", err);
                        }
                        format!("Admin {} demoted to user and active immediately.", user_id)
                    }
                    Err(_) => "Invalid user ID.".to_string(),
                }
            }
            _ => "Unknown command. Use: add, remove, promote, demote, list, tools".to_string(),
        }
    }

    async fn handle_admin_tools_subcommand(
        parts: &[&str],
        account: &masix_config::TelegramAccount,
        path: &Path,
        payload: &mut serde_json::Value,
        acl: &mut DynamicAcl,
    ) -> String {
        if parts.len() < 3 {
            return "Usage: /admin tools user <list|mode|allow|deny|clear|available> ..."
                .to_string();
        }

        if !parts[2].eq_ignore_ascii_case("user") {
            return "Only `user` role is supported for now. Usage: /admin tools user <...>"
                .to_string();
        }

        let action = parts.get(3).map(|s| s.to_ascii_lowercase());
        let action = action.as_deref().unwrap_or("list");

        match action {
            "list" => {
                let (effective_mode, effective_allowlist) =
                    Self::effective_telegram_user_tool_policy(account, acl);
                let static_allowlist = Self::normalized_account_user_tool_allowlist(account);
                let dynamic_allowlist = acl.user_allowed_tools.clone().unwrap_or_default();
                format!(
                    "User tools mode (effective): {}\nUser tools allowlist (effective): {}\nStatic mode: {}\nStatic allowlist: {}\nDynamic mode override: {}\nDynamic allowlist override: {}\n\nCommands:\n/admin tools user mode <none|selected>\n/admin tools user allow <tool_name>\n/admin tools user deny <tool_name>\n/admin tools user clear\n/admin tools user available",
                    Self::user_tools_mode_label(effective_mode),
                    Self::sorted_string_csv(&effective_allowlist),
                    Self::user_tools_mode_label(account.user_tools_mode),
                    Self::sorted_string_csv(&static_allowlist),
                    acl.user_tools_mode
                        .map(Self::user_tools_mode_label)
                        .unwrap_or("(none)"),
                    if acl.user_allowed_tools.is_some() {
                        Self::sorted_string_csv(&dynamic_allowlist)
                    } else {
                        "(none)".to_string()
                    }
                )
            }
            "available" => {
                let mut names: HashSet<String> = get_builtin_tool_definitions()
                    .into_iter()
                    .filter_map(|tool| Self::normalize_tool_name(&tool.function.name))
                    .collect();
                let mut list: Vec<String> = names.drain().collect();
                list.sort();
                let joined = if list.is_empty() {
                    "(none)".to_string()
                } else {
                    list.join(", ")
                };
                format!(
                    "Built-in runtime tools (whitelist names): {}\nUse /tools (admin) to inspect current runtime tools including MCP-prefixed names.",
                    joined
                )
            }
            "mode" => {
                let Some(mode_raw) = parts.get(4) else {
                    return "Usage: /admin tools user mode <none|selected>".to_string();
                };
                let mode = match mode_raw.trim().to_lowercase().as_str() {
                    "none" => UserToolsMode::None,
                    "selected" => UserToolsMode::Selected,
                    _ => return "Invalid mode. Use: none | selected".to_string(),
                };
                acl.user_tools_mode = Some(mode);
                if let Err(err) = Self::persist_dynamic_acl_payload(path, payload, acl).await {
                    return format!("Failed to persist ACL/tool policy: {}", err);
                }
                format!("User tools mode set to '{}' and active immediately.", Self::user_tools_mode_label(mode))
            }
            "allow" => {
                let Some(tool_raw) = parts.get(4) else {
                    return "Usage: /admin tools user allow <tool_name>".to_string();
                };
                let Some(tool_name) = Self::normalize_tool_name(tool_raw) else {
                    return "Invalid tool name.".to_string();
                };
                let (_, effective_allowlist) = Self::effective_telegram_user_tool_policy(account, acl);
                let allowlist = acl.user_allowed_tools.get_or_insert(effective_allowlist);
                allowlist.insert(tool_name.clone());
                acl.user_tools_mode = Some(UserToolsMode::Selected);
                if let Err(err) = Self::persist_dynamic_acl_payload(path, payload, acl).await {
                    return format!("Failed to persist ACL/tool policy: {}", err);
                }
                format!(
                    "User tool '{}' allowed (mode=selected) and active immediately.",
                    tool_name
                )
            }
            "deny" => {
                let Some(tool_raw) = parts.get(4) else {
                    return "Usage: /admin tools user deny <tool_name>".to_string();
                };
                let Some(tool_name) = Self::normalize_tool_name(tool_raw) else {
                    return "Invalid tool name.".to_string();
                };
                let (_, effective_allowlist) = Self::effective_telegram_user_tool_policy(account, acl);
                let allowlist = acl.user_allowed_tools.get_or_insert(effective_allowlist);
                let removed = allowlist.remove(&tool_name);
                if let Err(err) = Self::persist_dynamic_acl_payload(path, payload, acl).await {
                    return format!("Failed to persist ACL/tool policy: {}", err);
                }
                if removed {
                    format!("User tool '{}' denied and active immediately.", tool_name)
                } else {
                    format!("User tool '{}' was not in allowlist.", tool_name)
                }
            }
            "clear" => {
                acl.user_allowed_tools = Some(HashSet::new());
                if let Err(err) = Self::persist_dynamic_acl_payload(path, payload, acl).await {
                    return format!("Failed to persist ACL/tool policy: {}", err);
                }
                "User tool allowlist cleared (effective if mode=selected).".to_string()
            }
            _ => {
                "Unknown tools command. Use: /admin tools user <list|mode|allow|deny|clear|available>"
                    .to_string()
            }
        }
    }

    fn sorted_csv(values: &HashSet<i64>) -> String {
        let mut list: Vec<i64> = values.iter().copied().collect();
        list.sort_unstable();
        if list.is_empty() {
            "(none)".to_string()
        } else {
            list.into_iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    fn sorted_string_csv(values: &HashSet<String>) -> String {
        let mut list: Vec<String> = values.iter().cloned().collect();
        list.sort();
        if list.is_empty() {
            "(none)".to_string()
        } else {
            list.join(", ")
        }
    }

    fn user_tools_mode_label(mode: UserToolsMode) -> &'static str {
        match mode {
            UserToolsMode::None => "none",
            UserToolsMode::Selected => "selected",
        }
    }

    async fn persist_dynamic_acl_payload(
        path: &Path,
        payload: &mut serde_json::Value,
        acl: &DynamicAcl,
    ) -> Result<()> {
        if !payload.is_object() {
            *payload = serde_json::json!({});
        }

        let mut admins: Vec<i64> = acl.admins.iter().copied().collect();
        let mut users: Vec<i64> = acl.users.iter().copied().collect();
        let mut readonly: Vec<i64> = acl.readonly.iter().copied().collect();
        let mut user_allowed_tools: Vec<String> = acl
            .user_allowed_tools
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        admins.sort_unstable();
        users.sort_unstable();
        readonly.sort_unstable();
        user_allowed_tools.sort();

        if let Some(obj) = payload.as_object_mut() {
            obj.insert("admins".to_string(), serde_json::json!(admins));
            obj.insert("users".to_string(), serde_json::json!(users));
            obj.insert("readonly".to_string(), serde_json::json!(readonly));
            if let Some(mode) = acl.user_tools_mode {
                obj.insert(
                    "user_tools_mode".to_string(),
                    serde_json::json!(Self::user_tools_mode_label(mode)),
                );
            } else {
                obj.remove("user_tools_mode");
            }
            if acl.user_allowed_tools.is_some() {
                obj.insert(
                    "user_allowed_tools".to_string(),
                    serde_json::json!(user_allowed_tools),
                );
            } else {
                obj.remove("user_allowed_tools");
            }
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let content = serde_json::to_string_pretty(payload)?;
        fs::write(path, content).await?;
        Ok(())
    }

    async fn register_user(config: &Config, account_tag: Option<&str>, user_id: i64) -> Result<()> {
        let account = Self::get_telegram_account(config, account_tag);
        if let Some(account) = account {
            let path = Self::effective_register_file_path(account);

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }

            let mut registered = if path.exists() {
                let content = fs::read_to_string(&path).await?;
                serde_json::from_str::<serde_json::Value>(&content).unwrap_or(serde_json::json!({}))
            } else {
                serde_json::json!({})
            };

            let timestamp = chrono::Utc::now().to_rfc3339();
            if let Some(obj) = registered.as_object_mut() {
                obj.insert(
                    user_id.to_string(),
                    serde_json::json!({
                        "first_seen": timestamp,
                        "source": "auto_register"
                    }),
                );
            }

            let content = serde_json::to_string_pretty(&registered)?;
            fs::write(&path, content).await?;
            info!("Auto-registered user {} to {}", user_id, path.display());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn chat_with_fallback_chain(
        provider_router: &ProviderRouter,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<ToolDefinition>>,
        provider_chain: &[String],
        preferred_provider: Option<&str>,
        preferred_model: Option<&str>,
        retry_policy: &RetryPolicy,
        profile_name: &str,
    ) -> Result<(masix_providers::ChatResponse, String)> {
        const MAX_ATTEMPTS_PER_PROVIDER: usize = 3;
        let preferred_provider =
            preferred_provider.filter(|name| provider_router.get_provider(Some(name)).is_some());

        if provider_chain.is_empty() {
            return Err(anyhow::anyhow!("Provider chain is empty"));
        }

        let mut effective_chain: Vec<String> = provider_chain.to_vec();
        if let Some(preferred) = preferred_provider {
            if let Some(idx) = effective_chain.iter().position(|name| name == preferred) {
                if idx != 0 {
                    effective_chain.rotate_left(idx);
                }
            } else if provider_router.get_provider(Some(preferred)).is_some() {
                effective_chain.insert(0, preferred.to_string());
            }
        }

        // Single provider mode: use retry logic inside provider
        if effective_chain.len() <= 1 {
            let provider_name =
                preferred_provider.or_else(|| effective_chain.first().map(|s| s.as_str()));
            let response = if let Some(tool_defs) = &tools {
                provider_router
                    .chat_with_tools(
                        messages,
                        tool_defs.clone(),
                        provider_name,
                        preferred_model,
                        Some(retry_policy),
                    )
                    .await?
            } else {
                provider_router
                    .chat(messages, provider_name, preferred_model, Some(retry_policy))
                    .await?
            };
            let used = provider_name.unwrap_or("default").to_string();
            return Ok((response, used));
        }

        // Multi-provider fallback mode: rotate after 3 failures
        let mut current_idx = 0;
        let mut attempts_on_current = 0;
        let mut last_error: Option<anyhow::Error>;
        let mut tried_all = false;

        loop {
            let provider_name = &effective_chain[current_idx];

            let result = if let Some(tool_defs) = &tools {
                provider_router
                    .chat_with_tools(
                        messages.clone(),
                        tool_defs.clone(),
                        Some(provider_name),
                        preferred_model,
                        Some(retry_policy),
                    )
                    .await
            } else {
                provider_router
                    .chat(
                        messages.clone(),
                        Some(provider_name),
                        preferred_model,
                        Some(retry_policy),
                    )
                    .await
            };

            match result {
                Ok(response) => {
                    info!(
                        "Provider '{}' succeeded for bot '{}' after {} attempts",
                        provider_name,
                        profile_name,
                        attempts_on_current + 1
                    );
                    return Ok((response, provider_name.clone()));
                }
                Err(e) => {
                    attempts_on_current += 1;
                    let err_msg = e.to_string();

                    if Self::is_auth_error(&e) {
                        return Err(anyhow::anyhow!(
                            "Provider '{}' auth/permission error for bot '{}': {}",
                            provider_name,
                            profile_name,
                            err_msg
                        ));
                    }

                    if Self::is_rate_limit_error(&e) {
                        warn!(
                            "Provider '{}' hit rate limit for bot '{}'; retry/backoff+fallback in progress",
                            provider_name, profile_name
                        );
                    }

                    warn!(
                        "Provider '{}' failed for bot '{}' (attempt {}/{}): {}",
                        provider_name,
                        profile_name,
                        attempts_on_current,
                        MAX_ATTEMPTS_PER_PROVIDER,
                        err_msg
                    );

                    last_error = Some(e);

                    // Switch to next provider after max attempts
                    if attempts_on_current >= MAX_ATTEMPTS_PER_PROVIDER {
                        let prev_idx = current_idx;
                        current_idx = (current_idx + 1) % effective_chain.len();

                        // Check if we've tried all providers
                        if current_idx == 0 {
                            if tried_all {
                                // Full cycle completed, all providers exhausted
                                break;
                            }
                            tried_all = true;
                        }

                        warn!(
                            "Switching from '{}' to '{}' for bot '{}'",
                            effective_chain[prev_idx], effective_chain[current_idx], profile_name
                        );
                        attempts_on_current = 0;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All providers exhausted")))
    }

    fn is_auth_error(err: &anyhow::Error) -> bool {
        let msg = err.to_string().to_lowercase();
        msg.contains("401")
            || msg.contains("403")
            || msg.contains("unauthorized")
            || msg.contains("forbidden")
            || msg.contains("api key")
    }

    fn is_rate_limit_error(err: &anyhow::Error) -> bool {
        let msg = err.to_string().to_lowercase();
        msg.contains("429")
            || msg.contains("rate limit")
            || msg.contains("too many requests")
            || msg.contains("retry-after")
    }

    async fn load_bot_memory_file(context: &BotContext) -> Option<String> {
        fs::read_to_string(&context.memory_file).await.ok()
    }

    fn resolve_user_scope_id(envelope: &Envelope) -> Option<String> {
        if let MessageKind::Message { from, .. } = &envelope.kind {
            let trimmed = from.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        if let Some(value) = envelope.payload.get("from_user_id") {
            if let Some(id) = value.as_i64() {
                return Some(id.to_string());
            }
            if let Some(id) = value.as_str() {
                let trimmed = id.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }

        None
    }

    fn normalized_user_id(user_scope_id: Option<&str>, chat_id: Option<i64>) -> String {
        user_scope_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .or_else(|| chat_id.map(|id| format!("chat_{}", id)))
            .unwrap_or_else(|| "anonymous".to_string())
    }

    fn sanitize_scope_component(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for ch in value.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                out.push(ch);
            } else {
                out.push('_');
            }
        }
        let trimmed = out.trim_matches('_');
        if trimmed.is_empty() {
            "unknown".to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn account_scope(account_tag: Option<&str>) -> String {
        account_tag
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("__default__")
            .to_string()
    }

    fn user_state_key(
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
    ) -> String {
        let account = Self::sanitize_scope_component(&Self::account_scope(account_tag));
        let user =
            Self::sanitize_scope_component(&Self::normalized_user_id(user_scope_id, chat_id));
        format!("{}::{}", account, user)
    }

    fn scoped_value(account_tag: Option<&str>, value: &str) -> String {
        let account = Self::sanitize_scope_component(&Self::account_scope(account_tag));
        format!("{}::{}", account, value.trim())
    }

    fn get_telegram_account<'a>(
        config: &'a Config,
        account_tag: Option<&str>,
    ) -> Option<&'a masix_config::TelegramAccount> {
        let telegram = config.telegram.as_ref()?;
        let tag = account_tag.unwrap_or("");
        telegram.accounts.iter().find(|a| {
            let a_tag = masix_config::telegram_account_tag(&a.bot_token);
            a_tag == tag || (tag.is_empty() && telegram.accounts.len() == 1)
        })
    }

    fn resolve_sender_user_id(envelope: &Envelope, sender: &str) -> i64 {
        if let Some(value) = envelope.payload.get("from_user_id") {
            if let Some(id) = value.as_i64() {
                return id;
            }
            if let Some(raw) = value.as_str() {
                if let Ok(id) = raw.trim().parse::<i64>() {
                    return id;
                }
            }
        }

        if let Ok(id) = sender.trim().parse::<i64>() {
            return id;
        }

        envelope.chat_id.unwrap_or(0)
    }

    fn parse_id_array(value: Option<&serde_json::Value>) -> HashSet<i64> {
        let mut out = HashSet::new();
        let Some(serde_json::Value::Array(items)) = value else {
            return out;
        };
        for item in items {
            if let Some(id) = item.as_i64() {
                out.insert(id);
                continue;
            }
            if let Some(raw) = item.as_str() {
                if let Ok(id) = raw.trim().parse::<i64>() {
                    out.insert(id);
                }
            }
        }
        out
    }

    fn normalize_tool_name(value: &str) -> Option<String> {
        let normalized = value.trim().to_lowercase();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    }

    fn parse_tool_name_array(value: Option<&serde_json::Value>) -> Option<HashSet<String>> {
        let Some(serde_json::Value::Array(items)) = value else {
            return None;
        };
        let mut out = HashSet::new();
        for item in items {
            if let Some(raw) = item.as_str() {
                if let Some(name) = Self::normalize_tool_name(raw) {
                    out.insert(name);
                }
            }
        }
        Some(out)
    }

    fn parse_user_tools_mode(value: Option<&serde_json::Value>) -> Option<UserToolsMode> {
        let raw = value.and_then(|v| v.as_str())?;
        match raw.trim().to_lowercase().as_str() {
            "none" => Some(UserToolsMode::None),
            "selected" => Some(UserToolsMode::Selected),
            _ => None,
        }
    }

    fn parse_dynamic_acl(value: &serde_json::Value) -> DynamicAcl {
        let mut acl = DynamicAcl::default();
        let Some(obj) = value.as_object() else {
            return acl;
        };

        acl.admins = Self::parse_id_array(obj.get("admins"));
        acl.users = Self::parse_id_array(obj.get("users"));
        acl.readonly = Self::parse_id_array(obj.get("readonly"));
        acl.user_tools_mode = Self::parse_user_tools_mode(obj.get("user_tools_mode"));
        acl.user_allowed_tools = Self::parse_tool_name_array(obj.get("user_allowed_tools"));

        // Backward compatibility with legacy auto-register payload:
        // { "<user_id>": { ...metadata... } }
        for key in obj.keys() {
            if let Ok(id) = key.trim().parse::<i64>() {
                acl.users.insert(id);
            }
        }

        acl
    }

    fn register_file_path(register_file: &str) -> PathBuf {
        if register_file == "~" || register_file.starts_with("~/") {
            if let Ok(home) = std::env::var("HOME") {
                if register_file == "~" {
                    return PathBuf::from(home);
                }
                return PathBuf::from(home).join(register_file.trim_start_matches("~/"));
            }
        }
        PathBuf::from(register_file)
    }

    fn default_register_file_for_account(account: &masix_config::TelegramAccount) -> String {
        let account_tag = masix_config::telegram_account_tag(&account.bot_token);
        format!("~/.masix/accounts/telegram.{}.register.json", account_tag)
    }

    fn effective_register_file(account: &masix_config::TelegramAccount) -> String {
        account
            .register_to_file
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .unwrap_or_else(|| Self::default_register_file_for_account(account))
    }

    fn effective_register_file_path(account: &masix_config::TelegramAccount) -> PathBuf {
        let register_file = Self::effective_register_file(account);
        Self::register_file_path(&register_file)
    }

    fn load_dynamic_acl_for_account(account: &masix_config::TelegramAccount) -> DynamicAcl {
        let path = Self::effective_register_file_path(account);
        let Ok(raw) = std::fs::read_to_string(path) else {
            return DynamicAcl::default();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return DynamicAcl::default();
        };
        Self::parse_dynamic_acl(&value)
    }

    fn is_bot_tagged(text: &str, account: &masix_config::TelegramAccount) -> bool {
        let Some(username) = account.bot_username() else {
            return false;
        };
        let mention = format!("@{}", username);
        text.to_lowercase().contains(&mention)
    }

    fn telegram_user_permission(
        account: &masix_config::TelegramAccount,
        dynamic_acl: &DynamicAcl,
        user_id: i64,
    ) -> PermissionLevel {
        if account.admins.contains(&user_id) || dynamic_acl.admins.contains(&user_id) {
            PermissionLevel::Admin
        } else if account.users.contains(&user_id) || dynamic_acl.users.contains(&user_id) {
            PermissionLevel::User
        } else if account.readonly.contains(&user_id) || dynamic_acl.readonly.contains(&user_id) {
            PermissionLevel::Readonly
        } else if let Some(allowed) = &account.allowed_chats {
            if allowed.contains(&user_id) {
                PermissionLevel::User
            } else {
                PermissionLevel::None
            }
        } else {
            PermissionLevel::None
        }
    }

    fn telegram_permission_for_group(
        account: &masix_config::TelegramAccount,
        user_perm: PermissionLevel,
        user_id: i64,
        chat_id: i64,
        is_bot_tagged: bool,
    ) -> PermissionLevel {
        let is_private = user_id == chat_id;
        if is_private {
            return user_perm;
        }

        match account.group_mode {
            GroupMode::All => {
                if user_perm == PermissionLevel::Admin {
                    PermissionLevel::Admin
                } else {
                    PermissionLevel::User
                }
            }
            GroupMode::UsersOnly => {
                if user_perm == PermissionLevel::None {
                    PermissionLevel::None
                } else {
                    user_perm
                }
            }
            GroupMode::TagOnly => {
                if is_bot_tagged {
                    if user_perm == PermissionLevel::Admin {
                        PermissionLevel::Admin
                    } else {
                        PermissionLevel::User
                    }
                } else {
                    PermissionLevel::None
                }
            }
            GroupMode::UsersOrTag => {
                if user_perm != PermissionLevel::None {
                    user_perm
                } else if is_bot_tagged {
                    PermissionLevel::User
                } else {
                    PermissionLevel::None
                }
            }
            GroupMode::ListenOnly => {
                if user_perm == PermissionLevel::Admin && is_bot_tagged {
                    PermissionLevel::Admin
                } else if is_bot_tagged {
                    PermissionLevel::User
                } else {
                    PermissionLevel::None
                }
            }
        }
    }

    fn should_silently_ignore_telegram_permission_denial(
        config: &Config,
        account_tag: Option<&str>,
        user_id: i64,
        chat_id: i64,
        text: &str,
    ) -> bool {
        let Some(account) = Self::get_telegram_account(config, account_tag) else {
            return false;
        };

        if user_id == chat_id {
            return false;
        }

        if Self::is_bot_tagged(text, account) {
            return false;
        }

        matches!(
            account.group_mode,
            GroupMode::TagOnly | GroupMode::UsersOrTag | GroupMode::ListenOnly
        )
    }

    fn get_permission_level(
        config: &Config,
        account_tag: Option<&str>,
        channel: &str,
        sender: &str,
        user_id: i64,
        chat_id: i64,
        text: &str,
    ) -> PermissionLevel {
        match channel {
            "telegram" => {
                let Some(account) = Self::get_telegram_account(config, account_tag) else {
                    return PermissionLevel::None;
                };
                let dynamic_acl = Self::load_dynamic_acl_for_account(account);
                let user_perm = Self::telegram_user_permission(account, &dynamic_acl, user_id);
                let is_bot_tagged = Self::is_bot_tagged(text, account);
                Self::telegram_permission_for_group(
                    account,
                    user_perm,
                    user_id,
                    chat_id,
                    is_bot_tagged,
                )
            }
            "whatsapp" => config
                .whatsapp
                .as_ref()
                .map(|whatsapp| whatsapp.get_permission_level(sender))
                .unwrap_or(PermissionLevel::None),
            "sms" => config
                .sms
                .as_ref()
                .map(|sms| sms.get_permission_level(sender))
                .unwrap_or(PermissionLevel::None),
            _ => PermissionLevel::None,
        }
    }

    fn normalized_account_user_tool_allowlist(
        account: &masix_config::TelegramAccount,
    ) -> HashSet<String> {
        account
            .user_allowed_tools
            .iter()
            .filter_map(|name| Self::normalize_tool_name(name))
            .collect()
    }

    fn effective_telegram_user_tool_policy(
        account: &masix_config::TelegramAccount,
        dynamic_acl: &DynamicAcl,
    ) -> (UserToolsMode, HashSet<String>) {
        let mode = dynamic_acl
            .user_tools_mode
            .unwrap_or(account.user_tools_mode);
        let allowlist = dynamic_acl
            .user_allowed_tools
            .clone()
            .unwrap_or_else(|| Self::normalized_account_user_tool_allowlist(account));
        (mode, allowlist)
    }

    fn runtime_tool_access_for_message(
        config: &Config,
        account_tag: Option<&str>,
        channel: &str,
        permission: PermissionLevel,
    ) -> RuntimeToolAccess {
        match permission {
            PermissionLevel::Admin => RuntimeToolAccess::All,
            PermissionLevel::User if channel == "telegram" => {
                let Some(account) = Self::get_telegram_account(config, account_tag) else {
                    return RuntimeToolAccess::None;
                };
                let dynamic_acl = Self::load_dynamic_acl_for_account(account);
                let (mode, allowlist) =
                    Self::effective_telegram_user_tool_policy(account, &dynamic_acl);
                match mode {
                    UserToolsMode::Selected if !allowlist.is_empty() => {
                        RuntimeToolAccess::Selected(allowlist)
                    }
                    _ => RuntimeToolAccess::None,
                }
            }
            _ => RuntimeToolAccess::None,
        }
    }

    fn should_auto_register_user(
        config: &Config,
        account_tag: Option<&str>,
        channel: &str,
        user_id: i64,
        chat_id: i64,
    ) -> bool {
        if channel != "telegram" || user_id != chat_id {
            return false;
        }
        let Some(account) = Self::get_telegram_account(config, account_tag) else {
            return false;
        };
        if !account.should_auto_register() {
            return false;
        }
        let dynamic_acl = Self::load_dynamic_acl_for_account(account);
        Self::telegram_user_permission(account, &dynamic_acl, user_id) == PermissionLevel::None
    }

    fn user_memory_dir(
        context: &BotContext,
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
    ) -> PathBuf {
        let account = Self::sanitize_scope_component(&Self::account_scope(account_tag));
        let user =
            Self::sanitize_scope_component(&Self::normalized_user_id(user_scope_id, chat_id));
        context
            .memory_dir
            .join("accounts")
            .join(account)
            .join("users")
            .join(user)
    }

    fn chat_scope_label(chat_id: Option<i64>) -> String {
        chat_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "global".to_string())
    }

    fn scoped_chat_memory_path(
        context: &BotContext,
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
    ) -> PathBuf {
        Self::user_memory_dir(context, account_tag, user_scope_id, chat_id)
            .join(format!("chat_{}.jsonl", Self::chat_scope_label(chat_id)))
    }

    fn scoped_summary_path(
        context: &BotContext,
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
    ) -> PathBuf {
        Self::user_memory_dir(context, account_tag, user_scope_id, chat_id)
            .join(format!("summary_{}.md", Self::chat_scope_label(chat_id)))
    }

    fn legacy_chat_memory_path(context: &BotContext, chat_id: Option<i64>) -> Option<PathBuf> {
        chat_id.map(|id| context.memory_dir.join(format!("chat_{}.jsonl", id)))
    }

    async fn record_user_catalog(
        context: &BotContext,
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
        channel: &str,
    ) -> Result<()> {
        let account = Self::account_scope(account_tag);
        let user_id = Self::normalized_user_id(user_scope_id, chat_id);
        let user_dir = Self::user_memory_dir(context, account_tag, user_scope_id, chat_id);
        fs::create_dir_all(&user_dir).await?;

        let meta_path = user_dir.join("meta.json");
        let now = chrono::Utc::now().to_rfc3339();
        let mut meta = match fs::read_to_string(&meta_path).await {
            Ok(raw) => serde_json::from_str::<UserMemoryMeta>(&raw).unwrap_or(UserMemoryMeta {
                account_tag: account.clone(),
                user_id: user_id.clone(),
                first_seen: now.clone(),
                last_seen: now.clone(),
                channels: Vec::new(),
                chat_ids: Vec::new(),
            }),
            Err(_) => UserMemoryMeta {
                account_tag: account.clone(),
                user_id: user_id.clone(),
                first_seen: now.clone(),
                last_seen: now.clone(),
                channels: Vec::new(),
                chat_ids: Vec::new(),
            },
        };

        meta.account_tag = account;
        meta.user_id = user_id;
        meta.last_seen = now;

        let mut channel_set: HashSet<String> = meta.channels.into_iter().collect();
        channel_set.insert(channel.to_string());
        meta.channels = channel_set.into_iter().collect();
        meta.channels.sort();

        if let Some(id) = chat_id {
            let mut chat_set: HashSet<i64> = meta.chat_ids.into_iter().collect();
            chat_set.insert(id);
            meta.chat_ids = chat_set.into_iter().collect();
            meta.chat_ids.sort_unstable();
        }

        let body = serde_json::to_string_pretty(&meta)?;
        fs::write(meta_path, body).await?;
        Ok(())
    }

    async fn load_chat_memory_history(
        context: &BotContext,
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
        max_entries: usize,
    ) -> Vec<ChatMessage> {
        let path = Self::scoped_chat_memory_path(context, account_tag, user_scope_id, chat_id);
        let content = match fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => {
                let Some(legacy_path) = Self::legacy_chat_memory_path(context, chat_id) else {
                    return Vec::new();
                };
                match fs::read_to_string(legacy_path).await {
                    Ok(c) => c,
                    Err(_) => return Vec::new(),
                }
            }
        };

        let mut entries = Vec::new();
        for line in content.lines() {
            if let Ok(entry) = serde_json::from_str::<ChatMemoryEntry>(line) {
                if !entry.content.trim().is_empty()
                    && (entry.role == "user" || entry.role == "assistant")
                {
                    entries.push(entry);
                }
            }
        }

        let start = entries.len().saturating_sub(max_entries);
        entries[start..]
            .iter()
            .map(|entry| ChatMessage {
                role: entry.role.clone(),
                content: Some(entry.content.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            })
            .collect()
    }

    async fn append_chat_memory(
        context: &BotContext,
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
        role: &str,
        content: &str,
    ) -> Result<()> {
        if content.trim().is_empty() {
            return Ok(());
        }
        let path = Self::scoped_chat_memory_path(context, account_tag, user_scope_id, chat_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        let entry = ChatMemoryEntry {
            role: role.to_string(),
            content: content.to_string(),
            ts: chrono::Utc::now().to_rfc3339(),
        };
        let line = serde_json::to_string(&entry)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    async fn clear_chat_memory(
        context: &BotContext,
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
    ) {
        let path = Self::scoped_chat_memory_path(context, account_tag, user_scope_id, chat_id);
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        if let Some(legacy_path) = Self::legacy_chat_memory_path(context, chat_id) {
            if legacy_path.exists() {
                let _ = std::fs::remove_file(legacy_path);
            }
        }
        info!(
            "Cleared scoped chat memory for user={} chat={}",
            Self::normalized_user_id(user_scope_id, chat_id),
            Self::chat_scope_label(chat_id)
        );
    }

    async fn update_summary_snapshot(
        context: &BotContext,
        account_tag: Option<&str>,
        user_scope_id: Option<&str>,
        chat_id: Option<i64>,
    ) -> Result<()> {
        let history =
            Self::load_chat_memory_history(context, account_tag, user_scope_id, chat_id, 6).await;
        if history.is_empty() {
            return Ok(());
        }

        let user_label = Self::normalized_user_id(user_scope_id, chat_id);
        let chat_label = Self::chat_scope_label(chat_id);
        let mut lines = vec![
            format!(
                "# Chat Summary (user: {}, chat: {})",
                user_label, chat_label
            ),
            format!("Updated: {}", chrono::Utc::now().to_rfc3339()),
            String::new(),
        ];

        for msg in history {
            let role = msg.role;
            let content = msg.content.unwrap_or_default();
            let shortened = if content.chars().count() > 220 {
                format!("{}...", content.chars().take(220).collect::<String>())
            } else {
                content
            };
            lines.push(format!("- {}: {}", role, shortened.replace('\n', " ")));
        }

        let path = Self::scoped_summary_path(context, account_tag, user_scope_id, chat_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, lines.join("\n")).await?;
        Ok(())
    }

    async fn check_and_update_rate_limit(
        policy: &PolicyEngine,
        rate_state: &Arc<Mutex<HashMap<String, (i64, u32)>>>,
        sender_id: &str,
    ) -> bool {
        let minute_bucket = chrono::Utc::now().timestamp() / 60;
        let mut state = rate_state.lock().await;

        // Keep only recent buckets to bound memory usage.
        state.retain(|_, (bucket, _)| *bucket >= minute_bucket - 5);

        let entry = state
            .entry(sender_id.to_string())
            .or_insert((minute_bucket, 0));

        if entry.0 != minute_bucket {
            entry.0 = minute_bucket;
            entry.1 = 0;
        }

        entry.1 += 1;
        policy.check_rate_limit(entry.1)
    }

    fn send_outbound_text(
        outbound_sender: &broadcast::Sender<OutboundMessage>,
        channel: &str,
        account_tag: Option<String>,
        chat_id: i64,
        text: &str,
        reply_to: Option<i64>,
    ) {
        let _ = outbound_sender.send(OutboundMessage {
            channel: channel.to_string(),
            account_tag,
            chat_id,
            text: text.to_string(),
            reply_to,
            edit_message_id: None,
            inline_keyboard: None,
            chat_action: None,
        });
    }

    fn start_typing_heartbeat(
        outbound_sender: &broadcast::Sender<OutboundMessage>,
        channel: &str,
        account_tag: Option<String>,
        chat_id: Option<i64>,
    ) -> Option<TypingHeartbeat> {
        if channel != "telegram" {
            return None;
        }
        let chat_id = chat_id?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let outbound = outbound_sender.clone();
        let channel_name = channel.to_string();

        tokio::spawn(async move {
            while !stop_flag.load(Ordering::Relaxed) {
                let _ = outbound.send(OutboundMessage {
                    channel: channel_name.clone(),
                    account_tag: account_tag.clone(),
                    chat_id,
                    text: String::new(),
                    reply_to: None,
                    edit_message_id: None,
                    inline_keyboard: None,
                    chat_action: Some("typing".to_string()),
                });
                tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
            }
        });

        Some(TypingHeartbeat { stop })
    }

    fn get_data_dir(&self) -> Result<std::path::PathBuf> {
        if let Some(data_dir) = &self.config.core.data_dir {
            if data_dir == "~" || data_dir.starts_with("~/") {
                let home =
                    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
                if data_dir == "~" {
                    Ok(home)
                } else {
                    Ok(home.join(data_dir.trim_start_matches("~/")))
                }
            } else {
                Ok(std::path::PathBuf::from(data_dir))
            }
        } else {
            let home =
                dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
            Ok(home.join(".masix"))
        }
    }

    pub async fn storage(&self) -> tokio::sync::MutexGuard<'_, Storage> {
        self.storage.lock().await
    }

    pub fn policy(&self) -> &PolicyEngine {
        &self.policy
    }

    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    pub async fn chat(&self, messages: Vec<ChatMessage>, provider: Option<&str>) -> Result<String> {
        let response = self
            .provider_router
            .chat(messages, provider, None, None)
            .await?;
        Ok(response.content.unwrap_or_default())
    }

    async fn handle_provider_chat_command(
        text: &str,
        user_state_key: &str,
        config: &Config,
        user_providers: &Arc<Mutex<HashMap<String, String>>>,
    ) -> String {
        let rest = text.strip_prefix("/provider").unwrap_or("").trim();

        if rest.is_empty() || rest.eq_ignore_ascii_case("help") {
            let mut lines = vec![
                "🤖 *Provider Commands*".to_string(),
                String::new(),
                "/provider - Show current provider".to_string(),
                "/provider list - List all providers".to_string(),
                "/provider set <name> - Set provider for this user".to_string(),
                String::new(),
                "Available providers:".to_string(),
            ];
            for p in &config.providers.providers {
                let marker = if p.name == config.providers.default_provider {
                    " (default)"
                } else {
                    ""
                };
                lines.push(format!("  • {}{}", p.name, marker));
            }
            lines.join("\n")
        } else if rest.eq_ignore_ascii_case("list") {
            let mut lines = vec!["📋 *Configured Providers*".to_string(), String::new()];
            let current_provider = user_providers.lock().await.get(user_state_key).cloned();
            for p in &config.providers.providers {
                let is_default = p.name == config.providers.default_provider;
                let is_current = current_provider.as_deref() == Some(&p.name);

                let mut markers = Vec::new();
                if is_default {
                    markers.push("default");
                }
                if is_current {
                    markers.push("current");
                }

                let marker_str = if markers.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", markers.join(", "))
                };
                lines.push(format!("• {}{}", p.name, marker_str));
                if let Some(model) = &p.model {
                    lines.push(format!("  Model: {}", model));
                }
            }
            lines.join("\n")
        } else if let Some(name) = rest.strip_prefix("set ") {
            let name = name.trim();
            let exists = config.providers.providers.iter().any(|p| p.name == name);
            if !exists {
                format!(
                    "❌ Provider '{}' not found.\nUse /provider list to see available providers.",
                    name
                )
            } else {
                user_providers
                    .lock()
                    .await
                    .insert(user_state_key.to_string(), name.to_string());
                format!("✅ Provider set to '{}' for this user.", name)
            }
        } else {
            let current = user_providers
                .lock()
                .await
                .get(user_state_key)
                .cloned()
                .unwrap_or_else(|| config.providers.default_provider.clone());
            format!("📍 Current provider: {}", current)
        }
    }

    async fn handle_model_chat_command(
        text: &str,
        user_state_key: &str,
        config: &Config,
        user_providers: &Arc<Mutex<HashMap<String, String>>>,
        user_models: &Arc<Mutex<HashMap<String, String>>>,
    ) -> String {
        let rest = text.strip_prefix("/model").unwrap_or("").trim();

        if rest.is_empty() || rest.eq_ignore_ascii_case("help") {
            let current_provider = user_providers
                .lock()
                .await
                .get(user_state_key)
                .cloned()
                .unwrap_or_else(|| config.providers.default_provider.clone());
            let current_model = user_models.lock().await.get(user_state_key).cloned();

            let provider_config = config
                .providers
                .providers
                .iter()
                .find(|p| p.name == current_provider);

            let default_model = provider_config
                .and_then(|p| p.model.as_deref())
                .unwrap_or("unknown");

            let lines = [
                "🎯 *Model Commands*".to_string(),
                String::new(),
                format!("Current provider: {}", current_provider),
                format!(
                    "Current model: {}",
                    current_model.as_deref().unwrap_or(default_model)
                ),
                String::new(),
                "/model <name> - Set model for this user".to_string(),
                "/model reset - Reset to default model".to_string(),
            ];
            lines.join("\n")
        } else if rest.eq_ignore_ascii_case("reset") {
            user_models.lock().await.remove(user_state_key);
            "✅ Model reset to default for this user.".to_string()
        } else {
            let model = rest.trim();
            user_models
                .lock()
                .await
                .insert(user_state_key.to_string(), model.to_string());
            format!("✅ Model set to '{}' for this user.", model)
        }
    }

    async fn handle_mcp_chat_command(text: &str, config: &Config) -> String {
        let rest = text.strip_prefix("/mcp").unwrap_or("").trim();

        let mcp = config.mcp.as_ref();
        let is_enabled = mcp.map(|m| m.enabled).unwrap_or(false);
        let servers = mcp.map(|m| m.servers.as_slice()).unwrap_or(&[]);

        if rest.is_empty() || rest.eq_ignore_ascii_case("help") {
            let mut lines = vec![
                "🔧 *MCP Status*".to_string(),
                String::new(),
                format!("Enabled: {}", if is_enabled { "✅" } else { "❌" }),
                format!("Servers: {}", servers.len()),
            ];

            if !servers.is_empty() {
                lines.push(String::new());
                lines.push("Configured servers:".to_string());
                for s in servers {
                    lines.push(format!("  • {} ({} {:?})", s.name, s.command, s.args));
                }
            }

            lines.push(String::new());
            lines.push("Use CLI to manage MCP:".to_string());
            lines.push("  masix config mcp list".to_string());
            lines.push("  masix config mcp add <name> <cmd> [args]".to_string());
            lines.push("  masix config mcp remove <name>".to_string());

            lines.join("\n")
        } else if rest.eq_ignore_ascii_case("list") {
            if !is_enabled {
                "❌ MCP is disabled.".to_string()
            } else if servers.is_empty() {
                "📋 No MCP servers configured.".to_string()
            } else {
                let mut lines = vec!["📋 *MCP Servers*".to_string(), String::new()];
                for s in servers {
                    lines.push(format!("• {}", s.name));
                    lines.push(format!("  Command: {} {:?}", s.command, s.args));
                }
                lines.join("\n")
            }
        } else {
            "Unknown command. Use /mcp for status.".to_string()
        }
    }

    async fn handle_tools_chat_command(mcp_client: &Option<Arc<Mutex<McpClient>>>) -> String {
        let tools = Self::get_mcp_tools(mcp_client).await;
        if tools.is_empty() {
            return "⚠️ Nessun tool esposto in runtime.".to_string();
        }

        let mut names: Vec<String> = tools.iter().map(|t| t.function.name.clone()).collect();
        names.sort();
        names.dedup();

        let builtins = names.iter().filter(|name| is_builtin_tool(name)).count();
        let mcp_count = names.len().saturating_sub(builtins);

        let mut lines = vec![
            "🧰 *Runtime Tools*".to_string(),
            String::new(),
            format!("Totale: {}", names.len()),
            format!("Built-in: {}", builtins),
            format!("MCP: {}", mcp_count),
            String::new(),
            "Tool names:".to_string(),
        ];
        for name in names {
            lines.push(format!("• {}", name));
        }
        lines.join("\n")
    }
}
