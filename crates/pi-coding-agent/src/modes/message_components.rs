//! 消息组件体系
//!
//! 提供聊天消息显示的组件化体系，包括用户消息、助手消息、工具调用、状态栏等组件。
//! 所有组件都实现 pi_tui::tui::Component trait。

use pi_tui::components::markdown::Markdown;
use pi_tui::tui::Component;
use pi_tui::utils::{wrap_text_with_ansi, visible_width};

/// 用户消息组件
pub struct UserMessageComponent {
    content: String,
    edited: bool,
    needs_render: bool,
}

impl UserMessageComponent {
    /// 创建新的用户消息组件
    pub fn new(content: String) -> Self {
        Self {
            content,
            edited: false,
            needs_render: true,
        }
    }

    /// 设置编辑标记
    pub fn set_edited(&mut self, edited: bool) {
        self.edited = edited;
        self.needs_render = true;
    }
}

impl Component for UserMessageComponent {
    fn render(&self, width: u16) -> Vec<String> {
        let mut lines = Vec::new();

        // 标题行：粗体蓝色 "👤 You"
        let title = if self.edited {
            "\x1b[1;34m👤 You\x1b[0m (edited)".to_string()
        } else {
            "\x1b[1;34m👤 You\x1b[0m".to_string()
        };
        lines.push(title);

        // 消息内容（自动换行）
        if !self.content.is_empty() {
            let wrapped = wrap_text_with_ansi(&self.content, width as usize);
            lines.extend(wrapped);
        }

        lines
    }

    fn invalidate(&mut self) {
        self.needs_render = true;
    }
}

/// 工具调用显示组件
pub struct ToolCallDisplayComponent {
    tool_name: String,
    call_id: String,
    collapsed: bool,
    is_error: bool,
    is_running: bool,
    duration_ms: Option<u64>,
    needs_render: bool,
}

impl ToolCallDisplayComponent {
    /// 创建新的工具调用显示组件
    pub fn new(tool_name: String, call_id: String) -> Self {
        Self {
            tool_name,
            call_id,
            collapsed: false,
            is_error: false,
            is_running: true,
            duration_ms: None,
            needs_render: true,
        }
    }

    /// 设置完成状态
    pub fn set_completed(&mut self, is_error: bool, duration_ms: Option<u64>) {
        self.is_error = is_error;
        self.duration_ms = duration_ms;
        self.is_running = false;
        self.needs_render = true;
    }

    /// 切换折叠状态
    pub fn toggle_collapse(&mut self) {
        self.collapsed = !self.collapsed;
        self.needs_render = true;
    }

    /// 获取调用 ID
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// 获取工具名称
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// 检查是否仍在运行
    pub fn is_running(&self) -> bool {
        self.is_running
    }
}

impl Component for ToolCallDisplayComponent {
    fn render(&self, _width: u16) -> Vec<String> {
        let mut lines = Vec::new();

        let line = if self.is_running {
            // 运行中: 黄色
            format!("\x1b[33m⏳ {}...\x1b[0m", self.tool_name)
        } else if self.is_error {
            // 失败: 红色
            let duration_str = self
                .duration_ms
                .map(|d| format!(" ({}ms)", d))
                .unwrap_or_default();
            format!("\x1b[31m❌ {}{}\x1b[0m", self.tool_name, duration_str)
        } else {
            // 成功: 绿色
            let duration_str = self
                .duration_ms
                .map(|d| format!(" ({}ms)", d))
                .unwrap_or_default();
            format!("\x1b[32m✅ {}{}\x1b[0m", self.tool_name, duration_str)
        };

        lines.push(line);
        lines
    }

    fn invalidate(&mut self) {
        self.needs_render = true;
    }
}

/// 助手消息组件
pub struct AssistantMessageComponent {
    markdown: Markdown,
    thinking_text: String,
    thinking_collapsed: bool,
    tool_calls: Vec<ToolCallDisplayComponent>,
    is_streaming: bool,
    needs_render: bool,
}

impl AssistantMessageComponent {
    /// 创建新的助手消息组件
    pub fn new() -> Self {
        Self {
            markdown: Markdown::new(),
            thinking_text: String::new(),
            thinking_collapsed: true,
            tool_calls: Vec::new(),
            is_streaming: false,
            needs_render: true,
        }
    }

    /// 追加文本内容
    pub fn push_text(&mut self, delta: &str) {
        self.markdown.append_content(delta);
        self.needs_render = true;
    }

    /// 追加思考内容
    pub fn push_thinking(&mut self, delta: &str) {
        self.thinking_text.push_str(delta);
        self.needs_render = true;
    }

    /// 添加工具调用
    pub fn add_tool_call(&mut self, name: String, call_id: String) {
        self.tool_calls
            .push(ToolCallDisplayComponent::new(name, call_id));
        self.needs_render = true;
    }

    /// 更新工具调用状态（通过 call_id）
    pub fn update_tool_call(&mut self, call_id: &str, is_error: bool, duration_ms: Option<u64>) {
        for tool_call in &mut self.tool_calls {
            if tool_call.call_id() == call_id {
                tool_call.set_completed(is_error, duration_ms);
                break;
            }
        }
        self.needs_render = true;
    }

    /// 更新最后一个工具调用状态（通过 tool_name 匹配）
    pub fn update_last_tool_call(&mut self, tool_name: &str, is_error: bool, duration_ms: Option<u64>) {
        // 从后向前查找匹配 tool_name 且仍在运行中的工具调用
        for tool_call in self.tool_calls.iter_mut().rev() {
            if tool_call.tool_name() == tool_name && tool_call.is_running() {
                tool_call.set_completed(is_error, duration_ms);
                self.needs_render = true;
                break;
            }
        }
    }

    /// 设置流式状态
    pub fn set_streaming(&mut self, streaming: bool) {
        self.is_streaming = streaming;
        self.needs_render = true;
    }

    /// 切换思考内容折叠
    pub fn toggle_thinking(&mut self) {
        self.thinking_collapsed = !self.thinking_collapsed;
        self.needs_render = true;
    }
}

impl Default for AssistantMessageComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for AssistantMessageComponent {
    fn render(&self, width: u16) -> Vec<String> {
        let mut lines = Vec::new();

        // 1. 标题行：粗体绿色 "🤖 Assistant"
        lines.push("\x1b[1;32m🤖 Assistant\x1b[0m".to_string());

        // 2. 思考内容
        if !self.thinking_text.is_empty() {
            if self.thinking_collapsed {
                // 折叠时显示提示
                lines.push("\x1b[2m💭 Thinking... (click to expand)\x1b[0m".to_string());
            } else {
                // 展开时显示完整内容，每行前加 "│ "，dim 样式
                for line in self.thinking_text.lines() {
                    lines.push(format!("\x1b[2m│ {}\x1b[0m", line));
                }
            }
        }

        // 3. Markdown 渲染文本内容
        let md_lines = self.markdown.render(width);
        lines.extend(md_lines);

        // 4. 渲染所有工具调用组件
        for tool_call in &self.tool_calls {
            let tool_lines = tool_call.render(width);
            lines.extend(tool_lines);
        }

        // 5. 流式加载指示器
        if self.is_streaming {
            lines.push("▍".to_string());
        }

        lines
    }

    fn invalidate(&mut self) {
        self.needs_render = true;
        self.markdown.invalidate();
        for tool_call in &mut self.tool_calls {
            tool_call.invalidate();
        }
    }
}

/// 状态栏组件
pub struct StatusBarComponent {
    model_name: String,
    token_count: usize,
    context_window: usize,
    session_name: String,
    is_loading: bool,
    loading_frame: usize,
    needs_render: bool,
}

/// 加载动画帧
const LOADING_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

impl StatusBarComponent {
    /// 创建新的状态栏组件
    pub fn new() -> Self {
        Self {
            model_name: String::new(),
            token_count: 0,
            context_window: 0,
            session_name: String::new(),
            is_loading: false,
            loading_frame: 0,
            needs_render: true,
        }
    }

    /// 设置模型名
    pub fn set_model(&mut self, name: String) {
        self.model_name = name;
        self.needs_render = true;
    }

    /// 更新 Token 统计
    pub fn set_tokens(&mut self, count: usize, window: usize) {
        self.token_count = count;
        self.context_window = window;
        self.needs_render = true;
    }

    /// 设置会话名
    pub fn set_session_name(&mut self, name: String) {
        self.session_name = name;
        self.needs_render = true;
    }

    /// 设置加载状态
    pub fn set_loading(&mut self, loading: bool) {
        self.is_loading = loading;
        if !loading {
            self.loading_frame = 0;
        }
        self.needs_render = true;
    }

    /// 更新加载动画帧
    pub fn tick(&mut self) {
        if self.is_loading {
            self.loading_frame = (self.loading_frame + 1) % LOADING_FRAMES.len();
            self.needs_render = true;
        }
    }
}

impl Default for StatusBarComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for StatusBarComponent {
    fn render(&self, width: u16) -> Vec<String> {
        // 背景色：深灰色 \x1b[48;5;236m，白色前景 \x1b[37m
        let bg = "\x1b[48;5;236m";
        let fg = "\x1b[37m";
        let reset = "\x1b[0m";

        // 左侧：模型名 + 加载动画
        let loading_indicator = if self.is_loading {
            format!(
                " {} ",
                LOADING_FRAMES[self.loading_frame % LOADING_FRAMES.len()]
            )
        } else {
            String::new()
        };
        let left = format!("{}{}", self.model_name, loading_indicator);

        // 中间：Token 使用量
        let percent = if self.context_window > 0 {
            (self.token_count * 100) / self.context_window
        } else {
            0
        };
        let center = format!("tokens: {}/{} ({}%)", self.token_count, self.context_window, percent);

        // 右侧：会话名
        let right = self.session_name.clone();

        // 计算各部分宽度
        let left_width = visible_width_for_status(&left);
        let center_width = visible_width_for_status(&center);
        let right_width = visible_width_for_status(&right);

        // 计算需要的空格填充
        let total_content_width = left_width + center_width + right_width;
        let available_space = width as usize;

        let mut line = String::new();
        line.push_str(bg);
        line.push_str(fg);

        if total_content_width >= available_space {
            // 内容太长，直接拼接
            line.push_str(&left);
            line.push_str(&center);
            line.push_str(&right);
        } else {
            // 计算左右两侧的空格
            let remaining = available_space - total_content_width;
            let left_padding = remaining / 2;
            let right_padding = remaining - left_padding;

            line.push_str(&left);
            line.push_str(&" ".repeat(left_padding));
            line.push_str(&center);
            line.push_str(&" ".repeat(right_padding));
            line.push_str(&right);
        }

        line.push_str(reset);

        vec![line]
    }

    fn invalidate(&mut self) {
        self.needs_render = true;
    }
}

/// 计算字符串的可见宽度（用于状态栏，不包含 ANSI 转义序列）
fn visible_width_for_status(s: &str) -> usize {
    visible_width(s)
}

/// 分隔线组件
pub struct SeparatorComponent {
    needs_render: bool,
}

impl SeparatorComponent {
    /// 创建新的分隔线组件
    pub fn new() -> Self {
        Self {
            needs_render: true,
        }
    }
}

impl Default for SeparatorComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for SeparatorComponent {
    fn render(&self, width: u16) -> Vec<String> {
        // dim 样式 \x1b[2m，使用 "─" 字符
        let line = format!("\x1b[2m{}\x1b[0m", "─".repeat(width as usize));
        vec![line]
    }

    fn invalidate(&mut self) {
        self.needs_render = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_message_component() {
        let mut component = UserMessageComponent::new("Hello world".to_string());
        let lines = component.render(80);
        assert!(!lines.is_empty());
        assert!(lines[0].contains("👤 You"));
        assert!(lines[0].contains("\x1b[1;34m")); // 粗体蓝色

        component.set_edited(true);
        let lines = component.render(80);
        assert!(lines[0].contains("(edited)"));
    }

    #[test]
    fn test_tool_call_display_component() {
        let mut component = ToolCallDisplayComponent::new("read_file".to_string(), "call_1".to_string());
        
        // 运行中状态
        let lines = component.render(80);
        assert!(lines[0].contains("⏳"));
        assert!(lines[0].contains("read_file"));
        assert!(lines[0].contains("\x1b[33m")); // 黄色

        // 成功状态
        component.set_completed(false, Some(150));
        let lines = component.render(80);
        assert!(lines[0].contains("✅"));
        assert!(lines[0].contains("(150ms)"));
        assert!(lines[0].contains("\x1b[32m")); // 绿色

        // 失败状态
        component.set_completed(true, Some(200));
        let lines = component.render(80);
        assert!(lines[0].contains("❌"));
        assert!(lines[0].contains("\x1b[31m")); // 红色
    }

    #[test]
    fn test_assistant_message_component() {
        let mut component = AssistantMessageComponent::new();
        
        // 基本渲染
        let lines = component.render(80);
        assert!(!lines.is_empty());
        assert!(lines[0].contains("🤖 Assistant"));
        assert!(lines[0].contains("\x1b[1;32m")); // 粗体绿色

        // 添加思考内容
        component.push_thinking("Let me think...");
        let lines = component.render(80);
        // 默认折叠
        assert!(lines.iter().any(|l| l.contains("Thinking... (click to expand)")));

        // 展开思考内容
        component.toggle_thinking();
        let lines = component.render(80);
        assert!(lines.iter().any(|l| l.contains("│ Let me think...")));
        assert!(lines.iter().any(|l| l.contains("\x1b[2m"))); // dim 样式

        // 添加文本
        component.push_text("Hello **world**");
        let lines = component.render(80);
        assert!(lines.iter().any(|l| l.contains("world")));

        // 添加工具调用
        component.add_tool_call("grep".to_string(), "call_1".to_string());
        let lines = component.render(80);
        assert!(lines.iter().any(|l| l.contains("⏳")));

        // 更新工具调用
        component.update_tool_call("call_1", false, Some(100));
        let lines = component.render(80);
        assert!(lines.iter().any(|l| l.contains("✅")));

        // 流式状态
        component.set_streaming(true);
        let lines = component.render(80);
        assert!(lines.last().unwrap().contains("▍"));
    }

    #[test]
    fn test_status_bar_component() {
        let mut component = StatusBarComponent::new();
        component.set_model("claude-3".to_string());
        component.set_tokens(1000, 4000);
        component.set_session_name("test-session".to_string());

        let lines = component.render(80);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("claude-3"));
        assert!(lines[0].contains("tokens: 1000/4000 (25%)"));
        assert!(lines[0].contains("test-session"));
        assert!(lines[0].contains("\x1b[48;5;236m")); // 深灰色背景
        assert!(lines[0].contains("\x1b[37m")); // 白色前景

        // 加载动画
        component.set_loading(true);
        component.tick();
        let lines = component.render(80);
        // 应该包含加载动画帧中的一个字符
        let has_loading_frame = LOADING_FRAMES.iter().any(|&f| lines[0].contains(f));
        assert!(has_loading_frame);
    }

    #[test]
    fn test_separator_component() {
        let component = SeparatorComponent::new();
        let lines = component.render(10);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("──────────"));
        assert!(lines[0].contains("\x1b[2m")); // dim 样式
    }
}
