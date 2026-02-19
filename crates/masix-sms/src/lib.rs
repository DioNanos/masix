//! Masix SMS - Termux:API Wrapper
//!
//! SMS and call log integration for Android Termux

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsMessage {
    pub address: String,
    pub body: String,
    pub date: i64,
    pub read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallLog {
    pub number: String,
    pub name: Option<String>,
    pub date: i64,
    pub duration: i64,
    pub call_type: String, // incoming, outgoing, missed
}

pub struct SmsAdapter {
    watch_interval_secs: u64,
}

impl SmsAdapter {
    pub fn new(watch_interval_secs: Option<u64>) -> Self {
        Self {
            watch_interval_secs: watch_interval_secs.unwrap_or(30),
        }
    }

    pub async fn list_sms(&self, limit: u32) -> Result<Vec<SmsMessage>> {
        let output = Command::new("termux-sms-list")
            .args(["-l", &limit.to_string(), "-o", "json"])
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("termux-sms-list failed"));
        }

        let messages: Vec<SmsMessage> = serde_json::from_slice(&output.stdout)?;
        Ok(messages)
    }

    pub async fn send_sms(&self, to: &str, text: &str) -> Result<()> {
        let status = Command::new("termux-sms-send")
            .args(["-n", to, "-c", text])
            .status()
            .await?;

        if !status.success() {
            return Err(anyhow::anyhow!("termux-sms-send failed"));
        }

        info!("SMS sent to {}", to);
        Ok(())
    }

    pub async fn list_calls(&self, limit: u32) -> Result<Vec<CallLog>> {
        let output = Command::new("termux-call-log")
            .args(["-l", &limit.to_string(), "-o", "json"])
            .output()
            .await?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("termux-call-log failed"));
        }

        let logs: Vec<CallLog> = serde_json::from_slice(&output.stdout)?;
        Ok(logs)
    }

    pub async fn watch(&self) -> Result<()> {
        info!(
            "Starting SMS watcher (interval: {}s)",
            self.watch_interval_secs
        );

        let mut last_seen_id = 0i64;

        loop {
            match self.list_sms(1).await {
                Ok(messages) => {
                    if let Some(latest) = messages.first() {
                        // Simple check if new message arrived
                        if latest.date > last_seen_id {
                            info!("New SMS from {}: {}", latest.address, latest.body);
                            last_seen_id = latest.date;

                            // TODO: Check automation rules and trigger actions
                        }
                    }
                }
                Err(e) => {
                    warn!("SMS watch error: {}", e);
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(self.watch_interval_secs)).await;
        }
    }
}
