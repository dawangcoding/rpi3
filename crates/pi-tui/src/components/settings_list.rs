//! 设置列表组件
//! 支持多种设置类型的显示和编辑

use crate::tui::{Component, Focusable};
use crate::utils::{truncate_to_width_with_ellipsis, visible_width};

/// 值变更回调类型
type OnChangeCallback = Box<dyn Fn(&str, &SettingValue) + Send>;

/// 设置值类型
/// 
/// 支持的各种设置值类型
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    /// 布尔值
    Boolean(bool),
    /// 字符串值
    String(String),
    /// 数值
    Number(f64),
    /// 枚举值（选项列表和当前选中索引）
    Enum {
        /// 选项列表
        options: Vec<String>,
        /// 当前选中索引
        selected: usize
    },
}

impl SettingValue {
    /// 获取值的显示文本
    pub fn display_text(&self) -> String {
        match self {
            SettingValue::Boolean(b) => if *b { "✓" } else { "✗" }.to_string(),
            SettingValue::String(s) => s.clone(),
            SettingValue::Number(n) => format!("{:.2}", n),
            SettingValue::Enum { options, selected } => {
                options.get(*selected).cloned().unwrap_or_default()
            }
        }
    }

    /// 是否可交互编辑
    pub fn is_editable(&self) -> bool {
        matches!(self, SettingValue::Boolean(_) | SettingValue::Number(_) | SettingValue::Enum { .. })
    }
}

/// 设置项
/// 
/// 单个配置项的定义和当前值
#[derive(Debug, Clone)]
pub struct SettingEntry {
    /// 设置名称
    pub name: String,
    /// 设置描述
    pub description: String,
    /// 设置值
    pub value: SettingValue,
    /// 唯一标识键
    pub key: String,
}

impl SettingEntry {
    /// 创建新的设置项
    pub fn new(key: &str, name: &str, value: SettingValue) -> Self {
        Self {
            name: name.to_string(),
            description: String::new(),
            value,
            key: key.to_string(),
        }
    }

    /// 创建带描述的设置项
    pub fn with_description(key: &str, name: &str, description: &str, value: SettingValue) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            value,
            key: key.to_string(),
        }
    }

    /// 匹配过滤文本
    fn matches_filter(&self, filter: &str) -> bool {
        let filter_lower = filter.to_lowercase();
        self.name.to_lowercase().contains(&filter_lower)
            || self.description.to_lowercase().contains(&filter_lower)
            || self.key.to_lowercase().contains(&filter_lower)
    }
}

/// 设置分类
/// 
/// 将相关设置项分组显示
#[derive(Debug, Clone)]
pub struct SettingsCategory {
    /// 分类名称
    pub name: String,
    /// 分类下的设置项
    pub entries: Vec<SettingEntry>,
}

impl SettingsCategory {
    /// 创建新的设置分类
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            entries: Vec::new(),
        }
    }

    /// 添加设置项
    pub fn add_entry(&mut self, entry: SettingEntry) {
        self.entries.push(entry);
    }

    /// 创建带初始设置项的分类
    pub fn with_entries(name: &str, entries: Vec<SettingEntry>) -> Self {
        Self {
            name: name.to_string(),
            entries,
        }
    }
}

/// 扁平化的设置项引用（用于导航）
struct FlatEntry {
    category_index: usize,
    entry_index: usize,
    is_category_header: bool,
}

/// 设置列表组件
/// 
/// 支持多种设置类型的显示和交互式编辑
pub struct SettingsList {
    /// 设置分类列表
    categories: Vec<SettingsCategory>,
    /// 扁平化的索引映射
    flat_indices: Vec<FlatEntry>,
    /// 当前选中的索引
    selected_index: usize,
    /// 滚动偏移
    scroll_offset: usize,
    /// 是否聚焦
    focused: bool,
    /// 是否需要重绘
    needs_redraw: bool,
    /// 过滤文本
    filter_text: String,
    /// 最大可见项数
    max_visible: usize,
    /// 值变更回调
    on_change: Option<OnChangeCallback>,
}

impl SettingsList {
    /// 创建新的设置列表
    pub fn new(categories: Vec<SettingsCategory>) -> Self {
        let mut list = Self {
            categories,
            flat_indices: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
            focused: false,
            needs_redraw: true,
            filter_text: String::new(),
            max_visible: 10,
            on_change: None,
        };
        list.rebuild_flat_indices();
        list
    }

    /// 创建空的设置列表
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// 设置分类
    pub fn set_categories(&mut self, categories: Vec<SettingsCategory>) {
        self.categories = categories;
        self.rebuild_flat_indices();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.needs_redraw = true;
    }

    /// 设置过滤文本
    pub fn set_filter(&mut self, filter: &str) {
        self.filter_text = filter.to_lowercase();
        self.rebuild_flat_indices();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.needs_redraw = true;
    }

    /// 清除过滤
    pub fn clear_filter(&mut self) {
        self.filter_text.clear();
        self.rebuild_flat_indices();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.needs_redraw = true;
    }

    /// 设置最大可见项数
    pub fn set_max_visible(&mut self, max: usize) {
        self.max_visible = max.max(1);
        self.update_scroll_offset();
        self.needs_redraw = true;
    }

    /// 设置值变更回调
    pub fn on_change(&mut self, callback: OnChangeCallback) {
        self.on_change = Some(callback);
    }

    /// 获取当前选中的设置项
    pub fn selected_entry(&self) -> Option<(&SettingsCategory, &SettingEntry)> {
        let flat = self.flat_indices.get(self.selected_index)?;
        if flat.is_category_header {
            return None;
        }
        let category = self.categories.get(flat.category_index)?;
        let entry = category.entries.get(flat.entry_index)?;
        Some((category, entry))
    }

    /// 选择下一项
    pub fn select_next(&mut self) {
        if self.flat_indices.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.flat_indices.len();
        // 跳过分类标题
        if let Some(flat) = self.flat_indices.get(self.selected_index) {
            if flat.is_category_header && self.flat_indices.len() > 1 {
                self.selected_index = (self.selected_index + 1) % self.flat_indices.len();
            }
        }
        self.update_scroll_offset();
        self.needs_redraw = true;
    }

    /// 选择上一项
    pub fn select_prev(&mut self) {
        if self.flat_indices.is_empty() {
            return;
        }
        self.selected_index = if self.selected_index == 0 {
            self.flat_indices.len() - 1
        } else {
            self.selected_index - 1
        };
        // 跳过分类标题
        if let Some(flat) = self.flat_indices.get(self.selected_index) {
            if flat.is_category_header && self.flat_indices.len() > 1 {
                self.selected_index = if self.selected_index == 0 {
                    self.flat_indices.len() - 1
                } else {
                    self.selected_index - 1
                };
            }
        }
        self.update_scroll_offset();
        self.needs_redraw = true;
    }

    /// 切换布尔值或枚举选项
    pub fn toggle_selected(&mut self) -> bool {
        if let Some((_, entry)) = self.selected_entry() {
            let key = entry.key.clone();
            let new_value = match &entry.value {
                SettingValue::Boolean(b) => Some(SettingValue::Boolean(!b)),
                SettingValue::Enum { options, selected } => {
                    if options.is_empty() {
                        None
                    } else {
                        let new_selected = (selected + 1) % options.len();
                        Some(SettingValue::Enum {
                            options: options.clone(),
                            selected: new_selected,
                        })
                    }
                }
                _ => None,
            };

            if let Some(new_value) = new_value {
                // 更新值
                if let Some(flat) = self.flat_indices.get(self.selected_index) {
                    if let Some(category) = self.categories.get_mut(flat.category_index) {
                        if let Some(entry) = category.entries.get_mut(flat.entry_index) {
                            entry.value = new_value.clone();
                            // 触发回调
                            if let Some(ref callback) = self.on_change {
                                callback(&key, &new_value);
                            }
                        }
                    }
                }
                self.needs_redraw = true;
                return true;
            }
        }
        false
    }

    /// 调整数值（增加）
    pub fn increment_number(&mut self, delta: f64) -> bool {
        if let Some((_, entry)) = self.selected_entry() {
            if let SettingValue::Number(n) = entry.value {
                let key = entry.key.clone();
                let new_value = SettingValue::Number(n + delta);
                if let Some(flat) = self.flat_indices.get(self.selected_index) {
                    if let Some(category) = self.categories.get_mut(flat.category_index) {
                        if let Some(entry) = category.entries.get_mut(flat.entry_index) {
                            entry.value = new_value.clone();
                            if let Some(ref callback) = self.on_change {
                                callback(&key, &new_value);
                            }
                        }
                    }
                }
                self.needs_redraw = true;
                return true;
            }
        }
        false
    }

    /// 调整数值（减少）
    pub fn decrement_number(&mut self, delta: f64) -> bool {
        self.increment_number(-delta)
    }

    /// 获取总项数（包含分类标题）
    pub fn total_count(&self) -> usize {
        self.flat_indices.len()
    }

    /// 获取过滤后的设置项数
    pub fn filtered_entry_count(&self) -> usize {
        self.flat_indices
            .iter()
            .filter(|f| !f.is_category_header)
            .count()
    }

    /// 重建扁平化索引
    fn rebuild_flat_indices(&mut self) {
        self.flat_indices.clear();

        for (cat_idx, category) in self.categories.iter().enumerate() {
            // 添加分类标题
            self.flat_indices.push(FlatEntry {
                category_index: cat_idx,
                entry_index: 0,
                is_category_header: true,
            });

            // 添加过滤后的设置项
            for (entry_idx, entry) in category.entries.iter().enumerate() {
                if self.filter_text.is_empty() || entry.matches_filter(&self.filter_text) {
                    self.flat_indices.push(FlatEntry {
                        category_index: cat_idx,
                        entry_index: entry_idx,
                        is_category_header: false,
                    });
                }
            }
        }
    }

    /// 更新滚动偏移
    fn update_scroll_offset(&mut self) {
        if self.flat_indices.len() <= self.max_visible {
            self.scroll_offset = 0;
            return;
        }

        let half_visible = self.max_visible / 2;
        if self.selected_index < half_visible {
            self.scroll_offset = 0;
        } else if self.selected_index >= self.flat_indices.len() - half_visible {
            self.scroll_offset = self.flat_indices.len() - self.max_visible;
        } else {
            self.scroll_offset = self.selected_index - half_visible;
        }
    }

    /// 渲染分类标题
    fn render_category_header(&self, category: &SettingsCategory, is_selected: bool) -> String {
        let prefix = if is_selected { "▶ " } else { "  " };
        let style = "\x1b[1m"; // 粗体
        let reset = "\x1b[0m";
        format!("{}{}{}{}{}", prefix, style, &category.name, reset, " ─".repeat(10))
    }

    /// 渲染设置项
    fn render_entry(&self, entry: &SettingEntry, is_selected: bool, width: u16) -> String {
        let prefix = if is_selected { "→ " } else { "  " };
        let prefix_width = visible_width(prefix);
        let available = width as usize - prefix_width;

        // 计算名称和值的空间分配
        let name_width = (available / 3).min(24);
        let value_text = entry.value.display_text();
        let value_width = visible_width(&value_text);
        let remaining = available.saturating_sub(name_width + 2);

        // 截断名称和描述
        let truncated_name = truncate_to_width_with_ellipsis(&entry.name, name_width, "");

        // 渲染行
        let spacing = " ".repeat(name_width.saturating_sub(visible_width(&truncated_name)));

        // 如果有描述且空间足够，显示描述
        let description_part = if !entry.description.is_empty() && remaining > 10 {
            let desc_avail = remaining.saturating_sub(value_width + 4);
            if desc_avail > 5 {
                let truncated_desc = truncate_to_width_with_ellipsis(&entry.description, desc_avail, "");
                format!("  {}", truncated_desc)
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // 值部分（高亮显示）
        let value_style = if is_selected { "\x1b[36m" } else { "\x1b[90m" };
        let reset = "\x1b[0m";
        let value_part = format!("{}{}{}", value_style, value_text, reset);

        format!("{}{}{}{}  {}{}", prefix, truncated_name, spacing, description_part, value_part, reset)
    }
}

impl Component for SettingsList {
    fn render(&self, width: u16) -> Vec<String> {
        let mut lines = Vec::new();

        // 如果没有项，显示提示
        if self.flat_indices.is_empty() {
            lines.push("  No settings available".to_string());
            return lines;
        }

        // 计算可见范围
        let start = self.scroll_offset;
        let end = (start + self.max_visible).min(self.flat_indices.len());

        // 渲染可见项
        for i in start..end {
            if let Some(flat) = self.flat_indices.get(i) {
                let is_selected = i == self.selected_index;

                if flat.is_category_header {
                    if let Some(category) = self.categories.get(flat.category_index) {
                        lines.push(self.render_category_header(category, is_selected));
                    }
                } else if let Some(category) = self.categories.get(flat.category_index) {
                    if let Some(entry) = category.entries.get(flat.entry_index) {
                        lines.push(self.render_entry(entry, is_selected, width));
                    }
                }
            }
        }

        // 添加滚动指示器
        if self.flat_indices.len() > self.max_visible {
            let scroll_text = format!(
                "  ({}/{})",
                self.selected_index + 1,
                self.flat_indices.len()
            );
            let truncated = truncate_to_width_with_ellipsis(&scroll_text, width as usize - 2, "");
            lines.push(truncated);
        }

        lines
    }

    fn handle_input(&mut self, data: &str) -> bool {
        match data {
            // 向下
            "\x1b[B" | "\t" => {
                self.select_next();
                true
            }
            // 向上
            "\x1b[A" | "\x1b[Z" => {
                self.select_prev();
                true
            }
            // Enter - 切换布尔/枚举
            "\r" | "\n" => {
                self.toggle_selected()
            }
            // 右箭头 - 增加数值
            "\x1b[C" => {
                self.increment_number(0.1)
            }
            // 左箭头 - 减少数值
            "\x1b[D" => {
                self.decrement_number(0.1)
            }
            _ => false,
        }
    }

    fn invalidate(&mut self) {
        self.needs_redraw = true;
    }
}

impl Focusable for SettingsList {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.needs_redraw = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 创建测试用的设置列表
    fn create_test_settings() -> Vec<SettingsCategory> {
        vec![
            SettingsCategory::with_entries(
                "General",
                vec![
                    SettingEntry::with_description(
                        "auto_save",
                        "Auto Save",
                        "Automatically save changes",
                        SettingValue::Boolean(true),
                    ),
                    SettingEntry::new(
                        "username",
                        "Username",
                        SettingValue::String("user".to_string()),
                    ),
                ],
            ),
            SettingsCategory::with_entries(
                "Display",
                vec![
                    SettingEntry::new(
                        "font_size",
                        "Font Size",
                        SettingValue::Number(14.0),
                    ),
                    SettingEntry::with_description(
                        "theme",
                        "Theme",
                        "Color theme for the interface",
                        SettingValue::Enum {
                            options: vec!["Light".to_string(), "Dark".to_string(), "System".to_string()],
                            selected: 1,
                        },
                    ),
                ],
            ),
        ]
    }

    #[test]
    fn test_settings_list_new() {
        let settings = create_test_settings();
        let list = SettingsList::new(settings);

        assert_eq!(list.categories.len(), 2);
        assert!(list.selected_entry().is_none()); // 第一个是分类标题
    }

    #[test]
    fn test_settings_list_render() {
        let settings = create_test_settings();
        let list = SettingsList::new(settings);

        let lines = list.render(60);

        // 应该有分类标题和设置项
        assert!(!lines.is_empty());
        assert!(lines[0].contains("General"));
    }

    #[test]
    fn test_settings_list_navigation() {
        let settings = create_test_settings();
        let mut list = SettingsList::new(settings);

        // 初始选中分类标题，select_next 应该跳到第一个设置项
        list.select_next();
        let entry = list.selected_entry();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().1.key, "auto_save");

        // 继续导航
        list.select_next();
        assert_eq!(list.selected_entry().unwrap().1.key, "username");

        // 测试向上导航
        list.select_prev();
        assert_eq!(list.selected_entry().unwrap().1.key, "auto_save");
    }

    #[test]
    fn test_boolean_toggle() {
        let settings = create_test_settings();
        let mut list = SettingsList::new(settings);

        list.select_next(); // 选中 auto_save

        let initial_value = list.selected_entry().unwrap().1.value.clone();
        assert_eq!(initial_value, SettingValue::Boolean(true));

        // 切换
        let changed = list.toggle_selected();
        assert!(changed);

        let new_value = list.selected_entry().unwrap().1.value.clone();
        assert_eq!(new_value, SettingValue::Boolean(false));
    }

    #[test]
    fn test_enum_toggle() {
        let settings = create_test_settings();
        let mut list = SettingsList::new(settings);

        // 导航到 theme 设置
        list.select_next(); // auto_save
        list.select_next(); // username
        list.select_next(); // font_size
        list.select_next(); // theme

        let entry = list.selected_entry().unwrap().1;
        if let SettingValue::Enum { selected, .. } = &entry.value {
            assert_eq!(*selected, 1); // Dark
        } else {
            panic!("Expected Enum value");
        }

        // 切换枚举
        list.toggle_selected();

        let entry = list.selected_entry().unwrap().1;
        if let SettingValue::Enum { selected, .. } = &entry.value {
            assert_eq!(*selected, 2); // System
        } else {
            panic!("Expected Enum value");
        }
    }

    #[test]
    fn test_number_adjustment() {
        let settings = create_test_settings();
        let mut list = SettingsList::new(settings);

        // 导航到 font_size
        list.select_next(); // auto_save
        list.select_next(); // username
        list.select_next(); // font_size

        let initial = list.selected_entry().unwrap().1.value.clone();
        if let SettingValue::Number(n) = initial {
            assert_eq!(n, 14.0);
        } else {
            panic!("Expected Number value");
        }

        // 增加
        list.increment_number(2.0);

        let after_inc = list.selected_entry().unwrap().1.value.clone();
        if let SettingValue::Number(n) = after_inc {
            assert_eq!(n, 16.0);
        } else {
            panic!("Expected Number value");
        }

        // 减少
        list.decrement_number(1.0);

        let after_dec = list.selected_entry().unwrap().1.value.clone();
        if let SettingValue::Number(n) = after_dec {
            assert_eq!(n, 15.0);
        } else {
            panic!("Expected Number value");
        }
    }

    #[test]
    fn test_filter() {
        let settings = create_test_settings();
        let mut list = SettingsList::new(settings);

        // 过滤 "font"
        list.set_filter("font");

        // 应该只显示 font_size 设置
        list.select_next();
        let entry = list.selected_entry();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().1.key, "font_size");

        // 清除过滤
        list.clear_filter();
        list.select_next();
        assert_eq!(list.selected_entry().unwrap().1.key, "auto_save");
    }

    #[test]
    fn test_focus_management() {
        let settings = create_test_settings();
        let mut list = SettingsList::new(settings);

        assert!(!list.focused());

        list.set_focused(true);
        assert!(list.focused());

        list.set_focused(false);
        assert!(!list.focused());
    }

    #[test]
    fn test_callback() {
        let settings = create_test_settings();
        let mut list = SettingsList::new(settings);

        let changed_key = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let changed_key_clone = changed_key.clone();

        list.on_change(Box::new(move |key, _value| {
            *changed_key_clone.lock().unwrap() = key.to_string();
        }));

        list.select_next(); // 选中 auto_save
        list.toggle_selected();

        assert_eq!(*changed_key.lock().unwrap(), "auto_save");
    }

    #[test]
    fn test_setting_value_display() {
        assert_eq!(SettingValue::Boolean(true).display_text(), "✓");
        assert_eq!(SettingValue::Boolean(false).display_text(), "✗");
        assert_eq!(SettingValue::String("test".to_string()).display_text(), "test");
        assert_eq!(SettingValue::Number(3.15).display_text(), "3.15");
        assert_eq!(
            SettingValue::Enum {
                options: vec!["A".to_string(), "B".to_string()],
                selected: 0
            }
            .display_text(),
            "A"
        );
    }

    #[test]
    fn test_setting_value_editable() {
        assert!(SettingValue::Boolean(true).is_editable());
        assert!(SettingValue::Number(1.0).is_editable());
        assert!(SettingValue::Enum { options: vec![], selected: 0 }.is_editable());
        assert!(!SettingValue::String("test".to_string()).is_editable());
    }

    #[test]
    fn test_empty_list() {
        let list = SettingsList::empty();
        assert_eq!(list.total_count(), 0);
        assert!(list.selected_entry().is_none());

        let lines = list.render(40);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("No settings"));
    }

    #[test]
    fn test_handle_input() {
        let settings = create_test_settings();
        let mut list = SettingsList::new(settings);

        // 测试向下导航
        let handled = list.handle_input("\x1b[B");
        assert!(handled);
        assert_eq!(list.selected_entry().unwrap().1.key, "auto_save");

        // 测试向上导航
        let handled = list.handle_input("\x1b[A");
        assert!(handled);
        // 向上应该跳过分类标题

        // 测试 Enter 切换
        list.select_next();
        let handled = list.handle_input("\r");
        assert!(handled); // 切换布尔值

        // 测试左右调整数值（在布尔值上应该返回 false）
        list.handle_input("\x1b[C"); // 不应该有效果
    }
}
