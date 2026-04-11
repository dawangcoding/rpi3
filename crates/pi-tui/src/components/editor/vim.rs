//! Vim 模式状态机
//! 
//! 管理 Vim 编辑模式的状态、模式切换和命令缓冲

/// Vim 编辑模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    /// Normal 模式（默认，命令模式）
    Normal,
    /// Insert 模式（文本输入）
    Insert,
    /// Visual 模式（字符选择）
    Visual,
    /// Visual Line 模式（行选择）
    VisualLine,
    /// Command-line 模式（: 命令和 / 搜索）
    Command,
    /// Search 模式（/ 搜索输入）
    Search,
}

impl VimMode {
    /// 获取模式指示器文本
    pub fn indicator(&self) -> &'static str {
        match self {
            VimMode::Normal => "-- NORMAL --",
            VimMode::Insert => "-- INSERT --",
            VimMode::Visual => "-- VISUAL --",
            VimMode::VisualLine => "-- VISUAL LINE --",
            VimMode::Command => "",
            VimMode::Search => "",
        }
    }
}

/// 搜索方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    /// 向前搜索
    Forward,
    /// 向后搜索
    Backward,
}

/// Vim 寄存器内容类型（区分行模式和字符模式粘贴）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterContent {
    /// 字符模式（如 x、yw 等产生的内容）
    Chars(String),
    /// 行模式（如 dd、yy 等产生的内容）
    Lines(String),
}

/// 可重复的 Vim 命令
#[derive(Debug, Clone)]
pub enum VimCommand {
    /// 删除行
    DeleteLine,
    /// 删除字符
    DeleteChar,
    /// 替换字符
    ReplaceChar(char),
    /// 插入文本（记录 Insert 模式中输入的文本）
    InsertText(String),
    /// 粘贴到光标后
    PasteAfter,
    /// 粘贴到光标前
    PasteBefore,
    /// 复制行
    YankLine,
    /// 缩进
    Indent,
    /// 反缩进
    Outdent,
}

/// Vim 状态
pub struct VimState {
    /// 当前模式
    pub mode: VimMode,
    /// 多键命令缓冲（如 gg, dd）
    pub pending_keys: String,
    /// 上次可重复的命令（用于 . 重复）
    pub last_command: Option<VimCommand>,
    /// 命令行内容（: 后的文本）
    pub command_line: String,
    /// 搜索内容（/ 后的文本）
    pub search_input: String,
    /// 当前搜索模式
    pub search_pattern: Option<String>,
    /// 搜索方向
    pub search_direction: SearchDirection,
    /// 搜索匹配位置列表 (row, col)
    pub search_matches: Vec<(usize, usize)>,
    /// 当前搜索匹配索引
    pub search_match_index: usize,
    /// Visual 模式起始位置 (row, col)
    pub visual_start: Option<(usize, usize)>,
    /// 状态栏消息
    pub status_message: String,
    /// Vim 寄存器（默认寄存器）
    pub register: Option<RegisterContent>,
    /// Insert 模式中输入的文本（用于 . 重复）
    pub insert_text_buffer: String,
    /// 是否等待 r 命令的替换字符
    pub waiting_for_char: bool,
}

impl VimState {
    /// 创建新的 Vim 状态
    pub fn new() -> Self {
        let mode = VimMode::Normal;
        Self {
            mode,
            pending_keys: String::new(),
            last_command: None,
            command_line: String::new(),
            search_input: String::new(),
            search_pattern: None,
            search_direction: SearchDirection::Forward,
            search_matches: Vec::new(),
            search_match_index: 0,
            visual_start: None,
            status_message: mode.indicator().to_string(),
            register: None,
            insert_text_buffer: String::new(),
            waiting_for_char: false,
        }
    }

    /// 切换到指定模式
    pub fn switch_mode(&mut self, mode: VimMode) {
        // 退出当前模式的清理
        match self.mode {
            VimMode::Visual | VimMode::VisualLine => {
                self.visual_start = None;
            }
            VimMode::Command => {
                self.command_line.clear();
            }
            VimMode::Search => {
                self.search_input.clear();
            }
            VimMode::Insert => {
                // Insert 模式退出时，记录输入的文本用于 . 重复
                if !self.insert_text_buffer.is_empty() {
                    self.last_command = Some(VimCommand::InsertText(self.insert_text_buffer.clone()));
                    self.insert_text_buffer.clear();
                }
            }
            _ => {}
        }

        // 进入新模式
        self.mode = mode;
        self.pending_keys.clear();
        self.waiting_for_char = false;
        
        self.status_message = mode.indicator().to_string();
    }

    /// 获取当前模式的状态栏显示文本
    pub fn get_status_line(&self) -> String {
        match self.mode {
            VimMode::Command => format!(":{}", self.command_line),
            VimMode::Search => format!("/{}", self.search_input),
            _ => self.status_message.clone(),
        }
    }

    /// 清空 pending keys
    pub fn clear_pending(&mut self) {
        self.pending_keys.clear();
    }
}

impl Default for VimState {
    fn default() -> Self {
        Self::new()
    }
}
