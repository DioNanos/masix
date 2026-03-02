//! Masix LLM Providers
//!
//! OpenAI-compatible API client with tool calling support
//! Plus native Anthropic/Claude provider

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use reqwest::header::HeaderMap;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::time::sleep;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub window_secs: u64,
    pub initial_delay_secs: u64,
    pub backoff_factor: u32,
    pub max_delay_secs: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            window_secs: 600,
            initial_delay_secs: 2,
            backoff_factor: 2,
            max_delay_secs: 30,
        }
    }
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse>;
    async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse>;
    async fn chat_with_model(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        let _ = model_override;
        self.chat(messages, retry_policy).await
    }
    async fn chat_with_tools_and_model(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        model_override: Option<&str>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        let _ = model_override;
        self.chat_with_tools(messages, tools, retry_policy).await
    }
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

    async fn request_chat(
        &self,
        body: serde_json::Value,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let policy = retry_policy.cloned().unwrap_or_default();
        let start = Instant::now();
        let mut attempt: u32 = 1;

        loop {
            let response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match response {
                Ok(response) => {
                    let status = response.status();
                    let headers = response.headers().clone();
                    let raw_body = response.text().await?;

                    if status.is_success() {
                        let parsed: serde_json::Value =
                            serde_json::from_str(&raw_body).map_err(|e| {
                                anyhow!(
                                    "Provider response decode failed at {}: {} | body={}",
                                    url,
                                    e,
                                    Self::truncate_for_error(&raw_body, 600)
                                )
                            })?;
                        return self.parse_response(parsed);
                    }

                    let snippet = Self::truncate_for_error(&raw_body, 600);
                    let error_msg = format!("Provider HTTP {} at {}: {}", status, url, snippet);
                    if !Self::is_retryable_status(status.as_u16()) {
                        return Err(anyhow!(error_msg));
                    }

                    if let Some(delay) =
                        Self::next_retry_delay(&policy, attempt, &headers, start.elapsed())
                    {
                        tracing::warn!(
                            provider = %self.name,
                            status = %status.as_u16(),
                            attempt = attempt,
                            delay_ms = delay.as_millis(),
                            "Retrying provider request after transient HTTP error"
                        );
                        sleep(delay).await;
                        attempt += 1;
                        continue;
                    }

                    return Err(anyhow!(error_msg));
                }
                Err(err) => {
                    if !Self::is_retryable_reqwest(&err) {
                        return Err(err.into());
                    }

                    if let Some(delay) =
                        Self::next_retry_delay(&policy, attempt, &HeaderMap::new(), start.elapsed())
                    {
                        tracing::warn!(
                            provider = %self.name,
                            attempt = attempt,
                            delay_ms = delay.as_millis(),
                            error = %err,
                            "Retrying provider request after transient network error"
                        );
                        sleep(delay).await;
                        attempt += 1;
                        continue;
                    }

                    return Err(err.into());
                }
            }
        }
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

        // Variant 2: plain textual tool-call lines without markers
        idx = 0;
        while idx < lines.len() {
            if consumed[idx] {
                idx += 1;
                continue;
            }

            let line = lines[idx].trim();
            if !Self::is_textual_tool_call_line(line) {
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
                if Self::is_textual_tool_call_line(trimmed)
                    || trimmed.eq_ignore_ascii_case("### TOOL_CALL")
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
        let function_name = Self::extract_function_name(command_line)?;

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
                name: function_name,
                arguments,
            },
        })
    }

    fn is_textual_tool_call_line(line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        lower.starts_with("mcp.call")
            || lower.starts_with("call ")
            || lower.starts_with("tool.call ")
            || lower.starts_with("tool_call ")
            || lower.starts_with("function.call ")
            || lower.starts_with("function_call ")
    }

    fn extract_function_name(command_line: &str) -> Option<String> {
        if let Some((server, tool)) = Self::extract_server_and_tool(command_line) {
            return Some(format!("{}_{}", server, tool));
        }

        Self::extract_prefixed_function_name(command_line)
    }

    fn extract_prefixed_function_name(command_line: &str) -> Option<String> {
        let trimmed = command_line.trim();
        let lower = trimmed.to_ascii_lowercase();
        let rest = if lower.starts_with("call ") {
            trimmed.get(5..)?.trim()
        } else if lower.starts_with("tool.call ") || lower.starts_with("tool_call ") {
            trimmed.get(10..)?.trim()
        } else if lower.starts_with("function.call ") || lower.starts_with("function_call ") {
            trimmed.get(14..)?.trim()
        } else {
            return None;
        };

        let token = rest
            .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == ':')
            .find(|token| !token.is_empty())?;
        let cleaned = token.trim_matches(|c| c == '"' || c == '\'' || c == '`');

        if let Some((server, tool)) = cleaned.split_once('.') {
            if Self::is_identifier(server) && Self::is_identifier(tool) {
                return Some(format!("{}_{}", server, tool));
            }
        }

        if Self::is_identifier(cleaned) {
            return Some(cleaned.to_string());
        }

        None
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

    fn is_retryable_status(status: u16) -> bool {
        matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
    }

    fn is_retryable_reqwest(err: &reqwest::Error) -> bool {
        err.is_timeout() || err.is_connect() || err.is_request()
    }

    fn next_retry_delay(
        policy: &RetryPolicy,
        attempt: u32,
        headers: &HeaderMap,
        elapsed: Duration,
    ) -> Option<Duration> {
        let window = Duration::from_secs(policy.window_secs.max(1));
        if elapsed >= window {
            return None;
        }

        let header_delay = Self::parse_retry_after_headers(headers);
        let fallback_delay = Self::exponential_delay(policy, attempt);
        let mut delay = header_delay.unwrap_or(fallback_delay);

        let remaining = window.saturating_sub(elapsed);
        if remaining.is_zero() {
            return None;
        }
        if delay > remaining {
            delay = remaining;
        }

        if delay.is_zero() {
            Some(Duration::from_millis(1))
        } else {
            Some(delay)
        }
    }

    fn exponential_delay(policy: &RetryPolicy, attempt: u32) -> Duration {
        let initial = policy.initial_delay_secs.max(1);
        let factor = policy.backoff_factor.max(1) as u64;
        let max_delay = policy.max_delay_secs.max(1);

        let exponent = attempt.saturating_sub(1).min(20);
        let multiplier = factor.saturating_pow(exponent);
        let secs = initial.saturating_mul(multiplier).min(max_delay);
        Duration::from_secs(secs)
    }

    fn parse_retry_after_headers(headers: &HeaderMap) -> Option<Duration> {
        if let Some(v) = headers.get("retry-after-ms").and_then(|h| h.to_str().ok()) {
            if let Ok(ms) = v.trim().parse::<u64>() {
                if ms > 0 {
                    return Some(Duration::from_millis(ms));
                }
            }
        }

        if let Some(v) = headers.get("retry-after").and_then(|h| h.to_str().ok()) {
            let trimmed = v.trim();
            if let Ok(secs) = trimmed.parse::<u64>() {
                if secs > 0 {
                    return Some(Duration::from_secs(secs));
                }
            }

            if let Ok(http_date) = DateTime::parse_from_rfc2822(trimmed) {
                let now = Utc::now();
                let target = http_date.with_timezone(&Utc);
                if target > now {
                    let millis = (target - now).num_milliseconds();
                    if millis > 0 {
                        return Some(Duration::from_millis(millis as u64));
                    }
                }
            }
        }

        None
    }
}

#[async_trait::async_trait]
impl Provider for OpenAICompatibleProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        self.chat_with_model(messages, None, retry_policy).await
    }

    async fn chat_with_model(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        self.request_chat(
            serde_json::json!({
                "model": model_override.unwrap_or(&self.model),
                "messages": messages
            }),
            retry_policy,
        )
        .await
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        self.chat_with_tools_and_model(messages, tools, None, retry_policy)
            .await
    }

    async fn chat_with_tools_and_model(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        model_override: Option<&str>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        self.request_chat(
            serde_json::json!({
                "model": model_override.unwrap_or(&self.model),
                "messages": messages,
                "tools": tools,
                "tool_choice": "auto"
            }),
            retry_policy,
        )
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

pub struct AnthropicProvider {
    client: Client,
    name: String,
    api_key: String,
    base_url: String,
    model: String,
}

impl AnthropicProvider {
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
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            model: model.unwrap_or_else(|| "claude-3-5-sonnet-latest".to_string()),
        }
    }

    fn convert_messages_to_anthropic(
        messages: &[ChatMessage],
    ) -> (Option<String>, Vec<serde_json::Value>) {
        let mut system_prompt: Option<String> = None;
        let mut anthropic_messages: Vec<serde_json::Value> = Vec::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    system_prompt = msg.content.clone();
                }
                "user" | "assistant" => {
                    let mut content_blocks: Vec<serde_json::Value> = Vec::new();

                    if let Some(text) = &msg.content {
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text
                        }));
                    }

                    if let Some(tool_calls) = &msg.tool_calls {
                        for tc in tool_calls {
                            content_blocks.push(serde_json::json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.function.name,
                                "input": serde_json::from_str::<serde_json::Value>(&tc.function.arguments).unwrap_or(serde_json::json!({}))
                            }));
                        }
                    }

                    if !content_blocks.is_empty() {
                        anthropic_messages.push(serde_json::json!({
                            "role": msg.role,
                            "content": content_blocks
                        }));
                    }
                }
                "tool" => {
                    if let (Some(tool_id), Some(content)) = (&msg.tool_call_id, &msg.content) {
                        anthropic_messages.push(serde_json::json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": tool_id,
                                "content": content
                            }]
                        }));
                    }
                }
                _ => {}
            }
        }

        (system_prompt, anthropic_messages)
    }

    fn convert_tools_to_anthropic(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "input_schema": t.function.parameters
                })
            })
            .collect()
    }

    async fn request_anthropic(
        &self,
        body: serde_json::Value,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/v1/messages", self.base_url);
        let policy = retry_policy.cloned().unwrap_or_default();
        let start = Instant::now();
        let mut attempt: u32 = 1;

        loop {
            let response = self
                .client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match response {
                Ok(response) => {
                    let status = response.status();
                    let headers = response.headers().clone();
                    let raw_body = response.text().await?;

                    if status.is_success() {
                        let parsed: serde_json::Value =
                            serde_json::from_str(&raw_body).map_err(|e| {
                                anyhow!(
                                    "Anthropic response decode failed: {} | body={}",
                                    e,
                                    &raw_body.chars().take(600).collect::<String>()
                                )
                            })?;
                        return self.parse_anthropic_response(parsed);
                    }

                    let snippet = &raw_body.chars().take(600).collect::<String>();
                    let error_msg = format!("Anthropic HTTP {} at {}: {}", status, url, snippet);

                    if !OpenAICompatibleProvider::is_retryable_status(status.as_u16()) {
                        return Err(anyhow!(error_msg));
                    }

                    if let Some(delay) = OpenAICompatibleProvider::next_retry_delay(
                        &policy,
                        attempt,
                        &headers,
                        start.elapsed(),
                    ) {
                        tracing::warn!(
                            provider = %self.name,
                            status = %status.as_u16(),
                            attempt = attempt,
                            "Retrying Anthropic request"
                        );
                        sleep(delay).await;
                        attempt += 1;
                        continue;
                    }

                    return Err(anyhow!(error_msg));
                }
                Err(err) => {
                    if !OpenAICompatibleProvider::is_retryable_reqwest(&err) {
                        return Err(err.into());
                    }

                    if let Some(delay) = OpenAICompatibleProvider::next_retry_delay(
                        &policy,
                        attempt,
                        &HeaderMap::new(),
                        start.elapsed(),
                    ) {
                        tracing::warn!(
                            provider = %self.name,
                            attempt = attempt,
                            error = %err,
                            "Retrying Anthropic request after network error"
                        );
                        sleep(delay).await;
                        attempt += 1;
                        continue;
                    }

                    return Err(err.into());
                }
            }
        }
    }

    fn parse_anthropic_response(&self, response: serde_json::Value) -> Result<ChatResponse> {
        if let Some(error) = response.get("error") {
            return Err(anyhow!("Anthropic API error: {:?}", error));
        }

        let content_blocks = response
            .get("content")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("Missing 'content' array in Anthropic response"))?;

        let mut text_content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for block in content_blocks {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        if !text_content.is_empty() {
                            text_content.push('\n');
                        }
                        text_content.push_str(text);
                    }
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = block.get("input").cloned().unwrap_or(serde_json::json!({}));

                    tool_calls.push(ToolCall {
                        id,
                        tool_type: "function".to_string(),
                        function: FunctionCall {
                            name,
                            arguments: serde_json::to_string(&input)
                                .unwrap_or_else(|_| "{}".to_string()),
                        },
                    });
                }
                _ => {}
            }
        }

        let stop_reason = response
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let usage = response.get("usage").map(|u| Usage {
            prompt_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            total_tokens: 0,
        });

        let model = response
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.model)
            .to_string();

        Ok(ChatResponse {
            content: if text_content.is_empty() {
                None
            } else {
                Some(text_content)
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            model,
            usage,
            finish_reason: stop_reason,
        })
    }
}

#[async_trait::async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        self.chat_with_model(messages, None, retry_policy).await
    }

    async fn chat_with_model(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        let (system, anthropic_messages) = Self::convert_messages_to_anthropic(&messages);

        let mut body = serde_json::json!({
            "model": model_override.unwrap_or(&self.model),
            "max_tokens": 4096,
            "messages": anthropic_messages
        });

        if let Some(sys) = system {
            body["system"] = serde_json::json!(sys);
        }

        self.request_anthropic(body, retry_policy).await
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        self.chat_with_tools_and_model(messages, tools, None, retry_policy)
            .await
    }

    async fn chat_with_tools_and_model(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        model_override: Option<&str>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        let (system, anthropic_messages) = Self::convert_messages_to_anthropic(&messages);
        let anthropic_tools = Self::convert_tools_to_anthropic(&tools);

        let mut body = serde_json::json!({
            "model": model_override.unwrap_or(&self.model),
            "max_tokens": 4096,
            "messages": anthropic_messages,
            "tools": anthropic_tools
        });

        if let Some(sys) = system {
            body["system"] = serde_json::json!(sys);
        }

        self.request_anthropic(body, retry_policy).await
    }

    async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));
        match self
            .client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
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
        model_override: Option<&str>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        let provider = self
            .get_provider(provider)
            .ok_or_else(|| anyhow::anyhow!("Provider not found"))?;
        provider
            .chat_with_model(messages, model_override, retry_policy)
            .await
    }

    pub async fn chat_with_tools(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
        provider: Option<&str>,
        model_override: Option<&str>,
        retry_policy: Option<&RetryPolicy>,
    ) -> Result<ChatResponse> {
        let provider = self
            .get_provider(provider)
            .ok_or_else(|| anyhow::anyhow!("Provider not found"))?;
        provider
            .chat_with_tools_and_model(messages, tools, model_override, retry_policy)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenAICompatibleProvider, RetryPolicy};
    use reqwest::header::{HeaderMap, HeaderValue};
    use std::time::Duration;

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

    #[test]
    fn parse_response_infers_builtin_tool_call_from_marker_block() {
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
                        "content": "### TOOL_CALL\ncall exec\n{\"command\":\"pwd\"}\n### TOOL_CALL"
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
        assert_eq!(calls[0].function.name, "exec");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&calls[0].function.arguments)
                .expect("valid arguments"),
            serde_json::json!({"command": "pwd"})
        );
    }

    #[test]
    fn parse_response_infers_call_prefix_for_flattened_mcp_tool() {
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
                        "content": "tool.call webfetch.web_search\n{\"query\":\"masix\"}"
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
            serde_json::json!({"query": "masix"})
        );
    }

    #[test]
    fn retry_header_precedence_prefers_retry_after_ms() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after-ms", HeaderValue::from_static("1500"));
        headers.insert("retry-after", HeaderValue::from_static("99"));

        let delay =
            OpenAICompatibleProvider::parse_retry_after_headers(&headers).expect("expected delay");
        assert_eq!(delay, Duration::from_millis(1500));
    }

    #[test]
    fn retry_fallback_sequence_has_exponential_cap() {
        let policy = RetryPolicy::default();
        assert_eq!(
            OpenAICompatibleProvider::exponential_delay(&policy, 1),
            Duration::from_secs(2)
        );
        assert_eq!(
            OpenAICompatibleProvider::exponential_delay(&policy, 2),
            Duration::from_secs(4)
        );
        assert_eq!(
            OpenAICompatibleProvider::exponential_delay(&policy, 5),
            Duration::from_secs(30)
        );
        assert_eq!(
            OpenAICompatibleProvider::exponential_delay(&policy, 6),
            Duration::from_secs(30)
        );
    }
}
