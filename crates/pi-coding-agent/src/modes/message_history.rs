//! 消息历史容器
//!
//! 管理聊天消息历史，包含用户消息、助手消息、系统消息等。
//! 实现 Component trait，可与 TUI 框架集成。

use pi_tui::tui::Component;
use super::message_components::*;

/// 消息条目枚举
pub enum MessageEntry {
    User(UserMessageComponent),
    Assistant(AssistantMessageComponent),
    System(String),
    Separator(SeparatorComponent),
}

/// 消息历史容器 - 管理所有消息并实现 Component trait
pub struct MessageHistory {
    messages: Vec<MessageEntry>,
    scroll_offset: usize,       // 渲染起始行偏移
    auto_scroll: bool,          // 是否自动滚动到底部
    needs_render: bool,
    total_rendered_lines: usize, // 缓存的总渲染行数
}

impl MessageHistory {
    /// 创建新的消息历史容器
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            auto_scroll: true,
            needs_render: true,
            total_rendered_lines: 0,
        }
    }

    /// 添加用户消息
    pub fn add_user_message(&mut self, text: String) {
        self.messages.push(MessageEntry::User(UserMessageComponent::new(text)));
        self.messages.push(MessageEntry::Separator(SeparatorComponent::new()));
        self.needs_render = true;
    }

    /// 开始助手消息，返回可变引用用于流式更新
    pub fn start_assistant_message(&mut self) -> &mut AssistantMessageComponent {
        self.messages.push(MessageEntry::Assistant(AssistantMessageComponent::new()));
        self.needs_render = true;
        
        // 返回刚添加的 assistant message 的可变引用
        match self.messages.last_mut() {
            Some(MessageEntry::Assistant(ref mut comp)) => comp,
            _ => unreachable!("Just pushed an Assistant message"),
        }
    }

    /// 获取最后一个 Assistant 消息的可变引用（用于流式更新）
    pub fn current_streaming(&mut self) -> Option<&mut AssistantMessageComponent> {
        self.messages.iter_mut().rev().find_map(|entry| {
            if let MessageEntry::Assistant(ref mut comp) = entry {
                Some(comp)
            } else {
                None
            }
        })
    }

    /// 完成助手消息，添加分隔线
    pub fn finish_assistant_message(&mut self) {
        if let Some(assistant) = self.current_streaming() {
            assistant.set_streaming(false);
        }
        self.messages.push(MessageEntry::Separator(SeparatorComponent::new()));
        self.needs_render = true;
    }

    /// 添加系统消息
    pub fn add_system_message(&mut self, text: String) {
        self.messages.push(MessageEntry::System(text));
        self.needs_render = true;
    }

    /// 向上滚动
    pub fn scroll_up(&mut self, lines: usize) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
        self.needs_render = true;
    }

    /// 向下滚动
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
        self.needs_render = true;
        // 如果滚动到底部附近，重新启用 auto_scroll
        // 注意：实际渲染行数在 render 时计算，这里仅标记
    }

    /// 滚动到底部
    pub fn scroll_to_bottom(&mut self) {
        self.auto_scroll = true;
        self.scroll_offset = usize::MAX; // 渲染时会被裁剪
        self.needs_render = true;
    }

    /// 清空所有消息
    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
        self.needs_render = true;
        self.total_rendered_lines = 0;
    }

    /// 获取消息数量
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// 获取是否自动滚动
    pub fn auto_scroll(&self) -> bool {
        self.auto_scroll
    }

    /// 获取滚动偏移
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }
}

impl Default for MessageHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for MessageHistory {
    fn render(&self, width: u16) -> Vec<String> {
        // 1. 收集所有消息的渲染行
        let mut all_lines: Vec<String> = Vec::new();
        for entry in &self.messages {
            match entry {
                MessageEntry::User(comp) => all_lines.extend(comp.render(width)),
                MessageEntry::Assistant(comp) => all_lines.extend(comp.render(width)),
                MessageEntry::System(text) => {
                    // 系统消息: dim + italic
                    let styled = format!("\x1b[2;3m{}\x1b[0m", text);
                    all_lines.push(styled);
                }
                MessageEntry::Separator(comp) => all_lines.extend(comp.render(width)),
            }
        }

        // 2. 如果 auto_scroll，scroll_offset 指向底部
        // （让外层 Tui 的 VirtualViewport 处理实际的视口裁剪）
        // 这里返回所有行，由 Tui 框架负责虚拟滚动

        all_lines
    }

    fn invalidate(&mut self) {
        self.needs_render = true;
        // 也 invalidate 所有子组件
        for entry in &mut self.messages {
            match entry {
                MessageEntry::User(comp) => comp.invalidate(),
                MessageEntry::Assistant(comp) => comp.invalidate(),
                MessageEntry::Separator(comp) => comp.invalidate(),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_history_new() {
        let history = MessageHistory::new();
        assert_eq!(history.message_count(), 0);
        assert!(history.auto_scroll());
        assert_eq!(history.scroll_offset(), 0);
    }

    #[test]
    fn test_add_user_message() {
        let mut history = MessageHistory::new();
        history.add_user_message("Hello".to_string());
        
        // User message + Separator = 2 entries
        assert_eq!(history.message_count(), 2);
        
        let lines = history.render(80);
        assert!(!lines.is_empty());
        assert!(lines.iter().any(|l| l.contains("👤 You")));
    }

    #[test]
    fn test_start_assistant_message() {
        let mut history = MessageHistory::new();
        let assistant = history.start_assistant_message();
        assistant.push_text("Hello world");
        
        assert_eq!(history.message_count(), 1);
        
        let lines = history.render(80);
        assert!(lines.iter().any(|l| l.contains("🤖 Assistant")));
        assert!(lines.iter().any(|l| l.contains("Hello world")));
    }

    #[test]
    fn test_current_streaming() {
        let mut history = MessageHistory::new();
        history.start_assistant_message();
        
        // Update via current_streaming
        if let Some(assistant) = history.current_streaming() {
            assistant.push_text("Streaming text");
        }
        
        let lines = history.render(80);
        assert!(lines.iter().any(|l| l.contains("Streaming text")));
    }

    #[test]
    fn test_finish_assistant_message() {
        let mut history = MessageHistory::new();
        history.start_assistant_message();
        history.finish_assistant_message();
        
        // Assistant + Separator = 2 entries
        assert_eq!(history.message_count(), 2);
    }

    #[test]
    fn test_add_system_message() {
        let mut history = MessageHistory::new();
        history.add_system_message("System notification".to_string());
        
        assert_eq!(history.message_count(), 1);
        
        let lines = history.render(80);
        assert!(lines.iter().any(|l| l.contains("System notification")));
        // Check for dim + italic style
        assert!(lines.iter().any(|l| l.contains("\x1b[2;3m")));
    }

    #[test]
    fn test_scroll_operations() {
        let mut history = MessageHistory::new();
        
        // Initially auto_scroll is true
        assert!(history.auto_scroll());
        
        // Scroll up disables auto_scroll
        history.scroll_up(5);
        assert!(!history.auto_scroll());
        
        // Scroll to bottom re-enables auto_scroll
        history.scroll_to_bottom();
        assert!(history.auto_scroll());
    }

    #[test]
    fn test_clear() {
        let mut history = MessageHistory::new();
        history.add_user_message("Hello".to_string());
        history.start_assistant_message();
        
        assert_eq!(history.message_count(), 3); // User + Separator + Assistant
        
        history.clear();
        assert_eq!(history.message_count(), 0);
        assert_eq!(history.scroll_offset(), 0);
    }

    #[test]
    fn test_full_conversation_flow() {
        let mut history = MessageHistory::new();
        
        // User message
        history.add_user_message("What is Rust?".to_string());
        
        // Assistant response
        let assistant = history.start_assistant_message();
        assistant.push_text("Rust is a systems programming language.");
        assistant.push_thinking("Let me explain...");
        history.finish_assistant_message();
        
        // System message
        history.add_system_message("Context updated".to_string());
        
        // Another user message
        history.add_user_message("Thanks!".to_string());
        
        // Check total entries: User + Sep + Assistant + Sep + System + User + Sep = 7
        assert_eq!(history.message_count(), 7);
        
        let lines = history.render(80);
        assert!(lines.iter().any(|l| l.contains("👤 You")));
        assert!(lines.iter().any(|l| l.contains("🤖 Assistant")));
        assert!(lines.iter().any(|l| l.contains("Context updated")));
    }
}
