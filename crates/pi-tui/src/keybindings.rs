//! 快捷键管理模块
//
//! 提供快捷键定义、冲突检测和全局管理功能

use crate::keys::{matches_key, Key};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock, OnceLock};

// =============================================================================
// 类型定义
// =============================================================================

/// 快捷键定义
#[derive(Debug, Clone)]
pub struct KeybindingDefinition {
    /// 按键描述（如 "ctrl+c", "escape", "enter"）
    pub key: String,
    /// 操作名称（如 "cancel", "submit", "newline"）
    pub action: String,
    /// 描述
    pub description: String,
    /// 可选的上下文限定
    pub context: Option<String>,
}

impl KeybindingDefinition {
    /// 创建新的快捷键定义
    pub fn new(key: impl Into<String>, action: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            action: action.into(),
            description: description.into(),
            context: None,
        }
    }

    /// 添加上下文
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

/// 快捷键定义配置（内部使用）
/// 
/// 包含完整的快捷键定义列表
#[derive(Debug, Clone, Default)]
pub struct KeybindingsDefinitionConfig {
    /// 快捷键绑定列表
    pub bindings: Vec<KeybindingDefinition>,
}

impl KeybindingsDefinitionConfig {
    /// 创建新的空配置
    pub fn new() -> Self {
        Self { bindings: Vec::new() }
    }

    /// 添加绑定
    pub fn add(&mut self, definition: KeybindingDefinition) {
        self.bindings.push(definition);
    }

    /// 从默认配置创建
    pub fn default_bindings() -> Self {
        default_keybindings()
    }
}

/// 快捷键配置（用于 TOML 持久化）
/// 
/// 简化的配置格式，用于存储到 ~/.pi/keybindings.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeybindingsConfig {
    /// 按键到操作的映射 ("ctrl+c" -> "cancel")
    #[serde(default)]
    pub bindings: HashMap<String, String>,
    /// 当前使用的预设名
    #[serde(default)]
    pub preset: Option<String>,
}

impl KeybindingsConfig {
    /// 从 TOML 文件加载配置
    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse keybindings config: {}", e))?;
        Ok(config)
    }

    /// 保存配置到 TOML 文件
    pub fn save_to_file(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize keybindings config: {}", e))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// 验证配置格式，返回警告列表
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        
        // 检查空按键
        for (key, action) in &self.bindings {
            if key.is_empty() {
                warnings.push(format!("Empty key for action '{}'", action));
            }
            if action.is_empty() {
                warnings.push(format!("Empty action for key '{}'", key));
            }
            // 检查按键格式是否合法
            if !Self::is_valid_key_format(key) {
                warnings.push(format!("Potentially invalid key format: '{}'", key));
            }
        }
        
        warnings
    }

    /// 检查按键格式是否合法
    fn is_valid_key_format(key: &str) -> bool {
        // 允许的修饰键前缀
        let valid_modifiers = ["ctrl+", "alt+", "shift+", "meta+", "super+"];
        
        // 单个按键或带修饰键的格式
        let key_lower = key.to_lowercase();
        
        // 检查是否是特殊按键
        let special_keys = [
            "enter", "escape", "tab", "backspace", "delete", "insert",
            "home", "end", "pageup", "pagedown", "space",
            "up", "down", "left", "right",
            "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
        ];
        
        // 解析修饰键
        let mut remaining = key_lower.as_str();
        let mut has_modifier = false;
        
        loop {
            let mut found = false;
            for mod_key in &valid_modifiers {
                if remaining.starts_with(mod_key) {
                    remaining = &remaining[mod_key.len()..];
                    has_modifier = true;
                    found = true;
                    break;
                }
            }
            if !found {
                break;
            }
        }
        
        // 检查剩余部分
        if special_keys.contains(&remaining) {
            return true;
        }
        
        // 单个字符
        if remaining.len() == 1 {
            return true;
        }
        
        // 功能键组合
        if remaining.starts_with('f') && remaining[1..].parse::<u8>().is_ok() {
            return true;
        }
        
        // 如果有修饰键且剩余部分不为空，认为是有效的
        has_modifier && !remaining.is_empty()
    }

    /// 导出为 JSON 格式字符串
    pub fn export_to_json(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to export keybindings to JSON: {}", e))
    }

    /// 导出为 TOML 格式字符串
    pub fn export_to_toml(&self) -> anyhow::Result<String> {
        toml::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to export keybindings to TOML: {}", e))
    }

    /// 导出到文件（根据扩展名自动选择格式）
    pub fn export_to_file(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let content = match path.extension().and_then(|e| e.to_str()) {
            Some("json") => self.export_to_json()?,
            Some("toml") => self.export_to_toml()?,
            _ => self.export_to_toml()?,
        };

        std::fs::write(path, content)?;
        Ok(())
    }

    /// 从文件导入（自动检测格式：先尝试 TOML，再尝试 JSON）
    pub fn import_from_file(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Err(anyhow::anyhow!("File does not exist: {}", path.display()));
        }

        let content = std::fs::read_to_string(path)?;

        // 根据扩展名或内容检测格式
        let extension = path.extension().and_then(|e| e.to_str());

        match extension {
            Some("json") => {
                Self::parse_json(&content)
            }
            Some("toml") => {
                Self::parse_toml(&content)
            }
            _ => {
                // 自动检测：先尝试 TOML，再尝试 JSON
                if let Ok(config) = Self::parse_toml(&content) {
                    return Ok(config);
                }
                Self::parse_json(&content)
            }
        }
    }

    /// 解析 TOML 格式
    fn parse_toml(content: &str) -> anyhow::Result<Self> {
        toml::from_str(content)
            .map_err(|e| anyhow::anyhow!("Failed to parse TOML keybindings: {}", e))
    }

    /// 解析 JSON 格式
    fn parse_json(content: &str) -> anyhow::Result<Self> {
        serde_json::from_str(content)
            .map_err(|e| anyhow::anyhow!("Failed to parse JSON keybindings: {}", e))
    }

    /// 从预设创建配置
    pub fn from_preset(preset: KeybindingsPreset) -> Self {
        Self {
            bindings: preset.default_bindings(),
            preset: Some(preset.name().to_string()),
        }
    }
}

/// 快捷键冲突
#[derive(Debug, Clone)]
pub struct KeybindingConflict {
    /// 冲突的按键
    pub key: String,
    /// 冲突的操作列表
    pub actions: Vec<String>,
}

/// 快捷键预设方案
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeybindingsPreset {
    /// Emacs 风格快捷键
    Emacs,
    /// Vim 风格快捷键
    Vim,
    /// VSCode 风格快捷键
    VSCode,
}

impl KeybindingsPreset {
    /// 返回预设名称
    pub fn name(&self) -> &str {
        match self {
            KeybindingsPreset::Emacs => "Emacs",
            KeybindingsPreset::Vim => "Vim",
            KeybindingsPreset::VSCode => "VSCode",
        }
    }

    /// 返回所有预设
    pub fn all() -> Vec<KeybindingsPreset> {
        vec![KeybindingsPreset::Emacs, KeybindingsPreset::Vim, KeybindingsPreset::VSCode]
    }

    /// 返回该预设的默认快捷键映射
    pub fn default_bindings(&self) -> HashMap<String, String> {
        let mut bindings = HashMap::new();

        match self {
            KeybindingsPreset::Emacs => {
                // Emacs 风格快捷键
                // 光标移动
                bindings.insert("ctrl+a".into(), "line_start".into());
                bindings.insert("ctrl+e".into(), "line_end".into());
                bindings.insert("ctrl+n".into(), "cursor_down".into());
                bindings.insert("ctrl+p".into(), "cursor_up".into());
                bindings.insert("ctrl+f".into(), "cursor_right".into());
                bindings.insert("ctrl+b".into(), "cursor_left".into());
                bindings.insert("alt+f".into(), "word_right".into());
                bindings.insert("alt+b".into(), "word_left".into());
                bindings.insert("alt+<".into(), "doc_start".into());
                bindings.insert("alt+>".into(), "doc_end".into());
                bindings.insert("ctrl+v".into(), "page_down".into());
                bindings.insert("alt+v".into(), "page_up".into());
                
                // 编辑操作
                bindings.insert("ctrl+k".into(), "delete_to_line_end".into());
                bindings.insert("ctrl+u".into(), "delete_to_line_start".into());
                bindings.insert("ctrl+w".into(), "delete_word_backward".into());
                bindings.insert("alt+d".into(), "delete_word_forward".into());
                bindings.insert("ctrl+h".into(), "delete_backward".into());
                bindings.insert("ctrl+d".into(), "delete_forward".into());
                
                // 剪切/粘贴
                bindings.insert("ctrl+w".into(), "cut".into()); // 在 mark 模式下
                bindings.insert("ctrl+y".into(), "paste".into());
                bindings.insert("alt+y".into(), "paste_pop".into());
                
                // 搜索
                bindings.insert("ctrl+s".into(), "search_forward".into());
                bindings.insert("ctrl+r".into(), "search_backward".into());
                
                // 其他
                bindings.insert("ctrl+l".into(), "clear".into());
                bindings.insert("ctrl+g".into(), "cancel".into());
                bindings.insert("ctrl+x ctrl+c".into(), "quit".into());
                bindings.insert("ctrl+x ctrl+s".into(), "save".into());
                bindings.insert("ctrl+x ctrl+f".into(), "open".into());
                bindings.insert("ctrl+shift+-".into(), "undo".into());
                bindings.insert("ctrl+_".into(), "undo".into());
                bindings.insert("ctrl+x u".into(), "undo".into());
            }
            KeybindingsPreset::Vim => {
                // Vim Normal 模式快捷键
                // 光标移动
                bindings.insert("h".into(), "cursor_left".into());
                bindings.insert("j".into(), "cursor_down".into());
                bindings.insert("k".into(), "cursor_up".into());
                bindings.insert("l".into(), "cursor_right".into());
                bindings.insert("w".into(), "word_forward".into());
                bindings.insert("b".into(), "word_backward".into());
                bindings.insert("e".into(), "word_end".into());
                bindings.insert("0".into(), "line_start".into());
                bindings.insert("$".into(), "line_end".into());
                bindings.insert("^".into(), "line_first_nonblank".into());
                bindings.insert("gg".into(), "doc_start".into());
                bindings.insert("G".into(), "doc_end".into());
                bindings.insert("ctrl+f".into(), "page_down".into());
                bindings.insert("ctrl+b".into(), "page_up".into());
                bindings.insert("ctrl+d".into(), "half_page_down".into());
                bindings.insert("ctrl+u".into(), "half_page_up".into());
                
                // 插入模式
                bindings.insert("i".into(), "enter_insert".into());
                bindings.insert("a".into(), "enter_insert_after".into());
                bindings.insert("I".into(), "enter_insert_line_start".into());
                bindings.insert("A".into(), "enter_insert_line_end".into());
                bindings.insert("o".into(), "open_line_below".into());
                bindings.insert("O".into(), "open_line_above".into());
                
                // 编辑操作
                bindings.insert("x".into(), "delete_char".into());
                bindings.insert("X".into(), "delete_char_before".into());
                bindings.insert("dd".into(), "delete_line".into());
                bindings.insert("D".into(), "delete_to_line_end".into());
                bindings.insert("cc".into(), "change_line".into());
                bindings.insert("C".into(), "change_to_line_end".into());
                bindings.insert("cw".into(), "change_word".into());
                bindings.insert("dw".into(), "delete_word".into());
                bindings.insert("d$".into(), "delete_to_line_end".into());
                bindings.insert("d0".into(), "delete_to_line_start".into());
                
                // 复制粘贴
                bindings.insert("yy".into(), "yank_line".into());
                bindings.insert("yw".into(), "yank_word".into());
                bindings.insert("p".into(), "paste_after".into());
                bindings.insert("P".into(), "paste_before".into());
                
                // 撤销重做
                bindings.insert("u".into(), "undo".into());
                bindings.insert("ctrl+r".into(), "redo".into());
                
                // 搜索
                bindings.insert("/".into(), "search".into());
                bindings.insert("?".into(), "search_backward".into());
                bindings.insert("n".into(), "search_next".into());
                bindings.insert("N".into(), "search_prev".into());
                
                // 其他
                bindings.insert("escape".into(), "exit_mode".into());
                bindings.insert(":".into(), "enter_command".into());
                bindings.insert("v".into(), "enter_visual".into());
                bindings.insert("V".into(), "enter_visual_line".into());
                bindings.insert("ctrl+v".into(), "enter_visual_block".into());
                bindings.insert(".".into(), "repeat_last".into());
            }
            KeybindingsPreset::VSCode => {
                // VSCode 风格快捷键
                // 文件操作
                bindings.insert("ctrl+s".into(), "save".into());
                bindings.insert("ctrl+o".into(), "open".into());
                bindings.insert("ctrl+w".into(), "close".into());
                bindings.insert("ctrl+n".into(), "new_file".into());
                
                // 编辑操作
                bindings.insert("ctrl+z".into(), "undo".into());
                bindings.insert("ctrl+shift+z".into(), "redo".into());
                bindings.insert("ctrl+y".into(), "redo".into());
                bindings.insert("ctrl+x".into(), "cut".into());
                bindings.insert("ctrl+c".into(), "copy".into());
                bindings.insert("ctrl+v".into(), "paste".into());
                bindings.insert("ctrl+a".into(), "select_all".into());
                bindings.insert("ctrl+d".into(), "select_next_occurrence".into());
                bindings.insert("ctrl+shift+k".into(), "delete_line".into());
                bindings.insert("ctrl+enter".into(), "insert_line_below".into());
                bindings.insert("ctrl+shift+enter".into(), "insert_line_above".into());
                bindings.insert("alt+up".into(), "move_line_up".into());
                bindings.insert("alt+down".into(), "move_line_down".into());
                bindings.insert("ctrl+shift+up".into(), "copy_line_up".into());
                bindings.insert("ctrl+shift+down".into(), "copy_line_down".into());
                
                // 导航
                bindings.insert("ctrl+g".into(), "go_to_line".into());
                bindings.insert("ctrl+p".into(), "quick_open".into());
                bindings.insert("ctrl+shift+p".into(), "command_palette".into());
                bindings.insert("ctrl+shift+o".into(), "go_to_symbol".into());
                bindings.insert("ctrl+t".into(), "go_to_symbol_workspace".into());
                bindings.insert("ctrl+m".into(), "toggle_tab_move".into());
                bindings.insert("ctrl+home".into(), "doc_start".into());
                bindings.insert("ctrl+end".into(), "doc_end".into());
                bindings.insert("ctrl+up".into(), "scroll_up".into());
                bindings.insert("ctrl+down".into(), "scroll_down".into());
                
                // 搜索
                bindings.insert("ctrl+f".into(), "find".into());
                bindings.insert("ctrl+h".into(), "replace".into());
                bindings.insert("ctrl+shift+f".into(), "find_in_files".into());
                bindings.insert("ctrl+shift+h".into(), "replace_in_files".into());
                bindings.insert("f3".into(), "find_next".into());
                bindings.insert("shift+f3".into(), "find_prev".into());
                
                // 视图
                bindings.insert("ctrl+b".into(), "toggle_sidebar".into());
                bindings.insert("ctrl+j".into(), "toggle_panel".into());
                bindings.insert("ctrl+`".into(), "toggle_terminal".into());
                bindings.insert("ctrl+shift+e".into(), "show_explorer".into());
                bindings.insert("ctrl+shift+f".into(), "show_search".into());
                bindings.insert("ctrl+shift+g".into(), "show_scm".into());
                bindings.insert("ctrl+shift+d".into(), "show_debug".into());
                bindings.insert("ctrl+shift+x".into(), "show_extensions".into());
                
                // 调试
                bindings.insert("f5".into(), "start_debug".into());
                bindings.insert("f9".into(), "toggle_breakpoint".into());
                bindings.insert("f10".into(), "step_over".into());
                bindings.insert("f11".into(), "step_into".into());
                bindings.insert("shift+f11".into(), "step_out".into());
                
                // 其他
                bindings.insert("ctrl+k ctrl+s".into(), "open_keybindings".into());
                bindings.insert("ctrl+shift+n".into(), "new_window".into());
                bindings.insert("ctrl+shift+w".into(), "close_window".into());
                bindings.insert("ctrl+,".into(), "open_settings".into());
                bindings.insert("ctrl+k ctrl+t".into(), "select_theme".into());
            }
        }

        bindings
    }
}

impl std::fmt::Display for KeybindingsPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// =============================================================================
// 快捷键管理器
// =============================================================================

/// 快捷键管理器
pub struct KeybindingsManager {
    /// 所有绑定
    bindings: Vec<KeybindingDefinition>,
    /// 按键到操作的快速查找映射
    key_to_actions: HashMap<String, Vec<String>>,
    /// 操作到按键的映射
    action_to_keys: HashMap<String, Vec<String>>,
}

impl KeybindingsManager {
    /// 创建新的快捷键管理器
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
            key_to_actions: HashMap::new(),
            action_to_keys: HashMap::new(),
        }
    }

    /// 从定义配置创建
    pub fn from_config(config: KeybindingsDefinitionConfig) -> Self {
        let mut manager = Self::new();
        for binding in config.bindings {
            manager.add(binding);
        }
        manager
    }

    /// 应用 TOML 配置到管理器
    /// 
    /// 这会用配置中的绑定覆盖或添加到现有绑定
    pub fn apply_config(&mut self, config: &KeybindingsConfig) -> anyhow::Result<()> {
        // 验证配置
        let warnings = config.validate();
        for warning in &warnings {
            tracing::warn!("Keybindings config warning: {}", warning);
        }
        
        // 应用绑定
        for (key, action) in &config.bindings {
            // 移除该按键的现有绑定（避免冲突）
            self.remove_by_key(key);
            
            // 添加新绑定
            self.add(KeybindingDefinition::new(
                key.clone(),
                action.clone(),
                format!("Custom binding: {} -> {}", key, action),
            ));
        }
        
        Ok(())
    }

    /// 从管理器导出当前配置
    /// 
    /// 导出为简化的 TOML 配置格式
    pub fn to_config(&self) -> KeybindingsConfig {
        let mut bindings = HashMap::new();
        
        // 只导出没有上下文的绑定（全局绑定）
        for binding in &self.bindings {
            if binding.context.is_none() {
                // 如果同一按键有多个操作，取最后一个
                bindings.insert(binding.key.clone(), binding.action.clone());
            }
        }
        
        KeybindingsConfig {
            bindings,
            preset: None,
        }
    }

    /// 重建查找映射
    fn rebuild_maps(&mut self) {
        self.key_to_actions.clear();
        self.action_to_keys.clear();

        for binding in &self.bindings {
            self.key_to_actions
                .entry(binding.key.clone())
                .or_default()
                .push(binding.action.clone());

            self.action_to_keys
                .entry(binding.action.clone())
                .or_default()
                .push(binding.key.clone());
        }
    }

    /// 查找匹配的操作
    /// 返回第一个匹配的操作名称
    pub fn find_action(&self, key: &Key, context: Option<&str>) -> Option<&str> {
        for binding in &self.bindings {
            // 如果指定了上下文，检查是否匹配
            if let Some(ctx) = &binding.context {
                if let Some(req_ctx) = context {
                    if ctx != req_ctx {
                        continue;
                    }
                }
            }

            if matches_key(key, &binding.key) {
                return Some(&binding.action);
            }
        }
        None
    }

    /// 查找所有匹配的操作
    pub fn find_all_actions(&self, key: &Key, context: Option<&str>) -> Vec<&str> {
        let mut actions = Vec::new();
        for binding in &self.bindings {
            // 如果指定了上下文，检查是否匹配
            if let Some(ctx) = &binding.context {
                if let Some(req_ctx) = context {
                    if ctx != req_ctx {
                        continue;
                    }
                }
            }

            if matches_key(key, &binding.key) {
                actions.push(binding.action.as_str());
            }
        }
        actions
    }

    /// 检测冲突
    pub fn find_conflicts(&self) -> Vec<KeybindingConflict> {
        let mut conflicts = Vec::new();

        for (key, actions) in &self.key_to_actions {
            if actions.len() > 1 {
                conflicts.push(KeybindingConflict {
                    key: key.clone(),
                    actions: actions.clone(),
                });
            }
        }

        conflicts
    }

    /// 添加绑定
    pub fn add(&mut self, definition: KeybindingDefinition) {
        // 检查是否已存在相同的按键+操作组合
        let exists = self.bindings.iter().any(|b| {
            b.key == definition.key && b.action == definition.action && b.context == definition.context
        });

        if !exists {
            self.bindings.push(definition);
            self.rebuild_maps();
        }
    }

    /// 移除绑定
    pub fn remove(&mut self, key: &str, action: &str) {
        self.bindings.retain(|b| !(b.key == key && b.action == action));
        self.rebuild_maps();
    }

    /// 根据操作移除所有绑定
    pub fn remove_by_action(&mut self, action: &str) {
        self.bindings.retain(|b| b.action != action);
        self.rebuild_maps();
    }

    /// 根据按键移除所有绑定
    pub fn remove_by_key(&mut self, key: &str) {
        self.bindings.retain(|b| b.key != key);
        self.rebuild_maps();
    }

    /// 获取所有绑定
    pub fn get_bindings(&self) -> &[KeybindingDefinition] {
        &self.bindings
    }

    /// 获取操作的按键
    pub fn get_keys_for_action(&self, action: &str) -> Option<&[String]> {
        self.action_to_keys.get(action).map(|v| v.as_slice())
    }

    /// 获取按键的操作
    pub fn get_actions_for_key(&self, key: &str) -> Option<&[String]> {
        self.key_to_actions.get(key).map(|v| v.as_slice())
    }

    /// 检查按键是否有绑定
    pub fn has_binding(&self, key: &str) -> bool {
        self.key_to_actions.contains_key(key)
    }

    /// 检查操作是否有绑定
    pub fn has_action(&self, action: &str) -> bool {
        self.action_to_keys.contains_key(action)
    }

    /// 清空所有绑定
    pub fn clear(&mut self) {
        self.bindings.clear();
        self.key_to_actions.clear();
        self.action_to_keys.clear();
    }

    /// 获取绑定数量
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// 应用预设方案
    /// 
    /// 清除所有现有绑定，应用预设的默认绑定
    pub fn apply_preset(&mut self, preset: KeybindingsPreset) -> anyhow::Result<()> {
        // 清除现有绑定
        self.clear();

        // 应用预设绑定
        let bindings = preset.default_bindings();
        for (key, action) in bindings {
            self.add(KeybindingDefinition::new(
                key.clone(),
                action.clone(),
                format!("{} preset: {} -> {}", preset.name(), key, action),
            ));
        }

        Ok(())
    }

    /// 合并导入的配置
    /// 
    /// 将导入的配置合并到当前配置中，返回被覆盖的绑定列表
    pub fn merge_import(&mut self, config: &KeybindingsConfig) -> anyhow::Result<Vec<String>> {
        let mut overridden = Vec::new();

        // 验证配置
        let warnings = config.validate();
        for warning in &warnings {
            tracing::warn!("Import config warning: {}", warning);
        }

        // 应用绑定
        for (key, action) in &config.bindings {
            // 检查是否覆盖了现有绑定
            if self.has_binding(key) {
                if let Some(actions) = self.get_actions_for_key(key) {
                    for existing_action in actions {
                        overridden.push(format!("{} -> {}", key, existing_action));
                    }
                }
            }

            // 移除该按键的现有绑定
            self.remove_by_key(key);

            // 添加新绑定
            self.add(KeybindingDefinition::new(
                key.clone(),
                action.clone(),
                format!("Imported binding: {} -> {}", key, action),
            ));
        }

        Ok(overridden)
    }
}

impl Default for KeybindingsManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// 默认快捷键
// =============================================================================

/// 默认快捷键映射
pub fn default_keybindings() -> KeybindingsDefinitionConfig {
    KeybindingsDefinitionConfig {
        bindings: vec![
            // 取消/退出
            KeybindingDefinition {
                key: "ctrl+c".into(),
                action: "cancel".into(),
                description: "Cancel current operation".into(),
                context: None,
            },
            KeybindingDefinition {
                key: "ctrl+d".into(),
                action: "exit".into(),
                description: "Exit".into(),
                context: None,
            },
            KeybindingDefinition {
                key: "ctrl+q".into(),
                action: "quit".into(),
                description: "Quit application".into(),
                context: None,
            },
            // 输入操作
            KeybindingDefinition {
                key: "enter".into(),
                action: "submit".into(),
                description: "Submit input".into(),
                context: Some("input".into()),
            },
            KeybindingDefinition {
                key: "shift+enter".into(),
                action: "newline".into(),
                description: "Insert newline".into(),
                context: Some("input".into()),
            },
            KeybindingDefinition {
                key: "escape".into(),
                action: "dismiss".into(),
                description: "Dismiss overlay".into(),
                context: Some("overlay".into()),
            },
            // 编辑器操作
            KeybindingDefinition {
                key: "ctrl+z".into(),
                action: "undo".into(),
                description: "Undo".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+shift+z".into(),
                action: "redo".into(),
                description: "Redo".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+y".into(),
                action: "redo".into(),
                description: "Redo (alternative)".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+x".into(),
                action: "cut".into(),
                description: "Cut selection".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+c".into(),
                action: "copy".into(),
                description: "Copy selection".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+v".into(),
                action: "paste".into(),
                description: "Paste".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+a".into(),
                action: "select_all".into(),
                description: "Select all".into(),
                context: Some("editor".into()),
            },
            // 光标移动
            KeybindingDefinition {
                key: "up".into(),
                action: "cursor_up".into(),
                description: "Move cursor up".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "down".into(),
                action: "cursor_down".into(),
                description: "Move cursor down".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "left".into(),
                action: "cursor_left".into(),
                description: "Move cursor left".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "right".into(),
                action: "cursor_right".into(),
                description: "Move cursor right".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+left".into(),
                action: "word_left".into(),
                description: "Move cursor word left".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+right".into(),
                action: "word_right".into(),
                description: "Move cursor word right".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "home".into(),
                action: "line_start".into(),
                description: "Move to line start".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "end".into(),
                action: "line_end".into(),
                description: "Move to line end".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+home".into(),
                action: "doc_start".into(),
                description: "Move to document start".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+end".into(),
                action: "doc_end".into(),
                description: "Move to document end".into(),
                context: Some("editor".into()),
            },
            // 删除操作
            KeybindingDefinition {
                key: "backspace".into(),
                action: "delete_backward".into(),
                description: "Delete character backward".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "delete".into(),
                action: "delete_forward".into(),
                description: "Delete character forward".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+w".into(),
                action: "delete_word_backward".into(),
                description: "Delete word backward".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "alt+backspace".into(),
                action: "delete_word_backward".into(),
                description: "Delete word backward".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+delete".into(),
                action: "delete_word_forward".into(),
                description: "Delete word forward".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "alt+delete".into(),
                action: "delete_word_forward".into(),
                description: "Delete word forward".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+u".into(),
                action: "delete_to_line_start".into(),
                description: "Delete to line start".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "ctrl+k".into(),
                action: "delete_to_line_end".into(),
                description: "Delete to line end".into(),
                context: Some("editor".into()),
            },
            // 页面滚动
            KeybindingDefinition {
                key: "pageup".into(),
                action: "page_up".into(),
                description: "Page up".into(),
                context: Some("editor".into()),
            },
            KeybindingDefinition {
                key: "pagedown".into(),
                action: "page_down".into(),
                description: "Page down".into(),
                context: Some("editor".into()),
            },
            // 自动完成
            KeybindingDefinition {
                key: "tab".into(),
                action: "accept_completion".into(),
                description: "Accept autocomplete suggestion".into(),
                context: Some("autocomplete".into()),
            },
            KeybindingDefinition {
                key: "ctrl+n".into(),
                action: "next_completion".into(),
                description: "Next autocomplete suggestion".into(),
                context: Some("autocomplete".into()),
            },
            KeybindingDefinition {
                key: "ctrl+p".into(),
                action: "prev_completion".into(),
                description: "Previous autocomplete suggestion".into(),
                context: Some("autocomplete".into()),
            },
            KeybindingDefinition {
                key: "escape".into(),
                action: "cancel_completion".into(),
                description: "Cancel autocomplete".into(),
                context: Some("autocomplete".into()),
            },
            // 搜索
            KeybindingDefinition {
                key: "ctrl+f".into(),
                action: "find".into(),
                description: "Find".into(),
                context: None,
            },
            KeybindingDefinition {
                key: "ctrl+g".into(),
                action: "find_next".into(),
                description: "Find next".into(),
                context: Some("search".into()),
            },
            KeybindingDefinition {
                key: "ctrl+shift+g".into(),
                action: "find_prev".into(),
                description: "Find previous".into(),
                context: Some("search".into()),
            },
            KeybindingDefinition {
                key: "escape".into(),
                action: "close_search".into(),
                description: "Close search".into(),
                context: Some("search".into()),
            },
            // 其他
            KeybindingDefinition {
                key: "ctrl+l".into(),
                action: "clear".into(),
                description: "Clear screen".into(),
                context: None,
            },
            KeybindingDefinition {
                key: "ctrl+s".into(),
                action: "save".into(),
                description: "Save".into(),
                context: None,
            },
            KeybindingDefinition {
                key: "ctrl+o".into(),
                action: "open".into(),
                description: "Open".into(),
                context: None,
            },
            KeybindingDefinition {
                key: "f1".into(),
                action: "help".into(),
                description: "Show help".into(),
                context: None,
            },
            KeybindingDefinition {
                key: "ctrl+equal".into(),
                action: "zoom_in".into(),
                description: "Zoom in".into(),
                context: None,
            },
            KeybindingDefinition {
                key: "ctrl+minus".into(),
                action: "zoom_out".into(),
                description: "Zoom out".into(),
                context: None,
            },
            KeybindingDefinition {
                key: "ctrl+0".into(),
                action: "zoom_reset".into(),
                description: "Reset zoom".into(),
                context: None,
            },
            // Vim Normal 模式键位
            KeybindingDefinition::new("h", "cursor_left", "Move cursor left")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("j", "cursor_down", "Move cursor down")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("k", "cursor_up", "Move cursor up")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("l", "cursor_right", "Move cursor right")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("i", "enter_insert", "Enter insert mode")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("a", "enter_insert_after", "Enter insert mode after cursor")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("o", "open_line_below", "Open line below")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("w", "word_forward", "Move to next word")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("b", "word_backward", "Move to previous word")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("e", "word_end", "Move to end of word")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("0", "line_start", "Move to line start")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("$", "line_end", "Move to line end")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("x", "delete_char", "Delete character")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("u", "undo", "Undo")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("p", "paste_after", "Paste after cursor")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("v", "enter_visual", "Enter visual mode")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new(":", "enter_command", "Enter command mode")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("/", "search", "Search")
                .with_context("editor/vim/normal"),
            KeybindingDefinition::new("n", "search_next", "Next search match")
                .with_context("editor/vim/normal"),
            // Vim Insert 模式键位
            KeybindingDefinition::new("escape", "exit_mode", "Return to normal mode")
                .with_context("editor/vim/insert"),
            // Vim Visual 模式键位
            KeybindingDefinition::new("escape", "exit_visual", "Exit visual mode")
                .with_context("editor/vim/visual"),
        ],
    }
}

// =============================================================================
// 全局快捷键管理
// =============================================================================

static GLOBAL_KEYBINDINGS: OnceLock<Arc<RwLock<KeybindingsManager>>> = OnceLock::new();

/// 获取全局快捷键管理器
pub fn get_keybindings() -> Arc<RwLock<KeybindingsManager>> {
    GLOBAL_KEYBINDINGS
        .get_or_init(|| Arc::new(RwLock::new(KeybindingsManager::from_config(default_keybindings()))))
        .clone()
}

/// 设置全局快捷键配置
pub fn set_keybindings(config: KeybindingsDefinitionConfig) {
    if let Ok(mut manager) = get_keybindings().write() {
        *manager = KeybindingsManager::from_config(config);
    }
}

/// 应用 TOML 配置到全局快捷键管理器
pub fn apply_keybindings_config(config: &KeybindingsConfig) -> anyhow::Result<()> {
    if let Ok(mut manager) = get_keybindings().write() {
        manager.apply_config(config)?;
    }
    Ok(())
}

/// 导出当前全局快捷键为 TOML 配置
pub fn export_keybindings_config() -> KeybindingsConfig {
    if let Ok(manager) = get_keybindings().read() {
        manager.to_config()
    } else {
        KeybindingsConfig::default()
    }
}

/// 重置为默认快捷键
pub fn reset_to_default_keybindings() {
    set_keybindings(default_keybindings());
}

// =============================================================================
// 便捷函数
// =============================================================================

/// 查找按键对应的操作
pub fn find_action(key: &Key, context: Option<&str>) -> Option<String> {
    if let Ok(manager) = get_keybindings().read() {
        manager.find_action(key, context).map(|s| s.to_string())
    } else {
        None
    }
}

/// 检查按键是否匹配某个操作
pub fn is_action(key: &Key, action: &str, context: Option<&str>) -> bool {
    if let Ok(manager) = get_keybindings().read() {
        manager.find_action(key, context) == Some(action)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_keybinding_definition() {
        let def = KeybindingDefinition::new("ctrl+c", "cancel", "Cancel operation")
            .with_context("global");

        assert_eq!(def.key, "ctrl+c");
        assert_eq!(def.action, "cancel");
        assert_eq!(def.context, Some("global".to_string()));
    }

    #[test]
    fn test_keybindings_manager() {
        let mut manager = KeybindingsManager::new();

        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+v", "paste", "Paste"));

        assert_eq!(manager.len(), 2);
        assert!(manager.has_binding("ctrl+c"));
        assert!(manager.has_action("paste"));

        let keys = manager.get_keys_for_action("cancel").unwrap();
        assert!(keys.contains(&"ctrl+c".to_string()));
    }

    #[test]
    fn test_conflict_detection() {
        let mut manager = KeybindingsManager::new();

        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+c", "copy", "Copy"));

        let conflicts = manager.find_conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].key, "ctrl+c");
        assert_eq!(conflicts[0].actions.len(), 2);
    }

    #[test]
    fn test_default_keybindings() {
        let config = default_keybindings();
        assert!(!config.bindings.is_empty());

        // 检查一些关键绑定存在
        let has_ctrl_c = config.bindings.iter().any(|b| b.key == "ctrl+c" && b.action == "cancel");
        assert!(has_ctrl_c);
    }

    #[test]
    fn test_keybindings_config_toml_roundtrip() {
        let mut config = KeybindingsConfig::default();
        config.bindings.insert("ctrl+c".to_string(), "cancel".to_string());
        config.bindings.insert("ctrl+v".to_string(), "paste".to_string());
        config.preset = Some("default".to_string());

        // 序列化到 TOML
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("ctrl+c"));
        assert!(toml_str.contains("cancel"));

        // 反序列化
        let parsed: KeybindingsConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.bindings.get("ctrl+c"), Some(&"cancel".to_string()));
        assert_eq!(parsed.bindings.get("ctrl+v"), Some(&"paste".to_string()));
        assert_eq!(parsed.preset, Some("default".to_string()));
    }

    #[test]
    fn test_keybindings_config_file_roundtrip() {
        let mut config = KeybindingsConfig::default();
        config.bindings.insert("ctrl+x".to_string(), "cut".to_string());
        config.bindings.insert("ctrl+z".to_string(), "undo".to_string());

        // 创建临时文件
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // 保存
        config.save_to_file(&path).unwrap();

        // 加载
        let loaded = KeybindingsConfig::load_from_file(&path).unwrap();
        assert_eq!(loaded.bindings.get("ctrl+x"), Some(&"cut".to_string()));
        assert_eq!(loaded.bindings.get("ctrl+z"), Some(&"undo".to_string()));
    }

    #[test]
    fn test_keybindings_config_validate() {
        let mut config = KeybindingsConfig::default();
        
        // 有效配置
        config.bindings.insert("ctrl+c".to_string(), "cancel".to_string());
        let warnings = config.validate();
        assert!(warnings.is_empty());

        // 无效按键格式
        config.bindings.insert("invalid++key".to_string(), "test".to_string());
        let warnings = config.validate();
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_keybindings_config_load_missing_file() {
        let path = std::path::PathBuf::from("/nonexistent/path/keybindings.toml");
        let config = KeybindingsConfig::load_from_file(&path).unwrap();
        assert!(config.bindings.is_empty());
    }

    #[test]
    fn test_apply_config_to_manager() {
        let mut manager = KeybindingsManager::new();
        
        // 添加默认绑定
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+x", "cut", "Cut"));
        
        // 创建新配置（覆盖 cancel 和 cut）
        let mut config = KeybindingsConfig::default();
        config.bindings.insert("ctrl+c".to_string(), "copy".to_string());
        config.bindings.insert("ctrl+v".to_string(), "paste".to_string());
        config.bindings.insert("ctrl+x".to_string(), "cut".to_string()); // 保留 cut 但按键不变
        
        // 应用配置
        manager.apply_config(&config).unwrap();
        
        // 检查绑定已更新
        assert!(manager.has_binding("ctrl+c"));
        assert!(manager.has_binding("ctrl+v"));
        assert!(manager.has_binding("ctrl+x"));
        assert!(manager.has_action("copy"));
        assert!(manager.has_action("paste"));
        assert!(manager.has_action("cut"));
        // cancel 应该被移除，因为配置中用 ctrl+c 绑定了 copy
        assert!(!manager.has_action("cancel"));
    }

    #[test]
    fn test_manager_to_config() {
        let mut manager = KeybindingsManager::new();
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+v", "paste", "Paste"));
        // 添加带上下文的绑定（不应被导出）
        manager.add(KeybindingDefinition::new("enter", "submit", "Submit").with_context("input"));
        
        let config = manager.to_config();
        
        // 只导出全局绑定
        assert_eq!(config.bindings.get("ctrl+c"), Some(&"cancel".to_string()));
        assert_eq!(config.bindings.get("ctrl+v"), Some(&"paste".to_string()));
        assert!(!config.bindings.contains_key("enter")); // 带上下文的不导出
    }

    #[test]
    fn test_valid_key_format() {
        // 有效格式
        assert!(KeybindingsConfig::is_valid_key_format("a"));
        assert!(KeybindingsConfig::is_valid_key_format("ctrl+c"));
        assert!(KeybindingsConfig::is_valid_key_format("ctrl+shift+c"));
        assert!(KeybindingsConfig::is_valid_key_format("alt+enter"));
        assert!(KeybindingsConfig::is_valid_key_format("escape"));
        assert!(KeybindingsConfig::is_valid_key_format("f1"));
        assert!(KeybindingsConfig::is_valid_key_format("ctrl+f1"));
        
        // 无效格式
        assert!(!KeybindingsConfig::is_valid_key_format(""));
        assert!(!KeybindingsConfig::is_valid_key_format("ctrl+"));
    }

    // =============================================================================
    // 预设测试
    // =============================================================================

    #[test]
    fn test_preset_emacs_bindings() {
        let bindings = KeybindingsPreset::Emacs.default_bindings();
        
        // 检查 Emacs 关键绑定
        assert!(bindings.contains_key("ctrl+a"), "Emacs should have ctrl+a for line_start");
        assert!(bindings.contains_key("ctrl+e"), "Emacs should have ctrl+e for line_end");
        assert!(bindings.contains_key("ctrl+k"), "Emacs should have ctrl+k for delete_to_line_end");
        assert!(bindings.contains_key("ctrl+y"), "Emacs should have ctrl+y for paste");
        assert!(bindings.contains_key("ctrl+n"), "Emacs should have ctrl+n for cursor_down");
        assert!(bindings.contains_key("ctrl+p"), "Emacs should have ctrl+p for cursor_up");
        assert!(bindings.contains_key("alt+f"), "Emacs should have alt+f for word_right");
        assert!(bindings.contains_key("alt+b"), "Emacs should have alt+b for word_left");
        
        // 检查名称
        assert_eq!(KeybindingsPreset::Emacs.name(), "Emacs");
    }

    #[test]
    fn test_preset_vim_bindings() {
        let bindings = KeybindingsPreset::Vim.default_bindings();
        
        // 检查 Vim 关键绑定
        assert!(bindings.contains_key("h"), "Vim should have h for cursor_left");
        assert!(bindings.contains_key("j"), "Vim should have j for cursor_down");
        assert!(bindings.contains_key("k"), "Vim should have k for cursor_up");
        assert!(bindings.contains_key("l"), "Vim should have l for cursor_right");
        assert!(bindings.contains_key("i"), "Vim should have i for enter_insert");
        assert!(bindings.contains_key("dd"), "Vim should have dd for delete_line");
        assert!(bindings.contains_key("yy"), "Vim should have yy for yank_line");
        assert!(bindings.contains_key("p"), "Vim should have p for paste_after");
        assert!(bindings.contains_key("u"), "Vim should have u for undo");
        assert!(bindings.contains_key("escape"), "Vim should have escape for exit_mode");
        
        // 检查名称
        assert_eq!(KeybindingsPreset::Vim.name(), "Vim");
    }

    #[test]
    fn test_preset_vscode_bindings() {
        let bindings = KeybindingsPreset::VSCode.default_bindings();
        
        // 检查 VSCode 关键绑定
        assert!(bindings.contains_key("ctrl+s"), "VSCode should have ctrl+s for save");
        assert!(bindings.contains_key("ctrl+z"), "VSCode should have ctrl+z for undo");
        assert!(bindings.contains_key("ctrl+shift+z"), "VSCode should have ctrl+shift+z for redo");
        assert!(bindings.contains_key("ctrl+c"), "VSCode should have ctrl+c for copy");
        assert!(bindings.contains_key("ctrl+v"), "VSCode should have ctrl+v for paste");
        assert!(bindings.contains_key("ctrl+f"), "VSCode should have ctrl+f for find");
        assert!(bindings.contains_key("ctrl+p"), "VSCode should have ctrl+p for quick_open");
        assert!(bindings.contains_key("ctrl+shift+p"), "VSCode should have ctrl+shift+p for command_palette");
        assert!(bindings.contains_key("ctrl+`"), "VSCode should have ctrl+` for toggle_terminal");
        
        // 检查名称
        assert_eq!(KeybindingsPreset::VSCode.name(), "VSCode");
    }

    #[test]
    fn test_preset_all() {
        let presets = KeybindingsPreset::all();
        assert_eq!(presets.len(), 3);
        assert!(presets.contains(&KeybindingsPreset::Emacs));
        assert!(presets.contains(&KeybindingsPreset::Vim));
        assert!(presets.contains(&KeybindingsPreset::VSCode));
    }

    // =============================================================================
    // 导出/导入测试
    // =============================================================================

    #[test]
    fn test_export_to_json() {
        let mut config = KeybindingsConfig::default();
        config.bindings.insert("ctrl+c".to_string(), "cancel".to_string());
        config.bindings.insert("ctrl+v".to_string(), "paste".to_string());
        config.preset = Some("Emacs".to_string());

        let json = config.export_to_json().unwrap();
        assert!(json.contains("\"bindings\""));
        assert!(json.contains("\"ctrl+c\""));
        assert!(json.contains("\"cancel\""));
        assert!(json.contains("\"preset\""));
        assert!(json.contains("\"Emacs\""));
    }

    #[test]
    fn test_export_to_toml() {
        let mut config = KeybindingsConfig::default();
        config.bindings.insert("ctrl+c".to_string(), "cancel".to_string());
        config.bindings.insert("ctrl+v".to_string(), "paste".to_string());
        config.preset = Some("Vim".to_string());

        let toml = config.export_to_toml().unwrap();
        assert!(toml.contains("[bindings]"));
        assert!(toml.contains("ctrl+c"));
        assert!(toml.contains("cancel"));
        assert!(toml.contains("preset"));
        assert!(toml.contains("Vim"));
    }

    #[test]
    fn test_import_from_json() {
        let json = r#"{
            "bindings": {
                "ctrl+c": "cancel",
                "ctrl+v": "paste"
            },
            "preset": "VSCode"
        }"#;
        
        // 写入临时文件
        let temp_file = NamedTempFile::with_suffix(".json").unwrap();
        std::fs::write(&temp_file, json).unwrap();
        
        let config = KeybindingsConfig::import_from_file(temp_file.path()).unwrap();
        assert_eq!(config.bindings.get("ctrl+c"), Some(&"cancel".to_string()));
        assert_eq!(config.bindings.get("ctrl+v"), Some(&"paste".to_string()));
        assert_eq!(config.preset, Some("VSCode".to_string()));
    }

    #[test]
    fn test_import_from_toml() {
        let toml = r#"
preset = "Emacs"

[bindings]
ctrl-c = "cancel"
ctrl-v = "paste"
"#;
        
        // 写入临时文件
        let temp_file = NamedTempFile::with_suffix(".toml").unwrap();
        std::fs::write(&temp_file, toml).unwrap();
        
        let config = KeybindingsConfig::import_from_file(temp_file.path()).unwrap();
        assert_eq!(config.bindings.get("ctrl-c"), Some(&"cancel".to_string()));
        assert_eq!(config.bindings.get("ctrl-v"), Some(&"paste".to_string()));
        assert_eq!(config.preset, Some("Emacs".to_string()));
    }

    #[test]
    fn test_export_import_roundtrip() {
        let mut original = KeybindingsConfig::default();
        original.bindings.insert("ctrl+x".to_string(), "cut".to_string());
        original.bindings.insert("ctrl+y".to_string(), "redo".to_string());
        original.preset = Some("Vim".to_string());

        // TOML roundtrip
        let temp_toml = NamedTempFile::with_suffix(".toml").unwrap();
        original.export_to_file(temp_toml.path()).unwrap();
        let loaded_toml = KeybindingsConfig::import_from_file(temp_toml.path()).unwrap();
        assert_eq!(original.bindings, loaded_toml.bindings);
        assert_eq!(original.preset, loaded_toml.preset);

        // JSON roundtrip
        let temp_json = NamedTempFile::with_suffix(".json").unwrap();
        original.export_to_file(temp_json.path()).unwrap();
        let loaded_json = KeybindingsConfig::import_from_file(temp_json.path()).unwrap();
        assert_eq!(original.bindings, loaded_json.bindings);
        assert_eq!(original.preset, loaded_json.preset);
    }

    #[test]
    fn test_import_invalid_file() {
        // 测试导入不存在的文件
        let result = KeybindingsConfig::import_from_file(std::path::Path::new("/nonexistent/file.toml"));
        assert!(result.is_err());
        
        // 测试导入无效格式
        let temp_file = NamedTempFile::new().unwrap();
        std::fs::write(&temp_file, "not valid toml or json {{{").unwrap();
        let result = KeybindingsConfig::import_from_file(temp_file.path());
        assert!(result.is_err());
    }

    // =============================================================================
    // 预设应用测试
    // =============================================================================

    #[test]
    fn test_apply_preset() {
        let mut manager = KeybindingsManager::new();
        
        // 应用 Emacs 预设
        manager.apply_preset(KeybindingsPreset::Emacs).unwrap();
        
        // 检查 Emacs 绑定已应用
        assert!(manager.has_binding("ctrl+a"));
        assert!(manager.has_binding("ctrl+e"));
        assert!(manager.has_binding("ctrl+k"));
        assert!(manager.has_action("line_start"));
        assert!(manager.has_action("line_end"));
        
        // 应用 Vim 预设
        manager.apply_preset(KeybindingsPreset::Vim).unwrap();
        
        // Emacs 绑定应该被清除，Vim 绑定应该生效
        assert!(manager.has_binding("h"));
        assert!(manager.has_binding("j"));
        assert!(manager.has_binding("k"));
        assert!(manager.has_binding("l"));
        assert!(manager.has_action("cursor_left"));
        assert!(manager.has_action("cursor_down"));
    }

    #[test]
    fn test_from_preset() {
        let config = KeybindingsConfig::from_preset(KeybindingsPreset::Emacs);
        
        assert_eq!(config.preset, Some("Emacs".to_string()));
        assert!(!config.bindings.is_empty());
        assert!(config.bindings.contains_key("ctrl+a"));
        assert!(config.bindings.contains_key("ctrl+e"));
    }

    #[test]
    fn test_merge_import() {
        let mut manager = KeybindingsManager::new();
        
        // 初始绑定
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+x", "cut", "Cut"));
        
        // 导入配置（覆盖 ctrl+c，添加 ctrl+v）
        let mut config = KeybindingsConfig::default();
        config.bindings.insert("ctrl+c".to_string(), "copy".to_string());
        config.bindings.insert("ctrl+v".to_string(), "paste".to_string());
        
        let overridden = manager.merge_import(&config).unwrap();
        
        // 检查被覆盖的绑定
        assert_eq!(overridden.len(), 1);
        assert!(overridden[0].contains("ctrl+c"));
        assert!(overridden[0].contains("cancel"));
        
        // 检查新绑定
        assert!(manager.has_binding("ctrl+c"));
        assert!(manager.has_binding("ctrl+v"));
        assert!(manager.has_action("copy"));
        assert!(manager.has_action("paste"));
        // cancel 应该被替换
        assert!(!manager.has_action("cancel"));
        // cut 应该保留
        assert!(manager.has_binding("ctrl+x"));
        assert!(manager.has_action("cut"));
    }

    // === 额外的冲突检测和边界条件测试 ===

    #[test]
    fn test_keybinding_definition_builder() {
        let def = KeybindingDefinition::new("ctrl+s", "save", "Save file");
        assert_eq!(def.key, "ctrl+s");
        assert_eq!(def.action, "save");
        assert_eq!(def.description, "Save file");
        assert!(def.context.is_none());

        let def_with_context = KeybindingDefinition::new("escape", "cancel", "Cancel")
            .with_context("input");
        assert_eq!(def_with_context.context, Some("input".to_string()));
    }

    #[test]
    fn test_keybindings_definition_config() {
        let mut config = KeybindingsDefinitionConfig::new();
        assert!(config.bindings.is_empty());

        config.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        assert_eq!(config.bindings.len(), 1);

        let default_config = KeybindingsDefinitionConfig::default_bindings();
        assert!(!default_config.bindings.is_empty());
    }

    #[test]
    fn test_find_action_with_context() {
        use crate::keys::{Key, KeyId};

        let mut manager = KeybindingsManager::new();
        
        // 添加全局绑定
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        
        // 添加上下文特定绑定
        manager.add(KeybindingDefinition::new("escape", "close", "Close")
            .with_context("overlay"));
        manager.add(KeybindingDefinition::new("escape", "exit_insert", "Exit Insert")
            .with_context("editor/vim/insert"));

        let key_escape = Key::new(KeyId::Escape, Default::default());
        let key_ctrl_c = Key::new(KeyId::Char('c'), crate::keys::Modifiers { ctrl: true, ..Default::default() });

        // 全局绑定应该总是匹配
        assert_eq!(manager.find_action(&key_ctrl_c, None), Some("cancel"));
        assert_eq!(manager.find_action(&key_ctrl_c, Some("editor")), Some("cancel"));

        // 上下文特定绑定只在匹配上下文时返回
        assert_eq!(manager.find_action(&key_escape, None), Some("close"));
        assert_eq!(manager.find_action(&key_escape, Some("overlay")), Some("close"));
        assert_eq!(manager.find_action(&key_escape, Some("editor/vim/insert")), Some("exit_insert"));
    }

    #[test]
    fn test_find_all_actions() {
        use crate::keys::{Key, KeyId};

        let mut manager = KeybindingsManager::new();
        
        // 添加多个相同按键的绑定
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+c", "copy", "Copy"));

        let key = Key::new(KeyId::Char('c'), crate::keys::Modifiers { ctrl: true, ..Default::default() });
        let actions = manager.find_all_actions(&key, None);
        
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&"cancel"));
        assert!(actions.contains(&"copy"));
    }

    #[test]
    fn test_remove_binding() {
        let mut manager = KeybindingsManager::new();
        
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+c", "copy", "Copy"));
        manager.add(KeybindingDefinition::new("ctrl+v", "paste", "Paste"));

        assert_eq!(manager.len(), 3);

        // 移除特定绑定
        manager.remove("ctrl+c", "cancel");
        assert_eq!(manager.len(), 2);
        assert!(!manager.has_action("cancel"));
        assert!(manager.has_action("copy"));

        // 通过按键移除
        manager.remove_by_key("ctrl+c");
        assert_eq!(manager.len(), 1);
        assert!(!manager.has_binding("ctrl+c"));
        assert!(manager.has_binding("ctrl+v"));

        // 通过操作移除
        manager.remove_by_action("paste");
        assert_eq!(manager.len(), 0);
        assert!(!manager.has_action("paste"));
    }

    #[test]
    fn test_manager_clear() {
        let mut manager = KeybindingsManager::new();
        
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+v", "paste", "Paste"));
        
        assert!(!manager.is_empty());
        
        manager.clear();
        
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
        assert!(!manager.has_binding("ctrl+c"));
        assert!(!manager.has_action("cancel"));
    }

    #[test]
    fn test_get_keys_for_action() {
        let mut manager = KeybindingsManager::new();
        
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("escape", "cancel", "Cancel"));
        
        let keys = manager.get_keys_for_action("cancel").unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"ctrl+c".to_string()));
        assert!(keys.contains(&"escape".to_string()));

        // 不存在的操作
        assert!(manager.get_keys_for_action("nonexistent").is_none());
    }

    #[test]
    fn test_get_actions_for_key() {
        let mut manager = KeybindingsManager::new();
        
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+c", "copy", "Copy"));
        
        let actions = manager.get_actions_for_key("ctrl+c").unwrap();
        assert_eq!(actions.len(), 2);

        // 不存在的按键
        assert!(manager.get_actions_for_key("ctrl+nonexistent").is_none());
    }

    #[test]
    fn test_keybindings_config_empty_validation() {
        let config = KeybindingsConfig::default();
        let warnings = config.validate();
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_keybindings_config_empty_key() {
        let mut config = KeybindingsConfig::default();
        config.bindings.insert("".to_string(), "action".to_string());
        
        let warnings = config.validate();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.contains("Empty key")));
    }

    #[test]
    fn test_keybindings_config_empty_action() {
        let mut config = KeybindingsConfig::default();
        config.bindings.insert("ctrl+c".to_string(), "".to_string());
        
        let warnings = config.validate();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.contains("Empty action")));
    }

    #[test]
    fn test_preset_display() {
        assert_eq!(format!("{}", KeybindingsPreset::Emacs), "Emacs");
        assert_eq!(format!("{}", KeybindingsPreset::Vim), "Vim");
        assert_eq!(format!("{}", KeybindingsPreset::VSCode), "VSCode");
    }

    #[test]
    fn test_from_config() {
        let mut config = KeybindingsDefinitionConfig::new();
        config.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        config.add(KeybindingDefinition::new("ctrl+v", "paste", "Paste"));

        let manager = KeybindingsManager::from_config(config);
        
        assert_eq!(manager.len(), 2);
        assert!(manager.has_binding("ctrl+c"));
        assert!(manager.has_binding("ctrl+v"));
    }

    #[test]
    fn test_apply_config_preserves_existing() {
        let mut manager = KeybindingsManager::new();
        
        // 添加现有绑定
        manager.add(KeybindingDefinition::new("ctrl+x", "cut", "Cut"));
        
        // 应用只包含 ctrl+c 的配置
        let mut config = KeybindingsConfig::default();
        config.bindings.insert("ctrl+c".to_string(), "cancel".to_string());
        
        manager.apply_config(&config).unwrap();
        
        // ctrl+c 应该被添加，但 ctrl+x 应该被移除（因为 apply_config 会移除该按键的现有绑定）
        assert!(manager.has_binding("ctrl+c"));
        // 注意：apply_config 的实现会移除被覆盖的按键，但保留其他按键
    }

    #[test]
    fn test_keybindings_preset_equality() {
        assert_eq!(KeybindingsPreset::Emacs, KeybindingsPreset::Emacs);
        assert_ne!(KeybindingsPreset::Emacs, KeybindingsPreset::Vim);
        
        let presets = KeybindingsPreset::all();
        assert!(presets.contains(&KeybindingsPreset::Emacs));
        assert!(presets.contains(&KeybindingsPreset::Vim));
        assert!(presets.contains(&KeybindingsPreset::VSCode));
    }

    #[test]
    fn test_add_duplicate_binding() {
        let mut manager = KeybindingsManager::new();
        
        // 添加相同的绑定两次
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        
        // 应该只添加一次
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn test_conflict_detection_multiple() {
        let mut manager = KeybindingsManager::new();
        
        // 添加多个冲突
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+c", "copy", "Copy"));
        manager.add(KeybindingDefinition::new("ctrl+c", "cut", "Cut"));
        
        let conflicts = manager.find_conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].key, "ctrl+c");
        assert_eq!(conflicts[0].actions.len(), 3);
    }

    #[test]
    fn test_no_conflicts() {
        let mut manager = KeybindingsManager::new();
        
        manager.add(KeybindingDefinition::new("ctrl+c", "cancel", "Cancel"));
        manager.add(KeybindingDefinition::new("ctrl+v", "paste", "Paste"));
        
        let conflicts = manager.find_conflicts();
        assert!(conflicts.is_empty());
    }
}
