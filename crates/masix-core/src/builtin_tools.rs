//! Built-in tools for Masix
//!
//! Tools that are always available to the LLM without MCP

use anyhow::{anyhow, Result};
use masix_exec::{is_termux_environment, run_command, ExecMode, ExecPolicy};
use masix_intent::{execute_intent, IntentRequest};
use masix_providers::ToolDefinition;
use scraper::{Html, Selector};
use serde_json::Value;
use std::path::Path;

use crate::search::{MultiEngineSearch, TorrentSearch, MagnetExtractor};

const MAX_WEB_CONTENT: usize = 15000;

pub fn get_builtin_tool_definitions() -> Vec<ToolDefinition> {
    let mut tools = vec![
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "exec".to_string(),
                description: "Execute a shell command in the workdir. Only safe commands are allowed (pwd, ls, whoami, date, uname, uptime, df, du, free, head, tail, wc).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The command to execute (e.g., 'ls -la', 'pwd')"
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "termux".to_string(),
                description: "Execute a Termux-specific command (termux-info, termux-battery-status, termux-location, termux-wifi-connectioninfo, termux-telephony-deviceinfo, termux-clipboard-get).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The Termux command to execute (e.g., 'termux-battery-status')"
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "read_file".to_string(),
                description: "Read the contents of a file from the allowed directory.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path to the file (must be in workdir)"
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "write_file".to_string(),
                description: "Write content to a file in the allowed directory.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path to the file (must be in workdir)"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write to the file"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "list_dir".to_string(),
                description: "List contents of a directory.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path to the directory (optional, defaults to workdir)"
                        }
                    },
                    "required": []
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "web_search".to_string(),
                description: "Search the web using multiple engines (SearXNG, DuckDuckGo, Brave, Qwant). Returns aggregated results with titles, URLs, and snippets.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum number of results (default: 5, max: 10)"
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "torrent_search".to_string(),
                description: "Search torrent sites with automatic proxy/mirror fallback. Returns results with optional embedded magnets. Bypasses ISP DNS blocking.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Torrent search query (e.g. 'Debian 12.8 netinst')"
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum number of results (default: 8, max: 20)"
                        },
                        "providers": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Provider list. Use [\"all\"] (default) for full catalog, or names (e.g. [\"1337x\",\"nyaa\",\"thepiratebay\",\"yts\"])."
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "torrent_extract_magnet".to_string(),
                description: "Extract magnet URI from a torrent page URL. Uses cached results when available. Supports direct magnet URLs.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "Result page URL or direct magnet link"
                        }
                    },
                    "required": ["url"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "web_fetch".to_string(),
                description: "Fetch and extract text content from a web page. Returns cleaned text suitable for reading.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to fetch"
                        }
                    },
                    "required": ["url"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "device_info".to_string(),
                description: "Get device information (battery, network, storage, memory).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "cron".to_string(),
                description: "Manage reminders for the current chat/account. Commands: 'list', 'cancel <id>', or natural language schedule like 'domani alle 9 \"Meeting\"'.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Cron command body (without /cron prefix). Examples: 'list', 'cancel 12', 'domani alle 9 \"Meeting\"'"
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "vision".to_string(),
                description: "Return media metadata attached to the current inbound message (file id, mime type, caption). This does not perform OCR or image understanding.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".to_string(),
            function: masix_providers::FunctionDefinition {
                name: "intent".to_string(),
                description: "Dispatch Android intents via `am` (start/broadcast/service). Use for opening apps, deep links, or sending broadcast intents from Termux.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "mode": {
                            "type": "string",
                            "description": "Intent mode: start (default), broadcast, or service"
                        },
                        "action": {
                            "type": "string",
                            "description": "Intent action (e.g. android.intent.action.VIEW)"
                        },
                        "data": {
                            "type": "string",
                            "description": "Intent data URI (e.g. https://example.com)"
                        },
                        "package": {
                            "type": "string",
                            "description": "Package for explicit component target"
                        },
                        "class": {
                            "type": "string",
                            "description": "Class for explicit component target"
                        },
                        "extras_string": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "key": {"type": "string"},
                                    "value": {"type": "string"}
                                },
                                "required": ["key", "value"]
                            }
                        },
                        "extras_bool": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "key": {"type": "string"},
                                    "value": {"type": "boolean"}
                                },
                                "required": ["key", "value"]
                            }
                        },
                        "categories": {
                            "type": "array",
                            "items": {"type": "string"}
                        },
                        "flags": {
                            "type": "array",
                            "items": {"type": "string"}
                        },
                        "dry_run": {
                            "type": "boolean",
                            "description": "If true, return command without executing it"
                        }
                    },
                    "required": []
                }),
            },
        },
    ];

    if !is_termux_environment() {
        tools.retain(|tool| !matches!(tool.function.name.as_str(), "termux" | "intent"));
    }

    tools
}

pub async fn execute_builtin_tool(
    tool_name: &str,
    arguments: Value,
    exec_policy: &ExecPolicy,
    workdir: &Path,
) -> Result<String> {
    match tool_name {
        "exec" => {
            let command = arguments["command"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'command' argument"))?;

            if !exec_policy.enabled || !exec_policy.allow_base {
                return Ok("Exec commands are disabled. Enable in config with exec.enabled=true and exec.allow_base=true".to_string());
            }

            match run_command(exec_policy, ExecMode::Base, command, workdir).await {
                Ok(result) => Ok(result.format_for_chat()),
                Err(e) => Ok(format!("Error: {}", e)),
            }
        }
        "termux" => {
            if !is_termux_environment() {
                return Ok(
                    "Termux tool is unavailable on this platform (requires Android Termux)."
                        .to_string(),
                );
            }

            let command = arguments["command"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'command' argument"))?;

            if !exec_policy.enabled || !exec_policy.allow_termux {
                return Ok("Termux commands are disabled. Enable in config with exec.enabled=true and exec.allow_termux=true".to_string());
            }

            match run_command(exec_policy, ExecMode::Termux, command, workdir).await {
                Ok(result) => Ok(result.format_for_chat()),
                Err(e) => Ok(format!("Error: {}", e)),
            }
        }
        "read_file" => {
            let path = arguments["path"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'path' argument"))?;

            if path.contains("..") || path.starts_with('/') {
                return Ok(
                    "Error: Invalid path. Only relative paths without '..' are allowed."
                        .to_string(),
                );
            }

            let full_path = workdir.join(path);

            match tokio::fs::read_to_string(&full_path).await {
                Ok(content) => {
                    if content.len() > 10000 {
                        Ok(format!(
                            "{}\n\n... [truncated, {} bytes total]",
                            &content[..10000],
                            content.len()
                        ))
                    } else {
                        Ok(content)
                    }
                }
                Err(e) => Ok(format!("Error reading file: {}", e)),
            }
        }
        "write_file" => {
            let path = arguments["path"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'path' argument"))?;
            let content = arguments["content"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'content' argument"))?;

            if path.contains("..") || path.starts_with('/') {
                return Ok(
                    "Error: Invalid path. Only relative paths without '..' are allowed."
                        .to_string(),
                );
            }

            let full_path = workdir.join(path);

            if let Some(parent) = full_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return Ok(format!("Error creating directories: {}", e));
                }
            }

            match tokio::fs::write(&full_path, content).await {
                Ok(_) => Ok(format!(
                    "Successfully wrote {} bytes to {}",
                    content.len(),
                    path
                )),
                Err(e) => Ok(format!("Error writing file: {}", e)),
            }
        }
        "list_dir" => {
            let rel_path = arguments["path"].as_str().unwrap_or(".");

            if rel_path.contains("..") || rel_path.starts_with('/') {
                return Ok(
                    "Error: Invalid path. Only relative paths without '..' are allowed."
                        .to_string(),
                );
            }

            let full_path = workdir.join(rel_path);

            match tokio::fs::read_dir(&full_path).await {
                Ok(mut entries) => {
                    let mut result = Vec::new();
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let file_type =
                            if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                                "📁"
                            } else {
                                "📄"
                            };
                        result.push(format!("{} {}", file_type, name));
                    }
                    if result.is_empty() {
                        Ok("(empty directory)".to_string())
                    } else {
                        Ok(result.join("\n"))
                    }
                }
                Err(e) => Ok(format!("Error listing directory: {}", e)),
            }
        }
        "web_search" => {
            let query = arguments["query"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'query' argument"))?;
            let max_results = arguments["max_results"].as_u64().unwrap_or(5).min(10) as usize;

            match web_search_multi(query, max_results).await {
                Ok(results) => Ok(results),
                Err(e) => Ok(format!("Search error: {}", e)),
            }
        }
        "torrent_search" => {
            let query = arguments["query"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'query' argument"))?;
            let max_results = arguments["max_results"].as_u64().unwrap_or(8).min(20) as usize;
            let providers: Option<Vec<String>> = arguments.get("providers").and_then(|v| {
                v.as_array().map(|arr: &Vec<serde_json::Value>| {
                    arr.iter().filter_map(|item: &serde_json::Value| item.as_str().map(|s| s.to_string())).collect()
                })
            });

            match torrent_search_with_proxies(query, providers.as_deref(), max_results).await {
                Ok(results) => Ok(results),
                Err(e) => Ok(format!("Torrent search error: {}", e)),
            }
        }
        "torrent_extract_magnet" => {
            let url = arguments["url"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'url' argument"))?;

            match magnet_extract_cached(url).await {
                Ok(results) => Ok(results),
                Err(e) => Ok(format!("Torrent magnet extraction error: {}", e)),
            }
        }
        "web_fetch" => {
            let url = arguments["url"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'url' argument"))?;

            match web_fetch_page(url).await {
                Ok(content) => Ok(content),
                Err(e) => Ok(format!("Fetch error: {}", e)),
            }
        }
        "device_info" => get_device_info().await,
        "cron" => Ok(
            "Cron tool requires chat context and is executed by the runtime coordinator."
                .to_string(),
        ),
        "vision" => Ok(
            "Vision tool requires message media context and is executed by the runtime coordinator."
                .to_string(),
        ),
        "intent" => {
            if !is_termux_environment() {
                return Ok(
                    "Intent tool is unavailable on this platform (requires Android Termux)."
                        .to_string(),
                );
            }

            if !exec_policy.enabled || !exec_policy.allow_termux {
                return Ok("Intent tool is disabled. Enable in config with exec.enabled=true and exec.allow_termux=true".to_string());
            }

            let request: IntentRequest = serde_json::from_value(arguments)
                .map_err(|e| anyhow!("Invalid intent arguments: {}", e))?;
            match execute_intent(&request).await {
                Ok(result) => Ok(result),
                Err(e) => Ok(format!("Intent error: {}", e)),
            }
        }
        _ => Err(anyhow!("Unknown builtin tool: {}", tool_name)),
    }
}

pub fn is_builtin_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "exec"
            | "termux"
            | "read_file"
            | "write_file"
            | "list_dir"
            | "web_search"
            | "torrent_search"
            | "torrent_extract_magnet"
            | "web_fetch"
            | "device_info"
            | "cron"
            | "vision"
            | "intent"
    )
}

// ============================================================================
// Web Tools (using advanced search module)
// ============================================================================

async fn web_search_multi(query: &str, max_results: usize) -> Result<String> {
    let searcher = MultiEngineSearch::new()?;
    let results = searcher.search(query, max_results).await?;
    
    if results.is_empty() {
        return Ok("No results found.".to_string());
    }

    let mut chunks = Vec::new();
    chunks.push(format!("Found {} results (multi-engine):", results.len()));
    chunks.push(String::new());

    for item in results {
        chunks.push(format!(
            "**{}** [{}]\n{}\n🔗 {}\n",
            item.title,
            item.engine,
            if item.snippet.trim().is_empty() {
                "No description".to_string()
            } else {
                item.snippet.trim().to_string()
            },
            item.url.trim()
        ));
    }

    Ok(chunks.join("\n---\n"))
}

async fn torrent_search_with_proxies(
    query: &str,
    providers: Option<&[String]>,
    max_results: usize,
) -> Result<String> {
    let searcher = TorrentSearch::new()?;
    let results = searcher.search(query, providers, max_results).await?;

    if results.is_empty() {
        return Ok("No torrent results found. Try different providers or keywords.".to_string());
    }

    let mut lines = Vec::new();
    lines.push(format!("Found {} torrent results:", results.len()));
    lines.push(String::new());

    for (idx, entry) in results.iter().enumerate() {
        let title = if entry.title.trim().is_empty() {
            "Untitled".to_string()
        } else {
            entry.title.trim().to_string()
        };

        lines.push(format!("{}. **{}**", idx + 1, title));
        lines.push(format!("   provider: {}", entry.provider));
        
        if let Some(ref magnet) = entry.magnet {
            let short_magnet = if magnet.len() > 60 {
                format!("{}...", &magnet[..60])
            } else {
                magnet.clone()
            };
            lines.push(format!("   🧲 magnet: {}", short_magnet));
        }
        
        if let Some(ref size) = entry.size {
            lines.push(format!("   size: {}", size));
        }
        
        if let Some(seeds) = entry.seeds {
            lines.push(format!("   seeds: {}", seeds));
        }

        lines.push(String::new());
    }

    Ok(lines.join("\n"))
}

async fn magnet_extract_cached(url: &str) -> Result<String> {
    let url = url.trim();
    
    if url.is_empty() {
        return Ok("No URL provided.".to_string());
    }

    if url.starts_with("magnet:") {
        return Ok(format!("Magnet link:\n{}", url));
    }

    let extractor = MagnetExtractor::new()?;
    
    match extractor.extract(url).await {
        Ok(Some(magnet)) => {
            Ok(format!("Magnet found:\n{}", magnet))
        }
        Ok(None) => {
            Ok("No magnet URI found on that page. The site may require JavaScript or have Cloudflare protection.".to_string())
        }
        Err(e) => {
            Ok(format!("Failed to extract magnet: {}. The site may be blocked or require different access method.", e))
        }
    }
}

async fn web_fetch_page(url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(20))
        .build()?;

    let response = client.get(url).send().await?;
    let html = response.text().await?;

    let document = Html::parse_document(&html);

    let content_selectors = [
        "article", "main", ".content", "#content", ".post", ".article", "body",
    ];

    let mut text_content = String::new();

    for selector_str in &content_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            for element in document.select(&selector) {
                let mut el_text = element.text().collect::<String>();
                el_text = el_text.split_whitespace().collect::<Vec<_>>().join(" ");

                if el_text.len() > text_content.len() {
                    text_content = el_text;
                }
            }
            if !text_content.is_empty() {
                break;
            }
        }
    }

    if text_content.is_empty() {
        if let Ok(selector) = Selector::parse("body") {
            for element in document.select(&selector) {
                text_content = element.text().collect::<String>();
                text_content = text_content
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                break;
            }
        }
    }

    if text_content.len() > MAX_WEB_CONTENT {
        text_content = format!("{}... [truncated]", &text_content[..MAX_WEB_CONTENT]);
    }

    let title = if let Ok(selector) = Selector::parse("title") {
        document
            .select(&selector)
            .next()
            .map(|e| e.text().collect::<String>())
            .unwrap_or_default()
    } else {
        String::new()
    };

    Ok(format!(
        "**Title:** {}\n**URL:** {}\n\n{}",
        if title.is_empty() { "N/A" } else { &title },
        url,
        text_content
    ))
}

// ============================================================================
// Device Info
// ============================================================================

async fn get_device_info() -> Result<String> {
    let mut info = Vec::new();

    // Battery info (Termux)
    if masix_exec::is_termux_environment() {
        let policy = ExecPolicy {
            enabled: true,
            allow_termux: true,
            ..Default::default()
        };

        if let Ok(result) = run_command(
            &policy,
            ExecMode::Termux,
            "termux-battery-status",
            Path::new("."),
        )
        .await
        {
            if !result.stdout.is_empty() {
                info.push(format!("🔋 Battery:\n{}", result.stdout));
            }
        }
    }

    // Memory info
    if let Ok(meminfo) = tokio::fs::read_to_string("/proc/meminfo").await {
        let total: u64 = meminfo
            .lines()
            .find(|l| l.starts_with("MemTotal:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let available: u64 = meminfo
            .lines()
            .find(|l| l.starts_with("MemAvailable:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        if total > 0 {
            let used = total - available;
            info.push(format!(
                "💾 Memory: {:.1}GB / {:.1}GB used",
                used as f64 / 1_000_000.0,
                total as f64 / 1_000_000.0
            ));
        }
    }

    // Disk info
    if let Ok(df_output) = tokio::process::Command::new("df")
        .arg("-h")
        .arg("/")
        .output()
        .await
    {
        let output = String::from_utf8_lossy(&df_output.stdout);
        if let Some(line) = output.lines().nth(1) {
            info.push(format!("💿 Storage: {}", line));
        }
    }

    // Uptime
    if let Ok(uptime) = tokio::fs::read_to_string("/proc/uptime").await {
        if let Some(seconds) = uptime.split('.').next() {
            if let Ok(secs) = seconds.parse::<u64>() {
                let days = secs / 86400;
                let hours = (secs % 86400) / 3600;
                let mins = (secs % 3600) / 60;
                info.push(format!("⏱️ Uptime: {}d {}h {}m", days, hours, mins));
            }
        }
    }

    if info.is_empty() {
        Ok("Could not retrieve device information.".to_string())
    } else {
        Ok(info.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_torrent_search_available_providers() {
        let searcher = TorrentSearch::new().expect("TorrentSearch");
        let providers = searcher.get_providers();
        assert!(!providers.is_empty());
        assert!(providers.contains(&"1337x".to_string()));
        assert!(providers.contains(&"thepiratebay".to_string()));
        assert!(providers.contains(&"nyaa".to_string()));
    }

    #[tokio::test]
    async fn test_magnet_extractor_handles_direct_magnet() {
        let extractor = MagnetExtractor::new().expect("MagnetExtractor");
        let result = extractor.extract("magnet:?xt=urn:btih:ABCDEF123456").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("magnet:?xt=urn:btih:ABCDEF123456".to_string()));
    }

    #[tokio::test]
    async fn test_multi_engine_search_creates_client() {
        let searcher = MultiEngineSearch::new().expect("MultiEngineSearch");
        assert!(searcher.search("test query", 3).await.is_ok());
    }
}
