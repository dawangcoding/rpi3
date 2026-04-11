//! 会话持久化管理模块
//!
//! 负责会话的保存、加载、列表和删除

use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};
use pi_agent::types::AgentMessage;
use crate::config::AppConfig;
use crate::core::agent_session::SessionStats;

/// 压缩记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionRecord {
    /// 压缩时间戳
    pub compacted_at: i64,
    /// 被替换的消息范围 (start, end)
    pub removed_message_range: (usize, usize),
    /// 摘要 token 数
    pub summary_tokens: usize,
    /// 原始 token 数
    pub original_tokens: usize,
}

/// 会话元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// 会话 ID
    pub id: String,
    /// 会话标题
    pub title: Option<String>,
    /// 创建时间戳
    pub created_at: i64,
    /// 更新时间戳
    pub updated_at: i64,
    /// 消息数量
    pub message_count: usize,
    /// 使用的模型
    pub model: String,
    /// 父会话 ID
    #[serde(default)]
    pub parent_session_id: Option<String>,
    /// fork 消息索引
    #[serde(default)]
    pub fork_at_index: Option<usize>,
}

/// 保存的会话数据
#[derive(Debug, Serialize, Deserialize)]
pub struct SavedSession {
    /// 会话元信息
    pub metadata: SessionMetadata,
    /// 消息列表
    pub messages: Vec<AgentMessage>,
    /// 压缩历史
    #[serde(default)]
    pub compaction_history: Vec<CompactionRecord>,
    /// 会话统计
    #[serde(default)]
    pub stats: Option<SessionStats>,
}

/// 会话管理器
pub struct SessionManager {
    /// 会话目录路径
    sessions_dir: PathBuf,
}

impl SessionManager {
    /// 创建新的会话管理器
    pub fn new(config: &AppConfig) -> anyhow::Result<Self> {
        let sessions_dir = config.sessions_dir();
        std::fs::create_dir_all(&sessions_dir)?;
        Ok(Self { sessions_dir })
    }
    
    /// 从指定目录创建会话管理器
    pub fn with_dir(sessions_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&sessions_dir)?;
        Ok(Self { sessions_dir })
    }
    
    /// 保存会话
    pub async fn save_session(
        &self,
        session_id: &str,
        messages: &[AgentMessage],
    ) -> anyhow::Result<PathBuf> {
        self.save_session_with_compaction(session_id, messages, &[], None).await
    }

    /// 保存会话（带压缩历史）
    pub async fn save_session_with_compaction(
        &self,
        session_id: &str,
        messages: &[AgentMessage],
        compaction_history: &[CompactionRecord],
        stats: Option<&SessionStats>,  // 新增参数
    ) -> anyhow::Result<PathBuf> {
        let path = self.session_path(session_id);
        
        let title = extract_title(messages);
        let model = extract_model(messages);
        
        let session = SavedSession {
            metadata: SessionMetadata {
                id: session_id.to_string(),
                title,
                created_at: chrono::Utc::now().timestamp_millis(),
                updated_at: chrono::Utc::now().timestamp_millis(),
                message_count: messages.len(),
                model,
                parent_session_id: None,
                fork_at_index: None,
            },
            messages: messages.to_vec(),
            compaction_history: compaction_history.to_vec(),
            stats: stats.cloned(),
        };
        
        let json = serde_json::to_string_pretty(&session)?;
        tokio::fs::write(&path, json).await?;
        
        Ok(path)
    }
    
    /// 加载会话
    pub async fn load_session(&self, session_id: &str) -> anyhow::Result<SavedSession> {
        let path = self.session_path(session_id);
        let json = tokio::fs::read_to_string(&path).await?;
        Ok(serde_json::from_str(&json)?)
    }
    
    /// 从指定路径加载会话
    #[allow(dead_code)] // 预留方法供未来使用
    pub async fn load_session_from_path(path: &Path) -> anyhow::Result<SavedSession> {
        let json = tokio::fs::read_to_string(path).await?;
        Ok(serde_json::from_str(&json)?)
    }
    
    /// 列出所有会话
    pub async fn list_sessions(&self) -> anyhow::Result<Vec<SessionMetadata>> {
        let mut sessions = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.sessions_dir).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    if let Ok(session) = serde_json::from_str::<SavedSession>(&content) {
                        sessions.push(session.metadata);
                    }
                }
            }
        }
        
        // 按更新时间排序（最新的在前）
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        
        Ok(sessions)
    }
    
    /// 删除会话
    pub async fn delete_session(&self, session_id: &str) -> anyhow::Result<()> {
        let path = self.session_path(session_id);
        tokio::fs::remove_file(&path).await?;
        Ok(())
    }
    
    /// 检查会话是否存在
    #[allow(dead_code)] // 预留方法供未来使用
    pub fn session_exists(&self, session_id: &str) -> bool {
        self.session_path(session_id).exists()
    }
    
    /// 获取会话文件路径
    pub fn session_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{}.json", session_id))
    }
    
    /// 获取会话目录
    pub fn sessions_dir(&self) -> &Path {
        &self.sessions_dir
    }

    /// 递归删除指定会话及其所有子分支
    ///
    /// # Arguments
    /// * `session_id` - 要删除的会话 ID
    ///
    /// # Returns
    /// 返回删除的会话数量
    pub async fn delete_fork_tree(&self, session_id: &str) -> anyhow::Result<usize> {
        let tree = self.get_session_tree(session_id).await?;
        let mut deleted = 0;
        for session in &tree {
            if let Err(e) = self.delete_session(&session.id).await {
                eprintln!("Warning: {}", e);
            } else {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// 生成会话分支的树形可视化字符串
    ///
    /// # Arguments
    /// * `session_id` - 起始会话 ID（可以是树中的任意节点，会向上追溯到根）
    ///
    /// # Returns
    /// 返回格式化的树形字符串
    pub async fn format_session_tree(&self, session_id: &str) -> anyhow::Result<String> {
        // 首先向上追溯到根会话
        let root_id = self.find_root_session(session_id).await?;

        let sessions = self.list_sessions().await?;
        let mut output = String::new();

        // 找到根会话
        let root = sessions
            .iter()
            .find(|s| s.id == root_id)
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", root_id))?;

        // 格式化根节点
        let title = root.title.as_deref().unwrap_or("Untitled");
        output.push_str(&format!("● {} ({})", title, &root.id[..8.min(root.id.len())]));
        if root.id == session_id {
            output.push_str(" [current]");
        }
        output.push('\n');

        // 递归格式化子节点
        self.format_tree_recursive(&sessions, &root_id, "", &mut output, session_id);

        Ok(output)
    }

    fn format_tree_recursive(
        &self,
        sessions: &[SessionMetadata],
        parent_id: &str,
        prefix: &str,
        output: &mut String,
        current_session_id: &str,
    ) {
        let children: Vec<_> = sessions
            .iter()
            .filter(|s| s.parent_session_id.as_deref() == Some(parent_id))
            .collect();

        for (i, child) in children.iter().enumerate() {
            let is_last = i == children.len() - 1;
            let connector = if is_last { "└── " } else { "├── " };
            let child_prefix = if is_last { "    " } else { "│   " };

            let title = child.title.as_deref().unwrap_or("Untitled");
            let fork_info = child
                .fork_at_index
                .map(|idx| format!(" (forked at msg #{})", idx))
                .unwrap_or_default();

            output.push_str(&format!(
                "{}{}{} ({}){}",
                prefix,
                connector,
                title,
                &child.id[..8.min(child.id.len())],
                fork_info
            ));
            if child.id == current_session_id {
                output.push_str(" [current]");
            }
            output.push('\n');

            // 递归处理子节点
            let new_prefix = format!("{}{}", prefix, child_prefix);
            self.format_tree_recursive(sessions, &child.id, &new_prefix, output, current_session_id);
        }
    }

    /// 向上追溯找到根会话
    ///
    /// 从指定会话开始，沿 parent_session_id 向上追溯到根
    async fn find_root_session(&self, session_id: &str) -> anyhow::Result<String> {
        let mut current_id = session_id.to_string();

        loop {
            let sessions = self.list_sessions().await?;
            let session = sessions
                .iter()
                .find(|s| s.id == current_id)
                .ok_or_else(|| anyhow::anyhow!("Session not found: {}", current_id))?;

            match &session.parent_session_id {
                Some(parent_id) => {
                    current_id = parent_id.clone();
                }
                None => break,
            }
        }

        Ok(current_id)
    }

    /// 按 ID 前缀查找会话
    ///
    /// # Arguments
    /// * `prefix` - 会话 ID 前缀
    ///
    /// # Returns
    /// 返回匹配的会话 ID，如果没有找到返回 None，如果有多个匹配返回错误
    pub async fn find_session_by_prefix(&self, prefix: &str) -> anyhow::Result<Option<String>> {
        let sessions = self.list_sessions().await?;
        let matches: Vec<_> = sessions.iter().filter(|s| s.id.starts_with(prefix)).collect();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(Some(matches[0].id.clone())),
            _ => Err(anyhow::anyhow!(
                "Ambiguous prefix '{}': {} matches",
                prefix,
                matches.len()
            )),
        }
    }
    
    /// 生成新的会话 ID
    pub fn generate_session_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
    
    /// 查找最近的会话
    #[allow(dead_code)] // 预留方法供未来使用
    pub async fn find_most_recent(&self) -> anyhow::Result<Option<SessionMetadata>> {
        let sessions = self.list_sessions().await?;
        Ok(sessions.into_iter().next())
    }
    
    /// Fork 会话
    /// 
    /// 从指定会话的某个消息位置创建分支，生成新的会话
    /// 
    /// # Arguments
    /// * `session_id` - 原会话 ID
    /// * `fork_at_message_index` - fork 的消息索引，None 表示保留全部消息
    /// 
    /// # Returns
    /// 返回新会话的 ID
    pub async fn fork_session(
        &self,
        session_id: &str,
        fork_at_message_index: Option<usize>,
    ) -> anyhow::Result<String> {
        // 加载原会话
        let saved_session = self.load_session(session_id).await?;
        
        // 截断消息到指定索引（如果指定了索引）
        let messages: Vec<AgentMessage> = if let Some(index) = fork_at_message_index {
            saved_session.messages.into_iter().take(index).collect()
        } else {
            saved_session.messages
        };
        
        // 生成新会话 ID
        let new_session_id = Self::generate_session_id();
        
        // 创建新 metadata
        let title = extract_title(&messages);
        let model = extract_model(&messages);
        let now = chrono::Utc::now().timestamp_millis();
        
        let new_session = SavedSession {
            metadata: SessionMetadata {
                id: new_session_id.clone(),
                title,
                created_at: now,
                updated_at: now,
                message_count: messages.len(),
                model,
                parent_session_id: Some(session_id.to_string()),
                fork_at_index: fork_at_message_index,
            },
            messages,
            compaction_history: Vec::new(), // Fork 的会话不继承压缩历史
            stats: None, // Fork 的会话不继承统计
        };
        
        // 保存新会话文件
        let path = self.session_path(&new_session_id);
        let json = serde_json::to_string_pretty(&new_session)?;
        tokio::fs::write(&path, json).await?;
        
        Ok(new_session_id)
    }
    
    /// 列出指定会话的所有 fork 子会话
    /// 
    /// # Arguments
    /// * `session_id` - 父会话 ID
    /// 
    /// # Returns
    /// 返回所有 parent_session_id 等于给定 session_id 的会话元信息列表
    pub async fn list_forks(&self, session_id: &str) -> anyhow::Result<Vec<SessionMetadata>> {
        let all_sessions = self.list_sessions().await?;
        
        let forks: Vec<SessionMetadata> = all_sessions
            .into_iter()
            .filter(|s| s.parent_session_id.as_ref() == Some(&session_id.to_string()))
            .collect();
        
        Ok(forks)
    }
    
    /// 获取会话的完整分支树
    /// 
    /// 向上追溯到根会话，向下列出所有分支
    /// 
    /// # Arguments
    /// * `session_id` - 起始会话 ID
    /// 
    /// # Returns
    /// 返回包含该会话及其所有后代的会话元信息列表
    pub async fn get_session_tree(&self, session_id: &str) -> anyhow::Result<Vec<SessionMetadata>> {
        let mut tree = Vec::new();
        let mut to_process = vec![session_id.to_string()];
        let mut processed = std::collections::HashSet::new();
        
        while let Some(current_id) = to_process.pop() {
            if processed.contains(&current_id) {
                continue;
            }
            processed.insert(current_id.clone());
            
            // 尝试加载当前会话
            if let Ok(saved_session) = self.load_session(&current_id).await {
                tree.push(saved_session.metadata);
                
                // 查找该会话的所有子会话
                let children = self.list_forks(&current_id).await?;
                for child in children {
                    if !processed.contains(&child.id) {
                        to_process.push(child.id);
                    }
                }
            }
        }
        
        Ok(tree)
    }
}

/// 从消息中提取标题（取第一条用户消息的前 100 字符）
fn extract_title(messages: &[AgentMessage]) -> Option<String> {
    for msg in messages {
        if let AgentMessage::Llm(pi_ai::types::Message::User(user_msg)) = msg {
            let content = match &user_msg.content {
                pi_ai::types::UserContent::Text(text) => text.clone(),
                pi_ai::types::UserContent::Blocks(blocks) => {
                    blocks.iter()
                        .filter_map(|block| {
                            if let pi_ai::types::ContentBlock::Text(text) = block {
                                Some(text.text.clone())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            };
            
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                let title = if trimmed.len() > 100 {
                    format!("{}...", &trimmed[..100])
                } else {
                    trimmed.to_string()
                };
                return Some(title);
            }
        }
    }
    None
}

/// 从消息中提取模型信息
fn extract_model(messages: &[AgentMessage]) -> String {
    for msg in messages.iter().rev() {
        if let AgentMessage::Llm(pi_ai::types::Message::Assistant(assistant)) = msg {
            return format!("{:?}/{}", assistant.provider, assistant.model);
        }
    }
    String::new()
}

/// 会话过滤器
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // 预留结构体供未来使用
pub struct SessionFilter {
    /// 模型过滤
    pub model: Option<String>,
    /// 在此之前
    pub before: Option<i64>,
    /// 在此之后
    pub after: Option<i64>,
}

impl SessionManager {
    /// 列出会话（带过滤）
    #[allow(dead_code)] // 预留方法供未来使用
    pub async fn list_sessions_filtered(&self, filter: &SessionFilter) -> anyhow::Result<Vec<SessionMetadata>> {
        let all_sessions = self.list_sessions().await?;
        
        let filtered: Vec<SessionMetadata> = all_sessions
            .into_iter()
            .filter(|s| {
                // 模型过滤
                if let Some(ref model) = filter.model {
                    if !s.model.contains(model) {
                        return false;
                    }
                }
                
                // 时间范围过滤
                if let Some(before) = filter.before {
                    if s.updated_at >= before {
                        return false;
                    }
                }
                
                if let Some(after) = filter.after {
                    if s.updated_at <= after {
                        return false;
                    }
                }
                
                true
            })
            .collect();
        
        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_manager() -> (SessionManager, TempDir) {
        let dir = TempDir::new().unwrap();
        let manager = SessionManager::with_dir(dir.path().to_path_buf()).unwrap();
        (manager, dir)
    }

    fn create_assistant_message(_text: &str) -> AgentMessage {
        AgentMessage::Llm(pi_ai::types::Message::Assistant(pi_ai::types::AssistantMessage::new(
            pi_ai::types::Api::Anthropic,
            pi_ai::types::Provider::Anthropic,
            "claude-3"
        )))
    }

    #[tokio::test]
    async fn test_create_session() {
        let (_manager, _dir) = create_test_manager();
        let session_id = SessionManager::generate_session_id();
        
        // 验证生成的会话 ID 是有效的 UUID
        assert!(!session_id.is_empty());
        assert!(uuid::Uuid::parse_str(&session_id).is_ok());
    }

    #[tokio::test]
    async fn test_save_and_load_session() {
        let (manager, _dir) = create_test_manager();
        let session_id = "test-session-123";
        
        let messages = vec![
            AgentMessage::user("Hello, world!"),
            create_assistant_message("Hi there!"),
        ];
        
        // 保存会话
        let path = manager.save_session(session_id, &messages).await.unwrap();
        assert!(path.exists());
        
        // 加载会话
        let loaded = manager.load_session(session_id).await.unwrap();
        assert_eq!(loaded.metadata.id, session_id);
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.metadata.message_count, 2);
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let (manager, _dir) = create_test_manager();
        
        // 创建多个会话
        for i in 0..3 {
            let session_id = format!("session-{}", i);
            let messages = vec![AgentMessage::user(&format!("Message {}", i))];
            manager.save_session(&session_id, &messages).await.unwrap();
            // 小延迟确保更新时间不同
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        
        // 列出会话
        let sessions = manager.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 3);
        
        // 验证按更新时间排序（最新的在前）
        for i in 0..sessions.len() - 1 {
            assert!(sessions[i].updated_at >= sessions[i + 1].updated_at);
        }
    }

    #[tokio::test]
    async fn test_fork_session() {
        let (manager, _dir) = create_test_manager();
        let parent_id = "parent-session";
        
        let messages = vec![
            AgentMessage::user("Message 1"),
            create_assistant_message("Response 1"),
            AgentMessage::user("Message 2"),
            create_assistant_message("Response 2"),
        ];
        
        // 保存父会话
        manager.save_session(parent_id, &messages).await.unwrap();
        
        // Fork 会话（保留前 2 条消息）
        let forked_id = manager.fork_session(parent_id, Some(2)).await.unwrap();
        
        // 验证 Fork 的会话
        let forked = manager.load_session(&forked_id).await.unwrap();
        assert_eq!(forked.metadata.parent_session_id, Some(parent_id.to_string()));
        assert_eq!(forked.metadata.fork_at_index, Some(2));
        assert_eq!(forked.messages.len(), 2);
        assert!(forked.compaction_history.is_empty()); // Fork 不继承压缩历史
    }

    #[tokio::test]
    async fn test_fork_session_full() {
        let (manager, _dir) = create_test_manager();
        let parent_id = "parent-session-full";
        
        let messages = vec![
            AgentMessage::user("Message 1"),
            create_assistant_message("Response 1"),
        ];
        
        manager.save_session(parent_id, &messages).await.unwrap();
        
        // Fork 会话（保留全部消息）
        let forked_id = manager.fork_session(parent_id, None).await.unwrap();
        
        let forked = manager.load_session(&forked_id).await.unwrap();
        assert_eq!(forked.messages.len(), 2);
        assert_eq!(forked.metadata.fork_at_index, None);
    }

    #[tokio::test]
    async fn test_delete_session() {
        let (manager, _dir) = create_test_manager();
        let session_id = "to-delete";
        
        let messages = vec![AgentMessage::user("Test")];
        manager.save_session(session_id, &messages).await.unwrap();
        
        // 验证会话存在
        assert!(manager.session_exists(session_id));
        
        // 删除会话
        manager.delete_session(session_id).await.unwrap();
        
        // 验证会话已删除
        assert!(!manager.session_exists(session_id));
    }

    #[tokio::test]
    async fn test_list_forks() {
        let (manager, _dir) = create_test_manager();
        let parent_id = "parent-for-forks";
        
        let messages = vec![AgentMessage::user("Parent")];
        manager.save_session(parent_id, &messages).await.unwrap();
        
        // 创建两个 Fork
        let fork1 = manager.fork_session(parent_id, Some(1)).await.unwrap();
        let fork2 = manager.fork_session(parent_id, Some(1)).await.unwrap();
        
        // 列出 Forks
        let forks = manager.list_forks(parent_id).await.unwrap();
        assert_eq!(forks.len(), 2);
        
        let fork_ids: Vec<_> = forks.iter().map(|f| &f.id).collect();
        assert!(fork_ids.contains(&&fork1));
        assert!(fork_ids.contains(&&fork2));
    }

    #[tokio::test]
    async fn test_get_session_tree() {
        let (manager, _dir) = create_test_manager();
        let root_id = "root-session";
        
        let messages = vec![AgentMessage::user("Root")];
        manager.save_session(root_id, &messages).await.unwrap();
        
        // 创建 Fork 链
        let child1 = manager.fork_session(root_id, None).await.unwrap();
        let grandchild = manager.fork_session(&child1, None).await.unwrap();
        
        // 获取树
        let tree = manager.get_session_tree(root_id).await.unwrap();
        assert_eq!(tree.len(), 3);
        
        let ids: Vec<_> = tree.iter().map(|s| s.id.clone()).collect();
        assert!(ids.contains(&root_id.to_string()));
        assert!(ids.contains(&child1));
        assert!(ids.contains(&grandchild));
    }

    #[tokio::test]
    async fn test_find_most_recent() {
        let (manager, _dir) = create_test_manager();
        
        // 创建会话
        let session_id = "recent-session";
        let messages = vec![AgentMessage::user("Recent")];
        manager.save_session(session_id, &messages).await.unwrap();
        
        // 查找最近的会话
        let recent = manager.find_most_recent().await.unwrap();
        assert!(recent.is_some());
        assert_eq!(recent.unwrap().id, session_id);
    }

    #[tokio::test]
    async fn test_list_sessions_filtered() {
        let (manager, _dir) = create_test_manager();
        
        // 创建不同模型的会话
        let messages1 = vec![
            AgentMessage::user("Test"),
            AgentMessage::Llm(pi_ai::types::Message::Assistant(pi_ai::types::AssistantMessage::new(
                pi_ai::types::Api::Anthropic,
                pi_ai::types::Provider::Anthropic,
                "claude-3"
            ))),
        ];
        
        manager.save_session("session-1", &messages1).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        let messages2 = vec![AgentMessage::user("Test 2")];
        manager.save_session("session-2", &messages2).await.unwrap();
        
        // 按模型过滤
        let filter = SessionFilter {
            model: Some("claude".to_string()),
            before: None,
            after: None,
        };
        let filtered = manager.list_sessions_filtered(&filter).await.unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "session-1");
    }

    #[tokio::test]
    async fn test_save_session_with_compaction() {
        let (manager, _dir) = create_test_manager();
        let session_id = "compacted-session";
        
        let messages = vec![
            AgentMessage::user("Message 1"),
            AgentMessage::Llm(pi_ai::types::Message::Assistant(pi_ai::types::AssistantMessage::new(
                pi_ai::types::Api::Anthropic,
                pi_ai::types::Provider::Anthropic,
                "claude-3"
            ))),
        ];
        
        let compaction_history = vec![
            CompactionRecord {
                compacted_at: chrono::Utc::now().timestamp_millis(),
                removed_message_range: (0, 2),
                summary_tokens: 50,
                original_tokens: 200,
            },
        ];
        
        let path = manager.save_session_with_compaction(session_id, &messages, &compaction_history, None).await.unwrap();
        assert!(path.exists());
        
        let loaded = manager.load_session(session_id).await.unwrap();
        assert_eq!(loaded.compaction_history.len(), 1);
        assert_eq!(loaded.compaction_history[0].original_tokens, 200);
    }

    #[tokio::test]
    async fn test_load_session_from_path() {
        let (manager, _dir) = create_test_manager();
        let session_id = "path-test";
        
        let messages = vec![AgentMessage::user("Test")];
        let path = manager.save_session(session_id, &messages).await.unwrap();
        
        // 从路径加载
        let loaded = SessionManager::load_session_from_path(&path).await.unwrap();
        assert_eq!(loaded.metadata.id, session_id);
    }
    
    #[test]
    fn test_extract_title() {
        let messages = vec![
            AgentMessage::user("Hello, this is a test message that is quite long and should be truncated properly"),
        ];
        
        let title = extract_title(&messages);
        assert!(title.is_some());
        assert!(title.unwrap().len() <= 103); // 100 + "..."
    }
    
    #[test]
    fn test_extract_title_short() {
        let messages = vec![
            AgentMessage::user("Short"),
        ];
        
        let title = extract_title(&messages);
        assert_eq!(title, Some("Short".to_string()));
    }
    
    #[test]
    fn test_extract_title_empty() {
        let messages: Vec<AgentMessage> = vec![];
        let title = extract_title(&messages);
        assert!(title.is_none());
    }

    #[tokio::test]
    async fn test_delete_fork_tree() {
        let (manager, _dir) = create_test_manager();
        let root_id = "root-for-delete";

        let messages = vec![AgentMessage::user("Root")];
        manager.save_session(root_id, &messages).await.unwrap();

        // 创建 Fork 链
        let child1 = manager.fork_session(root_id, None).await.unwrap();
        let grandchild = manager.fork_session(&child1, None).await.unwrap();

        // 验证所有会话存在
        assert!(manager.session_exists(root_id));
        assert!(manager.session_exists(&child1));
        assert!(manager.session_exists(&grandchild));

        // 删除整个 fork 树（从根开始）
        let deleted = manager.delete_fork_tree(root_id).await.unwrap();
        assert_eq!(deleted, 3);

        // 验证所有会话已删除
        assert!(!manager.session_exists(root_id));
        assert!(!manager.session_exists(&child1));
        assert!(!manager.session_exists(&grandchild));
    }

    #[tokio::test]
    async fn test_format_session_tree() {
        let (manager, _dir) = create_test_manager();
        let root_id = "root-for-tree";

        let messages = vec![AgentMessage::user("Root Session")];
        manager.save_session(root_id, &messages).await.unwrap();

        // 创建 Fork 链
        let child1 = manager.fork_session(root_id, Some(1)).await.unwrap();
        let _grandchild = manager.fork_session(&child1, None).await.unwrap();

        // 格式化树
        let tree_output = manager.format_session_tree(root_id).await.unwrap();

        // 验证输出包含关键信息
        assert!(tree_output.contains("Root Session"));
        assert!(tree_output.contains("[current]"));
        assert!(tree_output.contains("forked at msg #1"));
        assert!(tree_output.contains("├──") || tree_output.contains("└──"));
    }

    #[tokio::test]
    async fn test_format_session_tree_from_child() {
        let (manager, _dir) = create_test_manager();
        let root_id = "root-from-child";

        let messages = vec![AgentMessage::user("Root")];
        manager.save_session(root_id, &messages).await.unwrap();

        // 创建子会话
        let child = manager.fork_session(root_id, None).await.unwrap();

        // 从子会话格式化树（应该向上追溯到根）
        let tree_output = manager.format_session_tree(&child).await.unwrap();

        // 验证包含根会话
        assert!(tree_output.contains("Root"));
        // 子会话应该被标记为 current
        assert!(tree_output.contains("[current]"));
    }

    #[tokio::test]
    async fn test_find_session_by_prefix() {
        let (manager, _dir) = create_test_manager();

        // 创建测试会话
        let session_id = "abc12345-test-session";
        let messages = vec![AgentMessage::user("Test")];
        manager.save_session(session_id, &messages).await.unwrap();

        // 测试精确前缀匹配
        let result = manager.find_session_by_prefix("abc123").await.unwrap();
        assert_eq!(result, Some(session_id.to_string()));

        // 测试完整 ID 匹配
        let result = manager.find_session_by_prefix(session_id).await.unwrap();
        assert_eq!(result, Some(session_id.to_string()));

        // 测试无匹配
        let result = manager.find_session_by_prefix("xyz").await.unwrap();
        assert_eq!(result, None);

        // 创建另一个会话用于测试模糊前缀
        let session_id2 = "abc67890-another-session";
        manager.save_session(session_id2, &messages).await.unwrap();

        // 测试模糊前缀（应该返回错误）
        let result = manager.find_session_by_prefix("abc").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Ambiguous prefix"));
    }

    #[tokio::test]
    async fn test_delete_session_not_found() {
        let (manager, _dir) = create_test_manager();

        // 尝试删除不存在的会话
        let result = manager.delete_session("non-existent-session").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_find_root_session() {
        let (manager, _dir) = create_test_manager();
        let root_id = "root-session-test";

        let messages = vec![AgentMessage::user("Root")];
        manager.save_session(root_id, &messages).await.unwrap();

        // 创建 Fork 链
        let child = manager.fork_session(root_id, None).await.unwrap();
        let grandchild = manager.fork_session(&child, None).await.unwrap();

        // 从孙子节点查找根
        let found_root = manager.find_root_session(&grandchild).await.unwrap();
        assert_eq!(found_root, root_id);

        // 从子节点查找根
        let found_root = manager.find_root_session(&child).await.unwrap();
        assert_eq!(found_root, root_id);

        // 从根节点查找根
        let found_root = manager.find_root_session(root_id).await.unwrap();
        assert_eq!(found_root, root_id);
    }

    #[tokio::test]
    async fn test_saved_session_with_stats_serialization() {
        // 测试带 stats 的 SavedSession 序列化和反序列化
        use crate::core::agent_session::{SessionStats, TokenStats};

        let (manager, _dir) = create_test_manager();
        let session_id = "session-with-stats";

        let messages = vec![
            AgentMessage::user("Hello"),
            create_assistant_message("Hi there!"),
        ];

        // 创建 stats
        let stats = SessionStats {
            session_id: session_id.to_string(),
            session_file: Some("test.json".to_string()),
            user_messages: 1,
            assistant_messages: 1,
            tool_calls: 0,
            tool_results: 0,
            total_messages: 2,
            tokens: TokenStats {
                input: 1000,
                output: 500,
                cache_read: 200,
                cache_write: 100,
                total: 1500,
            },
            cost: 0.015,
        };

        // 保存带 stats 的会话
        let path = manager.save_session_with_compaction(session_id, &messages, &[], Some(&stats)).await.unwrap();
        assert!(path.exists());

        // 加载并验证 stats
        let loaded = manager.load_session(session_id).await.unwrap();
        assert!(loaded.stats.is_some());
        
        let loaded_stats = loaded.stats.unwrap();
        assert_eq!(loaded_stats.session_id, session_id);
        assert_eq!(loaded_stats.user_messages, 1);
        assert_eq!(loaded_stats.assistant_messages, 1);
        assert_eq!(loaded_stats.tokens.input, 1000);
        assert_eq!(loaded_stats.tokens.output, 500);
        assert_eq!(loaded_stats.tokens.cache_read, 200);
        assert_eq!(loaded_stats.tokens.cache_write, 100);
        assert_eq!(loaded_stats.tokens.total, 1500);
        assert!((loaded_stats.cost - 0.015).abs() < 0.0001);
    }

    #[tokio::test]
    async fn test_saved_session_backward_compatibility() {
        // 测试旧文件（无 stats 字段）的向后兼容性
        let (manager, _dir) = create_test_manager();
        let session_id = "old-session";

        let messages = vec![AgentMessage::user("Test")];
        
        // 保存不带 stats 的会话（模拟旧文件）
        let path = manager.save_session(session_id, &messages).await.unwrap();
        assert!(path.exists());

        // 加载应该成功，stats 为 None
        let loaded = manager.load_session(session_id).await.unwrap();
        assert!(loaded.stats.is_none());
        assert_eq!(loaded.metadata.id, session_id);
        assert_eq!(loaded.messages.len(), 1);
    }
}
