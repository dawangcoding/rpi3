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
    Llm(Message),
    // 未来可扩展自定义消息类型
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
    Sequential,
    #[default]
    Parallel,
}


/// 工具执行结果
/// 
/// 包含工具执行返回的内容和元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolResult {
    pub content: Vec<ContentBlock>,
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
    pub assistant_message: AssistantMessage,
    pub tool_call: ToolCall,
    pub args: serde_json::Value,
}

/// beforeToolCall 结果
/// 
/// 用于控制是否阻止工具执行
#[derive(Debug, Clone, Default)]
pub struct BeforeToolCallResult {
    pub block: bool,
    pub reason: Option<String>,
}

impl BeforeToolCallResult {
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
    pub content: Option<Vec<ContentBlock>>,
    pub details: Option<serde_json::Value>,
    pub is_error: Option<bool>,
}

/// Agent 工具 trait
/// 
/// 定义 Agent 可调用的工具的接口
#[async_trait]
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &str;
    fn label(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value; // JSON Schema

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
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
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
    // Agent 生命周期
    BeforeAgentStart,
    AgentStart,
    AgentEnd { messages: Vec<AgentMessage> },
    /// Agent 即将结束（可拦截）
    BeforeAgentEnd { messages: Vec<AgentMessage> },

    // 上下文窗口警告
    ContextWarning {
        usage_percent: f64,
        total_tokens: usize,
        context_window: usize,
    },

    // Turn 生命周期
    TurnStart,
    TurnEnd {
        message: AgentMessage,
        tool_results: Vec<ToolResultMessage>,
    },
    /// Turn 执行出错
    TurnError {
        error: String,
        turn_index: usize,
    },

    // Message 生命周期
    MessageStart { message: AgentMessage },
    MessageUpdate {
        message: AgentMessage,
        event: AssistantMessageEvent,
    },
    MessageEnd { message: AgentMessage },
    /// 消息流式分块（用于流式传输中间内容）
    MessageChunk {
        message: AgentMessage,
        chunk_index: usize,
    },
    /// 消息处理出错
    MessageError {
        error: String,
    },

    // Tool 执行生命周期
    BeforeToolCall {
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
    },
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        args: serde_json::Value,
        partial_result: AgentToolResult,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: AgentToolResult,
        is_error: bool,
    },
    AfterToolCall {
        tool_call_id: String,
        tool_name: String,
        result: AgentToolResult,
        is_error: bool,
    },
    /// 工具执行出错
    ToolError {
        tool_call_id: String,
        tool_name: String,
        error: String,
    },

    // Slash 命令生命周期
    BeforeCommandExecute {
        command: String,
        args: String,
    },
    AfterCommandExecute {
        command: String,
        result: String,
    },
    /// 命令执行出错
    CommandError {
        command: String,
        error: String,
    },

    // 扩展生命周期
    ExtensionLoaded {
        name: String,
        version: String,
    },
    ExtensionError {
        name: String,
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
    pub system_prompt: String,
    pub model: Model,
    pub thinking_level: ThinkingLevel,
    pub tools: Vec<Arc<dyn AgentTool>>,
    pub messages: Vec<AgentMessage>,
    pub is_streaming: bool,
    pub streaming_message: Option<AgentMessage>,
    pub pending_tool_calls: HashSet<String>,
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
    All,
    #[default]
    OneAtATime,
}


/// 待处理消息队列
/// 
/// 管理待处理消息的缓冲队列
pub struct PendingMessageQueue {
    messages: Vec<AgentMessage>,
    pub mode: QueueMode,
}

impl PendingMessageQueue {
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
