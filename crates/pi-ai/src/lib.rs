//! LLM 统一 API 层
#![warn(missing_docs)]
//!
//! pi-ai 提供统一的 LLM API 抽象层，支持多 Provider 流式调用。
//!
//! # 支持的 Provider
//!
//! - **Anthropic**: Claude 系列模型（原生 API）
//! - **OpenAI**: GPT 系列模型（ChatCompletions API）
//! - **Google**: Gemini 系列模型
//! - **Mistral**: Mistral 系列模型
//! - **Amazon Bedrock**: AWS Bedrock 托管模型
//! - **Azure OpenAI**: Azure 托管的 OpenAI 模型
//! - **XAI**: Grok 系列模型
//! - **OpenRouter**: OpenRouter 聚合服务
//!
//! # 核心功能
//!
//! - **流式 API**: 通过 [`stream()`] 函数获取实时响应流
//! - **非流式 API**: 通过 [`complete()`] 函数获取完整响应
//! - **Provider 注册**: 通过 [`ApiRegistry`] 管理多个 Provider 实现
//! - **Token 计数**: 提供 [`TokenCounter`] trait 和多种实现
//! - **模型管理**: 内置模型列表和成本计算
//!
//! # 示例
//!
//! ```ignore
//! use pi_ai::{stream, Context, StreamOptions};
//!
//! let context = Context::new(vec![Message::User(UserMessage::new("Hello"))]);
//! let model = pi_ai::get_model("claude-sonnet-4-20250514").unwrap();
//! let options = StreamOptions::default();
//!
//! let mut event_stream = stream(&context, &model, &options).await?;
//! while let Some(event) = event_stream.next().await {
//!     // 处理流式事件
//! }
//! ```

/// 类型定义模块
pub mod types;
/// API 注册表模块
pub mod api_registry;
/// 流式 API 模块
pub mod stream;
/// 模型定义模块
pub mod models;
/// Provider 实现模块
pub mod providers;
/// 工具模块
pub mod utils;
/// Token 计数器模块
pub mod token_counter;
/// 重试策略模块
pub mod retry;

#[cfg(test)]
pub mod test_fixtures;

// 重导出核心类型，方便直接使用
pub use types::{
    Api, AssistantMessage, AssistantMessageEvent, CacheRetention, ContentBlock, Context,
    DoneReason, ErrorReason, ImageContent, InputModality, Message, Model, ModelCost, Provider,
    SimpleStreamOptions, StopReason, StreamOptions, TextContent, ThinkingBudgets, ThinkingContent,
    ThinkingLevel, Tool, ToolCall, ToolResultMessage, Transport, Usage, UserContent, UserMessage,
};

// 重导出 API 注册表
pub use api_registry::{
    ApiProvider, ApiRegistry, 
    register_api_provider, get_api_provider, has_api_provider,
    get_all_api_providers, clear_api_providers, resolve_api_provider,
};

// 重导出流式 API
pub use stream::{
    stream, stream_simple, complete, complete_simple,
    stream_by_model_id, complete_by_model_id, stream_with_retry,
};

// 重导出重试模块
pub use retry::{RetryConfig, RetryPolicy, StreamRecoveryState, StreamRecoveryStats};

// 重导出模型相关函数
pub use models::{
    get_model, get_models, get_models_by_provider, get_models_by_api,
    calculate_cost, supports_xhigh, models_are_equal,
    get_api_key_from_env, get_api_key_env_var,
};

// 重导出工具模块
pub use utils::{
    event_stream::{SseEvent, SseParser, parse_sse_line, parse_json_stream_events},
    json_parse::{parse_partial_json, IncrementalJsonParser, StreamingJsonParser},
};

// 重导出 token 计数器
pub use token_counter::{TokenCounter, EstimateTokenCounter, ModelTokenCounter};

/// 初始化并注册所有内置 Provider
///
/// 在应用启动时调用此函数，将 Anthropic、OpenAI、Google 三个 Provider
/// 注册到全局 ApiRegistry 中。重复调用是安全的（会跳过已注册的情况）。
pub fn init_providers() {
    use std::sync::Arc;

    // 避免重复注册
    if has_api_provider(&Api::Anthropic) {
        return;
    }

    register_api_provider(Arc::new(providers::AnthropicProvider::new()));
    register_api_provider(Arc::new(providers::OpenAiProvider::new()));
    register_api_provider(Arc::new(providers::GoogleProvider::new()));
    register_api_provider(Arc::new(providers::MistralProvider::new()));
    register_api_provider(Arc::new(providers::BedrockProvider::new()));
    register_api_provider(Arc::new(providers::AzureOpenAiProvider::new()));
    register_api_provider(Arc::new(providers::XaiProvider::new()));
    register_api_provider(Arc::new(providers::OpenRouterProvider::new()));
    register_api_provider(Arc::new(providers::GroqProvider::new()));
    register_api_provider(Arc::new(providers::CerebrasProvider::new()));
    register_api_provider(Arc::new(providers::MinimaxProvider::new()));
    register_api_provider(Arc::new(providers::HuggingfaceProvider::new()));
    register_api_provider(Arc::new(providers::MoonshotProvider::new()));
    register_api_provider(Arc::new(providers::OpencodeProvider::new()));
    register_api_provider(Arc::new(providers::DeepSeekProvider::new()));
    register_api_provider(Arc::new(providers::QwenProvider::new()));
    register_api_provider(Arc::new(providers::VertexAiProvider::new()));
    register_api_provider(Arc::new(providers::GeminiCliProvider::new()));
    register_api_provider(Arc::new(providers::GithubCopilotProvider::new()));

    tracing::debug!("Registered 19 built-in providers: Anthropic, OpenAI(ChatCompletions), Google, Mistral, Bedrock, AzureOpenAI, XAI, OpenRouter, Groq, Cerebras, Minimax, Huggingface, Moonshot, Opencode, DeepSeek, Qwen, VertexAI, GeminiCli, GithubCopilot");
}
