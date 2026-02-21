//! Masix Core
//!
//! Main runtime orchestration with MCP + Cron + LLM support

mod builtin_tools;

use anyhow::{anyhow, Result};
use base64::Engine;
use builtin_tools::{execute_builtin_tool, get_builtin_tool_definitions, is_builtin_tool};
use masix_config::{Config, RetryPolicyConfig};
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
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, error, info, warn};

use masix_telegram::menu::Language;

const MAX_TOOL_ITERATIONS: usize = 5;
const MEMORY_MAX_CONTEXT_ENTRIES: usize = 12;

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
        let bot_contexts_for_processor = Arc::clone(&bot_contexts);
        let default_cron_account_tag = self.default_telegram_account_tag();
        let user_languages_for_processor = Arc::clone(&self.user_languages);
        let user_providers_for_processor = Arc::clone(&self.user_providers);
        let user_models_for_processor = Arc::clone(&self.user_models);
        let config_for_processor = self.config.clone();

        tokio::spawn(async move {
            let mut cron_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

            loop {
                tokio::select! {
                    result = inbound_rx.recv() => {
                        match result {
                            Ok(envelope) => {
                                if let Err(e) = Self::process_inbound_message(
                                    envelope,
                                    outbound_for_processor.clone(),
                                    &provider_router,
                                    &storage_for_processor,
                                    &mcp_client,
                                    &system_prompt,
                                    &policy,
                                    &rate_state,
                                    &bot_contexts_for_processor,
                                    &user_languages_for_processor,
                                    &user_providers_for_processor,
                                    &user_models_for_processor,
                                    &config_for_processor,
                                ).await {
                                    error!("Error processing inbound message: {}", e);
                                }
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
        let memory_file = if let Some(path) = &self.config.core.soul_file {
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
            "\n\n# Tool Calling Protocol\nHai accesso a tool runtime.\nQuando una richiesta richiede azioni su shell/file/web/device/termux, usa un tool-call e non limitarti a descrivere i tool.\nPer torrent usa `torrent_search` per trovare link e `torrent_extract_magnet` solo sui link scelti (nessun download).\nPreferisci sempre il tool-calling nativo del provider.\nSe il provider non supporta tool-calling nativo, usa questo formato esatto:\n### TOOL_CALL\ncall <tool_name>\n{{\"arg\":\"value\"}}\n### TOOL_CALL\nBuilt-in tools sempre disponibili: {}\nMCP/extra tools disponibili: {}\nTotale tools disponibili: {}",
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
            let result = mcp.call_tool(server_name, mcp_tool_name, arguments).await?;

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
                Ok(format!("Tool error: {}", joined))
            } else {
                Ok(joined)
            }
        } else {
            Err(anyhow::anyhow!("No MCP client available"))
        }
    }

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

                let _ = Self::record_user_catalog(
                    &bot_context,
                    account_tag.as_deref(),
                    user_scope_id.as_deref(),
                    envelope.chat_id,
                    &envelope.channel,
                )
                .await;

                // Show command list when user types just "/"
                if text == "/" {
                    if let Some(chat_id) = envelope.chat_id {
                        let lang = user_languages
                            .lock()
                            .await
                            .get(&user_state_key)
                            .copied()
                            .unwrap_or_default();
                        let cmd_list = masix_telegram::menu::command_list(lang);
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: cmd_list,
                            reply_to: None,
                            edit_message_id: None,
                            inline_keyboard: None,
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
                    return Ok(());
                }

                if text.starts_with("/start") || text.starts_with("/menu") {
                    info!("Processing menu command");
                    if let Some(chat_id) = envelope.chat_id {
                        let lang = user_languages
                            .lock()
                            .await
                            .get(&user_state_key)
                            .copied()
                            .unwrap_or_default();
                        let (menu_text, keyboard) = masix_telegram::menu::home_menu(lang);
                        info!("Sending home menu to chat_id: {}", chat_id);
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
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
                    }
                    return Ok(());
                }

                if text.starts_with("/new") {
                    info!("Processing /new - session reset");
                    if let Some(chat_id) = envelope.chat_id {
                        let lang = user_languages
                            .lock()
                            .await
                            .get(&user_state_key)
                            .copied()
                            .unwrap_or_default();
                        let reset_text = masix_telegram::menu::session_reset_text(lang);
                        Self::clear_chat_memory(
                            &bot_context,
                            account_tag.as_deref(),
                            user_scope_id.as_deref(),
                            Some(chat_id),
                        )
                        .await;
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: reset_text,
                            reply_to: None,
                            edit_message_id: None,
                            inline_keyboard: None,
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
                    return Ok(());
                }

                if text.starts_with("/help") {
                    info!("Processing /help");
                    if let Some(chat_id) = envelope.chat_id {
                        let lang = user_languages
                            .lock()
                            .await
                            .get(&user_state_key)
                            .copied()
                            .unwrap_or_default();
                        let help_text = masix_telegram::menu::help_text(lang);
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: help_text,
                            reply_to: None,
                            edit_message_id: None,
                            inline_keyboard: None,
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
                    return Ok(());
                }

                if text.starts_with("/language") {
                    info!("Processing /language");
                    if let Some(chat_id) = envelope.chat_id {
                        let lang = user_languages
                            .lock()
                            .await
                            .get(&user_state_key)
                            .copied()
                            .unwrap_or_default();
                        let (menu_text, keyboard) = masix_telegram::menu::language_menu(lang);
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: menu_text,
                            reply_to: None,
                            edit_message_id: None,
                            inline_keyboard: Some(keyboard),
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
                    return Ok(());
                }

                if text.starts_with("/provider") {
                    info!("Processing /provider");
                    if let Some(chat_id) = envelope.chat_id {
                        let response = Self::handle_provider_chat_command(
                            text,
                            &user_state_key,
                            config,
                            user_providers,
                        )
                        .await;
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: response,
                            reply_to: None,
                            edit_message_id: None,
                            inline_keyboard: None,
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
                    return Ok(());
                }

                if text.starts_with("/model") {
                    info!("Processing /model");
                    if let Some(chat_id) = envelope.chat_id {
                        let response = Self::handle_model_chat_command(
                            text,
                            &user_state_key,
                            config,
                            user_providers,
                            user_models,
                        )
                        .await;
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: response,
                            reply_to: None,
                            edit_message_id: None,
                            inline_keyboard: None,
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
                    return Ok(());
                }

                if text.starts_with("/mcp") {
                    info!("Processing /mcp");
                    if let Some(chat_id) = envelope.chat_id {
                        let response = Self::handle_mcp_chat_command(text, config).await;
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: response,
                            reply_to: None,
                            edit_message_id: None,
                            inline_keyboard: None,
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
                    return Ok(());
                }

                if text.starts_with("/tools") {
                    info!("Processing /tools");
                    if let Some(chat_id) = envelope.chat_id {
                        let response = Self::handle_tools_chat_command(mcp_client).await;
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: response,
                            reply_to: None,
                            edit_message_id: None,
                            inline_keyboard: None,
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
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
                )
                .await?
                {
                    return Ok(());
                }

                let tools = Self::get_mcp_tools(mcp_client).await;
                let has_tools = !tools.is_empty();
                let builtin_tools_count = tools
                    .iter()
                    .filter(|tool| is_builtin_tool(&tool.function.name))
                    .count();
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

                // Send typing action while processing
                if let Some(chat_id) = envelope.chat_id {
                    let typing_msg = OutboundMessage {
                        channel: envelope.channel.clone(),
                        account_tag: account_tag.clone(),
                        chat_id,
                        text: String::new(),
                        reply_to: None,
                        edit_message_id: None,
                        inline_keyboard: None,
                        chat_action: Some("typing".to_string()),
                    };
                    let _ = outbound_sender.send(typing_msg);
                }

                let mut system_context = system_prompt.to_string();
                if let Some(memory) = Self::load_bot_memory_file(&bot_context).await {
                    if !memory.trim().is_empty() {
                        system_context.push_str("\n\n# Bot Memory\n");
                        system_context.push_str(&memory);
                    }
                }
                if has_tools {
                    system_context.push_str(&Self::build_tool_call_guidance(&tools));
                }

                let mut messages = vec![ChatMessage {
                    role: "system".to_string(),
                    content: Some(system_context),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                }];
                let mut user_message =
                    Self::enrich_user_message_with_media(text, &envelope.payload);
                let vision_analysis = match Self::analyze_media_with_vision_provider(
                    config,
                    &bot_context,
                    account_tag.as_deref(),
                    &envelope.payload,
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
                let history = Self::load_chat_memory_history(
                    &bot_context,
                    account_tag.as_deref(),
                    user_scope_id.as_deref(),
                    envelope.chat_id,
                    MEMORY_MAX_CONTEXT_ENTRIES,
                )
                .await;
                messages.extend(history);
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(user_message.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });

                let mut final_response = String::new();
                let mut iterations = 0;
                let mut selected_provider: Option<String> = None;
                let mut used_tools: Vec<String> = Vec::new();
                let mut used_tool_signatures: HashSet<String> = HashSet::new();

                loop {
                    iterations += 1;
                    if iterations > MAX_TOOL_ITERATIONS {
                        warn!("Max tool iterations reached");
                        break;
                    }

                    let (response, provider_used) = if has_tools {
                        Self::chat_with_fallback_chain(
                            provider_router,
                            messages.clone(),
                            Some(tools.clone()),
                            &bot_context.provider_chain,
                            selected_provider.as_deref(),
                            &bot_context.retry_policy,
                            &bot_context.profile_name,
                        )
                        .await?
                    } else {
                        Self::chat_with_fallback_chain(
                            provider_router,
                            messages.clone(),
                            None,
                            &bot_context.provider_chain,
                            selected_provider.as_deref(),
                            &bot_context.retry_policy,
                            &bot_context.profile_name,
                        )
                        .await?
                    };
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
                                    content: Some("Skipped duplicate tool call in same turn to prevent loops.".to_string()),
                                    tool_calls: None,
                                    tool_call_id: Some(tool_call.id.clone()),
                                    name: Some(tool_call.function.name.clone()),
                                });
                                continue;
                            }

                            let tool_result = match Self::execute_tool_call(
                                mcp_client,
                                tool_call,
                                &bot_context.exec_policy,
                                &bot_context.workdir,
                                storage,
                                &envelope,
                                account_tag.as_deref(),
                                vision_analysis.as_deref(),
                            )
                            .await
                            {
                                Ok(result) => result,
                                Err(e) => format!("Error: {}", e),
                            };

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

                if final_response.is_empty() {
                    final_response = "Non ho potuto generare una risposta.".to_string();
                }
                if !used_tools.is_empty() {
                    final_response.push_str("\n\n🧰 Tool usati: ");
                    final_response.push_str(&used_tools.join(", "));
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

                if envelope.channel == "telegram" {
                    if let Some(chat_id) = envelope.chat_id {
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: final_response,
                            reply_to: envelope.message_id,
                            edit_message_id: None,
                            inline_keyboard: None,
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
                } else if envelope.channel == "whatsapp" {
                    if let Some(msg) =
                        Self::build_whatsapp_forward_message(config, &envelope, &final_response)
                    {
                        let _ = outbound_sender.send(msg);
                    }
                } else if envelope.channel == "sms" {
                    if let Some(msg) =
                        Self::build_sms_forward_message(config, &envelope, &final_response)
                    {
                        let _ = outbound_sender.send(msg);
                    }
                }
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
                    if let Some(msg) = masix_telegram::menu::handle_callback(
                        data,
                        chat_id,
                        envelope.message_id,
                        account_tag.clone(),
                        lang,
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

        Some(format!(
            "kind: {}\nfile_id: {}\nmime_type: {}\nsize: {} bytes\nresolution: {}x{}\ncaption: {}",
            kind, file_id, mime_type, file_size, width, height, caption
        ))
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

        let mut mime_type = media
            .get("mime_type")
            .and_then(|v| v.as_str())
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| {
                if kind == "photo" {
                    "image/jpeg".to_string()
                } else {
                    "application/octet-stream".to_string()
                }
            });

        if mime_type == "application/octet-stream" && kind == "image_document" {
            mime_type = "image/jpeg".to_string();
        }

        if !mime_type.starts_with("image/") {
            return None;
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
            caption,
        })
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
    ) -> Result<bool> {
        let trimmed = text.trim();
        let Some(chat_id) = envelope.chat_id else {
            return Ok(false);
        };

        if trimmed == "/exec" || trimmed.starts_with("/exec ") {
            let rest = trimmed.strip_prefix("/exec").unwrap_or("");
            let command = rest.trim();
            if command.is_empty() || command.eq_ignore_ascii_case("help") {
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    "Uso: `/exec <command>`\nEsempio: `/exec ls -la`\nEsegue solo comandi in allowlist nella workdir del bot.",
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

    async fn chat_with_fallback_chain(
        provider_router: &ProviderRouter,
        messages: Vec<ChatMessage>,
        tools: Option<Vec<ToolDefinition>>,
        provider_chain: &[String],
        preferred_provider: Option<&str>,
        retry_policy: &RetryPolicy,
        profile_name: &str,
    ) -> Result<(masix_providers::ChatResponse, String)> {
        const MAX_ATTEMPTS_PER_PROVIDER: usize = 3;

        // Single provider mode: use retry logic inside provider
        if provider_chain.len() <= 1 {
            let provider_name = provider_chain
                .first()
                .map(|s| s.as_str())
                .or(preferred_provider);
            let response = if let Some(tool_defs) = &tools {
                provider_router
                    .chat_with_tools(
                        messages,
                        tool_defs.clone(),
                        provider_name,
                        Some(retry_policy),
                    )
                    .await?
            } else {
                provider_router
                    .chat(messages, provider_name, Some(retry_policy))
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
            let provider_name = &provider_chain[current_idx];

            let result = if let Some(tool_defs) = &tools {
                provider_router
                    .chat_with_tools(
                        messages.clone(),
                        tool_defs.clone(),
                        Some(provider_name),
                        None, // No internal retry, we manage fallback ourselves
                    )
                    .await
            } else {
                provider_router
                    .chat(messages.clone(), Some(provider_name), None)
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
                        current_idx = (current_idx + 1) % provider_chain.len();

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
                            provider_chain[prev_idx], provider_chain[current_idx], profile_name
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
        let response = self.provider_router.chat(messages, provider, None).await?;
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

            let lines = vec![
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
