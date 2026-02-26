//! Masix Telegram Adapter
//!
//! Telegram Bot API long-polling with offset persistence, client recreation,
//! inline keyboards, callback queries, and message chunking

pub mod menu;

use anyhow::{anyhow, Result};
use masix_config::TelegramAccount;
use masix_ipc::{Envelope, EventBus, InlineButton, MessageKind, OutboundMessage};
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::broadcast;
use tracing::{info, warn};

const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message: Option<TelegramMessage>,
    #[serde(default)]
    pub callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramMessage {
    pub message_id: i64,
    #[serde(default)]
    pub message_thread_id: Option<i64>,
    pub text: Option<String>,
    pub caption: Option<String>,
    #[serde(default)]
    pub photo: Option<Vec<TelegramPhotoSize>>,
    #[serde(default)]
    pub document: Option<TelegramDocument>,
    #[serde(default)]
    pub video: Option<TelegramVideo>,
    #[serde(default)]
    pub voice: Option<TelegramVoice>,
    #[serde(default)]
    pub audio: Option<TelegramAudio>,
    pub chat: TelegramChat,
    pub from: Option<TelegramUser>,
    #[serde(default)]
    pub reply_to_message: Option<Box<TelegramReplyToMessage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramPhotoSize {
    pub file_id: String,
    pub width: i64,
    pub height: i64,
    #[serde(default)]
    pub file_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramDocument {
    pub file_id: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub file_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramVideo {
    pub file_id: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub width: Option<i64>,
    #[serde(default)]
    pub height: Option<i64>,
    #[serde(default)]
    pub file_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramVoice {
    pub file_id: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub duration: Option<i64>,
    #[serde(default)]
    pub file_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramAudio {
    pub file_id: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub duration: Option<i64>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub performer: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub file_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramUser {
    pub id: i64,
    #[serde(default)]
    pub is_bot: Option<bool>,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramReplyToMessage {
    pub message_id: i64,
    pub from: Option<TelegramUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramCallbackQuery {
    pub id: String,
    pub from: TelegramUser,
    pub message: Option<TelegramMessage>,
    pub data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    ok: bool,
    result: T,
}

pub struct TelegramAdapter {
    client: Client,
    bot_token: String,
    account_tag: String,
    allowed_chats: Option<HashSet<i64>>,
    api_url: String,
    data_dir: PathBuf,
    poll_timeout_secs: u64,
    client_recreate_interval_secs: u64,
    event_bus: Option<EventBus>,
}

impl TelegramAdapter {
    pub fn new(
        account: &TelegramAccount,
        data_dir: PathBuf,
        config_timeout: Option<u64>,
        config_recreate: Option<u64>,
    ) -> Self {
        let api_url = format!("https://api.telegram.org/bot{}", account.bot_token);
        let account_tag = account
            .bot_token
            .split(':')
            .next()
            .unwrap_or("default")
            .to_string();
        let allowed_chats = account
            .allowed_chats
            .clone()
            .map(|items| items.into_iter().collect());
        let client = Self::build_client();
        let poll_timeout_secs = config_timeout.unwrap_or(60);
        let client_recreate_interval_secs = config_recreate.unwrap_or(60);

        Self {
            client,
            bot_token: account.bot_token.clone(),
            account_tag,
            allowed_chats,
            api_url,
            data_dir,
            poll_timeout_secs,
            client_recreate_interval_secs,
            event_bus: None,
        }
    }

    pub fn with_event_bus(mut self, event_bus: EventBus) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    fn build_client() -> Client {
        ClientBuilder::new()
            .pool_idle_timeout(Duration::from_secs(600))
            .pool_max_idle_per_host(10)
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .timeout(Duration::from_secs(180))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client")
    }

    fn offset_path(&self) -> PathBuf {
        let runtime_dir = self.data_dir.join("runtime");
        let _ = std::fs::create_dir_all(&runtime_dir);
        let bot_id = self.bot_token.split(':').next().unwrap_or("default");
        runtime_dir.join(format!("telegram.{}.offset", bot_id))
    }

    fn is_chat_allowed(&self, chat_id: i64) -> bool {
        self.allowed_chats
            .as_ref()
            .is_none_or(|allowed| allowed.contains(&chat_id))
    }

    async fn read_offset(&self) -> Option<i64> {
        let p = self.offset_path();
        match fs::read_to_string(&p).await {
            Ok(content) => content.trim().parse().ok(),
            Err(_) => None,
        }
    }

    async fn write_offset(&self, offset: i64) {
        let p = self.offset_path();
        let _ = fs::create_dir_all(p.parent().unwrap()).await;
        let _ = fs::write(&p, format!("{}\n", offset)).await;
    }

    pub async fn get_updates(
        &self,
        client: &Client,
        offset: Option<i64>,
    ) -> Result<Vec<TelegramUpdate>> {
        let url = format!("{}/getUpdates", self.api_url);

        let mut payload = serde_json::json!({
            "timeout": self.poll_timeout_secs,
            "allowed_updates": ["message", "callback_query"],
        });

        if let Some(offset) = offset {
            payload["offset"] = serde_json::json!(offset);
        }

        let resp = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow!("telegram getUpdates request failed: {}", e))?
            .error_for_status()
            .map_err(|e| anyhow!("telegram getUpdates HTTP error: {}", e))?;

        let parsed: ApiResponse<Vec<TelegramUpdate>> = resp
            .json()
            .await
            .map_err(|e| anyhow!("telegram getUpdates decode failed: {}", e))?;

        if !parsed.ok {
            return Err(anyhow!("telegram getUpdates returned ok=false"));
        }

        Ok(parsed.result)
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_to: Option<i64>,
        inline_keyboard: Option<Vec<Vec<InlineButton>>>,
    ) -> Result<()> {
        let chunks = self.chunk_message(text);

        for (i, chunk) in chunks.iter().enumerate() {
            let url = format!("{}/sendMessage", self.api_url);

            let mut payload = serde_json::json!({
                "chat_id": chat_id,
                "text": chunk,
                "parse_mode": "Markdown",
            });

            if let Some(reply_to_message_id) = reply_to {
                if i == 0 {
                    payload["reply_to_message_id"] = serde_json::json!(reply_to_message_id);
                }
            }

            if i == chunks.len() - 1 {
                if let Some(keyboard) = &inline_keyboard {
                    payload["reply_markup"] = serde_json::json!({
                        "inline_keyboard": keyboard.iter().map(|row| {
                            row.iter().map(|btn| serde_json::json!({
                                "text": btn.text,
                                "callback_data": btn.callback_data
                            })).collect::<Vec<_>>()
                        }).collect::<Vec<_>>()
                    });
                }
            }

            self.send_with_markdown_fallback(&url, payload).await?;
        }

        Ok(())
    }

    pub async fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        inline_keyboard: Option<Vec<Vec<InlineButton>>>,
    ) -> Result<()> {
        // editMessageText cannot be split into chunks: fallback to a new message if too long.
        if text.chars().count() > TELEGRAM_MAX_MESSAGE_LEN {
            return self
                .send_message(chat_id, text, None, inline_keyboard)
                .await;
        }

        let url = format!("{}/editMessageText", self.api_url);

        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
            "parse_mode": "Markdown",
        });

        if let Some(keyboard) = &inline_keyboard {
            payload["reply_markup"] = serde_json::json!({
                "inline_keyboard": keyboard.iter().map(|row| {
                    row.iter().map(|btn| serde_json::json!({
                        "text": btn.text,
                        "callback_data": btn.callback_data
                    })).collect::<Vec<_>>()
                }).collect::<Vec<_>>()
            });
        }

        self.send_with_markdown_fallback(&url, payload).await
    }

    pub async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> Result<()> {
        let url = format!("{}/answerCallbackQuery", self.api_url);

        let mut payload = serde_json::json!({
            "callback_query_id": callback_query_id,
        });

        if let Some(t) = text {
            payload["text"] = serde_json::json!(t);
        }

        let _ = self.client.post(&url).json(&payload).send().await;
        Ok(())
    }

    pub async fn send_chat_action(&self, chat_id: i64, action: &str) -> Result<()> {
        let url = format!("{}/sendChatAction", self.api_url);
        let payload = serde_json::json!({
            "chat_id": chat_id,
            "action": action,
        });
        let _ = self.client.post(&url).json(&payload).send().await;
        Ok(())
    }

    async fn send_with_markdown_fallback(
        &self,
        url: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        let endpoint = url.rsplit('/').next().unwrap_or("telegram");

        let first_resp = self
            .client
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow!("telegram {} request failed: {}", endpoint, e))?;

        if first_resp.status().is_success() {
            let parsed: ApiResponse<serde_json::Value> = first_resp
                .json()
                .await
                .map_err(|e| anyhow!("telegram {} decode failed: {}", endpoint, e))?;
            if parsed.ok {
                return Ok(());
            }
            warn!(
                "telegram {} returned ok=false with Markdown payload, retrying without parse_mode",
                endpoint
            );
        } else {
            let status = first_resp.status();
            let body = first_resp.text().await.unwrap_or_default();
            warn!(
                "telegram {} HTTP {} with Markdown payload, retrying without parse_mode: {}",
                endpoint, status, body
            );
        }

        let mut fallback_payload = payload;
        if let Some(obj) = fallback_payload.as_object_mut() {
            obj.remove("parse_mode");
        }

        let fallback_resp = self
            .client
            .post(url)
            .json(&fallback_payload)
            .send()
            .await
            .map_err(|e| anyhow!("telegram {} fallback request failed: {}", endpoint, e))?;

        if !fallback_resp.status().is_success() {
            let status = fallback_resp.status();
            let body = fallback_resp.text().await.unwrap_or_default();
            if Self::is_reply_target_missing(&body) {
                let mut no_reply_payload = fallback_payload.clone();
                if Self::remove_reply_to_message_id(&mut no_reply_payload) {
                    warn!(
                        "telegram {} fallback failed due to missing reply target; retrying without reply_to_message_id",
                        endpoint
                    );
                    return self
                        .send_without_reply_target(url, endpoint, no_reply_payload)
                        .await;
                }
            }
            return Err(anyhow!(
                "telegram {} fallback HTTP {}: {}",
                endpoint,
                status,
                body
            ));
        }

        let parsed: ApiResponse<serde_json::Value> = fallback_resp
            .json()
            .await
            .map_err(|e| anyhow!("telegram {} fallback decode failed: {}", endpoint, e))?;
        if !parsed.ok {
            return Err(anyhow!("telegram {} fallback returned ok=false", endpoint));
        }

        Ok(())
    }

    async fn send_without_reply_target(
        &self,
        url: &str,
        endpoint: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        let resp = self
            .client
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow!("telegram {} no-reply retry request failed: {}", endpoint, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "telegram {} no-reply retry HTTP {}: {}",
                endpoint,
                status,
                body
            ));
        }

        let parsed: ApiResponse<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| anyhow!("telegram {} no-reply retry decode failed: {}", endpoint, e))?;
        if !parsed.ok {
            return Err(anyhow!(
                "telegram {} no-reply retry returned ok=false",
                endpoint
            ));
        }

        Ok(())
    }

    fn remove_reply_to_message_id(payload: &mut serde_json::Value) -> bool {
        payload
            .as_object_mut()
            .map(|obj| obj.remove("reply_to_message_id").is_some())
            .unwrap_or(false)
    }

    fn is_reply_target_missing(body: &str) -> bool {
        body.to_ascii_lowercase()
            .contains("message to be replied not found")
    }

    fn chunk_message(&self, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        if chars.len() <= TELEGRAM_MAX_MESSAGE_LEN {
            return vec![text.to_string()];
        }

        let mut chunks = Vec::new();
        let mut start = 0usize;

        while start < chars.len() {
            let mut end = (start + TELEGRAM_MAX_MESSAGE_LEN).min(chars.len());

            if end < chars.len() {
                let mut split = end;
                for i in (start..end).rev() {
                    let c = chars[i];
                    if c == '\n' || c == ' ' || c == '.' || c == '!' || c == '?' {
                        split = i + 1;
                        break;
                    }
                }
                if split > start {
                    end = split;
                }
            }

            chunks.push(chars[start..end].iter().collect::<String>());
            start = end;
        }

        chunks
    }

    pub async fn poll(&self) -> Result<()> {
        let mut offset: Option<i64> = self.read_offset().await;

        info!(offset = ?offset, "Telegram polling started");

        let mut client = self.client.clone();
        let mut client_recreate_at =
            Instant::now() + Duration::from_secs(self.client_recreate_interval_secs);

        if let Err(err) = self.sync_bot_commands(&client).await {
            warn!("Failed to sync Telegram bot commands: {}", err);
        } else {
            info!("Telegram bot commands synced");
        }

        loop {
            if Instant::now() >= client_recreate_at {
                info!("Recreating HTTP client to prevent stale connections");
                client = Self::build_client();
                client_recreate_at =
                    Instant::now() + Duration::from_secs(self.client_recreate_interval_secs);
            }

            let updates = match self.get_updates(&client, offset).await {
                Ok(v) => v,
                Err(err) => {
                    warn!("Telegram polling error: {}", err);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            for update in updates {
                offset = Some(update.update_id + 1);
                self.write_offset(update.update_id + 1).await;

                if let Some(message) = &update.message {
                    self.handle_message(message).await;
                }

                if let Some(callback) = &update.callback_query {
                    self.handle_callback(callback).await;
                }
            }
        }
    }

    async fn sync_bot_commands(&self, client: &Client) -> Result<()> {
        let url = format!("{}/setMyCommands", self.api_url);
        let commands = serde_json::json!([
            { "command": "start", "description": "Open main menu" },
            { "command": "menu", "description": "Show menu" },
            { "command": "new", "description": "Reset conversation" },
            { "command": "help", "description": "Show help" },
            { "command": "whoiam", "description": "Show IDs and scope" },
            { "command": "language", "description": "Set language" },
            { "command": "provider", "description": "Manage provider" },
            { "command": "model", "description": "Set model" },
            { "command": "mcp", "description": "Show MCP status" },
            { "command": "tools", "description": "List runtime tools" },
            { "command": "cron", "description": "Manage reminders" },
            { "command": "exec", "description": "Run shell commands" },
            { "command": "termux", "description": "Use Termux tools" }
        ]);

        let payload = serde_json::json!({ "commands": commands });
        let resp = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow!("telegram setMyCommands request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("telegram setMyCommands HTTP {}: {}", status, body));
        }

        let parsed: ApiResponse<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| anyhow!("telegram setMyCommands decode failed: {}", e))?;

        if !parsed.ok {
            return Err(anyhow!("telegram setMyCommands returned ok=false"));
        }

        Ok(())
    }

    async fn handle_message(&self, message: &TelegramMessage) {
        let chat_id = message.chat.id;
        let message_id = message.message_id;

        if !self.is_chat_allowed(chat_id) {
            info!("Skipping message from unauthorized chat {}", chat_id);
            return;
        }

        let inbound_text = message
            .text
            .clone()
            .or_else(|| message.caption.clone())
            .or_else(|| {
                if message
                    .photo
                    .as_ref()
                    .is_some_and(|items| !items.is_empty())
                {
                    Some("[Media: photo]".to_string())
                } else if message
                    .document
                    .as_ref()
                    .and_then(|doc| doc.mime_type.as_deref())
                    .is_some_and(|mime| mime.starts_with("image/"))
                {
                    Some("[Media: image_document]".to_string())
                } else if message.video.is_some() {
                    Some("[Media: video]".to_string())
                } else if message.voice.is_some() {
                    Some("[Media: voice]".to_string())
                } else if message.audio.is_some() {
                    Some("[Media: audio]".to_string())
                } else {
                    None
                }
            });

        if let Some(text) = inbound_text {
            let from_username = message
                .from
                .as_ref()
                .and_then(|u| u.username.as_ref())
                .map(|s| s.as_str())
                .unwrap_or("unknown");
            let from_user_id = message.from.as_ref().map(|u| u.id);
            let from_id = message
                .from
                .as_ref()
                .map(|u| u.id.to_string())
                .unwrap_or_default();

            info!("Received message from {}: {}", from_username, text);

            if let Some(event_bus) = &self.event_bus {
                let mut payload = serde_json::json!({
                    "account_tag": self.account_tag.clone(),
                    "chat_type": message.chat.chat_type.clone(),
                });
                if let Some(from_user_id) = from_user_id {
                    if let Some(obj) = payload.as_object_mut() {
                        obj.insert("from_user_id".to_string(), serde_json::json!(from_user_id));
                    }
                }
                if let Some(media) = Self::extract_media_payload(message) {
                    if let Some(obj) = payload.as_object_mut() {
                        obj.insert("media".to_string(), media);
                    }
                }

                let envelope = Envelope::new(
                    "telegram",
                    MessageKind::Message {
                        from: from_id,
                        text,
                    },
                )
                .with_chat_id(chat_id)
                .with_message_id(message_id)
                .with_payload(payload);

                if let Err(e) = event_bus.publish(envelope) {
                    warn!("Failed to publish message to event bus: {}", e);
                }
            } else {
                info!("No event bus configured, message not forwarded");
            }
        }
    }

    fn extract_media_payload(message: &TelegramMessage) -> Option<serde_json::Value> {
        let caption = message
            .caption
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());

        if let Some(sizes) = &message.photo {
            if let Some(best) = sizes
                .iter()
                .max_by_key(|item| item.width.saturating_mul(item.height))
            {
                return Some(serde_json::json!({
                    "kind": "photo",
                    "file_id": best.file_id,
                    "width": best.width,
                    "height": best.height,
                    "file_size": best.file_size,
                    "caption": caption,
                }));
            }
        }

        if let Some(document) = &message.document {
            if document
                .mime_type
                .as_deref()
                .is_some_and(|mime| mime.starts_with("image/"))
            {
                return Some(serde_json::json!({
                    "kind": "image_document",
                    "file_id": document.file_id,
                    "mime_type": document.mime_type,
                    "file_name": document.file_name,
                    "file_size": document.file_size,
                    "caption": caption,
                }));
            }
        }

        if let Some(video) = &message.video {
            return Some(serde_json::json!({
                "kind": "video",
                "file_id": video.file_id,
                "mime_type": video.mime_type,
                "width": video.width,
                "height": video.height,
                "file_size": video.file_size,
                "caption": caption,
            }));
        }

        if let Some(voice) = &message.voice {
            return Some(serde_json::json!({
                "kind": "voice",
                "file_id": voice.file_id,
                "mime_type": voice.mime_type,
                "duration": voice.duration,
                "file_size": voice.file_size,
                "caption": caption,
            }));
        }

        if let Some(audio) = &message.audio {
            return Some(serde_json::json!({
                "kind": "audio",
                "file_id": audio.file_id,
                "mime_type": audio.mime_type,
                "duration": audio.duration,
                "file_name": audio.file_name,
                "performer": audio.performer,
                "title": audio.title,
                "file_size": audio.file_size,
                "caption": caption,
            }));
        }

        None
    }

    async fn handle_callback(&self, callback: &TelegramCallbackQuery) {
        let query_id = &callback.id;
        let chat_id = callback.message.as_ref().map(|m| m.chat.id);
        let message_id = callback.message.as_ref().map(|m| m.message_id);
        let data = callback.data.as_ref();

        info!("Received callback query: {:?}", data);

        if let Some(chat_id) = chat_id {
            if !self.is_chat_allowed(chat_id) {
                info!("Skipping callback from unauthorized chat {}", chat_id);
                return;
            }
        }

        if let Some(event_bus) = &self.event_bus {
            let chat_type = callback
                .message
                .as_ref()
                .map(|msg| msg.chat.chat_type.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let envelope = Envelope::new(
                "telegram",
                MessageKind::Callback {
                    query_id: query_id.clone(),
                    data: data.cloned().unwrap_or_default(),
                },
            )
            .with_payload(serde_json::json!({
                "account_tag": self.account_tag.clone(),
                "chat_type": chat_type,
                "from_user_id": callback.from.id,
            }));

            if let Some(chat_id) = chat_id {
                let mut envelope = envelope.with_chat_id(chat_id);
                if let Some(message_id) = message_id {
                    envelope = envelope.with_message_id(message_id);
                }
                if let Err(e) = event_bus.publish(envelope) {
                    warn!("Failed to publish callback to event bus: {}", e);
                }
            }
        }

        let _ = self.answer_callback_query(query_id, None).await;
    }

    pub async fn run_outbound_handler(&self, mut receiver: broadcast::Receiver<OutboundMessage>) {
        info!("Telegram outbound handler started");

        loop {
            match receiver.recv().await {
                Ok(msg) => {
                    if msg.channel != "telegram" {
                        continue;
                    }
                    if let Some(account_tag) = &msg.account_tag {
                        if account_tag != &self.account_tag {
                            continue;
                        }
                    }

                    // Handle chat action (typing, etc.)
                    if let Some(action) = &msg.chat_action {
                        if let Err(e) = self.send_chat_action(msg.chat_id, action).await {
                            warn!("Failed to send chat action: {}", e);
                        }
                        continue;
                    }

                    let send_result = if let Some(message_id) = msg.edit_message_id {
                        self.edit_message_text(
                            msg.chat_id,
                            message_id,
                            &msg.text,
                            msg.inline_keyboard,
                        )
                        .await
                    } else {
                        self.send_message(msg.chat_id, &msg.text, msg.reply_to, msg.inline_keyboard)
                            .await
                    };

                    if let Err(e) = send_result {
                        warn!("Failed to send outbound message: {}", e);
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Telegram outbound handler stopped: channel closed");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(
                        "Telegram outbound handler lagged; skipped {} messages",
                        skipped
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TelegramAdapter;
    use masix_config::TelegramAccount;

    fn make_adapter() -> TelegramAdapter {
        let account = TelegramAccount {
            bot_token: "123456:TESTTOKEN".to_string(),
            bot_name: None,
            allowed_chats: None,
            bot_profile: None,
            admins: vec![],
            users: vec![],
            readonly: vec![],
            isolated: true,
            shared_memory_with: vec![],
            allow_self_memory_edit: true,
            group_mode: masix_config::GroupMode::All,
            auto_register_users: false,
            register_to_file: None,
            user_tools_mode: masix_config::UserToolsMode::None,
            user_allowed_tools: vec![],
        };
        TelegramAdapter::new(&account, std::env::temp_dir(), Some(60), Some(60))
    }

    #[test]
    fn chunk_message_preserves_content_for_unicode_text() {
        let adapter = make_adapter();
        let text = format!("{} {}", "😀".repeat(5000), "fine");
        let chunks = adapter.chunk_message(&text);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_message_respects_telegram_limit_by_characters() {
        let adapter = make_adapter();
        let text = "abc😀".repeat(1500);
        let chunks = adapter.chunk_message(&text);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 4096));
    }

    #[test]
    fn remove_reply_to_message_id_when_present() {
        let mut payload = serde_json::json!({
            "chat_id": 123,
            "text": "hello",
            "reply_to_message_id": 42
        });
        assert!(TelegramAdapter::remove_reply_to_message_id(&mut payload));
        assert!(payload.get("reply_to_message_id").is_none());
    }

    #[test]
    fn detect_missing_reply_target_error() {
        let body = r#"{"ok":false,"error_code":400,"description":"Bad Request: message to be replied not found"}"#;
        assert!(TelegramAdapter::is_reply_target_missing(body));
    }
}
