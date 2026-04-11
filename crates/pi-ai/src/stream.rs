//! 统一流式 API 入口
//!
//! 提供流式和非流式的 LLM 调用接口

use futures::{Future, Stream, StreamExt};
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tracing::warn;

use crate::api_registry::resolve_api_provider;
use crate::models::get_model;
use crate::retry::RetryPolicy;
use crate::types::*;

/// 恢复 future 的类型别名
/// 用于简化复杂的 Pin<Box<dyn Future>> 嵌套类型
type RecoveryFuture = Pin<Box<dyn Future<Output = anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>>> + Send>>;

/// 流式调用 LLM（底层 API）
/// 
/// 返回事件流，用于实时接收模型响应
/// 如果 options 中包含 retry_config，则使用重试包装
pub async fn stream(
    context: &Context,
    model: &Model,
    options: &StreamOptions,
) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>> {
    // 如果有重试配置，使用带重试的流式调用
    if options.retry_config.is_some() {
        return stream_with_retry(context, model, options).await;
    }
    
    let provider = resolve_api_provider(&model.api)?;
    provider.stream(context, model, options).await
}

/// 带重试和恢复的流式调用
/// 
/// 在 stream() 调用外层包装重试逻辑，并支持流中断恢复
pub async fn stream_with_retry(
    context: &Context,
    model: &Model,
    options: &StreamOptions,
) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>> {
    let retry_config = options.retry_config.clone().unwrap_or_default();
    let policy = RetryPolicy::new(retry_config);
    
    // 使用重试策略执行流式调用
    let stream_result = policy.execute("stream", || async {
        let provider = resolve_api_provider(&model.api)?;
        provider.stream(context, model, options).await
    }).await;
    
    match stream_result {
        Ok(stream) => {
            // 包装流以支持中断恢复
            let resilient = ResilientStream::new(
                stream,
                context.clone(),
                model.clone(),
                options.clone(),
                policy,
            );
            Ok(Box::pin(resilient) as Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>)
        }
        Err(e) => Err(e),
    }
}

/// 支持恢复机制的流包装器
///
/// 监控流是否正常结束（收到 Done 事件），如果流异常终止，
/// 根据 retry_config 重新发起请求
pub struct ResilientStream {
    /// 当前活动的流
    inner: Option<Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>>,
    /// 原始上下文（用于恢复）
    context: Context,
    /// 原始模型（用于恢复）
    model: Model,
    /// 原始选项（用于恢复）
    options: StreamOptions,
    /// 重试策略
    policy: RetryPolicy,
    /// 恢复尝试计数
    recovery_attempts: u32,
    /// 是否已经收到 Done 事件
    received_done: bool,
    /// 是否已经失败（无法恢复）
    failed: bool,
    /// 缓冲的事件（在恢复期间）
    buffer: Vec<AssistantMessageEvent>,
    /// 当前缓冲位置
    buffer_pos: usize,
    /// 恢复 future（用于保持恢复状态）
    recovery_future: Option<RecoveryFuture>,
}

impl ResilientStream {
    /// 创建新的可恢复流
    pub fn new(
        inner: Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>,
        context: Context,
        model: Model,
        options: StreamOptions,
        policy: RetryPolicy,
    ) -> Self {
        Self {
            inner: Some(inner),
            context,
            model,
            options,
            policy,
            recovery_attempts: 0,
            received_done: false,
            failed: false,
            buffer: Vec::new(),
            buffer_pos: 0,
            recovery_future: None,
        }
    }

    /// 创建恢复 future（不借用 self）
    fn create_recovery_future(
        context: Context,
        model: Model,
        options: StreamOptions,
        delay: std::time::Duration,
    ) -> RecoveryFuture {
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            let provider = resolve_api_provider(&model.api)?;
            let new_stream = provider.stream(&context, &model, &options).await?;
            Ok(new_stream)
        })
    }
}

impl Stream for ResilientStream {
    type Item = anyhow::Result<AssistantMessageEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        // 如果有缓冲的事件，先返回缓冲的事件
        if self.buffer_pos < self.buffer.len() {
            let event = self.buffer[self.buffer_pos].clone();
            self.buffer_pos += 1;
            return Poll::Ready(Some(Ok(event)));
        }

        // 如果已经失败，返回 None
        if self.failed {
            return Poll::Ready(None);
        }

        // 如果正在恢复中，poll 恢复 future
        if let Some(ref mut fut) = self.recovery_future {
            match fut.as_mut().poll(cx) {
                Poll::Ready(Ok(new_stream)) => {
                    self.recovery_future = None;
                    self.inner = Some(new_stream);
                    self.received_done = false;
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Poll::Ready(Err(err)) => {
                    self.recovery_future = None;
                    self.failed = true;
                    return Poll::Ready(Some(Err(err)));
                }
                Poll::Pending => return Poll::Pending,
            }
        }

        // 尝试从当前流获取下一个事件
        if let Some(ref mut inner) = self.inner {
            match inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    // 检查是否是 Done 事件
                    if matches!(event, AssistantMessageEvent::Done { .. }) {
                        self.received_done = true;
                    }
                    Poll::Ready(Some(Ok(event)))
                }
                Poll::Ready(Some(Err(e))) => {
                    // 流发生错误，检查是否应该尝试恢复
                    if RetryPolicy::is_retryable(&e) && self.recovery_attempts < self.policy.max_retries() {
                        // 创建恢复 future
                        self.recovery_attempts += 1;
                        let delay = self.policy.delay_for_attempt(self.recovery_attempts);
                        
                        warn!(
                            "Stream interrupted, attempting recovery {}/{} after {:?}...",
                            self.recovery_attempts,
                            self.policy.max_retries(),
                            delay
                        );

                        self.recovery_future = Some(Self::create_recovery_future(
                            self.context.clone(),
                            self.model.clone(),
                            self.options.clone(),
                            delay,
                        ));
                        
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    } else {
                        // 不可重试错误或已达到最大重试次数
                        self.failed = true;
                        Poll::Ready(Some(Err(e)))
                    }
                }
                Poll::Ready(None) => {
                    // 流结束但未收到 Done 事件，尝试恢复
                    if !self.received_done && self.recovery_attempts < self.policy.max_retries() {
                        self.recovery_attempts += 1;
                        let delay = self.policy.delay_for_attempt(self.recovery_attempts);
                        
                        warn!(
                            "Stream interrupted, attempting recovery {}/{} after {:?}...",
                            self.recovery_attempts,
                            self.policy.max_retries(),
                            delay
                        );

                        self.recovery_future = Some(Self::create_recovery_future(
                            self.context.clone(),
                            self.model.clone(),
                            self.options.clone(),
                            delay,
                        ));
                        
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    } else {
                        Poll::Ready(None)
                    }
                }
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Ready(None)
        }
    }
}

/// 流式调用（简化版）
/// 
/// 使用 SimpleStreamOptions 进行流式调用
pub async fn stream_simple(
    context: &Context,
    model: &Model,
    options: &SimpleStreamOptions,
) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>> {
    // 将 SimpleStreamOptions 转换为 StreamOptions
    let stream_options = StreamOptions {
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        api_key: options.api_key.clone(),
        transport: options.transport.clone(),
        cache_retention: options.cache_retention.clone(),
        session_id: options.session_id.clone(),
        headers: options.headers.clone(),
        max_retry_delay_ms: options.max_retry_delay_ms,
        metadata: options.metadata.clone(),
        retry_config: options.retry_config.clone(),
    };
    
    stream(context, model, &stream_options).await
}

/// 非流式调用
/// 
/// 收集所有事件并返回完整消息
pub async fn complete(
    context: &Context,
    model: &Model,
    options: &StreamOptions,
) -> anyhow::Result<AssistantMessage> {
    let mut stream = stream(context, model, options).await?;
    let mut result_message: Option<AssistantMessage> = None;
    
    while let Some(event_result) = stream.next().await {
        let event = event_result?;
        
        match event {
            AssistantMessageEvent::Done { message, .. } => {
                result_message = Some(message);
                break;
            }
            AssistantMessageEvent::Error { error, .. } => {
                return Ok(error);
            }
            _ => {
                // 继续收集其他事件
            }
        }
    }
    
    result_message.ok_or_else(|| anyhow::anyhow!("Stream ended without Done event"))
}

/// 非流式调用（简化版）
/// 
/// 使用 SimpleStreamOptions 进行非流式调用
pub async fn complete_simple(
    context: &Context,
    model: &Model,
    options: &SimpleStreamOptions,
) -> anyhow::Result<AssistantMessage> {
    // 将 SimpleStreamOptions 转换为 StreamOptions
    let stream_options = StreamOptions {
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        api_key: options.api_key.clone(),
        transport: options.transport.clone(),
        cache_retention: options.cache_retention.clone(),
        session_id: options.session_id.clone(),
        headers: options.headers.clone(),
        max_retry_delay_ms: options.max_retry_delay_ms,
        metadata: options.metadata.clone(),
        retry_config: options.retry_config.clone(),
    };
    
    complete(context, model, &stream_options).await
}

/// 通过模型 ID 流式调用 LLM
/// 
/// 根据模型 ID 自动查找模型配置并流式调用
pub async fn stream_by_model_id(
    context: &Context,
    model_id: &str,
    options: &StreamOptions,
) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>> {
    let model = get_model(model_id)
        .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;
    stream(context, &model, options).await
}

/// 通过模型 ID 非流式调用 LLM
/// 
/// 根据模型 ID 自动查找模型配置并返回完整消息
pub async fn complete_by_model_id(
    context: &Context,
    model_id: &str,
    options: &StreamOptions,
) -> anyhow::Result<AssistantMessage> {
    let model = get_model(model_id)
        .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;
    complete(context, &model, options).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{UserMessage, UserContent};
    use futures::stream;

    fn create_test_context() -> Context {
        Context::new(vec![Message::User(UserMessage::new("Hello"))])
    }

    fn create_test_model() -> Model {
        Model {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            api: Api::Anthropic,
            provider: Provider::Anthropic,
            base_url: "https://api.example.com".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: crate::types::ModelCost {
                input: 1.0,
                output: 2.0,
                cache_read: None,
                cache_write: None,
            },
            context_window: 4096,
            max_tokens: 1024,
            headers: None,
            compat: None,
        }
    }

    #[test]
    fn test_stream_options_default() {
        let options = StreamOptions::default();
        assert!(options.temperature.is_none());
        assert!(options.max_tokens.is_none());
        assert!(options.api_key.is_none());
        assert!(options.retry_config.is_none());
    }

    #[test]
    fn test_stream_options_with_retry_config() {
        let config = crate::retry::RetryConfig::new(5);
        let options = StreamOptions {
            retry_config: Some(config.clone()),
            ..Default::default()
        };
        
        assert!(options.retry_config.is_some());
        assert_eq!(options.retry_config.unwrap().max_retries, 5);
    }

    #[test]
    fn test_simple_stream_options_conversion() {
        let simple = SimpleStreamOptions {
            temperature: Some(0.7),
            max_tokens: Some(1000),
            ..Default::default()
        };
        
        let stream_opts = StreamOptions {
            temperature: simple.temperature,
            max_tokens: simple.max_tokens,
            api_key: simple.api_key.clone(),
            transport: simple.transport.clone(),
            cache_retention: simple.cache_retention.clone(),
            session_id: simple.session_id.clone(),
            headers: simple.headers.clone(),
            max_retry_delay_ms: simple.max_retry_delay_ms,
            metadata: simple.metadata.clone(),
            retry_config: simple.retry_config.clone(),
        };
        
        assert_eq!(stream_opts.temperature, Some(0.7));
        assert_eq!(stream_opts.max_tokens, Some(1000));
    }

    #[test]
    fn test_resilient_stream_creation() {
        let context = create_test_context();
        let model = create_test_model();
        let options = StreamOptions::default();
        let policy = RetryPolicy::default();
        
        // 创建一个简单的测试流
        let test_stream = Box::pin(stream::iter(vec![])) as Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>;
        
        let _resilient = ResilientStream::new(
            test_stream,
            context,
            model,
            options,
            policy,
        );
    }

    #[test]
    fn test_context_with_system_prompt() {
        let context = Context::new(vec![Message::User(UserMessage::new("Hello"))])
            .with_system_prompt("You are a helpful assistant.");
        
        assert!(context.system_prompt.is_some());
        assert_eq!(context.system_prompt.unwrap(), "You are a helpful assistant.");
    }

    #[test]
    fn test_context_with_tools() {
        let tool = Tool::new(
            "test_tool",
            "A test tool",
            serde_json::json!({"type": "object"}),
        );
        
        let context = Context::new(vec![Message::User(UserMessage::new("Hello"))])
            .with_tools(vec![tool]);
        
        assert!(context.tools.is_some());
        assert_eq!(context.tools.unwrap().len(), 1);
    }

    #[test]
    fn test_message_user_content() {
        let msg = UserMessage::new("Hello, world!");
        
        match &msg.content {
            UserContent::Text(text) => assert_eq!(text, "Hello, world!"),
            UserContent::Blocks(_) => panic!("Expected text content"),
        }
    }

    #[test]
    fn test_user_content_from_string() {
        let content: UserContent = "Hello".into();
        assert!(matches!(content, UserContent::Text(s) if s == "Hello"));
    }

    #[test]
    fn test_user_content_from_blocks() {
        let blocks = vec![ContentBlock::Text(TextContent::new("Hello"))];
        let content: UserContent = blocks.clone().into();
        assert!(matches!(content, UserContent::Blocks(b) if b.len() == 1));
    }

    #[test]
    fn test_assistant_message_default() {
        let msg = AssistantMessage::default();
        assert!(msg.content.is_empty());
        assert_eq!(msg.stop_reason, StopReason::Stop);
    }

    #[test]
    fn test_assistant_message_builder() {
        let msg = AssistantMessage::new(Api::Anthropic, Provider::Anthropic, "claude-3")
            .with_content(vec![ContentBlock::Text(TextContent::new("Hello"))])
            .with_stop_reason(StopReason::ToolUse)
            .with_usage(Usage {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: None,
                cache_write_tokens: None,
            });
        
        assert_eq!(msg.model, "claude-3");
        assert_eq!(msg.content.len(), 1);
        assert_eq!(msg.stop_reason, StopReason::ToolUse);
        assert_eq!(msg.usage.input_tokens, 100);
    }

    #[test]
    fn test_assistant_message_with_error() {
        let msg = AssistantMessage::default()
            .with_error_message("Something went wrong");
        
        assert!(msg.error_message.is_some());
        assert_eq!(msg.stop_reason, StopReason::Error);
    }

    #[test]
    fn test_tool_result_message() {
        let result = ToolResultMessage::new(
            "tool-123",
            "test_tool",
            vec![ContentBlock::Text(TextContent::new("result"))],
        );
        
        assert_eq!(result.tool_call_id, "tool-123");
        assert_eq!(result.tool_name, "test_tool");
        assert!(!result.is_error);
    }

    #[test]
    fn test_tool_result_with_error() {
        let result = ToolResultMessage::new(
            "tool-123",
            "test_tool",
            vec![ContentBlock::Text(TextContent::new("error"))],
        ).with_error(true);
        
        assert!(result.is_error);
    }

    #[test]
    fn test_assistant_message_event_done() {
        let msg = AssistantMessage::default();
        let event = AssistantMessageEvent::Done {
            reason: DoneReason::Stop,
            message: msg.clone(),
        };
        
        // 验证事件类型
        assert!(matches!(event, AssistantMessageEvent::Done { .. }));
    }

    #[test]
    fn test_assistant_message_event_error() {
        let msg = AssistantMessage::default().with_error_message("test error");
        let event = AssistantMessageEvent::Error {
            reason: ErrorReason::Error,
            error: msg,
        };
        
        assert!(matches!(event, AssistantMessageEvent::Error { .. }));
    }

    #[test]
    fn test_assistant_message_event_text_delta() {
        let msg = AssistantMessage::default();
        let event = AssistantMessageEvent::TextDelta {
            content_index: 0,
            delta: "Hello".to_string(),
            partial: msg,
        };
        
        assert!(matches!(event, AssistantMessageEvent::TextDelta { .. }));
    }

    #[test]
    fn test_stream_by_model_id_not_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let context = create_test_context();
            let options = StreamOptions::default();
            
            let result = stream_by_model_id(&context, "nonexistent-model", &options).await;
            assert!(result.is_err());
            let err = result.err().unwrap();
            assert!(err.to_string().contains("Model not found"));
        });
    }

    #[test]
    fn test_complete_by_model_id_not_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let context = create_test_context();
            let options = StreamOptions::default();
            
            let result = complete_by_model_id(&context, "nonexistent-model", &options).await;
            assert!(result.is_err());
            let err = result.err().unwrap();
            assert!(err.to_string().contains("Model not found"));
        });
    }

    #[test]
    fn test_transport_enum() {
        assert_eq!(Transport::Sse, Transport::Sse);
        assert_eq!(Transport::Websocket, Transport::Websocket);
        assert_eq!(Transport::Auto, Transport::Auto);
        assert_ne!(Transport::Sse, Transport::Websocket);
    }

    #[test]
    fn test_cache_retention_enum() {
        assert_eq!(CacheRetention::None, CacheRetention::None);
        assert_eq!(CacheRetention::Short, CacheRetention::Short);
        assert_eq!(CacheRetention::Long, CacheRetention::Long);
    }

    #[test]
    fn test_thinking_level_enum() {
        assert_eq!(ThinkingLevel::Off, ThinkingLevel::Off);
        assert_eq!(ThinkingLevel::Minimal, ThinkingLevel::Minimal);
        assert_eq!(ThinkingLevel::Low, ThinkingLevel::Low);
        assert_eq!(ThinkingLevel::Medium, ThinkingLevel::Medium);
        assert_eq!(ThinkingLevel::High, ThinkingLevel::High);
        assert_eq!(ThinkingLevel::XHigh, ThinkingLevel::XHigh);
    }
}
