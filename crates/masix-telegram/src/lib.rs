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
    pub chat: TelegramChat,
    pub from: Option<TelegramUser>,
    #[serde(default)]
    pub reply_to_message: Option<Box<TelegramReplyToMessage>>,
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

    async fn handle_message(&self, message: &TelegramMessage) {
        let chat_id = message.chat.id;
        let message_id = message.message_id;

        if !self.is_chat_allowed(chat_id) {
            info!("Skipping message from unauthorized chat {}", chat_id);
            return;
        }

        if let Some(text) = &message.text {
            let from_username = message
                .from
                .as_ref()
                .and_then(|u| u.username.as_ref())
                .map(|s| s.as_str())
                .unwrap_or("unknown");
            let from_id = message
                .from
                .as_ref()
                .map(|u| u.id.to_string())
                .unwrap_or_default();

            info!("Received message from {}: {}", from_username, text);

            if let Some(event_bus) = &self.event_bus {
                let envelope = Envelope::new(
                    "telegram",
                    MessageKind::Message {
                        from: from_id,
                        text: text.clone(),
                    },
                )
                .with_chat_id(chat_id)
                .with_message_id(message_id)
                .with_payload(serde_json::json!({
                    "account_tag": self.account_tag.clone(),
                }));

                if let Err(e) = event_bus.publish(envelope) {
                    warn!("Failed to publish message to event bus: {}", e);
                }
            } else {
                info!("No event bus configured, message not forwarded");
            }
        }
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
            let envelope = Envelope::new(
                "telegram",
                MessageKind::Callback {
                    query_id: query_id.clone(),
                    data: data.cloned().unwrap_or_default(),
                },
            )
            .with_payload(serde_json::json!({
                "account_tag": self.account_tag.clone(),
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
            allowed_chats: None,
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
}
