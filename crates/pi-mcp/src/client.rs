//! MCP 客户端实现
//!
//! 提供与 MCP 服务器通信的高级接口

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use tracing::{debug, info, warn};

use crate::protocol::*;
use crate::transport::{IncomingMessage, Transport};

/// MCP 客户端错误类型
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    /// 服务器返回错误
    #[error("Server error: {0}")]
    ServerError(JsonRpcError),

    /// 传输错误
    #[error("Transport error: {0}")]
    TransportError(#[from] anyhow::Error),

    /// JSON 序列化/反序列化错误
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// 握手失败
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    /// 超时
    #[error("Operation timed out")]
    Timeout,

    /// 客户端未初始化
    #[error("Client not initialized")]
    NotInitialized,

    /// 方法不支持
    #[error("Method not supported: {0}")]
    MethodNotSupported(String),

    /// 意外的响应类型
    #[error("Unexpected response type")]
    UnexpectedResponseType,
}

/// MCP 客户端状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// 未初始化
    Uninitialized,
    /// 已初始化（握手完成）
    Initialized,
    /// 已关闭
    Closed,
}

/// MCP 客户端
pub struct McpClient {
    /// 传输层
    transport: Box<dyn Transport>,
    /// 下一个请求 ID
    next_id: AtomicI64,
    /// 客户端状态
    state: ClientState,
    /// 服务器能力
    server_capabilities: Option<ServerCapabilities>,
    /// 服务器信息
    server_info: Option<Implementation>,
    /// 客户端信息
    client_info: Implementation,
    /// 客户端能力
    client_capabilities: ClientCapabilities,
    /// 响应超时时间
    timeout: Duration,
}

impl McpClient {
    /// 创建新的 MCP 客户端
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            next_id: AtomicI64::new(1),
            state: ClientState::Uninitialized,
            server_capabilities: None,
            server_info: None,
            client_info: Implementation::new("pi-mcp", env!("CARGO_PKG_VERSION")),
            client_capabilities: ClientCapabilities::default(),
            timeout: Duration::from_secs(30),
        }
    }

    /// 设置客户端信息
    pub fn with_client_info(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.client_info = Implementation::new(name, version);
        self
    }

    /// 设置客户端能力
    pub fn with_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.client_capabilities = capabilities;
        self
    }

    /// 设置超时时间
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 获取客户端状态
    pub fn state(&self) -> ClientState {
        self.state
    }

    /// 获取服务器能力
    pub fn server_capabilities(&self) -> Option<&ServerCapabilities> {
        self.server_capabilities.as_ref()
    }

    /// 获取服务器信息
    pub fn server_info(&self) -> Option<&Implementation> {
        self.server_info.as_ref()
    }

    /// 检查服务器是否支持工具
    pub fn has_tools_capability(&self) -> bool {
        self.server_capabilities
            .as_ref()
            .map(|c| c.tools.is_some())
            .unwrap_or(false)
    }

    /// 检查服务器是否支持资源
    pub fn has_resources_capability(&self) -> bool {
        self.server_capabilities
            .as_ref()
            .map(|c| c.resources.is_some())
            .unwrap_or(false)
    }

    /// 生成下一个请求 ID
    fn next_request_id(&self) -> RequestId {
        RequestId::Number(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    /// 发送请求并等待响应
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, McpClientError> {
        let id = self.next_request_id();
        let request = JsonRpcRequest::new(id.clone(), method, params);

        debug!(id = %id, method = %method, "Sending request");
        self.transport.send_request(&request).await?;

        // 接收响应
        let response = self.receive_response(&id).await?;

        if let Some(error) = response.error {
            return Err(McpClientError::ServerError(error));
        }

        response.result.ok_or(McpClientError::UnexpectedResponseType)
    }

    /// 接收指定 ID 的响应
    async fn receive_response(&mut self, expected_id: &RequestId) -> Result<JsonRpcResponse, McpClientError> {
        loop {
            let incoming = self.transport.receive().await?;

            match incoming {
                IncomingMessage::Response(response) => {
                    if &response.id == expected_id {
                        return Ok(response);
                    } else {
                        warn!(
                            expected = %expected_id,
                            actual = %response.id,
                            "Received response with unexpected ID"
                        );
                        // 继续等待正确的响应
                    }
                }
                IncomingMessage::Notification(notification) => {
                    debug!(method = %notification.method, "Received notification while waiting for response");
                    // 可以在这里处理通知，暂时忽略继续等待
                }
                IncomingMessage::Request(request) => {
                    debug!(method = %request.method, "Received server request while waiting for response");
                    // 可以在这里处理服务端请求，暂时忽略继续等待
                }
            }
        }
    }

    /// 执行 initialize 握手
    pub async fn initialize(&mut self) -> Result<InitializeResult, McpClientError> {
        if self.state != ClientState::Uninitialized {
            return Err(McpClientError::HandshakeFailed(
                "Client already initialized".to_string(),
            ));
        }

        let params = InitializeParams {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: self.client_capabilities.clone(),
            client_info: self.client_info.clone(),
        };

        let result_value = self
            .send_request(METHOD_INITIALIZE, Some(serde_json::to_value(params)?))
            .await?;

        let result: InitializeResult =
            serde_json::from_value(result_value).map_err(|e| {
                McpClientError::HandshakeFailed(format!("Failed to parse initialize result: {}", e))
            })?;

        // 保存服务器信息
        self.server_capabilities = Some(result.capabilities.clone());
        self.server_info = Some(result.server_info.clone());

        info!(
            server = %result.server_info.name,
            version = %result.server_info.version,
            protocol = %result.protocol_version,
            "MCP server initialized"
        );

        Ok(result)
    }

    /// 发送 initialized 通知
    pub async fn initialized(&mut self) -> Result<(), McpClientError> {
        if self.state != ClientState::Uninitialized {
            return Err(McpClientError::HandshakeFailed(
                "Client already initialized".to_string(),
            ));
        }

        let notification = JsonRpcNotification::new(METHOD_NOTIFICATION_INITIALIZED, None);
        self.transport.send_notification(&notification).await?;

        self.state = ClientState::Initialized;
        info!("Sent initialized notification, handshake complete");

        Ok(())
    }

    /// 执行完整的握手流程
    pub async fn handshake(&mut self) -> Result<InitializeResult, McpClientError> {
        let result = self.initialize().await?;
        self.initialized().await?;
        Ok(result)
    }

    /// 获取工具列表
    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, McpClientError> {
        self.ensure_initialized()?;

        if !self.has_tools_capability() {
            return Err(McpClientError::MethodNotSupported("tools".to_string()));
        }

        let mut all_tools = Vec::new();
        let mut cursor = None;

        loop {
            let params = ListToolsParams { cursor: cursor.clone() };
            let params_value = serde_json::to_value(params)?;

            let result_value = self
                .send_request(METHOD_TOOLS_LIST, Some(params_value))
                .await?;

            let result: ListToolsResult =
                serde_json::from_value(result_value).map_err(|e| {
                    McpClientError::TransportError(anyhow::anyhow!(
                        "Failed to parse tools list: {}",
                        e
                    ))
                })?;

            all_tools.extend(result.tools);

            if result.next_cursor.is_none() {
                break;
            }
            cursor = result.next_cursor;
        }

        debug!(count = all_tools.len(), "Retrieved tools list");
        Ok(all_tools)
    }

    /// 调用工具
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, McpClientError> {
        self.ensure_initialized()?;

        let params = CallToolParams::new(name, arguments);
        let params_value = serde_json::to_value(params)?;

        let result_value = self
            .send_request(METHOD_TOOLS_CALL, Some(params_value))
            .await?;

        let result: CallToolResult = serde_json::from_value(result_value).map_err(|e| {
            McpClientError::TransportError(anyhow::anyhow!("Failed to parse tool result: {}", e))
        })?;

        debug!(
            tool = %name,
            is_error = result.is_error.unwrap_or(false),
            content_count = result.content.len(),
            "Tool call completed"
        );

        Ok(result)
    }

    /// 获取资源列表
    pub async fn list_resources(&mut self) -> Result<Vec<Resource>, McpClientError> {
        self.ensure_initialized()?;

        if !self.has_resources_capability() {
            return Err(McpClientError::MethodNotSupported("resources".to_string()));
        }

        let mut all_resources = Vec::new();
        let mut cursor = None;

        loop {
            let params = ListResourcesParams { cursor: cursor.clone() };
            let params_value = serde_json::to_value(params)?;

            let result_value = self
                .send_request(METHOD_RESOURCES_LIST, Some(params_value))
                .await?;

            let result: ListResourcesResult =
                serde_json::from_value(result_value).map_err(|e| {
                    McpClientError::TransportError(anyhow::anyhow!(
                        "Failed to parse resources list: {}",
                        e
                    ))
                })?;

            all_resources.extend(result.resources);

            if result.next_cursor.is_none() {
                break;
            }
            cursor = result.next_cursor;
        }

        debug!(count = all_resources.len(), "Retrieved resources list");
        Ok(all_resources)
    }

    /// 读取资源
    pub async fn read_resource(&mut self, uri: &str) -> Result<ReadResourceResult, McpClientError> {
        self.ensure_initialized()?;

        let params = ReadResourceParams::new(uri);
        let params_value = serde_json::to_value(params)?;

        let result_value = self
            .send_request(METHOD_RESOURCES_READ, Some(params_value))
            .await?;

        let result: ReadResourceResult = serde_json::from_value(result_value).map_err(|e| {
            McpClientError::TransportError(anyhow::anyhow!(
                "Failed to parse resource content: {}",
                e
            ))
        })?;

        debug!(uri = %uri, "Read resource");
        Ok(result)
    }

    /// 确保客户端已初始化
    fn ensure_initialized(&self) -> Result<(), McpClientError> {
        if self.state != ClientState::Initialized {
            Err(McpClientError::NotInitialized)
        } else {
            Ok(())
        }
    }

    /// 关闭客户端
    pub async fn close(&mut self) -> Result<(), McpClientError> {
        if self.state == ClientState::Closed {
            return Ok(());
        }

        self.transport.close().await?;
        self.state = ClientState::Closed;
        info!("MCP client closed");

        Ok(())
    }

    /// 发送 ping 通知以检查连接健康状态
    /// 
    /// 返回 Ok(()) 表示连接正常，Err 表示连接有问题
    pub async fn ping(&mut self) -> Result<(), McpClientError> {
        self.ensure_initialized()?;
        
        // 使用空的 notification 作为 ping，或者使用标准的 ping 方法
        let notification = JsonRpcNotification::new("ping", None);
        self.transport.send_notification(&notification).await?;
        Ok(())
    }
}

// === 单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    fn _create_mock_client() -> (McpClient, MockTransport) {
        let mock = MockTransport::new();
        let _client = McpClient::new(Box::new(mock));
        // 需要重新获取 mock，因为 Box 移动了所有权
        // 这里简化处理，直接在测试中创建
        unimplemented!("Use create_mock_client_with_mock instead")
    }

    #[tokio::test]
    async fn test_handshake() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": true }
                },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));

        // 执行握手
        let result = client.handshake().await.unwrap();
        assert_eq!(result.server_info.name, "test-server");
        assert_eq!(client.state(), ClientState::Initialized);
        assert!(client.has_tools_capability());
    }

    #[tokio::test]
    async fn test_list_tools() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        // 添加 tools/list 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Read a file",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" }
                            }
                        }
                    }
                ]
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read_file");
    }

    #[tokio::test]
    async fn test_call_tool() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        // 添加 tools/call 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": "File contents here"
                    }
                ]
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let result = client
            .call_tool("read_file", Some(serde_json::json!({ "path": "/tmp/test.txt" })))
            .await
            .unwrap();

        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].as_text(), Some("File contents here"));
    }

    #[tokio::test]
    async fn test_call_tool_error() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        // 添加 tools/call 错误响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "content": [
                    {
                        "type": "text",
                        "text": "File not found"
                    }
                ],
                "isError": true
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let result = client
            .call_tool("read_file", Some(serde_json::json!({ "path": "/nonexistent" })))
            .await
            .unwrap();

        assert_eq!(result.is_error, Some(true));
    }

    #[tokio::test]
    async fn test_list_resources() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "resources": {}
                },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        // 添加 resources/list 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "resources": [
                    {
                        "uri": "file:///tmp/test.txt",
                        "name": "test.txt",
                        "mimeType": "text/plain"
                    }
                ]
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let resources = client.list_resources().await.unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "file:///tmp/test.txt");
    }

    #[tokio::test]
    async fn test_read_resource() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "resources": {}
                },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        // 添加 resources/read 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "contents": [
                    {
                        "uri": "file:///tmp/test.txt",
                        "mimeType": "text/plain",
                        "text": "Hello, world!"
                    }
                ]
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let result = client.read_resource("file:///tmp/test.txt").await.unwrap();
        assert_eq!(result.contents.len(), 1);
        assert_eq!(result.contents[0].text, Some("Hello, world!".to_string()));
    }

    #[tokio::test]
    async fn test_not_initialized_error() {
        let mock = MockTransport::new();
        let mut client = McpClient::new(Box::new(mock));

        // 未初始化时调用工具应该报错
        let result = client.call_tool("test", None).await;
        assert!(matches!(result, Err(McpClientError::NotInitialized)));
    }

    #[tokio::test]
    async fn test_method_not_supported() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应（无 tools 能力）
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        // 不支持 tools 时调用应该报错
        let result = client.list_tools().await;
        assert!(matches!(result, Err(McpClientError::MethodNotSupported(_))));
    }

    #[tokio::test]
    async fn test_server_error() {
        let mut mock = MockTransport::new();

        // 添加 initialize 错误响应
        mock.push_response(JsonRpcResponse::error(
            RequestId::Number(1),
            JsonRpcError::new(JsonRpcError::INTERNAL_ERROR, "Internal server error"),
        ));

        let mut client = McpClient::new(Box::new(mock));
        let result = client.initialize().await;

        assert!(matches!(result, Err(McpClientError::ServerError(_))));
    }

    #[tokio::test]
    async fn test_pagination() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "test-server", "version": "1.0.0" }
            }),
        ));

        // 第一页
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "tools": [
                    { "name": "tool1", "inputSchema": {} }
                ],
                "nextCursor": "page2"
            }),
        ));

        // 第二页
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(3),
            serde_json::json!({
                "tools": [
                    { "name": "tool2", "inputSchema": {} }
                ]
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "tool1");
        assert_eq!(tools[1].name, "tool2");
    }

    #[tokio::test]
    async fn test_client_configuration() {
        let mock = MockTransport::new();

        let client = McpClient::new(Box::new(mock))
            .with_client_info("my-client", "2.0.0")
            .with_timeout(Duration::from_secs(60));

        assert_eq!(client.client_info.name, "my-client");
        assert_eq!(client.client_info.version, "2.0.0");
        assert_eq!(client.timeout, Duration::from_secs(60));
    }

    // === 边界条件测试 ===

    #[tokio::test]
    async fn test_client_new_default() {
        let mock = MockTransport::new();
        let client = McpClient::new(Box::new(mock));

        assert_eq!(client.state(), ClientState::Uninitialized);
        assert!(client.server_capabilities().is_none());
        assert!(client.server_info().is_none());
        assert_eq!(client.client_info.name, "pi-mcp");
        assert_eq!(client.timeout, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_client_with_capabilities() {
        let mock = MockTransport::new();
        let capabilities = ClientCapabilities {
            roots: Some(RootsCapability { list_changed: true }),
            sampling: Some(serde_json::json!({})),
        };

        let client = McpClient::new(Box::new(mock))
            .with_capabilities(capabilities.clone());

        assert!(client.client_capabilities.roots.is_some());
        assert!(client.client_capabilities.sampling.is_some());
    }

    #[tokio::test]
    async fn test_client_capabilities_check() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应（有 tools 能力）
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": true },
                    "resources": { "subscribe": true }
                },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        assert!(client.has_tools_capability());
        assert!(client.has_resources_capability());
    }

    #[tokio::test]
    async fn test_client_capabilities_check_no_capabilities() {
        let mut mock = MockTransport::new();

        // 添加 initialize 响应（无能力）
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        assert!(!client.has_tools_capability());
        assert!(!client.has_resources_capability());
    }

    #[tokio::test]
    async fn test_initialize_already_initialized() {
        let mut mock = MockTransport::new();

        // 第一次 initialize 响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        // 再次 initialize 应该失败
        let result = client.initialize().await;
        assert!(matches!(result, Err(McpClientError::HandshakeFailed(_))));
    }

    #[tokio::test]
    async fn test_initialized_already_initialized() {
        let mut mock = MockTransport::new();

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        // 再次 initialized 应该失败
        let result = client.initialized().await;
        assert!(matches!(result, Err(McpClientError::HandshakeFailed(_))));
    }

    #[tokio::test]
    async fn test_list_tools_empty() {
        let mut mock = MockTransport::new();

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "tools": []
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let tools = client.list_tools().await.unwrap();
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_list_resources_empty() {
        let mut mock = MockTransport::new();

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "resources": {} },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "resources": []
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let resources = client.list_resources().await.unwrap();
        assert!(resources.is_empty());
    }

    #[tokio::test]
    async fn test_call_tool_no_args() {
        let mut mock = MockTransport::new();

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "content": [{ "type": "text", "text": "success" }]
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let result = client.call_tool("test_tool", None).await.unwrap();
        assert_eq!(result.content.len(), 1);
    }

    #[tokio::test]
    async fn test_read_resource_empty() {
        let mut mock = MockTransport::new();

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "resources": {} },
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(2),
            serde_json::json!({
                "contents": []
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        let result = client.read_resource("file:///empty.txt").await.unwrap();
        assert!(result.contents.is_empty());
    }

    #[tokio::test]
    async fn test_close_already_closed() {
        let mut mock = MockTransport::new();

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        // 第一次关闭
        client.close().await.unwrap();
        assert_eq!(client.state(), ClientState::Closed);

        // 再次关闭应该成功（幂等）
        client.close().await.unwrap();
    }

    #[tokio::test]
    async fn test_server_error_variants() {
        let mut mock = MockTransport::new();

        // 测试不同的错误码
        let error_codes = [
            JsonRpcError::PARSE_ERROR,
            JsonRpcError::INVALID_REQUEST,
            JsonRpcError::METHOD_NOT_FOUND,
            JsonRpcError::INVALID_PARAMS,
            JsonRpcError::INTERNAL_ERROR,
        ];

        for (i, code) in error_codes.iter().enumerate() {
            mock.push_response(JsonRpcResponse::error(
                RequestId::Number(i as i64 + 1),
                JsonRpcError::new(*code, format!("Error {}", i)),
            ));
        }

        let mut client = McpClient::new(Box::new(mock));

        for i in 0..error_codes.len() {
            let result = client.initialize().await;
            assert!(matches!(result, Err(McpClientError::ServerError(_))));
        }
    }

    #[tokio::test]
    async fn test_unexpected_response_id() {
        let mut mock = MockTransport::new();

        // 先添加一个错误 ID 的响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(999), // 错误的 ID
            serde_json::json!({}),
        ));

        // 再添加正确的响应
        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        // 应该能正确处理并等待正确的响应
        let result = client.initialize().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_state_equality() {
        assert_eq!(ClientState::Uninitialized, ClientState::Uninitialized);
        assert_eq!(ClientState::Initialized, ClientState::Initialized);
        assert_eq!(ClientState::Closed, ClientState::Closed);
        assert_ne!(ClientState::Uninitialized, ClientState::Initialized);
        assert_ne!(ClientState::Initialized, ClientState::Closed);
    }

    #[test]
    fn test_client_state_debug() {
        let state = ClientState::Initialized;
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("Initialized"));
    }

    #[test]
    fn test_mcp_client_error_display() {
        let error = McpClientError::NotInitialized;
        let display = format!("{}", error);
        assert!(display.contains("not initialized"));

        let error = McpClientError::Timeout;
        let display = format!("{}", error);
        assert!(display.contains("timed out"));

        let error = McpClientError::UnexpectedResponseType;
        let display = format!("{}", error);
        assert!(display.contains("Unexpected"));
    }

    #[tokio::test]
    async fn test_ping_not_initialized() {
        let mock = MockTransport::new();
        let mut client = McpClient::new(Box::new(mock));

        let result = client.ping().await;
        assert!(matches!(result, Err(McpClientError::NotInitialized)));
    }

    #[tokio::test]
    async fn test_ping_success() {
        let mut mock = MockTransport::new();

        mock.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "serverInfo": {
                    "name": "test-server",
                    "version": "1.0.0"
                }
            }),
        ));

        let mut client = McpClient::new(Box::new(mock));
        client.handshake().await.unwrap();

        // ping 使用 notification，不需要响应
        let result = client.ping().await;
        assert!(result.is_ok());
    }
}
