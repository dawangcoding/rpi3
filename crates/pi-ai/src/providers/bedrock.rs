//! Amazon Bedrock Provider 实现

use std::pin::Pin;

use anyhow::{Context as AnyhowContext, Result};
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_bedrockruntime::primitives::Blob;
use aws_sdk_bedrockruntime::types::ResponseStream;
use futures::Stream;
use serde_json::json;

use crate::api_registry::ApiProvider;
use crate::models::get_api_key_from_env;
use crate::types::*;
use crate::utils::json_parse::parse_partial_json;

/// Amazon Bedrock API 提供者
///
/// 支持通过 AWS SDK 调用 Bedrock 托管的模型
pub struct BedrockProvider;

impl BedrockProvider {
    /// 创建新的 Bedrock 提供者实例
    pub fn new() -> Self {
        Self
    }

    fn parse_region(&self, base_url: &str) -> String {
        if let Some(stripped) = base_url.strip_prefix("bedrock://") {
            return stripped.to_string();
        }
        std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string())
    }

    fn build_request_body(&self, context: &Context, model: &Model, options: &StreamOptions) -> serde_json::Value {
        let messages = self.convert_messages(&context.messages, model);
        let max_tokens = options.max_tokens.unwrap_or(model.max_tokens / 3);

        let mut body = json!({
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": max_tokens,
            "messages": messages,
        });

        if let Some(ref prompt) = context.system_prompt {
            body["system"] = json!(prompt);
        }

        if let Some(temp) = options.temperature {
            if !model.reasoning {
                body["temperature"] = json!(temp);
            }
        }

        if let Some(ref tools) = context.tools {
            body["tools"] = json!(self.convert_tools(tools));
        }

        if model.reasoning {
            body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": 1024,
            });
        }

        body
    }

    fn convert_messages(&self, messages: &[Message], model: &Model) -> Vec<serde_json::Value> {
        let mut result = Vec::new();
        let mut i = 0;

        while i < messages.len() {
            match &messages[i] {
                Message::User(msg) => {
                    let content = match &msg.content {
                        UserContent::Text(text) => {
                            if text.trim().is_empty() {
                                i += 1;
                                continue;
                            }
                            json!(text.trim())
                        }
                        UserContent::Blocks(blocks) => {
                            let anthropic_blocks: Vec<_> = blocks
                                .iter()
                                .filter_map(|block| match block {
                                    ContentBlock::Text(t) => {
                                        if t.text.trim().is_empty() {
                                            None
                                        } else {
                                            Some(json!({"type": "text", "text": t.text}))
                                        }
                                    }
                                    ContentBlock::Image(img) => Some(json!({
                                        "type": "image",
                                        "source": {
                                            "type": "base64",
                                            "media_type": img.mime_type,
                                            "data": img.data,
                                        },
                                    })),
                                    _ => None,
                                })
                                .collect();

                            if anthropic_blocks.is_empty() {
                                i += 1;
                                continue;
                            }

                            let filtered: Vec<_> = if !model.input.contains(&InputModality::Image) {
                                anthropic_blocks.into_iter().filter(|b| b.get("type") != Some(&json!("image"))).collect()
                            } else {
                                anthropic_blocks
                            };

                            if filtered.is_empty() {
                                i += 1;
                                continue;
                            }

                            json!(filtered)
                        }
                    };

                    result.push(json!({"role": "user", "content": content}));
                }
                Message::Assistant(msg) => {
                    let blocks: Vec<_> = msg
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text(t) => {
                                if t.text.trim().is_empty() {
                                    None
                                } else {
                                    Some(json!({"type": "text", "text": t.text}))
                                }
                            }
                            ContentBlock::Thinking(th) => {
                                if th.redacted.unwrap_or(false) {
                                    Some(json!({
                                        "type": "redacted_thinking",
                                        "data": th.thinking_signature.as_deref().unwrap_or(""),
                                    }))
                                } else if th.thinking.trim().is_empty() {
                                    None
                                } else if th.thinking_signature.is_none()
                                    || th.thinking_signature.as_ref().unwrap().trim().is_empty()
                                {
                                    Some(json!({"type": "text", "text": th.thinking}))
                                } else {
                                    Some(json!({
                                        "type": "thinking",
                                        "thinking": th.thinking,
                                        "signature": th.thinking_signature,
                                    }))
                                }
                            }
                            ContentBlock::ToolCall(tc) => Some(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": tc.arguments,
                            })),
                            _ => None,
                        })
                        .collect();

                    if blocks.is_empty() {
                        i += 1;
                        continue;
                    }

                    result.push(json!({"role": "assistant", "content": blocks}));
                }
                Message::ToolResult(msg) => {
                    let mut tool_results = Vec::new();
                    tool_results.push(self.convert_tool_result(msg));

                    let mut j = i + 1;
                    while j < messages.len() {
                        if let Message::ToolResult(next_msg) = &messages[j] {
                            tool_results.push(self.convert_tool_result(next_msg));
                            j += 1;
                        } else {
                            break;
                        }
                    }

                    i = j - 1;
                    result.push(json!({"role": "user", "content": tool_results}));
                }
            }
            i += 1;
        }

        result
    }

    fn convert_tool_result(&self, msg: &ToolResultMessage) -> serde_json::Value {
        let content = if msg.content.len() == 1 {
            match &msg.content[0] {
                ContentBlock::Text(t) => json!(t.text),
                _ => json!(msg.content),
            }
        } else {
            let blocks: Vec<_> = msg
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text(t) => json!({"type": "text", "text": t.text}),
                    ContentBlock::Image(img) => json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": img.mime_type,
                            "data": img.data,
                        },
                    }),
                    _ => json!(null),
                })
                .filter(|v| !v.is_null())
                .collect();
            json!(blocks)
        };

        json!({
            "type": "tool_result",
            "tool_use_id": msg.tool_call_id,
            "content": content,
            "is_error": msg.is_error,
        })
    }

    fn convert_tools(&self, tools: &[Tool]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": {
                        "type": "object",
                        "properties": tool.parameters.get("properties").unwrap_or(&json!({})),
                        "required": tool.parameters.get("required").unwrap_or(&json!([])),
                    },
                })
            })
            .collect()
    }

    fn map_stop_reason(&self, reason: &str) -> StopReason {
        match reason {
            "end_turn" => StopReason::Stop,
            "max_tokens" => StopReason::Length,
            "tool_use" => StopReason::ToolUse,
            "stop_sequence" => StopReason::Stop,
            _ => StopReason::Error,
        }
    }

    fn map_stop_reason_to_done(&self, reason: &str) -> DoneReason {
        match reason {
            "end_turn" => DoneReason::Stop,
            "max_tokens" => DoneReason::Length,
            "tool_use" => DoneReason::ToolUse,
            _ => DoneReason::Stop,
        }
    }

    async fn do_stream(
        &self,
        context: &Context,
        model: &Model,
        options: &StreamOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<AssistantMessageEvent>> + Send>>> {
        let _api_key = options
            .api_key
            .clone()
            .or_else(|| get_api_key_from_env(&model.provider))
            .context("No AWS credentials available for Bedrock provider")?;

        let region = self.parse_region(&model.base_url);
        let sdk_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let bedrock_config = aws_sdk_bedrockruntime::config::Builder::from(&sdk_config)
            .region(Some(aws_config::Region::new(region)))
            .build();
        let client = aws_sdk_bedrockruntime::Client::from_conf(bedrock_config);

        let body = self.build_request_body(context, model, options);
        let body_bytes = serde_json::to_vec(&body)?;

        let response = client
            .invoke_model_with_response_stream()
            .model_id(&model.id)
            .body(Blob::new(body_bytes))
            .send()
            .await
            .context("Failed to invoke Bedrock model")?;

        let provider = Self::new();
        let stream = provider.process_stream(response, model.clone()).await;
        Ok(Box::pin(stream) as Pin<Box<dyn Stream<Item = Result<AssistantMessageEvent>> + Send>>)
    }

    async fn process_stream(
        self,
        mut response: aws_sdk_bedrockruntime::operation::invoke_model_with_response_stream::InvokeModelWithResponseStreamOutput,
        model: Model,
    ) -> impl Stream<Item = Result<AssistantMessageEvent>> {
        use async_stream::stream;

        let mut partial_message = AssistantMessage {
            role: "assistant".to_string(),
            content: Vec::new(),
            api: Api::AmazonBedrock,
            provider: model.provider.clone(),
            model: model.id.clone(),
            response_id: None,
            usage: Usage::default(),
            stop_reason: StopReason::Stop,
            error_message: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let mut content_blocks: Vec<ContentBlockState> = Vec::new();
        let mut usage = Usage::default();
        let mut stop_reason: Option<DoneReason> = None;

        stream! {
            while let Ok(Some(event)) = response.body.recv().await {
                if let ResponseStream::Chunk(chunk) = event {
                    if let Some(blob) = chunk.bytes() {
                        let bytes: &[u8] = blob.as_ref();
                        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(bytes) {
                            if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                                match event_type {
                                    "message_start" => {
                                        if let Some(message) = json.get("message") {
                                            partial_message.response_id = message.get("id").and_then(|v| v.as_str()).map(String::from);
                                            if let Some(msg_usage) = message.get("usage") {
                                                usage.input_tokens = msg_usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                                usage.output_tokens = msg_usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                                usage.cache_read_tokens = msg_usage.get("cache_read_input_tokens").and_then(|v| v.as_u64());
                                                usage.cache_write_tokens = msg_usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64());
                                            }
                                            partial_message.usage = usage.clone();
                                            yield Ok(AssistantMessageEvent::Start { partial: partial_message.clone() });
                                        }
                                    }
                                    "content_block_start" => {
                                        if let Some(index) = json.get("index").and_then(|v| v.as_u64()) {
                                            if let Some(block) = json.get("content_block") {
                                                let block_type = block.get("type").and_then(|v| v.as_str());
                                                match block_type {
                                                    Some("text") => {
                                                        content_blocks.push(ContentBlockState::Text { text: String::new(), index: index as usize });
                                                        partial_message.content.push(ContentBlock::Text(TextContent::new("")));
                                                        yield Ok(AssistantMessageEvent::TextStart { content_index: partial_message.content.len() - 1, partial: partial_message.clone() });
                                                    }
                                                    Some("thinking") => {
                                                        content_blocks.push(ContentBlockState::Thinking { thinking: String::new(), signature: None, index: index as usize });
                                                        partial_message.content.push(ContentBlock::Thinking(ThinkingContent::new("")));
                                                        yield Ok(AssistantMessageEvent::ThinkingStart { content_index: partial_message.content.len() - 1, partial: partial_message.clone() });
                                                    }
                                                    Some("tool_use") => {
                                                        let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                        let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                        content_blocks.push(ContentBlockState::ToolUse { id: id.clone(), name: name.clone(), input_json: String::new(), index: index as usize });
                                                        partial_message.content.push(ContentBlock::ToolCall(ToolCall::new(id, name, json!({}))));
                                                        yield Ok(AssistantMessageEvent::ToolCallStart { content_index: partial_message.content.len() - 1, partial: partial_message.clone() });
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                    "content_block_delta" => {
                                        if let Some(index) = json.get("index").and_then(|v| v.as_u64()) {
                                            if let Some(delta) = json.get("delta") {
                                                let delta_type = delta.get("type").and_then(|v| v.as_str());
                                                match delta_type {
                                                    Some("text_delta") => {
                                                        if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                                            if let Some(block_idx) = content_blocks.iter().position(|b| b.index() == index as usize) {
                                                                if let ContentBlockState::Text { text: ref mut t, .. } = content_blocks[block_idx] {
                                                                    t.push_str(text);
                                                                    if let Some(ContentBlock::Text(ref mut tc)) = partial_message.content.get_mut(block_idx) {
                                                                        tc.text.push_str(text);
                                                                    }
                                                                    yield Ok(AssistantMessageEvent::TextDelta { content_index: block_idx, delta: text.to_string(), partial: partial_message.clone() });
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Some("thinking_delta") => {
                                                        if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                                                            if let Some(block_idx) = content_blocks.iter().position(|b| b.index() == index as usize) {
                                                                if let ContentBlockState::Thinking { thinking: ref mut t, .. } = content_blocks[block_idx] {
                                                                    t.push_str(thinking);
                                                                    if let Some(ContentBlock::Thinking(ref mut tc)) = partial_message.content.get_mut(block_idx) {
                                                                        tc.thinking.push_str(thinking);
                                                                    }
                                                                    yield Ok(AssistantMessageEvent::ThinkingDelta { content_index: block_idx, delta: thinking.to_string(), partial: partial_message.clone() });
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Some("input_json_delta") => {
                                                        if let Some(partial_json) = delta.get("partial_json").and_then(|v| v.as_str()) {
                                                            if let Some(block_idx) = content_blocks.iter().position(|b| b.index() == index as usize) {
                                                                if let ContentBlockState::ToolUse { input_json: ref mut json, .. } = content_blocks[block_idx] {
                                                                    json.push_str(partial_json);
                                                                    if let Some(parsed) = parse_partial_json(json) {
                                                                        if let Some(ContentBlock::ToolCall(ref mut tc)) = partial_message.content.get_mut(block_idx) {
                                                                            tc.arguments = parsed;
                                                                        }
                                                                    }
                                                                    yield Ok(AssistantMessageEvent::ToolCallDelta { content_index: block_idx, delta: partial_json.to_string(), partial: partial_message.clone() });
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Some("signature_delta") => {
                                                        if let Some(sig) = delta.get("signature").and_then(|v| v.as_str()) {
                                                            if let Some(block_idx) = content_blocks.iter().position(|b| b.index() == index as usize) {
                                                                if let ContentBlockState::Thinking { signature: ref mut s, .. } = content_blocks[block_idx] {
                                                                    if let Some(ref mut sig_str) = s {
                                                                        sig_str.push_str(sig);
                                                                    } else {
                                                                        *s = Some(sig.to_string());
                                                                    }
                                                                    if let Some(ContentBlock::Thinking(ref mut tc)) = partial_message.content.get_mut(block_idx) {
                                                                        tc.thinking_signature = s.clone();
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                    "content_block_stop" => {
                                        if let Some(index) = json.get("index").and_then(|v| v.as_u64()) {
                                            if let Some(block_idx) = content_blocks.iter().position(|b| b.index() == index as usize) {
                                                let block = &content_blocks[block_idx];
                                                match block {
                                                    ContentBlockState::Text { text, .. } => {
                                                        yield Ok(AssistantMessageEvent::TextEnd { content_index: block_idx, content: text.clone(), partial: partial_message.clone() });
                                                    }
                                                    ContentBlockState::Thinking { thinking, .. } => {
                                                        yield Ok(AssistantMessageEvent::ThinkingEnd { content_index: block_idx, content: thinking.clone(), partial: partial_message.clone() });
                                                    }
                                                    ContentBlockState::ToolUse { id, name, input_json, .. } => {
                                                        let arguments = parse_partial_json(input_json).unwrap_or_else(|| json!({}));
                                                        if let Some(ContentBlock::ToolCall(ref mut tc)) = partial_message.content.get_mut(block_idx) {
                                                            tc.arguments = arguments.clone();
                                                        }
                                                        let tool_call = ToolCall {
                                                            content_type: "toolCall".to_string(),
                                                            id: id.clone(),
                                                            name: name.clone(),
                                                            arguments,
                                                            thought_signature: None,
                                                        };
                                                        yield Ok(AssistantMessageEvent::ToolCallEnd { content_index: block_idx, tool_call, partial: partial_message.clone() });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    "message_delta" => {
                                        if let Some(delta) = json.get("delta") {
                                            if let Some(reason) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                                                stop_reason = Some(self.map_stop_reason_to_done(reason));
                                                partial_message.stop_reason = self.map_stop_reason(reason);
                                            }
                                        }
                                        if let Some(msg_usage) = json.get("usage") {
                                            if let Some(tokens) = msg_usage.get("output_tokens").and_then(|v| v.as_u64()) {
                                                usage.output_tokens = tokens;
                                            }
                                            if let Some(tokens) = msg_usage.get("input_tokens").and_then(|v| v.as_u64()) {
                                                usage.input_tokens = tokens;
                                            }
                                            if let Some(tokens) = msg_usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
                                                usage.cache_read_tokens = Some(tokens);
                                            }
                                            if let Some(tokens) = msg_usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()) {
                                                usage.cache_write_tokens = Some(tokens);
                                            }
                                        }
                                        partial_message.usage = usage.clone();
                                    }
                                    "message_stop" => {
                                        let reason = stop_reason.clone().unwrap_or(DoneReason::Stop);
                                        let mut final_message = partial_message.clone();
                                        final_message.usage = usage.clone();
                                        yield Ok(AssistantMessageEvent::Done { reason, message: final_message });
                                        return;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }

            let reason = stop_reason.clone().unwrap_or(DoneReason::Stop);
            let mut final_message = partial_message.clone();
            final_message.usage = usage.clone();
            yield Ok(AssistantMessageEvent::Done { reason, message: final_message });
        }
    }
}

impl Default for BedrockProvider {
    fn default() -> Self {
        Self::new()
    }
}

enum ContentBlockState {
    Text { text: String, index: usize },
    Thinking { thinking: String, signature: Option<String>, index: usize },
    ToolUse { id: String, name: String, input_json: String, index: usize },
}

impl ContentBlockState {
    fn index(&self) -> usize {
        match self {
            ContentBlockState::Text { index, .. } => *index,
            ContentBlockState::Thinking { index, .. } => *index,
            ContentBlockState::ToolUse { index, .. } => *index,
        }
    }
}

#[async_trait]
impl ApiProvider for BedrockProvider {
    fn api(&self) -> Api {
        Api::AmazonBedrock
    }

    async fn stream(
        &self,
        context: &Context,
        model: &Model,
        options: &StreamOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<AssistantMessageEvent>> + Send>>> {
        self.do_stream(context, model, options).await
    }
}

/// 将 Bedrock 提供者注册到全局注册表
pub fn register() {
    let provider = std::sync::Arc::new(BedrockProvider::new());
    crate::api_registry::register_api_provider(provider);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::fixtures::*;

    #[test]
    fn test_parse_region() {
        let provider = BedrockProvider::new();
        assert_eq!(provider.parse_region("bedrock://us-west-2"), "us-west-2");
        assert_eq!(provider.parse_region("bedrock://eu-west-1"), "eu-west-1");
    }

    #[test]
    fn test_map_stop_reason() {
        let provider = BedrockProvider::new();
        assert_eq!(provider.map_stop_reason("end_turn"), StopReason::Stop);
        assert_eq!(provider.map_stop_reason("max_tokens"), StopReason::Length);
        assert_eq!(provider.map_stop_reason("tool_use"), StopReason::ToolUse);
    }

    #[test]
    fn test_build_request_body() {
        let provider = BedrockProvider::new();
        let model = sample_model(Api::AmazonBedrock, Provider::AmazonBedrock);
        let context = sample_context("You are a helpful assistant", vec![sample_user_message("Hello")]);
        let options = sample_stream_options("test-key");

        let body = provider.build_request_body(&context, &model, &options);

        // 验证基本结构
        assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
        assert!(body["max_tokens"].as_u64().is_some());
        assert_eq!(body["system"], "You are a helpful assistant");

        // 验证消息数组
        let messages = body["messages"].as_array().unwrap();
        assert!(!messages.is_empty());
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn test_build_request_body_with_tools() {
        let provider = BedrockProvider::new();
        let model = sample_model(Api::AmazonBedrock, Provider::AmazonBedrock);

        let tool = sample_tool("get_weather", "Get weather for a location");
        let context = sample_context_with_tools(
            "You are a helpful assistant",
            vec![sample_user_message("What's the weather?")],
            vec![tool],
        );
        let options = sample_stream_options("test-key");

        let body = provider.build_request_body(&context, &model, &options);

        // 验证工具定义
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert!(tools[0]["input_schema"].is_object());
    }

    #[test]
    fn test_build_request_body_with_reasoning() {
        let provider = BedrockProvider::new();
        let mut model = sample_model(Api::AmazonBedrock, Provider::AmazonBedrock);
        model.reasoning = true;

        let context = sample_context("You are a helpful assistant", vec![sample_user_message("Think deeply")]);
        let options = sample_stream_options("test-key");

        let body = provider.build_request_body(&context, &model, &options);

        // 验证 thinking 配置
        assert!(body["thinking"].is_object());
        assert_eq!(body["thinking"]["type"], "enabled");
        assert!(body["thinking"]["budget_tokens"].as_u64().is_some());
    }

    #[test]
    fn test_convert_messages() {
        let provider = BedrockProvider::new();
        let model = sample_model(Api::AmazonBedrock, Provider::AmazonBedrock);

        let context = sample_context(
            "You are helpful",
            vec![
                sample_user_message("Hello"),
                Message::Assistant(sample_assistant_message("Hi there!")),
            ],
        );

        let messages = provider.convert_messages(&context.messages, &model);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
    }

    #[test]
    fn test_convert_messages_with_tool_call() {
        let provider = BedrockProvider::new();
        let model = sample_model(Api::AmazonBedrock, Provider::AmazonBedrock);

        let mut assistant_msg = sample_assistant_message("Let me check");
        assistant_msg.content.push(ContentBlock::ToolCall(
            ToolCall::new("tool_123", "get_weather", serde_json::json!({"city": "Paris"}))
        ));

        let context = sample_context(
            "You are helpful",
            vec![
                sample_user_message("What's the weather?"),
                Message::Assistant(assistant_msg),
                sample_tool_result("tool_123", "get_weather", "Sunny, 25°C"),
            ],
        );

        let messages = provider.convert_messages(&context.messages, &model);

        // 应该有用户消息、助手消息（包含 tool_use）和工具结果
        assert!(messages.len() >= 2);

        // 找到包含 tool_use 的消息
        let assistant_msg_json = messages.iter().find(|m| m["role"] == "assistant").unwrap();
        let content = assistant_msg_json["content"].as_array().unwrap();
        let tool_use = content.iter().find(|c| c["type"] == "tool_use");
        assert!(tool_use.is_some());
    }

    #[test]
    fn test_convert_tool_result() {
        let provider = BedrockProvider::new();

        let tool_result = ToolResultMessage::new(
            "tool_123",
            "get_weather",
            vec![ContentBlock::Text(TextContent::new("Sunny, 25°C"))],
        );

        let result = provider.convert_tool_result(&tool_result);

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "tool_123");
        assert_eq!(result["content"], "Sunny, 25°C");
        assert_eq!(result["is_error"], false);
    }

    #[test]
    fn test_convert_tools() {
        let provider = BedrockProvider::new();

        let tools = vec![
            sample_tool("get_weather", "Get weather info"),
            sample_tool("search", "Search the web"),
        ];

        let converted = provider.convert_tools(&tools);

        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0]["name"], "get_weather");
        assert_eq!(converted[1]["name"], "search");
        assert!(converted[0]["input_schema"].is_object());
    }

    #[test]
    fn test_map_stop_reason_to_done() {
        let provider = BedrockProvider::new();

        assert_eq!(provider.map_stop_reason_to_done("end_turn"), DoneReason::Stop);
        assert_eq!(provider.map_stop_reason_to_done("max_tokens"), DoneReason::Length);
        assert_eq!(provider.map_stop_reason_to_done("tool_use"), DoneReason::ToolUse);
    }

    #[test]
    fn test_model_id_mapping() {
        // 测试 Bedrock 模型 ID 格式
        let model_ids = vec![
            "anthropic.claude-3-sonnet-20240229-v1:0",
            "anthropic.claude-3-opus-20240229-v1:0",
            "anthropic.claude-3-haiku-20240307-v1:0",
            "anthropic.claude-3-5-sonnet-20241022-v2:0",
        ];

        for id in model_ids {
            let mut model = sample_model(Api::AmazonBedrock, Provider::AmazonBedrock);
            model.id = id.to_string();
            assert_eq!(model.id, id);
            assert_eq!(model.api, Api::AmazonBedrock);
        }
    }

    #[test]
    fn test_parse_region_variations() {
        let provider = BedrockProvider::new();

        // 测试 bedrock:// 前缀的 URL
        assert_eq!(provider.parse_region("bedrock://us-east-1"), "us-east-1");
        assert_eq!(provider.parse_region("bedrock://eu-west-1"), "eu-west-1");
        assert_eq!(provider.parse_region("bedrock://ap-northeast-1"), "ap-northeast-1");

        // 测试没有前缀的情况（应该使用环境变量或默认值）
        // 这里我们无法测试环境变量，但可以测试默认行为
        let region = provider.parse_region("");
        // 应该返回环境变量值或默认值
        assert!(!region.is_empty());
    }

    #[test]
    fn test_region_configuration_validation() {
        let provider = BedrockProvider::new();

        // 测试有效的 AWS 区域格式
        let valid_regions = vec![
            "us-east-1",
            "us-west-2",
            "eu-west-1",
            "eu-central-1",
            "ap-northeast-1",
            "ap-southeast-1",
            "ca-central-1",
            "sa-east-1",
        ];

        for region in valid_regions {
            let parsed = provider.parse_region(&format!("bedrock://{}", region));
            assert_eq!(parsed, region, "Region {} should be parsed correctly", region);
        }
    }

    #[test]
    fn test_build_request_body_edge_cases() {
        let provider = BedrockProvider::new();
        let model = sample_model(Api::AmazonBedrock, Provider::AmazonBedrock);

        // 测试空消息列表
        let context = sample_context("System", vec![]);
        let options = sample_stream_options("test-key");
        let body = provider.build_request_body(&context, &model, &options);
        
        assert!(body["messages"].as_array().unwrap().is_empty());
        assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");

        // 测试带 reasoning 的模型
        let mut reasoning_model = model.clone();
        reasoning_model.reasoning = true;
        let body2 = provider.build_request_body(&context, &reasoning_model, &options);
        assert!(body2["thinking"].is_object());
        assert_eq!(body2["thinking"]["type"], "enabled");
    }

    #[test]
    fn test_convert_messages_with_only_whitespace() {
        let provider = BedrockProvider::new();
        let model = sample_model(Api::AmazonBedrock, Provider::AmazonBedrock);

        let context = sample_context(
            "System",
            vec![
                Message::User(UserMessage::new("")),
                Message::User(UserMessage::new("   ")),
                Message::User(UserMessage::new("\n\t")),
                sample_user_message("Valid"),
            ],
        );

        let messages = provider.convert_messages(&context.messages, &model);
        // 空白消息应该被过滤掉
        assert!(!messages.is_empty());
    }

    #[test]
    fn test_convert_tools_with_complex_schema() {
        let provider = BedrockProvider::new();

        // 测试复杂参数 schema 的工具
        let complex_tool = Tool {
            name: "complex_tool".to_string(),
            description: "A tool with complex schema".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "nested": {
                        "type": "object",
                        "properties": {
                            "field1": {"type": "string"},
                            "field2": {"type": "number"}
                        }
                    },
                    "array_field": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                },
                "required": ["nested"]
            }),
        };

        let converted = provider.convert_tools(&[complex_tool]);
        assert_eq!(converted.len(), 1);
        assert!(converted[0]["input_schema"]["properties"]["nested"].is_object());
    }

    #[test]
    fn test_map_stop_reason_edge_cases() {
        let provider = BedrockProvider::new();

        // 测试所有已知的 stop reason
        assert_eq!(provider.map_stop_reason_to_done("end_turn"), DoneReason::Stop);
        assert_eq!(provider.map_stop_reason_to_done("max_tokens"), DoneReason::Length);
        assert_eq!(provider.map_stop_reason_to_done("tool_use"), DoneReason::ToolUse);
        
        // 测试未知的 stop reason（应该返回 Stop 作为默认值）
        assert_eq!(provider.map_stop_reason_to_done("unknown"), DoneReason::Stop);
        assert_eq!(provider.map_stop_reason_to_done(""), DoneReason::Stop);
    }
}
