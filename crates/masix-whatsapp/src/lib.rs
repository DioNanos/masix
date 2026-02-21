//! Masix WhatsApp Adapter
//!
//! Read-only WhatsApp listener over JSONL ingress (typically a local bridge process).
//! No outbound send path is provided in this crate by design.

use anyhow::{anyhow, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use masix_config::WhatsappConfig;
use masix_ipc::{Envelope, EventBus, MessageKind};
use serde::Deserialize;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{info, warn};

type HmacSha256 = Hmac<sha2::Sha256>;
const WHATSAPP_SCHEMA_VERSION: &str = "whatsapp.v1";
const DEFAULT_MAX_MESSAGE_CHARS: usize = 4000;

#[derive(Debug, Deserialize)]
struct IngressMessage {
    schema_version: String,
    from: String,
    text: String,
    #[serde(default)]
    ts: Option<i64>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    meta: Option<serde_json::Value>,
}

pub struct WhatsAppAdapter {
    transport_path: String,
    read_only: bool,
    max_message_chars: usize,
    allowed_senders: Option<HashSet<String>>,
    ingress_shared_secret: Option<String>,
    event_bus: Option<EventBus>,
}

impl WhatsAppAdapter {
    pub fn from_config(config: &WhatsappConfig) -> Self {
        let allowed_senders = if config.allowed_senders.is_empty() {
            None
        } else {
            Some(
                config
                    .allowed_senders
                    .iter()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect::<HashSet<_>>(),
            )
        };

        Self {
            transport_path: config
                .transport_path
                .clone()
                .unwrap_or_else(|| "whatsapp-transport.js".to_string()),
            read_only: config.read_only,
            max_message_chars: config
                .max_message_chars
                .unwrap_or(DEFAULT_MAX_MESSAGE_CHARS),
            allowed_senders,
            ingress_shared_secret: config.ingress_shared_secret.clone(),
            event_bus: None,
        }
    }

    pub fn with_event_bus(mut self, event_bus: EventBus) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    pub async fn start(&self) -> Result<()> {
        if !self.read_only {
            anyhow::bail!("WhatsApp adapter only supports read_only mode");
        }

        info!(
            "Starting WhatsApp read-only listener using transport '{}'",
            self.transport_path
        );

        let mut child = Command::new("node")
            .arg(&self.transport_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to open transport stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("Failed to open transport stderr"))?;

        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => warn!("whatsapp-transport stderr: {}", line.trim_end()),
                    Err(err) => {
                        warn!("whatsapp-transport stderr read error: {}", err);
                        break;
                    }
                }
            }
        });

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            let read = reader.read_line(&mut line).await?;
            if read == 0 {
                break;
            }
            let payload = line.trim();
            if payload.is_empty() {
                continue;
            }

            match self.parse_ingress_line(payload) {
                Ok(envelope) => {
                    if let Some(event_bus) = &self.event_bus {
                        if let Err(err) = event_bus.publish(envelope) {
                            warn!("Failed to publish WhatsApp event: {}", err);
                        }
                    }
                }
                Err(err) => warn!("Dropped WhatsApp ingress line: {}", err),
            }
        }

        let status = child.wait().await?;
        if !status.success() {
            warn!("whatsapp-transport exited with status: {}", status);
        }

        Ok(())
    }

    fn parse_ingress_line(&self, line: &str) -> Result<Envelope> {
        let msg: IngressMessage = serde_json::from_str(line)?;
        if msg.schema_version != WHATSAPP_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported schema_version '{}', expected '{}'",
                msg.schema_version,
                WHATSAPP_SCHEMA_VERSION
            );
        }

        let from = msg.from.trim();
        if from.is_empty() {
            anyhow::bail!("missing sender id");
        }
        if let Some(allowed) = &self.allowed_senders {
            if !allowed.contains(from) {
                anyhow::bail!("sender '{}' is not in allowed_senders", from);
            }
        }

        let text = msg.text.trim().to_string();
        if text.is_empty() {
            anyhow::bail!("empty text payload");
        }
        if text.chars().count() > self.max_message_chars {
            anyhow::bail!(
                "message too long ({} chars, max {})",
                text.chars().count(),
                self.max_message_chars
            );
        }

        self.verify_signature(&msg)?;

        let virtual_chat_id = Self::virtual_chat_id(from);
        let mut payload = serde_json::json!({
            "schema_version": msg.schema_version,
            "source": "whatsapp-bridge",
            "read_only": true,
        });
        if let Some(ts) = msg.ts {
            payload["ts"] = serde_json::json!(ts);
        }
        if let Some(meta) = msg.meta {
            payload["meta"] = meta;
        }

        Ok(Envelope::new(
            "whatsapp",
            MessageKind::Message {
                from: from.to_string(),
                text,
            },
        )
        .with_chat_id(virtual_chat_id)
        .with_payload(payload))
    }

    fn verify_signature(&self, msg: &IngressMessage) -> Result<()> {
        let Some(secret) = self.ingress_shared_secret.as_ref() else {
            return Ok(());
        };

        let signature = msg
            .signature
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("signature required but missing"))?;

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
        let payload = format!(
            "{}\n{}\n{}\n{}",
            msg.schema_version,
            msg.from,
            msg.text,
            msg.ts.unwrap_or_default()
        );
        mac.update(payload.as_bytes());
        let expected = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

        if expected != signature {
            anyhow::bail!("invalid signature");
        }

        Ok(())
    }

    fn virtual_chat_id(sender: &str) -> i64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        sender.hash(&mut hasher);
        let raw = hasher.finish() & 0x7FFF_FFFF_FFFF_FFFF;
        -((raw.max(1)) as i64)
    }
}
