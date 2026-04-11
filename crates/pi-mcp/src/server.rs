//! MCP Server 管理模块
//!
//! 提供 MCP Server 的配置管理、启动、停止和监控功能

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use serde::{Deserialize, Serialize};

use crate::client::McpClient;
use crate::transport::{SseTransport, StdioTransport, Transport, TransportType};

/// MCP Server 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server 名称/标识
    /// 
    /// 注意：当使用 `mcpServers` 格式时，name 可以从 key 自动填充
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub name: String,
    /// 启动命令
    pub command: String,
    /// 命令参数
    #[serde(default)]
    pub args: Vec<String>,
    /// 环境变量
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// 工作目录
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// 传输类型（默认 stdio）
    #[serde(default = "default_transport_type")]
    pub transport: TransportType,
    /// SSE 端点 URL（仅 SSE 模式需要）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

fn default_transport_type() -> TransportType {
    TransportType::Stdio
}

/// Server 运行状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    /// 已停止
    Stopped,
    /// 启动中
    Starting,
    /// 运行中
    Running,
    /// 错误
    Error(String),
}

/// Server 句柄（内部使用）
pub struct McpServerHandle {
    /// 配置
    pub config: McpServerConfig,
    /// 状态
    pub status: ServerStatus,
    /// 客户端
    pub client: Option<McpClient>,
    process: Option<Child>,
    stderr_log: Arc<Mutex<Vec<String>>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
}

impl McpServerHandle {
    fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            status: ServerStatus::Stopped,
            client: None,
            process: None,
            stderr_log: Arc::new(Mutex::new(Vec::new())),
            stderr_task: None,
        }
    }

    /// 获取 stderr 日志
    pub async fn get_logs(&self) -> Vec<String> {
        self.stderr_log.lock().await.clone()
    }

    /// 停止 server
    async fn stop(&mut self) -> anyhow::Result<()> {
        // 关闭 client
        if let Some(mut client) = self.client.take() {
            if let Err(e) = client.close().await {
                warn!(error = %e, "Error closing MCP client");
            }
        }

        // 终止 stderr 日志收集任务
        if let Some(handle) = self.stderr_task.take() {
            handle.abort();
        }

        // 终止子进程
        if let Some(mut process) = self.process.take() {
            info!(name = %self.config.name, "Killing MCP server process");
            process.kill().await.map_err(|e| {
                anyhow::anyhow!("Failed to kill MCP server process: {}", e)
            })?;
        }

        self.status = ServerStatus::Stopped;
        Ok(())
    }
}

/// MCP Server 管理器
pub struct McpServerManager {
    servers: HashMap<String, McpServerHandle>,
}

impl McpServerManager {
    /// 创建新的 Server 管理器
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    /// 从配置文件加载 Server 配置
    /// 支持 JSON 和 YAML 格式
    pub fn load_config(path: &std::path::Path) -> anyhow::Result<Vec<McpServerConfig>> {
        let content = std::fs::read_to_string(path)?;
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let configs: Vec<McpServerConfig> = match extension.as_str() {
            "json" => {
                // 支持两种格式：
                // 1. 直接是 McpServerConfig 数组
                // 2. { "mcpServers": { name: config } } 格式
                let value: serde_json::Value = serde_json::from_str(&content)?;
                
                if let Some(servers) = value.get("mcpServers") {
                    // 第二种格式
                    servers
                        .as_object()
                        .ok_or_else(|| anyhow::anyhow!("Invalid mcpServers format"))?
                        .iter()
                        .map(|(name, config_value)| {
                            let mut config: McpServerConfig = serde_json::from_value(config_value.clone())?;
                            config.name = name.clone();
                            Ok(config)
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?
                } else {
                    // 第一种格式：直接是数组
                    serde_json::from_str(&content)?
                }
            }
            "yaml" | "yml" => {
                // 同样支持两种格式
                let value: serde_yaml::Value = serde_yaml::from_str(&content)?;
                
                if let Some(servers) = value.get("mcpServers") {
                    servers
                        .as_mapping()
                        .ok_or_else(|| anyhow::anyhow!("Invalid mcpServers format"))?
                        .iter()
                        .map(|(name, config_value)| {
                            let name_str = name
                                .as_str()
                                .ok_or_else(|| anyhow::anyhow!("Invalid server name"))?
                                .to_string();
                            let mut config: McpServerConfig = serde_yaml::from_value(config_value.clone())?;
                            config.name = name_str;
                            Ok(config)
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?
                } else {
                    serde_yaml::from_str(&content)?
                }
            }
            _ => return Err(anyhow::anyhow!("Unsupported config file format: {}", extension)),
        };

        Ok(configs)
    }

    /// 启动指定 Server
    pub async fn start_server(&mut self, config: McpServerConfig) -> anyhow::Result<()> {
        let name = config.name.clone();
        
        if self.servers.contains_key(&name) {
            return Err(anyhow::anyhow!("Server '{}' already exists", name));
        }

        info!(name = %name, command = %config.command, "Starting MCP server");

        let mut handle = McpServerHandle::new(config);
        handle.status = ServerStatus::Starting;

        // 根据 transport 类型创建 transport
        let transport: Box<dyn Transport> = match handle.config.transport {
            TransportType::Stdio => {
                let mut cmd = Command::new(&handle.config.command);
                cmd.args(&handle.config.args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                // 设置环境变量
                if !handle.config.env.is_empty() {
                    cmd.envs(&handle.config.env);
                }

                // 设置工作目录
                if let Some(ref cwd) = handle.config.cwd {
                    cmd.current_dir(cwd);
                }

                let mut child = cmd.spawn().map_err(|e| {
                    anyhow::anyhow!("Failed to spawn process '{}': {}", handle.config.command, e)
                })?;

                // 启动 stderr 日志收集
                if let Some(stderr) = child.stderr.take() {
                    let log_arc = handle.stderr_log.clone();
                    let server_name = name.clone();
                    let stderr_handle = tokio::spawn(async move {
                        let reader = BufReader::new(stderr);
                        let mut lines = reader.lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            debug!(server = %server_name, stderr = %line, "MCP server stderr");
                            let mut logs = log_arc.lock().await;
                            logs.push(line);
                        }
                    });
                    handle.stderr_task = Some(stderr_handle);
                }

                let stdin = child.stdin.take().ok_or_else(|| {
                    anyhow::anyhow!("Failed to get stdin of child process")
                })?;

                let stdout = child.stdout.take().ok_or_else(|| {
                    anyhow::anyhow!("Failed to get stdout of child process")
                })?;

                handle.process = Some(child);

                Box::new(StdioTransport::from_pipes(stdin, stdout)?)
            }
            TransportType::Sse => {
                // SSE 模式：启动子进程（如果有 command）
                if !handle.config.command.is_empty() {
                    let mut cmd = Command::new(&handle.config.command);
                    cmd.args(&handle.config.args)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped());

                    if !handle.config.env.is_empty() {
                        cmd.envs(&handle.config.env);
                    }

                    if let Some(ref cwd) = handle.config.cwd {
                        cmd.current_dir(cwd);
                    }

                    let mut child = cmd.spawn().map_err(|e| {
                        anyhow::anyhow!("Failed to spawn process '{}': {}", handle.config.command, e)
                    })?;

                    // 启动 stderr 日志收集
                    if let Some(stderr) = child.stderr.take() {
                        let log_arc = handle.stderr_log.clone();
                        let server_name = name.clone();
                        let stderr_handle = tokio::spawn(async move {
                            let reader = BufReader::new(stderr);
                            let mut lines = reader.lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                debug!(server = %server_name, stderr = %line, "MCP server stderr");
                                let mut logs = log_arc.lock().await;
                                logs.push(line);
                            }
                        });
                        handle.stderr_task = Some(stderr_handle);
                    }

                    handle.process = Some(child);
                }

                // 创建 SSE transport
                let url = handle
                    .config
                    .url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("URL is required for SSE transport"))?;
                
                Box::new(SseTransport::new(url).await?)
            }
        };

        // 创建 McpClient
        let mut client = McpClient::new(transport);

        // 执行握手
        match client.handshake().await {
            Ok(result) => {
                info!(
                    name = %name,
                    server = %result.server_info.name,
                    version = %result.server_info.version,
                    "MCP server started successfully"
                );
                handle.client = Some(client);
                handle.status = ServerStatus::Running;
            }
            Err(e) => {
                error!(name = %name, error = %e, "MCP server handshake failed");
                handle.status = ServerStatus::Error(e.to_string());
                
                // 清理
                if let Some(mut process) = handle.process.take() {
                    let _ = process.kill().await;
                }
                if let Some(task) = handle.stderr_task.take() {
                    task.abort();
                }
                
                return Err(anyhow::anyhow!("Handshake failed: {}", e));
            }
        }

        self.servers.insert(name, handle);
        Ok(())
    }

    /// 停止指定 Server
    pub async fn stop_server(&mut self, name: &str) -> anyhow::Result<()> {
        let mut handle = self
            .servers
            .remove(name)
            .ok_or_else(|| anyhow::anyhow!("Server '{}' not found", name))?;

        handle.stop().await?;
        info!(name = %name, "MCP server stopped");
        Ok(())
    }

    /// 重启指定 Server
    pub async fn restart_server(&mut self, name: &str) -> anyhow::Result<()> {
        let config = self
            .servers
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Server '{}' not found", name))?
            .config
            .clone();

        self.stop_server(name).await?;
        self.start_server(config).await?;
        Ok(())
    }

    /// 停止所有 Server
    pub async fn stop_all(&mut self) -> anyhow::Result<()> {
        let names: Vec<String> = self.servers.keys().cloned().collect();
        for name in names {
            if let Err(e) = self.stop_server(&name).await {
                warn!(name = %name, error = %e, "Error stopping server");
            }
        }
        Ok(())
    }

    /// 获取 Server 状态
    pub fn get_status(&self, name: &str) -> Option<&ServerStatus> {
        self.servers.get(name).map(|h| &h.status)
    }

    /// 获取所有 Server 名称和状态
    pub fn list_servers(&self) -> Vec<(&str, &ServerStatus)> {
        self.servers
            .iter()
            .map(|(name, handle)| (name.as_str(), &handle.status))
            .collect()
    }

    /// 获取 Server 的 MCP Client（用于工具调用等）
    pub fn get_client(&mut self, name: &str) -> Option<&mut McpClient> {
        self.servers
            .get_mut(name)
            .and_then(|h| h.client.as_mut())
    }

    /// 健康检查 — 对所有运行中的 server 发送 ping
    pub async fn health_check(&mut self) -> HashMap<String, bool> {
        let mut results = HashMap::new();

        for (name, handle) in &mut self.servers {
            if !matches!(handle.status, ServerStatus::Running) {
                results.insert(name.clone(), false);
                continue;
            }

            if let Some(ref mut client) = handle.client {
                // 使用 timeout 检查
                match tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    client.ping()
                ).await {
                    Ok(Ok(())) => {
                        results.insert(name.clone(), true);
                    }
                    Ok(Err(e)) => {
                        warn!(name = %name, error = %e, "Health check failed");
                        handle.status = ServerStatus::Error(format!("Health check failed: {}", e));
                        results.insert(name.clone(), false);
                    }
                    Err(_) => {
                        warn!(name = %name, "Health check timeout");
                        handle.status = ServerStatus::Error("Health check timeout".to_string());
                        results.insert(name.clone(), false);
                    }
                }
            } else {
                results.insert(name.clone(), false);
            }
        }

        results
    }

    /// 获取 Server 的 stderr 日志
    pub fn get_logs(&self, name: &str) -> Option<Arc<Mutex<Vec<String>>>> {
        self.servers
            .get(name)
            .map(|h| h.stderr_log.clone())
    }
}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new()
    }
}

// === 单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config_json() {
        let json_content = r#"{
            "mcpServers": {
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
                    "transport": "stdio"
                },
                "github": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-github"],
                    "env": {
                        "GITHUB_TOKEN": "xxx"
                    },
                    "transport": "stdio"
                }
            }
        }"#;

        let mut temp_file = NamedTempFile::with_suffix(".json").unwrap();
        temp_file.write_all(json_content.as_bytes()).unwrap();

        let configs = McpServerManager::load_config(temp_file.path()).unwrap();
        assert_eq!(configs.len(), 2);

        let fs_config = configs.iter().find(|c| c.name == "filesystem").unwrap();
        assert_eq!(fs_config.command, "npx");
        assert_eq!(fs_config.args, vec!["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]);
        assert!(matches!(fs_config.transport, TransportType::Stdio));

        let gh_config = configs.iter().find(|c| c.name == "github").unwrap();
        assert_eq!(gh_config.command, "npx");
        assert_eq!(gh_config.env.get("GITHUB_TOKEN"), Some(&"xxx".to_string()));
    }

    #[test]
    fn test_load_config_yaml() {
        let yaml_content = r#"
mcpServers:
  filesystem:
    command: npx
    args:
      - -y
      - "@modelcontextprotocol/server-filesystem"
      - /tmp
    transport: stdio
  github:
    command: npx
    args:
      - -y
      - "@modelcontextprotocol/server-github"
    env:
      GITHUB_TOKEN: xxx
    transport: stdio
"#;

        let mut temp_file = NamedTempFile::with_suffix(".yaml").unwrap();
        temp_file.write_all(yaml_content.as_bytes()).unwrap();

        let configs = McpServerManager::load_config(temp_file.path()).unwrap();
        assert_eq!(configs.len(), 2);

        let fs_config = configs.iter().find(|c| c.name == "filesystem").unwrap();
        assert_eq!(fs_config.command, "npx");
        assert_eq!(fs_config.args.len(), 3);
    }

    #[test]
    fn test_load_config_direct_array() {
        let json_content = r#"[
            {
                "name": "test-server",
                "command": "echo",
                "args": ["hello"],
                "transport": "stdio"
            }
        ]"#;

        let mut temp_file = NamedTempFile::with_suffix(".json").unwrap();
        temp_file.write_all(json_content.as_bytes()).unwrap();

        let configs = McpServerManager::load_config(temp_file.path()).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "test-server");
    }

    #[test]
    fn test_server_status() {
        let manager = McpServerManager::new();
        assert!(manager.get_status("nonexistent").is_none());
        
        let servers = manager.list_servers();
        assert!(servers.is_empty());
    }

    #[test]
    fn test_mcp_server_config_default_transport() {
        let config = McpServerConfig {
            name: "test".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            cwd: None,
            transport: default_transport_type(),
            url: None,
        };

        assert!(matches!(config.transport, TransportType::Stdio));
    }

    #[test]
    fn test_mcp_server_config_serialization() {
        let config = McpServerConfig {
            name: "test".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "server".to_string()],
            env: HashMap::from([("KEY".to_string(), "value".to_string())]),
            cwd: Some(PathBuf::from("/tmp")),
            transport: TransportType::Stdio,
            url: Some("http://localhost:3000".to_string()),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"name\":\"test\""));
        assert!(json.contains("\"command\":\"npx\""));
        
        let deserialized: McpServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.command, "npx");
    }

    #[test]
    fn test_server_status_equality() {
        assert_eq!(ServerStatus::Stopped, ServerStatus::Stopped);
        assert_eq!(ServerStatus::Starting, ServerStatus::Starting);
        assert_eq!(ServerStatus::Running, ServerStatus::Running);
        assert_eq!(
            ServerStatus::Error("test".to_string()),
            ServerStatus::Error("test".to_string())
        );
        
        assert_ne!(ServerStatus::Stopped, ServerStatus::Running);
        assert_ne!(
            ServerStatus::Error("a".to_string()),
            ServerStatus::Error("b".to_string())
        );
    }
}
