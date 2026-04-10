//! 主题系统
//!
//! 提供 Light/Dark 主题定义，用于统一管理 UI 颜色配置。

/// 文本样式 - ANSI 转义序列
#[derive(Debug, Clone)]
pub struct TextStyle {
    pub prefix: String,   // ANSI 开始序列
    pub suffix: String,   // ANSI 结束序列（通常是 \x1b[0m）
}

impl TextStyle {
    pub fn new(prefix: &str, suffix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
        }
    }
    
    /// 应用样式到文本
    pub fn apply(&self, text: &str) -> String {
        format!("{}{}{}", self.prefix, text, self.suffix)
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self::new("", "\x1b[0m")
    }
}

/// 消息样式
#[derive(Debug, Clone)]
pub struct MessageStyle {
    pub title: TextStyle,       // 标题行样式（如 "👤 You"）
    pub content: TextStyle,     // 内容样式
}

/// 工具调用样式
#[derive(Debug, Clone)]
pub struct ToolCallStyle {
    pub running: TextStyle,     // 运行中
    pub success: TextStyle,     // 成功
    pub error: TextStyle,       // 失败
}

/// 状态栏样式
#[derive(Debug, Clone)]
pub struct StatusBarStyle {
    pub background: TextStyle,  // 背景色 + 前景色
}

/// 主题定义
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub user_message: MessageStyle,
    pub assistant_message: MessageStyle,
    pub thinking: TextStyle,
    pub tool_call: ToolCallStyle,
    pub status_bar: StatusBarStyle,
    pub separator: TextStyle,
    pub system_message: TextStyle,
    pub prompt_prefix: TextStyle,   // "> " 提示符
    pub loading_indicator: TextStyle,
}

impl Theme {
    /// 深色主题（默认）
    pub fn dark() -> Self {
        Self {
            name: "dark".to_string(),
            user_message: MessageStyle {
                title: TextStyle::new("\x1b[1;34m", "\x1b[0m"),      // 粗体蓝色
                content: TextStyle::new("", "\x1b[0m"),               // 默认
            },
            assistant_message: MessageStyle {
                title: TextStyle::new("\x1b[1;32m", "\x1b[0m"),      // 粗体绿色
                content: TextStyle::new("", "\x1b[0m"),               // 默认
            },
            thinking: TextStyle::new("\x1b[2m", "\x1b[0m"),          // dim
            tool_call: ToolCallStyle {
                running: TextStyle::new("\x1b[33m", "\x1b[0m"),      // 黄色
                success: TextStyle::new("\x1b[32m", "\x1b[0m"),      // 绿色
                error: TextStyle::new("\x1b[31m", "\x1b[0m"),        // 红色
            },
            status_bar: StatusBarStyle {
                background: TextStyle::new("\x1b[48;5;236m\x1b[37m", "\x1b[0m"), // 深灰底+白字
            },
            separator: TextStyle::new("\x1b[2m", "\x1b[0m"),         // dim
            system_message: TextStyle::new("\x1b[2;3m", "\x1b[0m"),  // dim + italic
            prompt_prefix: TextStyle::new("\x1b[36m", "\x1b[0m"),    // cyan
            loading_indicator: TextStyle::new("\x1b[33m", "\x1b[0m"), // 黄色
        }
    }
    
    /// 浅色主题
    pub fn light() -> Self {
        Self {
            name: "light".to_string(),
            user_message: MessageStyle {
                title: TextStyle::new("\x1b[1;34m", "\x1b[0m"),      // 粗体蓝色
                content: TextStyle::new("\x1b[30m", "\x1b[0m"),       // 黑色
            },
            assistant_message: MessageStyle {
                title: TextStyle::new("\x1b[1;32m", "\x1b[0m"),      // 粗体绿色
                content: TextStyle::new("\x1b[30m", "\x1b[0m"),       // 黑色
            },
            thinking: TextStyle::new("\x1b[90m", "\x1b[0m"),         // 灰色
            tool_call: ToolCallStyle {
                running: TextStyle::new("\x1b[33m", "\x1b[0m"),      // 黄色
                success: TextStyle::new("\x1b[32m", "\x1b[0m"),      // 绿色
                error: TextStyle::new("\x1b[31m", "\x1b[0m"),        // 红色
            },
            status_bar: StatusBarStyle {
                background: TextStyle::new("\x1b[48;5;254m\x1b[30m", "\x1b[0m"), // 浅灰底+黑字
            },
            separator: TextStyle::new("\x1b[90m", "\x1b[0m"),        // 灰色
            system_message: TextStyle::new("\x1b[90;3m", "\x1b[0m"), // 灰色+斜体
            prompt_prefix: TextStyle::new("\x1b[34m", "\x1b[0m"),    // 蓝色
            loading_indicator: TextStyle::new("\x1b[33m", "\x1b[0m"), // 黄色
        }
    }
    
    /// 根据名称获取主题
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "dark" => Some(Self::dark()),
            "light" => Some(Self::light()),
            _ => None,
        }
    }
    
    /// 获取可用主题名称列表
    pub fn available_themes() -> Vec<&'static str> {
        vec!["dark", "light"]
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_text_style_apply() {
        let style = TextStyle::new("\x1b[1m", "\x1b[0m");
        assert_eq!(style.apply("hello"), "\x1b[1mhello\x1b[0m");
    }
    
    #[test]
    fn test_dark_theme() {
        let theme = Theme::dark();
        assert_eq!(theme.name, "dark");
        assert!(theme.user_message.title.prefix.contains("34")); // 蓝色
        assert!(theme.assistant_message.title.prefix.contains("32")); // 绿色
    }
    
    #[test]
    fn test_light_theme() {
        let theme = Theme::light();
        assert_eq!(theme.name, "light");
    }
    
    #[test]
    fn test_from_name() {
        assert!(Theme::from_name("dark").is_some());
        assert!(Theme::from_name("light").is_some());
        assert!(Theme::from_name("Dark").is_some()); // 大小写不敏感
        assert!(Theme::from_name("unknown").is_none());
    }
    
    #[test]
    fn test_available_themes() {
        let themes = Theme::available_themes();
        assert!(themes.contains(&"dark"));
        assert!(themes.contains(&"light"));
    }
}
