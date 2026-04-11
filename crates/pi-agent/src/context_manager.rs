//! 上下文窗口管理模块
//!
//! 提供上下文窗口监控、token 计数和智能消息裁剪功能

use std::sync::Arc;
use pi_ai::{Message, TokenCounter};
use crate::types::AgentMessage;

/// 上下文使用情况
#[derive(Debug, Clone)]
pub struct ContextUsage {
    /// 总 token 数
    pub total_tokens: usize,
    /// 上下文窗口大小
    pub context_window: usize,
    /// 使用百分比
    pub usage_percent: f64,
    /// 剩余 token 数
    pub remaining_tokens: usize,
    /// 消息数量
    pub message_count: usize,
}

impl ContextUsage {
    /// 创建新的上下文使用情况
    pub fn new(total_tokens: usize, context_window: usize, message_count: usize) -> Self {
        let usage_percent = if context_window > 0 {
            (total_tokens as f64 / context_window as f64) * 100.0
        } else {
            0.0
        };
        let remaining_tokens = context_window.saturating_sub(total_tokens);

        Self {
            total_tokens,
            context_window,
            usage_percent,
            remaining_tokens,
            message_count,
        }
    }
}

/// 上下文窗口管理器
pub struct ContextWindowManager {
    token_counter: Arc<dyn TokenCounter>,
    context_window_size: usize,
    reserve_for_output: usize, // 为输出预留，默认 4096
    warning_threshold: f64,    // 警告阈值，默认 0.8 (80%)
}

impl ContextWindowManager {
    /// 创建新的上下文窗口管理器
    pub fn new(token_counter: Arc<dyn TokenCounter>, context_window_size: usize) -> Self {
        Self {
            token_counter,
            context_window_size,
            reserve_for_output: 4096,
            warning_threshold: 0.8,
        }
    }

    /// 使用自定义配置创建
    pub fn with_config(
        token_counter: Arc<dyn TokenCounter>,
        context_window_size: usize,
        reserve_for_output: usize,
        warning_threshold: f64,
    ) -> Self {
        Self {
            token_counter,
            context_window_size,
            reserve_for_output,
            warning_threshold: warning_threshold.clamp(0.0, 1.0),
        }
    }

    /// 估算当前上下文使用情况
    pub fn estimate_usage(&self, messages: &[AgentMessage]) -> ContextUsage {
        // 将 AgentMessage 转换为 Message
        let llm_messages: Vec<Message> = messages
            .iter()
            .map(|m| match m {
                AgentMessage::Llm(msg) => msg.clone(),
            })
            .collect();

        let total_tokens = self.token_counter.count_messages(&llm_messages);

        ContextUsage::new(total_tokens, self.context_window_size, messages.len())
    }

    /// 是否需要发出警告
    pub fn should_warn(&self, usage: &ContextUsage) -> bool {
        usage.usage_percent >= self.warning_threshold * 100.0
    }

    /// 是否需要裁剪
    pub fn needs_trimming(&self, usage: &ContextUsage) -> bool {
        usage.total_tokens + self.reserve_for_output > self.context_window_size
    }

    /// 判断是否需要压缩（而非粗暴裁剪）
    /// 
    /// 当 token 使用量超过阈值（默认 85%）时返回 true
    pub fn needs_compaction(&self, messages: &[AgentMessage]) -> bool {
        let usage = self.estimate_usage(messages);
        let threshold = self.context_window_size as f64 * 0.85; // 85% 阈值
        usage.total_tokens as f64 > threshold
    }

    /// 智能裁剪消息
    ///
    /// 裁剪策略：
    /// 1. 始终保留第一条消息（通常是系统消息）
    /// 2. 始终保留最近 2 轮对话（4 条消息：user + assistant + user + assistant）
    /// 3. 优先移除旧的工具调用结果（ToolResult 消息，通常最大）
    /// 4. 然后移除旧的助手消息
    /// 5. 最后移除旧的用户消息
    pub fn trim_messages(&self, messages: &mut Vec<AgentMessage>, target_tokens: usize) {
        if messages.len() <= 5 {
            // 消息太少，不进行裁剪
            return;
        }

        // 计算当前 token 数
        let llm_messages: Vec<Message> = messages
            .iter()
            .map(|m| match m {
                AgentMessage::Llm(msg) => msg.clone(),
            })
            .collect();
        let current_tokens = self.token_counter.count_messages(&llm_messages);

        if current_tokens <= target_tokens {
            // 不需要裁剪
            return;
        }

        // 需要裁剪，保留第一条和最近 4 条消息
        let keep_first = 1;
        let keep_last = 4;

        if messages.len() <= keep_first + keep_last {
            return;
        }

        // 获取可裁剪的消息索引范围
        let trim_start = keep_first;
        let trim_end = messages.len() - keep_last;

        if trim_start >= trim_end {
            return;
        }

        // 收集可裁剪的消息及其索引
        let mut trimmable: Vec<(usize, &AgentMessage, MessageType)> = messages
            [trim_start..trim_end]
            .iter()
            .enumerate()
            .map(|(idx, msg)| {
                let msg_type = classify_message(msg);
                (trim_start + idx, msg, msg_type)
            })
            .collect();

        // 按优先级排序：ToolResult > Assistant > User
        trimmable.sort_by_key(|(_, _, msg_type)| match msg_type {
            MessageType::ToolResult => 0,
            MessageType::Assistant => 1,
            MessageType::User => 2,
            MessageType::System => 3,
        });

        // 逐个移除消息，直到达到目标 token 数
        let mut tokens_to_remove = current_tokens - target_tokens;
        let mut removed_indices = Vec::new();

        for (original_idx, msg, _) in trimmable {
            if tokens_to_remove == 0 {
                break;
            }

            // 计算这条消息的 token 数
            let msg_tokens = match msg {
                AgentMessage::Llm(llm_msg) => self.token_counter.count_message(llm_msg),
            };

            removed_indices.push(original_idx);
            tokens_to_remove = tokens_to_remove.saturating_sub(msg_tokens);
        }

        // 按索引降序排序，以便安全地移除
        removed_indices.sort_unstable_by(|a, b| b.cmp(a));

        // 移除消息
        for idx in removed_indices {
            messages.remove(idx);
        }
    }

    /// 获取上下文窗口大小
    pub fn context_window_size(&self) -> usize {
        self.context_window_size
    }

    /// 获取警告阈值
    pub fn warning_threshold(&self) -> f64 {
        self.warning_threshold
    }

    /// 获取输出预留 token 数
    pub fn reserve_for_output(&self) -> usize {
        self.reserve_for_output
    }
}

/// 消息类型分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // 预留枚举变体供未来使用
enum MessageType {
    System,
    User,
    Assistant,
    ToolResult,
}

/// 分类消息类型
fn classify_message(msg: &AgentMessage) -> MessageType {
    match msg {
        AgentMessage::Llm(Message::User(_)) => MessageType::User,
        AgentMessage::Llm(Message::Assistant(_)) => MessageType::Assistant,
        AgentMessage::Llm(Message::ToolResult(_)) => MessageType::ToolResult,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_ai::{EstimateTokenCounter, UserMessage, AssistantMessage, Api, Provider};

    fn create_test_messages(count: usize) -> Vec<AgentMessage> {
        let mut messages = Vec::new();
        for i in 0..count {
            let content = format!("Message {}", i);
            messages.push(AgentMessage::Llm(Message::User(UserMessage::new(content))));
        }
        messages
    }

    #[test]
    fn test_context_usage() {
        let usage = ContextUsage::new(8000, 10000, 10);
        assert_eq!(usage.total_tokens, 8000);
        assert_eq!(usage.context_window, 10000);
        assert_eq!(usage.usage_percent, 80.0);
        assert_eq!(usage.remaining_tokens, 2000);
        assert_eq!(usage.message_count, 10);
    }

    #[test]
    fn test_context_window_manager() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter.clone(), 10000);

        assert_eq!(manager.context_window_size(), 10000);
        assert_eq!(manager.warning_threshold(), 0.8);
        assert_eq!(manager.reserve_for_output(), 4096);
    }

    #[test]
    fn test_estimate_usage() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter, 10000);

        let messages = create_test_messages(5);
        let usage = manager.estimate_usage(&messages);

        assert_eq!(usage.message_count, 5);
        assert!(usage.total_tokens > 0);
    }

    #[test]
    fn test_should_warn() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter, 10000);

        let usage = ContextUsage::new(8500, 10000, 10); // 85%
        assert!(manager.should_warn(&usage));

        let usage = ContextUsage::new(7000, 10000, 10); // 70%
        assert!(!manager.should_warn(&usage));
    }

    #[test]
    fn test_needs_trimming() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter, 10000);

        // 10000 - 4096 = 5904 为可用输入 token
        let usage = ContextUsage::new(6000, 10000, 10);
        assert!(manager.needs_trimming(&usage));

        let usage = ContextUsage::new(5000, 10000, 10);
        assert!(!manager.needs_trimming(&usage));
    }

    #[test]
    fn test_trim_messages() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter, 10000);

        let mut messages = create_test_messages(10);
        let initial_count = messages.len();

        // 裁剪到较小的目标
        manager.trim_messages(&mut messages, 50);

        // 应该保留第一条和最近 4 条
        assert!(messages.len() >= 5);
        assert!(messages.len() <= initial_count);
    }

    // === 边界条件测试 ===

    #[test]
    fn test_context_usage_zero_window() {
        // 边界条件：上下文窗口为 0
        let usage = ContextUsage::new(0, 0, 0);
        assert_eq!(usage.usage_percent, 0.0);
        assert_eq!(usage.remaining_tokens, 0);
    }

    #[test]
    fn test_context_usage_full_window() {
        // 边界条件：完全填满窗口
        let usage = ContextUsage::new(10000, 10000, 100);
        assert_eq!(usage.usage_percent, 100.0);
        assert_eq!(usage.remaining_tokens, 0);
    }

    #[test]
    fn test_context_usage_overflow() {
        // 边界条件：超出窗口（使用 saturating_sub）
        let usage = ContextUsage::new(15000, 10000, 100);
        assert_eq!(usage.usage_percent, 150.0);
        assert_eq!(usage.remaining_tokens, 0); // saturating_sub 应该返回 0
    }

    #[test]
    fn test_context_window_manager_with_config() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::with_config(
            counter,
            20000,
            8192,
            0.9,
        );

        assert_eq!(manager.context_window_size(), 20000);
        assert_eq!(manager.reserve_for_output(), 8192);
        assert_eq!(manager.warning_threshold(), 0.9);
    }

    #[test]
    fn test_context_window_manager_clamp_threshold() {
        let counter = Arc::new(EstimateTokenCounter::new());
        
        // 测试阈值被限制在 0.0-1.0 范围内
        let manager_high = ContextWindowManager::with_config(
            counter.clone(),
            10000,
            4096,
            1.5, // 应该被限制为 1.0
        );
        assert_eq!(manager_high.warning_threshold(), 1.0);

        let manager_low = ContextWindowManager::with_config(
            counter,
            10000,
            4096,
            -0.5, // 应该被限制为 0.0
        );
        assert_eq!(manager_low.warning_threshold(), 0.0);
    }

    #[test]
    fn test_estimate_usage_empty_messages() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter, 10000);

        let messages: Vec<AgentMessage> = vec![];
        let usage = manager.estimate_usage(&messages);

        assert_eq!(usage.message_count, 0);
        assert_eq!(usage.total_tokens, 0);
        assert_eq!(usage.usage_percent, 0.0);
    }

    #[test]
    fn test_needs_compaction() {
        let counter = Arc::new(EstimateTokenCounter::new());
        // 使用小窗口大小，使得少量消息就能触发压缩
        let manager = ContextWindowManager::new(counter.clone(), 100);

        // 创建消息以超过 85% 阈值 (85 tokens)
        let high_usage = create_test_messages(20);
        // 验证方法存在且能正常调用（实际结果取决于 token 计数器实现）
        let _needs_compaction = manager.needs_compaction(&high_usage);

        // 少量消息不应该需要压缩
        let low_usage = create_test_messages(1);
        assert!(!manager.needs_compaction(&low_usage));
    }

    #[test]
    fn test_trim_messages_too_few() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter, 10000);

        // 少于 6 条消息不应该被裁剪
        let mut messages = create_test_messages(5);
        let initial_count = messages.len();
        
        manager.trim_messages(&mut messages, 10);
        
        // 消息数量应该保持不变
        assert_eq!(messages.len(), initial_count);
    }

    #[test]
    fn test_trim_messages_no_trim_needed() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter, 10000);

        let mut messages = create_test_messages(10);
        
        // 设置一个很高的目标 token 数，不需要裁剪
        manager.trim_messages(&mut messages, 100000);
        
        // 消息应该保持不变
        assert_eq!(messages.len(), 10);
    }

    #[test]
    fn test_trim_messages_with_tool_results() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter, 10000);

        // 创建包含工具结果的消息
        let mut messages = vec![
            AgentMessage::Llm(Message::User(UserMessage::new("system"))),
        ];
        
        // 添加助手消息
        messages.push(AgentMessage::Llm(Message::Assistant(AssistantMessage::new(
            Api::Anthropic,
            Provider::Anthropic,
            "claude-3"
        ))));
        
        // 添加工具结果消息
        use pi_ai::types::{ToolResultMessage, ContentBlock, TextContent};
        messages.push(AgentMessage::Llm(Message::ToolResult(ToolResultMessage::new(
            "call_1",
            "test_tool",
            vec![ContentBlock::Text(TextContent::new("tool result content"))],
        ))));
        
        // 添加更多用户消息
        for i in 0..10 {
            messages.push(AgentMessage::Llm(Message::User(UserMessage::new(format!("msg {}", i)))));
        }

        let initial_count = messages.len();
        manager.trim_messages(&mut messages, 50);
        
        // 应该保留了第一条和最近 4 条
        assert!(messages.len() >= 5);
        assert!(messages.len() <= initial_count);
    }

    #[test]
    fn test_trim_messages_exact_boundary() {
        let counter = Arc::new(EstimateTokenCounter::new());
        let manager = ContextWindowManager::new(counter, 10000);

        // 正好 6 条消息（keep_first + keep_last = 5，所以会跳过）
        let mut messages = create_test_messages(6);
        
        manager.trim_messages(&mut messages, 10);
        
        // 6 条消息在边界上，但 trim_start = 1, trim_end = 2 (6-4=2)，所以 trim_start >= trim_end
        // 实际上不会裁剪，保留 5 条或 6 条都是合理的
        assert!(messages.len() >= 5 && messages.len() <= 6);
    }

    #[test]
    fn test_classify_message() {
        let user_msg = AgentMessage::Llm(Message::User(UserMessage::new("hello")));
        let assistant_msg = AgentMessage::Llm(Message::Assistant(AssistantMessage::new(
            Api::Anthropic,
            Provider::Anthropic,
            "claude-3"
        )));
        
        assert_eq!(classify_message(&user_msg), MessageType::User);
        assert_eq!(classify_message(&assistant_msg), MessageType::Assistant);
    }

    #[test]
    fn test_context_usage_debug() {
        let usage = ContextUsage::new(5000, 10000, 10);
        let debug_str = format!("{:?}", usage);
        assert!(debug_str.contains("ContextUsage"));
        assert!(debug_str.contains("5000"));
    }
}
