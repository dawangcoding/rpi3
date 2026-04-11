//! 统一注册表模块
//!
//! 提供工具和命令的统一注册、查询和管理功能

use pi_agent::types::AgentTool;
use std::collections::HashMap;
use std::sync::Arc;
use super::types::SlashCommand;

// ==================== ToolRegistry ====================

/// 工具注册记录
///
/// 存储工具实例及其元数据
#[derive(Clone)]
pub struct ToolRegistration {
    /// 工具实例
    pub tool: Arc<dyn AgentTool>,
    /// 所属扩展名称
    pub extension_name: String,
    /// 参数 Schema（用于验证）
    pub parameter_schema: Option<serde_json::Value>,
}

impl std::fmt::Debug for ToolRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistration")
            .field("tool_name", &self.tool.name())
            .field("extension_name", &self.extension_name)
            .field("has_schema", &self.parameter_schema.is_some())
            .finish()
    }
}

/// 统一工具注册表
///
/// 管理所有扩展注册的工具，支持按扩展名称索引和参数验证
#[allow(dead_code)]
pub struct ToolRegistry {
    /// 工具名称 -> 注册记录
    tools: HashMap<String, ToolRegistration>,
    /// 扩展名称 -> 工具名称列表（反向索引）
    extension_tools: HashMap<String, Vec<String>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    /// 创建新的工具注册表
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            extension_tools: HashMap::new(),
        }
    }

    /// 注册工具
    ///
    /// # Arguments
    /// * `extension_name` - 所属扩展名称
    /// * `tool` - 工具实例
    ///
    /// # Errors
    /// 如果同名工具已存在，返回错误
    pub fn register_tool(
        &mut self,
        extension_name: &str,
        tool: Arc<dyn AgentTool>,
    ) -> anyhow::Result<()> {
        let tool_name = tool.name().to_string();
        
        if self.tools.contains_key(&tool_name) {
            anyhow::bail!("工具 '{}' 已存在", tool_name);
        }

        let registration = ToolRegistration {
            tool: Arc::clone(&tool),
            extension_name: extension_name.to_string(),
            parameter_schema: None,
        };

        self.tools.insert(tool_name.clone(), registration);
        self.extension_tools
            .entry(extension_name.to_string())
            .or_default()
            .push(tool_name);

        Ok(())
    }

    /// 注册工具并附带参数 Schema
    ///
    /// # Arguments
    /// * `extension_name` - 所属扩展名称
    /// * `tool` - 工具实例
    /// * `schema` - 参数 JSON Schema
    ///
    /// # Errors
    /// 如果同名工具已存在，返回错误
    pub fn register_tool_with_schema(
        &mut self,
        extension_name: &str,
        tool: Arc<dyn AgentTool>,
        schema: serde_json::Value,
    ) -> anyhow::Result<()> {
        let tool_name = tool.name().to_string();
        
        if self.tools.contains_key(&tool_name) {
            anyhow::bail!("工具 '{}' 已存在", tool_name);
        }

        let registration = ToolRegistration {
            tool: Arc::clone(&tool),
            extension_name: extension_name.to_string(),
            parameter_schema: Some(schema),
        };

        self.tools.insert(tool_name.clone(), registration);
        self.extension_tools
            .entry(extension_name.to_string())
            .or_default()
            .push(tool_name);

        Ok(())
    }

    /// 注销工具
    ///
    /// # Arguments
    /// * `tool_name` - 工具名称
    ///
    /// # Returns
    /// 被注销的工具注册记录，如果工具不存在则返回 `None`
    pub fn unregister_tool(&mut self, tool_name: &str) -> Option<ToolRegistration> {
        let registration = self.tools.remove(tool_name)?;
        
        // 从扩展索引中移除
        if let Some(tool_names) = self.extension_tools.get_mut(&registration.extension_name) {
            tool_names.retain(|name| name != tool_name);
            if tool_names.is_empty() {
                self.extension_tools.remove(&registration.extension_name);
            }
        }

        Some(registration)
    }

    /// 注销某扩展的所有工具
    ///
    /// # Arguments
    /// * `extension_name` - 扩展名称
    ///
    /// # Returns
    /// 被注销的所有工具注册记录
    pub fn unregister_extension_tools(&mut self, extension_name: &str) -> Vec<ToolRegistration> {
        let tool_names = self.extension_tools.remove(extension_name).unwrap_or_default();
        
        let mut removed = Vec::new();
        for tool_name in tool_names {
            if let Some(registration) = self.tools.remove(&tool_name) {
                removed.push(registration);
            }
        }

        removed
    }

    /// 获取工具注册记录
    ///
    /// # Arguments
    /// * `name` - 工具名称
    ///
    /// # Returns
    /// 工具注册记录的引用，如果不存在则返回 `None`
    pub fn get_tool(&self, name: &str) -> Option<&ToolRegistration> {
        self.tools.get(name)
    }

    /// 列出所有工具
    ///
    /// # Returns
    /// 所有工具注册记录的引用列表
    pub fn list_tools(&self) -> Vec<&ToolRegistration> {
        self.tools.values().collect()
    }

    /// 列出某扩展的工具
    ///
    /// # Arguments
    /// * `extension_name` - 扩展名称
    ///
    /// # Returns
    /// 该扩展注册的所有工具注册记录
    pub fn list_extension_tools(&self, extension_name: &str) -> Vec<&ToolRegistration> {
        self.extension_tools
            .get(extension_name)
            .map(|tool_names| {
                tool_names
                    .iter()
                    .filter_map(|name| self.tools.get(name))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 获取所有工具实例（兼容现有 API）
    ///
    /// # Returns
    /// 所有工具实例的克隆列表
    pub fn get_all_tools(&self) -> Vec<Arc<dyn AgentTool>> {
        self.tools.values().map(|r| Arc::clone(&r.tool)).collect()
    }

    /// 获取工具数量
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// 基础参数验证
    ///
    /// 检查参数中 required 字段是否存在
    ///
    /// # Arguments
    /// * `tool_name` - 工具名称
    /// * `params` - 参数 JSON 值
    ///
    /// # Errors
    /// 如果工具不存在或缺少必需参数，返回错误
    #[allow(dead_code)]
    pub fn validate_parameters(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let registration = self.tools.get(tool_name)
            .ok_or_else(|| anyhow::anyhow!("工具 '{}' 不存在", tool_name))?;

        // 如果有 Schema，检查 required 字段
        if let Some(ref schema) = registration.parameter_schema {
            if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
                let params_obj = params.as_object()
                    .ok_or_else(|| anyhow::anyhow!("参数必须是对象类型"))?;
                
                for field in required {
                    if let Some(field_name) = field.as_str() {
                        if !params_obj.contains_key(field_name) {
                            anyhow::bail!("缺少必需参数: {}", field_name);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 获取工具所属扩展
    ///
    /// # Arguments
    /// * `tool_name` - 工具名称
    ///
    /// # Returns
    /// 扩展名称，如果工具不存在则返回 `None`
    #[allow(dead_code)]
    pub fn get_tool_source(&self, tool_name: &str) -> Option<String> {
        self.tools.get(tool_name).map(|r| r.extension_name.clone())
    }
}

// ==================== CommandRegistry ====================

/// 命令注册记录
///
/// 存储命令定义及其元数据
#[derive(Clone)]
pub struct CommandRegistration {
    /// 命令定义
    pub command: SlashCommand,
    /// 所属扩展名称
    pub extension_name: String,
}

impl std::fmt::Debug for CommandRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRegistration")
            .field("command_name", &self.command.name)
            .field("extension_name", &self.extension_name)
            .field("aliases", &self.command.aliases)
            .finish()
    }
}

/// 统一命令注册表
///
/// 管理所有扩展注册的 Slash 命令，支持别名索引和帮助信息生成
#[allow(dead_code)]
pub struct CommandRegistry {
    /// 命令名称 -> 注册记录
    commands: HashMap<String, CommandRegistration>,
    /// 别名 -> 命令名称（别名索引）
    aliases: HashMap<String, String>,
    /// 扩展名称 -> 命令名称列表（反向索引）
    extension_commands: HashMap<String, Vec<String>>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRegistry {
    /// 创建新的命令注册表
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            aliases: HashMap::new(),
            extension_commands: HashMap::new(),
        }
    }

    /// 注册命令
    ///
    /// # Arguments
    /// * `extension_name` - 所属扩展名称
    /// * `command` - 命令定义
    ///
    /// # Errors
    /// 如果同名命令或别名已存在，返回错误
    pub fn register_command(
        &mut self,
        extension_name: &str,
        command: SlashCommand,
    ) -> anyhow::Result<()> {
        let command_name = command.name.clone();
        
        // 检查命令名是否已存在
        if self.commands.contains_key(&command_name) {
            anyhow::bail!("命令 '/{}' 已存在", command_name);
        }

        // 检查别名是否已被使用
        for alias in &command.aliases {
            if self.aliases.contains_key(alias) {
                anyhow::bail!("别名 '{}' 已被使用", alias);
            }
        }

        // 注册别名
        for alias in &command.aliases {
            self.aliases.insert(alias.clone(), command_name.clone());
        }

        // 创建注册记录
        let registration = CommandRegistration {
            command,
            extension_name: extension_name.to_string(),
        };

        // 存储命令
        self.commands.insert(command_name.clone(), registration);
        
        // 更新扩展索引
        self.extension_commands
            .entry(extension_name.to_string())
            .or_default()
            .push(command_name);

        Ok(())
    }

    /// 注销命令
    ///
    /// # Arguments
    /// * `command_name` - 命令名称
    ///
    /// # Returns
    /// 被注销的命令注册记录，如果命令不存在则返回 `None`
    pub fn unregister_command(&mut self, command_name: &str) -> Option<CommandRegistration> {
        let registration = self.commands.remove(command_name)?;
        
        // 移除关联的别名
        for alias in &registration.command.aliases {
            self.aliases.remove(alias);
        }

        // 从扩展索引中移除
        if let Some(command_names) = self.extension_commands.get_mut(&registration.extension_name) {
            command_names.retain(|name| name != command_name);
            if command_names.is_empty() {
                self.extension_commands.remove(&registration.extension_name);
            }
        }

        Some(registration)
    }

    /// 注销某扩展的所有命令
    ///
    /// # Arguments
    /// * `extension_name` - 扩展名称
    ///
    /// # Returns
    /// 被注销的所有命令注册记录
    pub fn unregister_extension_commands(&mut self, extension_name: &str) -> Vec<CommandRegistration> {
        let command_names = self.extension_commands.remove(extension_name).unwrap_or_default();
        
        let mut removed = Vec::new();
        for command_name in command_names {
            if let Some(registration) = self.commands.remove(&command_name) {
                // 移除关联的别名
                for alias in &registration.command.aliases {
                    self.aliases.remove(alias);
                }
                removed.push(registration);
            }
        }

        removed
    }

    /// 按名称或别名查找命令
    ///
    /// # Arguments
    /// * `name` - 命令名称或别名
    ///
    /// # Returns
    /// 命令注册记录的引用，如果不存在则返回 `None`
    pub fn get_command(&self, name: &str) -> Option<&CommandRegistration> {
        // 首先尝试直接查找
        if let Some(registration) = self.commands.get(name) {
            return Some(registration);
        }
        
        // 尝试通过别名查找
        self.aliases.get(name).and_then(|cmd_name| self.commands.get(cmd_name))
    }

    /// 列出所有命令
    ///
    /// # Returns
    /// 所有命令注册记录的引用列表
    pub fn list_commands(&self) -> Vec<&CommandRegistration> {
        self.commands.values().collect()
    }

    /// 列出某扩展的命令
    ///
    /// # Arguments
    /// * `extension_name` - 扩展名称
    ///
    /// # Returns
    /// 该扩展注册的所有命令注册记录
    pub fn list_extension_commands(&self, extension_name: &str) -> Vec<&CommandRegistration> {
        self.extension_commands
            .get(extension_name)
            .map(|command_names| {
                command_names
                    .iter()
                    .filter_map(|name| self.commands.get(name))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 获取所有命令（兼容现有 API）
    ///
    /// # Returns
    /// 所有命令的克隆列表
    pub fn get_all_commands(&self) -> Vec<SlashCommand> {
        self.commands.values().map(|r| r.command.clone()).collect()
    }

    /// 获取命令数量
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// 生成命令帮助信息
    ///
    /// 包含名称、描述、用法、别名
    ///
    /// # Arguments
    /// * `command_name` - 命令名称或别名
    ///
    /// # Returns
    /// 格式化的帮助信息，如果命令不存在则返回 `None`
    #[allow(dead_code)]
    pub fn generate_help(&self, command_name: &str) -> Option<String> {
        let registration = self.get_command(command_name)?;
        let cmd = &registration.command;
        
        let mut help = format!("/{}/\n", cmd.name);
        help.push_str(&format!("  {}\n", cmd.description));
        
        if let Some(ref usage) = cmd.usage {
            help.push_str(&format!("  用法: {}\n", usage));
        }
        
        if !cmd.aliases.is_empty() {
            help.push_str(&format!("  别名: {}\n", cmd.aliases.join(", ")));
        }
        
        Some(help)
    }

    /// 生成所有命令的帮助信息
    ///
    /// # Returns
    /// 所有命令的格式化帮助信息
    #[allow(dead_code)]
    pub fn generate_all_help(&self) -> String {
        let mut help = String::from("可用命令:\n\n");
        
        for registration in self.commands.values() {
            let cmd = &registration.command;
            help.push_str(&format!("/{}/\n", cmd.name));
            help.push_str(&format!("  {}\n", cmd.description));
            
            if let Some(ref usage) = cmd.usage {
                help.push_str(&format!("  用法: {}\n", usage));
            }
            
            if !cmd.aliases.is_empty() {
                help.push_str(&format!("  别名: {}\n", cmd.aliases.join(", ")));
            }
            
            help.push('\n');
        }
        
        help
    }
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use pi_agent::types::AgentToolResult;

    // ==================== Mock Tool ====================

    struct MockTool {
        name: String,
        description: String,
        parameters: serde_json::Value,
    }

    impl MockTool {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                description: format!("Mock tool: {}", name),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        fn with_parameters(name: &str, params: serde_json::Value) -> Self {
            Self {
                name: name.to_string(),
                description: format!("Mock tool: {}", name),
                parameters: params,
            }
        }
    }

    #[async_trait]
    impl AgentTool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn label(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn parameters(&self) -> serde_json::Value {
            self.parameters.clone()
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

    // ==================== ToolRegistry Tests ====================

    #[test]
    fn test_tool_registry_new() {
        let registry = ToolRegistry::new();
        assert_eq!(registry.tool_count(), 0);
    }

    #[test]
    fn test_tool_registry_default() {
        let registry = ToolRegistry::default();
        assert_eq!(registry.tool_count(), 0);
    }

    #[test]
    fn test_tool_registry_register_tool() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(MockTool::new("test-tool"));
        
        let result = registry.register_tool("ext1", tool);
        assert!(result.is_ok());
        assert_eq!(registry.tool_count(), 1);
    }

    #[test]
    fn test_tool_registry_register_duplicate_tool() {
        let mut registry = ToolRegistry::new();
        let tool1 = Arc::new(MockTool::new("test-tool"));
        let tool2 = Arc::new(MockTool::new("test-tool"));
        
        registry.register_tool("ext1", tool1).unwrap();
        let result = registry.register_tool("ext2", tool2);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("已存在"));
    }

    #[test]
    fn test_tool_registry_register_tool_with_schema() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(MockTool::new("test-tool"));
        let schema = serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {"type": "string"}
            }
        });
        
        let result = registry.register_tool_with_schema("ext1", tool, schema);
        assert!(result.is_ok());
        
        let registration = registry.get_tool("test-tool").unwrap();
        assert!(registration.parameter_schema.is_some());
    }

    #[test]
    fn test_tool_registry_unregister_tool() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(MockTool::new("test-tool"));
        
        registry.register_tool("ext1", tool).unwrap();
        assert_eq!(registry.tool_count(), 1);
        
        let removed = registry.unregister_tool("test-tool");
        assert!(removed.is_some());
        assert_eq!(registry.tool_count(), 0);
        assert_eq!(removed.unwrap().extension_name, "ext1");
    }

    #[test]
    fn test_tool_registry_unregister_nonexistent_tool() {
        let mut registry = ToolRegistry::new();
        
        let result = registry.unregister_tool("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_tool_registry_unregister_extension_tools() {
        let mut registry = ToolRegistry::new();
        
        registry.register_tool("ext1", Arc::new(MockTool::new("tool1"))).unwrap();
        registry.register_tool("ext1", Arc::new(MockTool::new("tool2"))).unwrap();
        registry.register_tool("ext2", Arc::new(MockTool::new("tool3"))).unwrap();
        
        assert_eq!(registry.tool_count(), 3);
        
        let removed = registry.unregister_extension_tools("ext1");
        assert_eq!(removed.len(), 2);
        assert_eq!(registry.tool_count(), 1);
        
        // 验证 ext2 的工具仍然存在
        assert!(registry.get_tool("tool3").is_some());
    }

    #[test]
    fn test_tool_registry_get_tool() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(MockTool::new("test-tool"));
        
        registry.register_tool("ext1", tool).unwrap();
        
        let registration = registry.get_tool("test-tool");
        assert!(registration.is_some());
        assert_eq!(registration.unwrap().tool.name(), "test-tool");
        
        assert!(registry.get_tool("nonexistent").is_none());
    }

    #[test]
    fn test_tool_registry_list_tools() {
        let mut registry = ToolRegistry::new();
        
        registry.register_tool("ext1", Arc::new(MockTool::new("tool1"))).unwrap();
        registry.register_tool("ext2", Arc::new(MockTool::new("tool2"))).unwrap();
        
        let tools = registry.list_tools();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_tool_registry_list_extension_tools() {
        let mut registry = ToolRegistry::new();
        
        registry.register_tool("ext1", Arc::new(MockTool::new("tool1"))).unwrap();
        registry.register_tool("ext1", Arc::new(MockTool::new("tool2"))).unwrap();
        registry.register_tool("ext2", Arc::new(MockTool::new("tool3"))).unwrap();
        
        let ext1_tools = registry.list_extension_tools("ext1");
        assert_eq!(ext1_tools.len(), 2);
        
        let ext2_tools = registry.list_extension_tools("ext2");
        assert_eq!(ext2_tools.len(), 1);
        
        let nonexistent = registry.list_extension_tools("nonexistent");
        assert!(nonexistent.is_empty());
    }

    #[test]
    fn test_tool_registry_get_all_tools() {
        let mut registry = ToolRegistry::new();
        
        registry.register_tool("ext1", Arc::new(MockTool::new("tool1"))).unwrap();
        registry.register_tool("ext2", Arc::new(MockTool::new("tool2"))).unwrap();
        
        let tools = registry.get_all_tools();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_tool_registry_validate_parameters() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(MockTool::new("test-tool"));
        let schema = serde_json::json!({
            "type": "object",
            "required": ["path", "name"],
            "properties": {
                "path": {"type": "string"},
                "name": {"type": "string"},
                "optional": {"type": "boolean"}
            }
        });
        
        registry.register_tool_with_schema("ext1", tool, schema).unwrap();
        
        // 有效参数
        let params = serde_json::json!({"path": "/tmp", "name": "test"});
        assert!(registry.validate_parameters("test-tool", &params).is_ok());
        
        // 缺少必需参数
        let params_missing = serde_json::json!({"path": "/tmp"});
        let result = registry.validate_parameters("test-tool", &params_missing);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("缺少必需参数"));
        
        // 工具不存在
        let result = registry.validate_parameters("nonexistent", &params);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_registry_get_tool_source() {
        let mut registry = ToolRegistry::new();
        
        registry.register_tool("ext1", Arc::new(MockTool::new("tool1"))).unwrap();
        
        assert_eq!(registry.get_tool_source("tool1"), Some("ext1".to_string()));
        assert_eq!(registry.get_tool_source("nonexistent"), None);
    }

    // ==================== Mock Command Handler ====================

    fn create_test_handler() -> super::super::types::SlashCommandHandler {
        use super::super::types::{CommandArgs, CommandResult};
        Arc::new(|_args: CommandArgs| {
            Box::pin(async move { Ok(CommandResult::new("test")) })
        })
    }

    // ==================== CommandRegistry Tests ====================

    #[test]
    fn test_command_registry_new() {
        let registry = CommandRegistry::new();
        assert_eq!(registry.command_count(), 0);
    }

    #[test]
    fn test_command_registry_default() {
        let registry = CommandRegistry::default();
        assert_eq!(registry.command_count(), 0);
    }

    #[test]
    fn test_command_registry_register_command() {
        let mut registry = CommandRegistry::new();
        let cmd = SlashCommand::new("test-cmd", "Test command", create_test_handler());
        
        let result = registry.register_command("ext1", cmd);
        assert!(result.is_ok());
        assert_eq!(registry.command_count(), 1);
    }

    #[test]
    fn test_command_registry_register_duplicate_command() {
        let mut registry = CommandRegistry::new();
        
        let cmd1 = SlashCommand::new("test-cmd", "Test 1", create_test_handler());
        let cmd2 = SlashCommand::new("test-cmd", "Test 2", create_test_handler());
        
        registry.register_command("ext1", cmd1).unwrap();
        let result = registry.register_command("ext2", cmd2);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("已存在"));
    }

    #[test]
    fn test_command_registry_register_command_with_aliases() {
        let mut registry = CommandRegistry::new();
        
        let cmd = SlashCommand::new("test-cmd", "Test command", create_test_handler())
            .with_aliases(vec!["tc".to_string(), "t".to_string()]);
        
        let result = registry.register_command("ext1", cmd);
        assert!(result.is_ok());
        
        // 可以通过别名查找
        assert!(registry.get_command("tc").is_some());
        assert!(registry.get_command("t").is_some());
    }

    #[test]
    fn test_command_registry_register_duplicate_alias() {
        let mut registry = CommandRegistry::new();
        
        let cmd1 = SlashCommand::new("cmd1", "Command 1", create_test_handler())
            .with_aliases(vec!["c".to_string()]);
        registry.register_command("ext1", cmd1).unwrap();
        
        let cmd2 = SlashCommand::new("cmd2", "Command 2", create_test_handler())
            .with_aliases(vec!["c".to_string()]);
        let result = registry.register_command("ext1", cmd2);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("别名"));
    }

    #[test]
    fn test_command_registry_unregister_command() {
        let mut registry = CommandRegistry::new();
        
        let cmd = SlashCommand::new("test-cmd", "Test command", create_test_handler())
            .with_aliases(vec!["tc".to_string()]);
        
        registry.register_command("ext1", cmd).unwrap();
        assert_eq!(registry.command_count(), 1);
        
        let removed = registry.unregister_command("test-cmd");
        assert!(removed.is_some());
        assert_eq!(registry.command_count(), 0);
        
        // 别名也应该被移除
        assert!(registry.get_command("tc").is_none());
    }

    #[test]
    fn test_command_registry_unregister_nonexistent_command() {
        let mut registry = CommandRegistry::new();
        
        let result = registry.unregister_command("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_command_registry_unregister_extension_commands() {
        let mut registry = CommandRegistry::new();
        
        let cmd1 = SlashCommand::new("cmd1", "Command 1", create_test_handler());
        let cmd2 = SlashCommand::new("cmd2", "Command 2", create_test_handler());
        let cmd3 = SlashCommand::new("cmd3", "Command 3", create_test_handler());
        
        registry.register_command("ext1", cmd1).unwrap();
        registry.register_command("ext1", cmd2).unwrap();
        registry.register_command("ext2", cmd3).unwrap();
        
        assert_eq!(registry.command_count(), 3);
        
        let removed = registry.unregister_extension_commands("ext1");
        assert_eq!(removed.len(), 2);
        assert_eq!(registry.command_count(), 1);
        
        // ext2 的命令仍然存在
        assert!(registry.get_command("cmd3").is_some());
    }

    #[test]
    fn test_command_registry_get_command() {
        let mut registry = CommandRegistry::new();
        
        let cmd = SlashCommand::new("test-cmd", "Test command", create_test_handler())
            .with_aliases(vec!["tc".to_string()]);
        
        registry.register_command("ext1", cmd).unwrap();
        
        // 通过名称查找
        let registration = registry.get_command("test-cmd");
        assert!(registration.is_some());
        assert_eq!(registration.unwrap().command.name, "test-cmd");
        
        // 通过别名查找
        let registration = registry.get_command("tc");
        assert!(registration.is_some());
        
        // 不存在的命令
        assert!(registry.get_command("nonexistent").is_none());
    }

    #[test]
    fn test_command_registry_list_commands() {
        let mut registry = CommandRegistry::new();
        
        registry.register_command("ext1", SlashCommand::new("cmd1", "Cmd 1", create_test_handler())).unwrap();
        registry.register_command("ext2", SlashCommand::new("cmd2", "Cmd 2", create_test_handler())).unwrap();
        
        let commands = registry.list_commands();
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn test_command_registry_list_extension_commands() {
        let mut registry = CommandRegistry::new();
        
        registry.register_command("ext1", SlashCommand::new("cmd1", "Cmd 1", create_test_handler())).unwrap();
        registry.register_command("ext1", SlashCommand::new("cmd2", "Cmd 2", create_test_handler())).unwrap();
        registry.register_command("ext2", SlashCommand::new("cmd3", "Cmd 3", create_test_handler())).unwrap();
        
        let ext1_commands = registry.list_extension_commands("ext1");
        assert_eq!(ext1_commands.len(), 2);
        
        let ext2_commands = registry.list_extension_commands("ext2");
        assert_eq!(ext2_commands.len(), 1);
        
        let nonexistent = registry.list_extension_commands("nonexistent");
        assert!(nonexistent.is_empty());
    }

    #[test]
    fn test_command_registry_get_all_commands() {
        let mut registry = CommandRegistry::new();
        
        registry.register_command("ext1", SlashCommand::new("cmd1", "Cmd 1", create_test_handler())).unwrap();
        registry.register_command("ext2", SlashCommand::new("cmd2", "Cmd 2", create_test_handler())).unwrap();
        
        let commands = registry.get_all_commands();
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn test_command_registry_generate_help() {
        let mut registry = CommandRegistry::new();
        
        let cmd = SlashCommand::new("test-cmd", "A test command", create_test_handler())
            .with_usage("/test-cmd [options]")
            .with_aliases(vec!["tc".to_string()]);
        
        registry.register_command("ext1", cmd).unwrap();
        
        let help = registry.generate_help("test-cmd");
        assert!(help.is_some());
        
        let help_text = help.unwrap();
        assert!(help_text.contains("test-cmd"));
        assert!(help_text.contains("A test command"));
        assert!(help_text.contains("/test-cmd [options]"));
        assert!(help_text.contains("tc"));
        
        // 通过别名也能获取帮助
        let help_via_alias = registry.generate_help("tc");
        assert!(help_via_alias.is_some());
        
        // 不存在的命令
        assert!(registry.generate_help("nonexistent").is_none());
    }

    #[test]
    fn test_command_registry_generate_all_help() {
        let mut registry = CommandRegistry::new();
        
        registry.register_command(
            "ext1",
            SlashCommand::new("cmd1", "First command", create_test_handler())
                .with_usage("/cmd1 <arg>")
        ).unwrap();
        
        registry.register_command(
            "ext2",
            SlashCommand::new("cmd2", "Second command", create_test_handler())
                .with_aliases(vec!["c2".to_string()])
        ).unwrap();
        
        let help = registry.generate_all_help();
        
        assert!(help.contains("可用命令"));
        assert!(help.contains("cmd1"));
        assert!(help.contains("cmd2"));
        assert!(help.contains("First command"));
        assert!(help.contains("Second command"));
        assert!(help.contains("c2"));
    }

    // ==================== Debug Trait Tests ====================

    #[test]
    fn test_tool_registration_debug() {
        let tool = Arc::new(MockTool::new("test-tool"));
        let registration = ToolRegistration {
            tool,
            extension_name: "ext1".to_string(),
            parameter_schema: Some(serde_json::json!({"type": "object"})),
        };
        
        let debug_str = format!("{:?}", registration);
        assert!(debug_str.contains("ToolRegistration"));
        assert!(debug_str.contains("test-tool"));
        assert!(debug_str.contains("ext1"));
    }

    #[test]
    fn test_command_registration_debug() {
        let cmd = SlashCommand::new("test-cmd", "Test", create_test_handler())
            .with_aliases(vec!["tc".to_string()]);
        
        let registration = CommandRegistration {
            command: cmd,
            extension_name: "ext1".to_string(),
        };
        
        let debug_str = format!("{:?}", registration);
        assert!(debug_str.contains("CommandRegistration"));
        assert!(debug_str.contains("test-cmd"));
        assert!(debug_str.contains("ext1"));
    }
}
