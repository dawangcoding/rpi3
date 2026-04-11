//! 快捷键配置视图
//!
//! 提供 TUI 界面用于查看和修改快捷键绑定

#![allow(dead_code)] // 快捷键配置视图尚未完全集成

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use pi_tui::keybindings::{get_keybindings, KeybindingsConfig, KeybindingDefinition, KeybindingsPreset, default_keybindings};

/// 快捷键条目（用于 UI 显示）
#[derive(Debug, Clone)]
pub struct KeybindingEntry {
    /// 操作名称
    pub action: String,
    /// 操作描述
    pub description: String,
    /// 当前按键显示
    pub key_display: String,
    /// 是否为默认绑定
    pub is_default: bool,
    /// 上下文（如果有）
    pub context: Option<String>,
}

/// 快捷键配置视图
pub struct KeybindingsConfigView {
    /// 绑定列表
    bindings: Vec<KeybindingEntry>,
    /// 当前选中索引
    selected_index: usize,
    /// 捕获模式（等待用户按键）
    capture_mode: bool,
    /// 等待捕获的按键
    pending_key: Option<String>,
    /// 冲突警告
    conflict_warning: Option<String>,
    /// 配置文件路径
    config_path: PathBuf,
    /// 是否已修改
    modified: bool,
    /// 滚动偏移
    scroll_offset: usize,
    /// 最大可见行数
    max_visible: usize,
    /// 是否应该退出
    should_exit: Arc<AtomicBool>,
    /// 当前预设
    current_preset: Option<KeybindingsPreset>,
    /// 预设选择模式
    preset_select_mode: bool,
    /// 预设列表选中索引
    preset_selected_index: usize,
    /// 导出模式
    export_mode: bool,
    /// 导出路径输入
    export_path: String,
    /// 导入模式
    import_mode: bool,
    /// 导入路径输入
    import_path: String,
    /// 状态消息
    status_message: Option<String>,
}

impl KeybindingsConfigView {
    /// 创建新的快捷键配置视图
    pub fn new(config_path: PathBuf) -> Self {
        let bindings = Self::load_bindings_from_manager();
        Self {
            bindings,
            selected_index: 0,
            capture_mode: false,
            pending_key: None,
            conflict_warning: None,
            config_path,
            modified: false,
            scroll_offset: 0,
            max_visible: 15,
            should_exit: Arc::new(AtomicBool::new(false)),
            current_preset: None,
            preset_select_mode: false,
            preset_selected_index: 0,
            export_mode: false,
            export_path: String::new(),
            import_mode: false,
            import_path: String::new(),
            status_message: None,
        }
    }

    /// 从全局管理器加载绑定
    fn load_bindings_from_manager() -> Vec<KeybindingEntry> {
        let defaults = default_keybindings();
        let default_bindings: std::collections::HashSet<(String, String)> = defaults
            .bindings
            .iter()
            .map(|b| (b.key.clone(), b.action.clone()))
            .collect();

        if let Ok(manager) = get_keybindings().read() {
            let all_bindings = manager.get_bindings();
            let mut entries: Vec<KeybindingEntry> = all_bindings
                .iter()
                .filter(|b| b.context.is_none()) // 只显示全局绑定
                .map(|b| {
                    let is_default = default_bindings.contains(&(b.key.clone(), b.action.clone()));
                    KeybindingEntry {
                        action: b.action.clone(),
                        description: b.description.clone(),
                        key_display: b.key.clone(),
                        is_default,
                        context: b.context.clone(),
                    }
                })
                .collect();

            // 按操作名排序
            entries.sort_by(|a, b| a.action.cmp(&b.action));
            entries
        } else {
            Vec::new()
        }
    }

    /// 渲染视图
    pub fn render(&self, width: u16, stdout: &mut impl Write) -> std::io::Result<()> {
        // 清屏
        write!(stdout, "\x1b[2J\x1b[H")?;

        // 预设选择模式
        if self.preset_select_mode {
            return self.render_preset_select(stdout);
        }

        // 导出模式
        if self.export_mode {
            return self.render_export(stdout);
        }

        // 导入模式
        if self.import_mode {
            return self.render_import(stdout);
        }

        // 标题
        writeln!(stdout, "\x1b[1m╭─ Keybindings Configuration ─╮\x1b[0m")?;
        
        // 显示当前预设
        if let Some(ref preset) = self.current_preset {
            writeln!(stdout, "\x1b[2m│ Preset: {}\x1b[0m", preset.name())?;
        }
        
        writeln!(stdout, "\x1b[2m│ Config: {}\x1b[0m", 
            truncate_path(&self.config_path, width.saturating_sub(12) as usize))?;

        // 显示状态消息
        if let Some(ref msg) = self.status_message {
            writeln!(stdout, "\x1b[32m│ {}\x1b[0m", msg)?;
        }

        if self.bindings.is_empty() {
            writeln!(stdout, "\x1b[2m│ No keybindings configured\x1b[0m")?;
        } else {
            // 计算可见范围
            let start = self.scroll_offset;
            let end = (start + self.max_visible).min(self.bindings.len());

            for i in start..end {
                let entry = &self.bindings[i];
                let is_selected = i == self.selected_index;

                // 渲染行
                let prefix = if is_selected { "→ " } else { "  " };
                let style = if is_selected { "\x1b[7m" } else { "" };
                let reset = "\x1b[0m";

                // 显示格式：按键 -> 操作
                let key_style = if entry.is_default { "\x1b[36m" } else { "\x1b[33m" };
                let default_marker = if entry.is_default { "" } else { " *" };

                writeln!(
                    stdout,
                    "{}{}{:20} {}{:15}{}{}  \x1b[2m{}",
                    prefix,
                    style,
                    entry.key_display,
                    key_style,
                    entry.action,
                    reset,
                    default_marker,
                    entry.description
                )?;
            }

            // 滚动指示器
            if self.bindings.len() > self.max_visible {
                writeln!(
                    stdout,
                    "\x1b[2m  ({}/{})\x1b[0m",
                    self.selected_index + 1,
                    self.bindings.len()
                )?;
            }
        }

        // 状态栏
        writeln!(stdout)?;
        if self.capture_mode {
            write!(stdout, "\x1b[33m  ⌨ Press a key to bind... (Esc to cancel)\x1b[0m")?;
        } else if let Some(ref warning) = self.conflict_warning {
            write!(stdout, "\x1b[31m  ⚠ {}\x1b[0m", warning)?;
        } else if self.modified {
            write!(stdout, "\x1b[33m  ● Modified (s: save, p: preset, e: export, i: import, q: quit)\x1b[0m")?;
        } else {
            write!(stdout, "\x1b[2m  ↑/↓: nav | Enter: change | d: reset | s: save | p: preset | e: export | i: import | q: quit\x1b[0m")?;
        }

        stdout.flush()
    }

    /// 渲染预设选择界面
    fn render_preset_select(&self, stdout: &mut impl Write) -> std::io::Result<()> {
        writeln!(stdout, "\x1b[1m╭─ Select Preset ─╮\x1b[0m")?;
        writeln!(stdout)?;

        let presets = KeybindingsPreset::all();
        for (i, preset) in presets.iter().enumerate() {
            let is_selected = i == self.preset_selected_index;
            let prefix = if is_selected { "→ " } else { "  " };
            let style = if is_selected { "\x1b[7m" } else { "" };
            let reset = "\x1b[0m";

            writeln!(
                stdout,
                "{}{}{:10}{} - {} style bindings",
                prefix,
                style,
                preset.name(),
                reset,
                preset.name()
            )?;
        }

        writeln!(stdout)?;
        write!(stdout, "\x1b[2m  ↑/↓: select | Enter: apply | Esc: cancel\x1b[0m")?;
        stdout.flush()
    }

    /// 渲染导出界面
    fn render_export(&self, stdout: &mut impl Write) -> std::io::Result<()> {
        writeln!(stdout, "\x1b[1m╭─ Export Keybindings ─╮\x1b[0m")?;
        writeln!(stdout)?;
        writeln!(stdout, "  Export current keybindings to a file.")?;
        writeln!(stdout)?;
        writeln!(stdout, "  File path: {}", if self.export_path.is_empty() { "~/.pi/keybindings_export.toml" } else { &self.export_path })?;
        writeln!(stdout)?;
        writeln!(stdout, "\x1b[2m  Enter file path (supports .toml or .json extension)\x1b[0m")?;
        write!(stdout, "\x1b[2m  Enter: confirm | Esc: cancel\x1b[0m")?;
        stdout.flush()
    }

    /// 渲染导入界面
    fn render_import(&self, stdout: &mut impl Write) -> std::io::Result<()> {
        writeln!(stdout, "\x1b[1m╭─ Import Keybindings ─╮\x1b[0m")?;
        writeln!(stdout)?;
        writeln!(stdout, "  Import keybindings from a file.")?;
        writeln!(stdout)?;
        writeln!(stdout, "  File path: {}", if self.import_path.is_empty() { "~/.pi/keybindings.toml" } else { &self.import_path })?;
        writeln!(stdout)?;
        writeln!(stdout, "\x1b[2m  Enter file path (supports .toml or .json extension)\x1b[0m")?;
        write!(stdout, "\x1b[2m  Enter: confirm | Esc: cancel\x1b[0m")?;
        stdout.flush()
    }

    /// 处理按键输入
    pub fn handle_input(&mut self, input: &str) -> bool {
        if self.preset_select_mode {
            self.handle_preset_select_input(input)
        } else if self.export_mode {
            self.handle_export_input(input)
        } else if self.import_mode {
            self.handle_import_input(input)
        } else if self.capture_mode {
            self.handle_capture_input(input)
        } else {
            self.handle_normal_input(input)
        }
    }

    /// 处理预设选择模式输入
    fn handle_preset_select_input(&mut self, input: &str) -> bool {
        let presets = KeybindingsPreset::all();
        
        match input {
            // 向下
            "\x1b[B" | "j" => {
                self.preset_selected_index = (self.preset_selected_index + 1) % presets.len();
                true
            }
            // 向上
            "\x1b[A" | "k" => {
                if self.preset_selected_index == 0 {
                    self.preset_selected_index = presets.len() - 1;
                } else {
                    self.preset_selected_index -= 1;
                }
                true
            }
            // Enter - 应用预设
            "\r" | "\n" => {
                let preset = presets[self.preset_selected_index];
                self.apply_preset(preset);
                self.preset_select_mode = false;
                true
            }
            // Escape - 取消
            "\x1b" => {
                self.preset_select_mode = false;
                true
            }
            _ => false,
        }
    }

    /// 处理导出模式输入
    fn handle_export_input(&mut self, input: &str) -> bool {
        match input {
            // Enter - 确认导出
            "\r" | "\n" => {
                let path = if self.export_path.is_empty() {
                    dirs::home_dir()
                        .map(|h| h.join(".pi/keybindings_export.toml"))
                        .unwrap_or_else(|| std::path::PathBuf::from("./keybindings_export.toml"))
                } else {
                    shellexpand::tilde(&self.export_path).into_owned().into()
                };
                
                match self.do_export(&path) {
                    Ok(()) => {
                        self.status_message = Some(format!("Exported to {}", path.display()));
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Export failed: {}", e));
                    }
                }
                self.export_mode = false;
                self.export_path.clear();
                true
            }
            // Escape - 取消
            "\x1b" => {
                self.export_mode = false;
                self.export_path.clear();
                true
            }
            // Backspace - 删除字符
            "\x7f" | "\x08" => {
                self.export_path.pop();
                true
            }
            // 普通字符
            _ => {
                if input.len() == 1 {
                    if let Some(c) = input.chars().next() {
                        self.export_path.push(c);
                    }
                }
                true
            }
        }
    }

    /// 处理导入模式输入
    fn handle_import_input(&mut self, input: &str) -> bool {
        match input {
            // Enter - 确认导入
            "\r" | "\n" => {
                let path = if self.import_path.is_empty() {
                    dirs::home_dir()
                        .map(|h| h.join(".pi/keybindings.toml"))
                        .unwrap_or_else(|| std::path::PathBuf::from("./keybindings.toml"))
                } else {
                    shellexpand::tilde(&self.import_path).into_owned().into()
                };
                
                match self.do_import(&path) {
                    Ok(msg) => {
                        self.status_message = Some(msg);
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Import failed: {}", e));
                    }
                }
                self.import_mode = false;
                self.import_path.clear();
                true
            }
            // Escape - 取消
            "\x1b" => {
                self.import_mode = false;
                self.import_path.clear();
                true
            }
            // Backspace - 删除字符
            "\x7f" | "\x08" => {
                self.import_path.pop();
                true
            }
            // 普通字符
            _ => {
                if input.len() == 1 {
                    if let Some(c) = input.chars().next() {
                        self.import_path.push(c);
                    }
                }
                true
            }
        }
    }

    /// 处理正常模式输入
    fn handle_normal_input(&mut self, input: &str) -> bool {
        // 清除状态消息
        self.status_message = None;
        
        match input {
            // 向下
            "\x1b[B" | "j" => {
                self.select_next();
                true
            }
            // 向上
            "\x1b[A" | "k" => {
                self.select_prev();
                true
            }
            // Enter - 进入捕获模式
            "\r" | "\n" => {
                if !self.bindings.is_empty() {
                    self.capture_mode = true;
                    self.conflict_warning = None;
                }
                true
            }
            // d - 恢复选中项为默认
            "d" => {
                self.reset_selected_to_default();
                true
            }
            // D - 恢复所有为默认
            "D" => {
                self.reset_to_defaults();
                true
            }
            // s - 保存
            "s" => {
                let _ = self.save_changes();
                true
            }
            // p - 预设选择
            "p" => {
                self.preset_select_mode = true;
                self.preset_selected_index = 0;
                true
            }
            // e - 导出
            "e" => {
                self.export_mode = true;
                self.export_path.clear();
                true
            }
            // i - 导入
            "i" => {
                self.import_mode = true;
                self.import_path.clear();
                true
            }
            // q - 退出
            "q" | "\x1b" => {
                self.should_exit.store(true, Ordering::SeqCst);
                true
            }
            _ => false,
        }
    }

    /// 处理捕获模式输入 - 采用立即提交模式
    /// 用户按第一个键（非 Esc）直接完成绑定，无需再按 Enter 确认
    fn handle_capture_input(&mut self, input: &str) -> bool {
        // Escape - 取消捕获
        if input == "\x1b" {
            self.capture_mode = false;
            self.pending_key = None;
            self.status_message = Some("Capture cancelled".to_string());
            return true;
        }

        // 解析按键
        if let Some(key) = parse_key_input(input) {
            // 检查冲突
            if let Some(entry) = self.bindings.get(self.selected_index) {
                if let Ok(manager) = get_keybindings().read() {
                    let conflicts: Vec<_> = manager.get_bindings()
                        .iter()
                        .filter(|b| b.key == key && b.action != entry.action)
                        .map(|b| b.action.clone())
                        .collect();
                    if !conflicts.is_empty() {
                        self.conflict_warning = Some(format!(
                            "'{}' is already bound to '{}' (overriding)",
                            key, conflicts.join(", ")
                        ));
                    }
                }
            }

            // 直接应用绑定
            if let Some(entry) = self.bindings.get_mut(self.selected_index) {
                entry.key_display = key.clone();
                entry.is_default = false;
                self.modified = true;

                // 更新全局管理器
                if let Ok(mut manager) = get_keybindings().write() {
                    // 移除旧绑定
                    manager.remove_by_action(&entry.action);
                    // 添加新绑定
                    manager.add(KeybindingDefinition::new(
                        key.clone(),
                        entry.action.clone(),
                        entry.description.clone(),
                    ));
                }

                self.status_message = Some(format!("Bound '{}' to '{}'", key, entry.action));
            }

            self.capture_mode = false;
            self.pending_key = None;
        }

        true
    }

    /// 检查按键冲突
    fn check_conflict(&self, key: &str) -> Option<String> {
        for entry in &self.bindings {
            if entry.key_display == key {
                return Some(entry.action.clone());
            }
        }
        None
    }

    /// 选择下一项
    fn select_next(&mut self) {
        if self.bindings.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.bindings.len();
        self.update_scroll_offset();
    }

    /// 选择上一项
    fn select_prev(&mut self) {
        if self.bindings.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.bindings.len() - 1;
        } else {
            self.selected_index -= 1;
        }
        self.update_scroll_offset();
    }

    /// 更新滚动偏移
    fn update_scroll_offset(&mut self) {
        if self.bindings.len() <= self.max_visible {
            self.scroll_offset = 0;
            return;
        }

        let half = self.max_visible / 2;
        if self.selected_index < half {
            self.scroll_offset = 0;
        } else if self.selected_index >= self.bindings.len() - half {
            self.scroll_offset = self.bindings.len() - self.max_visible;
        } else {
            self.scroll_offset = self.selected_index - half;
        }
    }

    /// 保存更改到配置文件
    pub fn save_changes(&mut self) -> anyhow::Result<()> {
        // 从管理器导出配置
        let config = if let Ok(manager) = get_keybindings().read() {
            manager.to_config()
        } else {
            KeybindingsConfig::default()
        };

        // 保存到文件
        config.save_to_file(&self.config_path)?;
        self.modified = false;
        tracing::info!("Saved keybindings to {}", self.config_path.display());
        Ok(())
    }

    /// 恢复所有快捷键为默认值
    pub fn reset_to_defaults(&mut self) {
        pi_tui::keybindings::reset_to_default_keybindings();
        self.bindings = Self::load_bindings_from_manager();
        self.modified = true;
    }

    /// 恢复选中快捷键为默认值
    fn reset_selected_to_default(&mut self) {
        if let Some(entry) = self.bindings.get_mut(self.selected_index) {
            // 从默认配置中查找
            let defaults = default_keybindings();
            if let Some(default) = defaults.bindings.iter().find(|b| b.action == entry.action) {
                entry.key_display = default.key.clone();
                entry.is_default = true;
                self.modified = true;

                // 更新全局管理器
                if let Ok(mut manager) = get_keybindings().write() {
                    manager.remove_by_action(&entry.action);
                    manager.add(default.clone());
                }
            }
        }
    }

    /// 检查是否应该退出
    pub fn should_exit(&self) -> bool {
        self.should_exit.load(Ordering::SeqCst)
    }

    /// 获取是否已修改
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// 应用预设方案
    fn apply_preset(&mut self, preset: KeybindingsPreset) {
        if let Ok(mut manager) = get_keybindings().write() {
            let _ = manager.apply_preset(preset);
        }
        self.current_preset = Some(preset);
        self.bindings = Self::load_bindings_from_manager();
        self.modified = true;
        self.status_message = Some(format!("Applied {} preset", preset.name()));
    }

    /// 执行导出
    fn do_export(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let config = if let Ok(manager) = get_keybindings().read() {
            manager.to_config()
        } else {
            KeybindingsConfig::default()
        };
        config.export_to_file(path)?;
        tracing::info!("Exported keybindings to {}", path.display());
        Ok(())
    }

    /// 执行导入
    fn do_import(&mut self, path: &std::path::Path) -> anyhow::Result<String> {
        let config = KeybindingsConfig::import_from_file(path)?;
        
        let overridden = if let Ok(mut manager) = get_keybindings().write() {
            manager.merge_import(&config)?
        } else {
            Vec::new()
        };
        
        self.bindings = Self::load_bindings_from_manager();
        self.modified = true;
        
        if overridden.is_empty() {
            Ok(format!("Imported {} bindings from {}", config.bindings.len(), path.display()))
        } else {
            Ok(format!("Imported {} bindings, {} overridden", config.bindings.len(), overridden.len()))
        }
    }
}

/// 解析用户输入的按键
fn parse_key_input(input: &str) -> Option<String> {
    // 特殊键
    match input {
        "\r" | "\n" => return Some("enter".to_string()),
        "\x1b" => return Some("escape".to_string()),
        "\x7f" | "\x08" => return Some("backspace".to_string()),
        "\t" => return Some("tab".to_string()),
        _ => {}
    }

    // 方向键
    if input == "\x1b[A" { return Some("up".to_string()); }
    if input == "\x1b[B" { return Some("down".to_string()); }
    if input == "\x1b[C" { return Some("right".to_string()); }
    if input == "\x1b[D" { return Some("left".to_string()); }

    // 功能键 F1-F12
    for i in 1..=12 {
        if input == format!("\x1b[{}~", i + 10) || input == format!("\x1bOP{}", i) {
            return Some(format!("f{}", i));
        }
    }

    // Ctrl 组合键
    if input.len() == 1 {
        let c = input.chars().next()?;
        let code = c as u8;
        // Ctrl 字符范围 0x00-0x1f
        if code < 0x20 {
            let letter = (code + 0x40) as char;
            return Some(format!("ctrl+{}", letter.to_lowercase()));
        }
        // 普通字符
        if c.is_alphanumeric() || c.is_ascii_punctuation() {
            return Some(c.to_string());
        }
    }

    // Alt 组合键 (ESC + char)
    if input.starts_with("\x1b") && input.len() == 2 {
        let c = input.chars().nth(1)?;
        return Some(format!("alt+{}", c.to_lowercase()));
    }

    None
}

/// 截断路径显示
fn truncate_path(path: &Path, max_len: usize) -> String {
    let s = path.display().to_string();
    if s.len() <= max_len {
        s
    } else {
        format!("...{}", &s[s.len().saturating_sub(max_len - 3)..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_input_special() {
        assert_eq!(parse_key_input("\r"), Some("enter".to_string()));
        assert_eq!(parse_key_input("\x1b"), Some("escape".to_string()));
        assert_eq!(parse_key_input("\t"), Some("tab".to_string()));
        assert_eq!(parse_key_input("\x7f"), Some("backspace".to_string()));
    }

    #[test]
    fn test_parse_key_input_arrows() {
        assert_eq!(parse_key_input("\x1b[A"), Some("up".to_string()));
        assert_eq!(parse_key_input("\x1b[B"), Some("down".to_string()));
        assert_eq!(parse_key_input("\x1b[C"), Some("right".to_string()));
        assert_eq!(parse_key_input("\x1b[D"), Some("left".to_string()));
    }

    #[test]
    fn test_parse_key_input_ctrl() {
        // Ctrl+A = 0x01
        assert_eq!(parse_key_input("\x01"), Some("ctrl+a".to_string()));
        // Ctrl+C = 0x03
        assert_eq!(parse_key_input("\x03"), Some("ctrl+c".to_string()));
    }

    #[test]
    fn test_parse_key_input_char() {
        assert_eq!(parse_key_input("a"), Some("a".to_string()));
        assert_eq!(parse_key_input("Z"), Some("Z".to_string()));
        assert_eq!(parse_key_input("1"), Some("1".to_string()));
    }

    #[test]
    fn test_parse_key_input_alt() {
        // Alt+a = ESC + 'a'
        assert_eq!(parse_key_input("\x1ba"), Some("alt+a".to_string()));
    }

    #[test]
    fn test_truncate_path() {
        let path = PathBuf::from("/Users/test/.pi/keybindings.toml");
        let truncated = truncate_path(&path, 20);
        assert!(truncated.len() <= 20);
        assert!(truncated.ends_with(".toml"));
    }
}
