//! Masix IPC - Inter-Process Communication
//!
//! Event bus for adapter-to-core communication

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub id: String,
    pub channel: String,
    pub kind: MessageKind,
    pub payload: serde_json::Value,
    pub chat_id: Option<i64>,
    pub message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessageKind {
    #[serde(rename = "message")]
    Message { from: String, text: String },

    #[serde(rename = "reply")]
    Reply { to: String, text: String },

    #[serde(rename = "callback")]
    Callback { query_id: String, data: String },

    #[serde(rename = "command")]
    Command { name: String, args: Vec<String> },

    #[serde(rename = "error")]
    Error { code: u16, message: String },
}

#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub channel: String,
    pub account_tag: Option<String>,
    pub chat_id: i64,
    pub text: String,
    pub reply_to: Option<i64>,
    pub edit_message_id: Option<i64>,
    pub inline_keyboard: Option<Vec<Vec<InlineButton>>>,
    pub chat_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineButton {
    pub text: String,
    pub callback_data: String,
}

impl Envelope {
    pub fn new(channel: &str, kind: MessageKind) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            channel: channel.to_string(),
            kind,
            payload: serde_json::json!({}),
            chat_id: None,
            message_id: None,
        }
    }

    pub fn with_chat_id(mut self, chat_id: i64) -> Self {
        self.chat_id = Some(chat_id);
        self
    }

    pub fn with_message_id(mut self, message_id: i64) -> Self {
        self.message_id = Some(message_id);
        self
    }

    pub fn with_payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

pub const EVENT_BUS_CAPACITY: usize = 256;
pub const OUTBOUND_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct EventBus {
    inbound: broadcast::Sender<Envelope>,
    outbound: broadcast::Sender<OutboundMessage>,
}

impl EventBus {
    pub fn new() -> Self {
        let (inbound_tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        let (outbound_tx, _) = broadcast::channel(OUTBOUND_CAPACITY);

        Self {
            inbound: inbound_tx,
            outbound: outbound_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.inbound.subscribe()
    }

    pub fn publish(&self, envelope: Envelope) -> anyhow::Result<()> {
        self.inbound.send(envelope)?;
        Ok(())
    }

    pub fn outbound_sender(&self) -> broadcast::Sender<OutboundMessage> {
        self.outbound.clone()
    }

    pub fn outbound_subscribe(&self) -> broadcast::Receiver<OutboundMessage> {
        self.outbound.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
