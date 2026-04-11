//! # pi-mcp: MCP (Model Context Protocol) 客户端实现
#![warn(missing_docs)]
//!
//! 这个 crate 提供了完整的 MCP 协议客户端实现，用于与 MCP 服务器通信。
//!
//! ## 功能特性
//!
//! - 完整的 JSON-RPC 2.0 消息类型支持
//! - 多种传输方式：Stdio、SSE
//! - 自动握手流程
//! - 工具发现和调用
//! - 资源读取
//!
//! ## 快速开始
//!
//! ```no_run
//! use pi_mcp::{McpClient, StdioTransport, Transport};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // 创建 Stdio 传输
//!     let transport = StdioTransport::new("mcp-server", &["--stdio"])?;
//!     
//!     // 创建客户端
//!     let mut client = McpClient::new(Box::new(transport));
//!     
//!     // 执行握手
//!     client.handshake().await?;
//!     
//!     // 列出工具
//!     let tools = client.list_tools().await?;
//!     for tool in &tools {
//!         println!("Tool: {}", tool.name);
//!     }
//!     
//!     // 调用工具
//!     let result = client.call_tool("read_file", Some(serde_json::json!({
//!         "path": "/tmp/test.txt"
//!     }))).await?;
//!     
//!     // 关闭客户端
//!     client.close().await?;
//!     
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod protocol;
pub mod server;
pub mod tools;
pub mod transport;

// 重导出 tools 模块的公共 API
pub use tools::{call_result_to_text, mcp_tool_to_ai_tool, parse_mcp_tool_name};

// 重导出公共 API
pub use client::{ClientState, McpClient, McpClientError};
pub use server::{McpServerConfig, McpServerManager, ServerStatus};
pub use protocol::{
    // JSON-RPC 类型
    JsonRpcError,
    JsonRpcNotification,
    JsonRpcRequest,
    JsonRpcResponse,
    RequestId,
    // MCP 类型
    CallToolParams,
    CallToolResult,
    ClientCapabilities,
    Implementation,
    InitializeParams,
    InitializeResult,
    ListResourcesParams,
    ListResourcesResult,
    ListToolsParams,
    ListToolsResult,
    McpTool,
    ReadResourceParams,
    ReadResourceResult,
    Resource,
    ResourceContent,
    ResourcesCapability,
    RootsCapability,
    ServerCapabilities,
    ToolContent,
    ToolsCapability,
    // 方法常量
    JSONRPC_VERSION,
    METHOD_INITIALIZE,
    METHOD_LOGGING_SET_LEVEL,
    METHOD_NOTIFICATION_INITIALIZED,
    METHOD_PROMPTS_GET,
    METHOD_PROMPTS_LIST,
    METHOD_RESOURCES_LIST,
    METHOD_RESOURCES_READ,
    METHOD_RESOURCES_TEMPLATES_LIST,
    METHOD_ROOTS_LIST,
    METHOD_SAMPLING_CREATE_MESSAGE,
    METHOD_TOOLS_CALL,
    METHOD_TOOLS_LIST,
    PROTOCOL_VERSION,
};
pub use transport::{
    IncomingMessage,
    SseTransport,
    StdioTransport,
    Transport,
    TransportType,
};

// 仅在测试中导出 MockTransport
#[cfg(test)]
pub use transport::MockTransport;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version() {
        assert_eq!(PROTOCOL_VERSION, "2024-11-05");
        assert_eq!(JSONRPC_VERSION, "2.0");
    }

    #[test]
    fn test_method_constants() {
        assert_eq!(METHOD_INITIALIZE, "initialize");
        assert_eq!(METHOD_TOOLS_LIST, "tools/list");
        assert_eq!(METHOD_TOOLS_CALL, "tools/call");
        assert_eq!(METHOD_RESOURCES_LIST, "resources/list");
        assert_eq!(METHOD_RESOURCES_READ, "resources/read");
    }

    #[test]
    fn test_transport_type() {
        let t = TransportType::Stdio;
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#""stdio""#);

        let t: TransportType = serde_json::from_str(r#""sse""#).unwrap();
        assert!(matches!(t, TransportType::Sse));
    }
}
