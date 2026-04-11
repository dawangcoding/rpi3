use async_trait::async_trait;
use pi_agent::types::{AgentTool, AgentEvent, AgentToolResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::future::Future;
use serde::{Serialize, Deserialize};
use thiserror::Error;

/// 扩展元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub entry_point: PathBuf,
}

/// 事件处理结果
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub enum EventResult {
    #[default]
    Continue,                    // 继续正常流程
    Block(String),               // 阻止操作（用于 Before* 事件）
    Modified(serde_json::Value), // 修改数据继续（用于 After* 事件）
    StopPropagation,             // 停止向后续处理器传播
}

/// 扩展 trait（Trait Object 方案，首版不用 WASM/动态库）
#[async_trait]
pub trait Extension: Send + Sync {
    /// 获取扩展元信息
    fn manifest(&self) -> &ExtensionManifest;
    
    /// 激活扩展
    async fn activate(&mut self, ctx: &super::api::ExtensionContext) -> anyhow::Result<()>;
    
    /// 停用扩展
    async fn deactivate(&mut self) -> anyhow::Result<()>;
    
    /// 获取扩展注册的工具
    fn registered_tools(&self) -> Vec<Arc<dyn AgentTool>>;
    
    /// 获取扩展注册的 slash 命令
    fn registered_commands(&self) -> Vec<SlashCommand>;
    
    /// 获取扩展的事件订阅配置
    /// 
    /// 返回该扩展关注的事件类型和优先级配置列表
    /// 默认返回空列表，表示使用默认订阅（All 事件，Normal 优先级）
    fn event_subscriptions(&self) -> Vec<super::events::EventSubscription> {
        Vec::new()
    }
    
    /// 处理 Agent 事件（异步，可返回控制信号）
    #[allow(dead_code)]
    async fn on_event(&self, event: &AgentEvent) -> anyhow::Result<EventResult> {
        let _ = event;
        Ok(EventResult::Continue)
    }
}

/// 命令来源
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum CommandSource {
    #[allow(dead_code)]
    Builtin,
    #[allow(dead_code)]
    Extension(String), // 扩展名称
}

/// 命令参数
#[derive(Debug, Clone)]
pub struct CommandArgs {
    /// 原始参数字符串
    #[allow(dead_code)]
    pub raw: String,
    /// 分割后的参数列表
    #[allow(dead_code)]
    pub parts: Vec<String>,
}

impl CommandArgs {
    pub fn new(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let parts: Vec<String> = raw.split_whitespace().map(|s| s.to_string()).collect();
        Self { raw, parts }
    }

    /// 获取第一个参数
    #[allow(dead_code)]
    pub fn first(&self) -> Option<&str> {
        self.parts.first().map(|s| s.as_str())
    }

    /// 是否为空
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.raw.trim().is_empty()
    }
}

/// 命令执行结果
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// 要显示的消息
    pub message: String,
    /// 是否需要重新渲染
    pub should_render: bool,
}

impl CommandResult {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            should_render: true,
        }
    }

    #[allow(dead_code)]
    pub fn silent() -> Self {
        Self {
            message: String::new(),
            should_render: false,
        }
    }
}

/// 命令处理器类型
pub type SlashCommandHandler = Arc<
    dyn Fn(CommandArgs) -> Pin<Box<dyn Future<Output = anyhow::Result<CommandResult>> + Send>> + Send + Sync
>;

/// Slash 命令定义
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub usage: Option<String>,       // 用法示例, e.g. "/counter-stats [--verbose]"
    pub aliases: Vec<String>,        // 命令别名
    pub source: CommandSource,       // 来源
    pub handler: SlashCommandHandler,
}

impl Clone for SlashCommand {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            description: self.description.clone(),
            usage: self.usage.clone(),
            aliases: self.aliases.clone(),
            source: self.source.clone(),
            handler: Arc::clone(&self.handler),
        }
    }
}

impl std::fmt::Debug for SlashCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlashCommand")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("usage", &self.usage)
            .field("aliases", &self.aliases)
            .field("source", &self.source)
            .finish()
    }
}

impl SlashCommand {
    #[allow(dead_code)]
    pub fn new(name: impl Into<String>, description: impl Into<String>, handler: SlashCommandHandler) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            usage: None,
            aliases: Vec::new(),
            source: CommandSource::Builtin,
            handler,
        }
    }

    #[allow(dead_code)]
    pub fn with_usage(mut self, usage: impl Into<String>) -> Self {
        self.usage = Some(usage.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_aliases(mut self, aliases: Vec<String>) -> Self {
        self.aliases = aliases;
        self
    }

    pub fn from_extension(
        name: impl Into<String>,
        description: impl Into<String>,
        extension_name: impl Into<String>,
        handler: SlashCommandHandler,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            usage: None,
            aliases: Vec::new(),
            source: CommandSource::Extension(extension_name.into()),
            handler,
        }
    }

    /// 检查名称是否匹配（包括别名）
    pub fn matches(&self, name: &str) -> bool {
        if self.name.eq_ignore_ascii_case(name) {
            return true;
        }
        self.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    }
}

/// 扩展工具包装器 - 带来源和权限信息
#[allow(dead_code)]
pub struct ExtensionToolWrapper {
    inner: Arc<dyn AgentTool>,
    extension_name: String,
    requires_permission: AtomicBool,
}

impl ExtensionToolWrapper {
    #[allow(dead_code)]
    pub fn new(inner: Arc<dyn AgentTool>, extension_name: String) -> Self {
        Self {
            inner,
            extension_name,
            requires_permission: AtomicBool::new(true), // 默认需要权限
        }
    }

    #[allow(dead_code)]
    pub fn extension_name(&self) -> &str {
        &self.extension_name
    }

    #[allow(dead_code)]
    pub fn set_requires_permission(&self, requires: bool) {
        self.requires_permission.store(requires, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn requires_permission(&self) -> bool {
        self.requires_permission.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl AgentTool for ExtensionToolWrapper {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn label(&self) -> &str {
        self.inner.label()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> serde_json::Value {
        self.inner.parameters()
    }

    fn prepare_arguments(&self, args: serde_json::Value) -> serde_json::Value {
        self.inner.prepare_arguments(args)
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        params: serde_json::Value,
        cancel: tokio_util::sync::CancellationToken,
        on_update: Option<Box<dyn Fn(AgentToolResult) + Send + Sync>>,
    ) -> anyhow::Result<AgentToolResult> {
        self.inner.execute(tool_call_id, params, cancel, on_update).await
    }
}

/// WASM 扩展加载错误
#[derive(Error, Debug, Clone)]
pub enum ExtensionLoadError {
    #[error("WASM 编译错误: {0}")]
    WasmCompileError(String),
    #[error("WASM 实例化错误: {0}")]
    WasmInstantiationError(String),
    #[error("初始化函数错误: {0}")]
    InitFunctionError(String),
    #[error("Manifest 解析错误: {0}")]
    ManifestError(String),
    #[error("IO 错误: {0}")]
    IoError(String),
    #[error("扩展已存在: {0}")]
    ExtensionAlreadyExists(String),
    #[error("扩展未找到: {0}")]
    ExtensionNotFound(String),
    #[error("沙箱违规: {0}")]
    SandboxViolation(String),
    #[error("资源限制超限: {0}")]
    ResourceLimitExceeded(String),
    #[error("沙箱配置错误: {0}")]
    SandboxConfigError(String),
}

/// 沙箱配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// 允许访问的文件系统路径
    #[serde(default)]
    pub allowed_paths: Vec<PathPermission>,
    /// 网络权限
    #[serde(default)]
    pub network: NetworkPermission,
    /// 资源限制
    #[serde(default = "default_resource_limits")]
    pub resource_limits: ResourceLimits,
}

/// 默认资源限制
fn default_resource_limits() -> ResourceLimits {
    ResourceLimits::default()
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            allowed_paths: Vec::new(),
            network: NetworkPermission::default(),
            resource_limits: ResourceLimits::default(),
        }
    }
}

/// 文件路径权限
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathPermission {
    /// 允许访问的路径
    pub path: PathBuf,
    /// 是否允许读取
    #[serde(default = "default_true")]
    pub read: bool,
    /// 是否允许写入
    #[serde(default)]
    pub write: bool,
}

fn default_true() -> bool {
    true
}

/// 网络权限
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkPermission {
    /// 是否允许网络访问
    #[serde(default)]
    pub enabled: bool,
    /// 允许访问的主机列表（格式: host:port 或 host）
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

/// 资源限制配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// 最大内存（字节），默认 64MB
    #[serde(default = "default_max_memory")]
    pub max_memory_bytes: u64,
    /// 最大燃料（CPU 执行步骤），默认 1,000,000
    #[serde(default = "default_max_fuel")]
    pub max_fuel: u64,
    /// 最大执行时间（毫秒），默认 5000ms
    #[serde(default = "default_max_execution_time")]
    pub max_execution_time_ms: u64,
}

fn default_max_memory() -> u64 {
    64 * 1024 * 1024 // 64MB
}

fn default_max_fuel() -> u64 {
    1_000_000
}

fn default_max_execution_time() -> u64 {
    5000 // 5 seconds
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: default_max_memory(),
            max_fuel: default_max_fuel(),
            max_execution_time_ms: default_max_execution_time(),
        }
    }
}

/// 权限声明枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "target")]
pub enum Permission {
    /// 文件读取权限
    FileRead(PathBuf),
    /// 文件写入权限
    FileWrite(PathBuf),
    /// 网络访问权限（host:port 格式）
    NetworkAccess(String),
    /// 完整网络访问权限
    FullNetwork,
}

impl Permission {
    /// 从字符串解析权限
    /// 支持格式: "fs.read:/path", "fs.write:/path", "net:host:port", "net:*"
    pub fn from_str(s: &str) -> Option<Self> {
        if let Some(path) = s.strip_prefix("fs.read:") {
            Some(Permission::FileRead(PathBuf::from(path)))
        } else if let Some(path) = s.strip_prefix("fs.write:") {
            Some(Permission::FileWrite(PathBuf::from(path)))
        } else if let Some(host) = s.strip_prefix("net:") {
            if host == "*" {
                Some(Permission::FullNetwork)
            } else {
                Some(Permission::NetworkAccess(host.to_string()))
            }
        } else {
            None
        }
    }
    
    /// 转换为字符串表示
    pub fn to_permission_string(&self) -> String {
        match self {
            Permission::FileRead(p) => format!("fs.read:{}", p.display()),
            Permission::FileWrite(p) => format!("fs.write:{}", p.display()),
            Permission::NetworkAccess(h) => format!("net:{}", h),
            Permission::FullNetwork => "net:*".to_string(),
        }
    }
}

/// WASM 扩展 Manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmExtensionManifest {
    /// 扩展名称
    pub name: String,
    /// 扩展版本
    pub version: String,
    /// 扩展描述
    pub description: String,
    /// 作者
    #[serde(default)]
    pub author: String,
    /// WASM 文件入口点（相对于扩展目录）
    pub wasm_entry: String,
    /// 权限声明列表（字符串格式，用于 manifest.json）
    #[serde(default)]
    pub permissions: Vec<String>,
    /// 结构化权限声明列表
    #[serde(default)]
    pub structured_permissions: Vec<Permission>,
    /// 沙箱配置（可选，覆盖默认配置）
    #[serde(default)]
    pub sandbox: Option<SandboxConfig>,
    /// 扩展 ID（自动生成）
    #[serde(skip)]
    pub id: String,
}

impl WasmExtensionManifest {
    /// 从文件路径加载 manifest
    pub fn from_file(path: &Path) -> Result<Self, ExtensionLoadError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ExtensionLoadError::IoError(e.to_string()))?;
        let mut manifest: Self = serde_json::from_str(&content)
            .map_err(|e| ExtensionLoadError::ManifestError(e.to_string()))?;
        // 生成唯一 ID
        manifest.id = format!("{}@{}", manifest.name, manifest.version);
        Ok(manifest)
    }

    /// 从 JSON 字符串解析
    pub fn from_json(json: &str) -> Result<Self, ExtensionLoadError> {
        let mut manifest: Self = serde_json::from_str(json)
            .map_err(|e| ExtensionLoadError::ManifestError(e.to_string()))?;
        manifest.id = format!("{}@{}", manifest.name, manifest.version);
        Ok(manifest)
    }
}

/// WASM 实例封装
/// 封装 wasmtime 的 Store、Instance、Module
pub struct WasmInstance {
    /// 扩展唯一标识
    pub id: String,
    /// 扩展名称
    pub name: String,
    /// 扩展版本
    pub version: String,
    /// WASM 文件路径
    pub wasm_path: PathBuf,
}

impl WasmInstance {
    /// 创建新的 WASM 实例描述
    pub fn new(id: String, name: String, version: String, wasm_path: PathBuf) -> Self {
        Self {
            id,
            name,
            version,
            wasm_path,
        }
    }
}

impl std::fmt::Debug for WasmInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmInstance")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("version", &self.version)
            .field("wasm_path", &self.wasm_path)
            .finish()
    }
}

/// WASM 扩展结构体
pub struct WasmExtension {
    /// 扩展唯一标识
    pub id: String,
    /// 扩展名称
    pub name: String,
    /// 扩展版本
    pub version: String,
    /// WASM 文件路径
    pub wasm_path: PathBuf,
    /// Manifest 信息
    pub manifest: WasmExtensionManifest,
    /// WASM 实例（加载后设置）
    pub instance: Option<WasmInstance>,
    /// 是否已激活
    pub is_active: bool,
    /// 加载时间戳
    pub loaded_at: Option<std::time::SystemTime>,
}

impl WasmExtension {
    /// 创建新的 WASM 扩展
    pub fn new(manifest: WasmExtensionManifest, wasm_path: PathBuf) -> Self {
        let id = manifest.id.clone();
        let name = manifest.name.clone();
        let version = manifest.version.clone();
        
        Self {
            id,
            name,
            version,
            wasm_path,
            manifest,
            instance: None,
            is_active: false,
            loaded_at: None,
        }
    }

    /// 设置 WASM 实例
    pub fn set_instance(&mut self, instance: WasmInstance) {
        self.instance = Some(instance);
        self.loaded_at = Some(std::time::SystemTime::now());
    }

    /// 获取扩展状态描述
    pub fn status(&self) -> &'static str {
        if self.is_active {
            "active"
        } else if self.instance.is_some() {
            "loaded"
        } else {
            "discovered"
        }
    }
}

impl std::fmt::Debug for WasmExtension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmExtension")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("version", &self.version)
            .field("wasm_path", &self.wasm_path)
            .field("is_active", &self.is_active)
            .field("status", &self.status())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_agent::types::AgentToolResult;
    use std::path::PathBuf;

    // ==================== EventResult Tests ====================
    
    #[test]
    fn test_event_result_default() {
        let result = EventResult::default();
        assert!(matches!(result, EventResult::Continue));
    }

    #[test]
    fn test_event_result_variants() {
        let continue_result = EventResult::Continue;
        let block_result = EventResult::Block("reason".to_string());
        let modified_result = EventResult::Modified(serde_json::json!({"key": "value"}));
        
        assert!(matches!(continue_result, EventResult::Continue));
        assert!(matches!(block_result, EventResult::Block(_)));
        assert!(matches!(modified_result, EventResult::Modified(_)));
        
        if let EventResult::Block(reason) = block_result {
            assert_eq!(reason, "reason");
        }
        if let EventResult::Modified(value) = modified_result {
            assert_eq!(value["key"], "value");
        }
    }

    // ==================== CommandSource Tests ====================

    #[test]
    fn test_command_source_variants() {
        let builtin = CommandSource::Builtin;
        let extension = CommandSource::Extension("test-extension".to_string());
        
        assert!(matches!(builtin, CommandSource::Builtin));
        assert!(matches!(extension, CommandSource::Extension(_)));
        
        if let CommandSource::Extension(name) = extension {
            assert_eq!(name, "test-extension");
        }
    }

    // ==================== CommandArgs Tests ====================

    #[test]
    fn test_command_args_new() {
        let args = CommandArgs::new("hello world test");
        
        assert_eq!(args.raw, "hello world test");
        assert_eq!(args.parts, vec!["hello", "world", "test"]);
    }

    #[test]
    fn test_command_args_new_empty() {
        let args = CommandArgs::new("");
        
        assert_eq!(args.raw, "");
        assert!(args.parts.is_empty());
        assert!(args.is_empty());
    }

    #[test]
    fn test_command_args_first() {
        let args = CommandArgs::new("first second third");
        
        assert_eq!(args.first(), Some("first"));
        
        let empty_args = CommandArgs::new("");
        assert_eq!(empty_args.first(), None);
    }

    #[test]
    fn test_command_args_is_empty() {
        let args = CommandArgs::new("  ");
        assert!(args.is_empty());
        
        let args_with_content = CommandArgs::new("  content  ");
        assert!(!args_with_content.is_empty());
    }

    // ==================== CommandResult Tests ====================

    #[test]
    fn test_command_result_new() {
        let result = CommandResult::new("test message");
        
        assert_eq!(result.message, "test message");
        assert!(result.should_render);
    }

    #[test]
    fn test_command_result_silent() {
        let result = CommandResult::silent();
        
        assert_eq!(result.message, "");
        assert!(!result.should_render);
    }

    // ==================== SlashCommand Tests ====================

    fn create_test_handler() -> SlashCommandHandler {
        Arc::new(|_args: CommandArgs| {
            Box::pin(async move { Ok(CommandResult::new("test")) })
        })
    }

    #[test]
    fn test_slash_command_new() {
        let handler = create_test_handler();
        let cmd = SlashCommand::new("test-cmd", "Test description", handler);
        
        assert_eq!(cmd.name, "test-cmd");
        assert_eq!(cmd.description, "Test description");
        assert!(cmd.usage.is_none());
        assert!(cmd.aliases.is_empty());
        assert!(matches!(cmd.source, CommandSource::Builtin));
    }

    #[test]
    fn test_slash_command_with_usage() {
        let handler = create_test_handler();
        let cmd = SlashCommand::new("test-cmd", "desc", handler)
            .with_usage("/test-cmd [options]");
        
        assert_eq!(cmd.usage, Some("/test-cmd [options]".to_string()));
    }

    #[test]
    fn test_slash_command_with_aliases() {
        let handler = create_test_handler();
        let cmd = SlashCommand::new("test-cmd", "desc", handler)
            .with_aliases(vec!["tc".to_string(), "t".to_string()]);
        
        assert_eq!(cmd.aliases, vec!["tc", "t"]);
    }

    #[test]
    fn test_slash_command_from_extension() {
        let handler = create_test_handler();
        let cmd = SlashCommand::from_extension(
            "test-cmd",
            "desc",
            "my-extension",
            handler
        );
        
        assert_eq!(cmd.name, "test-cmd");
        assert!(matches!(cmd.source, CommandSource::Extension(_)));
        
        if let CommandSource::Extension(name) = &cmd.source {
            assert_eq!(name, "my-extension");
        }
    }

    #[test]
    fn test_slash_command_matches() {
        let handler = create_test_handler();
        let cmd = SlashCommand::new("test-cmd", "desc", handler)
            .with_aliases(vec!["tc".to_string(), "t".to_string()]);

        // Match by name
        assert!(cmd.matches("test-cmd"));
        assert!(cmd.matches("TEST-CMD")); // case insensitive
        
        // Match by alias
        assert!(cmd.matches("tc"));
        assert!(cmd.matches("TC")); // case insensitive
        assert!(cmd.matches("t"));
        
        // No match
        assert!(!cmd.matches("other"));
        assert!(!cmd.matches("test"));
    }

    #[test]
    fn test_slash_command_clone() {
        let handler = create_test_handler();
        let cmd = SlashCommand::new("test-cmd", "desc", handler)
            .with_usage("/test-cmd")
            .with_aliases(vec!["t".to_string()]);
        
        let cloned = cmd.clone();
        
        assert_eq!(cloned.name, cmd.name);
        assert_eq!(cloned.description, cmd.description);
        assert_eq!(cloned.usage, cmd.usage);
        assert_eq!(cloned.aliases, cmd.aliases);
    }

    // ==================== ExtensionToolWrapper Tests ====================

    struct MockTool {
        name: String,
    }

    #[async_trait]
    impl AgentTool for MockTool {
        fn name(&self) -> &str { &self.name }
        fn label(&self) -> &str { "Mock Tool" }
        fn description(&self) -> &str { "A mock tool for testing" }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object" })
        }
        
        async fn execute(
            &self,
            _tool_call_id: &str,
            _params: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
            _on_update: Option<Box<dyn Fn(AgentToolResult) + Send + Sync>>,
        ) -> anyhow::Result<AgentToolResult> {
            Ok(AgentToolResult {
                content: vec![],
                details: serde_json::Value::Null,
            })
        }
    }

    #[test]
    fn test_extension_tool_wrapper_new() {
        let mock_tool = Arc::new(MockTool { name: "mock".to_string() });
        let wrapper = ExtensionToolWrapper::new(mock_tool, "test-extension".to_string());
        
        assert_eq!(wrapper.name(), "mock");
        assert_eq!(wrapper.label(), "Mock Tool");
        assert_eq!(wrapper.description(), "A mock tool for testing");
        assert_eq!(wrapper.extension_name(), "test-extension");
        assert!(wrapper.requires_permission()); // default is true
    }

    #[test]
    fn test_extension_tool_wrapper_permission() {
        let mock_tool = Arc::new(MockTool { name: "mock".to_string() });
        let wrapper = ExtensionToolWrapper::new(mock_tool, "ext".to_string());
        
        assert!(wrapper.requires_permission());
        
        wrapper.set_requires_permission(false);
        assert!(!wrapper.requires_permission());
        
        wrapper.set_requires_permission(true);
        assert!(wrapper.requires_permission());
    }

    #[tokio::test]
    async fn test_extension_tool_wrapper_execute() {
        let mock_tool = Arc::new(MockTool { name: "mock".to_string() });
        let wrapper = ExtensionToolWrapper::new(mock_tool, "ext".to_string());
        
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = wrapper.execute("call-id", serde_json::json!({}), cancel, None).await;
        
        assert!(result.is_ok());
    }

    // ==================== ExtensionManifest Tests ====================

    #[test]
    fn test_extension_manifest() {
        let manifest = ExtensionManifest {
            name: "test-ext".to_string(),
            version: "1.0.0".to_string(),
            description: "Test extension".to_string(),
            author: "test".to_string(),
            entry_point: PathBuf::from("test.rs"),
        };
        
        assert_eq!(manifest.name, "test-ext");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, "Test extension");
        assert_eq!(manifest.author, "test");
        assert_eq!(manifest.entry_point, PathBuf::from("test.rs"));
    }

    #[test]
    fn test_extension_manifest_serialization() {
        let manifest = ExtensionManifest {
            name: "test-ext".to_string(),
            version: "1.0.0".to_string(),
            description: "Test extension".to_string(),
            author: "test".to_string(),
            entry_point: PathBuf::from("test.rs"),
        };
        
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: ExtensionManifest = serde_json::from_str(&json).unwrap();
        
        assert_eq!(parsed.name, manifest.name);
        assert_eq!(parsed.version, manifest.version);
    }

    // ==================== ExtensionLoadError Tests ====================

    #[test]
    fn test_extension_load_error_variants() {
        let compile_err = ExtensionLoadError::WasmCompileError("syntax error".to_string());
        assert!(compile_err.to_string().contains("WASM 编译错误"));

        let inst_err = ExtensionLoadError::WasmInstantiationError("instantiation failed".to_string());
        assert!(inst_err.to_string().contains("WASM 实例化错误"));

        let init_err = ExtensionLoadError::InitFunctionError("init failed".to_string());
        assert!(init_err.to_string().contains("初始化函数错误"));

        let manifest_err = ExtensionLoadError::ManifestError("invalid json".to_string());
        assert!(manifest_err.to_string().contains("Manifest 解析错误"));

        let io_err = ExtensionLoadError::IoError("file not found".to_string());
        assert!(io_err.to_string().contains("IO 错误"));

        let exists_err = ExtensionLoadError::ExtensionAlreadyExists("ext1".to_string());
        assert!(exists_err.to_string().contains("扩展已存在"));

        let not_found_err = ExtensionLoadError::ExtensionNotFound("ext2".to_string());
        assert!(not_found_err.to_string().contains("扩展未找到"));
    }

    // ==================== WasmExtensionManifest Tests ====================

    #[test]
    fn test_wasm_extension_manifest_from_json() {
        let json = r#"{
            "name": "test-wasm",
            "version": "1.0.0",
            "description": "Test WASM extension",
            "author": "test-author",
            "wasm_entry": "test.wasm",
            "permissions": ["fs.read", "fs.write"]
        }"#;
        
        let manifest = WasmExtensionManifest::from_json(json).unwrap();
        assert_eq!(manifest.name, "test-wasm");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, "Test WASM extension");
        assert_eq!(manifest.author, "test-author");
        assert_eq!(manifest.wasm_entry, "test.wasm");
        assert_eq!(manifest.permissions, vec!["fs.read", "fs.write"]);
        assert_eq!(manifest.id, "test-wasm@1.0.0");
    }

    #[test]
    fn test_wasm_extension_manifest_from_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manifest_path = temp_dir.path().join("manifest.json");
        
        let json = r#"{
            "name": "file-test",
            "version": "2.0.0",
            "description": "Test from file",
            "wasm_entry": "main.wasm"
        }"#;
        
        std::fs::write(&manifest_path, json).unwrap();
        
        let manifest = WasmExtensionManifest::from_file(&manifest_path).unwrap();
        assert_eq!(manifest.name, "file-test");
        assert_eq!(manifest.version, "2.0.0");
        assert_eq!(manifest.id, "file-test@2.0.0");
    }

    #[test]
    fn test_wasm_extension_manifest_invalid_json() {
        let json = "invalid json {[";
        let result = WasmExtensionManifest::from_json(json);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            ExtensionLoadError::ManifestError(_) => (),
            other => panic!("Expected ManifestError, got {:?}", other),
        }
    }

    #[test]
    fn test_wasm_extension_manifest_default_permissions() {
        let json = r#"{
            "name": "test",
            "version": "1.0.0",
            "description": "Test",
            "wasm_entry": "test.wasm"
        }"#;
        
        let manifest = WasmExtensionManifest::from_json(json).unwrap();
        assert!(manifest.permissions.is_empty());
    }

    // ==================== WasmInstance Tests ====================

    #[test]
    fn test_wasm_instance_new() {
        let path = PathBuf::from("/path/to/test.wasm");
        let instance = WasmInstance::new(
            "test@1.0.0".to_string(),
            "test".to_string(),
            "1.0.0".to_string(),
            path.clone(),
        );
        
        assert_eq!(instance.id, "test@1.0.0");
        assert_eq!(instance.name, "test");
        assert_eq!(instance.version, "1.0.0");
        assert_eq!(instance.wasm_path, path);
    }

    #[test]
    fn test_wasm_instance_debug() {
        let instance = WasmInstance::new(
            "id".to_string(),
            "name".to_string(),
            "1.0.0".to_string(),
            PathBuf::from("test.wasm"),
        );
        
        let debug_str = format!("{:?}", instance);
        assert!(debug_str.contains("WasmInstance"));
        assert!(debug_str.contains("id"));
        assert!(debug_str.contains("name"));
    }

    // ==================== WasmExtension Tests ====================

    #[test]
    fn test_wasm_extension_new() {
        let manifest = WasmExtensionManifest::from_json(r#"{
            "name": "test-ext",
            "version": "1.0.0",
            "description": "Test",
            "wasm_entry": "test.wasm"
        }"#).unwrap();
        
        let path = PathBuf::from("/path/to/test.wasm");
        let extension = WasmExtension::new(manifest, path.clone());
        
        assert_eq!(extension.id, "test-ext@1.0.0");
        assert_eq!(extension.name, "test-ext");
        assert_eq!(extension.version, "1.0.0");
        assert_eq!(extension.wasm_path, path);
        assert!(extension.instance.is_none());
        assert!(!extension.is_active);
        assert!(extension.loaded_at.is_none());
    }

    #[test]
    fn test_wasm_extension_set_instance() {
        let manifest = WasmExtensionManifest::from_json(r#"{
            "name": "test",
            "version": "1.0.0",
            "description": "Test",
            "wasm_entry": "test.wasm"
        }"#).unwrap();
        
        let mut extension = WasmExtension::new(manifest, PathBuf::from("test.wasm"));
        
        let instance = WasmInstance::new(
            extension.id.clone(),
            extension.name.clone(),
            extension.version.clone(),
            extension.wasm_path.clone(),
        );
        
        extension.set_instance(instance);
        
        assert!(extension.instance.is_some());
        assert!(extension.loaded_at.is_some());
    }

    #[test]
    fn test_wasm_extension_status() {
        let manifest = WasmExtensionManifest::from_json(r#"{
            "name": "test",
            "version": "1.0.0",
            "description": "Test",
            "wasm_entry": "test.wasm"
        }"#).unwrap();
        
        let mut extension = WasmExtension::new(manifest, PathBuf::from("test.wasm"));
        
        // Initial status
        assert_eq!(extension.status(), "discovered");
        
        // After setting instance
        extension.set_instance(WasmInstance::new(
            extension.id.clone(),
            extension.name.clone(),
            extension.version.clone(),
            extension.wasm_path.clone(),
        ));
        assert_eq!(extension.status(), "loaded");
        
        // After activation
        extension.is_active = true;
        assert_eq!(extension.status(), "active");
    }

    #[test]
    fn test_wasm_extension_debug() {
        let manifest = WasmExtensionManifest::from_json(r#"{
            "name": "test",
            "version": "1.0.0",
            "description": "Test",
            "wasm_entry": "test.wasm"
        }"#).unwrap();
        
        let extension = WasmExtension::new(manifest, PathBuf::from("test.wasm"));
        
        let debug_str = format!("{:?}", extension);
        assert!(debug_str.contains("WasmExtension"));
        assert!(debug_str.contains("test@1.0.0"));
        assert!(debug_str.contains("status"));
    }
}
