//! Masix Core
//!
//! Main runtime orchestration with MCP + Cron + LLM support

use anyhow::Result;
use masix_config::Config;
use masix_ipc::{Envelope, EventBus, MessageKind, OutboundMessage};
use masix_mcp::McpClient;
use masix_policy::PolicyEngine;
use masix_providers::{
    ChatMessage, OpenAICompatibleProvider, ProviderRouter, ToolCall, ToolDefinition,
};
use masix_storage::Storage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info, warn};

const MAX_TOOL_ITERATIONS: usize = 5;

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

        self.start_telegram_adapters().await?;

        let mut inbound_rx = self.event_bus.subscribe();
        let outbound_for_processor = outbound_sender.clone();
        let provider_router = Arc::clone(&self.provider_router);
        let storage_for_processor = Arc::clone(&self.storage);
        let mcp_client = self.mcp_client.clone();
        let system_prompt = self.system_prompt.clone();
        let policy = self.policy.clone();
        let rate_state: Arc<Mutex<HashMap<String, (i64, u32)>>> =
            Arc::new(Mutex::new(HashMap::new()));

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
                                    &mcp_client,
                                    &system_prompt,
                                    &policy,
                                    &rate_state,
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
                        if let Err(e) = Self::check_cron_jobs(&storage_for_processor, &outbound_for_processor).await {
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

            if let Ok(chat_id) = recipient.parse::<i64>() {
                let msg = OutboundMessage {
                    channel,
                    account_tag: None,
                    chat_id,
                    text: message,
                    reply_to: None,
                    edit_message_id: None,
                    inline_keyboard: None,
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

    async fn start_telegram_adapters(&self) -> Result<()> {
        if let Some(telegram_config) = &self.config.telegram {
            let data_dir = self.get_data_dir()?;
            let poll_timeout = telegram_config.poll_timeout_secs;
            let recreate_interval = telegram_config.client_recreate_interval_secs;

            for (idx, account) in telegram_config.accounts.iter().enumerate() {
                info!("Telegram adapter enabled for account #{}", idx + 1);

                let account_clone = account.clone();
                let data_dir_clone = data_dir.clone();
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
        mcp_client: &Option<Arc<Mutex<McpClient>>>,
        system_prompt: &str,
        policy: &PolicyEngine,
        rate_state: &Arc<Mutex<HashMap<String, (i64, u32)>>>,
    ) -> Result<()> {
        let account_tag = envelope
            .payload
            .get("account_tag")
            .and_then(|v| v.as_str())
            .map(|value| value.to_string());

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
                        };
                        let _ = outbound_sender.send(msg);
                    }
                    return Ok(());
                }

                let tools = Self::get_mcp_tools(mcp_client).await;
                let has_tools = !tools.is_empty();

                let mut messages = vec![
                    ChatMessage {
                        role: "system".to_string(),
                        content: Some(system_prompt.to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    },
                    ChatMessage {
                        role: "user".to_string(),
                        content: Some(text.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    },
                ];

                let mut final_response = String::new();
                let mut iterations = 0;

                loop {
                    iterations += 1;
                    if iterations > MAX_TOOL_ITERATIONS {
                        warn!("Max tool iterations reached");
                        break;
                    }

                    let response = if has_tools {
                        provider_router
                            .chat_with_tools(messages.clone(), tools.clone(), None)
                            .await?
                    } else {
                        provider_router.chat(messages.clone(), None).await?
                    };

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
                    let msg = OutboundMessage {
                        channel: envelope.channel.clone(),
                        account_tag: account_tag.clone(),
                        chat_id,
                        text: final_response,
                        reply_to: envelope.message_id,
                        edit_message_id: None,
                        inline_keyboard: None,
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
        let response = self.provider_router.chat(messages, provider).await?;
        Ok(response.content.unwrap_or_default())
    }
}
