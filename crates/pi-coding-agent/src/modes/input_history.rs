//! 输入历史管理
//! 
//! 记录用户输入历史，支持 Up/Down 键导航。

/// 输入历史管理器
pub struct InputHistory {
    /// 历史条目列表（最新在末尾）
    entries: Vec<String>,
    /// 当前浏览位置（entries.len() 表示在最新位置/草稿）
    cursor: usize,
    /// 浏览历史时保存当前编辑器草稿
    draft: Option<String>,
    /// 最大历史条目数
    max_entries: usize,
}

impl InputHistory {
    /// 创建新的输入历史管理器
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            cursor: 0,
            draft: None,
            max_entries,
        }
    }
    
    /// 添加新条目到历史
    /// 自动去重（如果与最后一条相同则跳过）
    pub fn push(&mut self, entry: String) {
        let entry = entry.trim().to_string();
        if entry.is_empty() {
            return;
        }
        
        // 去重：如果与最后一条相同则不添加
        if self.entries.last().map(|s| s.as_str()) == Some(&entry) {
            self.reset_cursor();
            return;
        }
        
        self.entries.push(entry);
        
        // 限制大小
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
        
        self.reset_cursor();
    }
    
    /// 向上浏览历史（Up键）
    /// 
    /// `current_text` 是当前编辑器中的文本，首次调用时会保存为草稿
    /// 返回要显示的历史条目，None 表示已在最早条目
    pub fn prev(&mut self, current_text: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        
        // 首次浏览时保存草稿
        if self.cursor == self.entries.len() {
            self.draft = Some(current_text.to_string());
        }
        
        if self.cursor > 0 {
            self.cursor -= 1;
            Some(&self.entries[self.cursor])
        } else {
            // 已在最早的条目
            Some(&self.entries[0])
        }
    }
    
    /// 向下浏览历史（Down键）
    /// 
    /// 返回要显示的内容，可能是历史条目或保存的草稿
    /// None 表示已在最新位置
    pub fn next(&mut self) -> Option<&str> {
        if self.cursor >= self.entries.len() {
            return None; // 已在最新位置
        }
        
        self.cursor += 1;
        
        if self.cursor < self.entries.len() {
            Some(&self.entries[self.cursor])
        } else {
            // 回到草稿
            self.draft.as_deref()
        }
    }
    
    /// 重置浏览位置到最新
    pub fn reset_cursor(&mut self) {
        self.cursor = self.entries.len();
        self.draft = None;
    }
    
    /// 是否正在浏览历史
    pub fn is_browsing(&self) -> bool {
        self.cursor < self.entries.len()
    }
    
    /// 获取历史条目数
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    
    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for InputHistory {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_push_and_prev() {
        let mut history = InputHistory::new(100);
        history.push("first".to_string());
        history.push("second".to_string());
        history.push("third".to_string());
        
        assert_eq!(history.len(), 3);
        
        // 向上浏览
        assert_eq!(history.prev("current"), Some("third"));
        assert_eq!(history.prev("current"), Some("second"));
        assert_eq!(history.prev("current"), Some("first"));
        // 已到顶部
        assert_eq!(history.prev("current"), Some("first"));
    }
    
    #[test]
    fn test_next_returns_draft() {
        let mut history = InputHistory::new(100);
        history.push("first".to_string());
        history.push("second".to_string());
        
        // 浏览并检查草稿恢复
        let _ = history.prev("my draft");
        let _ = history.prev("my draft");
        
        assert_eq!(history.next(), Some("second"));
        assert_eq!(history.next(), Some("my draft"));
        assert_eq!(history.next(), None); // 已在最新
    }
    
    #[test]
    fn test_dedup() {
        let mut history = InputHistory::new(100);
        history.push("same".to_string());
        history.push("same".to_string());
        assert_eq!(history.len(), 1);
    }
    
    #[test]
    fn test_max_entries() {
        let mut history = InputHistory::new(3);
        history.push("a".to_string());
        history.push("b".to_string());
        history.push("c".to_string());
        history.push("d".to_string());
        
        assert_eq!(history.len(), 3);
        // "a" 应该被移除
        assert_eq!(history.prev(""), Some("d"));
        assert_eq!(history.prev(""), Some("c"));
        assert_eq!(history.prev(""), Some("b"));
    }
    
    #[test]
    fn test_empty_and_whitespace() {
        let mut history = InputHistory::new(100);
        history.push("".to_string());
        history.push("   ".to_string());
        assert_eq!(history.len(), 0);
    }
    
    #[test]
    fn test_reset_cursor() {
        let mut history = InputHistory::new(100);
        history.push("first".to_string());
        
        let _ = history.prev("draft");
        assert!(history.is_browsing());
        
        history.reset_cursor();
        assert!(!history.is_browsing());
    }
}
