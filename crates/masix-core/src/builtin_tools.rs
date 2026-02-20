//! Built-in tools for Masix
//!
//! Tools that are always available to the LLM without MCP

use anyhow::{anyhow, Result};
use masix_exec::{run_command, ExecMode, ExecPolicy};
use masix_providers::ToolDefinition;
use serde_json::Value;
use std::path::Path;

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

            // Security: prevent path traversal
            if path.contains("..") || path.starts_with('/') {
                return Ok(
                    "Error: Invalid path. Only relative paths without '..' are allowed."
                        .to_string(),
                );
            }

            let full_path = workdir.join(path);

            match tokio::fs::read_to_string(&full_path).await {
                Ok(content) => {
                    // Truncate if too large
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

            // Security: prevent path traversal
            if path.contains("..") || path.starts_with('/') {
                return Ok(
                    "Error: Invalid path. Only relative paths without '..' are allowed."
                        .to_string(),
                );
            }

            let full_path = workdir.join(path);

            // Create parent directories if needed
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

            // Security: prevent path traversal
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
        _ => Err(anyhow!("Unknown builtin tool: {}", tool_name)),
    }
}

pub fn is_builtin_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "exec" | "termux" | "read_file" | "write_file" | "list_dir"
    )
}
