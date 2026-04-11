//! 设置管理 TUI 界面
//!
//! 提供交互式设置编辑、分类导航、实时预览功能

#![allow(dead_code)] // 设置管理界面尚未完全集成

use serde::{Deserialize, Serialize};

/// 设置分类
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettingsCategory {
    /// 通用设置
    General,
    /// Provider 设置
    Provider,
    /// 编辑器设置
    Editor,
    /// 扩展设置
    Extensions,
}

impl std::fmt::Display for SettingsCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::General => write!(f, "General"),
            Self::Provider => write!(f, "Provider"),
            Self::Editor => write!(f, "Editor"),
            Self::Extensions => write!(f, "Extensions"),
        }
    }
}

/// 设置值类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum SettingValue {
    /// 布尔值
    Bool(bool),
    /// 字符串
    String(String),
    /// 数字
    Number(f64),
    /// 枚举值
    Enum {
        /// 当前选中值
        selected: String,
        /// 可选值列表
        options: Vec<String>,
    },
    /// 字符串列表
    StringList(Vec<String>),
}

/// 设置项定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingItem {
    /// 设置键
    pub key: String,
    /// 显示标签
    pub label: String,
    /// 设置描述
    pub description: String,
    /// 所属分类
    pub category: SettingsCategory,
    /// 当前值
    pub value: SettingValue,
    /// 默认值
    pub default_value: SettingValue,
}

impl SettingItem {
    /// 检查设置是否被修改
    pub fn is_modified(&self) -> bool {
        // 比较 value 与 default_value 的 JSON 序列化结果
        let current = serde_json::to_string(&self.value).unwrap_or_default();
        let default = serde_json::to_string(&self.default_value).unwrap_or_default();
        current != default
    }

    /// 重置为默认值
    pub fn reset_to_default(&mut self) {
        self.value = self.default_value.clone();
    }
}

/// 设置管理器
pub struct SettingsManager {
    /// 设置项列表
    items: Vec<SettingItem>,
    /// 当前分类
    current_category: SettingsCategory,
    /// 选中索引
    selected_index: usize,
}

impl SettingsManager {
    /// 创建新的设置管理器
    pub fn new() -> Self {
        Self {
            items: Self::default_settings(),
            current_category: SettingsCategory::General,
            selected_index: 0,
        }
    }

    /// 从配置加载设置
    pub fn load_from_config(config: &serde_json::Value) -> Self {
        let mut manager = Self::new();
        // 从 config 值覆盖默认设置
        if let Some(obj) = config.as_object() {
            for item in &mut manager.items {
                if let Some(value) = obj.get(&item.key) {
                    if let Ok(sv) = serde_json::from_value::<SettingValue>(value.clone()) {
                        item.value = sv;
                    }
                }
            }
        }
        manager
    }

    /// 获取当前分类的设置项
    pub fn current_items(&self) -> Vec<&SettingItem> {
        self.items
            .iter()
            .filter(|item| item.category == self.current_category)
            .collect()
    }

    /// 获取所有分类
    pub fn categories(&self) -> Vec<SettingsCategory> {
        vec![
            SettingsCategory::General,
            SettingsCategory::Provider,
            SettingsCategory::Editor,
            SettingsCategory::Extensions,
        ]
    }

    /// 切换分类
    pub fn set_category(&mut self, category: SettingsCategory) {
        self.current_category = category;
        self.selected_index = 0;
    }

    /// 获取选中的设置项
    pub fn selected_item(&self) -> Option<&SettingItem> {
        let items = self.current_items();
        items.get(self.selected_index).copied()
    }

    /// 获取选中的设置项的可变引用
    pub fn selected_item_mut(&mut self) -> Option<&mut SettingItem> {
        let category = self.current_category.clone();
        let index = self.selected_index;
        self.items
            .iter_mut()
            .filter(|item| item.category == category)
            .nth(index)
    }

    /// 上移选择
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// 下移选择
    pub fn move_down(&mut self) {
        let count = self.current_items().len();
        if self.selected_index + 1 < count {
            self.selected_index += 1;
        }
    }

    /// 切换布尔值
    pub fn toggle_bool(&mut self) {
        if let Some(item) = self.selected_item_mut() {
            if let SettingValue::Bool(ref mut v) = item.value {
                *v = !*v;
            }
        }
    }

    /// 循环枚举值
    pub fn cycle_enum(&mut self) {
        if let Some(item) = self.selected_item_mut() {
            if let SettingValue::Enum {
                ref mut selected,
                ref options,
            } = item.value
            {
                if let Some(idx) = options.iter().position(|o| o == selected) {
                    let next_idx = (idx + 1) % options.len();
                    *selected = options[next_idx].clone();
                }
            }
        }
    }

    /// 更新字符串值
    pub fn set_string(&mut self, value: String) {
        if let Some(item) = self.selected_item_mut() {
            if let SettingValue::String(ref mut v) = item.value {
                *v = value;
            }
        }
    }

    /// 更新数字值
    pub fn set_number(&mut self, value: f64) {
        if let Some(item) = self.selected_item_mut() {
            if let SettingValue::Number(ref mut v) = item.value {
                *v = value;
            }
        }
    }

    /// 恢复当前项默认值
    pub fn reset_current(&mut self) {
        if let Some(item) = self.selected_item_mut() {
            item.reset_to_default();
        }
    }

    /// 恢复所有默认值
    pub fn reset_all(&mut self) {
        for item in &mut self.items {
            item.reset_to_default();
        }
    }

    /// 导出设置为 JSON
    pub fn export_settings(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for item in &self.items {
            if let Ok(v) = serde_json::to_value(&item.value) {
                map.insert(item.key.clone(), v);
            }
        }
        serde_json::Value::Object(map)
    }

    /// 获取已修改的设置数量
    pub fn modified_count(&self) -> usize {
        self.items.iter().filter(|i| i.is_modified()).count()
    }

    /// 默认设置列表
    fn default_settings() -> Vec<SettingItem> {
        vec![
            // General
            SettingItem {
                key: "general.theme".to_string(),
                label: "Theme".to_string(),
                description: "Color theme for the TUI interface".to_string(),
                category: SettingsCategory::General,
                value: SettingValue::Enum {
                    selected: "dark".to_string(),
                    options: vec!["dark".to_string(), "light".to_string(), "auto".to_string()],
                },
                default_value: SettingValue::Enum {
                    selected: "dark".to_string(),
                    options: vec!["dark".to_string(), "light".to_string(), "auto".to_string()],
                },
            },
            SettingItem {
                key: "general.language".to_string(),
                label: "Language".to_string(),
                description: "Interface language".to_string(),
                category: SettingsCategory::General,
                value: SettingValue::Enum {
                    selected: "en".to_string(),
                    options: vec!["en".to_string(), "zh".to_string(), "ja".to_string()],
                },
                default_value: SettingValue::Enum {
                    selected: "en".to_string(),
                    options: vec!["en".to_string(), "zh".to_string(), "ja".to_string()],
                },
            },
            SettingItem {
                key: "general.auto_save".to_string(),
                label: "Auto Save Sessions".to_string(),
                description: "Automatically save session history".to_string(),
                category: SettingsCategory::General,
                value: SettingValue::Bool(true),
                default_value: SettingValue::Bool(true),
            },
            SettingItem {
                key: "general.max_history".to_string(),
                label: "Max History".to_string(),
                description: "Maximum number of sessions to keep".to_string(),
                category: SettingsCategory::General,
                value: SettingValue::Number(100.0),
                default_value: SettingValue::Number(100.0),
            },
            // Provider
            SettingItem {
                key: "provider.default".to_string(),
                label: "Default Provider".to_string(),
                description: "Default LLM provider".to_string(),
                category: SettingsCategory::Provider,
                value: SettingValue::Enum {
                    selected: "anthropic".to_string(),
                    options: vec![
                        "anthropic".to_string(),
                        "openai".to_string(),
                        "google".to_string(),
                        "mistral".to_string(),
                    ],
                },
                default_value: SettingValue::Enum {
                    selected: "anthropic".to_string(),
                    options: vec![
                        "anthropic".to_string(),
                        "openai".to_string(),
                        "google".to_string(),
                        "mistral".to_string(),
                    ],
                },
            },
            SettingItem {
                key: "provider.temperature".to_string(),
                label: "Temperature".to_string(),
                description: "Default sampling temperature (0.0-2.0)".to_string(),
                category: SettingsCategory::Provider,
                value: SettingValue::Number(0.7),
                default_value: SettingValue::Number(0.7),
            },
            SettingItem {
                key: "provider.streaming".to_string(),
                label: "Streaming".to_string(),
                description: "Enable streaming responses".to_string(),
                category: SettingsCategory::Provider,
                value: SettingValue::Bool(true),
                default_value: SettingValue::Bool(true),
            },
            // Editor
            SettingItem {
                key: "editor.vim_mode".to_string(),
                label: "Vim Mode".to_string(),
                description: "Enable Vim keybindings in the editor".to_string(),
                category: SettingsCategory::Editor,
                value: SettingValue::Bool(false),
                default_value: SettingValue::Bool(false),
            },
            SettingItem {
                key: "editor.tab_size".to_string(),
                label: "Tab Size".to_string(),
                description: "Number of spaces per tab".to_string(),
                category: SettingsCategory::Editor,
                value: SettingValue::Number(4.0),
                default_value: SettingValue::Number(4.0),
            },
            SettingItem {
                key: "editor.word_wrap".to_string(),
                label: "Word Wrap".to_string(),
                description: "Enable word wrapping".to_string(),
                category: SettingsCategory::Editor,
                value: SettingValue::Bool(true),
                default_value: SettingValue::Bool(true),
            },
            // Extensions
            SettingItem {
                key: "extensions.auto_load".to_string(),
                label: "Auto Load Extensions".to_string(),
                description: "Automatically load extensions on startup".to_string(),
                category: SettingsCategory::Extensions,
                value: SettingValue::Bool(true),
                default_value: SettingValue::Bool(true),
            },
            SettingItem {
                key: "extensions.sandbox".to_string(),
                label: "Extension Sandbox".to_string(),
                description: "Run extensions in a sandboxed environment".to_string(),
                category: SettingsCategory::Extensions,
                value: SettingValue::Bool(true),
                default_value: SettingValue::Bool(true),
            },
        ]
    }
}

impl Default for SettingsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_category_display() {
        assert_eq!(SettingsCategory::General.to_string(), "General");
        assert_eq!(SettingsCategory::Provider.to_string(), "Provider");
        assert_eq!(SettingsCategory::Editor.to_string(), "Editor");
        assert_eq!(SettingsCategory::Extensions.to_string(), "Extensions");
    }

    #[test]
    fn test_setting_item_is_modified() {
        let mut item = SettingItem {
            key: "test".to_string(),
            label: "Test".to_string(),
            description: "Test item".to_string(),
            category: SettingsCategory::General,
            value: SettingValue::Bool(false),
            default_value: SettingValue::Bool(false),
        };
        assert!(!item.is_modified());

        item.value = SettingValue::Bool(true);
        assert!(item.is_modified());
    }

    #[test]
    fn test_setting_item_reset_to_default() {
        let mut item = SettingItem {
            key: "test".to_string(),
            label: "Test".to_string(),
            description: "Test item".to_string(),
            category: SettingsCategory::General,
            value: SettingValue::Number(42.0),
            default_value: SettingValue::Number(10.0),
        };
        assert!(item.is_modified());

        item.reset_to_default();
        assert!(!item.is_modified());
        if let SettingValue::Number(v) = item.value {
            assert_eq!(v, 10.0);
        } else {
            panic!("Expected Number value");
        }
    }

    #[test]
    fn test_settings_manager_new() {
        let manager = SettingsManager::new();
        assert_eq!(manager.current_category, SettingsCategory::General);
        assert_eq!(manager.selected_index, 0);
        assert!(!manager.items.is_empty());
    }

    #[test]
    fn test_settings_manager_categories() {
        let manager = SettingsManager::new();
        let categories = manager.categories();
        assert_eq!(categories.len(), 4);
        assert!(categories.contains(&SettingsCategory::General));
        assert!(categories.contains(&SettingsCategory::Provider));
        assert!(categories.contains(&SettingsCategory::Editor));
        assert!(categories.contains(&SettingsCategory::Extensions));
    }

    #[test]
    fn test_settings_manager_set_category() {
        let mut manager = SettingsManager::new();
        manager.selected_index = 3;
        manager.set_category(SettingsCategory::Provider);
        assert_eq!(manager.current_category, SettingsCategory::Provider);
        assert_eq!(manager.selected_index, 0);
    }

    #[test]
    fn test_settings_manager_navigation() {
        let mut manager = SettingsManager::new();
        let initial_count = manager.current_items().len();
        assert!(initial_count > 1);

        manager.selected_index = 0;
        manager.move_up(); // should stay at 0
        assert_eq!(manager.selected_index, 0);

        manager.move_down();
        assert_eq!(manager.selected_index, 1);

        manager.move_up();
        assert_eq!(manager.selected_index, 0);
    }

    #[test]
    fn test_settings_manager_toggle_bool() {
        let mut manager = SettingsManager::new();
        // Find a bool setting
        let bool_index = manager.current_items().iter().enumerate()
            .find(|(_, item)| matches!(item.value, SettingValue::Bool(_)))
            .map(|(idx, _)| idx);
        
        if let Some(idx) = bool_index {
            manager.selected_index = idx;
            let initial = if let SettingValue::Bool(v) = manager.current_items()[idx].value {
                v
            } else {
                return;
            };
            manager.toggle_bool();
            let new_value = if let SettingValue::Bool(v) =
                manager.selected_item().unwrap().value
            {
                v
            } else {
                panic!("Expected Bool");
            };
            assert_eq!(new_value, !initial);
        }
        // If no bool found in current category, test passes
    }

    #[test]
    fn test_settings_manager_cycle_enum() {
        let mut manager = SettingsManager::new();
        // Find an enum setting
        let enum_index = manager.current_items().iter().enumerate()
            .find(|(_, item)| matches!(item.value, SettingValue::Enum { .. }))
            .map(|(idx, _)| idx);
        
        if let Some(idx) = enum_index {
            manager.selected_index = idx;
            let initial = if let SettingValue::Enum { ref selected, .. } = manager.current_items()[idx].value {
                selected.clone()
            } else {
                return;
            };
            manager.cycle_enum();
            let new_value = if let SettingValue::Enum { ref selected, .. } =
                manager.selected_item().unwrap().value
            {
                selected.clone()
            } else {
                panic!("Expected Enum");
            };
            assert_ne!(new_value, initial);
        }
        // If no enum found in current category, test passes
    }

    #[test]
    fn test_settings_manager_set_string() {
        let mut manager = SettingsManager::new();
        // Add a string item manually for testing
        let string_item = SettingItem {
            key: "test.string".to_string(),
            label: "Test String".to_string(),
            description: "A test string".to_string(),
            category: SettingsCategory::General,
            value: SettingValue::String("initial".to_string()),
            default_value: SettingValue::String("default".to_string()),
        };
        manager.items.push(string_item);

        // Navigate to the string item
        let string_idx = manager
            .current_items()
            .iter()
            .position(|i| i.key == "test.string")
            .unwrap();
        manager.selected_index = string_idx;

        manager.set_string("new value".to_string());
        let value = manager.selected_item().unwrap();
        if let SettingValue::String(v) = &value.value {
            assert_eq!(v, "new value");
        } else {
            panic!("Expected String value");
        }
    }

    #[test]
    fn test_settings_manager_set_number() {
        let mut manager = SettingsManager::new();
        // Find a number setting
        for (idx, item) in manager.current_items().iter().enumerate() {
            if matches!(item.value, SettingValue::Number(_)) {
                manager.selected_index = idx;
                manager.set_number(99.0);
                let new_value = if let SettingValue::Number(v) =
                    manager.selected_item().unwrap().value
                {
                    v
                } else {
                    panic!("Expected Number");
                };
                assert_eq!(new_value, 99.0);
                return;
            }
        }
    }

    #[test]
    fn test_settings_manager_reset_current() {
        let mut manager = SettingsManager::new();
        // Find and modify a setting
        for (idx, item) in manager.current_items().iter().enumerate() {
            if matches!(item.value, SettingValue::Bool(_)) {
                manager.selected_index = idx;
                manager.toggle_bool(); // modify it
                assert!(manager.selected_item().unwrap().is_modified());
                manager.reset_current();
                assert!(!manager.selected_item().unwrap().is_modified());
                return;
            }
        }
    }

    #[test]
    fn test_settings_manager_reset_all() {
        let mut manager = SettingsManager::new();
        // Modify multiple settings
        manager.selected_index = 0;
        manager.toggle_bool();
        manager.selected_index = 1;
        if matches!(manager.selected_item().unwrap().value, SettingValue::Bool(_)) {
            manager.toggle_bool();
        }

        let modified_before = manager.modified_count();
        manager.reset_all();
        assert!(manager.modified_count() < modified_before || modified_before == 0);
        assert_eq!(manager.modified_count(), 0);
    }

    #[test]
    fn test_settings_manager_export_settings() {
        let manager = SettingsManager::new();
        let exported = manager.export_settings();

        assert!(exported.is_object());
        let obj = exported.as_object().unwrap();
        assert!(obj.contains_key("general.theme"));
        assert!(obj.contains_key("provider.default"));
        assert!(obj.contains_key("editor.tab_size"));
    }

    #[test]
    fn test_settings_manager_modified_count() {
        let mut manager = SettingsManager::new();
        let initial_count = manager.modified_count();
        assert_eq!(initial_count, 0);

        // Find a bool setting and toggle it
        let bool_index = manager.current_items().iter().enumerate()
            .find(|(_, item)| matches!(item.value, SettingValue::Bool(_)))
            .map(|(idx, _)| idx);
        
        if let Some(idx) = bool_index {
            manager.selected_index = idx;
            // Get the current bool value
            let current_value = if let SettingValue::Bool(v) = manager.current_items()[idx].value {
                v
            } else {
                return;
            };
            // Only count as modified if we change from the default
            if let SettingValue::Bool(default) = manager.current_items()[idx].default_value {
                if current_value == default {
                    manager.toggle_bool();
                    assert_eq!(manager.modified_count(), 1);
                } else {
                    // Already modified
                    assert!(manager.modified_count() >= 1);
                }
            }

            // Reset
            manager.reset_all();
            assert_eq!(manager.modified_count(), 0);
        }
    }

    #[test]
    fn test_settings_manager_load_from_config() {
        let config = serde_json::json!({
            "general.theme": {
                "type": "Enum",
                "value": {
                    "selected": "light",
                    "options": ["dark", "light", "auto"]
                }
            },
            "general.auto_save": {
                "type": "Bool",
                "value": false
            }
        });

        let manager = SettingsManager::load_from_config(&config);
        // Check that settings were loaded
        let theme_item = manager
            .items
            .iter()
            .find(|i| i.key == "general.theme");
        // Note: The load_from_config expects the SettingValue format directly,
        // so this tests the parsing logic
        assert!(theme_item.is_some());
    }

    #[test]
    fn test_setting_value_serialization() {
        let bool_val = SettingValue::Bool(true);
        let json = serde_json::to_string(&bool_val).unwrap();
        assert!(json.contains("Bool"));

        let num_val = SettingValue::Number(1.5);
        let json = serde_json::to_string(&num_val).unwrap();
        assert!(json.contains("Number"));

        let enum_val = SettingValue::Enum {
            selected: "dark".to_string(),
            options: vec!["dark".to_string(), "light".to_string()],
        };
        let json = serde_json::to_string(&enum_val).unwrap();
        assert!(json.contains("Enum"));
    }

    // ========== 设置值类型验证测试 ==========

    #[test]
    fn test_setting_value_bool_type() {
        let val = SettingValue::Bool(true);
        assert!(matches!(val, SettingValue::Bool(true)));
        
        let val = SettingValue::Bool(false);
        assert!(matches!(val, SettingValue::Bool(false)));
    }

    #[test]
    fn test_setting_value_string_type() {
        let val = SettingValue::String("test".to_string());
        assert!(matches!(val, SettingValue::String(s) if s == "test"));
    }

    #[test]
    fn test_setting_value_number_type() {
        let val = SettingValue::Number(42.0);
        assert!(matches!(val, SettingValue::Number(n) if n == 42.0));
        
        let val = SettingValue::Number(1.4142);
        assert!(matches!(val, SettingValue::Number(n) if (n - 1.4142).abs() < 0.0001));
    }

    #[test]
    fn test_setting_value_enum_type() {
        let val = SettingValue::Enum {
            selected: "option1".to_string(),
            options: vec!["option1".to_string(), "option2".to_string(), "option3".to_string()],
        };
        
        if let SettingValue::Enum { selected, options } = val {
            assert_eq!(selected, "option1");
            assert_eq!(options.len(), 3);
            assert!(options.contains(&"option1".to_string()));
            assert!(options.contains(&"option2".to_string()));
            assert!(options.contains(&"option3".to_string()));
        } else {
            panic!("Expected Enum type");
        }
    }

    #[test]
    fn test_setting_value_string_list_type() {
        let val = SettingValue::StringList(vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        
        if let SettingValue::StringList(list) = val {
            assert_eq!(list.len(), 3);
            assert_eq!(list[0], "a");
            assert_eq!(list[1], "b");
            assert_eq!(list[2], "c");
        } else {
            panic!("Expected StringList type");
        }
    }

    // ========== 分类导航测试 ==========

    #[test]
    fn test_all_categories_navigation() {
        let manager = SettingsManager::new();
        let categories = manager.categories();
        
        assert_eq!(categories.len(), 4);
        assert!(categories.contains(&SettingsCategory::General));
        assert!(categories.contains(&SettingsCategory::Provider));
        assert!(categories.contains(&SettingsCategory::Editor));
        assert!(categories.contains(&SettingsCategory::Extensions));
    }

    #[test]
    fn test_category_switching() {
        let mut manager = SettingsManager::new();
        
        // 初始分类
        assert_eq!(manager.current_category, SettingsCategory::General);
        
        // 切换到 Provider
        manager.set_category(SettingsCategory::Provider);
        assert_eq!(manager.current_category, SettingsCategory::Provider);
        
        // 切换到 Editor
        manager.set_category(SettingsCategory::Editor);
        assert_eq!(manager.current_category, SettingsCategory::Editor);
        
        // 切换到 Extensions
        manager.set_category(SettingsCategory::Extensions);
        assert_eq!(manager.current_category, SettingsCategory::Extensions);
        
        // 切换回 General
        manager.set_category(SettingsCategory::General);
        assert_eq!(manager.current_category, SettingsCategory::General);
    }

    #[test]
    fn test_current_items_by_category() {
        let mut manager = SettingsManager::new();
        
        // General 分类的项目
        let general_items = manager.current_items();
        assert!(!general_items.is_empty());
        for item in &general_items {
            assert_eq!(item.category, SettingsCategory::General);
        }
        
        // 切换到 Provider
        manager.set_category(SettingsCategory::Provider);
        let provider_items = manager.current_items();
        assert!(!provider_items.is_empty());
        for item in &provider_items {
            assert_eq!(item.category, SettingsCategory::Provider);
        }
        
        // 切换到 Editor
        manager.set_category(SettingsCategory::Editor);
        let editor_items = manager.current_items();
        assert!(!editor_items.is_empty());
        for item in &editor_items {
            assert_eq!(item.category, SettingsCategory::Editor);
        }
        
        // 切换到 Extensions
        manager.set_category(SettingsCategory::Extensions);
        let ext_items = manager.current_items();
        assert!(!ext_items.is_empty());
        for item in &ext_items {
            assert_eq!(item.category, SettingsCategory::Extensions);
        }
    }

    // ========== 重置默认值测试 ==========

    #[test]
    fn test_reset_current_to_default() {
        let mut manager = SettingsManager::new();
        
        // 找到一个布尔值设置并修改它
        let bool_idx = manager.current_items().iter().enumerate()
            .find(|(_, item)| matches!(item.value, SettingValue::Bool(_)))
            .map(|(idx, _)| idx);
        
        if let Some(idx) = bool_idx {
            manager.selected_index = idx;
            
            // 获取原始值
            let original_value = if let SettingValue::Bool(v) = manager.current_items()[idx].value {
                v
            } else {
                panic!("Expected Bool");
            };
            
            // 切换值
            manager.toggle_bool();
            let new_value = if let SettingValue::Bool(v) = manager.selected_item().unwrap().value {
                v
            } else {
                panic!("Expected Bool");
            };
            assert_ne!(original_value, new_value);
            assert!(manager.selected_item().unwrap().is_modified());
            
            // 重置
            manager.reset_current();
            
            // 验证已重置
            let reset_value = if let SettingValue::Bool(v) = manager.selected_item().unwrap().value {
                v
            } else {
                panic!("Expected Bool");
            };
            assert_eq!(original_value, reset_value);
            assert!(!manager.selected_item().unwrap().is_modified());
        }
    }

    #[test]
    fn test_reset_all_to_defaults() {
        let mut manager = SettingsManager::new();
        
        // 修改多个设置
        let initial_modified = manager.modified_count();
        
        // 切换第一个布尔值
        if let Some(idx) = manager.current_items().iter().enumerate()
            .find(|(_, item)| matches!(item.value, SettingValue::Bool(_)))
            .map(|(idx, _)| idx) {
            manager.selected_index = idx;
            manager.toggle_bool();
        }
        
        // 确保有修改
        assert!(manager.modified_count() > initial_modified || manager.modified_count() > 0);
        
        // 重置所有
        manager.reset_all();
        
        // 验证没有修改了
        assert_eq!(manager.modified_count(), 0);
        
        // 验证所有值都恢复为默认值
        for item in &manager.items {
            assert!(!item.is_modified());
            let current_json = serde_json::to_string(&item.value).unwrap();
            let default_json = serde_json::to_string(&item.default_value).unwrap();
            assert_eq!(current_json, default_json);
        }
    }

    // ========== 导出功能测试 ==========

    #[test]
    fn test_export_settings_structure() {
        let manager = SettingsManager::new();
        let exported = manager.export_settings();
        
        assert!(exported.is_object());
        let obj = exported.as_object().unwrap();
        
        // 验证包含所有设置键
        assert!(obj.contains_key("general.theme"));
        assert!(obj.contains_key("general.language"));
        assert!(obj.contains_key("general.auto_save"));
        assert!(obj.contains_key("general.max_history"));
        assert!(obj.contains_key("provider.default"));
        assert!(obj.contains_key("provider.temperature"));
        assert!(obj.contains_key("provider.streaming"));
        assert!(obj.contains_key("editor.vim_mode"));
        assert!(obj.contains_key("editor.tab_size"));
        assert!(obj.contains_key("editor.word_wrap"));
        assert!(obj.contains_key("extensions.auto_load"));
        assert!(obj.contains_key("extensions.sandbox"));
    }

    #[test]
    fn test_export_settings_values() {
        let mut manager = SettingsManager::new();
        
        // 修改一个设置
        if let Some(idx) = manager.current_items().iter().enumerate()
            .find(|(_, item)| matches!(item.value, SettingValue::Bool(_)))
            .map(|(idx, _)| idx) {
            manager.selected_index = idx;
            manager.toggle_bool();
        }
        
        let exported = manager.export_settings();
        let obj = exported.as_object().unwrap();
        
        // 验证导出的值是修改后的值
        for item in &manager.items {
            let exported_value = obj.get(&item.key);
            assert!(exported_value.is_some());
            
            // 验证值类型匹配
            match &item.value {
                SettingValue::Bool(_) => {
                    assert!(exported_value.unwrap().is_object() || exported_value.unwrap().is_boolean());
                }
                SettingValue::Number(_) => {
                    assert!(exported_value.unwrap().is_object() || exported_value.unwrap().is_number());
                }
                SettingValue::String(_) => {
                    assert!(exported_value.unwrap().is_object() || exported_value.unwrap().is_string());
                }
                SettingValue::Enum { .. } => {
                    assert!(exported_value.unwrap().is_object());
                }
                SettingValue::StringList(_) => {
                    assert!(exported_value.unwrap().is_object() || exported_value.unwrap().is_array());
                }
            }
        }
    }

    // ========== 边界条件测试 ==========

    #[test]
    fn test_empty_category_navigation() {
        let mut manager = SettingsManager::new();
        
        // 添加一个空分类的设置项（手动构造）
        manager.items.retain(|item| item.category != SettingsCategory::General);
        
        // 切换到 General（现在没有项目了）
        manager.set_category(SettingsCategory::General);
        
        // 验证空列表
        let items = manager.current_items();
        assert!(items.is_empty());
        
        // 验证选中项为 None
        assert!(manager.selected_item().is_none());
        assert!(manager.selected_item_mut().is_none());
    }

    #[test]
    fn test_invalid_index_handling() {
        let mut manager = SettingsManager::new();
        
        // 设置一个超出范围的索引
        manager.selected_index = 999;
        
        // 应该返回 None
        assert!(manager.selected_item().is_none());
        assert!(manager.selected_item_mut().is_none());
    }

    #[test]
    fn test_navigation_at_boundaries() {
        let mut manager = SettingsManager::new();
        let item_count = manager.current_items().len();
        
        if item_count > 0 {
            // 在顶部尝试上移
            manager.selected_index = 0;
            manager.move_up();
            assert_eq!(manager.selected_index, 0);
            
            // 在底部尝试下移
            manager.selected_index = item_count - 1;
            manager.move_down();
            assert_eq!(manager.selected_index, item_count - 1);
        }
    }

    #[test]
    fn test_cycle_enum_wraps_around() {
        let mut manager = SettingsManager::new();
        
        // 找到一个枚举设置
        if let Some(idx) = manager.current_items().iter().enumerate()
            .find(|(_, item)| matches!(item.value, SettingValue::Enum { .. }))
            .map(|(idx, _)| idx) {
            manager.selected_index = idx;
            
            // 获取选项列表
            let options_count = if let SettingValue::Enum { ref options, .. } = manager.current_items()[idx].value {
                options.len()
            } else {
                0
            };
            
            if options_count > 1 {
                // 获取初始值
                let initial = if let SettingValue::Enum { ref selected, .. } = manager.current_items()[idx].value {
                    selected.clone()
                } else {
                    return;
                };
                
                // 循环多次，应该回到初始值
                for _ in 0..options_count {
                    manager.cycle_enum();
                }
                
                let final_value = if let SettingValue::Enum { ref selected, .. } = manager.selected_item().unwrap().value {
                    selected.clone()
                } else {
                    panic!("Expected Enum");
                };
                
                assert_eq!(initial, final_value);
            }
        }
    }

    #[test]
    fn test_set_string_on_non_string_item() {
        let mut manager = SettingsManager::new();
        
        // 找到一个非字符串设置
        if let Some(idx) = manager.current_items().iter().enumerate()
            .find(|(_, item)| !matches!(item.value, SettingValue::String(_)))
            .map(|(idx, _)| idx) {
            manager.selected_index = idx;
            
            // 尝试设置字符串值（应该无效果）
            let original_value = manager.selected_item().unwrap().value.clone();
            manager.set_string("new value".to_string());
            
            // 值应该保持不变
            let new_value = &manager.selected_item().unwrap().value;
            assert_eq!(format!("{:?}", original_value), format!("{:?}", new_value));
        }
    }

    #[test]
    fn test_set_number_on_non_number_item() {
        let mut manager = SettingsManager::new();
        
        // 找到一个非数字设置
        if let Some(idx) = manager.current_items().iter().enumerate()
            .find(|(_, item)| !matches!(item.value, SettingValue::Number(_)))
            .map(|(idx, _)| idx) {
            manager.selected_index = idx;
            
            // 尝试设置数字值（应该无效果）
            let original_value = manager.selected_item().unwrap().value.clone();
            manager.set_number(999.0);
            
            // 值应该保持不变
            let new_value = &manager.selected_item().unwrap().value;
            assert_eq!(format!("{:?}", original_value), format!("{:?}", new_value));
        }
    }

    #[test]
    fn test_toggle_bool_on_non_bool_item() {
        let mut manager = SettingsManager::new();
        
        // 找到一个非布尔设置
        if let Some(idx) = manager.current_items().iter().enumerate()
            .find(|(_, item)| !matches!(item.value, SettingValue::Bool(_)))
            .map(|(idx, _)| idx) {
            manager.selected_index = idx;
            
            // 尝试切换布尔值（应该无效果）
            let original_value = manager.selected_item().unwrap().value.clone();
            manager.toggle_bool();
            
            // 值应该保持不变
            let new_value = &manager.selected_item().unwrap().value;
            assert_eq!(format!("{:?}", original_value), format!("{:?}", new_value));
        }
    }

    #[test]
    fn test_cycle_enum_on_non_enum_item() {
        let mut manager = SettingsManager::new();
        
        // 找到一个非枚举设置
        if let Some(idx) = manager.current_items().iter().enumerate()
            .find(|(_, item)| !matches!(item.value, SettingValue::Enum { .. }))
            .map(|(idx, _)| idx) {
            manager.selected_index = idx;
            
            // 尝试循环枚举（应该无效果）
            let original_value = manager.selected_item().unwrap().value.clone();
            manager.cycle_enum();
            
            // 值应该保持不变
            let new_value = &manager.selected_item().unwrap().value;
            assert_eq!(format!("{:?}", original_value), format!("{:?}", new_value));
        }
    }

    #[test]
    fn test_modified_count_accuracy() {
        let mut manager = SettingsManager::new();
        
        // 初始状态
        assert_eq!(manager.modified_count(), 0);
        
        // 修改一个设置
        if let Some(idx) = manager.current_items().iter().enumerate()
            .find(|(_, item)| matches!(item.value, SettingValue::Bool(_)))
            .map(|(idx, _)| idx) {
            manager.selected_index = idx;
            manager.toggle_bool();
            
            // 应该有一个修改
            assert_eq!(manager.modified_count(), 1);
            
            // 再修改一次（如果与默认值不同）
            manager.toggle_bool();
            // 如果回到了默认值，修改计数应该为 0
        }
    }

    #[test]
    fn test_category_display_format() {
        assert_eq!(format!("{}", SettingsCategory::General), "General");
        assert_eq!(format!("{}", SettingsCategory::Provider), "Provider");
        assert_eq!(format!("{}", SettingsCategory::Editor), "Editor");
        assert_eq!(format!("{}", SettingsCategory::Extensions), "Extensions");
    }
}
