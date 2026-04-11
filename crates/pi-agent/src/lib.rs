//! Agent 运行时核心库
#![warn(missing_docs)]
//!
//! pi-agent 是 AI Agent 的运行时核心，提供以下功能：
//!
//! - **Agent Loop**: 核心事件循环，处理消息流、工具调用和转向消息
//! - **消息队列**: 支持 steering（中途注入）和 follow-up（后续追加）消息队列
//! - **工具框架**: 定义 [`AgentTool`] trait 和工具执行模式（并行/串行）
//! - **上下文管理**: 管理对话上下文窗口和消息历史
//!
//! # 核心类型
//!
//! - [`Agent`]: 有状态的 Agent 包装器，管理生命周期和事件订阅
//! - [`AgentLoopConfig`]: 核心循环配置，控制 LLM 流式响应和工具调用行为
//! - [`AgentTool`]: 工具 trait，所有内置和扩展工具都需要实现此接口
//! - [`AgentEvent`]: 事件系统，用于监听 Agent 运行时的各种事件
//!
//! # 示例
//!
//! ```ignore
//! use pi_agent::{Agent, AgentOptions};
//!
//! let options = AgentOptions {
//!     model: Some(model),
//!     system_prompt: Some("You are a helpful assistant".to_string()),
//!     ..Default::default()
//! };
//! let agent = Agent::new(options);
//! agent.prompt_text("Hello!").await?;
//! ```

/// 类型定义模块
pub mod types;
/// Agent 模块
pub mod agent;
/// Agent 循环模块
pub mod agent_loop;
/// 上下文管理器模块
pub mod context_manager;

#[cfg(test)]
pub mod test_fixtures;

// 重导出核心类型
pub use types::{
    AgentContext, AgentEvent, AgentMessage, AgentState, AgentTool, AgentToolResult,
    AfterToolCallResult, BeforeToolCallResult, QueueMode, PendingMessageQueue,
    ToolCallContext, ToolExecutionMode,
};

pub use agent::{Agent, AgentOptions, default_convert_to_llm};
pub use agent_loop::{AgentLoopConfig, run_agent_loop, run_agent_loop_continue};
pub use context_manager::{ContextWindowManager, ContextUsage};
