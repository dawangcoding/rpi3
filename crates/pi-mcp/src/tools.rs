//! MCP 工具桥接层
//! 
//! 将 MCP 工具转换为 pi-ai 的 Tool 类型，并处理工具调用转发

use crate::protocol::{CallToolResult, McpTool, ToolContent};

/// 将 MCP Tool 转换为 pi-ai Tool 格式
/// 
/// # Arguments
/// * `mcp_tool` - MCP 工具定义
/// * `server_name` - MCP Server 名称，用于命名空间隔离
/// 
/// # Returns
/// 返回带有命名空间的 pi-ai Tool
pub fn mcp_tool_to_ai_tool(mcp_tool: &McpTool, server_name: &str) -> pi_ai::types::Tool {
    let namespaced_name = format!("mcp_{}_{}", server_name, mcp_tool.name);
    let description = mcp_tool.description.clone().unwrap_or_default();
    
    pi_ai::types::Tool::new(
        namespaced_name,
        format!("[MCP:{}] {}", server_name, description),
        mcp_tool.input_schema.clone(),
    )
}

/// 将 MCP CallToolResult 转换为文本内容
/// 
/// 遍历结果中的所有内容块，提取文本内容
/// 
/// # Arguments
/// * `result` - MCP 工具调用结果
/// 
/// # Returns
/// 返回拼接后的文本内容
pub fn call_result_to_text(result: &CallToolResult) -> String {
    result.content.iter().map(|c| match c {
        ToolContent::Text { text } => text.clone(),
        ToolContent::Image { data, mime_type } => format!("[Image: {} bytes, {}]", data.len(), mime_type),
        ToolContent::Resource { resource } => {
            resource.text.clone().unwrap_or_else(|| format!("[Resource: {}]", resource.uri))
        }
    }).collect::<Vec<_>>().join("\n")
}

/// 从 namespaced 工具名中提取 server name 和原始工具名
/// 
/// MCP 工具名称格式: `mcp_{server_name}_{tool_name}`
/// 
/// # Arguments
/// * `namespaced` - 带命名空间的工具名称
/// 
/// # Returns
/// 返回 `(server_name, tool_name)` 元组，如果格式不匹配则返回 None
/// 
/// # Examples
/// ```
/// use pi_mcp::tools::parse_mcp_tool_name;
/// 
/// let result = parse_mcp_tool_name("mcp_filesystem_read_file");
/// assert_eq!(result, Some(("filesystem".to_string(), "read_file".to_string())));
/// 
/// let result = parse_mcp_tool_name("read_file");
/// assert_eq!(result, None);
/// ```
pub fn parse_mcp_tool_name(namespaced: &str) -> Option<(String, String)> {
    if !namespaced.starts_with("mcp_") {
        return None;
    }
    let rest = &namespaced[4..]; // 跳过 "mcp_"
    let parts: Vec<&str> = rest.splitn(2, '_').collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{McpTool, CallToolResult, ToolContent, ResourceContent};

    #[test]
    fn test_mcp_tool_to_ai_tool() {
        let mcp_tool = McpTool::new(
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
        
        let ai_tool = mcp_tool_to_ai_tool(&mcp_tool, "filesystem");
        
        assert_eq!(ai_tool.name, "mcp_filesystem_read_file");
        assert!(ai_tool.description.contains("[MCP:filesystem]"));
        assert!(ai_tool.description.contains("Read a file from the filesystem"));
    }

    #[test]
    fn test_mcp_tool_to_ai_tool_without_description() {
        let mcp_tool = McpTool::new(
            "simple_tool",
            None,
            serde_json::json!({"type": "object"}),
        );
        
        let ai_tool = mcp_tool_to_ai_tool(&mcp_tool, "test");
        
        assert_eq!(ai_tool.name, "mcp_test_simple_tool");
        assert_eq!(ai_tool.description, "[MCP:test] ");
    }

    #[test]
    fn test_call_result_to_text_single_text() {
        let result = CallToolResult::text("Hello, world!");
        
        let text = call_result_to_text(&result);
        assert_eq!(text, "Hello, world!");
    }

    #[test]
    fn test_call_result_to_text_multiple_contents() {
        let result = CallToolResult {
            content: vec![
                ToolContent::Text { text: "First line".to_string() },
                ToolContent::Text { text: "Second line".to_string() },
            ],
            is_error: None,
        };
        
        let text = call_result_to_text(&result);
        assert_eq!(text, "First line\nSecond line");
    }

    #[test]
    fn test_call_result_to_text_with_image() {
        let result = CallToolResult {
            content: vec![
                ToolContent::Text { text: "Here is an image:".to_string() },
                ToolContent::Image {
                    data: "base64imagedata".to_string(),
                    mime_type: "image/png".to_string(),
                },
            ],
            is_error: None,
        };
        
        let text = call_result_to_text(&result);
        assert!(text.contains("Here is an image:"));
        assert!(text.contains("[Image:"));
        assert!(text.contains("image/png"));
    }

    #[test]
    fn test_call_result_to_text_with_resource() {
        let result = CallToolResult {
            content: vec![
                ToolContent::Resource {
                    resource: ResourceContent::text("file:///test.txt", "File contents"),
                },
            ],
            is_error: None,
        };
        
        let text = call_result_to_text(&result);
        assert_eq!(text, "File contents");
    }

    #[test]
    fn test_call_result_to_text_with_resource_no_text() {
        let result = CallToolResult {
            content: vec![
                ToolContent::Resource {
                    resource: ResourceContent {
                        uri: "file:///binary.bin".to_string(),
                        mime_type: Some("application/octet-stream".to_string()),
                        text: None,
                    },
                },
            ],
            is_error: None,
        };
        
        let text = call_result_to_text(&result);
        assert!(text.contains("[Resource: file:///binary.bin]"));
    }

    #[test]
    fn test_parse_mcp_tool_name_valid() {
        let result = parse_mcp_tool_name("mcp_filesystem_read_file");
        assert_eq!(result, Some(("filesystem".to_string(), "read_file".to_string())));
        
        let result = parse_mcp_tool_name("mcp_github_create_issue");
        assert_eq!(result, Some(("github".to_string(), "create_issue".to_string())));
    }

    #[test]
    fn test_parse_mcp_tool_name_with_underscores() {
        // 工具名本身包含下划线的情况
        let result = parse_mcp_tool_name("mcp_server_my_tool_name");
        assert_eq!(result, Some(("server".to_string(), "my_tool_name".to_string())));
    }

    #[test]
    fn test_parse_mcp_tool_name_invalid_no_prefix() {
        let result = parse_mcp_tool_name("read_file");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_mcp_tool_name_invalid_only_prefix() {
        let result = parse_mcp_tool_name("mcp_");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_mcp_tool_name_invalid_no_tool_name() {
        let result = parse_mcp_tool_name("mcp_server");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_mcp_tool_name_empty() {
        let result = parse_mcp_tool_name("");
        assert_eq!(result, None);
    }

    // === 边界条件测试 ===

    #[test]
    fn test_mcp_tool_to_ai_tool_empty_schema() {
        let mcp_tool = McpTool::new(
            "empty_tool",
            Some("A tool with empty schema".to_string()),
            serde_json::json!({}),
        );
        
        let ai_tool = mcp_tool_to_ai_tool(&mcp_tool, "test-server");
        
        assert_eq!(ai_tool.name, "mcp_test-server_empty_tool");
        assert_eq!(ai_tool.parameters, serde_json::json!({}));
    }

    #[test]
    fn test_mcp_tool_to_ai_tool_complex_schema() {
        let mcp_tool = McpTool::new(
            "complex_tool",
            Some("A complex tool".to_string()),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "age": { "type": "integer", "minimum": 0 },
                    "email": { "type": "string", "format": "email" },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["name", "email"]
            }),
        );
        
        let ai_tool = mcp_tool_to_ai_tool(&mcp_tool, "test-server");
        
        assert_eq!(ai_tool.name, "mcp_test-server_complex_tool");
        assert!(ai_tool.description.contains("A complex tool"));
        assert!(ai_tool.parameters.get("properties").is_some());
    }

    #[test]
    fn test_call_result_to_text_empty() {
        let result = CallToolResult {
            content: vec![],
            is_error: None,
        };
        
        let text = call_result_to_text(&result);
        assert_eq!(text, "");
    }

    #[test]
    fn test_call_result_to_text_only_images() {
        let result = CallToolResult {
            content: vec![
                ToolContent::Image {
                    data: "base64data1".to_string(),
                    mime_type: "image/png".to_string(),
                },
                ToolContent::Image {
                    data: "base64data2".to_string(),
                    mime_type: "image/jpeg".to_string(),
                },
            ],
            is_error: None,
        };
        
        let text = call_result_to_text(&result);
        assert!(text.contains("[Image:"));
        assert!(text.contains("image/png"));
        assert!(text.contains("image/jpeg"));
        assert!(text.contains("11 bytes")); // base64data1 length
        assert!(text.contains("11 bytes")); // base64data2 length
    }

    #[test]
    fn test_call_result_to_text_mixed_content() {
        let result = CallToolResult {
            content: vec![
                ToolContent::Text { text: "First".to_string() },
                ToolContent::Image {
                    data: "img".to_string(),
                    mime_type: "image/png".to_string(),
                },
                ToolContent::Text { text: "Last".to_string() },
            ],
            is_error: None,
        };
        
        let text = call_result_to_text(&result);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "First");
        assert!(lines[1].contains("[Image:"));
        assert_eq!(lines[2], "Last");
    }

    #[test]
    fn test_call_result_to_text_error_result() {
        let result = CallToolResult::error("Error: File not found");
        let text = call_result_to_text(&result);
        assert_eq!(text, "Error: File not found");
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_parse_mcp_tool_name_single_underscore() {
        // 只有 mcp_ 前缀，没有 server 和 tool name
        let result = parse_mcp_tool_name("mcp_");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_mcp_tool_name_multiple_underscores() {
        // 工具名包含多个下划线
        let result = parse_mcp_tool_name("mcp_server_tool_with_many_underscores");
        assert_eq!(result, Some(("server".to_string(), "tool_with_many_underscores".to_string())));
    }

    #[test]
    fn test_parse_mcp_tool_name_unicode() {
        // 包含 Unicode 字符
        let result = parse_mcp_tool_name("mcp_服务器_工具");
        assert_eq!(result, Some(("服务器".to_string(), "工具".to_string())));
    }

    #[test]
    fn test_parse_mcp_tool_name_numbers() {
        let result = parse_mcp_tool_name("mcp_server123_tool456");
        assert_eq!(result, Some(("server123".to_string(), "tool456".to_string())));
    }

    #[test]
    fn test_mcp_tool_to_ai_tool_long_name() {
        let long_name = "a".repeat(100);
        let mcp_tool = McpTool::new(
            &long_name,
            None,
            serde_json::json!({}),
        );
        
        let ai_tool = mcp_tool_to_ai_tool(&mcp_tool, "server");
        assert!(ai_tool.name.contains(&long_name));
        assert!(ai_tool.name.starts_with("mcp_server_"));
    }

    #[test]
    fn test_mcp_tool_to_ai_tool_special_chars_in_name() {
        let mcp_tool = McpTool::new(
            "tool-with-dashes.and.dots",
            None,
            serde_json::json!({}),
        );
        
        let ai_tool = mcp_tool_to_ai_tool(&mcp_tool, "test-server");
        assert_eq!(ai_tool.name, "mcp_test-server_tool-with-dashes.and.dots");
    }

    #[test]
    fn test_resource_content_with_mime_type() {
        let resource = ResourceContent {
            uri: "file:///test.png".to_string(),
            mime_type: Some("image/png".to_string()),
            text: None,
        };
        
        let result = CallToolResult {
            content: vec![ToolContent::Resource { resource }],
            is_error: None,
        };
        
        let text = call_result_to_text(&result);
        assert!(text.contains("[Resource: file:///test.png]"));
    }

    #[test]
    fn test_tool_content_text_variants() {
        let text_content = ToolContent::text("plain text");
        assert_eq!(text_content.as_text(), Some("plain text"));

        let image_content = ToolContent::image("data", "image/png");
        assert_eq!(image_content.as_text(), None);
    }

    #[test]
    fn test_mcp_tool_clone() {
        let tool = McpTool::new(
            "test_tool",
            Some("description".to_string()),
            serde_json::json!({"type": "object"}),
        );
        let cloned = tool.clone();
        assert_eq!(tool.name, cloned.name);
        assert_eq!(tool.description, cloned.description);
        assert_eq!(tool.input_schema, cloned.input_schema);
    }

    #[test]
    fn test_mcp_tool_debug() {
        let tool = McpTool::new("test", None, serde_json::json!({}));
        let debug_str = format!("{:?}", tool);
        assert!(debug_str.contains("McpTool"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_call_tool_result_debug() {
        let result = CallToolResult::text("test");
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("CallToolResult"));
    }

    #[test]
    fn test_tool_content_debug() {
        let content = ToolContent::text("test");
        let debug_str = format!("{:?}", content);
        assert!(debug_str.contains("Text"));
    }
}
