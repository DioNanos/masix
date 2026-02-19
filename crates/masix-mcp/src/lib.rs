//! Masix MCP Client
//!
//! Model Context Protocol client for tool integration

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{oneshot, Mutex};
use tokio::time::{timeout, Duration};
use tracing::{debug, info, warn};

const MCP_REQUEST_TIMEOUT_SECS: u64 = 30;
type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { data: String, mime_type: String },
}

pub struct McpServer {
    name: String,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    request_id: Arc<Mutex<u64>>,
    pending: PendingMap,
}

impl McpServer {
    pub async fn start(name: String, command: String, args: Vec<String>) -> Result<Self> {
        info!("Starting MCP server '{}' ({})", name, command);

        let mut child = Command::new(command)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("Failed to get stderr"))?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        Self::spawn_stdout_router(name.clone(), stdout, Arc::clone(&pending));
        Self::spawn_stderr_logger(name.clone(), stderr);

        let server = Self {
            name,
            stdin: Arc::new(Mutex::new(stdin)),
            request_id: Arc::new(Mutex::new(1)),
            pending,
        };

        // Initialize MCP connection
        server.initialize().await?;

        Ok(server)
    }

    async fn initialize(&self) -> Result<()> {
        let id = self.next_id().await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "masix",
                    "version": "0.1.0"
                }
            }
        });

        self.send_request(request).await?;

        // Send initialized notification
        let initialized = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        self.send_notification(initialized).await?;

        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        let id = self.next_id().await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list",
            "params": {}
        });

        let response = self.send_request(request).await?;

        let tools = response["result"]
            .get("tools")
            .and_then(|t| serde_json::from_value(t.clone()).ok())
            .unwrap_or_default();

        Ok(tools)
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolResult> {
        let id = self.next_id().await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        });

        let response = self.send_request(request).await?;

        let result = response["result"].clone();
        let is_error = result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let content = result["content"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| serde_json::from_value(c.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(ToolResult { content, is_error })
    }

    pub async fn list_resources(&self) -> Result<Vec<Resource>> {
        let id = self.next_id().await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "resources/list",
            "params": {}
        });

        let response = self.send_request(request).await?;

        let resources = response["result"]
            .get("resources")
            .and_then(|r| serde_json::from_value(r.clone()).ok())
            .unwrap_or_default();

        Ok(resources)
    }

    pub async fn read_resource(&self, uri: &str) -> Result<String> {
        let id = self.next_id().await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "resources/read",
            "params": {
                "uri": uri
            }
        });

        let response = self.send_request(request).await?;

        let contents = response["result"]
            .get("contents")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(contents)
    }

    async fn send_request(&self, request: Value) -> Result<Value> {
        let id = request
            .get("id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("MCP request missing numeric id"))?;

        let (tx, rx) = oneshot::channel::<Value>();
        self.pending.lock().await.insert(id, tx);

        if let Err(e) = self.send_json_line(&request).await {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }

        let response = match timeout(Duration::from_secs(MCP_REQUEST_TIMEOUT_SECS), rx).await {
            Ok(Ok(value)) => value,
            Ok(Err(_)) => {
                return Err(anyhow!(
                    "MCP response channel closed (server='{}', id={})",
                    self.name,
                    id
                ))
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                return Err(anyhow!(
                    "MCP request timeout after {}s (server='{}', id={})",
                    MCP_REQUEST_TIMEOUT_SECS,
                    self.name,
                    id
                ));
            }
        };

        if let Some(error) = response.get("error") {
            return Err(anyhow!("MCP error from '{}': {:?}", self.name, error));
        }

        Ok(response)
    }

    async fn send_notification(&self, notification: Value) -> Result<()> {
        self.send_json_line(&notification).await
    }

    async fn send_json_line(&self, payload: &Value) -> Result<()> {
        let mut stdin = self.stdin.lock().await;
        let line = serde_json::to_string(payload)?;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn next_id(&self) -> u64 {
        let mut id = self.request_id.lock().await;
        let current = *id;
        *id += 1;
        current
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn spawn_stdout_router(
        server_name: String,
        stdout: tokio::process::ChildStdout,
        pending: PendingMap,
    ) {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        warn!("MCP server '{}' stdout closed", server_name);
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        let message: Value = match serde_json::from_str(trimmed) {
                            Ok(v) => v,
                            Err(e) => {
                                warn!(
                                    "MCP server '{}' emitted non-JSON line: {} ({})",
                                    server_name, trimmed, e
                                );
                                continue;
                            }
                        };

                        if let Some(id) = message.get("id").and_then(|v| v.as_u64()) {
                            let sender = { pending.lock().await.remove(&id) };
                            if let Some(tx) = sender {
                                let _ = tx.send(message);
                            } else {
                                debug!(
                                    "MCP server '{}' received response for unknown id {}",
                                    server_name, id
                                );
                            }
                        } else {
                            let method = message
                                .get("method")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            debug!(
                                "MCP server '{}' notification/event: method={}",
                                server_name, method
                            );
                        }
                    }
                    Err(e) => {
                        warn!("MCP server '{}' stdout read error: {}", server_name, e);
                        break;
                    }
                }
            }

            let disconnect_error = json!({
                "error": {
                    "code": -32000,
                    "message": format!("MCP server '{}' disconnected", server_name)
                }
            });
            let mut guard = pending.lock().await;
            for (_, tx) in guard.drain() {
                let _ = tx.send(disconnect_error.clone());
            }
        });
    }

    fn spawn_stderr_logger(server_name: String, stderr: tokio::process::ChildStderr) {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            warn!("MCP stderr [{}]: {}", server_name, trimmed);
                        }
                    }
                    Err(e) => {
                        warn!("MCP stderr read error [{}]: {}", server_name, e);
                        break;
                    }
                }
            }
        });
    }
}

pub struct McpClient {
    servers: Vec<Arc<McpServer>>,
}

impl McpClient {
    pub fn new() -> Self {
        Self {
            servers: Vec::new(),
        }
    }

    pub async fn add_server(
        &mut self,
        name: String,
        command: String,
        args: Vec<String>,
    ) -> Result<()> {
        let server = McpServer::start(name, command, args).await?;
        self.servers.push(Arc::new(server));
        Ok(())
    }

    pub async fn list_all_tools(&self) -> Vec<(String, Tool)> {
        let mut all_tools = Vec::new();

        for server in &self.servers {
            if let Ok(tools) = server.list_tools().await {
                for tool in tools {
                    all_tools.push((server.name().to_string(), tool));
                }
            }
        }

        all_tools
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolResult> {
        let server = self
            .servers
            .iter()
            .find(|s| s.name() == server_name)
            .ok_or_else(|| anyhow::anyhow!("Server not found: {}", server_name))?;

        server.call_tool(tool_name, arguments).await
    }

    pub async fn list_all_resources(&self) -> Vec<(String, Resource)> {
        let mut all_resources = Vec::new();

        for server in &self.servers {
            if let Ok(resources) = server.list_resources().await {
                for resource in resources {
                    all_resources.push((server.name().to_string(), resource));
                }
            }
        }

        all_resources
    }
}

impl Default for McpClient {
    fn default() -> Self {
        Self::new()
    }
}
