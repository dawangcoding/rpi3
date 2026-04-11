//! Agent 类型系统
//!
//! 定义 Agent 相关的核心类型，包括消息、工具、事件等

use pi_ai::types::*;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// Agent 消息 - 可以是 LLM 消息或自定义消息
#[derive(Debug, Clone)]
pub enum AgentMessage {
    /// LLM 消息
    Llm(Message),
}

impl Serialize for AgentMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // 直接序列化内部的 Message，不包装在 AgentMessage 中
        match self {
            AgentMessage::Llm(msg) => msg.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for AgentMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // 直接反序列化为 Message，然后包装为 AgentMessage
        Message::deserialize(deserializer).map(AgentMessage::Llm)
    }
}

impl AgentMessage {
    /// 创建用户文本消息
    pub fn user(content: &str) -> Self {
        AgentMessage::Llm(Message::User(UserMessage::new(content)))
    }

    /// 创建带图片的用户消息
    pub fn user_with_images(content: &str, images: Vec<ImageContent>) -> Self {
        let mut blocks: Vec<ContentBlock> = vec![ContentBlock::Text(TextContent::new(content))];
        for img in images {
            blocks.push(ContentBlock::Image(img));
        }
        AgentMessage::Llm(Message::User(UserMessage::new(blocks)))
    }

    /// 获取内部 Message 引用
    pub fn as_message(&self) -> Option<&Message> {
        match self {
            AgentMessage::Llm(msg) => Some(msg),
        }
    }

    /// 获取消息角色
    pub fn role(&self) -> &str {
        match self {
            AgentMessage::Llm(msg) => match msg {
                Message::User(_) => "user",
                Message::Assistant(_) => "assistant",
                Message::ToolResult(_) => "toolResult",
            },
        }
    }
}

/// 工具执行模式
/// 
/// 定义工具调用的执行策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum ToolExecutionMode {
    /// 串行执行
    Sequential,
    /// 并行执行
    #[default]
    Parallel,
}


/// 工具执行结果
/// 
/// 包含工具执行返回的内容和元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolResult {
    /// 结果内容
    pub content: Vec<ContentBlock>,
    /// 详细信息
    pub details: serde_json::Value,
}

impl AgentToolResult {
    /// 创建错误结果
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::Text(TextContent::new(message))],
            details: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

/// 工具调用上下文（用于钩子）
/// 
/// 传递给 before/after tool call 钩子的上下文信息
#[derive(Debug, Clone)]
pub struct ToolCallContext {
    /// 助手消息
    pub assistant_message: AssistantMessage,
    /// 工具调用
    pub tool_call: ToolCall,
    /// 参数
    pub args: serde_json::Value,
}

/// beforeToolCall 结果
/// 
/// 用于控制是否阻止工具执行
#[derive(Debug, Clone, Default)]
pub struct BeforeToolCallResult {
    /// 是否阻止
    pub block: bool,
    /// 阻止原因
    pub reason: Option<String>,
}

impl BeforeToolCallResult {
    /// 创建阻止结果
    pub fn blocked(reason: impl Into<String>) -> Self {
        Self {
            block: true,
            reason: Some(reason.into()),
        }
    }
}

/// afterToolCall 结果
/// 
/// 用于修改工具执行结果
#[derive(Debug, Clone, Default)]
pub struct AfterToolCallResult {
    /// 内容
    pub content: Option<Vec<ContentBlock>>,
    /// 详细信息
    pub details: Option<serde_json::Value>,
    /// 是否错误
    pub is_error: Option<bool>,
}

/// Agent 工具 trait
/// 
/// 定义 Agent 可调用的工具的接口
#[async_trait]
pub trait AgentTool: Send + Sync {
    /// 工具名称
    fn name(&self) -> &str;
    /// 工具标签
    fn label(&self) -> &str;
    /// 工具描述
    fn description(&self) -> &str;
    /// 工具参数（JSON Schema）
    fn parameters(&self) -> serde_json::Value;

    /// 准备参数（可选的兼容性转换）
    fn prepare_arguments(&self, args: serde_json::Value) -> serde_json::Value {
        args
    }

    /// 执行工具
    async fn execute(
        &self,
        tool_call_id: &str,
        params: serde_json::Value,
        cancel: tokio_util::sync::CancellationToken,
        on_update: Option<Box<dyn Fn(AgentToolResult) + Send + Sync>>,
    ) -> anyhow::Result<AgentToolResult>;
}

/// 将 AgentTool 转换为 LLM API 使用的 Tool 格式
pub fn agent_tool_to_llm_tool(tool: &dyn AgentTool) -> pi_ai::types::Tool {
    pi_ai::types::Tool {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
        parameters: tool.parameters(),
    }
}

/// Agent 上下文
/// 
/// 包含 Agent 运行时的系统提示词、消息历史和可用工具
pub struct AgentContext {
    /// 系统提示词
    pub system_prompt: String,
    /// 消息列表
    pub messages: Vec<AgentMessage>,
    /// 工具列表
    pub tools: Vec<Arc<dyn AgentTool>>,
}

impl std::fmt::Debug for AgentContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentContext")
            .field("system_prompt", &self.system_prompt)
            .field("messages", &self.messages)
            .field("tools", &format!("[{} tools]", self.tools.len()))
            .finish()
    }
}

impl Clone for AgentContext {
    fn clone(&self) -> Self {
        Self {
            system_prompt: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: self.tools.clone(),
        }
    }
}

impl AgentContext {
    /// 创建上下文快照
    pub fn snapshot(&self) -> Self {
        Self {
            system_prompt: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: self.tools.clone(),
        }
    }
}

/// Agent 事件
/// 
/// 描述 Agent 生命周期中的各种事件
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum AgentEvent {
    /// Agent 即将开始
    BeforeAgentStart,
    /// Agent 开始
    AgentStart,
    /// Agent 结束
    AgentEnd {
        /// 消息列表
        messages: Vec<AgentMessage>
    },
    /// Agent 即将结束（可拦截）
    BeforeAgentEnd {
        /// 消息列表
        messages: Vec<AgentMessage>
    },

    /// 上下文窗口警告
    ContextWarning {
        /// 使用百分比
        usage_percent: f64,
        /// 总 token 数
        total_tokens: usize,
        /// 上下文窗口大小
        context_window: usize,
    },

    /// Turn 开始
    TurnStart,
    /// Turn 结束
    TurnEnd {
        /// 消息
        message: AgentMessage,
        /// 工具结果
        tool_results: Vec<ToolResultMessage>,
    },
    /// Turn 执行出错
    TurnError {
        /// 错误信息
        error: String,
        /// Turn 索引
        turn_index: usize,
    },

    /// 消息开始
    MessageStart {
        /// 消息
        message: AgentMessage
    },
    /// 消息更新
    MessageUpdate {
        /// 消息
        message: AgentMessage,
        /// 事件
        event: AssistantMessageEvent,
    },
    /// 消息结束
    MessageEnd {
        /// 消息
        message: AgentMessage
    },
    /// 消息流式分块（用于流式传输中间内容）
    MessageChunk {
        /// 消息
        message: AgentMessage,
        /// 分块索引
        chunk_index: usize,
    },
    /// 消息处理出错
    MessageError {
        /// 错误信息
        error: String,
    },

    /// 工具调用前
    BeforeToolCall {
        /// 工具调用 ID
        tool_call_id: String,
        /// 工具名称
        tool_name: String,
        /// 参数
        args: serde_json::Value,
    },
    /// 工具执行开始
    ToolExecutionStart {
        /// 工具调用 ID
        tool_call_id: String,
        /// 工具名称
        tool_name: String,
        /// 参数
        args: serde_json::Value,
    },
    /// 工具执行更新
    ToolExecutionUpdate {
        /// 工具调用 ID
        tool_call_id: String,
        /// 工具名称
        tool_name: String,
        /// 参数
        args: serde_json::Value,
        /// 部分结果
        partial_result: AgentToolResult,
    },
    /// 工具执行结束
    ToolExecutionEnd {
        /// 工具调用 ID
        tool_call_id: String,
        /// 工具名称
        tool_name: String,
        /// 结果
        result: AgentToolResult,
        /// 是否错误
        is_error: bool,
    },
    /// 工具调用后
    AfterToolCall {
        /// 工具调用 ID
        tool_call_id: String,
        /// 工具名称
        tool_name: String,
        /// 结果
        result: AgentToolResult,
        /// 是否错误
        is_error: bool,
    },
    /// 工具执行出错
    ToolError {
        /// 工具调用 ID
        tool_call_id: String,
        /// 工具名称
        tool_name: String,
        /// 错误信息
        error: String,
    },

    /// 命令执行前
    BeforeCommandExecute {
        /// 命令
        command: String,
        /// 参数
        args: String,
    },
    /// 命令执行后
    AfterCommandExecute {
        /// 命令
        command: String,
        /// 结果
        result: String,
    },
    /// 命令执行出错
    CommandError {
        /// 命令
        command: String,
        /// 错误信息
        error: String,
    },

    /// 扩展加载完成
    ExtensionLoaded {
        /// 名称
        name: String,
        /// 版本
        version: String,
    },
    /// 扩展错误
    ExtensionError {
        /// 名称
        name: String,
        /// 错误信息
        error: String,
    },
}

/// 事件监听器回调类型
/// 
/// 用于接收 Agent 事件的回调函数类型
pub type AgentEventSink = Box<dyn Fn(AgentEvent) + Send + Sync>;

/// Agent 状态
/// 
/// 保存 Agent 运行时的完整状态
pub struct AgentState {
    /// 系统提示词
    pub system_prompt: String,
    /// 模型
    pub model: Model,
    /// 思考级别
    pub thinking_level: ThinkingLevel,
    /// 工具列表
    pub tools: Vec<Arc<dyn AgentTool>>,
    /// 消息列表
    pub messages: Vec<AgentMessage>,
    /// 是否正在流式传输
    pub is_streaming: bool,
    /// 流式消息
    pub streaming_message: Option<AgentMessage>,
    /// 待处理的工具调用
    pub pending_tool_calls: HashSet<String>,
    /// 错误消息
    pub error_message: Option<String>,
}

impl std::fmt::Debug for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentState")
            .field("system_prompt", &self.system_prompt)
            .field("model", &self.model)
            .field("thinking_level", &self.thinking_level)
            .field("tools", &format!("[{} tools]", self.tools.len()))
            .field("messages", &self.messages)
            .field("is_streaming", &self.is_streaming)
            .field("streaming_message", &self.streaming_message)
            .field("pending_tool_calls", &self.pending_tool_calls)
            .field("error_message", &self.error_message)
            .finish()
    }
}

impl Clone for AgentState {
    fn clone(&self) -> Self {
        Self {
            system_prompt: self.system_prompt.clone(),
            model: self.model.clone(),
            thinking_level: self.thinking_level.clone(),
            tools: self.tools.clone(),
            messages: self.messages.clone(),
            is_streaming: self.is_streaming,
            streaming_message: self.streaming_message.clone(),
            pending_tool_calls: self.pending_tool_calls.clone(),
            error_message: self.error_message.clone(),
        }
    }
}

impl AgentState {
    /// 创建新的 AgentState
    pub fn new(model: Model) -> Self {
        Self {
            system_prompt: String::new(),
            model,
            thinking_level: ThinkingLevel::Off,
            tools: Vec::new(),
            messages: Vec::new(),
            is_streaming: false,
            streaming_message: None,
            pending_tool_calls: HashSet::new(),
            error_message: None,
        }
    }
}

/// 队列模式
/// 
/// 定义消息队列的处理策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum QueueMode {
    /// 全部处理
    All,
    /// 一次处理一个
    #[default]
    OneAtATime,
}


/// 待处理消息队列
/// 
/// 管理待处理消息的缓冲队列
pub struct PendingMessageQueue {
    messages: Vec<AgentMessage>,
    /// 队列模式
    pub mode: QueueMode,
}

impl PendingMessageQueue {
    /// 创建新的待处理消息队列
    pub fn new(mode: QueueMode) -> Self {
        Self {
            messages: Vec::new(),
            mode,
        }
    }

    /// 添加消息到队列
    pub fn enqueue(&mut self, message: AgentMessage) {
        self.messages.push(message);
    }

    /// 检查是否有待处理消息
    pub fn has_items(&self) -> bool {
        !self.messages.is_empty()
    }

    /// 取出队列中的消息
    pub fn drain(&mut self) -> Vec<AgentMessage> {
        match self.mode {
            QueueMode::All => {
                let drained = self.messages.clone();
                self.messages.clear();
                drained
            }
            QueueMode::OneAtATime => {
                if self.messages.is_empty() {
                    Vec::new()
                } else {
                    let first = self.messages.remove(0);
                    vec![first]
                }
            }
        }
    }

    /// 清空队列
    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

#[cfg(test)]
mod serialization_tests {
    use super::*;
    use pi_ai::types::{Message, AssistantMessage, Api, Provider};
    
    #[test]
    fn test_agent_message_json() {
        let user_msg = AgentMessage::user("Hello");
        let json = serde_json::to_string_pretty(&user_msg).unwrap();
        println!("User message JSON:\n{}\n", json);
        
        // Try to deserialize back
        let result: Result<AgentMessage, _> = serde_json::from_str(&json);
        println!("Deserialization result: {:?}\n", result.is_ok());
        if let Err(ref e) = result {
            println!("Error: {}", e);
        }
        
        let assistant_msg = AgentMessage::Llm(Message::Assistant(AssistantMessage::new(
            Api::Anthropic,
            Provider::Anthropic,
            "claude-3"
        )));
        let json2 = serde_json::to_string_pretty(&assistant_msg).unwrap();
        println!("Assistant message JSON:\n{}\n", json2);
        
        // Try to deserialize back
        let result2: Result<AgentMessage, _> = serde_json::from_str(&json2);
        println!("Deserialization result: {:?}\n", result2.is_ok());
        if let Err(ref e) = result2 {
            println!("Error: {}", e);
        }
    }
}

#[cfg(test)]
mod agent_event_tests {
    use super::*;

    #[test]
    fn test_agent_event_lifecycle_variants() {
        // 测试 Agent 生命周期事件
        let before_start = AgentEvent::BeforeAgentStart;
        let start = AgentEvent::AgentStart;
        let end = AgentEvent::AgentEnd { messages: vec![] };
        let before_end = AgentEvent::BeforeAgentEnd { messages: vec![] };

        // 验证变体匹配
        assert!(matches!(before_start, AgentEvent::BeforeAgentStart));
        assert!(matches!(start, AgentEvent::AgentStart));
        assert!(matches!(end, AgentEvent::AgentEnd { .. }));
        assert!(matches!(before_end, AgentEvent::BeforeAgentEnd { .. }));
    }

    #[test]
    fn test_agent_event_context_warning() {
        let warning = AgentEvent::ContextWarning {
            usage_percent: 85.5,
            total_tokens: 8500,
            context_window: 10000,
        };

        match warning {
            AgentEvent::ContextWarning { usage_percent, total_tokens, context_window } => {
                assert_eq!(usage_percent, 85.5);
                assert_eq!(total_tokens, 8500);
                assert_eq!(context_window, 10000);
            }
            _ => panic!("Expected ContextWarning"),
        }
    }

    #[test]
    fn test_agent_event_turn_variants() {
        let turn_start = AgentEvent::TurnStart;
        let turn_end = AgentEvent::TurnEnd {
            message: AgentMessage::user("test"),
            tool_results: vec![],
        };
        let turn_error = AgentEvent::TurnError {
            error: "test error".to_string(),
            turn_index: 5,
        };

        assert!(matches!(turn_start, AgentEvent::TurnStart));
        assert!(matches!(turn_end, AgentEvent::TurnEnd { .. }));
        assert!(matches!(turn_error, AgentEvent::TurnError { .. }));
    }

    #[test]
    fn test_agent_event_message_variants() {
        let msg = AgentMessage::user("hello");
        
        let msg_start = AgentEvent::MessageStart { message: msg.clone() };
        let msg_end = AgentEvent::MessageEnd { message: msg.clone() };
        let msg_chunk = AgentEvent::MessageChunk {
            message: msg.clone(),
            chunk_index: 0,
        };
        let msg_error = AgentEvent::MessageError {
            error: "test".to_string(),
        };

        assert!(matches!(msg_start, AgentEvent::MessageStart { .. }));
        assert!(matches!(msg_end, AgentEvent::MessageEnd { .. }));
        assert!(matches!(msg_chunk, AgentEvent::MessageChunk { .. }));
        assert!(matches!(msg_error, AgentEvent::MessageError { .. }));
    }

    #[test]
    fn test_agent_event_tool_variants() {
        let tool_start = AgentEvent::ToolExecutionStart {
            tool_call_id: "call_1".to_string(),
            tool_name: "read_file".to_string(),
            args: serde_json::json!({"path": "/tmp/test"}),
        };
        let tool_end = AgentEvent::ToolExecutionEnd {
            tool_call_id: "call_1".to_string(),
            tool_name: "read_file".to_string(),
            result: AgentToolResult::error("test"),
            is_error: false,
        };
        let tool_error = AgentEvent::ToolError {
            tool_call_id: "call_1".to_string(),
            tool_name: "read_file".to_string(),
            error: "failed".to_string(),
        };

        assert!(matches!(tool_start, AgentEvent::ToolExecutionStart { .. }));
        assert!(matches!(tool_end, AgentEvent::ToolExecutionEnd { .. }));
        assert!(matches!(tool_error, AgentEvent::ToolError { .. }));
    }

    #[test]
    fn test_agent_event_command_variants() {
        let before_cmd = AgentEvent::BeforeCommandExecute {
            command: "test".to_string(),
            args: "arg1 arg2".to_string(),
        };
        let after_cmd = AgentEvent::AfterCommandExecute {
            command: "test".to_string(),
            result: "success".to_string(),
        };
        let cmd_error = AgentEvent::CommandError {
            command: "test".to_string(),
            error: "failed".to_string(),
        };

        assert!(matches!(before_cmd, AgentEvent::BeforeCommandExecute { .. }));
        assert!(matches!(after_cmd, AgentEvent::AfterCommandExecute { .. }));
        assert!(matches!(cmd_error, AgentEvent::CommandError { .. }));
    }

    #[test]
    fn test_agent_event_extension_variants() {
        let loaded = AgentEvent::ExtensionLoaded {
            name: "test-ext".to_string(),
            version: "1.0.0".to_string(),
        };
        let error = AgentEvent::ExtensionError {
            name: "test-ext".to_string(),
            error: "load failed".to_string(),
        };

        assert!(matches!(loaded, AgentEvent::ExtensionLoaded { .. }));
        assert!(matches!(error, AgentEvent::ExtensionError { .. }));
    }

    #[test]
    fn test_agent_event_debug_trait() {
        let event = AgentEvent::AgentStart;
        let debug_str = format!("{:?}", event);
        assert!(debug_str.contains("AgentStart"));
    }

    #[test]
    fn test_agent_event_clone() {
        let event = AgentEvent::ContextWarning {
            usage_percent: 50.0,
            total_tokens: 5000,
            context_window: 10000,
        };
        let cloned = event.clone();
        
        match (event, cloned) {
            (
                AgentEvent::ContextWarning { usage_percent: u1, total_tokens: t1, context_window: c1 },
                AgentEvent::ContextWarning { usage_percent: u2, total_tokens: t2, context_window: c2 }
            ) => {
                assert_eq!(u1, u2);
                assert_eq!(t1, t2);
                assert_eq!(c1, c2);
            }
            _ => panic!("Clone failed"),
        }
    }

    #[test]
    fn test_agent_message_role() {
        let user_msg = AgentMessage::user("hello");
        assert_eq!(user_msg.role(), "user");

        let assistant_msg = AgentMessage::Llm(Message::Assistant(AssistantMessage::new(
            Api::Anthropic,
            Provider::Anthropic,
            "claude-3"
        )));
        assert_eq!(assistant_msg.role(), "assistant");
    }

    #[test]
    fn test_agent_message_user_with_images() {
        use pi_ai::types::ImageContent;
        
        let images = vec![ImageContent::new("base64data", "image/png")];
        let msg = AgentMessage::user_with_images("describe this", images);
        
        assert_eq!(msg.role(), "user");
        
        // 验证可以获取到消息
        let inner = msg.as_message();
        assert!(inner.is_some());
    }

    #[test]
    fn test_agent_tool_result_error() {
        let result = AgentToolResult::error("something went wrong");
        assert_eq!(result.content.len(), 1);
        assert!(result.details.is_object());
    }

    #[test]
    fn test_before_tool_call_result_blocked() {
        let result = BeforeToolCallResult::blocked("security check");
        assert!(result.block);
        assert_eq!(result.reason, Some("security check".to_string()));
    }

    #[test]
    fn test_tool_execution_mode_default() {
        let mode: ToolExecutionMode = Default::default();
        assert_eq!(mode, ToolExecutionMode::Parallel);
    }

    #[test]
    fn test_queue_mode_default() {
        let mode: QueueMode = Default::default();
        assert_eq!(mode, QueueMode::OneAtATime);
    }

    #[test]
    fn test_pending_message_queue_operations() {
        let mut queue = PendingMessageQueue::new(QueueMode::All);
        
        // 测试空队列
        assert!(!queue.has_items());
        
        // 添加消息
        queue.enqueue(AgentMessage::user("msg1"));
        queue.enqueue(AgentMessage::user("msg2"));
        assert!(queue.has_items());
        
        // 取出消息
        let drained = queue.drain();
        assert_eq!(drained.len(), 2);
        assert!(!queue.has_items());
        
        // 测试清空
        queue.enqueue(AgentMessage::user("msg3"));
        queue.clear();
        assert!(!queue.has_items());
    }

    #[test]
    fn test_pending_message_queue_one_at_a_time() {
        let mut queue = PendingMessageQueue::new(QueueMode::OneAtATime);
        
        queue.enqueue(AgentMessage::user("msg1"));
        queue.enqueue(AgentMessage::user("msg2"));
        queue.enqueue(AgentMessage::user("msg3"));
        
        // OneAtATime 模式每次只取一个
        let drained = queue.drain();
        assert_eq!(drained.len(), 1);
        assert!(queue.has_items()); // 还有剩余
        
        // 继续取
        let drained2 = queue.drain();
        assert_eq!(drained2.len(), 1);
        assert!(queue.has_items());
    }

    #[test]
    fn test_agent_context_snapshot() {
        let ctx = AgentContext {
            system_prompt: "test prompt".to_string(),
            messages: vec![AgentMessage::user("hello")],
            tools: vec![],
        };
        
        let snapshot = ctx.snapshot();
        assert_eq!(snapshot.system_prompt, ctx.system_prompt);
        assert_eq!(snapshot.messages.len(), ctx.messages.len());
    }

    #[test]
    fn test_agent_context_debug() {
        let ctx = AgentContext {
            system_prompt: "test".to_string(),
            messages: vec![],
            tools: vec![],
        };
        
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("AgentContext"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_agent_state_new() {
        use pi_ai::types::Model;
        
        let model = Model {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            api: Api::Anthropic,
            provider: Provider::Anthropic,
            base_url: String::new(),
            reasoning: false,
            input: vec![pi_ai::types::InputModality::Text],
            cost: pi_ai::types::ModelCost {
                input: 0.0,
                output: 0.0,
                cache_read: None,
                cache_write: None,
            },
            context_window: 10000,
            max_tokens: 4096,
            headers: None,
            compat: None,
        };
        
        let state = AgentState::new(model.clone());
        assert_eq!(state.model.id, "test-model");
        assert!(!state.is_streaming);
        assert!(state.streaming_message.is_none());
        assert!(state.pending_tool_calls.is_empty());
    }

    #[test]
    fn test_agent_state_clone() {
        use pi_ai::types::Model;
        
        let model = Model {
            id: "test".to_string(),
            name: "Test".to_string(),
            api: Api::Anthropic,
            provider: Provider::Anthropic,
            base_url: String::new(),
            reasoning: false,
            input: vec![pi_ai::types::InputModality::Text],
            cost: pi_ai::types::ModelCost {
                input: 0.0,
                output: 0.0,
                cache_read: None,
                cache_write: None,
            },
            context_window: 10000,
            max_tokens: 4096,
            headers: None,
            compat: None,
        };
        
        let state = AgentState::new(model);
        let cloned = state.clone();
        
        assert_eq!(state.system_prompt, cloned.system_prompt);
        assert_eq!(state.is_streaming, cloned.is_streaming);
    }
}
