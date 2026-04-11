//! 列表选择器组件
//! 提供可筛选、可选择的列表界面

use crate::tui::{Component, Focusable};
use crate::utils::{truncate_to_width_with_ellipsis, visible_width};

const DEFAULT_PRIMARY_COLUMN_WIDTH: usize = 32;
const PRIMARY_COLUMN_GAP: usize = 2;
const MIN_DESCRIPTION_WIDTH: usize = 10;

/// 列表项
#[derive(Debug, Clone)]
pub struct SelectItem {
    /// 标签
    pub label: String,
    /// 详细描述
    pub detail: Option<String>,
    /// 值
    pub value: String,
}

impl SelectItem {
    /// 创建新的列表项
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            label: label.to_string(),
            detail: None,
            value: value.to_string(),
        }
    }

    /// 创建带有描述的列表项
    pub fn with_detail(value: &str, label: &str, detail: &str) -> Self {
        Self {
            label: label.to_string(),
            detail: Some(detail.to_string()),
            value: value.to_string(),
        }
    }
}

/// 列表选择器
pub struct SelectList {
    items: Vec<SelectItem>,
    filtered_indices: Vec<usize>,
    selected_index: usize,
    scroll_offset: usize,
    filter: String,
    focused: bool,
    needs_render: bool,
    max_visible: usize,
    #[allow(clippy::type_complexity)]
    on_select: Option<Box<dyn Fn(&SelectItem) + Send>>,
}

impl SelectList {
    /// 创建新的列表选择器
    pub fn new(items: Vec<SelectItem>) -> Self {
        let count = items.len();
        Self {
            items,
            filtered_indices: (0..count).collect(),
            selected_index: 0,
            scroll_offset: 0,
            filter: String::new(),
            focused: false,
            needs_render: true,
            max_visible: 5,
            on_select: None,
        }
    }

    /// 设置列表项
    pub fn set_items(&mut self, items: Vec<SelectItem>) {
        self.items = items;
        self.apply_filter();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.needs_render = true;
    }

    /// 获取当前选中的项
    pub fn selected(&self) -> Option<&SelectItem> {
        self.filtered_indices
            .get(self.selected_index)
            .and_then(|&idx| self.items.get(idx))
    }

    /// 选择下一项
    pub fn select_next(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.filtered_indices.len();
            self.update_scroll_offset();
            self.needs_render = true;
        }
    }

    /// 选择上一项
    pub fn select_prev(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.selected_index = if self.selected_index == 0 {
                self.filtered_indices.len() - 1
            } else {
                self.selected_index - 1
            };
            self.update_scroll_offset();
            self.needs_render = true;
        }
    }

    /// 设置筛选条件
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_lowercase();
        self.apply_filter();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.needs_render = true;
    }

    /// 清除筛选
    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.apply_filter();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.needs_render = true;
    }

    /// 设置最大可见项数
    pub fn set_max_visible(&mut self, max: usize) {
        self.max_visible = max.max(1);
        self.update_scroll_offset();
        self.needs_render = true;
    }

    /// 获取最大可见项数
    pub fn max_visible(&self) -> usize {
        self.max_visible
    }

    /// 设置选择回调
    pub fn on_select(&mut self, callback: Box<dyn Fn(&SelectItem) + Send>) {
        self.on_select = Some(callback);
    }

    /// 触发选择回调
    pub fn confirm_selection(&self) {
        if let Some(ref callback) = self.on_select {
            if let Some(item) = self.selected() {
                callback(item);
            }
        }
    }

    /// 获取过滤后的项目数量
    pub fn filtered_count(&self) -> usize {
        self.filtered_indices.len()
    }

    /// 获取原始项目数量
    pub fn total_count(&self) -> usize {
        self.items.len()
    }

    /// 应用筛选
    fn apply_filter(&mut self) {
        if self.filter.is_empty() {
            self.filtered_indices = (0..self.items.len()).collect();
        } else {
            self.filtered_indices = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.value.to_lowercase().contains(&self.filter)
                        || item.label.to_lowercase().contains(&self.filter)
                })
                .map(|(idx, _)| idx)
                .collect();
        }
    }

    /// 更新滚动偏移
    fn update_scroll_offset(&mut self) {
        if self.filtered_indices.len() <= self.max_visible {
            self.scroll_offset = 0;
        } else {
            // 确保选中项在可见范围内
            let half_visible = self.max_visible / 2;
            if self.selected_index < half_visible {
                self.scroll_offset = 0;
            } else if self.selected_index >= self.filtered_indices.len() - half_visible {
                self.scroll_offset = self.filtered_indices.len() - self.max_visible;
            } else {
                self.scroll_offset = self.selected_index - half_visible;
            }
        }
    }

    /// 渲染单个项目
    fn render_item(
        &self,
        item: &SelectItem,
        is_selected: bool,
        width: u16,
        primary_column_width: usize,
    ) -> String {
        let prefix = if is_selected { "→ " } else { "  " };
        let prefix_width = visible_width(prefix);
        let available_width = width as usize - prefix_width;

        if let Some(ref detail) = item.detail {
            let detail_single_line = detail.replace('\n', " ");
            
            if available_width > 40 {
                // 双列布局：标签 + 描述
                let effective_primary_width = primary_column_width.min(available_width - 4);
                let max_primary_width = effective_primary_width.saturating_sub(PRIMARY_COLUMN_GAP);
                
                let truncated_label = truncate_to_width_with_ellipsis(
                    &item.label,
                    max_primary_width,
                    "",
                );
                let label_width = visible_width(&truncated_label);
                let spacing = " ".repeat(effective_primary_width - label_width);
                
                let description_start = prefix_width + label_width + spacing.len();
                let remaining_width = width as usize - description_start - 2;

                if remaining_width > MIN_DESCRIPTION_WIDTH {
                    let truncated_desc = truncate_to_width_with_ellipsis(
                        &detail_single_line,
                        remaining_width,
                        "",
                    );

                    format!("{}{}{}{}", prefix, truncated_label, spacing, truncated_desc)
                } else {
                    // 空间不足，只显示标签
                    let truncated = truncate_to_width_with_ellipsis(&item.label, available_width, "");
                    format!("{}{}", prefix, truncated)
                }
            } else {
                // 单列布局
                let truncated = truncate_to_width_with_ellipsis(&item.label, available_width, "");
                format!("{}{}", prefix, truncated)
            }
        } else {
            // 无描述，只显示标签
            let truncated = truncate_to_width_with_ellipsis(&item.label, available_width, "");
            format!("{}{}", prefix, truncated)
        }
    }

    /// 计算主列宽度
    fn calculate_primary_column_width(&self) -> usize {
        let widest = self
            .filtered_indices
            .iter()
            .filter_map(|&idx| self.items.get(idx))
            .map(|item| visible_width(&item.label) + PRIMARY_COLUMN_GAP)
            .max()
            .unwrap_or(0);

        widest.clamp(10, DEFAULT_PRIMARY_COLUMN_WIDTH)
    }
}

impl Component for SelectList {
    fn render(&self, width: u16) -> Vec<String> {
        let mut lines = Vec::new();

        // 如果没有匹配项，显示提示
        if self.filtered_indices.is_empty() {
            lines.push("  No matching items".to_string());
            return lines;
        }

        let primary_column_width = self.calculate_primary_column_width();

        // 计算可见范围
        let start_index = self.scroll_offset;
        let end_index = (start_index + self.max_visible).min(self.filtered_indices.len());

        // 渲染可见项
        for i in start_index..end_index {
            if let Some(&idx) = self.filtered_indices.get(i) {
                if let Some(item) = self.items.get(idx) {
                    let is_selected = i == self.selected_index;
                    lines.push(self.render_item(item, is_selected, width, primary_column_width));
                }
            }
        }

        // 添加滚动指示器
        if self.filtered_indices.len() > self.max_visible {
            let scroll_text = format!("  ({}/{})", self.selected_index + 1, self.filtered_indices.len());
            let truncated = truncate_to_width_with_ellipsis(&scroll_text, width as usize - 2, "");
            lines.push(truncated);
        }

        lines
    }

    fn handle_input(&mut self, data: &str) -> bool {
        match data {
            // 向下/Tab
            "\x1b[B" | "\t" => {
                self.select_next();
                true
            }
            // 向上/Shift+Tab
            "\x1b[A" | "\x1b[Z" => {
                self.select_prev();
                true
            }
            // Enter
            "\r" | "\n" => {
                self.confirm_selection();
                true
            }
            _ => false,
        }
    }

    fn invalidate(&mut self) {
        self.needs_render = true;
    }
}

impl Focusable for SelectList {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.needs_render = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_list_new() {
        let items = vec![
            SelectItem::new("1", "Item 1"),
            SelectItem::new("2", "Item 2"),
            SelectItem::new("3", "Item 3"),
        ];
        let list = SelectList::new(items);
        
        assert_eq!(list.total_count(), 3);
        assert_eq!(list.filtered_count(), 3);
        assert!(list.selected().is_some());
    }

    #[test]
    fn test_select_list_navigation() {
        let items = vec![
            SelectItem::new("1", "Item 1"),
            SelectItem::new("2", "Item 2"),
            SelectItem::new("3", "Item 3"),
        ];
        let mut list = SelectList::new(items);
        
        assert_eq!(list.selected().unwrap().value, "1");
        
        list.select_next();
        assert_eq!(list.selected().unwrap().value, "2");
        
        list.select_next();
        assert_eq!(list.selected().unwrap().value, "3");
        
        list.select_next();
        assert_eq!(list.selected().unwrap().value, "1"); // 循环
        
        list.select_prev();
        assert_eq!(list.selected().unwrap().value, "3");
    }

    #[test]
    fn test_select_list_filter() {
        let items = vec![
            SelectItem::new("apple", "Apple"),
            SelectItem::new("banana", "Banana"),
            SelectItem::new("apricot", "Apricot"),
        ];
        let mut list = SelectList::new(items);
        
        list.set_filter("ap");
        assert_eq!(list.filtered_count(), 2);
        
        list.set_filter("ban");
        assert_eq!(list.filtered_count(), 1);
        assert_eq!(list.selected().unwrap().value, "banana");
    }

    #[test]
    fn test_select_list_with_detail() {
        let item = SelectItem::with_detail("1", "Label", "This is a description");
        assert_eq!(item.label, "Label");
        assert_eq!(item.detail, Some("This is a description".to_string()));
    }

    #[test]
    fn test_select_list_render() {
        let items = vec![
            SelectItem::new("1", "Item 1"),
            SelectItem::new("2", "Item 2"),
        ];
        let list = SelectList::new(items);
        let lines = list.render(40);
        
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("Item 1"));
        assert!(lines[1].contains("Item 2"));
    }

    #[test]
    fn test_select_list_callback() {
        let items = vec![
            SelectItem::new("1", "Item 1"),
            SelectItem::new("2", "Item 2"),
        ];
        let mut list = SelectList::new(items);
        
        let selected = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let selected_clone = selected.clone();
        
        list.on_select(Box::new(move |item| {
            *selected_clone.lock().unwrap() = item.value.clone();
        }));
        
        list.confirm_selection();
        assert_eq!(*selected.lock().unwrap(), "1");
    }
}
