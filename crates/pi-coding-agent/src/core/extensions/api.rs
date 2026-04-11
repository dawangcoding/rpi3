use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use pi_agent::types::AgentTool;
use crate::config::AppConfig;
use super::types::SlashCommand;

/// 扩展专用日志记录器
#[derive(Clone)]
#[allow(dead_code)] // 扩展 API 方法供扩展开发者使用
pub struct ExtensionLogger {
    prefix: String,
}

impl ExtensionLogger {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self { prefix: prefix.into() }
    }
    
    pub fn info(&self, msg: &str) {
        tracing::info!("[{}] {}", self.prefix, msg);
    }
    
    #[allow(dead_code)]
    pub fn warn(&self, msg: &str) {
        tracing::warn!("[{}] {}", self.prefix, msg);
    }
    
    #[allow(dead_code)]
    pub fn error(&self, msg: &str) {
        tracing::error!("[{}] {}", self.prefix, msg);
    }
    
    #[allow(dead_code)]
    pub fn debug(&self, msg: &str) {
        tracing::debug!("[{}] {}", self.prefix, msg);
    }
}

/// 提供给扩展使用的 API 上下文
#[derive(Clone)]
#[allow(dead_code)] // 扩展 API 字段和方法供扩展开发者使用
pub struct ExtensionContext {
    /// 当前工作目录
    pub cwd: PathBuf,
    /// 应用配置（只读副本）
    pub config: AppConfig,
    /// 当前会话 ID
    pub session_id: String,
    /// 扩展数据目录（`~/.pi/extensions/<name>/data/`）
    pub data_dir: PathBuf,
    /// 日志前缀（扩展名称）
    pub log_prefix: String,
    /// 扩展目录
    pub extension_dir: PathBuf,
    /// 动态工具注册表
    tool_registry: Arc<RwLock<Vec<Arc<dyn AgentTool>>>>,
    /// 动态命令注册表
    command_registry: Arc<RwLock<Vec<SlashCommand>>>,
}

impl std::fmt::Debug for ExtensionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionContext")
            .field("cwd", &self.cwd)
            .field("config", &self.config)
            .field("session_id", &self.session_id)
            .field("data_dir", &self.data_dir)
            .field("log_prefix", &self.log_prefix)
            .field("extension_dir", &self.extension_dir)
            .finish_non_exhaustive()
    }
}

impl ExtensionContext {
    pub fn new(cwd: PathBuf, config: AppConfig, session_id: String, extension_name: &str) -> Self {
        let base_dir = directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().join(".pi").join("extensions").join(extension_name))
            .unwrap_or_else(|| PathBuf::from(".pi/extensions").join(extension_name));
        
        let data_dir = base_dir.join("data");
        let extension_dir = base_dir;
        
        Self {
            cwd,
            config,
            session_id,
            data_dir,
            log_prefix: extension_name.to_string(),
            extension_dir,
            tool_registry: Arc::new(RwLock::new(Vec::new())),
            command_registry: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    /// 注册一个自定义工具
    #[allow(dead_code)]
    pub async fn register_tool(&self, tool: Arc<dyn AgentTool>) {
        tracing::info!("[{}] Registering tool: {}", self.log_prefix, tool.name());
        self.tool_registry.write().await.push(tool);
    }
    
    /// 注册一个 Slash 命令
    #[allow(dead_code)]
    pub async fn register_command(&self, command: SlashCommand) {
        tracing::info!("[{}] Registering command: /{}", self.log_prefix, command.name);
        self.command_registry.write().await.push(command);
    }
    
    /// 获取已注册的工具
    #[allow(dead_code)]
    pub async fn registered_tools(&self) -> Vec<Arc<dyn AgentTool>> {
        self.tool_registry.read().await.clone()
    }
    
    /// 获取已注册的命令
    #[allow(dead_code)]
    pub async fn registered_commands(&self) -> Vec<SlashCommand> {
        self.command_registry.read().await.clone()
    }
    
    /// 注册事件处理器
    ///
    /// 扩展通过此方法声明对特定事件的订阅
    #[allow(dead_code)]
    pub fn subscribe_event(
        &self,
        filter: super::events::EventTypeFilter,
        priority: super::events::EventPriority,
    ) -> super::events::EventSubscription {
        super::events::EventSubscription {
            filter,
            priority,
        }
    }
    
    /// 读取扩展私有数据
    pub fn read_data(&self, key: &str) -> anyhow::Result<Option<String>> {
        let file_path = self.data_dir.join(format!("{}.dat", key));
        if file_path.exists() {
            let content = std::fs::read_to_string(&file_path)?;
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }
    
    /// 写入扩展私有数据
    #[allow(dead_code)]
    pub fn write_data(&self, key: &str, value: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        let file_path = self.data_dir.join(format!("{}.dat", key));
        std::fs::write(&file_path, value)?;
        Ok(())
    }
    
    /// 获取日志记录器
    pub fn logger(&self) -> ExtensionLogger {
        ExtensionLogger::new(&self.log_prefix)
    }
}
