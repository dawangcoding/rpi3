//! 模型注册表
//!
//! 管理内置的 LLM 模型定义和查询

use std::collections::HashMap;
use std::sync::OnceLock;
use crate::types::*;

/// 模型成本（每百万 token 的美元价格）
/// 
/// 定义模型的定价信息
#[derive(Debug, Clone, Copy)]
pub struct ModelCost {
    /// 输入 token 成本（$/million）
    pub input: f64,
    /// 输出 token 成本（$/million）
    pub output: f64,
    /// 缓存读取成本（$/million）
    pub cache_read: Option<f64>,
    /// 缓存写入成本（$/million）
    pub cache_write: Option<f64>,
}

impl Default for ModelCost {
    fn default() -> Self {
        Self {
            input: 0.0,
            output: 0.0,
            cache_read: None,
            cache_write: None,
        }
    }
}

impl From<ModelCost> for crate::types::ModelCost {
    fn from(cost: ModelCost) -> Self {
        Self {
            input: cost.input,
            output: cost.output,
            cache_read: cost.cache_read,
            cache_write: cost.cache_write,
        }
    }
}

/// 注册内置模型
fn builtin_models() -> Vec<Model> {
    vec![
        // ==================== Anthropic ====================
        Model {
            id: "claude-sonnet-4-20250514".to_string(),
            name: "Claude Sonnet 4".to_string(),
            api: Api::Anthropic,
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: Some(0.3),
                cache_write: Some(3.75),
            }.into(),
            context_window: 200000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "claude-3-5-sonnet-20241022".to_string(),
            name: "Claude 3.5 Sonnet".to_string(),
            api: Api::Anthropic,
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: Some(0.3),
                cache_write: Some(3.75),
            }.into(),
            context_window: 200000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },
        Model {
            id: "claude-opus-4-20250514".to_string(),
            name: "Claude Opus 4".to_string(),
            api: Api::Anthropic,
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 15.0,
                output: 75.0,
                cache_read: Some(1.5),
                cache_write: Some(18.75),
            }.into(),
            context_window: 200000,
            max_tokens: 32000,
            headers: None,
            compat: None,
        },
        Model {
            id: "claude-3-7-sonnet-20250219".to_string(),
            name: "Claude 3.7 Sonnet".to_string(),
            api: Api::Anthropic,
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: Some(0.3),
                cache_write: Some(3.75),
            }.into(),
            context_window: 200000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },
        
        // ==================== OpenAI ====================
        Model {
            id: "gpt-4o".to_string(),
            name: "GPT-4o".to_string(),
            api: Api::OpenAiChatCompletions,
            provider: Provider::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 2.5,
                output: 10.0,
                cache_read: Some(1.25),
                cache_write: Some(2.5),
            }.into(),
            context_window: 128000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "gpt-4o-mini".to_string(),
            name: "GPT-4o Mini".to_string(),
            api: Api::OpenAiChatCompletions,
            provider: Provider::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.15,
                output: 0.6,
                cache_read: Some(0.075),
                cache_write: Some(0.15),
            }.into(),
            context_window: 128000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "o3-mini".to_string(),
            name: "o3-mini".to_string(),
            api: Api::OpenAiChatCompletions,
            provider: Provider::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 1.1,
                output: 4.4,
                cache_read: Some(0.55),
                cache_write: Some(1.1),
            }.into(),
            context_window: 200000,
            max_tokens: 100000,
            headers: None,
            compat: None,
        },
        Model {
            id: "o1".to_string(),
            name: "o1".to_string(),
            api: Api::OpenAiChatCompletions,
            provider: Provider::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 15.0,
                output: 60.0,
                cache_read: Some(7.5),
                cache_write: Some(15.0),
            }.into(),
            context_window: 200000,
            max_tokens: 100000,
            headers: None,
            compat: None,
        },
        
        // ==================== Google ====================
        Model {
            id: "gemini-2.5-pro-preview-05-06".to_string(),
            name: "Gemini 2.5 Pro".to_string(),
            api: Api::Google,
            provider: Provider::Google,
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 1.25,
                output: 10.0,
                cache_read: Some(0.315),
                cache_write: Some(1.25),
            }.into(),
            context_window: 1048576,
            max_tokens: 65536,
            headers: None,
            compat: None,
        },
        Model {
            id: "gemini-2.5-flash-preview-04-17".to_string(),
            name: "Gemini 2.5 Flash".to_string(),
            api: Api::Google,
            provider: Provider::Google,
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.15,
                output: 0.6,
                cache_read: Some(0.0375),
                cache_write: Some(0.15),
            }.into(),
            context_window: 1048576,
            max_tokens: 65536,
            headers: None,
            compat: None,
        },
        Model {
            id: "gemini-2.0-flash".to_string(),
            name: "Gemini 2.0 Flash".to_string(),
            api: Api::Google,
            provider: Provider::Google,
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.1,
                output: 0.4,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 1048576,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },

        // ==================== Google Vertex AI ====================
        Model {
            id: "vertex/gemini-2.5-pro-preview-05-06".to_string(),
            name: "Gemini 2.5 Pro (Vertex AI)".to_string(),
            api: Api::GoogleVertex,
            provider: Provider::GoogleVertex,
            base_url: "https://us-central1-aiplatform.googleapis.com".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 1.25,
                output: 10.0,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 1048576,
            max_tokens: 65536,
            headers: None,
            compat: None,
        },
        Model {
            id: "vertex/gemini-2.0-flash".to_string(),
            name: "Gemini 2.0 Flash (Vertex AI)".to_string(),
            api: Api::GoogleVertex,
            provider: Provider::GoogleVertex,
            base_url: "https://us-central1-aiplatform.googleapis.com".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.1,
                output: 0.4,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 1048576,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },

        // ==================== Mistral ====================
        Model {
            id: "mistral-large-latest".to_string(),
            name: "Mistral Large".to_string(),
            api: Api::Mistral,
            provider: Provider::Mistral,
            base_url: "https://api.mistral.ai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 2.0,
                output: 6.0,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 128000,
            max_tokens: 4096,
            headers: None,
            compat: None,
        },
        Model {
            id: "mistral-small-latest".to_string(),
            name: "Mistral Small".to_string(),
            api: Api::Mistral,
            provider: Provider::Mistral,
            base_url: "https://api.mistral.ai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.1,
                output: 0.3,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 128000,
            max_tokens: 4096,
            headers: None,
            compat: None,
        },
        Model {
            id: "codestral-latest".to_string(),
            name: "Codestral".to_string(),
            api: Api::Mistral,
            provider: Provider::Mistral,
            base_url: "https://api.mistral.ai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost {
                input: 0.3,
                output: 0.9,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 256000,
            max_tokens: 4096,
            headers: None,
            compat: None,
        },

        // ==================== Amazon Bedrock ====================
        Model {
            id: "anthropic.claude-3-5-sonnet-20241022-v2:0".to_string(),
            name: "Claude 3.5 Sonnet (Bedrock)".to_string(),
            api: Api::AmazonBedrock,
            provider: Provider::AmazonBedrock,
            base_url: "bedrock://us-east-1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: Some(0.3),
                cache_write: Some(3.75),
            }.into(),
            context_window: 200000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },
        Model {
            id: "anthropic.claude-3-opus-20240229-v1:0".to_string(),
            name: "Claude 3 Opus (Bedrock)".to_string(),
            api: Api::AmazonBedrock,
            provider: Provider::AmazonBedrock,
            base_url: "bedrock://us-east-1".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 15.0,
                output: 75.0,
                cache_read: Some(1.5),
                cache_write: Some(18.75),
            }.into(),
            context_window: 200000,
            max_tokens: 4096,
            headers: None,
            compat: None,
        },
        Model {
            id: "anthropic.claude-3-haiku-20240307-v1:0".to_string(),
            name: "Claude 3 Haiku (Bedrock)".to_string(),
            api: Api::AmazonBedrock,
            provider: Provider::AmazonBedrock,
            base_url: "bedrock://us-east-1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.25,
                output: 1.25,
                cache_read: Some(0.03),
                cache_write: Some(0.3),
            }.into(),
            context_window: 200000,
            max_tokens: 4096,
            headers: None,
            compat: None,
        },
        Model {
            id: "anthropic.claude-3-5-haiku-20241022-v1:0".to_string(),
            name: "Claude 3.5 Haiku (Bedrock)".to_string(),
            api: Api::AmazonBedrock,
            provider: Provider::AmazonBedrock,
            base_url: "bedrock://us-east-1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.8,
                output: 4.0,
                cache_read: Some(0.08),
                cache_write: Some(1.0),
            }.into(),
            context_window: 200000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },

        // ==================== Azure OpenAI ====================
        Model {
            id: "azure/gpt-4o".to_string(),
            name: "GPT-4o (Azure)".to_string(),
            api: Api::AzureOpenAiResponses,
            provider: Provider::AzureOpenAiResponses,
            base_url: "".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 2.5,
                output: 10.0,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 128000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "azure/gpt-4o-mini".to_string(),
            name: "GPT-4o Mini (Azure)".to_string(),
            api: Api::AzureOpenAiResponses,
            provider: Provider::AzureOpenAiResponses,
            base_url: "".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.15,
                output: 0.6,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 128000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "azure/o3-mini".to_string(),
            name: "o3-mini (Azure)".to_string(),
            api: Api::AzureOpenAiResponses,
            provider: Provider::AzureOpenAiResponses,
            base_url: "".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 1.1,
                output: 4.4,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 200000,
            max_tokens: 100000,
            headers: None,
            compat: None,
        },
        Model {
            id: "azure/o1".to_string(),
            name: "o1 (Azure)".to_string(),
            api: Api::AzureOpenAiResponses,
            provider: Provider::AzureOpenAiResponses,
            base_url: "".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 15.0,
                output: 60.0,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 200000,
            max_tokens: 100000,
            headers: None,
            compat: None,
        },

        // ==================== xAI ====================
        Model {
            id: "grok-3".to_string(),
            name: "Grok 3".to_string(),
            api: Api::Xai,
            provider: Provider::Xai,
            base_url: "https://api.x.ai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 131072,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "grok-3-mini".to_string(),
            name: "Grok 3 Mini".to_string(),
            api: Api::Xai,
            provider: Provider::Xai,
            base_url: "https://api.x.ai/v1".to_string(),
            reasoning: true,
            input: vec![InputModality::Text],
            cost: ModelCost {
                input: 0.3,
                output: 0.5,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 131072,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "grok-2-vision-1212".to_string(),
            name: "Grok 2 Vision".to_string(),
            api: Api::Xai,
            provider: Provider::Xai,
            base_url: "https://api.x.ai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 2.0,
                output: 10.0,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 32768,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },

        // ==================== OpenRouter ====================
        Model {
            id: "openrouter/auto".to_string(),
            name: "OpenRouter Auto".to_string(),
            api: Api::Openrouter,
            provider: Provider::Openrouter,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost::default().into(),
            context_window: 128000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "openrouter/anthropic/claude-sonnet-4-20250514".to_string(),
            name: "Claude Sonnet 4 (OpenRouter)".to_string(),
            api: Api::Openrouter,
            provider: Provider::Openrouter,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: Some(0.3),
                cache_write: Some(3.75),
            }.into(),
            context_window: 200000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "openrouter/google/gemini-2.5-flash-preview-04-17".to_string(),
            name: "Gemini 2.5 Flash (OpenRouter)".to_string(),
            api: Api::Openrouter,
            provider: Provider::Openrouter,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.15,
                output: 0.6,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 1048576,
            max_tokens: 65536,
            headers: None,
            compat: None,
        },
        Model {
            id: "openrouter/openai/gpt-4o".to_string(),
            name: "GPT-4o (OpenRouter)".to_string(),
            api: Api::Openrouter,
            provider: Provider::Openrouter,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 2.5,
                output: 10.0,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 128000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "openrouter/meta-llama/llama-4-maverick".to_string(),
            name: "Llama 4 Maverick (OpenRouter)".to_string(),
            api: Api::Openrouter,
            provider: Provider::Openrouter,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost {
                input: 0.25,
                output: 1.0,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 1048576,
            max_tokens: 65536,
            headers: None,
            compat: None,
        },
        Model {
            id: "openrouter/deepseek/deepseek-r1".to_string(),
            name: "DeepSeek R1 (OpenRouter)".to_string(),
            api: Api::Openrouter,
            provider: Provider::Openrouter,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            reasoning: true,
            input: vec![InputModality::Text],
            cost: ModelCost {
                input: 0.55,
                output: 2.19,
                cache_read: None,
                cache_write: None,
            }.into(),
            context_window: 163840,
            max_tokens: 65536,
            headers: None,
            compat: None,
        },

        // ==================== Groq ====================
        Model {
            id: "llama-3.3-70b-versatile".to_string(),
            name: "Llama 3.3 70B Versatile".to_string(),
            api: Api::Groq,
            provider: Provider::Groq,
            base_url: "https://api.groq.com/openai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.59, output: 0.79, cache_read: None, cache_write: None }.into(),
            context_window: 128000,
            max_tokens: 32768,
            headers: None,
            compat: None,
        },
        Model {
            id: "llama-3.1-8b-instant".to_string(),
            name: "Llama 3.1 8B Instant".to_string(),
            api: Api::Groq,
            provider: Provider::Groq,
            base_url: "https://api.groq.com/openai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.05, output: 0.08, cache_read: None, cache_write: None }.into(),
            context_window: 131072,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },
        Model {
            id: "mixtral-8x7b-32768".to_string(),
            name: "Mixtral 8x7B".to_string(),
            api: Api::Groq,
            provider: Provider::Groq,
            base_url: "https://api.groq.com/openai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.24, output: 0.24, cache_read: None, cache_write: None }.into(),
            context_window: 32768,
            max_tokens: 32768,
            headers: None,
            compat: None,
        },

        // ==================== Cerebras ====================
        Model {
            id: "llama3.1-8b".to_string(),
            name: "Llama 3.1 8B (Cerebras)".to_string(),
            api: Api::Cerebras,
            provider: Provider::Cerebras,
            base_url: "https://api.cerebras.ai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.10, output: 0.10, cache_read: None, cache_write: None }.into(),
            context_window: 128000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },
        Model {
            id: "llama3.1-70b".to_string(),
            name: "Llama 3.1 70B (Cerebras)".to_string(),
            api: Api::Cerebras,
            provider: Provider::Cerebras,
            base_url: "https://api.cerebras.ai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.60, output: 0.60, cache_read: None, cache_write: None }.into(),
            context_window: 128000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },

        // ==================== DeepSeek ====================
        Model {
            id: "deepseek-chat".to_string(),
            name: "DeepSeek V3".to_string(),
            api: Api::DeepSeek,
            provider: Provider::DeepSeek,
            base_url: "https://api.deepseek.com/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.27, output: 1.1, cache_read: None, cache_write: None }.into(),
            context_window: 128000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },
        Model {
            id: "deepseek-reasoner".to_string(),
            name: "DeepSeek R1".to_string(),
            api: Api::DeepSeek,
            provider: Provider::DeepSeek,
            base_url: "https://api.deepseek.com/v1".to_string(),
            reasoning: true,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.55, output: 2.19, cache_read: None, cache_write: None }.into(),
            context_window: 128000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },

        // ==================== Qwen ====================
        Model {
            id: "qwen-max".to_string(),
            name: "Qwen Max".to_string(),
            api: Api::Qwen,
            provider: Provider::Qwen,
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 2.4, output: 9.6, cache_read: None, cache_write: None }.into(),
            context_window: 131072,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },
        Model {
            id: "qwen-plus".to_string(),
            name: "Qwen Plus".to_string(),
            api: Api::Qwen,
            provider: Provider::Qwen,
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.8, output: 2.0, cache_read: None, cache_write: None }.into(),
            context_window: 131072,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },

        // ==================== Minimax ====================
        Model {
            id: "abab6.5s-chat".to_string(),
            name: "ABAB 6.5S Chat".to_string(),
            api: Api::Minimax,
            provider: Provider::Minimax,
            base_url: "https://api.minimax.chat/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 1.0, output: 1.0, cache_read: None, cache_write: None }.into(),
            context_window: 245760,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },

        // ==================== Moonshot (Kimi) ====================
        Model {
            id: "moonshot-v1-8k".to_string(),
            name: "Moonshot V1 8K".to_string(),
            api: Api::KimiCoding,
            provider: Provider::KimiCoding,
            base_url: "https://api.moonshot.cn/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 1.0, output: 1.0, cache_read: None, cache_write: None }.into(),
            context_window: 8192,
            max_tokens: 4096,
            headers: None,
            compat: None,
        },
        Model {
            id: "moonshot-v1-128k".to_string(),
            name: "Moonshot V1 128K".to_string(),
            api: Api::KimiCoding,
            provider: Provider::KimiCoding,
            base_url: "https://api.moonshot.cn/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 1.0, output: 1.0, cache_read: None, cache_write: None }.into(),
            context_window: 128000,
            max_tokens: 4096,
            headers: None,
            compat: None,
        },

        // ==================== Huggingface ====================
        Model {
            id: "meta-llama/Llama-3.1-8B-Instruct".to_string(),
            name: "Llama 3.1 8B Instruct (Huggingface)".to_string(),
            api: Api::Huggingface,
            provider: Provider::Huggingface,
            base_url: "https://api-inference.huggingface.co/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.0, output: 0.0, cache_read: None, cache_write: None }.into(),
            context_window: 128000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },

        // ==================== Opencode ====================
        Model {
            id: "opencode-latest".to_string(),
            name: "Opencode Latest".to_string(),
            api: Api::Opencode,
            provider: Provider::Opencode,
            base_url: "https://api.opencode.ai/v1".to_string(),
            reasoning: false,
            input: vec![InputModality::Text],
            cost: ModelCost { input: 0.0, output: 0.0, cache_read: None, cache_write: None }.into(),
            context_window: 128000,
            max_tokens: 8192,
            headers: None,
            compat: None,
        },

        // ==================== GitHub Copilot ====================
        Model {
            id: "copilot/gpt-4o".to_string(),
            name: "GPT-4o (Copilot)".to_string(),
            api: Api::Other("github-copilot".to_string()),
            provider: Provider::GithubCopilot,
            base_url: "https://api.githubcopilot.com".to_string(),
            reasoning: false,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost { input: 0.0, output: 0.0, cache_read: None, cache_write: None }.into(),
            context_window: 128000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
        Model {
            id: "copilot/claude-sonnet-4".to_string(),
            name: "Claude Sonnet 4 (Copilot)".to_string(),
            api: Api::Other("github-copilot".to_string()),
            provider: Provider::GithubCopilot,
            base_url: "https://api.githubcopilot.com".to_string(),
            reasoning: true,
            input: vec![InputModality::Text, InputModality::Image],
            cost: ModelCost { input: 0.0, output: 0.0, cache_read: None, cache_write: None }.into(),
            context_window: 200000,
            max_tokens: 16384,
            headers: None,
            compat: None,
        },
    ]
}

// 全局模型注册表
static MODEL_REGISTRY: OnceLock<HashMap<String, Model>> = OnceLock::new();

/// 初始化模型注册表
fn init_model_registry() -> HashMap<String, Model> {
    let models = builtin_models();
    let mut registry = HashMap::with_capacity(models.len());
    
    for model in models {
        registry.insert(model.id.clone(), model);
    }
    
    registry
}

/// 获取模型注册表
fn get_registry() -> &'static HashMap<String, Model> {
    MODEL_REGISTRY.get_or_init(init_model_registry)
}

/// 通过模型 ID 获取模型
/// 
/// 从模型注册表中查找指定 ID 的模型
pub fn get_model(id: &str) -> Option<Model> {
    get_registry().get(id).cloned()
}

/// 获取所有模型
/// 
/// 返回注册表中所有可用的模型
pub fn get_models() -> Vec<Model> {
    get_registry().values().cloned().collect()
}

/// 根据 provider 获取模型
/// 
/// 筛选指定提供商的模型列表
pub fn get_models_by_provider(provider: &Provider) -> Vec<Model> {
    get_registry()
        .values()
        .filter(|m| &m.provider == provider)
        .cloned()
        .collect()
}

/// 根据 API 类型获取模型
/// 
/// 筛选指定 API 类型的模型列表
pub fn get_models_by_api(api: &Api) -> Vec<Model> {
    get_registry()
        .values()
        .filter(|m| &m.api == api)
        .cloned()
        .collect()
}

/// 计算成本（返回美元）
/// 
/// 根据 token 使用量计算 API 调用成本
pub fn calculate_cost(model: &Model, usage: &Usage) -> f64 {
    let input_cost = (model.cost.input / 1_000_000.0) * usage.input_tokens as f64;
    let output_cost = (model.cost.output / 1_000_000.0) * usage.output_tokens as f64;
    
    let cache_read_cost = usage.cache_read_tokens.map(|tokens| {
        model.cost.cache_read.unwrap_or(0.0) / 1_000_000.0 * tokens as f64
    }).unwrap_or(0.0);
    
    let cache_write_cost = usage.cache_write_tokens.map(|tokens| {
        model.cost.cache_write.unwrap_or(0.0) / 1_000_000.0 * tokens as f64
    }).unwrap_or(0.0);
    
    input_cost + output_cost + cache_read_cost + cache_write_cost
}

/// 检查模型是否支持 xhigh thinking level
/// 
/// 判断模型是否支持最高级别的思考模式
pub fn supports_xhigh(model: &Model) -> bool {
    let id = &model.id;
    id.contains("gpt-5.2") || id.contains("gpt-5.3") || id.contains("gpt-5.4")
        || id.contains("opus-4-6") || id.contains("opus-4.6")
}

/// 检查两个模型是否相等
/// 
/// 比较两个模型的 ID 和提供商
pub fn models_are_equal(a: Option<&Model>, b: Option<&Model>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.id == b.id && a.provider == b.provider,
        _ => false,
    }
}

/// 从环境变量获取 API Key
/// 
/// 根据提供商从环境变量读取 API 密钥
pub fn get_api_key_from_env(provider: &Provider) -> Option<String> {
    use std::env;
    
    match provider {
        Provider::Anthropic => {
            env::var("ANTHROPIC_OAUTH_TOKEN")
                .ok()
                .or_else(|| env::var("ANTHROPIC_API_KEY").ok())
        }
        Provider::Openai => env::var("OPENAI_API_KEY").ok(),
        Provider::Google | Provider::GoogleGeminiCli | Provider::GoogleAntigravity => {
            env::var("GOOGLE_API_KEY")
                .ok()
                .or_else(|| env::var("GEMINI_API_KEY").ok())
        }
        Provider::GoogleVertex => {
            env::var("GOOGLE_CLOUD_API_KEY").ok()
        }
        Provider::Groq => env::var("GROQ_API_KEY").ok(),
        Provider::Cerebras => env::var("CEREBRAS_API_KEY").ok(),
        Provider::Xai => env::var("XAI_API_KEY").ok(),
        Provider::Openrouter => env::var("OPENROUTER_API_KEY").ok(),
        Provider::VercelAiGateway => env::var("AI_GATEWAY_API_KEY").ok(),
        Provider::Mistral => env::var("MISTRAL_API_KEY").ok(),
        Provider::Minimax => env::var("MINIMAX_API_KEY").ok(),
        Provider::MinimaxCn => env::var("MINIMAX_CN_API_KEY").ok(),
        Provider::Huggingface => env::var("HF_TOKEN").ok(),
        Provider::Opencode | Provider::OpencodeGo => env::var("OPENCODE_API_KEY").ok(),
        Provider::KimiCoding => env::var("KIMI_API_KEY").ok(),
        Provider::AzureOpenAiResponses => env::var("AZURE_OPENAI_API_KEY").ok(),
        Provider::OpenAiCodex => env::var("OPENAI_CODEX_API_KEY").ok(),
        Provider::GithubCopilot => {
            env::var("COPILOT_GITHUB_TOKEN")
                .ok()
                .or_else(|| env::var("GH_TOKEN").ok())
                .or_else(|| env::var("GITHUB_TOKEN").ok())
        }
        Provider::Zai => env::var("ZAI_API_KEY").ok(),
        Provider::DeepSeek => env::var("DEEPSEEK_API_KEY").ok(),
        Provider::Qwen => env::var("DASHSCOPE_API_KEY").ok(),
        Provider::AmazonBedrock => {
            // Amazon Bedrock 使用 AWS 凭证
            if env::var("AWS_PROFILE").is_ok()
                || (env::var("AWS_ACCESS_KEY_ID").is_ok() && env::var("AWS_SECRET_ACCESS_KEY").is_ok())
                || env::var("AWS_BEARER_TOKEN_BEDROCK").is_ok()
                || env::var("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI").is_ok()
                || env::var("AWS_CONTAINER_CREDENTIALS_FULL_URI").is_ok()
                || env::var("AWS_WEB_IDENTITY_TOKEN_FILE").is_ok()
            {
                Some("<authenticated>".to_string())
            } else {
                None
            }
        }
        Provider::Other(_) => None,
    }
}

/// 获取指定 provider 的 API key 环境变量名称
/// 
/// 返回提供商对应的环境变量名
pub fn get_api_key_env_var(provider: &Provider) -> Option<&'static str> {
    match provider {
        Provider::Anthropic => Some("ANTHROPIC_API_KEY"),
        Provider::Openai => Some("OPENAI_API_KEY"),
        Provider::Google | Provider::GoogleGeminiCli | Provider::GoogleAntigravity => Some("GEMINI_API_KEY"),
        Provider::GoogleVertex => Some("GOOGLE_CLOUD_API_KEY"),
        Provider::Groq => Some("GROQ_API_KEY"),
        Provider::Cerebras => Some("CEREBRAS_API_KEY"),
        Provider::Xai => Some("XAI_API_KEY"),
        Provider::Openrouter => Some("OPENROUTER_API_KEY"),
        Provider::VercelAiGateway => Some("AI_GATEWAY_API_KEY"),
        Provider::Mistral => Some("MISTRAL_API_KEY"),
        Provider::Minimax => Some("MINIMAX_API_KEY"),
        Provider::MinimaxCn => Some("MINIMAX_CN_API_KEY"),
        Provider::Huggingface => Some("HF_TOKEN"),
        Provider::Opencode | Provider::OpencodeGo => Some("OPENCODE_API_KEY"),
        Provider::KimiCoding => Some("KIMI_API_KEY"),
        Provider::AzureOpenAiResponses => Some("AZURE_OPENAI_API_KEY"),
        Provider::OpenAiCodex => Some("OPENAI_CODEX_API_KEY"),
        Provider::GithubCopilot => Some("GITHUB_TOKEN"),
        Provider::Zai => Some("ZAI_API_KEY"),
        Provider::AmazonBedrock => Some("AWS_ACCESS_KEY_ID"),
        Provider::DeepSeek => Some("DEEPSEEK_API_KEY"),
        Provider::Qwen => Some("DASHSCOPE_API_KEY"),
        Provider::Other(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_model_existing() {
        // 测试已知模型
        let model = get_model("claude-sonnet-4-20250514");
        assert!(model.is_some());
        let model = model.unwrap();
        assert_eq!(model.id, "claude-sonnet-4-20250514");
        assert_eq!(model.provider, Provider::Anthropic);
        assert_eq!(model.api, Api::Anthropic);
    }

    #[test]
    fn test_get_model_not_found() {
        let model = get_model("nonexistent-model-xyz");
        assert!(model.is_none());
    }

    #[test]
    fn test_get_models_non_empty() {
        let models = get_models();
        assert!(!models.is_empty());
        // 验证至少有一些主要模型
        assert!(models.iter().any(|m| m.provider == Provider::Anthropic));
        assert!(models.iter().any(|m| m.provider == Provider::Openai));
        assert!(models.iter().any(|m| m.provider == Provider::Google));
    }

    #[test]
    fn test_model_id_format_validation() {
        let models = get_models();
        
        for model in &models {
            // ID 不应为空
            assert!(!model.id.is_empty(), "Model ID should not be empty");
            
            // ID 不应包含空格
            assert!(!model.id.contains(' '), "Model ID {} should not contain spaces", model.id);
            
            // ID 应只包含有效字符
            assert!(
                model.id.chars().all(|c| {
                    c.is_ascii_alphanumeric() ||
                    c == '-' || c == '_' || c == '.' || c == ':' || c == '/'
                }),
                "Model ID {} contains invalid characters",
                model.id
            );
        }
    }

    #[test]
    fn test_model_id_uniqueness() {
        let models = get_models();
        let mut ids = std::collections::HashSet::new();
        
        for model in &models {
            assert!(ids.insert(&model.id), "Model ID {} should be unique", model.id);
        }
    }

    #[test]
    fn test_get_models_by_provider_anthropic() {
        let models = get_models_by_provider(&Provider::Anthropic);
        assert!(!models.is_empty());
        
        for model in &models {
            assert_eq!(model.provider, Provider::Anthropic);
        }
    }

    #[test]
    fn test_get_models_by_provider_openai() {
        let models = get_models_by_provider(&Provider::Openai);
        assert!(!models.is_empty());
        
        for model in &models {
            assert_eq!(model.provider, Provider::Openai);
        }
    }

    #[test]
    fn test_get_models_by_provider_google() {
        let models = get_models_by_provider(&Provider::Google);
        assert!(!models.is_empty());
        
        for model in &models {
            assert_eq!(model.provider, Provider::Google);
        }
    }

    #[test]
    fn test_get_models_by_api_anthropic() {
        let models = get_models_by_api(&Api::Anthropic);
        assert!(!models.is_empty());
        
        for model in &models {
            assert_eq!(model.api, Api::Anthropic);
        }
    }

    #[test]
    fn test_get_models_by_api_openai_chat() {
        let models = get_models_by_api(&Api::OpenAiChatCompletions);
        assert!(!models.is_empty());
        
        for model in &models {
            assert_eq!(model.api, Api::OpenAiChatCompletions);
        }
    }

    #[test]
    fn test_calculate_cost_basic() {
        let model = get_model("claude-sonnet-4-20250514").unwrap();
        let usage = Usage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: None,
            cache_write_tokens: None,
        };
        
        let cost = calculate_cost(&model, &usage);
        
        // Claude Sonnet 4: input $3/M, output $15/M
        assert!(cost > 0.0);
        assert!(cost < 1.0); // 合理范围
    }

    #[test]
    fn test_calculate_cost_with_cache() {
        let model = get_model("claude-sonnet-4-20250514").unwrap();
        let usage = Usage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: Some(500),
            cache_write_tokens: Some(200),
        };
        
        let cost_with_cache = calculate_cost(&model, &usage);
        assert!(cost_with_cache > 0.0);
    }

    #[test]
    fn test_calculate_cost_zero_usage() {
        let model = get_model("claude-sonnet-4-20250514").unwrap();
        let usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: None,
            cache_write_tokens: None,
        };
        
        let cost = calculate_cost(&model, &usage);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_model_context_window_valid() {
        let models = get_models();
        
        for model in &models {
            assert!(model.context_window > 0, "Model {} should have positive context_window", model.id);
            assert!(model.max_tokens > 0, "Model {} should have positive max_tokens", model.id);
            assert!(model.max_tokens <= model.context_window, 
                "Model {} max_tokens ({}) should not exceed context_window ({})", 
                model.id, model.max_tokens, model.context_window);
        }
    }

    #[test]
    fn test_model_input_modalities() {
        let models = get_models();
        
        for model in &models {
            assert!(!model.input.is_empty(), "Model {} should have at least one input modality", model.id);
            assert!(model.input.contains(&InputModality::Text), 
                "Model {} should support text input", model.id);
        }
    }

    #[test]
    fn test_supports_xhigh() {
        // 创建测试模型
        let gpt52 = Model {
            id: "gpt-5.2-turbo".to_string(),
            name: "GPT-5.2".to_string(),
            api: Api::OpenAiChatCompletions,
            provider: Provider::Openai,
            base_url: "".to_string(),
            reasoning: true,
            input: vec![InputModality::Text],
            cost: crate::types::ModelCost {
                input: 0.0,
                output: 0.0,
                cache_read: None,
                cache_write: None,
            },
            context_window: 100000,
            max_tokens: 10000,
            headers: None,
            compat: None,
        };
        assert!(supports_xhigh(&gpt52));
        
        let claude = get_model("claude-sonnet-4-20250514").unwrap();
        assert!(!supports_xhigh(&claude));
    }

    #[test]
    fn test_models_are_equal() {
        let model1 = get_model("claude-sonnet-4-20250514");
        let model2 = get_model("claude-sonnet-4-20250514");
        let model3 = get_model("gpt-4o");
        
        assert!(models_are_equal(model1.as_ref(), model2.as_ref()));
        assert!(!models_are_equal(model1.as_ref(), model3.as_ref()));
        assert!(!models_are_equal(None, model1.as_ref()));
        assert!(!models_are_equal(model1.as_ref(), None));
        // 两个 None 不算相等（函数定义如此）
        assert!(!models_are_equal(None::<&Model>, None::<&Model>));
    }

    #[test]
    fn test_get_api_key_env_var_mappings() {
        assert_eq!(get_api_key_env_var(&Provider::Anthropic), Some("ANTHROPIC_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::Openai), Some("OPENAI_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::Google), Some("GEMINI_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::GoogleGeminiCli), Some("GEMINI_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::Groq), Some("GROQ_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::Cerebras), Some("CEREBRAS_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::Xai), Some("XAI_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::Openrouter), Some("OPENROUTER_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::Mistral), Some("MISTRAL_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::DeepSeek), Some("DEEPSEEK_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::Qwen), Some("DASHSCOPE_API_KEY"));
        assert_eq!(get_api_key_env_var(&Provider::Other("custom".to_string())), None);
    }

    #[test]
    fn test_model_provider_association() {
        let models = get_models();
        
        for model in &models {
            match &model.provider {
                Provider::Anthropic => {
                    assert!(matches!(model.api, Api::Anthropic | Api::AnthropicMessages));
                }
                Provider::Openai => {
                    assert!(matches!(model.api, 
                        Api::OpenAiChatCompletions | 
                        Api::OpenAiCompletions | 
                        Api::OpenAiResponses |
                        Api::OpenAiCodexResponses
                    ));
                }
                Provider::Google => {
                    assert!(matches!(model.api, Api::Google | Api::GoogleGenerativeAi));
                }
                Provider::Groq => {
                    assert_eq!(model.api, Api::Groq);
                }
                Provider::Cerebras => {
                    assert_eq!(model.api, Api::Cerebras);
                }
                Provider::Xai => {
                    assert_eq!(model.api, Api::Xai);
                }
                Provider::Openrouter => {
                    assert_eq!(model.api, Api::Openrouter);
                }
                Provider::Mistral => {
                    assert!(matches!(model.api, Api::Mistral | Api::MistralConversations));
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_model_cost_default() {
        let cost = ModelCost::default();
        assert_eq!(cost.input, 0.0);
        assert_eq!(cost.output, 0.0);
        assert!(cost.cache_read.is_none());
        assert!(cost.cache_write.is_none());
    }

    #[test]
    fn test_model_cost_into_types() {
        let cost = ModelCost {
            input: 3.0,
            output: 15.0,
            cache_read: Some(0.3),
            cache_write: Some(3.75),
        };
        
        let types_cost: crate::types::ModelCost = cost.into();
        assert_eq!(types_cost.input, 3.0);
        assert_eq!(types_cost.output, 15.0);
        assert_eq!(types_cost.cache_read, Some(0.3));
        assert_eq!(types_cost.cache_write, Some(3.75));
    }

    #[test]
    fn test_model_base_url_non_empty_for_major_providers() {
        let models = get_models();
        
        for model in &models {
            match model.provider {
                Provider::Anthropic | 
                Provider::Openai | 
                Provider::Google | 
                Provider::Mistral | 
                Provider::Groq | 
                Provider::Cerebras | 
                Provider::Xai | 
                Provider::Openrouter => {
                    assert!(!model.base_url.is_empty(), 
                        "Model {} from provider {:?} should have a base_url", 
                        model.id, model.provider);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_get_models_by_provider_exhaustive() {
        let major_providers = vec![
            Provider::Anthropic,
            Provider::Openai,
            Provider::Google,
            Provider::Mistral,
            Provider::Groq,
            Provider::Cerebras,
            Provider::Xai,
            Provider::Openrouter,
            Provider::DeepSeek,
            Provider::Qwen,
        ];
        
        for provider in major_providers {
            let models = get_models_by_provider(&provider);
            assert!(!models.is_empty(), "Provider {:?} should have models", provider);
        }
    }
}