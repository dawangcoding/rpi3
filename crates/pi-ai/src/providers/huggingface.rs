//! Huggingface API Provider 实现
//!
//! 复用 OpenAI Chat Completions API 接口格式
//! Huggingface 提供 Inference API 服务

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::api_registry::ApiProvider;
use crate::types::*;
use super::openai::OpenAiProvider;

/// Huggingface API Provider
/// 
/// Huggingface 使用 OpenAI 兼容的 Chat Completions API
pub struct HuggingfaceProvider {
    inner: OpenAiProvider,
}

impl HuggingfaceProvider {
    /// 创建新的 Huggingface Provider
    pub fn new() -> Self {
        Self {
            inner: OpenAiProvider::new(),
        }
    }
}

impl Default for HuggingfaceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ApiProvider for HuggingfaceProvider {
    fn api(&self) -> Api {
        Api::Huggingface
    }

    async fn stream(
        &self,
        context: &Context,
        model: &Model,
        options: &StreamOptions,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>> {
        // 委托给 OpenAI Provider（Huggingface API 完全兼容 OpenAI 格式）
        self.inner.stream(context, model, options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::fixtures::*;
    use mockito::Server;
    use futures::StreamExt;

    #[test]
    fn test_huggingface_api_type() {
        let provider = HuggingfaceProvider::new();
        assert_eq!(provider.api(), Api::Huggingface);
    }

    #[tokio::test]
    async fn test_huggingface_stream_text_response() {
        let mut server = Server::new_async().await;
        let provider = HuggingfaceProvider::new();

        // Huggingface SSE 格式（与 OpenAI 兼容）
        let sse_body = r#"data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1677652288,"model":"meta-llama/Llama-3.1-8B-Instruct","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1677652288,"model":"meta-llama/Llama-3.1-8B-Instruct","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1677652288,"model":"meta-llama/Llama-3.1-8B-Instruct","choices":[{"index":0,"delta":{"content":" from Huggingface!"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1677652288,"model":"meta-llama/Llama-3.1-8B-Instruct","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
"#;

        let mock = server.mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_body)
            .create_async()
            .await;

        let mut model = sample_model(Api::Huggingface, Provider::Huggingface);
        model.base_url = server.url();

        let context = sample_context("You are a helpful assistant", vec![sample_user_message("Say hello")]);
        let options = sample_stream_options("test-api-key");

        let mut stream = provider.stream(&context, &model, &options).await.unwrap();

        let mut events = vec![];
        while let Some(event) = stream.next().await {
            events.push(event.unwrap());
        }

        // 验证事件序列
        assert!(!events.is_empty());

        // 查找文本事件
        let text_deltas: Vec<_> = events.iter().filter_map(|e| match e {
            AssistantMessageEvent::TextDelta { delta, .. } => Some(delta.clone()),
            _ => None,
        }).collect();
        
        assert!(!text_deltas.is_empty(), "Should have TextDelta events");
        assert!(text_deltas.iter().any(|d| d.contains("Huggingface")), "Should contain 'Huggingface' in response");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_huggingface_stream_tool_call() {
        let mut server = Server::new_async().await;
        let provider = HuggingfaceProvider::new();

        let sse_body = r#"data: {"id":"chatcmpl-456","object":"chat.completion.chunk","created":1677652288,"model":"meta-llama/Llama-3.1-8B-Instruct","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_abc123","type":"function","function":{"name":"get_weather","arguments":"{\"location\":\"Paris\"}"}}]},"finish_reason":null}]}

data: {"id":"chatcmpl-456","object":"chat.completion.chunk","created":1677652288,"model":"meta-llama/Llama-3.1-8B-Instruct","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}

data: [DONE]
"#;

        let mock = server.mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "text/event-stream")
            .with_body(sse_body)
            .create_async()
            .await;

        let mut model = sample_model(Api::Huggingface, Provider::Huggingface);
        model.base_url = server.url();

        let tool = sample_tool("get_weather", "Get weather for a location");
        let context = sample_context_with_tools(
            "You are a helpful assistant",
            vec![sample_user_message("What's the weather in Paris?")],
            vec![tool],
        );
        let options = sample_stream_options("test-api-key");

        let mut stream = provider.stream(&context, &model, &options).await.unwrap();

        let mut events = vec![];
        while let Some(event) = stream.next().await {
            events.push(event.unwrap());
        }

        // 验证 ToolCall 事件
        let tool_call_starts: Vec<_> = events.iter().filter(|e| matches!(e, AssistantMessageEvent::ToolCallStart { .. })).collect();
        assert!(!tool_call_starts.is_empty(), "Expected at least one ToolCallStart event");

        // 验证 Done 事件存在
        assert!(matches!(events.last().unwrap(), AssistantMessageEvent::Done { .. }), "Expected Done event at the end");

        mock.assert_async().await;
    }
}
