//! Masix WhatsApp Adapter
//!
//! WhatsApp adapter using Node.js JSONL transport (whatsapp-web.js)

use anyhow::Result;
use masix_ipc::Envelope;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{info, warn};

pub struct WhatsAppAdapter {
    transport_path: String,
}

impl WhatsAppAdapter {
    pub fn new(transport_path: Option<String>) -> Self {
        Self {
            transport_path: transport_path.unwrap_or_else(|| "whatsapp-transport.js".to_string()),
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting WhatsApp transport...");

        let mut child = Command::new("node")
            .arg(&self.transport_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let _stdin = child.stdin.take().expect("Failed to open stdin");
        let stdout = child.stdout.take().expect("Failed to open stdout");
        let mut reader = BufReader::new(stdout);

        // Read JSONL messages from transport
        tokio::spawn(async move {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(envelope) = Envelope::from_json(&line.trim()) {
                            info!("Received WhatsApp message: {:?}", envelope);
                            // TODO: Process message
                        }
                    }
                    Err(e) => {
                        warn!("Read error: {}", e);
                        break;
                    }
                }
            }
        });

        // Keep process alive
        let _ = child.wait().await;

        Ok(())
    }

    pub async fn send_message(&self, to: &str, text: &str) -> Result<()> {
        let command = serde_json::json!({
            "action": "send",
            "to": to,
            "text": text
        });

        // TODO: Write to transport stdin
        let _ = command;

        Ok(())
    }
}
