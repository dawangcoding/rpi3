//! HTML 导出器
//!
//! 将会话导出为自包含的 HTML 文件

use std::path::Path;
use anyhow::{Context, Result};
use pi_agent::types::AgentMessage;
use pi_ai::types::{ContentBlock, Message, UserContent};
use crate::core::session_manager::SavedSession;

use super::html_template::{CSS_STYLES, HTML_FOOTER, HTML_HEADER, JS_CODE};

/// 导出主题
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(dead_code)] // Dark 主题供未来使用
pub enum ExportTheme {
    /// 浅色主题
    #[default]
    Light,
    /// 深色主题
    Dark,
}

impl ExportTheme {
    fn as_class(&self) -> &'static str {
        match self {
            ExportTheme::Light => "light",
            ExportTheme::Dark => "dark",
        }
    }
}

/// HTML 导出器
#[derive(Debug, Clone)]
#[allow(dead_code)] // 字段供未来扩展使用
pub struct HtmlExporter {
    /// 导出主题
    theme: ExportTheme,
    /// 是否包含统计信息
    include_stats: bool,
    /// 是否折叠工具调用
    collapse_tool_calls: bool,
}

impl Default for HtmlExporter {
    fn default() -> Self {
        Self {
            theme: ExportTheme::default(),
            include_stats: true,
            collapse_tool_calls: true,
        }
    }
}

impl HtmlExporter {
    /// 创建新的 HTML 导出器
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置主题
    #[allow(dead_code)]
    pub fn with_theme(mut self, theme: ExportTheme) -> Self {
        self.theme = theme;
        self
    }

    /// 设置是否包含统计信息
    #[allow(dead_code)]
    pub fn with_stats(mut self, include: bool) -> Self {
        self.include_stats = include;
        self
    }

    /// 设置是否折叠工具调用
    #[allow(dead_code)]
    pub fn with_collapse_tool_calls(mut self, collapse: bool) -> Self {
        self.collapse_tool_calls = collapse;
        self
    }

    /// 导出会话到文件
    pub fn export_session(
        &self,
        session: &SavedSession,
        output_path: &Path,
    ) -> Result<()> {
        let html = self.render_session_to_html(session)?;
        std::fs::write(output_path, html)
            .with_context(|| format!("Failed to write HTML to {}", output_path.display()))?;
        Ok(())
    }

    /// 将会话渲染为 HTML 字符串
    pub fn render_session_to_html(&self, session: &SavedSession) -> Result<String> {
        let title = session
            .metadata
            .title
            .as_deref()
            .unwrap_or("Untitled Session");

        let meta_info = self.format_meta_info(session);
        let messages_html = self.render_messages(&session.messages)?;
        let stats_html = if self.include_stats {
            self.render_stats(session)
        } else {
            String::new()
        };

        let header = HTML_HEADER
            .replace("{title}", &escape_html(title))
            .replace("{css_styles}", CSS_STYLES)
            .replace("{theme_class}", self.theme.as_class())
            .replace("{meta_info}", &meta_info);

        let footer = HTML_FOOTER
            .replace("{js_code}", JS_CODE);

        Ok(format!("{}{}{}{}", header, messages_html, stats_html, footer))
    }

    /// 格式化元信息
    fn format_meta_info(&self, session: &SavedSession) -> String {
        let mut parts = Vec::new();

        // 模型信息
        if !session.metadata.model.is_empty() {
            parts.push(format!("Model: {}", escape_html(&session.metadata.model)));
        }

        // 消息数量
        parts.push(format!("Messages: {}", session.metadata.message_count));

        // 时间
        let created = format_timestamp(session.metadata.created_at);
        parts.push(format!("Created: {}", created));

        if session.metadata.updated_at != session.metadata.created_at {
            let updated = format_timestamp(session.metadata.updated_at);
            parts.push(format!("Updated: {}", updated));
        }

        parts.join(" | ")
    }

    /// 渲染消息列表
    fn render_messages(&self, messages: &[AgentMessage]) -> Result<String> {
        let mut html_parts = Vec::new();

        for (index, msg) in messages.iter().enumerate() {
            let msg_html = self.render_message(msg, index)?;
            html_parts.push(msg_html);
        }

        Ok(html_parts.join("\n"))
    }

    /// 渲染单条消息
    fn render_message(&self, msg: &AgentMessage, index: usize) -> Result<String> {
        match msg {
            AgentMessage::Llm(Message::User(user_msg)) => {
                self.render_user_message(user_msg, index)
            }
            AgentMessage::Llm(Message::Assistant(assistant_msg)) => {
                self.render_assistant_message(assistant_msg, index)
            }
            AgentMessage::Llm(Message::ToolResult(tool_result)) => {
                self.render_tool_result_message(tool_result, index)
            }
        }
    }

    /// 渲染用户消息
    fn render_user_message(&self, msg: &pi_ai::types::UserMessage, index: usize) -> Result<String> {
        let content_html = match &msg.content {
            UserContent::Text(text) => markdown_to_html(text),
            UserContent::Blocks(blocks) => {
                let texts: Vec<String> = blocks
                    .iter()
                    .filter_map(|block| {
                        if let ContentBlock::Text(t) = block {
                            Some(t.text.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                markdown_to_html(&texts.join("\n\n"))
            }
        };

        Ok(format!(
            r#"<div class="message user" id="msg-{index}">
<div class="role">User</div>
<div class="content">{content_html}</div>
</div>"#
        ))
    }

    /// 渲染助手消息
    fn render_assistant_message(
        &self,
        msg: &pi_ai::types::AssistantMessage,
        index: usize,
    ) -> Result<String> {
        let mut content_parts = Vec::new();

        for block in &msg.content {
            let block_html = self.render_content_block(block)?;
            content_parts.push(block_html);
        }

        let content_html = content_parts.join("\n");

        Ok(format!(
            r#"<div class="message assistant" id="msg-{index}">
<div class="role">Assistant</div>
<div class="content">{content_html}</div>
</div>"#
        ))
    }

    /// 渲染内容块
    fn render_content_block(&self, block: &ContentBlock) -> Result<String> {
        match block {
            ContentBlock::Text(text) => Ok(markdown_to_html(&text.text)),
            ContentBlock::Thinking(thinking) => {
                let text = &thinking.thinking;
                Ok(format!(
                    r#"<details class="thinking">
<summary>Thinking</summary>
<pre>{}</pre>
</details>"#,
                    escape_html(text)
                ))
            }
            ContentBlock::ToolCall(tool_call) => {
                let json_input = serde_json::to_string_pretty(&tool_call.arguments)
                    .unwrap_or_else(|_| tool_call.arguments.to_string());
                Ok(format!(
                    r#"<details class="tool-call">
<summary>Tool: {}</summary>
<pre><code>{}</code></pre>
</details>"#,
                    escape_html(&tool_call.name),
                    escape_html(&json_input)
                ))
            }
            ContentBlock::Image(_image) => {
                Ok(r#"<div class="image-placeholder">[Image]</div>"#.to_string())
            }
        }
    }

    /// 渲染工具结果消息
    fn render_tool_result_message(
        &self,
        msg: &pi_ai::types::ToolResultMessage,
        index: usize,
    ) -> Result<String> {
        let error_class = if msg.is_error { " error" } else { "" };

        // 提取内容文本
        let content_text: Vec<String> = msg
            .content
            .iter()
            .filter_map(|block| {
                if let ContentBlock::Text(t) = block {
                    Some(t.text.clone())
                } else {
                    None
                }
            })
            .collect();

        let content_html = if content_text.is_empty() {
            "<em>(No content)</em>".to_string()
        } else {
            let combined = content_text.join("\n");
            // 工具结果通常是代码或结构化数据，使用 <pre> 包裹
            format!(
                "<pre><code>{}</code></pre>",
                escape_html(&combined)
            )
        };

        Ok(format!(
            r#"<div class="message tool-result{error_class}" id="msg-{index}">
<div class="role">Tool Result: {}</div>
<div class="content">{content_html}</div>
</div>"#,
            escape_html(&msg.tool_name)
        ))
    }

    /// 渲染统计信息
    fn render_stats(&self, session: &SavedSession) -> String {
        // 计算统计
        let user_count = session
            .messages
            .iter()
            .filter(|m| matches!(m, AgentMessage::Llm(Message::User(_))))
            .count();
        let assistant_count = session
            .messages
            .iter()
            .filter(|m| matches!(m, AgentMessage::Llm(Message::Assistant(_))))
            .count();
        let tool_count = session
            .messages
            .iter()
            .filter(|m| matches!(m, AgentMessage::Llm(Message::ToolResult(_))))
            .count();

        format!(
            r#"<div class="stats">
<h3>Session Statistics</h3>
<div class="stats-grid">
<div class="stat-item"><span class="stat-label">User Messages:</span><span class="stat-value">{}</span></div>
<div class="stat-item"><span class="stat-label">Assistant Messages:</span><span class="stat-value">{}</span></div>
<div class="stat-item"><span class="stat-label">Tool Calls:</span><span class="stat-value">{}</span></div>
<div class="stat-item"><span class="stat-label">Total Messages:</span><span class="stat-value">{}</span></div>
</div>
</div>"#,
            user_count,
            assistant_count,
            tool_count,
            session.metadata.message_count
        )
    }
}

/// 将 Markdown 转换为 HTML
fn markdown_to_html(text: &str) -> String {
    use pulldown_cmark::{html::push_html, Options, Parser};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(text, options);
    let mut html = String::new();
    push_html(&mut html, parser);

    // 如果结果为空，返回原始文本的 HTML 转义版本
    if html.trim().is_empty() && !text.trim().is_empty() {
        format!("<p>{}</p>", escape_html(text))
    } else {
        html
    }
}

/// HTML 转义
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// 格式化时间戳
fn format_timestamp(millis: i64) -> String {
    use chrono::TimeZone;

    let dt = chrono::Utc.timestamp_millis_opt(millis);
    match dt.single() {
        Some(dt) => dt.format("%Y-%m-%d %H:%M UTC").to_string(),
        None => format!("{} ms", millis),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session_manager::{SavedSession, SessionMetadata};

    fn create_test_session(messages: Vec<AgentMessage>) -> SavedSession {
        SavedSession {
            metadata: SessionMetadata {
                id: "test-session".to_string(),
                title: Some("Test Session".to_string()),
                created_at: 1704067200000, // 2024-01-01
                updated_at: 1704153600000,
                message_count: messages.len(),
                model: "claude-3-opus".to_string(),
                parent_session_id: None,
                fork_at_index: None,
            },
            messages,
            compaction_history: vec![],
            stats: None,
        }
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("&"), "&amp;");
        assert_eq!(escape_html("\"test\""), "&quot;test&quot;");
    }

    #[test]
    fn test_markdown_to_html() {
        let md = "# Hello\n\nThis is **bold** and *italic*.";
        let html = markdown_to_html(md);
        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn test_export_theme() {
        assert_eq!(ExportTheme::Light.as_class(), "light");
        assert_eq!(ExportTheme::Dark.as_class(), "dark");
    }

    #[test]
    fn test_export_empty_session() {
        let session = create_test_session(vec![]);
        let exporter = HtmlExporter::new();
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证基本结构
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Test Session"));
        assert!(html.contains("Messages: 0"));
        assert!(html.contains("Model: claude-3-opus"));
    }

    #[test]
    fn test_export_with_messages() {
        let messages = vec![
            AgentMessage::user("Hello, AI!"),
            AgentMessage::Llm(Message::Assistant(pi_ai::types::AssistantMessage {
                content: vec![pi_ai::types::ContentBlock::Text(pi_ai::types::TextContent::new("Hello! How can I help you today?"))],
                model: "claude-3".to_string(),
                provider: pi_ai::types::Provider::Anthropic,
                stop_reason: pi_ai::types::StopReason::Stop,
                usage: pi_ai::types::Usage { input_tokens: 10, output_tokens: 20, cache_read_tokens: None, cache_write_tokens: None },
                api: pi_ai::types::Api::Anthropic,
                error_message: None,
                response_id: None,
                role: "assistant".to_string(),
                timestamp: 0,
            })),
        ];
        let session = create_test_session(messages);
        let exporter = HtmlExporter::new();
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证消息渲染
        assert!(html.contains("Hello, AI!"));
        assert!(html.contains("Hello! How can I help you today?"));
        assert!(html.contains("user"));
        assert!(html.contains("assistant"));
    }

    #[test]
    fn test_export_light_theme() {
        let session = create_test_session(vec![]);
        let exporter = HtmlExporter::new().with_theme(ExportTheme::Light);
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证亮色主题类
        assert!(html.contains("class=\"light\""));
    }

    #[test]
    fn test_export_dark_theme() {
        let session = create_test_session(vec![]);
        let exporter = HtmlExporter::new().with_theme(ExportTheme::Dark);
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证暗色主题类
        assert!(html.contains("class=\"dark\""));
    }

    #[test]
    fn test_export_tool_call_folding() {
        let messages = vec![
            AgentMessage::Llm(Message::Assistant(pi_ai::types::AssistantMessage {
                content: vec![
                    pi_ai::types::ContentBlock::ToolCall(pi_ai::types::ToolCall::new(
                        "call_123",
                        "read_file",
                        serde_json::json!({"path": "/test/file.txt"}),
                    )),
                ],
                model: "claude-3".to_string(),
                provider: pi_ai::types::Provider::Anthropic,
                stop_reason: pi_ai::types::StopReason::ToolUse,
                usage: pi_ai::types::Usage::default(),
                api: pi_ai::types::Api::Anthropic,
                error_message: None,
                response_id: None,
                role: "assistant".to_string(),
                timestamp: 0,
            })),
        ];
        let session = create_test_session(messages);
        let exporter = HtmlExporter::new();
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证工具调用折叠结构
        assert!(html.contains("tool-call"));
        assert!(html.contains("read_file"));
        assert!(html.contains("<details"));
    }

    #[test]
    fn test_export_markdown_rendering() {
        let messages = vec![
            AgentMessage::user("# Heading\n\n- Item 1\n- Item 2\n\n**Bold text**"),
        ];
        let session = create_test_session(messages);
        let exporter = HtmlExporter::new();
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证 Markdown 转换为 HTML
        assert!(html.contains("<h1>Heading</h1>") || html.contains("<h1>"));
        assert!(html.contains("<li>"));
        assert!(html.contains("<strong>Bold text</strong>") || html.contains("<strong>"));
    }

    #[test]
    fn test_export_without_stats() {
        let messages = vec![
            AgentMessage::user("Test"),
            AgentMessage::Llm(Message::Assistant(pi_ai::types::AssistantMessage {
                content: vec![pi_ai::types::ContentBlock::Text(pi_ai::types::TextContent::new("Response"))],
                model: "claude-3".to_string(),
                provider: pi_ai::types::Provider::Anthropic,
                stop_reason: pi_ai::types::StopReason::Stop,
                usage: pi_ai::types::Usage { input_tokens: 5, output_tokens: 10, cache_read_tokens: None, cache_write_tokens: None },
                api: pi_ai::types::Api::Anthropic,
                error_message: None,
                response_id: None,
                role: "assistant".to_string(),
                timestamp: 0,
            })),
        ];
        let session = create_test_session(messages);
        let exporter = HtmlExporter::new().with_stats(false);
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证不包含统计信息
        assert!(!html.contains("Session Statistics"));
    }

    #[test]
    fn test_export_with_stats() {
        let messages = vec![
            AgentMessage::user("Test"),
            AgentMessage::Llm(Message::Assistant(pi_ai::types::AssistantMessage {
                content: vec![pi_ai::types::ContentBlock::Text(pi_ai::types::TextContent::new("Response"))],
                model: "claude-3".to_string(),
                provider: pi_ai::types::Provider::Anthropic,
                stop_reason: pi_ai::types::StopReason::Stop,
                usage: pi_ai::types::Usage { input_tokens: 5, output_tokens: 10, cache_read_tokens: None, cache_write_tokens: None },
                api: pi_ai::types::Api::Anthropic,
                error_message: None,
                response_id: None,
                role: "assistant".to_string(),
                timestamp: 0,
            })),
        ];
        let session = create_test_session(messages);
        let exporter = HtmlExporter::new().with_stats(true);
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证包含统计信息
        assert!(html.contains("Session Statistics"));
        assert!(html.contains("User Messages:"));
        assert!(html.contains("Assistant Messages:"));
    }

    #[test]
    fn test_export_thinking_block() {
        let messages = vec![
            AgentMessage::Llm(Message::Assistant(pi_ai::types::AssistantMessage {
                content: vec![
                    pi_ai::types::ContentBlock::Thinking(pi_ai::types::ThinkingContent::new("Let me think about this...")),
                ],
                model: "claude-3".to_string(),
                provider: pi_ai::types::Provider::Anthropic,
                stop_reason: pi_ai::types::StopReason::Stop,
                usage: pi_ai::types::Usage::default(),
                api: pi_ai::types::Api::Anthropic,
                error_message: None,
                response_id: None,
                role: "assistant".to_string(),
                timestamp: 0,
            })),
        ];
        let session = create_test_session(messages);
        let exporter = HtmlExporter::new();
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证 thinking 块渲染
        assert!(html.contains("thinking"));
        assert!(html.contains("Let me think about this..."));
    }

    #[test]
    fn test_export_tool_result() {
        let messages = vec![
            AgentMessage::Llm(Message::ToolResult(pi_ai::types::ToolResultMessage::new(
                "call_123",
                "read_file",
                vec![pi_ai::types::ContentBlock::Text(pi_ai::types::TextContent::new("File contents here"))],
            ).with_details(serde_json::json!({})))),
        ];
        let session = create_test_session(messages);
        let exporter = HtmlExporter::new();
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证工具结果渲染
        assert!(html.contains("tool-result"));
        assert!(html.contains("read_file"));
        assert!(html.contains("File contents here"));
    }

    #[test]
    fn test_export_tool_result_error() {
        let messages = vec![
            AgentMessage::Llm(Message::ToolResult(
                pi_ai::types::ToolResultMessage::new(
                    "call_456",
                    "read_file",
                    vec![pi_ai::types::ContentBlock::Text(pi_ai::types::TextContent::new("File not found"))],
                ).with_error(true).with_details(serde_json::json!({}))
            )),
        ];
        let session = create_test_session(messages);
        let exporter = HtmlExporter::new();
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证错误样式
        assert!(html.contains("error"));
    }

    #[test]
    fn test_export_to_file() {
        let messages = vec![AgentMessage::user("Test message")];
        let session = create_test_session(messages);
        let exporter = HtmlExporter::new();
        
        let temp_dir = tempfile::TempDir::new().unwrap();
        let output_path = temp_dir.path().join("test_export.html");
        
        exporter.export_session(&session, &output_path).unwrap();
        
        // 验证文件已创建
        assert!(output_path.exists());
        
        // 验证文件内容
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("<!DOCTYPE html>"));
        assert!(content.contains("Test message"));
    }

    #[test]
    fn test_export_untitled_session() {
        let session = SavedSession {
            metadata: SessionMetadata {
                id: "untitled".to_string(),
                title: None,
                created_at: 1704067200000,
                updated_at: 1704153600000,
                message_count: 0,
                model: String::new(),
                parent_session_id: None,
                fork_at_index: None,
            },
            messages: vec![],
            compaction_history: vec![],
            stats: None,
        };
        let exporter = HtmlExporter::new();
        
        let html = exporter.render_session_to_html(&session).unwrap();
        
        // 验证使用默认标题
        assert!(html.contains("Untitled Session"));
    }

    #[test]
    fn test_format_timestamp() {
        let timestamp = 1704067200000i64; // 2024-01-01 00:00:00 UTC
        let formatted = format_timestamp(timestamp);
        
        // 验证时间戳格式化
        assert!(formatted.contains("2024-01-01"));
        assert!(formatted.contains("UTC"));
    }
}
