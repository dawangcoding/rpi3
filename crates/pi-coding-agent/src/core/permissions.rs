//! 工具权限管理模块
//!
//! 提供工具执行权限控制，包括权限级别定义、权限检查、Bash安全增强等功能

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

/// 权限级别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PermissionLevel {
    /// 始终允许
    AlwaysAllow,
    /// 首次询问
    #[default]
    AskFirst,
    /// 每次询问
    AskEveryTime,
    /// 拒绝
    Deny,
}


/// 权限检查结果
#[derive(Debug, Clone)]
#[allow(dead_code)] // 预留枚举供未来使用
pub enum PermissionCheckResult {
    /// 允许执行
    Allowed,
    /// 需要确认
    NeedsConfirmation {
        /// 确认原因
        reason: String
    },
    /// 拒绝执行
    Denied {
        /// 拒绝原因
        reason: String
    },
}

/// 工具权限配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionConfig {
    /// 默认权限级别
    #[serde(default)]
    pub default_level: PermissionLevel,
    /// 工具级别覆盖
    #[serde(default)]
    pub tool_overrides: HashMap<String, PermissionLevel>,
    /// Bash 黑名单命令
    #[serde(default)]
    pub bash_blocked_commands: Vec<String>,
    /// Bash 白名单命令（如果设置，只允许这些命令）
    pub bash_allowed_commands: Option<Vec<String>>,
    /// 最大执行时间（秒）
    #[serde(default = "default_max_execution_time")]
    pub max_execution_time_secs: u64,
}

fn default_max_execution_time() -> u64 {
    120
}

impl Default for ToolPermissionConfig {
    fn default() -> Self {
        Self {
            default_level: PermissionLevel::AskFirst,
            tool_overrides: HashMap::new(),
            bash_blocked_commands: default_blocked_commands(),
            bash_allowed_commands: None,
            max_execution_time_secs: 120,
        }
    }
}

/// 默认黑名单命令
fn default_blocked_commands() -> Vec<String> {
    vec![
        "rm -rf /".into(),
        "rm -rf ~".into(),
        "sudo".into(),
        "mkfs".into(),
        "dd".into(),
        "chmod 777".into(),
        ":(){:|:&};:".into(),
    ]
}

/// 权限管理器
#[allow(dead_code)] // 字段和方法供未来权限控制功能使用
pub struct PermissionManager {
    config: ToolPermissionConfig,
    granted_tools: HashSet<String>,
}

impl PermissionManager {
    #![allow(dead_code)] // 多个方法供未来扩展使用
    /// 创建新的权限管理器
    pub fn new(config: ToolPermissionConfig) -> Self {
        Self {
            config,
            granted_tools: HashSet::new(),
        }
    }

    /// 使用默认配置创建
    pub fn with_default() -> Self {
        Self::new(ToolPermissionConfig::default())
    }

    /// 检查工具权限
    /// 
    /// 检查顺序：
    /// 1. 检查 tool_overrides 中是否有该工具的特定配置
    /// 2. 检查该工具是否已被授予权限（granted_tools）
    /// 3. 使用 default_level
    pub fn check_tool_permission(&self, tool_name: &str) -> PermissionCheckResult {
        // 1. 检查工具特定覆盖
        if let Some(level) = self.config.tool_overrides.get(tool_name) {
            return self.evaluate_permission_level(level, tool_name);
        }

        // 2. 检查是否已被授予权限
        if self.granted_tools.contains(tool_name) {
            return PermissionCheckResult::Allowed;
        }

        // 3. 使用默认级别
        self.evaluate_permission_level(&self.config.default_level, tool_name)
    }

    /// 评估权限级别
    fn evaluate_permission_level(&self, level: &PermissionLevel, tool_name: &str) -> PermissionCheckResult {
        match level {
            PermissionLevel::AlwaysAllow => PermissionCheckResult::Allowed,
            PermissionLevel::AskFirst => {
                if self.granted_tools.contains(tool_name) {
                    PermissionCheckResult::Allowed
                } else {
                    PermissionCheckResult::NeedsConfirmation {
                        reason: format!("工具 '{}' 需要首次授权", tool_name),
                    }
                }
            }
            PermissionLevel::AskEveryTime => PermissionCheckResult::NeedsConfirmation {
                reason: format!("工具 '{}' 每次使用都需要授权", tool_name),
            },
            PermissionLevel::Deny => PermissionCheckResult::Denied {
                reason: format!("工具 '{}' 已被禁用", tool_name),
            },
        }
    }

    /// 检查 Bash 命令权限
    /// 
    /// 检查顺序：
    /// 1. 检查命令是否在黑名单中（部分匹配）- 最高优先级
    /// 2. 检查命令是否匹配危险模式
    /// 3. 如果有白名单，检查是否在白名单中
    pub fn check_bash_command(&self, command: &str) -> PermissionCheckResult {
        // 1. 检查黑名单（最高优先级）
        for blocked in &self.config.bash_blocked_commands {
            if command.contains(blocked) {
                return PermissionCheckResult::Denied {
                    reason: format!("命令 '{}' 包含被禁止的操作: '{}'", command, blocked),
                };
            }
        }

        // 2. 检查危险模式
        if self.is_dangerous_command(command) {
            return PermissionCheckResult::NeedsConfirmation {
                reason: format!("命令 '{}' 包含危险操作，需要确认", command),
            };
        }

        // 3. 检查白名单（如果设置了）
        if let Some(ref allowed) = self.config.bash_allowed_commands {
            let is_allowed = allowed.iter().any(|pattern| command.contains(pattern));
            if !is_allowed {
                return PermissionCheckResult::Denied {
                    reason: format!("命令 '{}' 不在允许的命令列表中", command),
                };
            }
        }

        PermissionCheckResult::Allowed
    }

    /// 授予工具权限
    pub fn grant_tool(&mut self, tool_name: &str) {
        self.granted_tools.insert(tool_name.to_string());
    }

    /// 撤销工具权限
    pub fn revoke_tool(&mut self, tool_name: &str) {
        self.granted_tools.remove(tool_name);
    }

    /// 检查是否已授予工具权限
    pub fn is_tool_granted(&self, tool_name: &str) -> bool {
        self.granted_tools.contains(tool_name)
    }

    /// 检测危险命令
    ///
    /// 检测以下危险模式：
    /// - `rm -rf /` 或类似命令
    /// - `sudo` 命令
    /// - `chmod 777`
    /// - `mkfs` 格式化命令
    /// - `dd` 磁盘操作
    /// - `> /dev/sda` 直接写入磁盘
    /// - fork bomb `:(){:|:&};:`
    pub fn is_dangerous_command(&self, command: &str) -> bool {
        let dangerous_patterns = [
            // rm -rf 危险用法
            "rm -rf /",
            "rm -rf ~",
            "rm -rf /*",
            "rm -rf ~/",
            // 权限提升
            "sudo ",
            "sudo\t",
            // 危险权限修改
            "chmod 777",
            "chmod -R 777",
            // 磁盘操作
            "mkfs.",
            "mkfs ",
            "dd if=",
            "dd of=/dev/",
            "dd of=/dev/sd",
            "dd of=/dev/hd",
            // 直接写入磁盘
            "> /dev/sda",
            "> /dev/hda",
            "> /dev/sd",
            "> /dev/hd",
            "> /dev/null",
            // fork bomb
            ":(){:|:&};:",
            // 其他危险命令
            ":(){ :|:& };:",
            "shutdown",
            "reboot",
            "halt",
            "poweroff",
            "init 0",
            "init 6",
            // 系统文件操作
            "> /etc/",
            "> /boot/",
            "> /sys/",
            "> /proc/",
        ];

        let cmd_lower = command.to_lowercase();
        for pattern in &dangerous_patterns {
            if cmd_lower.contains(pattern) {
                return true;
            }
        }

        false
    }

    /// 获取配置引用
    pub fn config(&self) -> &ToolPermissionConfig {
        &self.config
    }

    /// 获取可变配置引用
    pub fn config_mut(&mut self) -> &mut ToolPermissionConfig {
        &mut self.config
    }

    /// 获取最大执行时间
    pub fn max_execution_time(&self) -> u64 {
        self.config.max_execution_time_secs
    }
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::with_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_level_default() {
        assert_eq!(PermissionLevel::default(), PermissionLevel::AskFirst);
    }

    #[test]
    fn test_default_blocked_commands() {
        let config = ToolPermissionConfig::default();
        assert!(!config.bash_blocked_commands.is_empty());
        assert!(config.bash_blocked_commands.contains(&"sudo".to_string()));
        assert!(config.bash_blocked_commands.contains(&"rm -rf /".to_string()));
    }

    #[test]
    fn test_permission_manager_default() {
        let manager = PermissionManager::with_default();
        assert_eq!(manager.config().default_level, PermissionLevel::AskFirst);
    }

    #[test]
    fn test_check_tool_permission_always_allow() {
        let mut config = ToolPermissionConfig::default();
        config.tool_overrides.insert("test_tool".to_string(), PermissionLevel::AlwaysAllow);
        let manager = PermissionManager::new(config);
        
        match manager.check_tool_permission("test_tool") {
            PermissionCheckResult::Allowed => {}
            _ => panic!("Expected Allowed for AlwaysAllow"),
        }
    }

    #[test]
    fn test_check_tool_permission_deny() {
        let mut config = ToolPermissionConfig::default();
        config.tool_overrides.insert("test_tool".to_string(), PermissionLevel::Deny);
        let manager = PermissionManager::new(config);
        
        match manager.check_tool_permission("test_tool") {
            PermissionCheckResult::Denied { .. } => {}
            _ => panic!("Expected Denied for Deny level"),
        }
    }

    #[test]
    fn test_grant_and_revoke_tool() {
        let mut manager = PermissionManager::with_default();
        
        // 初始状态：需要确认
        match manager.check_tool_permission("test_tool") {
            PermissionCheckResult::NeedsConfirmation { .. } => {}
            _ => panic!("Expected NeedsConfirmation for ungranted tool"),
        }
        
        // 授予权限
        manager.grant_tool("test_tool");
        assert!(manager.is_tool_granted("test_tool"));
        
        match manager.check_tool_permission("test_tool") {
            PermissionCheckResult::Allowed => {}
            _ => panic!("Expected Allowed after granting"),
        }
        
        // 撤销权限
        manager.revoke_tool("test_tool");
        assert!(!manager.is_tool_granted("test_tool"));
    }

    #[test]
    fn test_is_dangerous_command() {
        let manager = PermissionManager::with_default();
        
        assert!(manager.is_dangerous_command("rm -rf /"));
        assert!(manager.is_dangerous_command("sudo ls"));
        assert!(manager.is_dangerous_command("chmod 777 file"));
        assert!(manager.is_dangerous_command("mkfs.ext4 /dev/sda1"));
        assert!(manager.is_dangerous_command("dd if=/dev/zero of=/dev/sda"));
        assert!(manager.is_dangerous_command(":(){:|:&};:"));
        
        assert!(!manager.is_dangerous_command("ls -la"));
        assert!(!manager.is_dangerous_command("cat file.txt"));
    }

    #[test]
    fn test_check_bash_command_blocked() {
        let config = ToolPermissionConfig::default();
        let manager = PermissionManager::new(config);
        
        match manager.check_bash_command("rm -rf /") {
            PermissionCheckResult::Denied { .. } => {}
            _ => panic!("Expected Denied for blocked command"),
        }
    }

    #[test]
    fn test_check_bash_command_allowed_list() {
        let config = ToolPermissionConfig {
            bash_allowed_commands: Some(vec!["ls".to_string(), "cat".to_string()]),
            ..Default::default()
        };
        let manager = PermissionManager::new(config);
        
        // 允许的命令
        match manager.check_bash_command("ls -la") {
            PermissionCheckResult::Allowed => {}
            _ => panic!("Expected Allowed for whitelisted command"),
        }
        
        // 不允许的命令
        match manager.check_bash_command("rm file") {
            PermissionCheckResult::Denied { .. } => {}
            _ => panic!("Expected Denied for non-whitelisted command"),
        }
    }
}
