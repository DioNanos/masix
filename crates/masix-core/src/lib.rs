//! Masix Core
//!
//! Main runtime orchestration with MCP + Cron + LLM support

use anyhow::{anyhow, Result};
use masix_config::{Config, RetryPolicyConfig};
use masix_exec::{manage_termux_boot, run_command, BootAction, ExecMode, ExecPolicy};
use masix_ipc::{Envelope, EventBus, MessageKind, OutboundMessage};
use masix_mcp::McpClient;
use masix_policy::PolicyEngine;
use masix_providers::{
    ChatMessage, OpenAICompatibleProvider, ProviderRouter, RetryPolicy, ToolCall, ToolDefinition,
};
use masix_storage::Storage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info, warn};

const MAX_TOOL_ITERATIONS: usize = 5;
const MEMORY_MAX_CONTEXT_ENTRIES: usize = 12;

#[derive(Debug, Clone)]
struct BotContext {
    profile_name: String,
    workdir: PathBuf,
    memory_dir: PathBuf,
    memory_file: PathBuf,
    provider_chain: Vec<String>,
    retry_policy: RetryPolicy,
    exec_policy: ExecPolicy,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ChatMemoryEntry {
    role: String,
    content: String,
    ts: String,
}

pub struct MasixRuntime {
    config: Config,
    storage: Arc<Mutex<Storage>>,
    policy: PolicyEngine,
    provider_router: Arc<ProviderRouter>,
    mcp_client: Option<Arc<Mutex<McpClient>>>,
    event_bus: EventBus,
    system_prompt: String,
}

impl MasixRuntime {
    pub fn new(config: Config, storage: Storage) -> Result<Self> {
        let policy = PolicyEngine::new(config.policy.as_ref());

        let mut provider_router = ProviderRouter::new(config.providers.default_provider.clone());

        for provider_config in &config.providers.providers {
            let provider = OpenAICompatibleProvider::new(
                provider_config.name.clone(),
                provider_config.api_key.clone(),
                provider_config.base_url.clone(),
                provider_config.model.clone(),
            );
            provider_router.add_provider(Box::new(provider));
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

            for (idx, account) in telegram_config.accounts.iter().enumerate() {
                info!("Telegram adapter enabled for account #{}", idx + 1);

                let account_clone = account.clone();
                let account_tag = Self::account_tag_from_token(&account.bot_token);
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
                let context = if let Some(profile_name) = &account.bot_profile {
                    if let Some(profile) = profile_map.get(profile_name) {
                        let workdir =
                            Self::resolve_path_with_base(&profile.workdir, base_data_dir)?;
                        let memory_file =
                            Self::resolve_path_with_base(&profile.memory_file, &workdir)?;
                        let mut provider_chain = vec![profile.provider_primary.clone()];
                        provider_chain.extend(profile.provider_fallback.clone());

                        BotContext {
                            profile_name: profile.name.clone(),
                            memory_dir: workdir.join("memory"),
                            workdir,
                            memory_file,
                            provider_chain,
                            retry_policy: Self::retry_policy_from_config(profile.retry.as_ref()),
                            exec_policy: Self::exec_policy_from_config(self.config.exec.as_ref()),
                        }
                    } else {
                        default_context.clone()
                    }
                } else {
                    default_context.clone()
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
            retry_policy: RetryPolicy::default(),
            exec_policy: Self::exec_policy_from_config(self.config.exec.as_ref()),
        };
        Self::ensure_bot_context_dirs(&context)?;
        Ok(context)
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
                retry_policy: RetryPolicy::default(),
                exec_policy: ExecPolicy::default(),
            })
    }

    async fn get_mcp_tools(mcp_client: &Option<Arc<Mutex<McpClient>>>) -> Vec<ToolDefinition> {
        let mut tools = Vec::new();

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

    async fn execute_tool_call(
        mcp_client: &Option<Arc<Mutex<McpClient>>>,
        tool_call: &ToolCall,
    ) -> Result<String> {
        let parts: Vec<&str> = tool_call.function.name.splitn(2, '_').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid tool name format: {}",
                tool_call.function.name
            ));
        }

        let server_name = parts[0];
        let tool_name = parts[1];

        let arguments: serde_json::Value = serde_json::from_str(&tool_call.function.arguments)
            .unwrap_or_else(|_| serde_json::json!({}));

        if let Some(client) = mcp_client {
            let mcp = client.lock().await;
            let result = mcp.call_tool(server_name, tool_name, arguments).await?;

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
    ) -> Result<()> {
        let account_tag = envelope
            .payload
            .get("account_tag")
            .and_then(|v| v.as_str())
            .map(|value| value.to_string());
        let bot_context = Self::resolve_bot_context(bot_contexts, account_tag.as_deref());

        match &envelope.kind {
            MessageKind::Message { from, text } => {
                info!("Processing message from {}: {}", from, text);

                let sender_id = envelope
                    .chat_id
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| from.clone());

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

                if !Self::check_and_update_rate_limit(policy, rate_state, &sender_id).await {
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

                if text.starts_with("/start") {
                    let (menu_text, keyboard) = masix_telegram::menu::home_menu();
                    if let Some(chat_id) = envelope.chat_id {
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: menu_text,
                            reply_to: None,
                            edit_message_id: envelope.message_id,
                            inline_keyboard: Some(keyboard),
                            chat_action: None,
                        };
                        let _ = outbound_sender.send(msg);
                    }
                    return Ok(());
                }

                if text.starts_with("/menu") {
                    let (menu_text, keyboard) = masix_telegram::menu::home_menu();
                    if let Some(chat_id) = envelope.chat_id {
                        let msg = OutboundMessage {
                            channel: envelope.channel.clone(),
                            account_tag: account_tag.clone(),
                            chat_id,
                            text: menu_text,
                            reply_to: None,
                            edit_message_id: envelope.message_id,
                            inline_keyboard: Some(keyboard),
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

                let mut messages = vec![ChatMessage {
                    role: "system".to_string(),
                    content: Some(system_context),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                }];
                if let Some(chat_id) = envelope.chat_id {
                    let history = Self::load_chat_memory_history(
                        &bot_context,
                        chat_id,
                        MEMORY_MAX_CONTEXT_ENTRIES,
                    )
                    .await;
                    messages.extend(history);
                }
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(text.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });

                let mut final_response = String::new();
                let mut iterations = 0;
                let mut selected_provider: Option<String> = None;

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

                            let tool_result =
                                match Self::execute_tool_call(mcp_client, tool_call).await {
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

                if let Some(chat_id) = envelope.chat_id {
                    let _ = Self::append_chat_memory(&bot_context, chat_id, "user", text).await;
                    let _ = Self::append_chat_memory(
                        &bot_context,
                        chat_id,
                        "assistant",
                        &final_response,
                    )
                    .await;
                    let _ = Self::update_summary_snapshot(&bot_context, chat_id).await;
                }

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
            }
            MessageKind::Callback { query_id, data } => {
                info!("Processing callback {}: {}", query_id, data);

                if let Some(chat_id) = envelope.chat_id {
                    if !policy.is_allowed(&chat_id.to_string()) {
                        warn!("Blocked callback by policy from chat {}", chat_id);
                        return Ok(());
                    }
                    if let Some(msg) = masix_telegram::menu::handle_callback(
                        data,
                        chat_id,
                        envelope.message_id,
                        account_tag.clone(),
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
            .unwrap_or("__default__")
            .to_string();
        let recipient = chat_id.to_string();

        if rest.is_empty() || rest.eq_ignore_ascii_case("help") {
            let help = "Reminder commands:\n\
                        - `/cron domani alle 9 \"Meeting\"`\n\
                        - `/cron list`\n\
                        - `/cron cancel <id>`";
            Self::send_outbound_text(
                outbound_sender,
                &envelope.channel,
                account_tag.clone(),
                chat_id,
                help,
                envelope.message_id,
            );
            return Ok(true);
        }

        if rest.eq_ignore_ascii_case("list") {
            let storage_guard = storage.lock().await;
            let jobs = storage_guard
                .list_enabled_cron_jobs_for_account_recipient(&scoped_account_tag, &recipient)?;
            drop(storage_guard);

            if jobs.is_empty() {
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    "Nessun reminder attivo per questa chat.",
                    envelope.message_id,
                );
            } else {
                let mut lines = vec!["Reminder attivi:".to_string()];
                for job in jobs {
                    lines.push(format!(
                        "- ID {} | {} | recurring: {}\n  {}",
                        job.id, job.schedule, job.recurring, job.message
                    ));
                }
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    &lines.join("\n"),
                    envelope.message_id,
                );
            }
            return Ok(true);
        }

        let lower = rest.to_lowercase();
        if lower.starts_with("cancel ") || lower.starts_with("delete ") {
            let id_part = rest.split_whitespace().nth(1).unwrap_or_default();
            let id = id_part
                .parse::<i64>()
                .map_err(|_| anyhow!("Invalid cron id '{}'", id_part))?;
            let storage_guard = storage.lock().await;
            let changed = storage_guard.disable_cron_job_for_account(id, &scoped_account_tag)?;
            drop(storage_guard);

            let out = if changed {
                format!("Reminder {} eliminato.", id)
            } else {
                format!(
                    "Reminder {} non trovato per questo bot/chat (scope account: {}).",
                    id, scoped_account_tag
                )
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

        let parser = masix_cron::CronParser::new();
        let parsed = parser.parse(rest, "telegram", &recipient)?;
        let storage_guard = storage.lock().await;
        let id = storage_guard.create_cron_job(
            "telegram",
            &parsed.schedule,
            &parsed.channel,
            &parsed.recipient,
            Some(&scoped_account_tag),
            &parsed.message,
            &parsed.timezone,
            parsed.recurring,
        )?;
        drop(storage_guard);

        let confirmation = format!(
            "Reminder creato.\nID: {}\nSchedule: {}\nRecurring: {}\nMessage: {}",
            id, parsed.schedule, parsed.recurring, parsed.message
        );
        Self::send_outbound_text(
            outbound_sender,
            &envelope.channel,
            account_tag.clone(),
            chat_id,
            &confirmation,
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

            if command.is_empty() || command.eq_ignore_ascii_case("help") {
                Self::send_outbound_text(
                    outbound_sender,
                    &envelope.channel,
                    account_tag.clone(),
                    chat_id,
                    "Uso:\n- `/termux info`\n- `/termux battery`\n- `/termux cmd <termux-command>`\n- `/termux boot on|off|status`",
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
                                "Termux boot script: `{}`\nEnabled: {}",
                                status.script_path.display(),
                                status.enabled
                            )
                        } else {
                            format!(
                                "Termux boot aggiornato.\nScript: `{}`\nEnabled: {}",
                                status.script_path.display(),
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
        if let Some(provider_name) = preferred_provider {
            let response = if let Some(tool_defs) = &tools {
                provider_router
                    .chat_with_tools(
                        messages,
                        tool_defs.clone(),
                        Some(provider_name),
                        Some(retry_policy),
                    )
                    .await?
            } else {
                provider_router
                    .chat(messages, Some(provider_name), Some(retry_policy))
                    .await?
            };
            return Ok((response, provider_name.to_string()));
        }

        let mut last_error: Option<anyhow::Error> = None;
        let chain = if provider_chain.is_empty() {
            vec!["".to_string()]
        } else {
            provider_chain.to_vec()
        };

        for provider_name in chain {
            let result = if let Some(tool_defs) = &tools {
                provider_router
                    .chat_with_tools(
                        messages.clone(),
                        tool_defs.clone(),
                        if provider_name.is_empty() {
                            None
                        } else {
                            Some(provider_name.as_str())
                        },
                        Some(retry_policy),
                    )
                    .await
            } else {
                provider_router
                    .chat(
                        messages.clone(),
                        if provider_name.is_empty() {
                            None
                        } else {
                            Some(provider_name.as_str())
                        },
                        Some(retry_policy),
                    )
                    .await
            };

            match result {
                Ok(response) => {
                    let used = if provider_name.is_empty() {
                        "default".to_string()
                    } else {
                        provider_name
                    };
                    return Ok((response, used));
                }
                Err(e) => {
                    let provider_label = if provider_name.is_empty() {
                        "default"
                    } else {
                        provider_name.as_str()
                    };

                    if Self::is_auth_error(&e) {
                        return Err(anyhow::anyhow!(
                            "Provider '{}' auth/permission error for bot '{}': {}",
                            provider_label,
                            profile_name,
                            e
                        ));
                    }

                    warn!(
                        "Provider '{}' failed for bot '{}', trying fallback: {}",
                        provider_label, profile_name, e
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("No provider available")))
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

    async fn load_chat_memory_history(
        context: &BotContext,
        chat_id: i64,
        max_entries: usize,
    ) -> Vec<ChatMessage> {
        let path = context.memory_dir.join(format!("chat_{}.jsonl", chat_id));
        let content = match fs::read_to_string(path).await {
            Ok(c) => c,
            Err(_) => return Vec::new(),
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
        chat_id: i64,
        role: &str,
        content: &str,
    ) -> Result<()> {
        if content.trim().is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&context.memory_dir).await?;
        let path = context.memory_dir.join(format!("chat_{}.jsonl", chat_id));
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

    async fn update_summary_snapshot(context: &BotContext, chat_id: i64) -> Result<()> {
        let history = Self::load_chat_memory_history(context, chat_id, 6).await;
        if history.is_empty() {
            return Ok(());
        }

        let mut lines = vec![
            format!("# Chat Summary ({})", chat_id),
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

        let path = context.memory_dir.join(format!("summary_{}.md", chat_id));
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
}
