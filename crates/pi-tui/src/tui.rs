//! TUI 主模块
//! 提供差分渲染引擎、组件系统和覆盖层管理

use crate::terminal::Terminal;
use crate::utils::{visible_width, slice_by_column, CURSOR_MARKER};
use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// 组件 trait
/// 
/// 所有 UI 组件的基础接口，定义渲染和输入处理能力
pub trait Component: Send {
    /// 渲染组件，返回每行的 ANSI 字符串
    fn render(&self, width: u16) -> Vec<String>;
    
    /// 处理键盘输入
    fn handle_input(&mut self, _data: &str) -> bool {
        false
    }
    
    /// 是否需要键释放事件
    fn wants_key_release(&self) -> bool {
        false
    }
    
    /// 标记需要重新渲染
    fn invalidate(&mut self);
}

/// 可聚焦 trait
/// 
/// 定义可接收焦点的组件行为
pub trait Focusable {
    /// 检查是否已聚焦
    fn focused(&self) -> bool;
    /// 设置聚焦状态
    fn set_focused(&mut self, focused: bool);
}

/// 覆盖层定位锚点
/// 
/// 定义覆盖层在屏幕上的定位方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum OverlayAnchor {
    /// 居中
    #[default]
    Center,
    /// 左上角
    TopLeft,
    /// 右上角
    TopRight,
    /// 左下角
    BottomLeft,
    /// 右下角
    BottomRight,
    /// 顶部居中
    TopCenter,
    /// 底部居中
    BottomCenter,
    /// 左侧居中
    LeftCenter,
    /// 右侧居中
    RightCenter,
}


/// 尺寸值
/// 
/// 支持绝对像素或百分比尺寸
#[derive(Debug, Clone, Copy)]
pub enum SizeValue {
    /// 绝对像素值
    Absolute(u16),
    /// 百分比值
    Percent(f32),
}

impl SizeValue {
    /// 解析为绝对值
    #[allow(clippy::wrong_self_convention)]
    fn to_absolute(&self, reference: u16) -> u16 {
        match self {
            SizeValue::Absolute(v) => *v,
            SizeValue::Percent(p) => ((reference as f32 * p) / 100.0) as u16,
        }
    }
}

impl From<u16> for SizeValue {
    fn from(v: u16) -> Self {
        SizeValue::Absolute(v)
    }
}

/// 覆盖层边距
/// 
/// 定义覆盖层的内边距
#[derive(Debug, Clone, Copy)]
#[derive(Default)]
pub struct OverlayMargin {
    /// 上边距
    pub top: u16,
    /// 右边距
    pub right: u16,
    /// 下边距
    pub bottom: u16,
    /// 左边距
    pub left: u16,
}

impl OverlayMargin {
    /// 创建统一的边距
    pub fn uniform(margin: u16) -> Self {
        Self {
            top: margin,
            right: margin,
            bottom: margin,
            left: margin,
        }
    }
}


/// 覆盖层选项
/// 
/// 配置覆盖层的尺寸、位置和显示行为
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct OverlayOptions {
    /// 宽度（绝对或百分比）
    pub width: Option<SizeValue>,
    /// 最小宽度
    pub min_width: Option<u16>,
    /// 最大高度（绝对或百分比）
    pub max_height: Option<SizeValue>,
    /// 定位锚点
    pub anchor: Option<OverlayAnchor>,
    /// 水平偏移
    pub offset_x: Option<i16>,
    /// 垂直偏移
    pub offset_y: Option<i16>,
    /// 行位置（绝对或百分比）
    pub row: Option<SizeValue>,
    /// 列位置（绝对或百分比）
    pub col: Option<SizeValue>,
    /// 边距
    pub margin: Option<OverlayMargin>,
    /// 是否不捕获焦点
    pub non_capturing: bool,
}


/// 内部覆盖层条目
struct OverlayEntry {
    component: Box<dyn Component>,
    options: OverlayOptions,
    pre_focus: Option<usize>, // 存储聚焦组件的索引
    hidden: AtomicBool,
    focus_order: AtomicUsize,
}

/// 覆盖层句柄
/// 
/// 用于控制和管理覆盖层的生命周期
#[derive(Clone)]
pub struct OverlayHandle {
    index: usize,
    hidden: Arc<AtomicBool>,
    focus_order: Arc<AtomicUsize>,
}

impl OverlayHandle {
    /// 隐藏覆盖层（从堆栈中移除）
    pub fn hide(&self, tui: &mut Tui) {
        tui.remove_overlay(self.index);
    }
    
    /// 设置隐藏状态
    pub fn set_hidden(&self, hidden: bool) {
        self.hidden.store(hidden, Ordering::SeqCst);
    }
    
    /// 检查是否隐藏
    pub fn is_hidden(&self) -> bool {
        self.hidden.load(Ordering::SeqCst)
    }
    
    /// 聚焦此覆盖层
    pub fn focus(&self, tui: &mut Tui) {
        if !self.is_hidden() {
            self.focus_order.fetch_add(1, Ordering::SeqCst);
            tui.focus_overlay(self.index);
        }
    }
    
    /// 取消聚焦
    pub fn unfocus(&self, tui: &mut Tui) {
        tui.unfocus_overlay(self.index);
    }
    
    /// 检查是否聚焦
    pub fn is_focused(&self, tui: &Tui) -> bool {
        tui.is_overlay_focused(self.index)
    }
}

/// 容器组件
/// 
/// 包含多个子组件的复合组件
pub struct Container {
    children: Vec<Box<dyn Component>>,
}

impl Container {
    /// 创建新的容器
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }
    
    /// 添加子组件
    pub fn add_child(&mut self, component: Box<dyn Component>) {
        self.children.push(component);
    }
    
    /// 移除子组件
    pub fn remove_child(&mut self, index: usize) -> Option<Box<dyn Component>> {
        if index < self.children.len() {
            Some(self.children.remove(index))
        } else {
            None
        }
    }
    
    /// 清空所有子组件
    pub fn clear(&mut self) {
        self.children.clear();
    }
    
    /// 获取子组件数量
    pub fn len(&self) -> usize {
        self.children.len()
    }
    
    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Container {
    fn render(&self, width: u16) -> Vec<String> {
        let mut lines = Vec::new();
        for child in &self.children {
            let child_lines = child.render(width);
            lines.extend(child_lines);
        }
        lines
    }
    
    fn invalidate(&mut self) {
        for child in &mut self.children {
            child.invalidate();
        }
    }
}

/// 虚拟视口
/// 
/// 优化大内容的渲染性能，只渲染可见区域
pub struct VirtualViewport {
    /// 所有内容行
    total_lines: usize,
    /// 视口高度
    viewport_height: usize,
    /// 滚动偏移（从顶部计算）
    scroll_offset: usize,
    /// 是否自动跟随底部
    auto_follow: bool,
    /// 大历史自动启用阈值
    threshold: usize,
}

impl VirtualViewport {
    /// 创建新的虚拟视口
    pub fn new(viewport_height: usize) -> Self {
        Self {
            total_lines: 0,
            viewport_height,
            scroll_offset: 0,
            auto_follow: true,
            threshold: 1000,
        }
    }
    
    /// 更新总行数
    pub fn set_total_lines(&mut self, total: usize) {
        self.total_lines = total;
        if self.auto_follow {
            self.scroll_to_bottom();
        }
    }
    
    /// 滚动到底部
    pub fn scroll_to_bottom(&mut self) {
        if self.total_lines > self.viewport_height {
            self.scroll_offset = self.total_lines - self.viewport_height;
        } else {
            self.scroll_offset = 0;
        }
    }
    
    /// 向上滚动
    pub fn scroll_up(&mut self, lines: usize) {
        self.auto_follow = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }
    
    /// 向下滚动
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = (self.scroll_offset + lines)
            .min(self.total_lines.saturating_sub(self.viewport_height));
        
        // 如果滚动到底部，重新启用自动跟随
        if self.scroll_offset >= self.total_lines.saturating_sub(self.viewport_height) {
            self.auto_follow = true;
        }
    }
    
    /// 获取当前应该显示的行范围
    pub fn visible_range(&self) -> std::ops::Range<usize> {
        let start = self.scroll_offset;
        let end = (start + self.viewport_height).min(self.total_lines);
        start..end
    }
    
    /// 是否需要虚拟滚动
    pub fn is_virtual(&self) -> bool {
        self.total_lines > self.threshold
    }
    
    /// 设置视口高度
    pub fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height;
        if self.auto_follow {
            self.scroll_to_bottom();
        }
    }
}

/// 主 TUI 结构体
/// 
/// 差分渲染引擎，管理组件树和覆盖层
pub struct Tui {
    terminal: Box<dyn Terminal>,
    root: Container,
    overlays: Vec<OverlayEntry>,
    prev_buffer: Vec<String>,
    focused: Option<usize>, // 覆盖层索引，None 表示根容器
    needs_render: bool,
    focus_order_counter: usize,
    cursor_row: usize,
    hardware_cursor_row: usize,
    prev_width: u16,
    prev_height: u16,
    prev_viewport_top: usize,
    max_lines_rendered: usize,
    full_redraw_count: usize,
    /// 批量更新模式
    batch_mode: bool,
    /// 虚拟视口（用于大内容优化）
    viewport: Option<VirtualViewport>,
}

impl Tui {
    /// 创建新的 TUI 实例
    pub fn new(terminal: Box<dyn Terminal>) -> Self {
        Self {
            terminal,
            root: Container::new(),
            overlays: Vec::new(),
            prev_buffer: Vec::new(),
            focused: None,
            needs_render: true,
            focus_order_counter: 0,
            cursor_row: 0,
            hardware_cursor_row: 0,
            prev_width: 0,
            prev_height: 0,
            prev_viewport_top: 0,
            max_lines_rendered: 0,
            full_redraw_count: 0,
            batch_mode: false,
            viewport: None,
        }
    }
    
    /// 批量更新模式 - 多个组件变更合并为一次渲染
    /// 调用 begin_batch() 后，所有 invalidate 只标记但不触发渲染
    /// 调用 end_batch() 时执行一次统一渲染
    pub fn begin_batch(&mut self) {
        self.batch_mode = true;
    }
    
    /// 结束批量更新并执行渲染
    pub fn end_batch(&mut self) -> Result<()> {
        self.batch_mode = false;
        // 执行统一渲染
        if self.needs_render {
            self.render()?;
        }
        Ok(())
    }
    
    /// 设置虚拟视口
    pub fn set_viewport(&mut self, viewport: VirtualViewport) {
        self.viewport = Some(viewport);
        self.needs_render = true;
    }
    
    /// 获取虚拟视口的可变引用
    pub fn viewport_mut(&mut self) -> Option<&mut VirtualViewport> {
        self.viewport.as_mut()
    }
    
    /// 获取根容器的可变引用
    pub fn root(&mut self) -> &mut Container {
        &mut self.root
    }
    
    /// 标记需要重渲染
    pub fn invalidate(&mut self) {
        self.needs_render = true;
        // 批量模式下不立即触发渲染，只标记
        if !self.batch_mode {
            self.root.invalidate();
            for overlay in &mut self.overlays {
                overlay.component.invalidate();
            }
        }
    }
    
    /// 显示覆盖层
    pub fn show_overlay(&mut self, component: Box<dyn Component>, options: OverlayOptions) -> OverlayHandle {
        let non_capturing = options.non_capturing;
        
        let entry = OverlayEntry {
            component,
            options,
            pre_focus: self.focused,
            hidden: AtomicBool::new(false),
            focus_order: AtomicUsize::new(self.focus_order_counter),
        };
        
        let index = self.overlays.len();
        self.overlays.push(entry);
        
        // 如果不是非捕获模式，设置焦点
        if !non_capturing {
            self.focused = Some(index);
        }
        
        let _ = self.terminal.hide_cursor();
        self.needs_render = true;
        
        OverlayHandle {
            index,
            hidden: Arc::new(AtomicBool::new(false)),
            focus_order: Arc::new(AtomicUsize::new(self.focus_order_counter)),
        }
    }
    
    /// 移除覆盖层
    fn remove_overlay(&mut self, index: usize) {
        if index < self.overlays.len() {
            let overlay = self.overlays.remove(index);
            
            // 恢复焦点
            if self.focused == Some(index) {
                self.focused = overlay.pre_focus;
            }
            
            if self.overlays.is_empty() {
                let _ = self.terminal.hide_cursor();
            }
            
            self.needs_render = true;
        }
    }
    
    /// 聚焦覆盖层
    fn focus_overlay(&mut self, index: usize) {
        if index < self.overlays.len() && !self.overlays[index].hidden.load(Ordering::SeqCst) {
            self.focused = Some(index);
            self.focus_order_counter += 1;
            self.overlays[index].focus_order.store(self.focus_order_counter, Ordering::SeqCst);
            self.needs_render = true;
        }
    }
    
    /// 取消聚焦覆盖层
    fn unfocus_overlay(&mut self, index: usize) {
        if self.focused == Some(index) {
            // 找到下一个可见的覆盖层或恢复之前的焦点
            if let Some(top_visible) = self.get_topmost_visible_overlay() {
                if top_visible != index {
                    self.focused = Some(top_visible);
                } else if let Some(pre_focus) = self.overlays[index].pre_focus {
                    self.focused = Some(pre_focus);
                } else {
                    self.focused = None;
                }
            } else {
                self.focused = self.overlays[index].pre_focus;
            }
            self.needs_render = true;
        }
    }
    
    /// 检查覆盖层是否聚焦
    fn is_overlay_focused(&self, index: usize) -> bool {
        self.focused == Some(index)
    }
    
    /// 获取最顶层的可见覆盖层
    fn get_topmost_visible_overlay(&self) -> Option<usize> {
        (0..self.overlays.len()).rev().find(|&i| !self.overlays[i].hidden.load(Ordering::SeqCst) && !self.overlays[i].options.non_capturing)
    }
    
    /// 设置焦点
    pub fn focus(&mut self, overlay_index: Option<usize>) {
        self.focused = overlay_index;
        self.needs_render = true;
    }
    
    /// 处理输入事件
    pub fn handle_input(&mut self, data: &str) {
        // 如果聚焦的覆盖层不再可见，转移焦点
        if let Some(focused_idx) = self.focused {
            if focused_idx < self.overlays.len() {
                if self.overlays[focused_idx].hidden.load(Ordering::SeqCst) {
                    // 找到下一个可见的
                    if let Some(top_visible) = self.get_topmost_visible_overlay() {
                        self.focused = Some(top_visible);
                    } else {
                        self.focused = self.overlays[focused_idx].pre_focus;
                    }
                }
            } else {
                self.focused = None;
            }
        }
        
        // 传递输入到聚焦的组件
        let handled = if let Some(focused_idx) = self.focused {
            if focused_idx < self.overlays.len() {
                let overlay = &mut self.overlays[focused_idx];
                // 检查是否需要键释放事件
                if !data.starts_with("\x1b[") || overlay.component.wants_key_release() {
                    overlay.component.handle_input(data)
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            // 根容器不处理输入
            false
        };
        
        if handled {
            self.needs_render = true;
        }
    }
    
    /// 执行一次渲染周期（差分更新）
    pub fn render(&mut self) -> Result<()> {
        if !self.needs_render {
            return Ok(());
        }
        
        let (width, height) = self.terminal.size();
        let width_changed = self.prev_width != 0 && self.prev_width != width;
        let height_changed = self.prev_height != 0 && self.prev_height != height;
        
        // 渲染根组件
        let mut new_lines = self.root.render(width);
        
        // 合成覆盖层
        if !self.overlays.is_empty() {
            new_lines = self.composite_overlays(new_lines, width, height);
        }
        
        // 提取光标位置
        let cursor_pos = self.extract_cursor_position(&mut new_lines, height as usize);
        
        // 应用行重置
        new_lines = self.apply_line_resets(new_lines);
        
        // 检查是否需要完整重绘
        let needs_full_redraw = width_changed 
            || height_changed 
            || self.prev_buffer.is_empty()
            || new_lines.len() < self.max_lines_rendered && self.overlays.is_empty();
        
        if needs_full_redraw {
            self.full_redraw(&new_lines, width_changed || height_changed || self.prev_buffer.is_empty())?;
        } else {
            self.differential_render(&new_lines, width, height)?;
        }
        
        // 定位硬件光标
        self.position_hardware_cursor(cursor_pos, new_lines.len());
        
        // 更新状态
        let new_lines_len = new_lines.len();
        self.max_lines_rendered = self.max_lines_rendered.max(new_lines_len);
        self.prev_buffer = new_lines;
        self.prev_width = width;
        self.prev_height = height;
        self.prev_viewport_top = new_lines_len.saturating_sub(height as usize);
        self.needs_render = false;
        
        self.terminal.flush()?;
        Ok(())
    }
    
    /// 完整重绘
    fn full_redraw(&mut self, lines: &[String], clear: bool) -> Result<()> {
        self.full_redraw_count += 1;
        
        let mut buffer = String::new();
        
        // 开始同步输出
        buffer.push_str("\x1b[?2026h");
        
        if clear {
            // 清屏、归位、清除滚动缓冲区
            buffer.push_str("\x1b[2J\x1b[H\x1b[3J");
        }
        
        // 输出所有行
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                buffer.push_str("\r\n");
            }
            buffer.push_str(line);
        }
        
        // 结束同步输出
        buffer.push_str("\x1b[?2026l");
        
        self.terminal.write(&buffer)?;
        
        self.cursor_row = lines.len().saturating_sub(1);
        self.hardware_cursor_row = self.cursor_row;
        
        if clear {
            self.max_lines_rendered = lines.len();
        }
        
        Ok(())
    }
    
    /// 差分渲染
    fn differential_render(&mut self, new_lines: &[String], width: u16, height: u16) -> Result<()> {
        // 找到第一个和最后一个变化的行
        let mut first_changed: Option<usize> = None;
        let mut last_changed: Option<usize> = None;
        
        let max_lines = new_lines.len().max(self.prev_buffer.len());
        
        for i in 0..max_lines {
            let old_line = self.prev_buffer.get(i).map(|s| s.as_str()).unwrap_or("");
            let new_line = new_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            
            if old_line != new_line {
                if first_changed.is_none() {
                    first_changed = Some(i);
                }
                last_changed = Some(i);
            }
        }
        
        // 没有变化
        if first_changed.is_none() {
            return Ok(());
        }
        
        let first = first_changed.unwrap();
        let last = last_changed.unwrap();
        
        // 构建差分输出
        let mut buffer = String::new();
        buffer.push_str("\x1b[?2026h"); // 开始同步输出
        
        // 计算视口位置
        let _viewport_top = if new_lines.len() > height as usize {
            new_lines.len() - height as usize
        } else {
            0
        };
        
        // 移动光标到第一行变化处
        let target_row = first;
        let row_diff = target_row as i32 - self.hardware_cursor_row as i32;
        
        if row_diff > 0 {
            buffer.push_str(&format!("\x1b[{}B", row_diff));
        } else if row_diff < 0 {
            buffer.push_str(&format!("\x1b[{}A", -row_diff));
        }
        
        buffer.push('\r'); // 回到行首
        
        // 输出变化的行
        for i in first..=last {
            if i > first {
                buffer.push_str("\r\n");
            }
            buffer.push_str("\x1b[2K"); // 清除当前行
            
            if let Some(line) = new_lines.get(i) {
                // 验证行宽度
                let line_width = visible_width(line);
                if line_width > width as usize {
                    // 行太宽，截断
                    let truncated = slice_by_column(line, 0, width as usize, true);
                    buffer.push_str(&truncated);
                } else {
                    buffer.push_str(line);
                }
            }
        }
        
        // 如果之前有更多行，清除它们
        if self.prev_buffer.len() > new_lines.len() {
            let extra_lines = self.prev_buffer.len() - new_lines.len();
            for _ in 0..extra_lines {
                buffer.push_str("\r\n\x1b[2K");
            }
            // 移回
            if extra_lines > 0 {
                buffer.push_str(&format!("\x1b[{}A", extra_lines));
            }
        }
        
        buffer.push_str("\x1b[?2026l"); // 结束同步输出
        
        self.terminal.write(&buffer)?;
        
        self.cursor_row = new_lines.len().saturating_sub(1);
        self.hardware_cursor_row = last;
        
        Ok(())
    }
    
    /// 合成覆盖层
    fn composite_overlays(&self, mut lines: Vec<String>, term_width: u16, term_height: u16) -> Vec<String> {
        if self.overlays.is_empty() {
            return lines;
        }
        
        let mut rendered_overlays: Vec<(Vec<String>, u16, u16, u16)> = Vec::new();
        let mut min_lines_needed = lines.len();
        
        // 收集可见覆盖层
        let mut visible_indices: Vec<usize> = (0..self.overlays.len())
            .filter(|&i| !self.overlays[i].hidden.load(Ordering::SeqCst))
            .collect();
        
        // 按 focus_order 排序
        visible_indices.sort_by_key(|&i| self.overlays[i].focus_order.load(Ordering::SeqCst));
        
        for &idx in &visible_indices {
            let overlay = &self.overlays[idx];
            let options = &overlay.options;
            
            // 解析布局
            let layout = self.resolve_overlay_layout(options, 0, term_width, term_height);
            let overlay_width = layout.width;
            
            // 渲染组件
            let mut overlay_lines = overlay.component.render(overlay_width);
            
            // 应用最大高度限制
            if let Some(max_h) = layout.max_height {
                if overlay_lines.len() > max_h as usize {
                    overlay_lines.truncate(max_h as usize);
                }
            }
            
            let overlay_lines_len = overlay_lines.len();
            
            // 重新计算位置（考虑实际高度）
            let layout = self.resolve_overlay_layout(options, overlay_lines_len as u16, term_width, term_height);
            
            rendered_overlays.push((overlay_lines, layout.row, layout.col, overlay_width));
            min_lines_needed = min_lines_needed.max(layout.row as usize + overlay_lines_len);
        }
        
        // 确保有足够的行
        let working_height = lines.len().max(term_height as usize).max(min_lines_needed);
        while lines.len() < working_height {
            lines.push(String::new());
        }
        
        let viewport_start = working_height.saturating_sub(term_height as usize);
        
        // 合成每个覆盖层
        for (overlay_lines, row, col, w) in rendered_overlays {
            for (i, overlay_line) in overlay_lines.iter().enumerate() {
                let idx = viewport_start + row as usize + i;
                if idx < lines.len() {
                    lines[idx] = self.composite_line_at(&lines[idx], overlay_line, col as usize, w as usize, term_width as usize);
                }
            }
        }
        
        lines
    }
    
    /// 在指定位置合成一行
    fn composite_line_at(&self, base_line: &str, overlay_line: &str, start_col: usize, overlay_width: usize, total_width: usize) -> String {
        // 提取基础行的前后部分
        let (before, before_width, after, after_width) = crate::utils::extract_segments(
            base_line,
            start_col,
            start_col + overlay_width,
            total_width.saturating_sub(start_col + overlay_width),
            true,
        );
        
        // 截断覆盖层行到声明宽度
        let truncated_overlay = if visible_width(overlay_line) > overlay_width {
            slice_by_column(overlay_line, 0, overlay_width, true)
        } else {
            overlay_line.to_string()
        };
        
        // 计算填充
        let before_pad = start_col.saturating_sub(before_width);
        let overlay_pad = overlay_width.saturating_sub(visible_width(&truncated_overlay));
        let after_target = total_width.saturating_sub(before_width.max(start_col) + overlay_width);
        let after_pad = after_target.saturating_sub(after_width);
        
        // 组合结果
        let reset = "\x1b[0m\x1b]8;;\x07";
        let result = format!(
            "{}{}{}{}{}{}{}{}",
            before,
            " ".repeat(before_pad),
            reset,
            truncated_overlay,
            " ".repeat(overlay_pad),
            reset,
            after,
            " ".repeat(after_pad)
        );
        
        // 最终验证和截断
        let result_width = visible_width(&result);
        if result_width > total_width {
            slice_by_column(&result, 0, total_width, true)
        } else {
            result
        }
    }
    
    /// 解析覆盖层布局
    fn resolve_overlay_layout(
        &self,
        options: &OverlayOptions,
        overlay_height: u16,
        term_width: u16,
        term_height: u16,
    ) -> OverlayLayout {
        let margin = options.margin.unwrap_or_default();
        
        // 可用空间
        let avail_width = (term_width as i32 - margin.left as i32 - margin.right as i32).max(1) as u16;
        let avail_height = (term_height as i32 - margin.top as i32 - margin.bottom as i32).max(1) as u16;
        
        // 解析宽度
        let mut width = options.width.map(|w| w.to_absolute(term_width)).unwrap_or(avail_width.min(80));
        if let Some(min_w) = options.min_width {
            width = width.max(min_w);
        }
        width = width.min(avail_width).max(1);
        
        // 解析最大高度
        let max_height = options.max_height.map(|h| h.to_absolute(term_height).min(avail_height).max(1));
        
        // 有效高度
        let effective_height = max_height.map(|h| overlay_height.min(h)).unwrap_or(overlay_height);
        
        // 解析位置
        let (mut row, mut col) = if let Some(row_val) = options.row {
            let r = row_val.to_absolute(term_height);
            let c = options.col.map(|c| c.to_absolute(term_width)).unwrap_or_else(|| {
                // 默认居中
                margin.left + (avail_width - width) / 2
            });
            (r, c)
        } else {
            // 使用锚点
            let anchor = options.anchor.unwrap_or_default();
            let r = self.resolve_anchor_row(anchor, effective_height, avail_height, margin.top);
            let c = self.resolve_anchor_col(anchor, width, avail_width, margin.left);
            (r, c)
        };
        
        // 应用偏移
        if let Some(offset_y) = options.offset_y {
            row = (row as i16 + offset_y) as u16;
        }
        if let Some(offset_x) = options.offset_x {
            col = (col as i16 + offset_x) as u16;
        }
        
        // 限制在边界内
        row = row.clamp(margin.top, term_height - margin.bottom - effective_height);
        col = col.clamp(margin.left, term_width - margin.right - width);
        
        OverlayLayout {
            width,
            row,
            col,
            max_height,
        }
    }
    
    /// 根据锚点解析行位置
    fn resolve_anchor_row(&self, anchor: OverlayAnchor, height: u16, avail_height: u16, margin_top: u16) -> u16 {
        match anchor {
            OverlayAnchor::TopLeft |
            OverlayAnchor::TopCenter |
            OverlayAnchor::TopRight => margin_top,
            OverlayAnchor::BottomLeft |
            OverlayAnchor::BottomCenter |
            OverlayAnchor::BottomRight => margin_top + avail_height.saturating_sub(height),
            _ => margin_top + (avail_height.saturating_sub(height)) / 2,
        }
    }
    
    /// 根据锚点解析列位置
    fn resolve_anchor_col(&self, anchor: OverlayAnchor, width: u16, avail_width: u16, margin_left: u16) -> u16 {
        match anchor {
            OverlayAnchor::TopLeft |
            OverlayAnchor::LeftCenter |
            OverlayAnchor::BottomLeft => margin_left,
            OverlayAnchor::TopRight |
            OverlayAnchor::RightCenter |
            OverlayAnchor::BottomRight => margin_left + avail_width.saturating_sub(width),
            _ => margin_left + (avail_width.saturating_sub(width)) / 2,
        }
    }
    
    /// 提取光标位置
    fn extract_cursor_position(&self, lines: &mut [String], height: usize) -> Option<(usize, usize)> {
        let viewport_top = lines.len().saturating_sub(height);
        
        for row in (viewport_top..lines.len()).rev() {
            if let Some(line) = lines.get(row) {
                if let Some(marker_pos) = line.find(CURSOR_MARKER) {
                    // 计算光标列位置
                    let before_marker = &line[..marker_pos];
                    let col = visible_width(before_marker);
                    
                    // 从行中移除标记
                    let new_line = format!("{}{}", before_marker, &line[marker_pos + CURSOR_MARKER.len()..]);
                    lines[row] = new_line;
                    
                    return Some((row, col));
                }
            }
        }
        
        None
    }
    
    /// 应用行重置
    fn apply_line_resets(&self, lines: Vec<String>) -> Vec<String> {
        const SEGMENT_RESET: &str = "\x1b[0m\x1b]8;;\x07";
        
        lines.into_iter()
            .map(|line| {
                // 这里可以添加图像行检测逻辑
                format!("{}{}", line, SEGMENT_RESET)
            })
            .collect()
    }
    
    /// 定位硬件光标
    fn position_hardware_cursor(&mut self, cursor_pos: Option<(usize, usize)>, total_lines: usize) {
        if let Some((row, col)) = cursor_pos {
            if total_lines > 0 {
                let target_row = row.min(total_lines - 1);
                
                // 移动光标
                let row_delta = target_row as i32 - self.hardware_cursor_row as i32;
                let mut buffer = String::new();
                
                if row_delta > 0 {
                    buffer.push_str(&format!("\x1b[{}B", row_delta));
                } else if row_delta < 0 {
                    buffer.push_str(&format!("\x1b[{}A", -row_delta));
                }
                
                // 移动到指定列（1-indexed）
                buffer.push_str(&format!("\x1b[{}G", col + 1));
                
                let _ = self.terminal.write(&buffer);
                
                self.hardware_cursor_row = target_row;
            }
        } else {
            let _ = self.terminal.hide_cursor();
        }
    }
    
    /// 获取完整重绘次数（调试用）
    pub fn full_redraw_count(&self) -> usize {
        self.full_redraw_count
    }
    
    /// 检查是否有可见覆盖层
    pub fn has_visible_overlay(&self) -> bool {
        self.overlays.iter().any(|o| !o.hidden.load(Ordering::SeqCst))
    }
}

/// 覆盖层布局计算结果
struct OverlayLayout {
    width: u16,
    row: u16,
    col: u16,
    max_height: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    struct MockComponent {
        lines: Vec<String>,
    }
    
    impl Component for MockComponent {
        fn render(&self, _width: u16) -> Vec<String> {
            self.lines.clone()
        }
        
        fn invalidate(&mut self) {}
    }
    
    #[test]
    fn test_container() {
        let mut container = Container::new();
        assert!(container.is_empty());
        
        container.add_child(Box::new(MockComponent {
            lines: vec!["line1".to_string()],
        }));
        
        assert_eq!(container.len(), 1);
        assert!(!container.is_empty());
    }
    
    #[test]
    fn test_overlay_anchor() {
        assert_eq!(OverlayAnchor::default(), OverlayAnchor::Center);
    }
    
    #[test]
    fn test_size_value() {
        let abs = SizeValue::Absolute(10);
        assert_eq!(abs.to_absolute(100), 10);
        
        let pct = SizeValue::Percent(50.0);
        assert_eq!(pct.to_absolute(100), 50);
    }
}
