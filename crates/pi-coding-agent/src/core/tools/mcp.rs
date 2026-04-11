//! MCP 工具适配器
//!
//! 将 MCP Server 提供的工具集成到 Agent 工具系统中

#![allow(dead_code)] // MCP 工具集成尚未完全完成

use std::sync::Arc;
use tokio::sync::Mutex;
use pi_mcp::{McpServerManager, ServerStatus};
use pi_mcp::{mcp_tool_to_ai_tool, call_result_to_text, parse_mcp_tool_name};
use pi_ai::types::Tool;
use tracing::{info, warn};

/// MCP 工具管理器
/// 
/// 管理所有 MCP Server 并提供工具发现和调用能力
pub struct McpToolManager {
    server_manager: Arc<Mutex<McpServerManager>>,
}

impl McpToolManager {
    /// 创建新的 MCP 工具管理器
    pub fn new() -> Self {
        Self {
            server_manager: Arc::new(Mutex::new(McpServerManager::new())),
        }
    }
    
    /// 从配置文件初始化 MCP Servers
    /// 
    /// 按顺序查找配置文件，只加载第一个找到的：
    /// 1. ~/.pi/mcp_servers.json
    /// 2. ~/.pi/mcp_servers.yaml
    /// 
    /// # Returns
    /// 成功返回 Ok(())，即使没有配置文件或 server 启动失败也不报错
    pub async fn init_from_config(&self) -> anyhow::Result<()> {
        let config_paths = vec![
            dirs::home_dir().map(|h| h.join(".pi").join("mcp_servers.json")),
            dirs::home_dir().map(|h| h.join(".pi").join("mcp_servers.yaml")),
        ];
        
        for path in config_paths.into_iter().flatten() {
            if path.exists() {
                info!(path = %path.display(), "Loading MCP server config");
                let configs = McpServerManager::load_config(&path)?;
                let mut manager = self.server_manager.lock().await;
                for config in configs {
                    let server_name = config.name.clone();
                    match manager.start_server(config).await {
                        Ok(()) => {
                            info!(server = %server_name, "MCP server started successfully");
                        }
                        Err(e) => {
                            warn!(server = %server_name, error = %e, "Failed to start MCP server");
                        }
                    }
                }
                break; // 只加载第一个找到的配置文件
            }
        }
        
        Ok(())
    }
    
    /// 发现所有 MCP Server 提供的工具
    /// 
    /// 遍历所有运行中的 MCP Server，收集它们提供的工具
    /// 
    /// # Returns
    /// 返回所有工具的列表，格式为 pi-ai Tool
    pub async fn discover_tools(&self) -> anyhow::Result<Vec<Tool>> {
        let mut all_tools = Vec::new();
        let mut manager = self.server_manager.lock().await;
        
        // 收集所有运行中的 server 名称
        let server_names: Vec<String> = manager.list_servers()
            .iter()
            .filter(|(_, status)| matches!(status, ServerStatus::Running))
            .map(|(name, _)| name.to_string())
            .collect();
        
        // 遍历每个 server 获取工具列表
        for name in server_names {
            if let Some(client) = manager.get_client(&name) {
                match client.list_tools().await {
                    Ok(tools) => {
                        info!(server = %name, tool_count = tools.len(), "Discovered MCP tools");
                        for mcp_tool in &tools {
                            all_tools.push(mcp_tool_to_ai_tool(mcp_tool, &name));
                        }
                    }
                    Err(e) => {
                        warn!(server = %name, error = %e, "Failed to list tools from MCP server");
                    }
                }
            }
        }
        
        Ok(all_tools)
    }
    
    /// 调用 MCP 工具
    /// 
    /// # Arguments
    /// * `namespaced_name` - 带命名空间的工具名，格式: `mcp_{server}_{tool}`
    /// * `arguments` - 工具参数
    /// 
    /// # Returns
    /// 成功返回工具结果的文本内容，失败返回错误
    pub async fn call_tool(&self, namespaced_name: &str, arguments: serde_json::Value) -> anyhow::Result<String> {
        let (server_name, tool_name) = parse_mcp_tool_name(namespaced_name)
            .ok_or_else(|| anyhow::anyhow!("Invalid MCP tool name: {}", namespaced_name))?;
        
        let mut manager = self.server_manager.lock().await;
        let client = manager.get_client(&server_name)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not found or not running", server_name))?;
        
        let result = client.call_tool(&tool_name, Some(arguments)).await?;
        let is_error = result.is_error.unwrap_or(false);
        let text = call_result_to_text(&result);
        
        if is_error {
            Err(anyhow::anyhow!("MCP tool error: {}", text))
        } else {
            Ok(text)
        }
    }
    
    /// 检查名称是否是 MCP 工具
    /// 
    /// MCP 工具名称以 "mcp_" 开头
    pub fn is_mcp_tool(name: &str) -> bool {
        name.starts_with("mcp_")
    }
    
    /// 停止所有 MCP Servers
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        let mut manager = self.server_manager.lock().await;
        manager.stop_all().await
    }
    
    /// 获取所有 MCP Server 的状态
    pub async fn list_servers(&self) -> Vec<(String, ServerStatus)> {
        let manager = self.server_manager.lock().await;
        manager.list_servers()
            .into_iter()
            .map(|(name, status)| (name.to_string(), status.clone()))
            .collect()
    }
}

impl Default for McpToolManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_mcp_tool() {
        assert!(McpToolManager::is_mcp_tool("mcp_filesystem_read_file"));
        assert!(McpToolManager::is_mcp_tool("mcp_server_tool"));
        assert!(!McpToolManager::is_mcp_tool("read_file"));
        assert!(!McpToolManager::is_mcp_tool("bash"));
        assert!(!McpToolManager::is_mcp_tool(""));
    }

    #[test]
    fn test_mcp_tool_manager_new() {
        let manager = McpToolManager::new();
        // 新创建的 manager 应该没有 server
        let servers = tokio_test::block_on(manager.list_servers());
        assert!(servers.is_empty());
    }

    #[test]
    fn test_parse_mcp_tool_name_via_manager() {
        // 通过 pi_mcp 的公共 API 测试
        let result = parse_mcp_tool_name("mcp_filesystem_read_file");
        assert_eq!(result, Some(("filesystem".to_string(), "read_file".to_string())));
        
        let result = parse_mcp_tool_name("read_file");
        assert_eq!(result, None);
    }
}
