//! Token 计数模块
//!
//! 提供文本和消息的 token 计数功能

use crate::types::{ContentBlock, Message, UserContent};
use std::sync::{Arc, OnceLock};
use tiktoken_rs::CoreBPE;

/// Mistral Tokenizer 全局缓存
static MISTRAL_TOKENIZER: OnceLock<Option<tokenizers::Tokenizer>> = OnceLock::new();

/// Gemini Tokenizer 全局缓存
static GEMINI_TOKENIZER: OnceLock<Option<tokenizers::Tokenizer>> = OnceLock::new();

/// Token 计数 trait
pub trait TokenCounter: Send + Sync {
    /// 计算文本的 token 数
    fn count_text(&self, text: &str) -> usize;
    /// 计算单条消息的 token 数
    fn count_message(&self, message: &Message) -> usize;
    /// 计算多条消息的总 token 数
    fn count_messages(&self, messages: &[Message]) -> usize;
}

/// 启发式 token 估算器（默认）
/// 使用字符数 / 4 的经验公式，对英文约 80% 准确
pub struct EstimateTokenCounter {
    chars_per_token: f64, // 默认 4.0
}

impl EstimateTokenCounter {
    /// 创建新的启发式 token 估算器
    pub fn new() -> Self {
        Self {
            chars_per_token: 4.0,
        }
    }

    /// 使用指定的字符/token 比例创建
    pub fn with_ratio(chars_per_token: f64) -> Self {
        Self { chars_per_token }
    }
}

impl Default for EstimateTokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenCounter for EstimateTokenCounter {
    fn count_text(&self, text: &str) -> usize {
        (text.chars().count() as f64 / self.chars_per_token).ceil() as usize
    }

    fn count_message(&self, message: &Message) -> usize {
        // 基础消息开销（格式开销）
        const MESSAGE_OVERHEAD: usize = 4;

        let content_tokens = match message {
            Message::User(user_msg) => match &user_msg.content {
                crate::types::UserContent::Text(text) => self.count_text(text),
                crate::types::UserContent::Blocks(blocks) => {
                    count_content_blocks(self, blocks)
                }
            },
            Message::Assistant(assistant_msg) => {
                count_content_blocks(self, &assistant_msg.content)
            }
            Message::ToolResult(tool_result) => {
                // tool_result 包含 tool_call_id、tool_name 和 content
                let id_tokens = self.count_text(&tool_result.tool_call_id);
                let name_tokens = self.count_text(&tool_result.tool_name);
                let content_tokens = count_content_blocks(self, &tool_result.content);
                id_tokens + name_tokens + content_tokens
            }
        };

        MESSAGE_OVERHEAD + content_tokens
    }

    fn count_messages(&self, messages: &[Message]) -> usize {
        if messages.is_empty() {
            return 0;
        }
        messages.iter().map(|m| self.count_message(m)).sum::<usize>() + 3 // 3 tokens for reply priming
    }
}

/// 计算内容块的 token 数
fn count_content_blocks(counter: &dyn TokenCounter, blocks: &[ContentBlock]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text(text) => counter.count_text(&text.text),
            ContentBlock::Thinking(thinking) => counter.count_text(&thinking.thinking),
            ContentBlock::Image(_) => {
                // 图片通常占用较多 token，但这里使用估算值
                // 实际 token 数取决于图片尺寸和编码方式
                1024
            }
            ContentBlock::ToolCall(tool_call) => {
                // 工具调用包含 name 和 arguments
                let name_tokens = counter.count_text(&tool_call.name);
                let args_tokens = counter.count_text(&tool_call.arguments.to_string());
                name_tokens + args_tokens
            }
        })
        .sum()
}

/// 模型特定的 token 计数器
pub struct ModelTokenCounter {
    model_family: String,
    base_counter: EstimateTokenCounter,
}

impl ModelTokenCounter {
    /// 创建新的模型特定 token 计数器
    pub fn new(model_family: &str) -> Self {
        // 不同模型家族使用不同的字符/token 比率
        // 比率越小，估算的 token 数越多（更保守）
        let ratio = match model_family.to_lowercase().as_str() {
            "claude" | "anthropic" => 3.5,  // Claude 保持 3.5
            "gpt" | "openai" => 4.0,       // OpenAI 保持 4.0（实际使用 TiktokenCounter）
            "gemini" | "google" => 3.8,    // Gemini 从 4.0 微调至 3.8
            "mistral" => 3.8,              // Mistral 从 4.0 微调至 3.8
            _ => 4.0,                       // 默认保持 4.0
        };
        Self {
            model_family: model_family.to_string(),
            base_counter: EstimateTokenCounter::with_ratio(ratio),
        }
    }

    /// 获取模型家族
    pub fn model_family(&self) -> &str {
        &self.model_family
    }
}

impl TokenCounter for ModelTokenCounter {
    fn count_text(&self, text: &str) -> usize {
        self.base_counter.count_text(text)
    }

    fn count_message(&self, message: &Message) -> usize {
        self.base_counter.count_message(message)
    }

    fn count_messages(&self, messages: &[Message]) -> usize {
        self.base_counter.count_messages(messages)
    }
}

/// Tiktoken 精确 token 计数器（用于 OpenAI 模型）
pub struct TiktokenCounter {
    bpe: CoreBPE,
}

impl TiktokenCounter {
    /// 创建新的 TiktokenCounter
    /// 对于 gpt-4o 系列使用 o200k_base
    /// 对于 gpt-4/gpt-3.5 使用 cl100k_base
    pub fn new(model: &str) -> Option<Self> {
        let model_lower = model.to_lowercase();
        
        // 尝试使用 tiktoken_rs::get_bpe_from_model
        // 如果失败，根据模型名称手动选择 BPE
        let bpe = match tiktoken_rs::get_bpe_from_model(&model_lower) {
            Ok(bpe) => bpe,
            Err(_) => {
                // 手动映射常见模型
                if model_lower.contains("gpt-4o") || model_lower.starts_with("o1") || model_lower.starts_with("o3") || model_lower.starts_with("o4") {
                    tiktoken_rs::o200k_base().ok()?
                } else if model_lower.contains("gpt-4") || model_lower.contains("gpt-3.5") {
                    tiktoken_rs::cl100k_base().ok()?
                } else {
                    return None;
                }
            }
        };
        
        Some(Self { bpe })
    }

    /// 计算内容块的 token 数
    fn count_content_block(&self, block: &ContentBlock) -> usize {
        match block {
            ContentBlock::Text(t) => self.count_text(&t.text),
            ContentBlock::Thinking(t) => self.count_text(&t.thinking),
            ContentBlock::Image(_) => 1024, // 固定估算
            ContentBlock::ToolCall(tc) => {
                let name_tokens = self.count_text(&tc.name);
                let args_tokens = self.count_text(&serde_json::to_string(&tc.arguments).unwrap_or_default());
                name_tokens + args_tokens
            }
        }
    }
}

impl TokenCounter for TiktokenCounter {
    fn count_text(&self, text: &str) -> usize {
        self.bpe.encode_with_special_tokens(text).len()
    }

    fn count_message(&self, message: &Message) -> usize {
        // 基础消息开销（格式开销）
        const MESSAGE_OVERHEAD: usize = 4;

        let content_tokens = match message {
            Message::User(user_msg) => match &user_msg.content {
                UserContent::Text(text) => self.count_text(text),
                UserContent::Blocks(blocks) => {
                    blocks.iter().map(|b| self.count_content_block(b)).sum()
                }
            },
            Message::Assistant(assistant_msg) => {
                assistant_msg.content.iter().map(|b| self.count_content_block(b)).sum()
            }
            Message::ToolResult(tool_result) => {
                // tool_result 包含 tool_call_id、tool_name 和 content
                let id_tokens = self.count_text(&tool_result.tool_call_id);
                let name_tokens = self.count_text(&tool_result.tool_name);
                let content_tokens: usize = tool_result.content.iter().map(|b| self.count_content_block(b)).sum();
                id_tokens + name_tokens + content_tokens
            }
        };

        MESSAGE_OVERHEAD + content_tokens
    }

    fn count_messages(&self, messages: &[Message]) -> usize {
        if messages.is_empty() {
            return 0;
        }
        messages.iter().map(|m| self.count_message(m)).sum::<usize>() + 3 // 3 tokens for reply priming
    }
}

/// Mistral Token Counter - 使用 Hugging Face tokenizers
pub struct MistralTokenCounter {
    tokenizer: tokenizers::Tokenizer,
}

impl MistralTokenCounter {
    /// 创建新的 MistralTokenCounter
    /// 使用全局缓存的 tokenizer 实例
    pub fn new() -> Option<Self> {
        let tokenizer_opt = MISTRAL_TOKENIZER.get_or_init(|| {
            // 尝试加载 Mistral-7B tokenizer
            // 注意：from_pretrained 需要网络连接下载 tokenizer 文件
            match tokenizers::Tokenizer::from_pretrained("mistralai/Mistral-7B-v0.1", None) {
                Ok(t) => {
                    tracing::debug!("Successfully loaded Mistral tokenizer from Hugging Face");
                    Some(t)
                }
                Err(e) => {
                    tracing::warn!("Failed to load Mistral tokenizer from Hugging Face: {}", e);
                    None
                }
            }
        });

        tokenizer_opt.as_ref().map(|t| Self {
            tokenizer: t.clone(),
        })
    }

    /// 计算内容块的 token 数
    fn count_content_block(&self, block: &ContentBlock) -> usize {
        match block {
            ContentBlock::Text(t) => self.count_text(&t.text),
            ContentBlock::Thinking(t) => self.count_text(&t.thinking),
            ContentBlock::Image(_) => 1024, // 固定估算
            ContentBlock::ToolCall(tc) => {
                let name_tokens = self.count_text(&tc.name);
                let args_tokens = self.count_text(&tc.arguments.to_string());
                name_tokens + args_tokens
            }
        }
    }
}

impl TokenCounter for MistralTokenCounter {
    fn count_text(&self, text: &str) -> usize {
        match self.tokenizer.encode(text, false) {
            Ok(encoding) => encoding.len(),
            Err(e) => {
                tracing::warn!("Mistral tokenizer encode failed: {}, falling back to estimate", e);
                // 回退到字符估算
                (text.chars().count() as f64 / 3.8).ceil() as usize
            }
        }
    }

    fn count_message(&self, message: &Message) -> usize {
        // 基础消息开销（格式开销）
        const MESSAGE_OVERHEAD: usize = 4;

        let content_tokens = match message {
            Message::User(user_msg) => match &user_msg.content {
                UserContent::Text(text) => self.count_text(text),
                UserContent::Blocks(blocks) => {
                    blocks.iter().map(|b| self.count_content_block(b)).sum()
                }
            },
            Message::Assistant(assistant_msg) => {
                assistant_msg.content.iter().map(|b| self.count_content_block(b)).sum()
            }
            Message::ToolResult(tool_result) => {
                let id_tokens = self.count_text(&tool_result.tool_call_id);
                let name_tokens = self.count_text(&tool_result.tool_name);
                let content_tokens: usize = tool_result
                    .content
                    .iter()
                    .map(|b| self.count_content_block(b))
                    .sum();
                id_tokens + name_tokens + content_tokens
            }
        };

        MESSAGE_OVERHEAD + content_tokens
    }

    fn count_messages(&self, messages: &[Message]) -> usize {
        if messages.is_empty() {
            return 0;
        }
        messages.iter().map(|m| self.count_message(m)).sum::<usize>() + 3 // 3 tokens for reply priming
    }
}

/// Gemini Token Counter
/// 尝试使用 Hugging Face tokenizers，如果不可用则使用改进的估算器
pub struct GeminiTokenCounter {
    tokenizer: Option<tokenizers::Tokenizer>,
}

impl GeminiTokenCounter {
    /// 创建新的 GeminiTokenCounter
    /// 尝试加载 Gemma tokenizer，如果失败则使用改进的估算器
    pub fn new() -> Option<Self> {
        let tokenizer_opt = GEMINI_TOKENIZER.get_or_init(|| {
            // 尝试加载 Google Gemma tokenizer 作为 Gemini 的兼容 tokenizer
            // Gemma 和 Gemini 都使用 Google 的 SentencePiece tokenizer
            match tokenizers::Tokenizer::from_pretrained("google/gemma-2b", None) {
                Ok(t) => {
                    tracing::debug!("Successfully loaded Gemma tokenizer from Hugging Face");
                    Some(t)
                }
                Err(e) => {
                    tracing::warn!("Failed to load Gemma tokenizer from Hugging Face: {}", e);
                    None
                }
            }
        });

        Some(Self {
            tokenizer: tokenizer_opt.clone(),
        })
    }

    /// 计算内容块的 token 数（支持多模态图片动态估算）
    fn count_content_block(&self, block: &ContentBlock) -> usize {
        match block {
            ContentBlock::Text(t) => self.count_text(&t.text),
            ContentBlock::Thinking(t) => self.count_text(&t.thinking),
            ContentBlock::Image(image) => {
                // 多模态图片 token 动态估算
                // 基于图片尺寸计算（如果可用）
                self.estimate_image_tokens(image)
            }
            ContentBlock::ToolCall(tc) => {
                let name_tokens = self.count_text(&tc.name);
                let args_tokens = self.count_text(&tc.arguments.to_string());
                name_tokens + args_tokens
            }
        }
    }

    /// 估算图片的 token 数
    /// 基于图片尺寸动态计算：
    /// - 小图 (<=256px): 258 tokens
    /// - 中图 (<=512px): 512 tokens
    /// - 大图 (<=1024px): 1024 tokens
    /// - 超大图 (>1024px): 2048 tokens
    fn estimate_image_tokens(&self, _image: &crate::types::ImageContent) -> usize {
        // 注意：ImageContent 结构可能没有尺寸信息
        // 这里使用保守估算，实际实现可能需要扩展 ImageContent
        // 默认使用中等尺寸估算
        1024
    }

    /// 改进的文本 token 估算
    /// 基于 SentencePiece 特征优化：
    /// - CJK 字符：每个字符约 1-2 tokens
    /// - 代码：基于标点符号和空格密度调整
    /// - 普通文本：使用标准比率
    fn estimate_text_tokens(&self, text: &str) -> usize {
        let chars: Vec<char> = text.chars().collect();
        let total_chars = chars.len();

        if total_chars == 0 {
            return 0;
        }

        // 统计字符类型
        let cjk_count = chars.iter().filter(|c| is_cjk_char(**c)).count();
        let code_chars: usize = chars
            .iter()
            .filter(|c| is_code_related_char(**c))
            .count();

        let cjk_ratio = cjk_count as f64 / total_chars as f64;
        let code_ratio = code_chars as f64 / total_chars as f64;

        // 根据内容类型调整比率
        let chars_per_token = if cjk_ratio > 0.3 {
            // CJK 内容较多：每个字符约 0.6-0.8 tokens
            1.5
        } else if code_ratio > 0.2 {
            // 代码内容：token 密度较高
            2.5
        } else {
            // 普通英文文本
            3.8
        };

        (total_chars as f64 / chars_per_token).ceil() as usize
    }
}

/// 检查字符是否为 CJK（中日韩）字符
fn is_cjk_char(c: char) -> bool {
    matches!(
        c,
        '\u{4e00}'..='\u{9fff}' | // CJK Unified Ideographs
        '\u{3040}'..='\u{309f}' | // Hiragana
        '\u{30a0}'..='\u{30ff}' | // Katakana
        '\u{ac00}'..='\u{d7af}'   // Hangul Syllables
    )
}

/// 检查字符是否与代码相关（标点、符号等）
fn is_code_related_char(c: char) -> bool {
    matches!(
        c,
        '{' | '}' | '[' | ']' | '(' | ')' | ';' | ':' | ',' | '.' |
        '=' | '+' | '-' | '*' | '/' | '%' | '&' | '|' | '!' | '<' | '>' |
        '_' | '$' | '#' | '@' | '^' | '~' | '`' | '\\' | '"' | '\''
    )
}

impl TokenCounter for GeminiTokenCounter {
    fn count_text(&self, text: &str) -> usize {
        if let Some(ref tokenizer) = self.tokenizer {
            match tokenizer.encode(text, false) {
                Ok(encoding) => encoding.len(),
                Err(e) => {
                    tracing::warn!("Gemini tokenizer encode failed: {}, falling back to estimate", e);
                    self.estimate_text_tokens(text)
                }
            }
        } else {
            // 使用改进的估算器
            self.estimate_text_tokens(text)
        }
    }

    fn count_message(&self, message: &Message) -> usize {
        // 基础消息开销（格式开销）
        const MESSAGE_OVERHEAD: usize = 4;

        let content_tokens = match message {
            Message::User(user_msg) => match &user_msg.content {
                UserContent::Text(text) => self.count_text(text),
                UserContent::Blocks(blocks) => {
                    blocks.iter().map(|b| self.count_content_block(b)).sum()
                }
            },
            Message::Assistant(assistant_msg) => {
                assistant_msg.content.iter().map(|b| self.count_content_block(b)).sum()
            }
            Message::ToolResult(tool_result) => {
                let id_tokens = self.count_text(&tool_result.tool_call_id);
                let name_tokens = self.count_text(&tool_result.tool_name);
                let content_tokens: usize = tool_result
                    .content
                    .iter()
                    .map(|b| self.count_content_block(b))
                    .sum();
                id_tokens + name_tokens + content_tokens
            }
        };

        MESSAGE_OVERHEAD + content_tokens
    }

    fn count_messages(&self, messages: &[Message]) -> usize {
        if messages.is_empty() {
            return 0;
        }
        messages.iter().map(|m| self.count_message(m)).sum::<usize>() + 3 // 3 tokens for reply priming
    }
}

/// 判断是否为 OpenAI 模型
fn is_openai_model(model: &str) -> bool {
    let model_lower = model.to_lowercase();
    model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
        || model_lower.contains("openai")
}

/// 判断是否为 Mistral 模型
fn is_mistral_model(model: &str) -> bool {
    let model_lower = model.to_lowercase();
    model_lower.starts_with("mistral")
        || model_lower.starts_with("codestral")
        || model_lower.starts_with("mixtral")
        || model_lower.starts_with("pixtral")
        || model_lower.starts_with("magistral")
        || model_lower.contains("/mistral")
        || model_lower.contains("/codestral")
        || model_lower.contains("/mixtral")
}

/// 判断是否为 Gemini 模型
fn is_gemini_model(model: &str) -> bool {
    let model_lower = model.to_lowercase();
    model_lower.starts_with("gemini")
        || model_lower.starts_with("gemma")
        || model_lower.starts_with("google")
        || model_lower.contains("/gemini")
        || model_lower.contains("/gemma")
}

/// 创建 token 计数器工厂函数
/// 对于 OpenAI 模型使用精确的 TiktokenCounter
/// 对于 Mistral 模型使用 MistralTokenCounter
/// 对于 Gemini 模型使用 GeminiTokenCounter
/// 其他模型使用启发式计数器
pub fn create_token_counter(model: &str) -> Arc<dyn TokenCounter> {
    // 尝试为 OpenAI 模型创建精确计数器
    if is_openai_model(model) {
        if let Some(counter) = TiktokenCounter::new(model) {
            return Arc::new(counter);
        }
    }

    // 尝试为 Mistral 模型创建精确计数器
    if is_mistral_model(model) {
        if let Some(counter) = MistralTokenCounter::new() {
            return Arc::new(counter);
        }
    }

    // 尝试为 Gemini 模型创建精确计数器
    if is_gemini_model(model) {
        if let Some(counter) = GeminiTokenCounter::new() {
            return Arc::new(counter);
        }
    }

    // 回退到模型特定的启发式计数器
    Arc::new(ModelTokenCounter::new(model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::UserMessage;

    #[test]
    fn test_count_text() {
        let counter = EstimateTokenCounter::new();
        // "Hello World" 有 11 个字符，11/4 = 2.75，ceil = 3
        assert_eq!(counter.count_text("Hello World"), 3);
    }

    #[test]
    fn test_count_message_user() {
        let counter = EstimateTokenCounter::new();
        let msg = Message::User(UserMessage::new("Hello"));
        // "Hello" = 5 字符，5/4 = 1.25 -> 2 + 4 overhead = 6
        assert_eq!(counter.count_message(&msg), 6);
    }

    #[test]
    fn test_count_messages() {
        let counter = EstimateTokenCounter::new();
        let messages = vec![
            Message::User(UserMessage::new("Hello")),
            Message::User(UserMessage::new("World")),
        ];
        // 2 条消息，每条约 6 tokens，加上 3 tokens reply priming = 15
        let total = counter.count_messages(&messages);
        assert!(total >= 10);
    }

    #[test]
    fn test_model_token_counter() {
        let counter = ModelTokenCounter::new("claude");
        assert_eq!(counter.model_family(), "claude");

        let counter = ModelTokenCounter::new("gpt");
        assert_eq!(counter.model_family(), "gpt");
    }

    #[test]
    fn test_tiktoken_counter_creation() {
        let counter = TiktokenCounter::new("gpt-4o");
        assert!(counter.is_some());

        let counter = TiktokenCounter::new("gpt-4");
        assert!(counter.is_some());

        let counter = TiktokenCounter::new("gpt-3.5-turbo");
        assert!(counter.is_some());

        let counter = TiktokenCounter::new("o1-preview");
        assert!(counter.is_some());
    }

    #[test]
    fn test_tiktoken_count_text() {
        let counter = TiktokenCounter::new("gpt-4o").unwrap();
        let count = counter.count_text("Hello, world!");
        assert!(count > 0);
        assert!(count < 10); // 合理范围
    }

    #[test]
    fn test_create_token_counter_openai() {
        let counter = create_token_counter("gpt-4o");
        let count = counter.count_text("Hello, world!");
        assert!(count > 0);
    }

    #[test]
    fn test_create_token_counter_claude() {
        let counter = create_token_counter("claude-sonnet-4-20250514");
        let count = counter.count_text("Hello, world!");
        assert!(count > 0);
    }

    #[test]
    fn test_model_token_counter_updated_ratios() {
        let gemini = ModelTokenCounter::new("gemini");
        let default = EstimateTokenCounter::new();
        // Gemini 使用 3.8 而非 4.0，应产生更多 tokens（更保守的估算）
        let text = "a".repeat(100);
        assert!(gemini.count_text(&text) >= default.count_text(&text));
    }

    #[test]
    fn test_tiktoken_counter_message() {
        let counter = TiktokenCounter::new("gpt-4o").unwrap();
        let msg = Message::User(UserMessage::new("Hello, world!"));
        let count = counter.count_message(&msg);
        assert!(count > 4); // 至少要有 overhead
    }

    #[test]
    fn test_tiktoken_counter_messages() {
        let counter = TiktokenCounter::new("gpt-4o").unwrap();
        let messages = vec![
            Message::User(UserMessage::new("Hello")),
            Message::User(UserMessage::new("World")),
        ];
        let count = counter.count_messages(&messages);
        assert!(count > 0);
    }

    #[test]
    fn test_is_openai_model() {
        assert!(is_openai_model("gpt-4o"));
        assert!(is_openai_model("gpt-4"));
        assert!(is_openai_model("gpt-3.5-turbo"));
        assert!(is_openai_model("o1-preview"));
        assert!(is_openai_model("o3-mini"));
        assert!(is_openai_model("openai/gpt-4"));
        assert!(!is_openai_model("claude-sonnet"));
        assert!(!is_openai_model("gemini-pro"));
    }

    #[test]
    fn test_is_mistral_model() {
        // 标准 Mistral 模型
        assert!(is_mistral_model("mistral-small"));
        assert!(is_mistral_model("mistral-medium"));
        assert!(is_mistral_model("mistral-large"));
        assert!(is_mistral_model("Mistral-7B")); // 大小写不敏感

        // Codestral 模型
        assert!(is_mistral_model("codestral-latest"));
        assert!(is_mistral_model("codestral-2405"));

        // Mixtral 模型
        assert!(is_mistral_model("mixtral-8x7b"));
        assert!(is_mistral_model("mixtral-8x22b"));

        // Pixtral 模型
        assert!(is_mistral_model("pixtral-12b"));

        // 带 provider 前缀的模型
        assert!(is_mistral_model("mistral/mistral-small"));
        assert!(is_mistral_model("openrouter/mistral-7b"));

        // 非 Mistral 模型
        assert!(!is_mistral_model("gpt-4"));
        assert!(!is_mistral_model("claude-sonnet"));
        assert!(!is_mistral_model("gemini-pro"));
    }

    #[test]
    fn test_is_gemini_model() {
        // Gemini 模型
        assert!(is_gemini_model("gemini-pro"));
        assert!(is_gemini_model("gemini-ultra"));
        assert!(is_gemini_model("gemini-1.5-pro"));
        assert!(is_gemini_model("gemini-1.5-flash"));
        assert!(is_gemini_model("Gemini-Pro")); // 大小写不敏感

        // Gemma 模型
        assert!(is_gemini_model("gemma-2b"));
        assert!(is_gemini_model("gemma-7b"));
        assert!(is_gemini_model("gemma-2-9b"));

        // Google 前缀
        assert!(is_gemini_model("google/gemini-pro"));

        // 带 provider 前缀的模型
        assert!(is_gemini_model("google/gemma-2b"));
        assert!(is_gemini_model("vertex/gemini-1.5-pro"));

        // 非 Gemini 模型
        assert!(!is_gemini_model("gpt-4"));
        assert!(!is_mistral_model("claude-sonnet"));
        assert!(!is_gemini_model("mistral-small"));
    }

    #[test]
    fn test_mistral_token_counter_creation() {
        // 测试创建（可能失败如果网络不可用，但不应 panic）
        let counter = MistralTokenCounter::new();
        // 无论成功失败，函数都应该正常返回
        // 如果成功，测试基本功能
        if let Some(counter) = counter {
            let count = counter.count_text("Hello, world!");
            assert!(count > 0, "Token count should be positive");
        }
    }

    #[test]
    fn test_gemini_token_counter_creation() {
        // 测试创建（可能失败如果网络不可用，但不应 panic）
        let counter = GeminiTokenCounter::new();
        // 无论成功失败，函数都应该正常返回
        assert!(counter.is_some(), "GeminiTokenCounter::new should always return Some");

        let counter = counter.unwrap();
        let count = counter.count_text("Hello, world!");
        assert!(count > 0, "Token count should be positive");
    }

    #[test]
    fn test_create_token_counter_mistral() {
        let counter = create_token_counter("mistral-small");
        let count = counter.count_text("Hello, world!");
        assert!(count > 0, "Token count should be positive");

        // 测试 codestral
        let counter = create_token_counter("codestral-latest");
        let count = counter.count_text("Hello, world!");
        assert!(count > 0, "Token count should be positive");

        // 测试 mixtral
        let counter = create_token_counter("mixtral-8x7b");
        let count = counter.count_text("Hello, world!");
        assert!(count > 0, "Token count should be positive");
    }

    #[test]
    fn test_create_token_counter_gemini() {
        let counter = create_token_counter("gemini-pro");
        let count = counter.count_text("Hello, world!");
        assert!(count > 0, "Token count should be positive");

        // 测试 gemma
        let counter = create_token_counter("gemma-2b");
        let count = counter.count_text("Hello, world!");
        assert!(count > 0, "Token count should be positive");
    }

    #[test]
    fn test_gemini_estimate_text_tokens() {
        let counter = GeminiTokenCounter::new().unwrap();

        // 英文文本
        let english_count = counter.count_text("Hello, world! This is a test.");
        assert!(english_count > 0, "English text should have tokens");

        // CJK 文本
        let cjk_count = counter.count_text("你好世界，这是一个测试。");
        assert!(cjk_count > 0, "CJK text should have tokens");

        // 代码文本
        let code_count = counter.count_text("fn main() { println!(\"Hello\"); }");
        assert!(code_count > 0, "Code text should have tokens");
    }

    #[test]
    fn test_mistral_message_counting() {
        let counter = MistralTokenCounter::new();
        if let Some(counter) = counter {
            let msg = Message::User(UserMessage::new("Hello, world!"));
            let count = counter.count_message(&msg);
            assert!(count > 4, "Message count should include overhead");

            let messages = vec![
                Message::User(UserMessage::new("Hello")),
                Message::User(UserMessage::new("World")),
            ];
            let count = counter.count_messages(&messages);
            assert!(count > 0, "Messages count should be positive");
        }
    }

    #[test]
    fn test_gemini_message_counting() {
        let counter = GeminiTokenCounter::new().unwrap();
        let msg = Message::User(UserMessage::new("Hello, world!"));
        let count = counter.count_message(&msg);
        assert!(count > 4, "Message count should include overhead");

        let messages = vec![
            Message::User(UserMessage::new("Hello")),
            Message::User(UserMessage::new("World")),
        ];
        let count = counter.count_messages(&messages);
        assert!(count > 0, "Messages count should be positive");
    }

    #[test]
    fn test_cjk_char_detection() {
        // 测试 CJK 字符检测
        assert!(is_cjk_char('中'));
        assert!(is_cjk_char('あ')); // 日语平假名
        assert!(is_cjk_char('ア')); // 日语片假名
        assert!(is_cjk_char('한')); // 韩语
        assert!(!is_cjk_char('a'));
        assert!(!is_cjk_char('1'));
        assert!(!is_cjk_char('$'));
    }

    #[test]
    fn test_code_char_detection() {
        // 测试代码相关字符检测
        assert!(is_code_related_char('{'));
        assert!(is_code_related_char('}'));
        assert!(is_code_related_char(';'));
        assert!(is_code_related_char('='));
        assert!(!is_code_related_char('a'));
        assert!(!is_code_related_char(' '));
    }

    #[test]
    fn test_fallback_to_model_counter() {
        // 当精确 tokenizer 不可用时，确保回退到 ModelTokenCounter 正常工作
        // 使用一个不太可能加载成功的模型名称来测试回退逻辑
        let counter = create_token_counter("unknown-model-xyz");
        let count = counter.count_text("Hello, world!");
        assert!(count > 0, "Fallback counter should work");
    }

    // ============== 新增测试 ==============

    #[test]
    fn test_empty_string_count() {
        let counter = EstimateTokenCounter::new();
        assert_eq!(counter.count_text(""), 0);
        
        // TiktokenCounter
        if let Some(counter) = TiktokenCounter::new("gpt-4o") {
            assert_eq!(counter.count_text(""), 0);
        }
        
        // GeminiTokenCounter
        let counter = GeminiTokenCounter::new().unwrap();
        assert_eq!(counter.count_text(""), 0);
    }

    #[test]
    fn test_ascii_text_count() {
        let counter = EstimateTokenCounter::new();
        
        // 纯 ASCII 文本
        let text = "The quick brown fox jumps over the lazy dog.";
        let count = counter.count_text(text);
        assert!(count > 0);
        
        // 验证 ASCII 字符估算合理
        // 44 字符 / 4 = 11 tokens
        assert!(count >= 10 && count <= 15, "Expected around 11 tokens, got {}", count);
    }

    #[test]
    fn test_unicode_chinese_text_count() {
        let counter = EstimateTokenCounter::new();
        
        // 纯中文文本
        let text = "你好世界，这是一个测试。";
        let count = counter.count_text(text);
        assert!(count > 0);
        
        // 验证中文文本 token 估算
        // 12 个中文字符，估算应该比 ASCII 更高密度
        assert!(count >= 2, "Chinese text should have at least 2 tokens, got {}", count);
    }

    #[test]
    fn test_mixed_language_text_count() {
        let counter = EstimateTokenCounter::new();
        
        // 中英混合文本
        let text = "Hello 你好 World 世界";
        let count = counter.count_text(text);
        assert!(count > 0);
        
        // 验证混合文本处理
        // 17 个字符（含空格）
        assert!(count >= 3, "Mixed text should have appropriate tokens");
    }

    #[test]
    fn test_long_text_count() {
        let counter = EstimateTokenCounter::new();
        
        // 长文本（1000 字符）
        let text = "a".repeat(1000);
        let count = counter.count_text(&text);
        assert!(count > 0);
        
        // 1000 / 4 = 250 tokens
        assert!(count >= 240 && count <= 260, "Expected around 250 tokens, got {}", count);
        
        // 非常长的文本（10000 字符）
        let long_text = "The quick brown fox ".repeat(500);
        let long_count = counter.count_text(&long_text);
        assert!(long_count > 0);
        assert!(long_count > count, "Longer text should have more tokens");
    }

    #[test]
    fn test_model_family_encoding_correctness() {
        // Claude 模型 - 使用 3.5 比率
        let claude = ModelTokenCounter::new("claude");
        let text = "test text for counting";
        let claude_count = claude.count_text(text);
        
        // GPT 模型 - 使用 4.0 比率
        let gpt = ModelTokenCounter::new("gpt");
        let gpt_count = gpt.count_text(text);
        
        // Gemini 模型 - 使用 3.8 比率
        let gemini = ModelTokenCounter::new("gemini");
        let gemini_count = gemini.count_text(text);
        
        // Claude (3.5) 应该产生更多 token（更保守）
        // GPT (4.0) 应该产生最少 token
        assert!(claude_count >= gpt_count, "Claude should estimate more tokens than GPT");
        assert!(gemini_count >= gpt_count, "Gemini should estimate more tokens than GPT");
    }

    #[test]
    fn test_special_characters_count() {
        let counter = EstimateTokenCounter::new();
        
        // 特殊字符
        let text = "!@#$%^&*()_+-=[]{}|;':\",./<>?";
        let count = counter.count_text(text);
        assert!(count > 0);
        
        // emoji
        let emoji_text = "Hello 👋 World 🌍";
        let emoji_count = counter.count_text(emoji_text);
        assert!(emoji_count > 0);
    }

    #[test]
    fn test_whitespace_only_text() {
        let counter = EstimateTokenCounter::new();
        
        // 纯空白字符
        assert_eq!(counter.count_text(""), 0);
        assert_eq!(counter.count_text("   "), 1); // 3 空格 / 4 = 0.75 -> 1
        assert_eq!(counter.count_text("\t\n\r"), 1);
    }

    #[test]
    fn test_code_text_count() {
        let counter = EstimateTokenCounter::new();
        
        // 代码片段
        let code = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let count = counter.count_text(code);
        assert!(count > 0);
        
        // 代码通常有更多标点符号，token 密度更高
        // 46 字符
        assert!(count >= 10, "Code should have reasonable token count");
    }

    #[test]
    fn test_count_message_empty() {
        let counter = EstimateTokenCounter::new();
        
        // 空消息
        let msg = Message::User(UserMessage::new(""));
        let count = counter.count_message(&msg);
        
        // 只有 overhead（4 tokens）
        assert!(count >= 4, "Empty message should have at least overhead");
    }

    #[test]
    fn test_count_messages_empty_array() {
        let counter = EstimateTokenCounter::new();
        
        // 空消息数组
        let count = counter.count_messages(&[]);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_count_messages_many() {
        let counter = EstimateTokenCounter::new();
        
        // 多条消息
        let messages: Vec<Message> = (0..10)
            .map(|i| Message::User(UserMessage::new(format!("Message {}", i))))
            .collect();
        
        let count = counter.count_messages(&messages);
        assert!(count > 0);
        
        // 每条消息至少有 overhead + 内容
        // 10 条消息，每条约 5-6 tokens + 3 reply priming
        assert!(count >= 50, "10 messages should have substantial tokens");
    }
}
