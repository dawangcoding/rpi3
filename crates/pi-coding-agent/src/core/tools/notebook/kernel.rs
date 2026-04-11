//! Kernel 管理模块
//!
//! 实现 Python/Node.js Kernel 的发现、启动、停止和健康监控。
//! KernelManager 主要用于发现和验证可用的运行时，实际的代码执行采用
//! 每次执行创建独立子进程的方式。

#![allow(dead_code)] // Notebook 功能尚未完全集成

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// Kernel 类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KernelType {
    /// Python
    Python,
    /// Node.js
    NodeJs,
}

impl KernelType {
    /// 获取语言名称（用于代码块标识）
    pub fn language_name(&self) -> &str {
        match self {
            KernelType::Python => "python",
            KernelType::NodeJs => "javascript",
        }
    }

    /// 获取显示名称
    pub fn display_name(&self) -> &str {
        match self {
            KernelType::Python => "Python 3",
            KernelType::NodeJs => "Node.js",
        }
    }

    /// 获取文件扩展名
    pub fn file_extension(&self) -> &str {
        match self {
            KernelType::Python => ".py",
            KernelType::NodeJs => ".js",
        }
    }
}

impl std::fmt::Display for KernelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl std::str::FromStr for KernelType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "python" | "python3" => Ok(KernelType::Python),
            "javascript" | "nodejs" | "node" | "js" => Ok(KernelType::NodeJs),
            _ => Err(format!("Unknown kernel type: {}", s)),
        }
    }
}

/// Kernel 状态枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelStatus {
    /// 启动中
    Starting,
    /// 运行中
    Running,
    /// 已停止
    Stopped,
    /// 已崩溃
    Crashed,
}

/// Kernel 规格信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelSpec {
    /// Kernel 类型
    pub kernel_type: KernelType,
    /// 可执行文件路径
    pub executable: PathBuf,
    /// 版本
    pub version: String,
    /// 显示名称
    pub display_name: String,
}

/// Kernel 管理器
///
/// 负责发现和管理系统中可用的 Python 和 Node.js 运行时。
/// 注意：实际的代码执行采用每次执行创建独立子进程的方式，
/// 因此 KernelManager 主要用于发现和验证可用的运行时。
pub struct KernelManager {
    specs: Vec<KernelSpec>,
    cwd: PathBuf,
}

impl KernelManager {
    /// 创建新的 KernelManager
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            specs: Vec::new(),
            cwd,
        }
    }

    /// 发现系统中可用的 Python 和 Node.js 运行时
    ///
    /// 使用 `which python3`、`which python`、`which node` 检测
    /// 然后用 `python3 --version`、`node --version` 获取版本
    pub async fn discover_kernels(&mut self) -> Vec<KernelSpec> {
        let mut discovered = Vec::new();

        // 发现 Python
        if let Some(spec) = discover_single(&["python3", "python"], KernelType::Python).await {
            discovered.push(spec);
        }

        // 发现 Node.js
        if let Some(spec) = discover_single(&["node"], KernelType::NodeJs).await {
            discovered.push(spec);
        }

        self.specs = discovered.clone();
        discovered
    }

    /// 获取指定类型的 Kernel 规格
    pub fn get_kernel_spec(&self, kernel_type: KernelType) -> Option<&KernelSpec> {
        self.specs.iter().find(|s| s.kernel_type == kernel_type)
    }

    /// 检查指定类型的 Kernel 是否可用
    pub fn is_available(&self, kernel_type: KernelType) -> bool {
        self.get_kernel_spec(kernel_type).is_some()
    }

    /// 获取所有已发现的 Kernel 规格
    pub fn available_kernels(&self) -> &[KernelSpec] {
        &self.specs
    }

    /// 获取可执行文件路径（如果可用）
    pub fn get_executable(&self, kernel_type: KernelType) -> Option<&PathBuf> {
        self.get_kernel_spec(kernel_type).map(|s| &s.executable)
    }

    /// 获取工作目录
    pub fn cwd(&self) -> &PathBuf {
        &self.cwd
    }
}

/// 发现单个 Kernel 类型
///
/// 尝试多个可执行文件名，返回第一个找到的 Kernel 规格
async fn discover_single(executable_names: &[&str], kernel_type: KernelType) -> Option<KernelSpec> {
    for name in executable_names {
        // 1. 用 which <name> 查找路径
        let which_output = Command::new("which")
            .arg(name)
            .output()
            .await;

        if let Ok(output) = which_output {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let executable = PathBuf::from(&path);

                // 2. 获取版本
                let version_flag = match kernel_type {
                    KernelType::Python => "--version",
                    KernelType::NodeJs => "--version",
                };

                let version_output = Command::new(&executable)
                    .arg(version_flag)
                    .output()
                    .await;

                let version = if let Ok(v) = version_output {
                    // Python 输出 "Python 3.11.5"，Node 输出 "v20.10.0"
                    let stdout = String::from_utf8_lossy(&v.stdout);
                    let stderr = String::from_utf8_lossy(&v.stderr);
                    let raw = if !stdout.trim().is_empty() { stdout } else { stderr };
                    raw.trim().to_string()
                } else {
                    "unknown".to_string()
                };

                return Some(KernelSpec {
                    kernel_type,
                    executable,
                    version: version.clone(),
                    display_name: format!("{} ({})", kernel_type.display_name(), version),
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_kernel_type_display() {
        assert_eq!(format!("{}", KernelType::Python), "Python 3");
        assert_eq!(format!("{}", KernelType::NodeJs), "Node.js");
    }

    #[test]
    fn test_kernel_type_from_str() {
        // Python variants
        assert_eq!("python".parse::<KernelType>().unwrap(), KernelType::Python);
        assert_eq!("python3".parse::<KernelType>().unwrap(), KernelType::Python);
        assert_eq!("PYTHON".parse::<KernelType>().unwrap(), KernelType::Python);
        assert_eq!("Python".parse::<KernelType>().unwrap(), KernelType::Python);

        // Node.js variants
        assert_eq!("javascript".parse::<KernelType>().unwrap(), KernelType::NodeJs);
        assert_eq!("nodejs".parse::<KernelType>().unwrap(), KernelType::NodeJs);
        assert_eq!("node".parse::<KernelType>().unwrap(), KernelType::NodeJs);
        assert_eq!("js".parse::<KernelType>().unwrap(), KernelType::NodeJs);
        assert_eq!("NODE".parse::<KernelType>().unwrap(), KernelType::NodeJs);

        // Invalid
        assert!("invalid".parse::<KernelType>().is_err());
        assert!("ruby".parse::<KernelType>().is_err());
    }

    #[test]
    fn test_kernel_type_language_name() {
        assert_eq!(KernelType::Python.language_name(), "python");
        assert_eq!(KernelType::NodeJs.language_name(), "javascript");
    }

    #[test]
    fn test_kernel_type_file_extension() {
        assert_eq!(KernelType::Python.file_extension(), ".py");
        assert_eq!(KernelType::NodeJs.file_extension(), ".js");
    }

    #[test]
    fn test_kernel_manager_new() {
        let dir = TempDir::new().unwrap();
        let manager = KernelManager::new(dir.path().to_path_buf());

        assert_eq!(manager.cwd(), dir.path());
        assert!(manager.available_kernels().is_empty());
        assert!(!manager.is_available(KernelType::Python));
        assert!(!manager.is_available(KernelType::NodeJs));
    }

    #[test]
    fn test_kernel_manager_get_executable_without_discovery() {
        let dir = TempDir::new().unwrap();
        let manager = KernelManager::new(dir.path().to_path_buf());

        assert!(manager.get_executable(KernelType::Python).is_none());
        assert!(manager.get_executable(KernelType::NodeJs).is_none());
    }

    #[tokio::test]
    #[ignore = "Depends on system environment - requires python3 or python to be installed"]
    async fn test_discover_kernels_python() {
        let dir = TempDir::new().unwrap();
        let mut manager = KernelManager::new(dir.path().to_path_buf());

        let _kernels = manager.discover_kernels().await;

        // 检查是否发现了 Python（如果系统安装了 Python）
        if manager.is_available(KernelType::Python) {
            let spec = manager.get_kernel_spec(KernelType::Python).unwrap();
            assert_eq!(spec.kernel_type, KernelType::Python);
            assert!(!spec.executable.as_os_str().is_empty());
            assert!(!spec.version.is_empty());
            assert!(spec.display_name.contains("Python"));
        }
    }

    #[tokio::test]
    #[ignore = "Depends on system environment - requires node to be installed"]
    async fn test_discover_kernels_nodejs() {
        let dir = TempDir::new().unwrap();
        let mut manager = KernelManager::new(dir.path().to_path_buf());

        let _kernels = manager.discover_kernels().await;

        // 检查是否发现了 Node.js（如果系统安装了 Node.js）
        if manager.is_available(KernelType::NodeJs) {
            let spec = manager.get_kernel_spec(KernelType::NodeJs).unwrap();
            assert_eq!(spec.kernel_type, KernelType::NodeJs);
            assert!(!spec.executable.as_os_str().is_empty());
            assert!(!spec.version.is_empty());
            assert!(spec.display_name.contains("Node.js"));
        }
    }

    #[tokio::test]
    #[ignore = "Depends on system environment"]
    async fn test_discover_kernels_returns_specs() {
        let dir = TempDir::new().unwrap();
        let mut manager = KernelManager::new(dir.path().to_path_buf());

        let kernels = manager.discover_kernels().await;

        // 返回的 specs 应该和 manager 内部存储的一致
        assert_eq!(kernels.len(), manager.available_kernels().len());

        for spec in &kernels {
            assert!(manager.is_available(spec.kernel_type));
            assert!(manager.get_executable(spec.kernel_type).is_some());
        }
    }

    #[test]
    fn test_kernel_spec_serialization() {
        let spec = KernelSpec {
            kernel_type: KernelType::Python,
            executable: PathBuf::from("/usr/bin/python3"),
            version: "Python 3.11.5".to_string(),
            display_name: "Python 3 (Python 3.11.5)".to_string(),
        };

        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("Python"));
        assert!(json.contains("/usr/bin/python3"));

        let deserialized: KernelSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.kernel_type, KernelType::Python);
        assert_eq!(deserialized.executable, PathBuf::from("/usr/bin/python3"));
        assert_eq!(deserialized.version, "Python 3.11.5");
    }

    #[test]
    fn test_kernel_type_serialization() {
        // Python
        let json = serde_json::to_string(&KernelType::Python).unwrap();
        assert_eq!(json, "\"Python\"");

        let deserialized: KernelType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, KernelType::Python);

        // NodeJs
        let json = serde_json::to_string(&KernelType::NodeJs).unwrap();
        assert_eq!(json, "\"NodeJs\"");

        let deserialized: KernelType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, KernelType::NodeJs);
    }
}
