//! 键盘输入处理模块
//!
//! 支持标准 ANSI CSI 序列和 Kitty keyboard protocol
//! 参考: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>

use std::sync::atomic::{AtomicBool, Ordering};

// =============================================================================
// 全局 Kitty Protocol 状态
// =============================================================================

static KITTY_PROTOCOL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 设置全局 Kitty keyboard protocol 状态
pub fn set_kitty_protocol_active(active: bool) {
    KITTY_PROTOCOL_ACTIVE.store(active, Ordering::SeqCst);
}

/// 查询 Kitty keyboard protocol 是否当前激活
pub fn is_kitty_protocol_active() -> bool {
    KITTY_PROTOCOL_ACTIVE.load(Ordering::SeqCst)
}

// =============================================================================
// 类型定义
// =============================================================================

/// 按键事件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum KeyEventType {
    /// 按下
    #[default]
    Press,
    /// 重复
    Repeat,
    /// 释放
    Release,
}


/// 按键标识符
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyId {
    /// 字符键
    Char(char),
    /// 回车键
    Enter,
    /// Tab 键
    Tab,
    /// 退格键
    Backspace,
    /// 删除键
    Delete,
    /// Esc 键
    Escape,
    /// 上方向键
    Up,
    /// 下方向键
    Down,
    /// 左方向键
    Left,
    /// 右方向键
    Right,
    /// Home 键
    Home,
    /// End 键
    End,
    /// Page Up 键
    PageUp,
    /// Page Down 键
    PageDown,
    /// Insert 键
    Insert,
    /// F1-F24 功能键
    F(u8),
    /// 空格键
    Space,
    /// Clear 键
    Clear,
}

impl std::fmt::Display for KeyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyId::Char(c) => write!(f, "{}", c),
            KeyId::Enter => write!(f, "enter"),
            KeyId::Tab => write!(f, "tab"),
            KeyId::Backspace => write!(f, "backspace"),
            KeyId::Delete => write!(f, "delete"),
            KeyId::Escape => write!(f, "escape"),
            KeyId::Up => write!(f, "up"),
            KeyId::Down => write!(f, "down"),
            KeyId::Left => write!(f, "left"),
            KeyId::Right => write!(f, "right"),
            KeyId::Home => write!(f, "home"),
            KeyId::End => write!(f, "end"),
            KeyId::PageUp => write!(f, "pageUp"),
            KeyId::PageDown => write!(f, "pageDown"),
            KeyId::Insert => write!(f, "insert"),
            KeyId::F(n) => write!(f, "f{}", n),
            KeyId::Space => write!(f, "space"),
            KeyId::Clear => write!(f, "clear"),
        }
    }
}

/// 修饰符标志
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Shift 键
    pub shift: bool,
    /// Ctrl 键
    pub ctrl: bool,
    /// Alt 键
    pub alt: bool,
    /// Meta 键（Super/Win/Cmd）
    pub meta: bool,
}

impl Modifiers {
    /// 从 CSI 修饰符值解析 (1-indexed, bit flags)
    /// shift=1, alt=2, ctrl=4, meta=8
    pub fn from_csi_value(value: u8) -> Self {
        let modifier = value.saturating_sub(1);
        Self {
            shift: modifier & 1 != 0,
            alt: modifier & 2 != 0,
            ctrl: modifier & 4 != 0,
            meta: modifier & 8 != 0,
        }
    }

    /// 转换为 CSI 修饰符值
    pub fn to_csi_value(&self) -> u8 {
        // CSI 修饰符值是 1-indexed 的
        // 基础值 1 表示无修饰符
        // shift=1, alt=2, ctrl=4, meta=8 是 bit flags
        let mut value = 1;
        if self.shift {
            value += 1; // bit 0
        }
        if self.alt {
            value += 2; // bit 1
        }
        if self.ctrl {
            value += 4; // bit 2
        }
        if self.meta {
            value += 8; // bit 3
        }
        value
    }

    /// 检查是否为空（无修饰符）
    pub fn is_empty(&self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.meta
    }
}

/// 按键事件
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// 按键标识符
    pub id: KeyId,
    /// 修饰符
    pub modifiers: Modifiers,
    /// 事件类型
    pub event_type: KeyEventType,
    /// 原始字节
    pub raw: String,
}

impl Key {
    /// 创建新的按键事件
    pub fn new(id: KeyId, modifiers: Modifiers) -> Self {
        Self {
            id,
            modifiers,
            event_type: KeyEventType::Press,
            raw: String::new(),
        }
    }

    /// 设置事件类型
    pub fn with_event_type(mut self, event_type: KeyEventType) -> Self {
        self.event_type = event_type;
        self
    }

    /// 设置原始数据
    pub fn with_raw(mut self, raw: impl Into<String>) -> Self {
        self.raw = raw.into();
        self
    }
}

// =============================================================================
// 常量定义
// =============================================================================

/// 修饰符位标志
const MOD_SHIFT: u8 = 1;
const MOD_ALT: u8 = 2;
const MOD_CTRL: u8 = 4;
const MOD_META: u8 = 8;

/// Lock 掩码 (Caps Lock + Num Lock)
const LOCK_MASK: u8 = 64 + 128;

/// 特殊码点
const CODEPOINT_ESCAPE: u32 = 27;
const CODEPOINT_TAB: u32 = 9;
const CODEPOINT_ENTER: u32 = 13;
const CODEPOINT_SPACE: u32 = 32;
const CODEPOINT_BACKSPACE: u32 = 127;
const CODEPOINT_KP_ENTER: u32 = 57414; // Numpad Enter (Kitty protocol)

/// 方向键码点 (内部表示)
const ARROW_UP: i32 = -1;
const ARROW_DOWN: i32 = -2;
const ARROW_RIGHT: i32 = -3;
const ARROW_LEFT: i32 = -4;

/// 功能键码点 (内部表示)
const FN_DELETE: i32 = -10;
const FN_INSERT: i32 = -11;
const FN_PAGE_UP: i32 = -12;
const FN_PAGE_DOWN: i32 = -13;
const FN_HOME: i32 = -14;
const FN_END: i32 = -15;

/// 符号键集合
const SYMBOL_KEYS: &[char] = &[
    '`', '-', '=', '[', ']', '\\', ';', '\'', ',', '.', '/', '!', '@', '#', '$', '%', '^', '&',
    '*', '(', ')', '_', '+', '|', '~', '{', '}', ':', '<', '>', '?',
];

/// Kitty 功能键等效映射
fn normalize_kitty_functional_codepoint(codepoint: u32) -> u32 {
    match codepoint {
        57399 => 48,  // KP_0 -> 0
        57400 => 49,  // KP_1 -> 1
        57401 => 50,  // KP_2 -> 2
        57402 => 51,  // KP_3 -> 3
        57403 => 52,  // KP_4 -> 4
        57404 => 53,  // KP_5 -> 5
        57405 => 54,  // KP_6 -> 6
        57406 => 55,  // KP_7 -> 7
        57407 => 56,  // KP_8 -> 8
        57408 => 57,  // KP_9 -> 9
        57409 => 46,  // KP_DECIMAL -> .
        57410 => 47,  // KP_DIVIDE -> /
        57411 => 42,  // KP_MULTIPLY -> *
        57412 => 45,  // KP_SUBTRACT -> -
        57413 => 43,  // KP_ADD -> +
        57415 => 61,  // KP_EQUAL -> =
        57416 => 44,  // KP_SEPARATOR -> ,
        57417 => ARROW_LEFT as u32,
        57418 => ARROW_RIGHT as u32,
        57419 => ARROW_UP as u32,
        57420 => ARROW_DOWN as u32,
        57421 => FN_PAGE_UP as u32,
        57422 => FN_PAGE_DOWN as u32,
        57423 => FN_HOME as u32,
        57424 => FN_END as u32,
        57425 => FN_INSERT as u32,
        57426 => FN_DELETE as u32,
        _ => codepoint,
    }
}

/// 检查字符是否是符号键
fn is_symbol_key(c: char) -> bool {
    SYMBOL_KEYS.contains(&c)
}

/// 检查字符是否是拉丁字母
#[allow(dead_code)] // 预留函数供未来使用
fn is_latin_letter(c: char) -> bool {
    c.is_ascii_lowercase()
}

/// 检查字符是否是数字
#[allow(dead_code)] // 预留函数供未来使用
fn is_digit_key(c: char) -> bool {
    c.is_ascii_digit()
}

// =============================================================================
// 解析 Kitty 序列
// =============================================================================

/// 解析的事件类型
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // 解析结构体字段供未来使用
struct ParsedKittySequence {
    codepoint: u32,
    shifted_key: Option<u32>,
    base_layout_key: Option<u32>,
    modifier: u8,
    event_type: KeyEventType,
}

/// 解析 Kitty CSI-u 序列
fn parse_kitty_sequence(data: &str) -> Option<ParsedKittySequence> {
    // CSI u format with alternate keys (flag 4):
    // \x1b[<codepoint>u
    // \x1b[<codepoint>;<mod>u
    // \x1b[<codepoint>;<mod>:<event>u
    // \x1b[<codepoint>:<shifted>;<mod>u
    // \x1b[<codepoint>:<shifted>:<base>;<mod>u
    // \x1b[<codepoint>::<base>;<mod>u (no shifted key, only base)

    // 尝试匹配 CSI u 序列
    if let Some(rest) = data.strip_prefix("\x1b[") {
        if let Some(params) = rest.strip_suffix('u') {
            let parts: Vec<&str> = params.split(';').collect();

            // 解析 codepoint 和可选的 shifted/base keys
            let codepoint_part = parts.first()?;
            let codepoint_parts: Vec<&str> = codepoint_part.split(':').collect();

            let codepoint = codepoint_parts.first()?.parse::<u32>().ok()?;
            let shifted_key = codepoint_parts
                .get(1)
                .and_then(|s| if s.is_empty() { None } else { s.parse::<u32>().ok() });
            let base_layout_key = codepoint_parts.get(2).and_then(|s| s.parse::<u32>().ok());

            // 解析修饰符和事件类型
            let (modifier, event_type) = if parts.len() > 1 {
                let mod_part = parts[1];
                let mod_parts: Vec<&str> = mod_part.split(':').collect();
                let mod_value = mod_parts.first().and_then(|s| s.parse::<u8>().ok()).unwrap_or(1);
                let event_type = mod_parts
                    .get(1)
                    .and_then(|s| s.parse::<u8>().ok())
                    .map(|e| match e {
                        2 => KeyEventType::Repeat,
                        3 => KeyEventType::Release,
                        _ => KeyEventType::Press,
                    })
                    .unwrap_or(KeyEventType::Press);
                (mod_value.saturating_sub(1), event_type)
            } else {
                (0, KeyEventType::Press)
            };

            return Some(ParsedKittySequence {
                codepoint,
                shifted_key,
                base_layout_key,
                modifier,
                event_type,
            });
        }
    }

    // 方向键带修饰符: \x1b[1;<mod>A/B/C/D 或 \x1b[1;<mod>:<event>A/B/C/D
    if let Some(rest) = data.strip_prefix("\x1b[1;") {
        let arrow_codes: std::collections::HashMap<char, i32> = [
            ('A', ARROW_UP),
            ('B', ARROW_DOWN),
            ('C', ARROW_RIGHT),
            ('D', ARROW_LEFT),
        ]
        .into_iter()
        .collect();

        for (arrow_char, arrow_code) in &arrow_codes {
            if let Some(prefix) = rest.strip_suffix(*arrow_char) {
                let parts: Vec<&str> = prefix.split(':').collect();
                if let Ok(mod_value) = parts.first()?.parse::<u8>() {
                    let event_type = parts
                        .get(1)
                        .and_then(|s| s.parse::<u8>().ok())
                        .map(|e| match e {
                            2 => KeyEventType::Repeat,
                            3 => KeyEventType::Release,
                            _ => KeyEventType::Press,
                        })
                        .unwrap_or(KeyEventType::Press);

                    return Some(ParsedKittySequence {
                        codepoint: *arrow_code as u32,
                        shifted_key: None,
                        base_layout_key: None,
                        modifier: mod_value.saturating_sub(1),
                        event_type,
                    });
                }
            }
        }
    }

    // 功能键: \x1b[<num>~ 或 \x1b[<num>;<mod>~ 或 \x1b[<num>;<mod>:<event>~
    if let Some(rest) = data.strip_prefix("\x1b[") {
        if let Some(prefix) = rest.strip_suffix('~') {
            let parts: Vec<&str> = prefix.split(';').collect();
            if let Ok(key_num) = parts.first()?.parse::<u32>() {
                let func_codes: std::collections::HashMap<u32, i32> = [
                    (2, FN_INSERT),
                    (3, FN_DELETE),
                    (5, FN_PAGE_UP),
                    (6, FN_PAGE_DOWN),
                    (7, FN_HOME),
                    (8, FN_END),
                ]
                .into_iter()
                .collect();

                if let Some(&codepoint) = func_codes.get(&key_num) {
                    let mod_value = parts.get(1).and_then(|s| s.split(':').next()).and_then(|s| s.parse::<u8>().ok()).unwrap_or(1);
                    let event_type = parts
                        .get(1)
                        .and_then(|s| s.split(':').nth(1))
                        .and_then(|s| s.parse::<u8>().ok())
                        .map(|e| match e {
                            2 => KeyEventType::Repeat,
                            3 => KeyEventType::Release,
                            _ => KeyEventType::Press,
                        })
                        .unwrap_or(KeyEventType::Press);

                    return Some(ParsedKittySequence {
                        codepoint: codepoint as u32,
                        shifted_key: None,
                        base_layout_key: None,
                        modifier: mod_value.saturating_sub(1),
                        event_type,
                    });
                }
            }
        }
    }

    // Home/End 带修饰符: \x1b[1;<mod>H/F 或 \x1b[1;<mod>:<event>H/F
    if let Some(rest) = data.strip_prefix("\x1b[1;") {
        let home_end_codes: std::collections::HashMap<char, i32> =
            [('H', FN_HOME), ('F', FN_END)].into_iter().collect();

        for (end_char, end_code) in &home_end_codes {
            if let Some(prefix) = rest.strip_suffix(*end_char) {
                let parts: Vec<&str> = prefix.split(':').collect();
                if let Ok(mod_value) = parts.first()?.parse::<u8>() {
                    let event_type = parts
                        .get(1)
                        .and_then(|s| s.parse::<u8>().ok())
                        .map(|e| match e {
                            2 => KeyEventType::Repeat,
                            3 => KeyEventType::Release,
                            _ => KeyEventType::Press,
                        })
                        .unwrap_or(KeyEventType::Press);

                    return Some(ParsedKittySequence {
                        codepoint: *end_code as u32,
                        shifted_key: None,
                        base_layout_key: None,
                        modifier: mod_value.saturating_sub(1),
                        event_type,
                    });
                }
            }
        }
    }

    None
}

/// 解析 xterm modifyOtherKeys 序列: CSI 27 ; modifiers ; keycode ~
fn parse_modify_other_keys_sequence(data: &str) -> Option<(u32, u8)> {
    if let Some(rest) = data.strip_prefix("\x1b[27;") {
        if let Some(prefix) = rest.strip_suffix('~') {
            let parts: Vec<&str> = prefix.split(';').collect();
            if parts.len() == 2 {
                let mod_value = parts[0].parse::<u8>().ok()?;
                let codepoint = parts[1].parse::<u32>().ok()?;
                return Some((codepoint, mod_value.saturating_sub(1)));
            }
        }
    }
    None
}

// =============================================================================
// 核心解析函数
// =============================================================================

/// 从原始输入字节解析按键
/// 支持标准 ANSI CSI 序列和 Kitty keyboard protocol
pub fn parse_key(data: &str) -> Option<Key> {
    // 首先尝试 Kitty 序列
    if let Some(kitty) = parse_kitty_sequence(data) {
        let key_id = codepoint_to_key_id(kitty.codepoint, kitty.base_layout_key)?;
        let modifiers = Modifiers {
            shift: kitty.modifier & MOD_SHIFT != 0,
            alt: kitty.modifier & MOD_ALT != 0,
            ctrl: kitty.modifier & MOD_CTRL != 0,
            meta: kitty.modifier & MOD_META != 0,
        };
        return Some(
            Key::new(key_id, modifiers)
                .with_event_type(kitty.event_type)
                .with_raw(data),
        );
    }

    // 尝试 modifyOtherKeys 序列
    if let Some((codepoint, modifier)) = parse_modify_other_keys_sequence(data) {
        let key_id = codepoint_to_key_id(codepoint, None)?;
        let modifiers = Modifiers {
            shift: modifier & MOD_SHIFT != 0,
            alt: modifier & MOD_ALT != 0,
            ctrl: modifier & MOD_CTRL != 0,
            meta: modifier & MOD_META != 0,
        };
        return Some(Key::new(key_id, modifiers).with_raw(data));
    }

    // 模式感知的遗留序列
    let kitty_active = is_kitty_protocol_active();

    // 当 Kitty protocol 激活时，某些序列有特殊含义
    if kitty_active
        && (data == "\x1b\r" || data == "\n") {
            return Some(
                Key::new(KeyId::Enter, Modifiers { shift: true, ..Default::default() }).with_raw(data),
            );
        }

    // 遗留序列映射
    let legacy_map: std::collections::HashMap<&str, (KeyId, Modifiers)> = [
        ("\x1b", (KeyId::Escape, Modifiers::default())),
        ("\x1b\x1b", (KeyId::Char('['), Modifiers { alt: true, ctrl: true, ..Default::default() })),
        ("\x1b\x1c", (KeyId::Char('\\'), Modifiers { alt: true, ctrl: true, ..Default::default() })),
        ("\x1b\x1d", (KeyId::Char(']'), Modifiers { alt: true, ctrl: true, ..Default::default() })),
        ("\x1b\x1f", (KeyId::Char('-'), Modifiers { alt: true, ctrl: true, ..Default::default() })),
        ("\t", (KeyId::Tab, Modifiers::default())),
        ("\r", (KeyId::Enter, Modifiers::default())),
        ("\x1bOM", (KeyId::Enter, Modifiers::default())), // SS3 M (numpad enter)
        ("\x00", (KeyId::Space, Modifiers { ctrl: true, ..Default::default() })),
        (" ", (KeyId::Space, Modifiers::default())),
        ("\x7f", (KeyId::Backspace, Modifiers::default())),
        ("\x08", (KeyId::Backspace, if is_windows_terminal() { Modifiers { ctrl: true, ..Default::default() } } else { Modifiers::default() })),
        ("\x1b[Z", (KeyId::Tab, Modifiers { shift: true, ..Default::default() })),
        ("\x1b\x7f", (KeyId::Backspace, Modifiers { alt: true, ..Default::default() })),
        ("\x1b\x08", (KeyId::Backspace, Modifiers { alt: true, ..Default::default() })),
        // 方向键
        ("\x1b[A", (KeyId::Up, Modifiers::default())),
        ("\x1b[B", (KeyId::Down, Modifiers::default())),
        ("\x1b[C", (KeyId::Right, Modifiers::default())),
        ("\x1b[D", (KeyId::Left, Modifiers::default())),
        // SS3 方向键
        ("\x1bOA", (KeyId::Up, Modifiers::default())),
        ("\x1bOB", (KeyId::Down, Modifiers::default())),
        ("\x1bOC", (KeyId::Right, Modifiers::default())),
        ("\x1bOD", (KeyId::Left, Modifiers::default())),
        // Home/End
        ("\x1b[H", (KeyId::Home, Modifiers::default())),
        ("\x1b[F", (KeyId::End, Modifiers::default())),
        ("\x1bOH", (KeyId::Home, Modifiers::default())),
        ("\x1bOF", (KeyId::End, Modifiers::default())),
        // 功能键
        ("\x1b[2~", (KeyId::Insert, Modifiers::default())),
        ("\x1b[3~", (KeyId::Delete, Modifiers::default())),
        ("\x1b[5~", (KeyId::PageUp, Modifiers::default())),
        ("\x1b[6~", (KeyId::PageDown, Modifiers::default())),
        // 带修饰符的遗留序列
        ("\x1b[1;2A", (KeyId::Up, Modifiers { shift: true, ..Default::default() })),
        ("\x1b[1;2B", (KeyId::Down, Modifiers { shift: true, ..Default::default() })),
        ("\x1b[1;2C", (KeyId::Right, Modifiers { shift: true, ..Default::default() })),
        ("\x1b[1;2D", (KeyId::Left, Modifiers { shift: true, ..Default::default() })),
        ("\x1b[1;3A", (KeyId::Up, Modifiers { alt: true, ..Default::default() })),
        ("\x1b[1;3B", (KeyId::Down, Modifiers { alt: true, ..Default::default() })),
        ("\x1b[1;3C", (KeyId::Right, Modifiers { alt: true, ..Default::default() })),
        ("\x1b[1;3D", (KeyId::Left, Modifiers { alt: true, ..Default::default() })),
        ("\x1b[1;5A", (KeyId::Up, Modifiers { ctrl: true, ..Default::default() })),
        ("\x1b[1;5B", (KeyId::Down, Modifiers { ctrl: true, ..Default::default() })),
        ("\x1b[1;5C", (KeyId::Right, Modifiers { ctrl: true, ..Default::default() })),
        ("\x1b[1;5D", (KeyId::Left, Modifiers { ctrl: true, ..Default::default() })),
        ("\x1b[1;2H", (KeyId::Home, Modifiers { shift: true, ..Default::default() })),
        ("\x1b[1;2F", (KeyId::End, Modifiers { shift: true, ..Default::default() })),
        ("\x1b[1;5H", (KeyId::Home, Modifiers { ctrl: true, ..Default::default() })),
        ("\x1b[1;5F", (KeyId::End, Modifiers { ctrl: true, ..Default::default() })),
        // SS3 带修饰符
        ("\x1bOa", (KeyId::Up, Modifiers { ctrl: true, ..Default::default() })),
        ("\x1bOb", (KeyId::Down, Modifiers { ctrl: true, ..Default::default() })),
        ("\x1bOc", (KeyId::Right, Modifiers { ctrl: true, ..Default::default() })),
        ("\x1bOd", (KeyId::Left, Modifiers { ctrl: true, ..Default::default() })),
        // 功能键 F1-F12
        ("\x1bOP", (KeyId::F(1), Modifiers::default())),
        ("\x1bOQ", (KeyId::F(2), Modifiers::default())),
        ("\x1bOR", (KeyId::F(3), Modifiers::default())),
        ("\x1bOS", (KeyId::F(4), Modifiers::default())),
        ("\x1b[15~", (KeyId::F(5), Modifiers::default())),
        ("\x1b[17~", (KeyId::F(6), Modifiers::default())),
        ("\x1b[18~", (KeyId::F(7), Modifiers::default())),
        ("\x1b[19~", (KeyId::F(8), Modifiers::default())),
        ("\x1b[20~", (KeyId::F(9), Modifiers::default())),
        ("\x1b[21~", (KeyId::F(10), Modifiers::default())),
        ("\x1b[23~", (KeyId::F(11), Modifiers::default())),
        ("\x1b[24~", (KeyId::F(12), Modifiers::default())),
        // Alt+方向键 (遗留)
        ("\x1bb", (KeyId::Left, Modifiers { alt: true, ..Default::default() })),
        ("\x1bf", (KeyId::Right, Modifiers { alt: true, ..Default::default() })),
        ("\x1bp", (KeyId::Up, Modifiers { alt: true, ..Default::default() })),
        ("\x1bn", (KeyId::Down, Modifiers { alt: true, ..Default::default() })),
    ]
    .into_iter()
    .collect();

    if let Some((key_id, modifiers)) = legacy_map.get(data).cloned() {
        return Some(Key::new(key_id, modifiers).with_raw(data));
    }

    // 非 Kitty 模式下的特殊序列
    if !kitty_active {
        if data == "\n" {
            return Some(Key::new(KeyId::Enter, Modifiers::default()).with_raw(data));
        }
        if data == "\x1b\r" {
            return Some(Key::new(KeyId::Enter, Modifiers { alt: true, ..Default::default() }).with_raw(data));
        }
        if data == "\x1b " {
            return Some(Key::new(KeyId::Space, Modifiers { alt: true, ..Default::default() }).with_raw(data));
        }
        if data == "\x1bB" {
            return Some(Key::new(KeyId::Left, Modifiers { alt: true, ..Default::default() }).with_raw(data));
        }
        if data == "\x1bF" {
            return Some(Key::new(KeyId::Right, Modifiers { alt: true, ..Default::default() }).with_raw(data));
        }

        // Alt+字母/数字 (ESC 后跟字符)
        if data.len() == 2 && data.starts_with('\x1b') {
            let ch = data.chars().nth(1)?;
            let code = ch as u32;
            // Ctrl+Alt+字母 (code 1-26)
            if (1..=26).contains(&code) {
                let letter = (code + 96) as u8 as char;
                return Some(
                    Key::new(KeyId::Char(letter), Modifiers { ctrl: true, alt: true, ..Default::default() })
                        .with_raw(data),
                );
            }
            // Alt+字母/数字
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
                return Some(Key::new(KeyId::Char(ch), Modifiers { alt: true, ..Default::default() }).with_raw(data));
            }
        }
    }

    // 单个字符
    if data.len() == 1 {
        let ch = data.chars().next()?;
        let code = ch as u32;

        // Ctrl+字母 (code 1-26)
        if (1..=26).contains(&code) {
            let letter = (code + 96) as u8 as char;
            return Some(
                Key::new(KeyId::Char(letter), Modifiers { ctrl: true, ..Default::default() }).with_raw(data),
            );
        }

        // 可打印字符 (32-126)
        if (32..=126).contains(&code) {
            return Some(Key::new(KeyId::Char(ch), Modifiers::default()).with_raw(data));
        }
    }

    None
}

/// 将码点转换为 KeyId
fn codepoint_to_key_id(codepoint: u32, base_layout_key: Option<u32>) -> Option<KeyId> {
    let normalized = normalize_kitty_functional_codepoint(codepoint);

    // 检查是否是拉丁字母、数字或符号
    let is_latin = (97..=122).contains(&normalized); // a-z
    let is_digit = (48..=57).contains(&normalized); // 0-9
    let is_known_symbol = (normalized as u8 as char).is_ascii() && is_symbol_key(normalized as u8 as char);

    // 使用 base layout key 只有当 codepoint 不是已知的拉丁字母、数字或符号
    let effective_codepoint = if is_latin || is_digit || is_known_symbol {
        normalized
    } else {
        base_layout_key.unwrap_or(normalized)
    };

    match effective_codepoint {
        CODEPOINT_ESCAPE => Some(KeyId::Escape),
        CODEPOINT_TAB => Some(KeyId::Tab),
        CODEPOINT_ENTER | CODEPOINT_KP_ENTER => Some(KeyId::Enter),
        CODEPOINT_SPACE => Some(KeyId::Space),
        CODEPOINT_BACKSPACE => Some(KeyId::Backspace),
        cp if cp == FN_DELETE as u32 => Some(KeyId::Delete),
        cp if cp == FN_INSERT as u32 => Some(KeyId::Insert),
        cp if cp == FN_HOME as u32 => Some(KeyId::Home),
        cp if cp == FN_END as u32 => Some(KeyId::End),
        cp if cp == FN_PAGE_UP as u32 => Some(KeyId::PageUp),
        cp if cp == FN_PAGE_DOWN as u32 => Some(KeyId::PageDown),
        cp if cp == ARROW_UP as u32 => Some(KeyId::Up),
        cp if cp == ARROW_DOWN as u32 => Some(KeyId::Down),
        cp if cp == ARROW_LEFT as u32 => Some(KeyId::Left),
        cp if cp == ARROW_RIGHT as u32 => Some(KeyId::Right),
        cp if (48..=57).contains(&cp) => Some(KeyId::Char(cp as u8 as char)), // 0-9
        cp if (97..=122).contains(&cp) => Some(KeyId::Char(cp as u8 as char)), // a-z
        cp if is_symbol_key(cp as u8 as char) => Some(KeyId::Char(cp as u8 as char)),
        _ => None,
    }
}

/// 检查是否是 Windows Terminal 会话
fn is_windows_terminal() -> bool {
    // 检查 WT_SESSION 环境变量
    std::env::var("WT_SESSION").is_ok()
        && std::env::var("SSH_CONNECTION").is_err()
        && std::env::var("SSH_CLIENT").is_err()
        && std::env::var("SSH_TTY").is_err()
}

// =============================================================================
// 按键匹配函数
// =============================================================================

/// 解析按键描述字符串（如 "ctrl+c", "shift+enter"）
fn parse_key_description(description: &str) -> Option<(String, Modifiers)> {
    let parts: Vec<&str> = description.split('+').collect();
    if parts.is_empty() {
        return None;
    }

    let key = parts.last()?.to_lowercase();
    let modifiers = Modifiers {
        shift: parts.iter().any(|p| p.eq_ignore_ascii_case("shift")),
        ctrl: parts.iter().any(|p| p.eq_ignore_ascii_case("ctrl")),
        alt: parts.iter().any(|p| p.eq_ignore_ascii_case("alt")),
        meta: parts.iter().any(|p| p.eq_ignore_ascii_case("meta") || p.eq_ignore_ascii_case("super") || p.eq_ignore_ascii_case("cmd")),
    };

    Some((key, modifiers))
}

/// 检查按键是否匹配描述字符串（如 "ctrl+c", "shift+enter"）
pub fn matches_key(key: &Key, description: &str) -> bool {
    let (expected_key, expected_mods) = match parse_key_description(description) {
        Some((k, m)) => (k, m),
        None => return false,
    };

    // 检查修饰符是否匹配
    if key.modifiers.shift != expected_mods.shift
        || key.modifiers.ctrl != expected_mods.ctrl
        || key.modifiers.alt != expected_mods.alt
        || key.modifiers.meta != expected_mods.meta
    {
        return false;
    }

    // 检查按键 ID 是否匹配
    let key_matches = match &key.id {
        KeyId::Char(c) => {
            let c_lower = c.to_lowercase().next().unwrap_or(*c);
            expected_key == c_lower.to_string()
        }
        KeyId::Enter => expected_key == "enter" || expected_key == "return",
        KeyId::Tab => expected_key == "tab",
        KeyId::Backspace => expected_key == "backspace",
        KeyId::Delete => expected_key == "delete",
        KeyId::Escape | KeyId::Clear => expected_key == "escape" || expected_key == "esc" || expected_key == "clear",
        KeyId::Up => expected_key == "up",
        KeyId::Down => expected_key == "down",
        KeyId::Left => expected_key == "left",
        KeyId::Right => expected_key == "right",
        KeyId::Home => expected_key == "home",
        KeyId::End => expected_key == "end",
        KeyId::PageUp => expected_key == "pageup" || expected_key == "page_up",
        KeyId::PageDown => expected_key == "pagedown" || expected_key == "page_down",
        KeyId::Insert => expected_key == "insert",
        KeyId::F(n) => expected_key == format!("f{}", n),
        KeyId::Space => expected_key == "space",
    };

    key_matches
}

/// 检查是否是按键释放事件
pub fn is_key_release(key: &Key) -> bool {
    key.event_type == KeyEventType::Release
}

/// 检查是否是按键重复事件
pub fn is_key_repeat(key: &Key) -> bool {
    key.event_type == KeyEventType::Repeat
}

/// 从原始数据检查是否是按键释放事件（用于 CSI 序列快速检测）
pub fn is_key_release_raw(data: &str) -> bool {
    // 不处理 bracketed paste 内容
    if data.contains("\x1b[200~") {
        return false;
    }

    // 快速检查：释放事件包含 ":3"
    data.contains(":3u")
        || data.contains(":3~")
        || data.contains(":3A")
        || data.contains(":3B")
        || data.contains(":3C")
        || data.contains(":3D")
        || data.contains(":3H")
        || data.contains(":3F")
}

/// 从原始数据检查是否是按键重复事件
pub fn is_key_repeat_raw(data: &str) -> bool {
    // 不处理 bracketed paste 内容
    if data.contains("\x1b[200~") {
        return false;
    }

    data.contains(":2u")
        || data.contains(":2~")
        || data.contains(":2A")
        || data.contains(":2B")
        || data.contains(":2C")
        || data.contains(":2D")
        || data.contains(":2H")
        || data.contains(":2F")
}

// =============================================================================
// Kitty CSI-u 可打印字符解码
// =============================================================================

const KITTY_PRINTABLE_ALLOWED_MODIFIERS: u8 = MOD_SHIFT | LOCK_MASK;

/// 解码 Kitty 可打印字符
/// 当 Kitty keyboard protocol flag 1 (disambiguate) 激活时，终端会为所有键发送 CSI-u 序列，包括普通可打印字符
pub fn decode_kitty_printable(codepoint: u32) -> Option<char> {
    // 只接受普通或 Shift 修饰的文本键
    // 拒绝控制字符或无效码点
    if codepoint < 32 {
        return None;
    }

    char::from_u32(codepoint)
}

/// 从 CSI-u 序列解码可打印字符
pub fn decode_kitty_printable_from_sequence(data: &str) -> Option<char> {
    if let Some(rest) = data.strip_prefix("\x1b[") {
        if let Some(params) = rest.strip_suffix('u') {
            let parts: Vec<&str> = params.split(';').collect();

            // 解析 codepoint
            let codepoint_part = parts.first()?;
            let codepoint_parts: Vec<&str> = codepoint_part.split(':').collect();
            let codepoint = codepoint_parts.first()?.parse::<u32>().ok()?;

            // 解析 shifted key
            let shifted_key = codepoint_parts
                .get(1)
                .and_then(|s| if s.is_empty() { None } else { s.parse::<u32>().ok() });

            // 解析修饰符
            let mod_value = parts.get(1).and_then(|s| s.split(':').next()).and_then(|s| s.parse::<u8>().ok()).unwrap_or(1);
            let modifier = mod_value.saturating_sub(1);

            // 只接受普通或 Shift 修饰的文本键
            // 拒绝 Alt、Ctrl 和不支持的修饰符组合
            if (modifier & !KITTY_PRINTABLE_ALLOWED_MODIFIERS) != 0 {
                return None;
            }
            if modifier & (MOD_ALT | MOD_CTRL) != 0 {
                return None;
            }

            // 当 Shift 按下时优先使用 shifted keycode
            let effective_codepoint = if modifier & MOD_SHIFT != 0 {
                shifted_key.unwrap_or(codepoint)
            } else {
                codepoint
            };

            let normalized = normalize_kitty_functional_codepoint(effective_codepoint);

            // 拒绝控制字符或无效码点
            if normalized < 32 {
                return None;
            }

            return char::from_u32(normalized);
        }
    }

    None
}

// =============================================================================
// 辅助函数
// =============================================================================

/// 获取控制字符（用于 Ctrl+key）
/// 使用通用公式：code & 0x1f（掩码到低 5 位）
pub fn get_ctrl_char(key: char) -> Option<char> {
    let ch = key.to_lowercase().next().unwrap_or(key);
    let code = ch as u32;

    if (97..=122).contains(&code) // a-z
        || ch == '['
        || ch == '\\'
        || ch == ']'
        || ch == '_'
    {
        return Some((code & 0x1f) as u8 as char);
    }

    // 处理 - 作为 _（US 键盘上相同的物理键）
    if ch == '-' {
        return Some(31_u8 as char); // 与 Ctrl+_ 相同
    }

    None
}

/// 将按键转换为字符串表示
pub fn key_to_string(key: &Key) -> String {
    let mut parts = Vec::new();

    if key.modifiers.ctrl {
        parts.push("ctrl".to_string());
    }
    if key.modifiers.alt {
        parts.push("alt".to_string());
    }
    if key.modifiers.shift {
        parts.push("shift".to_string());
    }
    if key.modifiers.meta {
        parts.push("meta".to_string());
    }

    parts.push(key.id.to_string());
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_keys() {
        assert!(parse_key("a").is_some());
        assert!(parse_key("\r").is_some());
        assert!(parse_key("\t").is_some());
        assert!(parse_key("\x1b").is_some());
        assert!(parse_key("\x7f").is_some());
    }

    #[test]
    fn test_parse_arrow_keys() {
        assert!(parse_key("\x1b[A").is_some());
        assert!(parse_key("\x1b[B").is_some());
        assert!(parse_key("\x1b[C").is_some());
        assert!(parse_key("\x1b[D").is_some());
    }

    #[test]
    fn test_matches_key() {
        let key = Key::new(KeyId::Char('c'), Modifiers { ctrl: true, ..Default::default() });
        assert!(matches_key(&key, "ctrl+c"));
        assert!(!matches_key(&key, "ctrl+x"));

        let key2 = Key::new(KeyId::Enter, Modifiers::default());
        assert!(matches_key(&key2, "enter"));
    }

    #[test]
    fn test_modifiers() {
        let mods = Modifiers::from_csi_value(5); // ctrl
        assert!(!mods.shift);
        assert!(!mods.alt);
        assert!(mods.ctrl);
        assert!(!mods.meta);

        let mods2 = Modifiers::from_csi_value(2); // shift
        assert!(mods2.shift);
        assert!(!mods2.alt);
        assert!(!mods2.ctrl);
        assert!(!mods2.meta);
    }

    // === 额外的按键解析测试 ===

    #[test]
    fn test_key_id_display() {
        assert_eq!(format!("{}", KeyId::Char('a')), "a");
        assert_eq!(format!("{}", KeyId::Enter), "enter");
        assert_eq!(format!("{}", KeyId::Tab), "tab");
        assert_eq!(format!("{}", KeyId::Backspace), "backspace");
        assert_eq!(format!("{}", KeyId::Delete), "delete");
        assert_eq!(format!("{}", KeyId::Escape), "escape");
        assert_eq!(format!("{}", KeyId::Up), "up");
        assert_eq!(format!("{}", KeyId::Down), "down");
        assert_eq!(format!("{}", KeyId::Left), "left");
        assert_eq!(format!("{}", KeyId::Right), "right");
        assert_eq!(format!("{}", KeyId::Home), "home");
        assert_eq!(format!("{}", KeyId::End), "end");
        assert_eq!(format!("{}", KeyId::PageUp), "pageUp");
        assert_eq!(format!("{}", KeyId::PageDown), "pageDown");
        assert_eq!(format!("{}", KeyId::Insert), "insert");
        assert_eq!(format!("{}", KeyId::F(1)), "f1");
        assert_eq!(format!("{}", KeyId::F(12)), "f12");
        assert_eq!(format!("{}", KeyId::Space), "space");
        assert_eq!(format!("{}", KeyId::Clear), "clear");
    }

    #[test]
    fn test_modifiers_default() {
        let mods: Modifiers = Default::default();
        assert!(!mods.shift);
        assert!(!mods.ctrl);
        assert!(!mods.alt);
        assert!(!mods.meta);
        assert!(mods.is_empty());
    }

    #[test]
    fn test_modifiers_to_csi_value() {
        // 测试 to_csi_value 和 from_csi_value 的一致性
        // 无修饰符
        let mods = Modifiers::default();
        assert_eq!(mods.to_csi_value(), 1);

        // shift
        let mods = Modifiers { shift: true, ..Default::default() };
        assert_eq!(mods.to_csi_value(), 2);

        // alt
        let mods = Modifiers { alt: true, ..Default::default() };
        assert_eq!(mods.to_csi_value(), 3);

        // ctrl
        let mods = Modifiers { ctrl: true, ..Default::default() };
        assert_eq!(mods.to_csi_value(), 5);

        // meta
        let mods = Modifiers { meta: true, ..Default::default() };
        assert_eq!(mods.to_csi_value(), 9);

        // shift + ctrl
        let mods = Modifiers { shift: true, ctrl: true, ..Default::default() };
        assert_eq!(mods.to_csi_value(), 6);
    }

    #[test]
    fn test_modifiers_roundtrip() {
        // 测试 from_csi_value -> to_csi_value -> from_csi_value 的一致性
        let test_mods = [
            Modifiers::default(),
            Modifiers { shift: true, ..Default::default() },
            Modifiers { alt: true, ..Default::default() },
            Modifiers { ctrl: true, ..Default::default() },
            Modifiers { meta: true, ..Default::default() },
            Modifiers { shift: true, ctrl: true, ..Default::default() },
        ];
        
        for mods in test_mods {
            let csi_value = mods.to_csi_value();
            let parsed = Modifiers::from_csi_value(csi_value);
            assert_eq!(mods.shift, parsed.shift, "shift mismatch");
            assert_eq!(mods.alt, parsed.alt, "alt mismatch");
            assert_eq!(mods.ctrl, parsed.ctrl, "ctrl mismatch");
            assert_eq!(mods.meta, parsed.meta, "meta mismatch");
        }
    }

    #[test]
    fn test_key_event_type_default() {
        let event_type: KeyEventType = Default::default();
        assert_eq!(event_type, KeyEventType::Press);
    }

    #[test]
    fn test_key_builder() {
        let key = Key::new(KeyId::Char('a'), Modifiers::default())
            .with_event_type(KeyEventType::Repeat)
            .with_raw("raw data");

        assert_eq!(key.id, KeyId::Char('a'));
        assert_eq!(key.event_type, KeyEventType::Repeat);
        assert_eq!(key.raw, "raw data");
    }

    #[test]
    fn test_parse_key_function_keys() {
        // F1-F4 (SS3 序列)
        assert!(parse_key("\x1bOP").is_some());
        assert!(parse_key("\x1bOQ").is_some());
        assert!(parse_key("\x1bOR").is_some());
        assert!(parse_key("\x1bOS").is_some());

        // F5-F12 (CSI 序列)
        assert!(parse_key("\x1b[15~").is_some());
        assert!(parse_key("\x1b[17~").is_some());
        assert!(parse_key("\x1b[18~").is_some());
        assert!(parse_key("\x1b[19~").is_some());
        assert!(parse_key("\x1b[20~").is_some());
        assert!(parse_key("\x1b[21~").is_some());
        assert!(parse_key("\x1b[23~").is_some());
        assert!(parse_key("\x1b[24~").is_some());
    }

    #[test]
    fn test_parse_key_special_keys() {
        assert!(parse_key("\x1b[2~").is_some()); // Insert
        assert!(parse_key("\x1b[3~").is_some()); // Delete
        assert!(parse_key("\x1b[5~").is_some()); // PageUp
        assert!(parse_key("\x1b[6~").is_some()); // PageDown
        assert!(parse_key("\x1b[H").is_some()); // Home
        assert!(parse_key("\x1b[F").is_some()); // End
        assert!(parse_key("\x1bOH").is_some()); // Home (alternate)
        assert!(parse_key("\x1bOF").is_some()); // End (alternate)
    }

    #[test]
    fn test_parse_key_modified_arrows() {
        // Shift + arrows
        assert!(parse_key("\x1b[1;2A").is_some());
        assert!(parse_key("\x1b[1;2B").is_some());
        assert!(parse_key("\x1b[1;2C").is_some());
        assert!(parse_key("\x1b[1;2D").is_some());

        // Alt + arrows
        assert!(parse_key("\x1b[1;3A").is_some());
        assert!(parse_key("\x1b[1;3B").is_some());
        assert!(parse_key("\x1b[1;3C").is_some());
        assert!(parse_key("\x1b[1;3D").is_some());

        // Ctrl + arrows
        assert!(parse_key("\x1b[1;5A").is_some());
        assert!(parse_key("\x1b[1;5B").is_some());
        assert!(parse_key("\x1b[1;5C").is_some());
        assert!(parse_key("\x1b[1;5D").is_some());
    }

    #[test]
    fn test_parse_key_ctrl_combinations() {
        // Ctrl+A (0x01)
        let key = parse_key("\x01");
        assert!(key.is_some());
        let key = key.unwrap();
        assert!(key.modifiers.ctrl);
        assert_eq!(key.id, KeyId::Char('a'));

        // Ctrl+Z (0x1a)
        let key = parse_key("\x1a");
        assert!(key.is_some());
        let key = key.unwrap();
        assert!(key.modifiers.ctrl);
        assert_eq!(key.id, KeyId::Char('z'));
    }

    #[test]
    fn test_parse_key_alt_combinations() {
        // Alt+字母
        assert!(parse_key("\x1ba").is_some());
        assert!(parse_key("\x1bz").is_some());
        assert!(parse_key("\x1b1").is_some());
        assert!(parse_key("\x1b9").is_some());
    }

    #[test]
    fn test_parse_key_space() {
        let key = parse_key(" ");
        assert!(key.is_some());
        assert_eq!(key.unwrap().id, KeyId::Space);

        // Ctrl+Space
        let key = parse_key("\x00");
        assert!(key.is_some());
        let key = key.unwrap();
        assert!(key.modifiers.ctrl);
        assert_eq!(key.id, KeyId::Space);
    }

    #[test]
    fn test_matches_key_variations() {
        // 测试不同的按键名称变体
        let key_enter = Key::new(KeyId::Enter, Modifiers::default());
        assert!(matches_key(&key_enter, "enter"));
        assert!(matches_key(&key_enter, "return"));

        let key_esc = Key::new(KeyId::Escape, Modifiers::default());
        assert!(matches_key(&key_esc, "escape"));
        assert!(matches_key(&key_esc, "esc"));

        let key_pgup = Key::new(KeyId::PageUp, Modifiers::default());
        assert!(matches_key(&key_pgup, "pageup"));
        assert!(matches_key(&key_pgup, "page_up"));

        let key_pgdn = Key::new(KeyId::PageDown, Modifiers::default());
        assert!(matches_key(&key_pgdn, "pagedown"));
        assert!(matches_key(&key_pgdn, "page_down"));
    }

    #[test]
    fn test_matches_key_modifiers() {
        let key = Key::new(KeyId::Char('c'), Modifiers { ctrl: true, alt: true, ..Default::default() });
        assert!(matches_key(&key, "ctrl+alt+c"));
        assert!(!matches_key(&key, "ctrl+c")); // 缺少 alt
        assert!(!matches_key(&key, "alt+c")); // 缺少 ctrl
        assert!(!matches_key(&key, "ctrl+alt+x")); // 错误的按键
    }

    #[test]
    fn test_matches_key_case_insensitive() {
        let key = Key::new(KeyId::Char('A'), Modifiers { shift: true, ..Default::default() });
        assert!(matches_key(&key, "shift+a"));
        assert!(matches_key(&key, "shift+A"));
    }

    #[test]
    fn test_is_key_release() {
        let key_press = Key::new(KeyId::Char('a'), Modifiers::default());
        assert!(!is_key_release(&key_press));

        let key_release = Key::new(KeyId::Char('a'), Modifiers::default())
            .with_event_type(KeyEventType::Release);
        assert!(is_key_release(&key_release));
    }

    #[test]
    fn test_is_key_repeat() {
        let key_press = Key::new(KeyId::Char('a'), Modifiers::default());
        assert!(!is_key_repeat(&key_press));

        let key_repeat = Key::new(KeyId::Char('a'), Modifiers::default())
            .with_event_type(KeyEventType::Repeat);
        assert!(is_key_repeat(&key_repeat));
    }

    #[test]
    fn test_is_key_release_raw() {
        assert!(is_key_release_raw("\x1b[97:3u"));
        assert!(is_key_release_raw("\x1b[1;2:3A"));
        assert!(!is_key_release_raw("\x1b[97u"));
        assert!(!is_key_release_raw("\x1b[200~")); // bracketed paste
    }

    #[test]
    fn test_is_key_repeat_raw() {
        assert!(is_key_repeat_raw("\x1b[97:2u"));
        assert!(is_key_repeat_raw("\x1b[1;2:2A"));
        assert!(!is_key_repeat_raw("\x1b[97u"));
        assert!(!is_key_repeat_raw("\x1b[200~")); // bracketed paste
    }

    #[test]
    fn test_key_to_string() {
        let key = Key::new(KeyId::Char('c'), Modifiers { ctrl: true, ..Default::default() });
        assert_eq!(key_to_string(&key), "ctrl+c");

        let key = Key::new(KeyId::Char('a'), Modifiers { ctrl: true, alt: true, shift: true, ..Default::default() });
        assert_eq!(key_to_string(&key), "ctrl+alt+shift+a");

        let key = Key::new(KeyId::Enter, Modifiers::default());
        assert_eq!(key_to_string(&key), "enter");
    }

    #[test]
    fn test_get_ctrl_char() {
        assert_eq!(get_ctrl_char('a'), Some('\x01'));
        assert_eq!(get_ctrl_char('z'), Some('\x1a'));
        assert_eq!(get_ctrl_char('['), Some('\x1b'));
        assert_eq!(get_ctrl_char('\\'), Some('\x1c'));
        assert_eq!(get_ctrl_char(']'), Some('\x1d'));
        assert_eq!(get_ctrl_char('_'), Some('\x1f'));
        assert_eq!(get_ctrl_char('-'), Some('\x1f'));
        assert_eq!(get_ctrl_char('1'), None);
        assert_eq!(get_ctrl_char(' '), None);
    }

    #[test]
    fn test_kitty_protocol_state() {
        // 初始状态应该是 false
        assert!(!is_kitty_protocol_active());

        // 设置为 true
        set_kitty_protocol_active(true);
        assert!(is_kitty_protocol_active());

        // 设置回 false
        set_kitty_protocol_active(false);
        assert!(!is_kitty_protocol_active());
    }

    #[test]
    fn test_decode_kitty_printable() {
        assert_eq!(decode_kitty_printable(97), Some('a'));
        assert_eq!(decode_kitty_printable(65), Some('A'));
        assert_eq!(decode_kitty_printable(32), Some(' '));
        assert_eq!(decode_kitty_printable(31), None); // 控制字符
        assert_eq!(decode_kitty_printable(0), None); // 控制字符
    }

    #[test]
    fn test_parse_key_invalid() {
        // 无法解析的序列应该返回 None
        assert!(parse_key("").is_none());
        assert!(parse_key("\x1b[999~").is_none()); // 未知的功能键
    }

    #[test]
    fn test_parse_key_numbers() {
        // 数字键
        for i in '0'..='9' {
            let key = parse_key(&i.to_string());
            assert!(key.is_some(), "Failed to parse key: {}", i);
            assert_eq!(key.unwrap().id, KeyId::Char(i));
        }
    }

    #[test]
    fn test_parse_key_symbols() {
        // 符号键
        let symbols = ['`', '-', '=', '[', ']', '\\', ';', '\'', ',', '.', '/'];
        for sym in symbols {
            let key = parse_key(&sym.to_string());
            assert!(key.is_some(), "Failed to parse key: {}", sym);
        }
    }
}
