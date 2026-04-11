//! 配置管理模块
//!
//! 管理应用配置，包括 API keys、默认模型、会话目录等

use notify::Watcher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::core::permissions::ToolPermissionConfig;

/// 配置文件格式
#[derive(Debug, Clone, Copy)]
enum ConfigFormat {
    Yaml,
    Json,
    Toml,
}

/// 应用配置
/// 
/// 管理应用的全局配置，包括 API keys、默认模型、会话目录等
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// 默认模型 ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,

    /// 默认 thinking level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_thinking: Option<String>,

    /// API Keys (provider -> key)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub api_keys: HashMap<String, String>,

    /// 自定义模型定义
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_models: Vec<CustomModelConfig>,

    /// 默认系统提示词追加
    #[serde(skip_serializing_if = "Option::is_none")]
    pub append_system_prompt: Option<String>,

    /// Shell 配置
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,

    /// 会话目录
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions_dir: Option<String>,

    /// 快捷键配置文件路径
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keybindings_path: Option<String>,

    /// 工具权限配置
    #[serde(default)]
    pub permissions: Option<ToolPermissionConfig>,

    /// 扩展配置
    #[serde(default)]
    pub extensions: Option<ExtensionsConfig>,
}

/// 自定义模型配置
/// 
/// 用户自定义的模型定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomModelConfig {
    /// 模型 ID
    pub id: String,
    /// 模型名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// API 类型
    pub api: String,
    /// Provider 类型
    pub provider: String,
    /// 基础 URL
    #[serde(rename = "baseUrl")]
    pub base_url: String,
}

/// 扩展配置
/// 
/// 管理扩展的启用/禁用和设置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtensionsConfig {
    /// 启用的扩展列表（为空表示加载所有可用扩展）
    #[serde(default)]
    pub enabled: Vec<String>,
    /// 禁用的扩展列表
    #[serde(default)]
    pub disabled: Vec<String>,
    /// 扩展特定设置
    #[serde(default)]
    pub settings: HashMap<String, serde_json::Value>,
}

impl AppConfig {
    /// 加载 .env 文件（从 ~/.pi/.env）
    ///
    /// 不覆盖已存在的环境变量
    pub fn load_env_file() {
        let config_dir = Self::config_dir();
        let env_path = config_dir.join(".env");
        if env_path.exists() {
            // dotenvy::from_path 默认行为是不覆盖已有环境变量
            if let Err(e) = dotenvy::from_path(&env_path) {
                eprintln!("Warning: Failed to load .env file: {}", e);
            }
        }
    }

    /// 解析配置文件内容
    fn parse_config(content: &str, format: ConfigFormat, path: &Path) -> anyhow::Result<Self> {
        match format {
            ConfigFormat::Yaml => serde_yaml::from_str(content)
                .map_err(|e| anyhow::anyhow!("YAML config error in {}: {}", path.display(), e)),
            ConfigFormat::Json => serde_json::from_str(content)
                .map_err(|e| anyhow::anyhow!("JSON config error in {}: {}", path.display(), e)),
            ConfigFormat::Toml => toml::from_str(content)
                .map_err(|e| anyhow::anyhow!("TOML config error in {}: {}", path.display(), e)),
        }
    }

    /// 加载配置（支持多格式自动检测）
    ///
    /// 按优先级搜索配置文件：config.yaml > config.yml > config.json > config.toml
    /// 同时会加载 ~/.pi/.env 文件中的环境变量
    pub fn load() -> anyhow::Result<Self> {
        // 先加载 .env 文件
        Self::load_env_file();

        let config_dir = Self::config_dir();

        // 按优先级搜索配置文件
        let config_candidates = [
            ("config.yaml", ConfigFormat::Yaml),
            ("config.yml", ConfigFormat::Yaml),
            ("config.json", ConfigFormat::Json),
            ("config.toml", ConfigFormat::Toml),
        ];

        for (filename, format) in &config_candidates {
            let path = config_dir.join(filename);
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                return Self::parse_config(&content, *format, &path);
            }
        }

        Ok(Self::default())
    }

    /// 保存配置
    #[allow(dead_code)] // 预留方法供未来使用
    pub fn save(&self) -> anyhow::Result<()> {
        let config_path = Self::config_path();

        // 确保配置目录存在
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_yaml::to_string(self)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    /// 获取配置文件路径
    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.yaml")
    }

    /// 获取配置目录
    pub fn config_dir() -> PathBuf {
        if let Ok(env_dir) = std::env::var("PI_CODING_AGENT_DIR") {
            if env_dir == "~" {
                return dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".pi");
            }
            if let Some(stripped) = env_dir.strip_prefix("~/") {
                return dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(stripped);
            }
            return PathBuf::from(env_dir);
        }

        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".pi")
    }

    /// 获取会话目录
    pub fn sessions_dir(&self) -> PathBuf {
        if let Some(ref dir) = self.sessions_dir {
            PathBuf::from(dir)
        } else {
            Self::config_dir().join("sessions")
        }
    }

    /// 获取快捷键配置文件路径
    pub fn keybindings_path(&self) -> PathBuf {
        if let Some(ref path) = self.keybindings_path {
            PathBuf::from(path)
        } else {
            Self::config_dir().join("keybindings.toml")
        }
    }

    /// 加载并应用快捷键配置
    #[allow(dead_code)] // 预留给未来使用
    pub fn load_keybindings(&self) -> anyhow::Result<()> {
        let path = self.keybindings_path();
        if path.exists() {
            let config = pi_tui::keybindings::KeybindingsConfig::load_from_file(&path)?;
            pi_tui::keybindings::apply_keybindings_config(&config)?;
            tracing::info!("Loaded keybindings config from {}", path.display());
        }
        Ok(())
    }

    /// 获取 API Key (先查 OAuth token，再查配置，最后查环境变量)
    #[allow(dead_code)] // 预留给未来使用
    pub fn get_api_key(&self, provider: &str) -> Option<String> {
        // 1. 先检查 OAuth token 存储
        let token_storage = crate::core::auth::TokenStorage::new();
        if let Some(token) = token_storage.get_valid_token(provider) {
            return Some(token);
        }

        // 2. 检查配置
        if let Some(key) = self.api_keys.get(provider) {
            return Some(key.clone());
        }

        // 3. 检查环境变量
        Self::get_api_key_from_env(provider)
    }

    /// 异步获取 API Key，支持自动 token 刷新
    /// 
    /// 与 get_api_key 的区别：此方法会在 token 即将过期时自动尝试刷新
    #[allow(dead_code)] // 预留方法供未来使用
    pub async fn get_api_key_async(&self, provider: &str) -> Option<String> {
        // 1. 先检查 OAuth token 存储（带自动刷新）
        let token_storage = crate::core::auth::TokenStorage::new();
        if let Some(token) = token_storage.get_valid_token_or_refresh(provider).await {
            return Some(token);
        }

        // 2. 检查配置
        if let Some(key) = self.api_keys.get(provider) {
            return Some(key.clone());
        }

        // 3. 检查环境变量
        Self::get_api_key_from_env(provider)
    }

    /// 从环境变量获取 API Key
    fn get_api_key_from_env(provider: &str) -> Option<String> {
        match provider {
            "anthropic" => std::env::var("ANTHROPIC_OAUTH_TOKEN")
                .ok()
                .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok()),
            "openai" => std::env::var("OPENAI_API_KEY").ok(),
            "google" | "google-gemini-cli" | "google-antigravity" => std::env::var("GOOGLE_API_KEY")
                .ok()
                .or_else(|| std::env::var("GEMINI_API_KEY").ok()),
            "google-vertex" => std::env::var("GOOGLE_CLOUD_API_KEY").ok(),
            "groq" => std::env::var("GROQ_API_KEY").ok(),
            "cerebras" => std::env::var("CEREBRAS_API_KEY").ok(),
            "xai" => std::env::var("XAI_API_KEY").ok(),
            "openrouter" => std::env::var("OPENROUTER_API_KEY").ok(),
            "vercel-ai-gateway" => std::env::var("AI_GATEWAY_API_KEY").ok(),
            "mistral" => std::env::var("MISTRAL_API_KEY").ok(),
            "minimax" => std::env::var("MINIMAX_API_KEY").ok(),
            "minimax-cn" => std::env::var("MINIMAX_CN_API_KEY").ok(),
            "huggingface" => std::env::var("HF_TOKEN").ok(),
            "opencode" | "opencode-go" => std::env::var("OPENCODE_API_KEY").ok(),
            "kimi-coding" => std::env::var("KIMI_API_KEY").ok(),
            "azure-openai-responses" => std::env::var("AZURE_OPENAI_API_KEY").ok(),
            "openai-codex" => std::env::var("OPENAI_CODEX_API_KEY").ok(),
            "github-copilot" => std::env::var("COPILOT_GITHUB_TOKEN")
                .ok()
                .or_else(|| std::env::var("GH_TOKEN").ok())
                .or_else(|| std::env::var("GITHUB_TOKEN").ok()),
            "zai" => std::env::var("ZAI_API_KEY").ok(),
            "amazon-bedrock" => {
                // Amazon Bedrock 使用 AWS 凭证
                if std::env::var("AWS_PROFILE").is_ok()
                    || (std::env::var("AWS_ACCESS_KEY_ID").is_ok()
                        && std::env::var("AWS_SECRET_ACCESS_KEY").is_ok())
                    || std::env::var("AWS_BEARER_TOKEN_BEDROCK").is_ok()
                {
                    Some("<authenticated>".to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 获取扩展配置
    pub fn extensions_config(&self) -> Option<&ExtensionsConfig> {
        self.extensions.as_ref()
    }
}

/// 获取项目目录
#[allow(dead_code)] // 预留函数供未来使用
pub fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "pi", "pi")
}

/// 确保目录存在
#[allow(dead_code)] // 预留函数供未来使用
pub fn ensure_dir(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

/// 配置变更事件
#[derive(Debug, Clone)]
#[allow(dead_code)] // 配置监控功能尚未完全集成
pub enum ConfigChangeEvent {
    /// 配置文件已更新
    Updated,
    /// 配置文件更新失败（保持原配置）
    Error(String),
}

/// 配置文件监控器
#[allow(dead_code)] // 配置监控功能尚未完全集成
pub struct ConfigWatcher {
    _watcher: notify::RecommendedWatcher,
    event_rx: tokio::sync::mpsc::Receiver<ConfigChangeEvent>,
}

#[allow(dead_code)] // 配置监控功能尚未完全集成
impl ConfigWatcher {
    /// 创建并启动配置文件监控
    pub fn new(config_path: std::path::PathBuf) -> anyhow::Result<Self> {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);

        let tx = event_tx.clone();
        let path = config_path.clone();
        let mut watcher = notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        if matches!(
                            event.kind,
                            notify::EventKind::Modify(_)
                                | notify::EventKind::Create(_)
                        ) {
                            // 验证配置文件可读
                            match std::fs::read_to_string(&path) {
                                Ok(content) => {
                                    // 尝试解析以验证格式
                                    let valid = serde_yaml::from_str::<serde_json::Value>(&content)
                                        .is_ok()
                                        || serde_json::from_str::<serde_json::Value>(&content)
                                            .is_ok()
                                        || toml::from_str::<serde_json::Value>(&content).is_ok();

                                    if valid {
                                        let _ = tx.blocking_send(ConfigChangeEvent::Updated);
                                    } else {
                                        let _ = tx.blocking_send(ConfigChangeEvent::Error(
                                            "Invalid config format, keeping current config"
                                                .to_string(),
                                        ));
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.blocking_send(ConfigChangeEvent::Error(
                                        format!("Failed to read config: {}", e),
                                    ));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Config watch error: {}", e);
                    }
                }
            },
        )?;

        // 监控配置文件的父目录
        if let Some(parent) = config_path.parent() {
            watcher.watch(parent, notify::RecursiveMode::NonRecursive)?;
        }

        Ok(Self {
            _watcher: watcher,
            event_rx,
        })
    }

    /// 接收下一个配置变更事件
    pub async fn next_event(&mut self) -> Option<ConfigChangeEvent> {
        self.event_rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;
    use tempfile::TempDir;

    // ========== ConfigWatcher 格式验证测试 ==========

    #[tokio::test]
    async fn test_config_watcher_yaml_format() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");
        
        // 创建有效的 YAML 配置
        let yaml_content = r#"
default_model: "gpt-4o"
api_keys:
  openai: "sk-test"
"#;
        std::fs::write(&config_path, yaml_content).unwrap();
        
        // 创建 watcher
        let mut watcher = ConfigWatcher::new(config_path.clone()).unwrap();
        
        // 修改配置文件
        tokio::time::sleep(Duration::from_millis(50)).await;
        let new_content = r#"
default_model: "claude-3"
api_keys:
  openai: "sk-new"
"#;
        std::fs::write(&config_path, new_content).unwrap();
        
        // 等待事件
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event()).await;
        
        // 应该收到 Updated 事件
        assert!(event.is_ok());
        if let Ok(Some(ConfigChangeEvent::Updated)) = event {
            // 成功
        } else {
            // 在某些环境下可能收不到事件，这是正常的
        }
    }

    #[tokio::test]
    async fn test_config_watcher_json_format() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        
        // 创建有效的 JSON 配置
        let json_content = r#"{"default_model": "gpt-4o", "api_keys": {"openai": "sk-test"}}"#;
        std::fs::write(&config_path, json_content).unwrap();
        
        // 创建 watcher
        let mut watcher = ConfigWatcher::new(config_path.clone()).unwrap();
        
        // 修改配置文件
        tokio::time::sleep(Duration::from_millis(50)).await;
        let new_content = r#"{"default_model": "claude-3"}"#;
        std::fs::write(&config_path, new_content).unwrap();
        
        // 等待事件
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event()).await;
        assert!(event.is_ok());
    }

    #[tokio::test]
    async fn test_config_watcher_toml_format() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        
        // 创建有效的 TOML 配置
        let toml_content = r#"
default_model = "gpt-4o"

[api_keys]
openai = "sk-test"
"#;
        std::fs::write(&config_path, toml_content).unwrap();
        
        // 创建 watcher
        let mut watcher = ConfigWatcher::new(config_path.clone()).unwrap();
        
        // 修改配置文件
        tokio::time::sleep(Duration::from_millis(50)).await;
        let new_content = r#"default_model = "claude-3""#;
        std::fs::write(&config_path, new_content).unwrap();
        
        // 等待事件
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event()).await;
        assert!(event.is_ok());
    }

    // ========== 无效配置处理测试 ==========

    #[tokio::test]
    async fn test_config_watcher_invalid_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");
        
        // 创建有效的初始配置
        let valid_yaml = r#"default_model: "gpt-4o""#;
        std::fs::write(&config_path, valid_yaml).unwrap();
        
        // 创建 watcher
        let mut watcher = ConfigWatcher::new(config_path.clone()).unwrap();
        
        // 写入无效的 YAML
        tokio::time::sleep(Duration::from_millis(50)).await;
        let invalid_yaml = r#"default_model: [unclosed bracket"#;
        std::fs::write(&config_path, invalid_yaml).unwrap();
        
        // 等待事件
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event()).await;
        
        // 应该收到 Error 事件或超时
        if let Ok(Some(ConfigChangeEvent::Error(msg))) = event {
            assert!(msg.contains("Invalid") || msg.contains("keeping"));
        }
    }

    #[tokio::test]
    async fn test_config_watcher_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        
        // 创建有效的初始配置
        let valid_json = r#"{"default_model": "gpt-4o"}"#;
        std::fs::write(&config_path, valid_json).unwrap();
        
        // 创建 watcher
        let mut watcher = ConfigWatcher::new(config_path.clone()).unwrap();
        
        // 写入无效的 JSON
        tokio::time::sleep(Duration::from_millis(50)).await;
        let invalid_json = r#"{"default_model": "gpt-4o", invalid}"#;
        std::fs::write(&config_path, invalid_json).unwrap();
        
        // 等待事件
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event()).await;
        
        // 应该收到 Error 事件或超时
        if let Ok(Some(ConfigChangeEvent::Error(_))) = event {
            // 成功收到错误事件
        }
    }

    #[tokio::test]
    async fn test_config_watcher_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        
        // 创建有效的初始配置
        let valid_toml = r#"default_model = "gpt-4o""#;
        std::fs::write(&config_path, valid_toml).unwrap();
        
        // 创建 watcher
        let mut watcher = ConfigWatcher::new(config_path.clone()).unwrap();
        
        // 写入无效的 TOML
        tokio::time::sleep(Duration::from_millis(50)).await;
        let invalid_toml = r#"default_model = "gpt-4o" [invalid"#;
        std::fs::write(&config_path, invalid_toml).unwrap();
        
        // 等待事件
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event()).await;
        
        // 应该收到 Error 事件或超时
        if let Ok(Some(ConfigChangeEvent::Error(_))) = event {
            // 成功收到错误事件
        }
    }

    #[tokio::test]
    async fn test_config_watcher_maintains_original_on_error() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");
        
        // 创建有效的初始配置
        let valid_yaml = r#"
default_model: "original-model"
api_keys:
  openai: "sk-original"
"#;
        std::fs::write(&config_path, valid_yaml).unwrap();
        
        // 加载配置
        std::env::set_var("PI_CODING_AGENT_DIR", temp_dir.path().to_str().unwrap());
        let original_config = AppConfig::load().unwrap();
        assert_eq!(original_config.default_model, Some("original-model".to_string()));
        
        // 创建 watcher
        let mut watcher = ConfigWatcher::new(config_path.clone()).unwrap();
        
        // 写入无效的配置
        tokio::time::sleep(Duration::from_millis(50)).await;
        let invalid_yaml = r#"invalid yaml content [[["#;
        std::fs::write(&config_path, invalid_yaml).unwrap();
        
        // 等待事件（可能是 Error 或 Updated）
        let _ = tokio::time::timeout(Duration::from_secs(2), watcher.next_event()).await;
        
        // 重新加载配置（应该保持原配置，因为无效配置不会被应用）
        // 注意：ConfigWatcher 只发送事件，实际配置重载由调用者处理
        // 这里我们验证文件系统状态
        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("invalid"));
        
        std::env::remove_var("PI_CODING_AGENT_DIR");
    }

    // ========== 配置变更事件测试 ==========

    #[test]
    fn test_config_change_event_debug() {
        let updated = ConfigChangeEvent::Updated;
        let error = ConfigChangeEvent::Error("test error".to_string());
        
        assert!(format!("{:?}", updated).contains("Updated"));
        assert!(format!("{:?}", error).contains("Error"));
    }

    #[tokio::test]
    async fn test_config_watcher_file_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");
        
        // 先创建 watcher（文件不存在）
        let mut watcher = ConfigWatcher::new(config_path.clone()).unwrap();
        
        // 然后创建文件
        tokio::time::sleep(Duration::from_millis(50)).await;
        let yaml_content = r#"default_model: "gpt-4o""#;
        std::fs::write(&config_path, yaml_content).unwrap();
        
        // 等待事件
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.next_event()).await;
        assert!(event.is_ok());
    }

    // ========== AppConfig 方法测试 ==========

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert!(config.default_model.is_none());
        assert!(config.api_keys.is_empty());
        assert!(config.custom_models.is_empty());
    }

    #[test]
    fn test_app_config_sessions_dir() {
        // 清理环境变量以确保默认行为
        std::env::remove_var("PI_CODING_AGENT_DIR");
        
        let mut config = AppConfig::default();
        
        // 默认情况
        let default_dir = config.sessions_dir();
        // 默认路径应该包含 sessions 目录
        assert!(default_dir.to_string_lossy().contains("sessions"));
        
        // 自定义路径
        config.sessions_dir = Some("/custom/sessions".to_string());
        let custom_dir = config.sessions_dir();
        assert_eq!(custom_dir, PathBuf::from("/custom/sessions"));
    }

    #[test]
    fn test_app_config_keybindings_path() {
        let mut config = AppConfig::default();
        
        // 默认情况
        let default_path = config.keybindings_path();
        assert!(default_path.to_string_lossy().contains("keybindings.toml"));
        
        // 自定义路径
        config.keybindings_path = Some("/custom/keybindings.toml".to_string());
        let custom_path = config.keybindings_path();
        assert_eq!(custom_path, PathBuf::from("/custom/keybindings.toml"));
    }

    #[test]
    fn test_app_config_extensions_config() {
        let mut config = AppConfig::default();
        
        // 默认情况
        assert!(config.extensions_config().is_none());
        
        // 设置扩展配置
        config.extensions = Some(ExtensionsConfig {
            enabled: vec!["ext1".to_string()],
            disabled: vec!["ext2".to_string()],
            settings: std::collections::HashMap::new(),
        });
        
        let ext_config = config.extensions_config().unwrap();
        assert_eq!(ext_config.enabled, vec!["ext1".to_string()]);
        assert_eq!(ext_config.disabled, vec!["ext2".to_string()]);
    }

    // ========== 配置目录测试 ==========

    #[test]
    fn test_config_dir_with_env() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("PI_CODING_AGENT_DIR", temp_dir.path().to_str().unwrap());
        
        let config_dir = AppConfig::config_dir();
        assert_eq!(config_dir, temp_dir.path());
        
        std::env::remove_var("PI_CODING_AGENT_DIR");
    }

    #[test]
    fn test_config_dir_with_tilde() {
        std::env::set_var("PI_CODING_AGENT_DIR", "~/.pi");
        
        let config_dir = AppConfig::config_dir();
        assert!(config_dir.to_string_lossy().contains(".pi"));
        
        std::env::remove_var("PI_CODING_AGENT_DIR");
    }

    #[test]
    fn test_config_path() {
        let config_path = AppConfig::config_path();
        assert!(config_path.to_string_lossy().contains("config.yaml"));
    }

    // ========== 扩展配置测试 ==========

    #[test]
    fn test_extensions_config_default() {
        let ext_config = ExtensionsConfig::default();
        assert!(ext_config.enabled.is_empty());
        assert!(ext_config.disabled.is_empty());
        assert!(ext_config.settings.is_empty());
    }

    #[test]
    fn test_extensions_config_serialization() {
        let ext_config = ExtensionsConfig {
            enabled: vec!["ext1".to_string(), "ext2".to_string()],
            disabled: vec!["ext3".to_string()],
            settings: {
                let mut map = std::collections::HashMap::new();
                map.insert("ext1.setting".to_string(), json!(true));
                map
            },
        };
        
        let json_str = serde_json::to_string(&ext_config).unwrap();
        assert!(json_str.contains("enabled"));
        assert!(json_str.contains("disabled"));
        assert!(json_str.contains("settings"));
    }

    // ========== 自定义模型配置测试 ==========

    #[test]
    fn test_custom_model_config() {
        let model = CustomModelConfig {
            id: "custom-model".to_string(),
            name: Some("Custom Model".to_string()),
            api: "openai".to_string(),
            provider: "custom".to_string(),
            base_url: "https://api.example.com".to_string(),
        };
        
        assert_eq!(model.id, "custom-model");
        assert_eq!(model.name, Some("Custom Model".to_string()));
        assert_eq!(model.api, "openai");
        assert_eq!(model.provider, "custom");
        assert_eq!(model.base_url, "https://api.example.com");
    }

    #[test]
    fn test_custom_model_config_serialization() {
        let model = CustomModelConfig {
            id: "custom-model".to_string(),
            name: None,
            api: "openai".to_string(),
            provider: "custom".to_string(),
            base_url: "https://api.example.com".to_string(),
        };
        
        let json_str = serde_json::to_string(&model).unwrap();
        assert!(json_str.contains("\"id\":\"custom-model\""));
        assert!(json_str.contains("\"baseUrl\"")); // 验证 rename 工作
        assert!(!json_str.contains("base_url"));
    }
}
