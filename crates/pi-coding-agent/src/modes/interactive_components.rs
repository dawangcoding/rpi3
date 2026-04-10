//! 交互模式 TUI 组件
//!
//! 提供 Markdown 渲染和流式内容差分更新能力，
//! 作为交互模式 (interactive.rs) 的渲染辅助模块。

use std::io::Write;
use pi_tui::components::markdown::Markdown;
use pi_tui::components::editor::Editor;
use pi_tui::tui::Component;
use super::message_components::StatusBarComponent;

/// 使用 Markdown 组件将内容渲染为 ANSI 格式的终端行
pub fn render_markdown_lines(content: &str, width: u16) -> Vec<String> {
    if content.trim().is_empty() {
        return Vec::new();
    }
    let mut md = Markdown::new();
    md.set_content(content);
    md.render(width)
}

/// 渲染编辑器区域（带 > 提示符）
/// 
/// 用于在流式结束后或需要重新渲染输入区域时调用
pub fn render_editor_area(
    editor: &Editor,
    width: u16,
    stdout: &mut impl Write,
) -> std::io::Result<()> {
    let lines = editor.render(width.saturating_sub(2));
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            write!(stdout, "\x1b[36m> \x1b[0m{}", line)?;
        } else {
            write!(stdout, "\r\n  {}", line)?;
        }
    }
    stdout.flush()
}

/// 渲染状态栏 + 编辑器区域（在流式结束后调用）
pub fn render_status_and_editor(
    status_bar: &StatusBarComponent,
    editor: &Editor,
    width: u16,
    stdout: &mut impl Write,
) -> std::io::Result<()> {
    // 状态栏
    let bar_lines = status_bar.render(width);
    for line in &bar_lines {
        write!(stdout, "{}\r\n", line)?;
    }
    write!(stdout, "\r\n")?;
    // 编辑器
    render_editor_area(editor, width, stdout)
}

/// 渲染思考内容为 dim 样式的终端行
pub fn render_thinking_lines(content: &str) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    content
        .lines()
        .map(|line| format!("\x1b[2m{}\x1b[0m", line))
        .collect()
}

/// 流式内容差分渲染块
///
/// 管理一块终端区域的就地更新。累积流式内容后，
/// 通过光标回退 + 行清除实现差分刷新，避免全屏重绘。
///
/// 渲染流程：
/// 1. 回退光标到流式区域起始行
/// 2. 清除旧行
/// 3. 使用 Markdown 组件重新渲染累积内容
pub struct StreamingBlock {
    /// 上一次渲染占用的终端行数
    prev_line_count: usize,
    /// 累积的文本内容
    text: String,
    /// 累积的思考内容
    thinking: String,
}

impl Default for StreamingBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingBlock {
    pub fn new() -> Self {
        Self {
            prev_line_count: 0,
            text: String::new(),
            thinking: String::new(),
        }
    }

    /// 追加文本 delta
    pub fn push_text(&mut self, delta: &str) {
        self.text.push_str(delta);
    }

    /// 追加思考 delta
    pub fn push_thinking(&mut self, delta: &str) {
        self.thinking.push_str(delta);
    }

    /// 生成差分更新 ANSI 序列
    ///
    /// 先回退到流式区域起始行，清除旧行，再用 Markdown 渲染新内容。
    /// 返回值可直接 write 到 stdout。
    pub fn diff_update(&mut self, width: u16) -> String {
        let mut buf = String::new();

        // 1. 回退光标到流式区域起始行
        //    prev_line_count 行的内容占 prev_line_count-1 个行间距
        let go_back = self.prev_line_count.saturating_sub(1);
        if go_back > 0 {
            buf.push_str(&format!("\x1b[{}A", go_back));
        }
        buf.push('\r');

        // 2. 渲染新内容
        let mut lines = Vec::new();
        if !self.thinking.is_empty() {
            lines.extend(render_thinking_lines(&self.thinking));
        }
        if !self.text.is_empty() {
            lines.extend(render_markdown_lines(&self.text, width));
        }
        // 至少保留一行（避免光标位置问题）
        if lines.is_empty() {
            lines.push(String::new());
        }

        // 3. 输出新行（每行先清除旧内容）
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                buf.push_str("\r\n");
            }
            buf.push_str("\x1b[2K");
            buf.push_str(line);
        }

        // 4. 清除新内容行数不足时的多余旧行
        if lines.len() < self.prev_line_count {
            let extra = self.prev_line_count - lines.len();
            for _ in 0..extra {
                buf.push_str("\r\n\x1b[2K");
            }
            if extra > 0 {
                buf.push_str(&format!("\x1b[{}A", extra));
            }
        }

        self.prev_line_count = lines.len();
        buf
    }

    /// 完成流式渲染，重置状态
    ///
    /// 返回 (text, thinking) 的最终累积内容
    pub fn finish(&mut self) -> (String, String) {
        self.prev_line_count = 0;
        (
            std::mem::take(&mut self.text),
            std::mem::take(&mut self.thinking),
        )
    }

    /// 检查是否有累积内容
    pub fn has_content(&self) -> bool {
        !self.text.is_empty() || !self.thinking.is_empty()
    }

    /// 增量追加模式 - 只渲染新增内容
    /// 当只有文本追加（非删除/替换）时使用，避免全量重绘
    pub fn append_update(&mut self, new_text: &str, width: u16) -> String {
        // 保存之前的长度用于比较
        let _prev_text_len = self.text.len();
        self.text.push_str(new_text);
        
        // 检查是否只是简单追加（无需重绘）
        let new_lines = render_markdown_lines(&self.text, width);
        let old_line_count = self.prev_line_count;
        
        if new_lines.len() == old_line_count && old_line_count > 0 {
            // 行数未变，只需更新最后一行
            // 计算新增内容在最后一行的位置
            let last_line = &new_lines[new_lines.len() - 1];
            
            // 使用 \r 回到行首，清除行，然后重新输出最后一行
            let output = format!("\r\x1b[2K{}", last_line);
            return output;
        }
        
        // 行数变化，回退到全量差分更新
        self.diff_update(width)
    }
    
    /// 智能更新 - 根据内容变化选择最优更新策略
    pub fn smart_update(&mut self, delta: &str, width: u16, is_thinking: bool) -> String {
        if is_thinking {
            self.push_thinking(delta);
        } else {
            // 检查是否是纯追加（delta 不包含特殊字符且当前文本不以换行结尾）
            let is_simple_append = !delta.contains('\n') && 
                                   !delta.contains('\r') && 
                                   !self.text.ends_with('\n') &&
                                   self.prev_line_count > 0;
            
            if is_simple_append {
                return self.append_update(delta, width);
            } else {
                self.push_text(delta);
            }
        }
        
        self.diff_update(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown_lines_empty() {
        let lines = render_markdown_lines("", 80);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_render_markdown_lines_basic() {
        let lines = render_markdown_lines("Hello **world**", 80);
        assert!(!lines.is_empty());
        // 应该包含 bold ANSI 序列
        assert!(lines[0].contains("world"));
    }

    #[test]
    fn test_render_thinking_lines() {
        let lines = render_thinking_lines("thinking...\nstep 2");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\x1b[2m")); // dim
        assert!(lines[1].contains("step 2"));
    }

    #[test]
    fn test_streaming_block_empty() {
        let block = StreamingBlock::new();
        assert!(!block.has_content());
    }

    #[test]
    fn test_streaming_block_push() {
        let mut block = StreamingBlock::new();
        block.push_text("hello");
        block.push_thinking("hmm");
        assert!(block.has_content());
    }

    #[test]
    fn test_streaming_block_diff_first_render() {
        let mut block = StreamingBlock::new();
        block.push_text("Hello");
        let output = block.diff_update(80);
        // 第一次渲染不应有光标回退（\x1b[NA 形式）
        assert!(!output.contains("\x1b[1A"));
        assert!(!output.contains("\x1b[2A"));
        // 包含行清除 + 内容
        assert!(output.contains("\x1b[2K"));
        assert!(output.contains("Hello"));
    }

    #[test]
    fn test_streaming_block_diff_update() {
        let mut block = StreamingBlock::new();
        block.push_text("Hello");
        let _ = block.diff_update(80);
        // 追加更多内容
        block.push_text(" World");
        let output = block.diff_update(80);
        // 单行内容，回退 0 行（prev=1, go_back=0），只有 \r
        assert!(output.starts_with('\r'));
        assert!(output.contains("Hello World"));
    }

    #[test]
    fn test_streaming_block_diff_multiline() {
        let mut block = StreamingBlock::new();
        block.push_text("Line 1\n\nLine 2");
        let _ = block.diff_update(80);
        // 此时 prev_line_count > 1
        block.push_text("\n\nLine 3");
        let output = block.diff_update(80);
        // 应包含光标回退
        assert!(output.contains("\x1b["));
    }

    #[test]
    fn test_streaming_block_finish() {
        let mut block = StreamingBlock::new();
        block.push_text("text");
        block.push_thinking("think");
        let _ = block.diff_update(80);
        let (text, thinking) = block.finish();
        assert_eq!(text, "text");
        assert_eq!(thinking, "think");
        assert!(!block.has_content());
    }
}
