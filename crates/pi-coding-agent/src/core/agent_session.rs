//! Agent 会话管理核心
//!
//! 负责 Agent 会话的生命周期管理、事件处理和统计

use std::sync::Arc;
use tokio::sync::RwLock;
use pi_agent::agent::Agent;
use pi_agent::types::*;
use pi_agent::ContextWindowManager;
use pi_ai::types::*;
use pi_ai::{EstimateTokenCounter, TokenCounter};
use tokio_util::sync::CancellationToken;

use super::system_prompt::*;
use super::tools;
use super::session_manager::{SessionManager, CompactionRecord};
use super::permissions::{PermissionManager, PermissionCheckResult};
use super::compaction::{SessionCompactor, CompactionResult};
use super::extensions::{ExtensionManager, ExtensionLoader, ExtensionContext, ExtensionRegistry};
use super::extensions::builtin::CounterExtensionFactory;
use crate::config::AppConfig;

/// 会话统计
/// 
/// 记录 Agent 会话的各项统计数据
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SessionStats {
    /// 会话 ID
    pub session_id: String,
    /// 会话文件路径
    pub session_file: Option<String>,
    /// 用户消息数量
    pub user_messages: usize,
    /// 助手消息数量
    pub assistant_messages: usize,
    /// 工具调用次数
    pub tool_calls: usize,
    /// 工具结果数量
    pub tool_results: usize,
    /// 总消息数量
    pub total_messages: usize,
    /// Token 统计
    pub tokens: TokenStats,
    /// 成本（美元）
    pub cost: f64,
}

/// Token 统计
/// 
/// 记录会话的 token 使用情况
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TokenStats {
    /// 输入 token 数量
    pub input: u64,
    /// 输出 token 数量
    pub output: u64,
    /// 缓存读取 token 数量
    pub cache_read: u64,
    /// 缓存写入 token 数量
    pub cache_write: u64,
    /// 总 token 数量
    pub total: u64,
}

/// 会话配置
/// 
/// Agent 会话的初始化参数
pub struct AgentSessionConfig {
    /// 使用的模型
    pub model: Model,
    /// 思考级别
    pub thinking_level: ThinkingLevel,
    /// 系统提示词
    pub system_prompt: Option<String>,
    /// 追加的系统提示词
    pub append_system_prompt: Option<String>,
    /// 上下文文件列表
    pub context_files: Vec<String>,
    /// 当前工作目录
    pub cwd: std::path::PathBuf,
    /// 禁用 Bash 工具
    pub no_bash: bool,
    /// 禁用编辑工具
    pub no_edit: bool,
    /// 应用配置
    pub app_config: AppConfig,
    /// 会话 ID
    pub session_id: Option<String>,
}

/// Agent 会话
/// 
/// 管理 Agent 会话的生命周期、事件处理和统计
#[allow(dead_code)] // 多个字段和方法供未来扩展使用
pub struct AgentSession {
    agent: Agent,
    config: AgentSessionConfig,
    stats: Arc<RwLock<SessionStats>>,
    session_manager: Option<SessionManager>,
    permission_manager: Arc<RwLock<PermissionManager>>,
    context_manager: Arc<ContextWindowManager>,
    compactor: Arc<SessionCompactor>,
    compaction_history: Arc<RwLock<Vec<CompactionRecord>>>,
    extension_manager: ExtensionManager,
    mcp_tool_manager: Option<Arc<tools::McpToolManager>>,
}

impl AgentSession {
    #![allow(dead_code)] // 多个方法供未来扩展使用
    /// 创建新会话
    /// 
    /// 初始化 Agent 会话及其所有依赖组件
    pub async fn new(config: AgentSessionConfig) -> anyhow::Result<Self> {
        let session_id = config.session_id.clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        
        // ========================================
        // 1. 初始化扩展注册表并加载扩展（在 Agent 创建之前）
        // ========================================
        let mut extension_registry = ExtensionRegistry::new();
        // 注册内置扩展工厂
        extension_registry.register_factory(Box::new(CounterExtensionFactory));
        
        // 从注册表加载启用的扩展
        let extension_loader = ExtensionLoader::new();
        let loaded_extensions = extension_loader.load_extensions(&extension_registry, &config.app_config);
        
        // 创建扩展管理器并注册已加载的扩展
        let mut extension_manager = ExtensionManager::new();
        for ext in loaded_extensions {
            extension_manager.register(ext);
        }
        
        // 创建扩展上下文
        let extension_ctx = ExtensionContext::new(
            config.cwd.clone(),
            config.app_config.clone(),
            session_id.clone(),
            "global"
        );
        
        // 激活扩展（忽略激活失败，不影响主程序）
        if let Err(e) = extension_manager.activate_all(&extension_ctx).await {
            tracing::warn!("Some extensions failed to activate: {}", e);
        }
        
        // 获取扩展注册的工具
        let extension_tools = extension_manager.get_all_tools();
        if !extension_tools.is_empty() {
            tracing::info!("Loaded {} tool(s) from extensions", extension_tools.len());
        }
        
        // ========================================
        // 2. 构建工具列表（包含扩展工具）
        // ========================================
        let mut tool_list: Vec<Arc<dyn AgentTool>> = vec![
            Arc::new(tools::ReadTool::new(config.cwd.clone())),
            Arc::new(tools::WriteTool::new(config.cwd.clone())),
            Arc::new(tools::GrepTool::new(config.cwd.clone())),
            Arc::new(tools::FindTool::new(config.cwd.clone())),
            Arc::new(tools::LsTool::new(config.cwd.clone())),
        ];
        
        if !config.no_bash {
            tool_list.push(Arc::new(tools::BashTool::new(config.cwd.clone())));
        }
        if !config.no_edit {
            tool_list.push(Arc::new(tools::EditTool::new(config.cwd.clone())));
        }
        
        // 添加 NotebookTool
        tool_list.push(Arc::new(tools::NotebookTool::new(config.cwd.clone())));
        
        // 合并扩展工具到工具列表
        tool_list.extend(extension_tools);
        
        // ========================================
        // 2.5 初始化 MCP 工具管理器并发现工具
        // ========================================
        let mcp_tool_manager = Arc::new(tools::McpToolManager::new());
        
        // 非阻塞方式初始化 MCP Servers - 错误不影响 Agent 启动
        if let Err(e) = mcp_tool_manager.init_from_config().await {
            tracing::warn!("Failed to initialize MCP servers: {}", e);
        }
        
        // 发现 MCP 工具并添加到工具列表
        match mcp_tool_manager.discover_tools().await {
            Ok(mcp_tools) => {
                if !mcp_tools.is_empty() {
                    tracing::info!(tool_count = mcp_tools.len(), "Discovered MCP tools");
                    // MCP 工具是 Tool 类型，不是 AgentTool，所以需要转换
                    // 但 MCP 工具的调用由 McpToolManager 处理，这里只用于展示
                    // 实际的工具调用逻辑需要在 agent_loop 中处理
                    tracing::debug!("MCP tools will be registered with the agent");
                }
            }
            Err(e) => {
                tracing::warn!("Failed to discover MCP tools: {}", e);
            }
        }
        
        // ========================================
        // 3. 构建系统提示词
        // ========================================
        let context_files = load_all_context_files(&config.context_files, &config.cwd);
        let system_prompt = build_system_prompt(&BuildSystemPromptOptions {
            custom_prompt: config.system_prompt.clone(),
            append_system_prompt: config.append_system_prompt.clone(),
            tools: tool_list.clone(),
            guidelines: vec![],
            context_files,
            cwd: config.cwd.clone(),
        });
        
        // ========================================
        // 4. 创建 Agent（此时工具列表已包含扩展工具）
        // ========================================
        // 获取 API key（异步版本，支持 token 自动刷新）
        let provider_str = format!("{:?}", config.model.provider).to_lowercase();
        let api_key = config.app_config.get_api_key_async(&provider_str).await;
        
        let mut agent_options = pi_agent::agent::AgentOptions {
            model: Some(config.model.clone()),
            system_prompt: Some(system_prompt),
            tools: tool_list,
            thinking_level: config.thinking_level.clone(),
            tool_execution: ToolExecutionMode::Parallel,
            session_id: Some(session_id.clone()),
            ..Default::default()
        };
        
        // 设置 API key 回调
        if let Some(key) = api_key {
            agent_options.get_api_key = Some(Arc::new(move |_provider: &str| Some(key.clone())));
        }
        
        let agent = Agent::new(agent_options);
        
        let stats = Arc::new(RwLock::new(SessionStats {
            session_id: session_id.clone(),
            ..Default::default()
        }));
        
        // ========================================
        // 5. 设置事件监听器用于统计
        // ========================================
        let stats_clone = stats.clone();
        let model_cost = config.model.cost.clone();
        let _ = agent.subscribe(Arc::new(move |event: AgentEvent, _cancel: CancellationToken| {
            let stats = stats_clone.clone();
            let model_cost = model_cost.clone();
            tokio::spawn(async move {
                let mut s = stats.write().await;
                match &event {
                    AgentEvent::MessageEnd { message } => {
                        match message {
                            AgentMessage::Llm(Message::User(_)) => s.user_messages += 1,
                            AgentMessage::Llm(Message::Assistant(msg)) => {
                                s.assistant_messages += 1;
                                s.tokens.input += msg.usage.input_tokens;
                                s.tokens.output += msg.usage.output_tokens;
                                if let Some(cr) = msg.usage.cache_read_tokens {
                                    s.tokens.cache_read += cr;
                                }
                                if let Some(cw) = msg.usage.cache_write_tokens {
                                    s.tokens.cache_write += cw;
                                }
                                s.tokens.total = s.tokens.input + s.tokens.output;
                                
                                // 计算本次消息成本
                                let cost = &model_cost;
                                let input_cost = (msg.usage.input_tokens as f64) * cost.input / 1_000_000.0;
                                let output_cost = (msg.usage.output_tokens as f64) * cost.output / 1_000_000.0;
                                let cache_read_cost = msg.usage.cache_read_tokens
                                    .unwrap_or(0) as f64 * cost.cache_read.unwrap_or(0.0) / 1_000_000.0;
                                let cache_write_cost = msg.usage.cache_write_tokens
                                    .unwrap_or(0) as f64 * cost.cache_write.unwrap_or(0.0) / 1_000_000.0;
                                s.cost += input_cost + output_cost + cache_read_cost + cache_write_cost;
                            },
                            AgentMessage::Llm(Message::ToolResult(_)) => s.tool_results += 1,
                        }
                        s.total_messages += 1;
                    },
                    AgentEvent::ToolExecutionEnd { .. } => {
                        s.tool_calls += 1;
                    },
                    _ => {}
                }
            });
        }));
        
        // ========================================
        // 6. 初始化其他组件
        // ========================================
        // 会话管理器
        let session_manager = SessionManager::new(&config.app_config)?;
        
        // 初始化权限管理器
        let permission_config = config.app_config.permissions.clone().unwrap_or_default();
        let permission_manager = Arc::new(RwLock::new(PermissionManager::new(permission_config)));

        // 初始化上下文窗口管理器
        let token_counter: Arc<dyn TokenCounter> = Arc::new(EstimateTokenCounter::new());
        let context_window = config.model.context_window as usize;
        let context_manager = Arc::new(ContextWindowManager::new(token_counter.clone(), context_window));

        // 初始化会话压缩器
        let compactor = Arc::new(SessionCompactor::new(token_counter, context_window));
        let compaction_history = Arc::new(RwLock::new(Vec::new()));

        // 扫描外部扩展目录（仅记录，不加载）
        let _manifests = extension_loader.scan_extensions();
        if !_manifests.is_empty() {
            tracing::info!("Found {} extension manifest(s) in {:?}", _manifests.len(), extension_loader.extensions_dir());
        }

        Ok(Self {
            agent,
            config,
            stats,
            session_manager: Some(session_manager),
            permission_manager,
            context_manager,
            compactor,
            compaction_history,
            extension_manager,
            mcp_tool_manager: Some(mcp_tool_manager),
        })
    }
    
    /// 发送 prompt
    pub async fn prompt(&self, message: AgentMessage) -> anyhow::Result<()> {
        self.agent.prompt(message).await
    }
    
    /// 发送文本 prompt
    pub async fn prompt_text(&self, text: &str) -> anyhow::Result<()> {
        self.agent.prompt_text(text).await
    }
    
    /// 获取 Agent 引用
    pub fn agent(&self) -> &Agent { 
        &self.agent 
    }
    
    /// 获取统计
    pub async fn stats(&self) -> SessionStats {
        self.stats.read().await.clone()
    }

    /// 获取估算成本（美元）
    pub async fn estimated_cost(&self) -> f64 {
        self.stats.read().await.cost
    }
    
    /// 中止
    pub async fn abort(&self) { 
        self.agent.abort().await; 
    }
    
    /// 等待空闲
    pub async fn wait_for_idle(&self) { 
        self.agent.wait_for_idle().await; 
    }
    
    /// 保存会话
    pub async fn save(&self) -> anyhow::Result<()> {
        if let Some(mgr) = &self.session_manager {
            let state = self.agent.state().await;
            mgr.save_session(&self.stats.read().await.session_id, &state.messages).await?;
        }
        Ok(())
    }
    
    /// 获取会话 ID
    pub fn session_id(&self) -> String {
        self.stats.blocking_read().session_id.clone()
    }
    
    /// 获取会话 ID (异步版本)
    pub async fn session_id_async(&self) -> String {
        self.stats.read().await.session_id.clone()
    }

    /// 设置会话 ID
    pub async fn set_session_id(&self, id: String) {
        self.stats.write().await.session_id = id;
    }
    
    /// 获取会话目录
    pub fn sessions_dir(&self) -> Option<std::path::PathBuf> {
        self.session_manager.as_ref().map(|mgr| mgr.sessions_dir().to_path_buf())
    }
    
    /// 获取配置引用
    pub fn config(&self) -> &AgentSessionConfig {
        &self.config
    }
    
    /// 更新会话统计中的会话文件路径
    pub async fn set_session_file(&self, file: Option<String>) {
        self.stats.write().await.session_file = file;
    }
    
    /// 获取权限管理器
    pub fn permission_manager(&self) -> Arc<RwLock<PermissionManager>> {
        self.permission_manager.clone()
    }
    
    /// 检查工具权限
    pub async fn check_tool_permission(&self, tool_name: &str) -> PermissionCheckResult {
        self.permission_manager.read().await.check_tool_permission(tool_name)
    }
    
    /// 检查 Bash 命令权限
    pub async fn check_bash_command(&self, command: &str) -> PermissionCheckResult {
        self.permission_manager.read().await.check_bash_command(command)
    }
    
    /// 授予工具权限
    pub async fn grant_tool(&self, tool_name: &str) {
        self.permission_manager.write().await.grant_tool(tool_name);
    }
    
    /// 撤销工具权限
    pub async fn revoke_tool(&self, tool_name: &str) {
        self.permission_manager.write().await.revoke_tool(tool_name);
    }

    /// 获取当前上下文使用情况
    pub async fn context_usage(&self) -> pi_agent::ContextUsage {
        let messages = self.agent.state().await.messages;
        self.context_manager.estimate_usage(&messages)
    }

    /// 获取上下文窗口管理器
    pub fn context_manager(&self) -> Arc<ContextWindowManager> {
        self.context_manager.clone()
    }
    
    /// Fork 当前会话
    /// 
    /// 从当前会话的某个消息位置创建分支
    /// 
    /// # Arguments
    /// * `fork_at_message_index` - fork 的消息索引，None 表示保留全部消息
    /// 
    /// # Returns
    /// 返回新会话的 ID
    pub async fn fork(&self, fork_at_message_index: Option<usize>) -> anyhow::Result<String> {
        if let Some(mgr) = &self.session_manager {
            let session_id = self.session_id();
            mgr.fork_session(&session_id, fork_at_message_index).await
        } else {
            anyhow::bail!("Session manager not available")
        }
    }

    /// 手动触发压缩
    pub async fn compact(&self) -> anyhow::Result<CompactionResult> {
        let state = self.agent.state().await;
        let messages = &state.messages;

        if messages.len() < 10 {
            anyhow::bail!("Not enough messages to compact (minimum 10)");
        }

        // 执行压缩
        let result = self.compactor.compact(messages, &self.config.model).await?;

        // 应用压缩结果到 agent 的消息列表
        // 注意：这里需要修改 agent 的内部状态，我们通过重置并重新添加消息来实现
        let mut new_messages: Vec<AgentMessage> = messages.clone();
        self.compactor.apply_compaction(&mut new_messages, &result);

        // 更新 agent 状态（通过重置并重新加载）
        self.agent.reset().await;
        
        // 重新添加系统提示词后的消息
        for msg in new_messages {
            // 使用 steer 方法添加消息到队列
            self.agent.steer(msg).await;
        }

        // 记录压缩历史
        self.compaction_history.write().await.push(result.record.clone());

        // 保存会话
        self.save_with_compaction().await?;

        Ok(result)
    }

    /// 检查并自动压缩（在 prompt 前调用）
    pub async fn auto_compact_if_needed(&self) -> anyhow::Result<Option<CompactionResult>> {
        let state = self.agent.state().await;
        let messages = &state.messages;

        if !self.compactor.needs_compaction(messages) {
            return Ok(None);
        }

        // 需要压缩，执行压缩
        match self.compact().await {
            Ok(result) => Ok(Some(result)),
            Err(e) => {
                tracing::warn!("Auto-compaction failed: {}", e);
                Ok(None)
            }
        }
    }

    /// 检查是否需要压缩
    pub async fn needs_compaction(&self) -> bool {
        let state = self.agent.state().await;
        self.compactor.needs_compaction(&state.messages)
    }

    /// 获取压缩历史
    pub async fn compaction_history(&self) -> Vec<CompactionRecord> {
        self.compaction_history.read().await.clone()
    }

    /// 获取压缩器
    pub fn compactor(&self) -> Arc<SessionCompactor> {
        self.compactor.clone()
    }

    /// 获取扩展管理器
    pub fn extension_manager(&self) -> &ExtensionManager {
        &self.extension_manager
    }

    /// 获取会话管理器
    pub fn session_manager(&self) -> Option<&SessionManager> {
        self.session_manager.as_ref()
    }
    
    /// 获取 MCP 工具管理器
    pub fn mcp_tool_manager(&self) -> Option<Arc<tools::McpToolManager>> {
        self.mcp_tool_manager.clone()
    }

    /// 关闭会话，清理资源
    pub async fn shutdown(&mut self) -> anyhow::Result<()> {
        // 停用所有扩展
        self.extension_manager.deactivate_all().await?;
        
        // 停止所有 MCP Servers
        if let Some(ref mcp_manager) = self.mcp_tool_manager {
            if let Err(e) = mcp_manager.shutdown().await {
                tracing::warn!("Failed to shutdown MCP servers: {}", e);
            }
        }
        
        Ok(())
    }

    /// 保存会话（带压缩历史）
    async fn save_with_compaction(&self) -> anyhow::Result<()> {
        if let Some(mgr) = &self.session_manager {
            let state = self.agent.state().await;
            let history = self.compaction_history.read().await.clone();
            let stats = self.stats.read().await.clone();
            mgr.save_session_with_compaction(
                &stats.session_id,
                &state.messages,
                &history,
                Some(&stats),
            ).await?;
        }
        Ok(())
    }
}

/// AgentSession 构建器
/// 
/// 使用 Builder 模式创建 AgentSession
#[allow(dead_code)] // Builder 模式供未来扩展使用
pub struct AgentSessionBuilder {
    model: Option<Model>,
    thinking_level: ThinkingLevel,
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    context_files: Vec<String>,
    cwd: std::path::PathBuf,
    no_bash: bool,
    no_edit: bool,
    app_config: Option<AppConfig>,
    session_id: Option<String>,
}

impl Default for AgentSessionBuilder {
    fn default() -> Self {
        Self {
            model: None,
            thinking_level: ThinkingLevel::Off,
            system_prompt: None,
            append_system_prompt: None,
            context_files: vec![],
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            no_bash: false,
            no_edit: false,
            app_config: None,
            session_id: None,
        }
    }
}

impl AgentSessionBuilder {
    #![allow(dead_code)] // Builder 方法供未来扩展使用
    /// 创建新的 Builder
    pub fn new() -> Self {
        Self::default()
    }
    
    /// 设置模型
    pub fn model(mut self, model: Model) -> Self {
        self.model = Some(model);
        self
    }
    
    /// 设置思考级别
    pub fn thinking_level(mut self, level: ThinkingLevel) -> Self {
        self.thinking_level = level;
        self
    }
    
    /// 设置系统提示词
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }
    
    /// 追加系统提示词
    pub fn append_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.append_system_prompt = Some(prompt.into());
        self
    }
    
    /// 设置上下文文件列表
    pub fn context_files(mut self, files: Vec<String>) -> Self {
        self.context_files = files;
        self
    }
    
    /// 设置工作目录
    pub fn cwd(mut self, cwd: std::path::PathBuf) -> Self {
        self.cwd = cwd;
        self
    }
    
    /// 设置是否禁用 Bash 工具
    pub fn no_bash(mut self, no_bash: bool) -> Self {
        self.no_bash = no_bash;
        self
    }
    
    /// 设置是否禁用编辑工具
    pub fn no_edit(mut self, no_edit: bool) -> Self {
        self.no_edit = no_edit;
        self
    }
    
    /// 设置应用配置
    pub fn app_config(mut self, config: AppConfig) -> Self {
        self.app_config = Some(config);
        self
    }
    
    /// 设置会话 ID
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }
    
    /// 构建 AgentSession
    pub async fn build(self) -> anyhow::Result<AgentSession> {
        let app_config = self.app_config.unwrap_or_else(|| AppConfig::load().unwrap_or_default());
        
        let model = self.model.unwrap_or_else(|| {
            // 使用默认模型
            pi_ai::models::get_model("claude-sonnet-4-20250514")
                .unwrap_or_else(|| pi_ai::types::Model {
                    id: "claude-sonnet-4-20250514".to_string(),
                    name: "Claude Sonnet 4".to_string(),
                    api: pi_ai::types::Api::Anthropic,
                    provider: pi_ai::types::Provider::Anthropic,
                    base_url: "https://api.anthropic.com".to_string(),
                    reasoning: true,
                    input: vec![pi_ai::types::InputModality::Text, pi_ai::types::InputModality::Image],
                    cost: pi_ai::types::ModelCost {
                        input: 3.0,
                        output: 15.0,
                        cache_read: Some(0.3),
                        cache_write: Some(3.75),
                    },
                    context_window: 200000,
                    max_tokens: 16384,
                    headers: None,
                    compat: None,
                })
        });
        
        let config = AgentSessionConfig {
            model,
            thinking_level: self.thinking_level,
            system_prompt: self.system_prompt,
            append_system_prompt: self.append_system_prompt,
            context_files: self.context_files,
            cwd: self.cwd,
            no_bash: self.no_bash,
            no_edit: self.no_edit,
            app_config,
            session_id: self.session_id,
        };
        
        AgentSession::new(config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_ai::types::ModelCost;

    #[test]
    fn test_cost_calculation_formula() {
        // 测试成本计算公式
        // Claude Sonnet 4 定价: input=3.0, output=15.0 per million
        let cost = ModelCost {
            input: 3.0,
            output: 15.0,
            cache_read: Some(0.3),
            cache_write: Some(3.75),
        };
        
        // 1000 input tokens + 500 output tokens
        let input_tokens = 1000u64;
        let output_tokens = 500u64;
        
        let input_cost = (input_tokens as f64) * cost.input / 1_000_000.0;  // $0.003
        let output_cost = (output_tokens as f64) * cost.output / 1_000_000.0;  // $0.0075
        let expected = input_cost + output_cost;  // $0.0105
        
        assert!((input_cost - 0.003).abs() < 0.0001);
        assert!((output_cost - 0.0075).abs() < 0.0001);
        assert!((expected - 0.0105).abs() < 0.0001);
    }

    #[test]
    fn test_cost_with_cache_tokens() {
        // 测试包含缓存 token 的成本计算
        let cost = ModelCost {
            input: 3.0,
            output: 15.0,
            cache_read: Some(0.3),
            cache_write: Some(3.75),
        };
        
        // 模拟使用缓存的请求
        let input_tokens = 1000u64;
        let output_tokens = 500u64;
        let cache_read_tokens = 8000u64;  // 从缓存读取
        let cache_write_tokens = 2000u64; // 写入缓存
        
        let input_cost = (input_tokens as f64) * cost.input / 1_000_000.0;
        let output_cost = (output_tokens as f64) * cost.output / 1_000_000.0;
        let cache_read_cost = (cache_read_tokens as f64) * cost.cache_read.unwrap_or(0.0) / 1_000_000.0;
        let cache_write_cost = (cache_write_tokens as f64) * cost.cache_write.unwrap_or(0.0) / 1_000_000.0;
        
        let total_cost = input_cost + output_cost + cache_read_cost + cache_write_cost;
        
        // 验证各项成本
        assert!((input_cost - 0.003).abs() < 0.0001);
        assert!((output_cost - 0.0075).abs() < 0.0001);
        assert!((cache_read_cost - 0.0024).abs() < 0.0001);  // 8000 * 0.3 / 1M
        assert!((cache_write_cost - 0.0075).abs() < 0.0001); // 2000 * 3.75 / 1M
        assert!((total_cost - 0.0204).abs() < 0.0001);
    }

    #[test]
    fn test_cost_accumulation() {
        // 测试多次消息的成本累加
        let cost = ModelCost {
            input: 3.0,
            output: 15.0,
            cache_read: Some(0.3),
            cache_write: Some(3.75),
        };
        
        // 第一条消息
        let msg1_input = 1000u64;
        let msg1_output = 500u64;
        let cost1 = (msg1_input as f64) * cost.input / 1_000_000.0 
                  + (msg1_output as f64) * cost.output / 1_000_000.0;
        
        // 第二条消息
        let msg2_input = 2000u64;
        let msg2_output = 1000u64;
        let cost2 = (msg2_input as f64) * cost.input / 1_000_000.0 
                  + (msg2_output as f64) * cost.output / 1_000_000.0;
        
        // 累加
        let total = cost1 + cost2;
        
        assert!((cost1 - 0.0105).abs() < 0.0001);
        assert!((cost2 - 0.021).abs() < 0.0001);
        assert!((total - 0.0315).abs() < 0.0001);
    }

    #[test]
    fn test_cost_zero_for_no_cache_pricing() {
        // 测试没有缓存定价时的成本为 0
        let cost = ModelCost {
            input: 3.0,
            output: 15.0,
            cache_read: None,
            cache_write: None,
        };
        
        let cache_read_tokens: u64 = 8000;
        let cache_write_tokens: u64 = 2000;
        
        let cache_read_cost = cache_read_tokens as f64 * cost.cache_read.unwrap_or(0.0) / 1_000_000.0;
        let cache_write_cost = cache_write_tokens as f64 * cost.cache_write.unwrap_or(0.0) / 1_000_000.0;
        
        assert!((cache_read_cost - 0.0).abs() < 0.0001);
        assert!((cache_write_cost - 0.0).abs() < 0.0001);
    }

    #[test]
    fn test_session_stats_default() {
        // 测试 SessionStats 默认值
        let stats = SessionStats::default();
        assert_eq!(stats.cost, 0.0);
        assert_eq!(stats.tokens.input, 0);
        assert_eq!(stats.tokens.output, 0);
        assert_eq!(stats.user_messages, 0);
        assert_eq!(stats.assistant_messages, 0);
    }
}