//! Masix IPC - Inter-Process Communication
//!
//! Event bus for adapter-to-core communication

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

static NEXT_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn generate_trace_id() -> String {
    let ts = now_unix_secs();
    let n = NEXT_TRACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("trace-{}-{}", ts, n)
}

fn default_schema_version() -> u16 {
    1
}

fn default_trace_id() -> String {
    generate_trace_id()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(default = "default_schema_version")]
    pub schema_version: u16,
    #[serde(default = "default_trace_id")]
    pub trace_id: String,
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
            schema_version: default_schema_version(),
            trace_id: generate_trace_id(),
            id: generate_trace_id(),
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

    pub fn with_trace_id(mut self, trace_id: String) -> Self {
        self.trace_id = trace_id;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_has_schema_version() {
        let env = Envelope::new(
            "test",
            MessageKind::Message {
                from: "user".to_string(),
                text: "hello".to_string(),
            },
        );
        assert_eq!(env.schema_version, 1);
    }

    #[test]
    fn envelope_has_trace_id() {
        let env = Envelope::new(
            "test",
            MessageKind::Message {
                from: "user".to_string(),
                text: "hello".to_string(),
            },
        );
        assert!(env.trace_id.starts_with("trace-"));
    }

    #[test]
    fn backward_compat_deserialize_without_new_fields() {
        let old_json = r#"{
            "id": "test-id",
            "channel": "telegram",
            "kind": {"type": "message", "from": "user", "text": "hello"},
            "payload": {},
            "chat_id": 123,
            "message_id": 456
        }"#;
        let env: Envelope = serde_json::from_str(old_json).expect("deserialize");
        assert_eq!(env.schema_version, 1);
        assert!(env.trace_id.starts_with("trace-"));
        assert_eq!(env.id, "test-id");
    }

    #[test]
    fn trace_id_different_for_each_envelope() {
        let env1 = Envelope::new(
            "test",
            MessageKind::Message {
                from: "user".to_string(),
                text: "hello".to_string(),
            },
        );
        let env2 = Envelope::new(
            "test",
            MessageKind::Message {
                from: "user".to_string(),
                text: "hello".to_string(),
            },
        );
        assert_ne!(env1.trace_id, env2.trace_id);
    }

    #[test]
    fn serialize_roundtrip_preserves_new_fields() {
        let env = Envelope::new(
            "telegram",
            MessageKind::Message {
                from: "user".to_string(),
                text: "test".to_string(),
            },
        )
        .with_chat_id(123)
        .with_message_id(456);

        let json = serde_json::to_string(&env).expect("serialize");
        let parsed: Envelope = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed.schema_version, env.schema_version);
        assert_eq!(parsed.trace_id, env.trace_id);
        assert_eq!(parsed.id, env.id);
        assert_eq!(parsed.chat_id, Some(123));
        assert_eq!(parsed.message_id, Some(456));
    }

    #[test]
    fn legacy_json_with_missing_fields_gets_defaults() {
        let legacy_jsons = vec![
            r#"{"id":"x","channel":"telegram","kind":{"type":"message","from":"u","text":"t"},"payload":{}}"#,
            r#"{"id":"y","channel":"telegram","kind":{"type":"command","name":"help","args":[]},"payload":{}}"#,
        ];

        for json in legacy_jsons {
            let env: Envelope =
                serde_json::from_str(json).unwrap_or_else(|_| panic!("parse: {}", json));
            assert_eq!(env.schema_version, 1, "schema_version should default to 1");
            assert!(!env.trace_id.is_empty(), "trace_id should be generated");
        }
    }
}
