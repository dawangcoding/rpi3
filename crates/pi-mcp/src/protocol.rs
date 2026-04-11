//! MCP (Model Context Protocol) 协议类型定义
//!
//! 基于 JSON-RPC 2.0 规范实现 MCP 协议的消息类型

use serde::{Deserialize, Serialize};

// === JSON-RPC 2.0 基础类型 ===

/// JSON-RPC 请求 ID
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    /// 数字 ID
    Number(i64),
    /// 字符串 ID
    String(String),
}

impl Default for RequestId {
    fn default() -> Self {
        RequestId::Number(1)
    }
}

impl From<i64> for RequestId {
    fn from(n: i64) -> Self {
        RequestId::Number(n)
    }
}

impl From<String> for RequestId {
    fn from(s: String) -> Self {
        RequestId::String(s)
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestId::Number(n) => write!(f, "{}", n),
            RequestId::String(s) => write!(f, "{}", s),
        }
    }
}

/// JSON-RPC 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC 版本，始终为 "2.0"
    pub jsonrpc: String,
    /// 请求 ID
    pub id: RequestId,
    /// 方法名
    pub method: String,
    /// 请求参数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// 创建新的 JSON-RPC 请求
    pub fn new(id: RequestId, method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC 版本
    pub jsonrpc: String,
    /// 对应的请求 ID
    pub id: RequestId,
    /// 成功结果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// 创建成功响应
    pub fn success(id: RequestId, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// 创建错误响应
    pub fn error(id: RequestId, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// 检查是否为错误响应
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// JSON-RPC 错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// 错误码
    pub code: i32,
    /// 错误消息
    pub message: String,
    /// 额外错误数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// 标准错误码：解析错误
    pub const PARSE_ERROR: i32 = -32700;
    /// 标准错误码：无效请求
    pub const INVALID_REQUEST: i32 = -32600;
    /// 标准错误码：方法不存在
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// 标准错误码：无效参数
    pub const INVALID_PARAMS: i32 = -32602;
    /// 标准错误码：内部错误
    pub const INTERNAL_ERROR: i32 = -32603;

    /// 创建新的错误
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// 创建带数据的错误
    pub fn with_data(code: i32, message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JsonRpcError(code={}, message={}", self.code, self.message)?;
        if let Some(ref data) = self.data {
            write!(f, ", data={}", data)?;
        }
        write!(f, ")")
    }
}

impl std::error::Error for JsonRpcError {}

/// JSON-RPC 通知（无 ID）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// JSON-RPC 版本
    pub jsonrpc: String,
    /// 方法名
    pub method: String,
    /// 通知参数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    /// 创建新的通知
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
        }
    }
}

// === MCP 特定类型 ===

/// 客户端能力
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCapabilities {
    /// 根目录能力
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,
    /// 采样能力
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<serde_json::Value>,
}

/// 根目录能力
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootsCapability {
    /// 是否支持 listChanged 通知
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
}

/// 服务器能力
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    /// 工具能力
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    /// 资源能力
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    /// 提示能力
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<serde_json::Value>,
    /// 日志能力
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<serde_json::Value>,
}

/// 工具能力
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapability {
    /// 是否支持 listChanged 通知
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// 资源能力
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesCapability {
    /// 是否支持 listChanged 通知
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
    /// 是否支持订阅
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
}

/// 实现信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    /// 名称
    pub name: String,
    /// 版本
    pub version: String,
}

impl Implementation {
    /// 创建新的实现信息
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// Initialize 请求参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// 协议版本
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    /// 客户端能力
    pub capabilities: ClientCapabilities,
    /// 客户端信息
    #[serde(rename = "clientInfo")]
    pub client_info: Implementation,
}

impl Default for InitializeParams {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation::new("pi-mcp", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// Initialize 响应结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// 协议版本
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    /// 服务器能力
    pub capabilities: ServerCapabilities,
    /// 服务器信息
    #[serde(rename = "serverInfo")]
    pub server_info: Implementation,
}

/// MCP Tool 定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// 工具名称
    pub name: String,
    /// 工具描述
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 输入参数 Schema (JSON Schema)
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

impl McpTool {
    /// 创建新的工具定义
    pub fn new(
        name: impl Into<String>,
        description: Option<String>,
        input_schema: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description,
            input_schema,
        }
    }
}

/// tools/list 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsResult {
    /// 工具列表
    pub tools: Vec<McpTool>,
    /// 分页游标
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// tools/list 请求参数
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListToolsParams {
    /// 分页游标
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// tools/call 请求参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    /// 工具名称
    pub name: String,
    /// 工具参数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

impl CallToolParams {
    /// 创建新的调用参数
    pub fn new(name: impl Into<String>, arguments: Option<serde_json::Value>) -> Self {
        Self {
            name: name.into(),
            arguments,
        }
    }
}

/// tools/call 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    /// 内容列表
    pub content: Vec<ToolContent>,
    /// 是否为错误
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl CallToolResult {
    /// 创建文本结果
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: None,
        }
    }

    /// 创建错误结果
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: Some(true),
        }
    }
}

/// 工具内容
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    /// 文本内容
    #[serde(rename = "text")]
    Text {
        /// 文本内容
        text: String,
    },
    /// 图片内容
    #[serde(rename = "image")]
    Image {
        /// Base64 编码的图片数据
        data: String,
        /// MIME 类型
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    /// 资源内容
    #[serde(rename = "resource")]
    Resource {
        /// 资源内容
        resource: ResourceContent,
    },
}

impl ToolContent {
    /// 创建文本内容
    pub fn text(text: impl Into<String>) -> Self {
        ToolContent::Text { text: text.into() }
    }

    /// 创建图片内容
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        ToolContent::Image {
            data: data.into(),
            mime_type: mime_type.into(),
        }
    }

    /// 获取文本内容（如果是文本类型）
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ToolContent::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// 资源内容
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    /// 资源 URI
    pub uri: String,
    /// MIME 类型
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// 文本内容
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl ResourceContent {
    /// 创建文本资源内容
    pub fn text(uri: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            mime_type: Some("text/plain".to_string()),
            text: Some(text.into()),
        }
    }
}

/// 资源定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// 资源 URI
    pub uri: String,
    /// 资源名称
    pub name: String,
    /// 资源描述
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// MIME 类型
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// resources/list 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourcesResult {
    /// 资源列表
    pub resources: Vec<Resource>,
    /// 分页游标
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// resources/list 请求参数
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListResourcesParams {
    /// 分页游标
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// resources/read 请求参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceParams {
    /// 资源 URI
    pub uri: String,
}

impl ReadResourceParams {
    /// 创建新的读取参数
    pub fn new(uri: impl Into<String>) -> Self {
        Self { uri: uri.into() }
    }
}

/// resources/read 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceResult {
    /// 内容列表
    pub contents: Vec<ResourceContent>,
}

// === MCP 方法常量 ===

/// Initialize 方法
pub const METHOD_INITIALIZE: &str = "initialize";
/// Initialized 通知
pub const METHOD_NOTIFICATION_INITIALIZED: &str = "notifications/initialized";
/// tools/list 方法
pub const METHOD_TOOLS_LIST: &str = "tools/list";
/// tools/call 方法
pub const METHOD_TOOLS_CALL: &str = "tools/call";
/// resources/list 方法
pub const METHOD_RESOURCES_LIST: &str = "resources/list";
/// resources/read 方法
pub const METHOD_RESOURCES_READ: &str = "resources/read";
/// resources/templates/list 方法
pub const METHOD_RESOURCES_TEMPLATES_LIST: &str = "resources/templates/list";
/// prompts/list 方法
pub const METHOD_PROMPTS_LIST: &str = "prompts/list";
/// prompts/get 方法
pub const METHOD_PROMPTS_GET: &str = "prompts/get";
/// logging/setLevel 方法
pub const METHOD_LOGGING_SET_LEVEL: &str = "logging/setLevel";
/// roots/list 方法
pub const METHOD_ROOTS_LIST: &str = "roots/list";
/// sampling/createMessage 方法
pub const METHOD_SAMPLING_CREATE_MESSAGE: &str = "sampling/createMessage";

// === 协议版本常量 ===

/// MCP 协议版本
pub const PROTOCOL_VERSION: &str = "2024-11-05";
/// JSON-RPC 版本
pub const JSONRPC_VERSION: &str = "2.0";

// === 单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_id_serialization() {
        // 数字 ID
        let id = RequestId::Number(42);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "42");

        // 字符串 ID
        let id = RequestId::String("abc-123".to_string());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""abc-123""#);
    }

    #[test]
    fn test_request_id_deserialization() {
        // 数字 ID
        let id: RequestId = serde_json::from_str("42").unwrap();
        assert_eq!(id, RequestId::Number(42));

        // 字符串 ID
        let id: RequestId = serde_json::from_str(r#""abc-123""#).unwrap();
        assert_eq!(id, RequestId::String("abc-123".to_string()));
    }

    #[test]
    fn test_json_rpc_request() {
        let request = JsonRpcRequest::new(
            RequestId::Number(1),
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            })),
        );

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""jsonrpc":"2.0""#));
        assert!(json.contains(r#""id":1"#));
        assert!(json.contains(r#""method":"initialize""#));

        let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, RequestId::Number(1));
        assert_eq!(parsed.method, "initialize");
    }

    #[test]
    fn test_json_rpc_response_success() {
        let response = JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({ "result": "ok" }),
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""result":{"result":"ok"}"#));
        assert!(!json.contains("error"));

        let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        assert!(!parsed.is_error());
    }

    #[test]
    fn test_json_rpc_response_error() {
        let response = JsonRpcResponse::error(
            RequestId::Number(1),
            JsonRpcError::new(JsonRpcError::METHOD_NOT_FOUND, "Method not found"),
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("error"));
        assert!(json.contains("-32601"));

        let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_error());
        assert_eq!(parsed.error.unwrap().code, JsonRpcError::METHOD_NOT_FOUND);
    }

    #[test]
    fn test_json_rpc_notification() {
        let notification = JsonRpcNotification::new(
            "notifications/initialized",
            None,
        );

        let json = serde_json::to_string(&notification).unwrap();
        assert!(json.contains(r#""jsonrpc":"2.0""#));
        assert!(json.contains(r#""method":"notifications/initialized""#));
        assert!(!json.contains("id"));

        let parsed: JsonRpcNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.method, "notifications/initialized");
    }

    #[test]
    fn test_initialize_params() {
        let params = InitializeParams::default();

        assert_eq!(params.protocol_version, PROTOCOL_VERSION);
        assert_eq!(params.client_info.name, "pi-mcp");

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains(r#""protocolVersion":"2024-11-05""#));
    }

    #[test]
    fn test_server_capabilities() {
        let caps = ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(true),
            }),
            resources: Some(ResourcesCapability {
                list_changed: None,
                subscribe: Some(true),
            }),
            prompts: None,
            logging: None,
        };

        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.contains(r#""tools":{"listChanged":true}"#));
        assert!(json.contains(r#""resources":{"subscribe":true}"#));
    }

    #[test]
    fn test_mcp_tool() {
        let tool = McpTool::new(
            "read_file",
            Some("Read a file from the filesystem".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        );

        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains(r#""name":"read_file""#));
        assert!(json.contains(r#""description":"Read a file from the filesystem""#));
        assert!(json.contains(r#""inputSchema""#));

        let parsed: McpTool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "read_file");
        assert!(parsed.description.is_some());
    }

    #[test]
    fn test_call_tool_params() {
        let params = CallToolParams::new(
            "read_file",
            Some(serde_json::json!({ "path": "/tmp/test.txt" })),
        );

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains(r#""name":"read_file""#));
        assert!(json.contains(r#""path":"/tmp/test.txt""#));
    }

    #[test]
    fn test_tool_content_text() {
        let content = ToolContent::text("Hello, world!");
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains(r#""text":"Hello, world!""#));

        let parsed: ToolContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_text(), Some("Hello, world!"));
    }

    #[test]
    fn test_tool_content_image() {
        let content = ToolContent::image("base64data", "image/png");
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""type":"image""#));
        assert!(json.contains(r#""mimeType":"image/png""#));
    }

    #[test]
    fn test_call_tool_result() {
        let result = CallToolResult::text("File contents here");
        assert!(result.is_error.is_none());
        assert_eq!(result.content.len(), 1);

        let error_result = CallToolResult::error("File not found");
        assert_eq!(error_result.is_error, Some(true));
    }

    #[test]
    fn test_resource() {
        let resource = Resource {
            uri: "file:///tmp/test.txt".to_string(),
            name: "test.txt".to_string(),
            description: Some("A test file".to_string()),
            mime_type: Some("text/plain".to_string()),
        };

        let json = serde_json::to_string(&resource).unwrap();
        assert!(json.contains(r#""uri":"file:///tmp/test.txt""#));
        assert!(json.contains(r#""mimeType":"text/plain""#));
    }

    #[test]
    fn test_list_tools_result() {
        let result = ListToolsResult {
            tools: vec![
                McpTool::new("tool1", None, serde_json::json!({})),
                McpTool::new("tool2", None, serde_json::json!({})),
            ],
            next_cursor: Some("next-page".to_string()),
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: ListToolsResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tools.len(), 2);
        assert_eq!(parsed.next_cursor, Some("next-page".to_string()));
    }

    // === 边界条件测试 ===

    #[test]
    fn test_request_id_from_i64() {
        let id: RequestId = 42i64.into();
        assert_eq!(id, RequestId::Number(42));
    }

    #[test]
    fn test_request_id_from_string() {
        let id: RequestId = "test-id".to_string().into();
        assert_eq!(id, RequestId::String("test-id".to_string()));
    }

    #[test]
    fn test_request_id_display() {
        let num_id = RequestId::Number(42);
        assert_eq!(format!("{}", num_id), "42");

        let str_id = RequestId::String("test".to_string());
        assert_eq!(format!("{}", str_id), "test");
    }

    #[test]
    fn test_request_id_default() {
        let id: RequestId = Default::default();
        assert_eq!(id, RequestId::Number(1));
    }

    #[test]
    fn test_json_rpc_request_no_params() {
        let request = JsonRpcRequest::new(
            RequestId::Number(1),
            "test",
            None,
        );

        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("params"));
    }

    #[test]
    fn test_json_rpc_response_is_error() {
        let success = JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({}),
        );
        assert!(!success.is_error());

        let error = JsonRpcResponse::error(
            RequestId::Number(1),
            JsonRpcError::new(JsonRpcError::INTERNAL_ERROR, "error"),
        );
        assert!(error.is_error());
    }

    #[test]
    fn test_json_rpc_error_constants() {
        assert_eq!(JsonRpcError::PARSE_ERROR, -32700);
        assert_eq!(JsonRpcError::INVALID_REQUEST, -32600);
        assert_eq!(JsonRpcError::METHOD_NOT_FOUND, -32601);
        assert_eq!(JsonRpcError::INVALID_PARAMS, -32602);
        assert_eq!(JsonRpcError::INTERNAL_ERROR, -32603);
    }

    #[test]
    fn test_json_rpc_error_with_data() {
        let error = JsonRpcError::with_data(
            JsonRpcError::INVALID_PARAMS,
            "Invalid params",
            serde_json::json!({ "field": "name" }),
        );

        assert_eq!(error.code, JsonRpcError::INVALID_PARAMS);
        assert_eq!(error.message, "Invalid params");
        assert!(error.data.is_some());
        assert_eq!(error.data.unwrap()["field"], "name");
    }

    #[test]
    fn test_json_rpc_error_display() {
        let error = JsonRpcError::new(JsonRpcError::INTERNAL_ERROR, "test error");
        let display = format!("{}", error);
        assert!(display.contains("JsonRpcError"));
        assert!(display.contains("-32603"));
        assert!(display.contains("test error"));
    }

    #[test]
    fn test_json_rpc_notification_no_params() {
        let notification = JsonRpcNotification::new("test", None);
        let json = serde_json::to_string(&notification).unwrap();
        assert!(!json.contains("params"));
    }

    #[test]
    fn test_implementation_new() {
        let impl_info = Implementation::new("test-server", "1.0.0");
        assert_eq!(impl_info.name, "test-server");
        assert_eq!(impl_info.version, "1.0.0");
    }

    #[test]
    fn test_initialize_params_default() {
        let params = InitializeParams::default();
        assert_eq!(params.protocol_version, PROTOCOL_VERSION);
        assert_eq!(params.client_info.name, "pi-mcp");
    }

    #[test]
    fn test_mcp_tool_without_description() {
        let tool = McpTool::new(
            "test_tool",
            None,
            serde_json::json!({"type": "object"}),
        );

        let json = serde_json::to_string(&tool).unwrap();
        assert!(!json.contains("description"));
    }

    #[test]
    fn test_mcp_tool_empty_schema() {
        let tool = McpTool::new("test", None, serde_json::json!({}));
        assert_eq!(tool.input_schema, serde_json::json!({}));
    }

    #[test]
    fn test_list_tools_params_default() {
        let params = ListToolsParams::default();
        assert!(params.cursor.is_none());
    }

    #[test]
    fn test_call_tool_params_new() {
        let params = CallToolParams::new("test_tool", Some(serde_json::json!({"arg": "value"})));
        assert_eq!(params.name, "test_tool");
        assert!(params.arguments.is_some());
    }

    #[test]
    fn test_call_tool_params_no_args() {
        let params = CallToolParams::new("test_tool", None);
        assert_eq!(params.name, "test_tool");
        assert!(params.arguments.is_none());
    }

    #[test]
    fn test_call_tool_result_text() {
        let result = CallToolResult::text("Hello, world!");
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.is_error, None);
        assert_eq!(result.content[0].as_text(), Some("Hello, world!"));
    }

    #[test]
    fn test_call_tool_result_error() {
        let result = CallToolResult::error("Something went wrong");
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_tool_content_as_text_none() {
        let image = ToolContent::image("base64data", "image/png");
        assert_eq!(image.as_text(), None);

        let resource = ToolContent::Resource {
            resource: ResourceContent::text("file:///test.txt", "content"),
        };
        assert_eq!(resource.as_text(), None);
    }

    #[test]
    fn test_resource_content_text() {
        let content = ResourceContent::text("file:///test.txt", "Hello, world!");
        assert_eq!(content.uri, "file:///test.txt");
        assert_eq!(content.mime_type, Some("text/plain".to_string()));
        assert_eq!(content.text, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_list_resources_params_default() {
        let params = ListResourcesParams::default();
        assert!(params.cursor.is_none());
    }

    #[test]
    fn test_read_resource_params_new() {
        let params = ReadResourceParams::new("file:///test.txt");
        assert_eq!(params.uri, "file:///test.txt");
    }

    #[test]
    fn test_client_capabilities_default() {
        let caps = ClientCapabilities::default();
        assert!(caps.roots.is_none());
        assert!(caps.sampling.is_none());
    }

    #[test]
    fn test_server_capabilities_default() {
        let caps = ServerCapabilities::default();
        assert!(caps.tools.is_none());
        assert!(caps.resources.is_none());
        assert!(caps.prompts.is_none());
        assert!(caps.logging.is_none());
    }

    #[test]
    fn test_tools_capability() {
        let caps = ToolsCapability { list_changed: Some(true) };
        assert_eq!(caps.list_changed, Some(true));

        let caps = ToolsCapability { list_changed: None };
        assert_eq!(caps.list_changed, None);
    }

    #[test]
    fn test_resources_capability() {
        let caps = ResourcesCapability {
            list_changed: Some(true),
            subscribe: Some(false),
        };
        assert_eq!(caps.list_changed, Some(true));
        assert_eq!(caps.subscribe, Some(false));
    }

    #[test]
    fn test_roots_capability() {
        let caps = RootsCapability { list_changed: true };
        assert!(caps.list_changed);
    }

    #[test]
    fn test_resource_without_optional_fields() {
        let resource = Resource {
            uri: "file:///test.txt".to_string(),
            name: "test.txt".to_string(),
            description: None,
            mime_type: None,
        };

        let json = serde_json::to_string(&resource).unwrap();
        assert!(!json.contains("description"));
        assert!(!json.contains("mimeType"));
    }

    #[test]
    fn test_list_tools_result_no_cursor() {
        let result = ListToolsResult {
            tools: vec![],
            next_cursor: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("nextCursor"));
    }

    #[test]
    fn test_list_resources_result() {
        let result = ListResourcesResult {
            resources: vec![],
            next_cursor: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("nextCursor"));
    }

    #[test]
    fn test_read_resource_result() {
        let result = ReadResourceResult {
            contents: vec![
                ResourceContent::text("file:///test1.txt", "content1"),
                ResourceContent::text("file:///test2.txt", "content2"),
            ],
        };

        assert_eq!(result.contents.len(), 2);
    }

    #[test]
    fn test_protocol_constants() {
        assert_eq!(PROTOCOL_VERSION, "2024-11-05");
        assert_eq!(JSONRPC_VERSION, "2.0");
        assert_eq!(METHOD_INITIALIZE, "initialize");
        assert_eq!(METHOD_NOTIFICATION_INITIALIZED, "notifications/initialized");
        assert_eq!(METHOD_TOOLS_LIST, "tools/list");
        assert_eq!(METHOD_TOOLS_CALL, "tools/call");
        assert_eq!(METHOD_RESOURCES_LIST, "resources/list");
        assert_eq!(METHOD_RESOURCES_READ, "resources/read");
    }

    #[test]
    fn test_request_id_serde_variants() {
        // 测试数字 ID 的序列化和反序列化
        let num_id = RequestId::Number(12345);
        let json = serde_json::to_string(&num_id).unwrap();
        assert_eq!(json, "12345");
        let parsed: RequestId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, num_id);

        // 测试字符串 ID 的序列化和反序列化
        let str_id = RequestId::String("abc-xyz-123".to_string());
        let json = serde_json::to_string(&str_id).unwrap();
        assert_eq!(json, "\"abc-xyz-123\"");
        let parsed: RequestId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, str_id);
    }

    #[test]
    fn test_invalid_json_rpc_response() {
        // 测试既没有 result 也没有 error 的响应
        let json = r#"{"jsonrpc":"2.0","id":1}"#;
        let response: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(!response.is_error());
        assert!(response.result.is_none());
    }

    #[test]
    fn test_json_rpc_error_clone() {
        let error = JsonRpcError::with_data(
            JsonRpcError::INVALID_PARAMS,
            "test",
            serde_json::json!({"key": "value"}),
        );
        let cloned = error.clone();
        assert_eq!(error.code, cloned.code);
        assert_eq!(error.message, cloned.message);
    }
}
