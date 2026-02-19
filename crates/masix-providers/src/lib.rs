//! Masix LLM Providers
//!
//! OpenAI-compatible API client with tool calling support

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub model: String,
    pub usage: Option<Usage>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat(&self, messages: Vec<ChatMessage>) -> Result<ChatResponse>;
    async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<ChatResponse>;
    async fn health_check(&self) -> Result<bool>;
}

pub struct OpenAICompatibleProvider {
    client: Client,
    name: String,
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAICompatibleProvider {
    pub fn new(
        name: String,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            name,
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            model: model.unwrap_or_else(|| "gpt-3.5-turbo".to_string()),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    fn truncate_for_error(text: &str, max_chars: usize) -> String {
        if text.chars().count() <= max_chars {
            text.to_string()
        } else {
            let truncated: String = text.chars().take(max_chars).collect();
            format!("{}...", truncated)
        }
    }

    async fn request_chat(&self, body: serde_json::Value) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let raw_body = response.text().await?;

        if !status.is_success() {
            let snippet = Self::truncate_for_error(&raw_body, 600);
            return Err(anyhow!("Provider HTTP {} at {}: {}", status, url, snippet));
        }

        let parsed: serde_json::Value = serde_json::from_str(&raw_body).map_err(|e| {
            anyhow!(
                "Provider response decode failed at {}: {} | body={}",
                url,
                e,
                Self::truncate_for_error(&raw_body, 600)
            )
        })?;

        self.parse_response(parsed)
    }

    fn parse_response(&self, response: serde_json::Value) -> Result<ChatResponse> {
        if let Some(error) = response.get("error") {
            return Err(anyhow!("API error: {:?}", error));
        }

        let choices = response
            .get("choices")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("Missing 'choices' array in provider response"))?;

        let choice = choices
            .first()
            .ok_or_else(|| anyhow!("Empty 'choices' array in provider response"))?;

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow!("Missing 'message' object in provider response"))?;

        let mut content = message
            .get("content")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut tool_calls = message.get("tool_calls").and_then(|tc| {
            serde_json::from_value::<Vec<ToolCall>>(tc.clone())
                .ok()
                .filter(|items| !items.is_empty())
        });

        // Compatibility fallback: some providers/models return textual pseudo-tool-calls
        // (e.g. "### TOOL_CALL ...") instead of OpenAI-native tool_calls.
        if tool_calls.is_none() {
            if let Some(raw_content) = content.as_deref() {
                let (cleaned_content, inferred_calls) =
                    Self::infer_tool_calls_from_content(raw_content);
                if !inferred_calls.is_empty() {
                    tracing::debug!(
                        provider = %self.name,
                        model = %self.model,
                        inferred_calls = inferred_calls.len(),
                        "Inferred tool calls from textual response"
                    );
                    tool_calls = Some(inferred_calls);
                    content = cleaned_content;
                }
            }
        }

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let usage = response.get("usage").map(|u| Usage {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        });

        let model = response
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.model)
            .to_string();

        Ok(ChatResponse {
            content,
            tool_calls,
            model,
            usage,
            finish_reason,
        })
    }

    fn infer_tool_calls_from_content(content: &str) -> (Option<String>, Vec<ToolCall>) {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return (None, Vec::new());
        }

        let mut calls = Vec::new();
        let mut consumed = vec![false; lines.len()];

        // Variant 1: explicit marker blocks
        let mut idx = 0;
        while idx < lines.len() {
            if lines[idx].trim().eq_ignore_ascii_case("### TOOL_CALL") {
                consumed[idx] = true;
                idx += 1;

                let block_start = idx;
                while idx < lines.len() && !lines[idx].trim().eq_ignore_ascii_case("### TOOL_CALL")
                {
                    idx += 1;
                }
                let block_end = idx;

                if block_end < lines.len() {
                    consumed[block_end] = true;
                    idx += 1;
                }

                for marker in consumed.iter_mut().take(block_end).skip(block_start) {
                    *marker = true;
                }

                let block = lines[block_start..block_end].join("\n");
                if let Some(call) = Self::parse_tool_call_block(&block, calls.len() + 1) {
                    calls.push(call);
                }
            } else {
                idx += 1;
            }
        }

        // Variant 2: plain `mcp.call ...` lines without markers
        idx = 0;
        while idx < lines.len() {
            if consumed[idx] {
                idx += 1;
                continue;
            }

            let line = lines[idx].trim();
            if !line.starts_with("mcp.call") {
                idx += 1;
                continue;
            }

            let start = idx;
            consumed[idx] = true;
            idx += 1;

            let mut brace_balance: i32 = 0;
            let mut saw_json = false;
            while idx < lines.len() {
                if consumed[idx] {
                    break;
                }
                let trimmed = lines[idx].trim();
                if trimmed.starts_with("mcp.call") || trimmed.eq_ignore_ascii_case("### TOOL_CALL")
                {
                    break;
                }

                if !trimmed.is_empty() {
                    let opens = trimmed.matches('{').count() as i32;
                    let closes = trimmed.matches('}').count() as i32;
                    if opens > 0 || closes > 0 || saw_json {
                        saw_json = true;
                        brace_balance += opens - closes;
                        consumed[idx] = true;
                        idx += 1;
                        if brace_balance <= 0 && saw_json {
                            break;
                        }
                        continue;
                    }
                }

                // Stop at blank/non-json line after command.
                break;
            }

            let end = idx;
            let block = lines[start..end].join("\n");
            if let Some(call) = Self::parse_tool_call_block(&block, calls.len() + 1) {
                calls.push(call);
            }
        }

        let cleaned = lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| if consumed[i] { None } else { Some(*line) })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();

        let cleaned_content = if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        };
        (cleaned_content, calls)
    }

    fn parse_tool_call_block(block: &str, idx: usize) -> Option<ToolCall> {
        let normalized_lines = block
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("```"))
            .collect::<Vec<_>>();

        if normalized_lines.is_empty() {
            return None;
        }

        let command_line = normalized_lines[0];
        let (server, tool) = Self::extract_server_and_tool(command_line)?;

        let args_from_lines = if normalized_lines.len() > 1 {
            normalized_lines[1..].join("\n")
        } else {
            String::new()
        };

        let arguments = Self::normalize_arguments(command_line, &args_from_lines)
            .unwrap_or_else(|| "{}".to_string());

        Some(ToolCall {
            id: format!("inferred_tool_call_{}", idx),
            tool_type: "function".to_string(),
            function: FunctionCall {
                name: format!("{}_{}", server, tool),
                arguments,
            },
        })
    }

    fn extract_server_and_tool(command_line: &str) -> Option<(String, String)> {
        if !command_line.contains("mcp.call") {
            return None;
        }

        let after_call = command_line.split_once("mcp.call")?.1.trim();
        let candidate_tokens = after_call
            .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == ':');

        for token in candidate_tokens {
            let cleaned = token.trim_matches(|c| c == '"' || c == '\'' || c == '`');
            if let Some((server, tool)) = cleaned.split_once('.') {
                if Self::is_identifier(server) && Self::is_identifier(tool) {
                    return Some((server.to_string(), tool.to_string()));
                }
            }
        }

        None
    }

    fn is_identifier(value: &str) -> bool {
        !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }

    fn normalize_arguments(command_line: &str, arguments_block: &str) -> Option<String> {
        if let Some(value) = Self::extract_json_value(arguments_block) {
            return Some(value.to_string());
        }

        if let Some(value) = Self::extract_json_value(command_line) {
            return Some(value.to_string());
        }

        Some("{}".to_string())
    }

    fn extract_json_value(input: &str) -> Option<serde_json::Value> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return Some(value);
        }

        let start = trimmed.find('{')?;
        let end = trimmed.rfind('}')?;
        if end <= start {
            return None;
        }

        let snippet = &trimmed[start..=end];
        serde_json::from_str::<serde_json::Value>(snippet).ok()
    }
}

#[async_trait::async_trait]
impl Provider for OpenAICompatibleProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(&self, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        self.request_chat(serde_json::json!({
            "model": self.model,
            "messages": messages
        }))
        .await
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<ChatResponse> {
        self.request_chat(serde_json::json!({
            "model": self.model,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto"
        }))
        .await
    }

    async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/models", self.base_url);
        match self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
        {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

pub struct ProviderRouter {
    providers: Vec<Box<dyn Provider>>,
    default_provider: String,
}

impl ProviderRouter {
    pub fn new(default_provider: String) -> Self {
        Self {
            providers: Vec::new(),
            default_provider,
        }
    }

    pub fn add_provider(&mut self, provider: Box<dyn Provider>) {
        self.providers.push(provider);
    }

    pub fn get_provider(&self, name: Option<&str>) -> Option<&dyn Provider> {
        let name = name.unwrap_or(&self.default_provider);
        self.providers
            .iter()
            .find(|p| p.name() == name)
            .map(|p| p.as_ref())
    }

    pub async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        provider: Option<&str>,
    ) -> Result<ChatResponse> {
        let provider = self
            .get_provider(provider)
            .ok_or_else(|| anyhow::anyhow!("Provider not found"))?;
        provider.chat(messages).await
    }

    pub async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        provider: Option<&str>,
    ) -> Result<ChatResponse> {
        let provider = self
            .get_provider(provider)
            .ok_or_else(|| anyhow::anyhow!("Provider not found"))?;
        provider.chat_with_tools(messages, tools).await
    }
}

#[cfg(test)]
mod tests {
    use super::OpenAICompatibleProvider;

    #[test]
    fn parse_response_errors_on_missing_choices() {
        let provider = OpenAICompatibleProvider::new(
            "test".to_string(),
            "key".to_string(),
            None,
            Some("model".to_string()),
        );

        let response = serde_json::json!({
            "id": "x"
        });

        let parsed = provider.parse_response(response);
        assert!(parsed.is_err());
    }

    #[test]
    fn parse_response_reads_content_and_model() {
        let provider = OpenAICompatibleProvider::new(
            "test".to_string(),
            "key".to_string(),
            None,
            Some("fallback-model".to_string()),
        );

        let response = serde_json::json!({
            "model": "real-model",
            "choices": [
                {
                    "message": { "content": "hello" },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 2,
                "total_tokens": 3
            }
        });

        let parsed = provider
            .parse_response(response)
            .expect("expected parse success");
        assert_eq!(parsed.content.as_deref(), Some("hello"));
        assert_eq!(parsed.model, "real-model");
        assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn parse_response_infers_tool_call_from_marker_block() {
        let provider = OpenAICompatibleProvider::new(
            "test".to_string(),
            "key".to_string(),
            None,
            Some("fallback-model".to_string()),
        );

        let response = serde_json::json!({
            "model": "real-model",
            "choices": [
                {
                    "message": {
                        "content": "### TOOL_CALL\nmcp.call webfetch.web_search\n{\"query\":\"masix\",\"num_results\":3}\n### TOOL_CALL"
                    },
                    "finish_reason": "stop"
                }
            ]
        });

        let parsed = provider
            .parse_response(response)
            .expect("expected parse success");
        let calls = parsed.tool_calls.expect("expected inferred tool call");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "webfetch_web_search");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&calls[0].function.arguments)
                .expect("valid arguments"),
            serde_json::json!({"query": "masix", "num_results": 3})
        );
        assert!(parsed.content.is_none());
    }

    #[test]
    fn parse_response_infers_tool_call_without_markers() {
        let provider = OpenAICompatibleProvider::new(
            "test".to_string(),
            "key".to_string(),
            None,
            Some("fallback-model".to_string()),
        );

        let response = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "content": "mcp.call webfetch.web_search\n{\"query\":\"nexuscore\"}"
                    },
                    "finish_reason": "stop"
                }
            ]
        });

        let parsed = provider
            .parse_response(response)
            .expect("expected parse success");
        let calls = parsed.tool_calls.expect("expected inferred tool call");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "webfetch_web_search");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&calls[0].function.arguments)
                .expect("valid arguments"),
            serde_json::json!({"query": "nexuscore"})
        );
    }

    #[test]
    fn parse_response_keeps_non_tool_text_when_infer_tool_call() {
        let provider = OpenAICompatibleProvider::new(
            "test".to_string(),
            "key".to_string(),
            None,
            Some("fallback-model".to_string()),
        );

        let response = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "content": "Eseguo una ricerca.\n### TOOL_CALL\nmcp.call webfetch.web_search\n{\"query\":\"termux\"}\n### TOOL_CALL\nAttendo il risultato."
                    }
                }
            ]
        });

        let parsed = provider
            .parse_response(response)
            .expect("expected parse success");
        let calls = parsed.tool_calls.expect("expected inferred tool call");
        assert_eq!(calls.len(), 1);
        let content = parsed.content.expect("cleaned text");
        assert!(content.contains("Eseguo una ricerca."));
        assert!(content.contains("Attendo il risultato."));
        assert!(!content.contains("### TOOL_CALL"));
    }
}
