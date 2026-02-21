//! Built-in tools for Masix
//!
//! Tools that are always available to the LLM without MCP

use anyhow::{anyhow, Result};
use masix_exec::{run_command, ExecMode, ExecPolicy};
use masix_intent::{execute_intent, IntentRequest};
use masix_providers::ToolDefinition;
use scraper::{Html, Selector};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

const MAX_WEB_CONTENT: usize = 15000;
const TORRENT_PROVIDER_CATALOG: &[(&str, &str)] = &[
    ("1337x", "1337x.to"),
    ("thepiratebay", "thepiratebay.org"),
    ("torrentgalaxy", "torrentgalaxy.to"),
    ("yts", "yts.mx"),
    ("limetorrents", "limetorrents.lol"),
    ("eztv", "eztv.re"),
    ("nyaa", "nyaa.si"),
    ("torlock", "torlock.com"),
    ("kickass", "kickasstorrents.to"),
    ("yourbittorrent", "yourbittorrent.com"),
    ("magnetdl", "magnetdl.com"),
    ("bt4g", "bt4gprx.com"),
    ("idope", "idope.se"),
    ("solidtorrents", "solidtorrents.to"),
];

#[derive(Debug, Clone)]
struct SearchResult {
    title: String,
    snippet: String,
    url: String,
}

#[derive(Debug, Clone)]
struct TorrentSearchEntry {
    provider: String,
    result: SearchResult,
    magnet: Option<String>,
}

pub fn get_builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
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
                description: "Search the web using DuckDuckGo. Returns search results with titles, URLs, and snippets.".to_string(),
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
                description: "Search torrent-related results on the web and optionally extract magnet links from result pages.".to_string(),
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
                        "include_magnet": {
                            "type": "boolean",
                            "description": "If true, try extracting magnet links from result pages (default: true)"
                        },
                        "providers": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Provider list. Use [\"all\"] (default) for full catalog, or names/domains (e.g. [\"1337x\",\"nyaa\"])."
                        }
                    },
                    "required": ["query"]
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
    ]
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

            match web_search_duckduckgo(query, max_results).await {
                Ok(results) => Ok(results),
                Err(e) => Ok(format!("Search error: {}", e)),
            }
        }
        "torrent_search" => {
            let query = arguments["query"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing 'query' argument"))?;
            let max_results = arguments["max_results"].as_u64().unwrap_or(8).min(20) as usize;
            let include_magnet = arguments["include_magnet"].as_bool().unwrap_or(true);
            let providers = arguments.get("providers");

            match torrent_search(query, max_results, include_magnet, providers).await {
                Ok(results) => Ok(results),
                Err(e) => Ok(format!("Torrent search error: {}", e)),
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
            | "web_fetch"
            | "device_info"
            | "cron"
            | "vision"
            | "intent"
    )
}

// ============================================================================
// Web Tools
// ============================================================================

async fn web_search_duckduckgo(query: &str, max_results: usize) -> Result<String> {
    let results = web_search_duckduckgo_results(query, max_results).await?;
    Ok(format_search_results(
        &results,
        &format!("Found {} results:", results.len()),
    ))
}

async fn web_search_duckduckgo_results(
    query: &str,
    max_results: usize,
) -> Result<Vec<SearchResult>> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let encoded_query: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);

    let response = client.get(&url).send().await?;
    let html = response.text().await?;

    let document = Html::parse_document(&html);
    let result_selector = Selector::parse(".result").ok();
    let title_selector = Selector::parse(".result__a").ok();
    let snippet_selector = Selector::parse(".result__snippet").ok();

    let mut results: Vec<SearchResult> = Vec::new();

    if let (Some(result_sel), Some(title_sel), Some(snippet_sel)) =
        (result_selector, title_selector, snippet_selector)
    {
        for result in document.select(&result_sel).take(max_results) {
            let title = result
                .select(&title_sel)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default()
                .trim()
                .to_string();

            let snippet = result
                .select(&snippet_sel)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default()
                .trim()
                .to_string();

            // Get link from href attribute
            let raw_link = result
                .select(&title_sel)
                .next()
                .and_then(|e| e.value().attr("href"))
                .unwrap_or("")
                .to_string();
            let link = normalize_search_link(&raw_link);

            if !title.is_empty() {
                results.push(SearchResult {
                    title,
                    snippet,
                    url: link,
                });
            }
        }
    }

    if results.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(results)
    }
}

fn format_search_results(results: &[SearchResult], header: &str) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }

    let mut chunks = Vec::new();
    for item in results {
        chunks.push(format!(
            "**{}**\n{}\n🔗 {}\n",
            item.title,
            if item.snippet.trim().is_empty() {
                "No description".to_string()
            } else {
                item.snippet.trim().to_string()
            },
            if item.url.trim().is_empty() {
                "N/A".to_string()
            } else {
                item.url.trim().to_string()
            }
        ));
    }

    format!("{}\n\n{}", header, chunks.join("\n---\n"))
}

fn normalize_search_link(raw_link: &str) -> String {
    if raw_link.trim().is_empty() {
        return String::new();
    }

    if let Ok(parsed) = url::Url::parse(raw_link) {
        for (key, value) in parsed.query_pairs() {
            if key == "uddg" {
                return value.into_owned();
            }
        }
        return raw_link.to_string();
    }

    if raw_link.starts_with('/') {
        if let Ok(base) = url::Url::parse("https://duckduckgo.com") {
            if let Ok(joined) = base.join(raw_link) {
                for (key, value) in joined.query_pairs() {
                    if key == "uddg" {
                        return value.into_owned();
                    }
                }
            }
        }
    }

    raw_link.to_string()
}

async fn torrent_search(
    query: &str,
    max_results: usize,
    include_magnet: bool,
    providers_value: Option<&Value>,
) -> Result<String> {
    let providers = resolve_torrent_providers(providers_value);
    if providers.is_empty() {
        return Ok("No torrent providers available.".to_string());
    }

    let magnet_client = if include_magnet {
        Some(
            reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36")
                .timeout(std::time::Duration::from_secs(12))
                .build()?,
        )
    } else {
        None
    };

    let mut entries: Vec<TorrentSearchEntry> = Vec::new();
    let mut seen_urls: HashSet<String> = HashSet::new();
    let per_provider_limit = ((max_results / providers.len()).max(1)).min(4);
    let enhanced_query = if query.to_ascii_lowercase().contains("torrent") {
        query.to_string()
    } else {
        format!("{} torrent", query)
    };

    for (provider_label, provider_domain) in &providers {
        if entries.len() >= max_results {
            break;
        }

        let provider_query = format!("site:{} {}", provider_domain, enhanced_query);
        let fetched = web_search_duckduckgo_results(&provider_query, per_provider_limit).await;
        let provider_results = match fetched {
            Ok(items) => items,
            Err(_) => continue,
        };

        for result in provider_results {
            if entries.len() >= max_results {
                break;
            }
            if result.url.trim().is_empty() {
                continue;
            }

            let key = result.url.trim().to_ascii_lowercase();
            if !seen_urls.insert(key) {
                continue;
            }

            let magnet = if include_magnet {
                if let Some(client) = &magnet_client {
                    fetch_first_magnet_link(client, &result.url)
                        .await
                        .ok()
                        .flatten()
                } else {
                    None
                }
            } else {
                None
            };

            let provider =
                detect_provider_label(&result.url).unwrap_or_else(|| provider_label.clone());
            entries.push(TorrentSearchEntry {
                provider,
                result,
                magnet,
            });
        }
    }

    if entries.is_empty() {
        let fallback_results = web_search_duckduckgo_results(&enhanced_query, max_results).await?;
        for result in fallback_results {
            if entries.len() >= max_results {
                break;
            }
            if result.url.trim().is_empty() {
                continue;
            }
            let key = result.url.trim().to_ascii_lowercase();
            if !seen_urls.insert(key) {
                continue;
            }
            let magnet = if include_magnet {
                if let Some(client) = &magnet_client {
                    fetch_first_magnet_link(client, &result.url)
                        .await
                        .ok()
                        .flatten()
                } else {
                    None
                }
            } else {
                None
            };
            let provider =
                detect_provider_label(&result.url).unwrap_or_else(|| "generic".to_string());
            entries.push(TorrentSearchEntry {
                provider,
                result,
                magnet,
            });
        }
    }

    if entries.is_empty() {
        return Ok("No torrent results found.".to_string());
    }

    Ok(format_torrent_entries(&entries))
}

fn resolve_torrent_providers(providers_value: Option<&Value>) -> Vec<(String, String)> {
    let mut requested: Vec<String> = Vec::new();

    if let Some(value) = providers_value {
        if let Some(items) = value.as_array() {
            for item in items {
                if let Some(s) = item.as_str() {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        requested.push(trimmed.to_ascii_lowercase());
                    }
                }
            }
        } else if let Some(s) = value.as_str() {
            for token in s.split(',') {
                let trimmed = token.trim();
                if !trimmed.is_empty() {
                    requested.push(trimmed.to_ascii_lowercase());
                }
            }
        }
    }

    let use_all = requested.is_empty() || requested.iter().any(|item| item == "all");
    let mut providers: Vec<(String, String)> = Vec::new();

    if use_all {
        for (name, domain) in TORRENT_PROVIDER_CATALOG {
            providers.push(((*name).to_string(), (*domain).to_string()));
        }
        return providers;
    }

    for requested_item in requested {
        if let Some((name, domain)) = TORRENT_PROVIDER_CATALOG
            .iter()
            .find(|(name, _)| name.to_ascii_lowercase() == requested_item)
        {
            providers.push(((*name).to_string(), (*domain).to_string()));
            continue;
        }
        if requested_item.contains('.') {
            providers.push((requested_item.clone(), requested_item));
        }
    }

    let mut seen = HashSet::new();
    providers
        .into_iter()
        .filter(|(_, domain)| seen.insert(domain.clone()))
        .collect()
}

fn detect_provider_label(url: &str) -> Option<String> {
    let host = extract_host(url)?;
    for (name, domain) in TORRENT_PROVIDER_CATALOG {
        if host == *domain || host.ends_with(&format!(".{}", domain)) {
            return Some((*name).to_string());
        }
    }
    Some(host)
}

fn extract_host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    parsed.host_str().map(|host| host.to_ascii_lowercase())
}

fn format_torrent_entries(entries: &[TorrentSearchEntry]) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Found {} torrent links:", entries.len()));
    lines.push(String::new());

    for (idx, entry) in entries.iter().enumerate() {
        let title = if entry.result.title.trim().is_empty() {
            "Untitled result".to_string()
        } else {
            entry.result.title.trim().to_string()
        };
        lines.push(format!(
            "{}. [{}]({})",
            idx + 1,
            title,
            entry.result.url.trim()
        ));
        lines.push(format!("   provider: {}", entry.provider));
        if !entry.result.snippet.trim().is_empty() {
            lines.push(format!("   snippet: {}", entry.result.snippet.trim()));
        }
        if let Some(magnet) = &entry.magnet {
            lines.push(format!("   magnet: {}", magnet));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

async fn fetch_first_magnet_link(
    client: &reqwest::Client,
    page_url: &str,
) -> Result<Option<String>> {
    if page_url.trim().is_empty() {
        return Ok(None);
    }
    if !page_url.starts_with("http://") && !page_url.starts_with("https://") {
        return Ok(None);
    }

    let response = client.get(page_url).send().await?;
    let html = response.text().await?;
    Ok(extract_first_magnet_link(&html))
}

fn extract_first_magnet_link(html: &str) -> Option<String> {
    let marker = "magnet:?";
    let start = html.find(marker)?;
    let tail = &html[start..];
    let end = tail
        .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>')
        .unwrap_or(tail.len());
    if end == 0 {
        return None;
    }
    let link = tail[..end].replace("&amp;", "&");
    if link.starts_with("magnet:?xt=urn:btih:") {
        Some(link)
    } else {
        None
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

    // Extract text from main content areas
    let content_selectors = [
        "article", "main", ".content", "#content", ".post", ".article", "body",
    ];

    let mut text_content = String::new();

    for selector_str in &content_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            for element in document.select(&selector) {
                let mut el_text = element.text().collect::<String>();

                // Clean up whitespace
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
        // Fallback: get all text from body
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

    // Truncate if too large
    if text_content.len() > MAX_WEB_CONTENT {
        text_content = format!("{}... [truncated]", &text_content[..MAX_WEB_CONTENT]);
    }

    // Try to get title
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
