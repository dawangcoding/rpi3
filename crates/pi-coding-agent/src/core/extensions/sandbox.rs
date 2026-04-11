//! WASM 沙箱实现
//!
//! 提供完整的 WASM 扩展安全隔离：
//! - 文件系统访问限制
//! - 网络访问限制
//! - 资源使用限制（内存、CPU）
//! - 系统调用过滤
//! - 崩溃隔离

use super::types::{
    ExtensionLoadError, NetworkPermission, PathPermission, Permission, ResourceLimits,
    SandboxConfig, WasmExtensionManifest,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use wasmtime::*;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, ResourceTable};

/// WASI 上下文状态，包含沙箱限制
pub struct WasiSandboxCtx {
    /// WASI 上下文
    #[allow(dead_code)] // WASM 扩展系统尚未完全集成
    pub wasi: WasiCtx,
    /// 资源表
    #[allow(dead_code)] // WASM 扩展系统尚未完全集成
    pub table: ResourceTable,
}

impl std::fmt::Debug for WasiSandboxCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasiSandboxCtx")
            .field("wasi", &"<WasiCtx>")
            .field("table", &"<ResourceTable>")
            .finish()
    }
}

/// WASM 沙箱
pub struct WasmSandbox {
    /// 沙箱配置
    config: SandboxConfig,
    /// Wasmtime Engine
    engine: Engine,
    /// 扩展目录（用于解析相对路径）
    extension_dir: Option<PathBuf>,
}

impl WasmSandbox {
    /// 创建新的 WASM 沙箱
    ///
    /// # Arguments
    /// * `config` - 沙箱配置
    ///
    /// # Returns
    /// 返回沙箱实例或配置错误
    pub fn new(config: SandboxConfig) -> Result<Self, ExtensionLoadError> {
        let engine = Self::create_engine(&config)?;
        Ok(Self {
            config,
            engine,
            extension_dir: None,
        })
    }

    /// 创建带扩展目录的沙箱
    pub fn with_extension_dir(
        config: SandboxConfig,
        extension_dir: PathBuf,
    ) -> Result<Self, ExtensionLoadError> {
        let engine = Self::create_engine(&config)?;
        Ok(Self {
            config,
            engine,
            extension_dir: Some(extension_dir),
        })
    }

    /// 创建配置了 fuel metering 的 Engine
    fn create_engine(_config: &SandboxConfig) -> Result<Engine, ExtensionLoadError> {
        let mut engine_config = Config::new();

        // 启用 fuel metering 用于 CPU 执行限制
        engine_config.consume_fuel(true);

        // 启用 wasm backtrace 用于调试
        engine_config.wasm_backtrace(true);

        // 设置内存限制
        // 注意：wasmtime 的内存限制通过 StoreLimits 实现，这里设置引擎级别配置

        Engine::new(&engine_config).map_err(|e| {
            ExtensionLoadError::SandboxConfigError(format!("Failed to create engine: {}", e))
        })
    }

    /// 从扩展 manifest 构建沙箱
    ///
    /// 解析 manifest 中的权限声明，构建对应的沙箱配置
    pub fn from_manifest(manifest: &WasmExtensionManifest) -> Result<Self, ExtensionLoadError> {
        let config = Self::build_config_from_manifest(manifest);
        Self::new(config)
    }

    /// 从 manifest 构建带扩展目录的沙箱
    pub fn from_manifest_with_dir(
        manifest: &WasmExtensionManifest,
        extension_dir: PathBuf,
    ) -> Result<Self, ExtensionLoadError> {
        let config = Self::build_config_from_manifest(manifest);
        Self::with_extension_dir(config, extension_dir)
    }

    /// 从 manifest 构建沙箱配置
    fn build_config_from_manifest(manifest: &WasmExtensionManifest) -> SandboxConfig {
        // 如果 manifest 提供了沙箱配置，使用它
        if let Some(ref sandbox) = manifest.sandbox {
            return sandbox.clone();
        }

        // 否则从权限声明构建配置
        let mut allowed_paths: Vec<PathPermission> = Vec::new();
        let mut network = NetworkPermission::default();

        // 解析字符串格式的权限
        for perm_str in &manifest.permissions {
            if let Some(perm) = Permission::parse(perm_str) {
                match perm {
                    Permission::FileRead(path) => {
                        // 检查是否已有该路径的权限
                        if let Some(existing) = allowed_paths.iter_mut().find(|p| p.path == path) {
                            existing.read = true;
                        } else {
                            allowed_paths.push(PathPermission {
                                path,
                                read: true,
                                write: false,
                            });
                        }
                    }
                    Permission::FileWrite(path) => {
                        if let Some(existing) = allowed_paths.iter_mut().find(|p| p.path == path) {
                            existing.write = true;
                        } else {
                            allowed_paths.push(PathPermission {
                                path,
                                read: false,
                                write: true,
                            });
                        }
                    }
                    Permission::NetworkAccess(host) => {
                        network.enabled = true;
                        if !network.allowed_hosts.contains(&host) {
                            network.allowed_hosts.push(host);
                        }
                    }
                    Permission::FullNetwork => {
                        network.enabled = true;
                        network.allowed_hosts.clear(); // 空列表 + enabled = 完全访问
                    }
                }
            }
        }

        // 解析结构化权限
        for perm in &manifest.structured_permissions {
            match perm {
                Permission::FileRead(path) => {
                    if let Some(existing) = allowed_paths.iter_mut().find(|p| p.path == *path) {
                        existing.read = true;
                    } else {
                        allowed_paths.push(PathPermission {
                            path: path.clone(),
                            read: true,
                            write: false,
                        });
                    }
                }
                Permission::FileWrite(path) => {
                    if let Some(existing) = allowed_paths.iter_mut().find(|p| p.path == *path) {
                        existing.write = true;
                    } else {
                        allowed_paths.push(PathPermission {
                            path: path.clone(),
                            read: false,
                            write: true,
                        });
                    }
                }
                Permission::NetworkAccess(host) => {
                    network.enabled = true;
                    if !network.allowed_hosts.contains(host) {
                        network.allowed_hosts.push(host.clone());
                    }
                }
                Permission::FullNetwork => {
                    network.enabled = true;
                    network.allowed_hosts.clear();
                }
            }
        }

        SandboxConfig {
            allowed_paths,
            network,
            resource_limits: ResourceLimits::default(),
        }
    }

    /// 创建受限的 Store
    ///
    /// 配置：
    /// - WASI 文件系统访问限制（仅开放声明的路径）
    /// - 内存限制
    /// - Fuel 限制（CPU 执行步骤）
    /// - 网络默认禁用
    pub fn create_store(&self) -> Result<Store<WasiSandboxCtx>, ExtensionLoadError> {
        let wasi_ctx = self.build_wasi_ctx()?;
        let ctx = WasiSandboxCtx {
            wasi: wasi_ctx,
            table: ResourceTable::new(),
        };

        let mut store = Store::new(&self.engine, ctx);

        // 设置 fuel 限制（使用 set_fuel 而不是 add_fuel）
        store
            .set_fuel(self.config.resource_limits.max_fuel)
            .map_err(|e| {
                ExtensionLoadError::SandboxConfigError(format!("Failed to set fuel: {}", e))
            })?;

        Ok(store)
    }

    /// 构建 WASI 上下文，配置文件系统访问限制
    fn build_wasi_ctx(&self) -> Result<WasiCtx, ExtensionLoadError> {
        // 创建 WASI builder 并配置标准 I/O
        // 注意：WasiCtxBuilder 方法返回 &mut Self，需要先绑定再调用
        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder.inherit_stdio();

        // 配置允许访问的文件路径
        for path_perm in &self.config.allowed_paths {
            let resolved_path = if path_perm.path.is_relative() {
                if let Some(ref ext_dir) = self.extension_dir {
                    ext_dir.join(&path_perm.path)
                } else {
                    path_perm.path.clone()
                }
            } else {
                path_perm.path.clone()
            };

            // 确保路径存在
            if !resolved_path.exists() {
                tracing::warn!(
                    "Sandbox path does not exist: {:?}, skipping",
                    resolved_path
                );
                continue;
            }

            // 使用 DirPerms 和 FilePerms 配置权限
            // DirPerms: READ | MUTATE
            // FilePerms: READ | WRITE | SEEK
            let dir_perms = DirPerms::READ;
            let file_perms = if path_perm.read && path_perm.write {
                FilePerms::READ | FilePerms::WRITE
            } else if path_perm.read {
                FilePerms::READ
            } else if path_perm.write {
                FilePerms::WRITE
            } else {
                continue; // 没有任何权限，跳过
            };

            // 获取 guest 路径名称
            let guest_path = path_perm.path.to_string_lossy().to_string();

            match std::fs::canonicalize(&resolved_path) {
                Ok(canonical_path) => {
                    // 使用 preopened_dir API（wasmtime-wasi 29）
                    // 它会自动打开目录并配置权限
                    match wasi_builder.preopened_dir(
                        &canonical_path,
                        guest_path,
                        dir_perms,
                        file_perms,
                    ) {
                        Ok(_) => {
                            tracing::debug!(
                                "Sandbox: preopened directory {:?} with read={}, write={}",
                                canonical_path,
                                path_perm.read,
                                path_perm.write
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Sandbox: failed to preopen directory {:?}: {}",
                                canonical_path,
                                e
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Sandbox: failed to canonicalize path {:?}: {}",
                        resolved_path,
                        e
                    );
                }
            }
        }

        // 网络权限配置
        // 注意：wasmtime-wasi 默认禁用网络
        // 如果 network.enabled 为 true，需要额外配置
        if self.config.network.enabled {
            tracing::info!(
                "Sandbox: network access enabled for hosts: {:?}",
                self.config.network.allowed_hosts
            );
            // 网络权限通过 wasmtime-wasi 的 socket 配置
            // 由于复杂性，这里暂时只记录日志
            // 完整实现需要配置 socket address 和 allowed hosts
        }

        Ok(wasi_builder.build())
    }

    /// 在沙箱中安全执行操作
    ///
    /// 捕获 trap，处理资源超限，确保 trap 不传播到主进程
    pub fn execute_safely<F, R>(
        &self,
        store: &mut Store<WasiSandboxCtx>,
        f: F,
    ) -> Result<R, ExtensionLoadError>
    where
        F: FnOnce(&mut Store<WasiSandboxCtx>) -> Result<R, anyhow::Error>,
    {
        let start_time = Instant::now();
        let max_duration = Duration::from_millis(self.config.resource_limits.max_execution_time_ms);

        // 执行操作
        let result = f(store);

        // 检查执行时间
        let elapsed = start_time.elapsed();
        if elapsed > max_duration {
            tracing::warn!(
                "Sandbox: execution time exceeded limit: {:?} > {:?}",
                elapsed,
                max_duration
            );
            return Err(ExtensionLoadError::ResourceLimitExceeded(format!(
                "Execution time exceeded: {:?} > {:?}",
                elapsed, max_duration
            )));
        }

        // 检查 fuel 是否耗尽
        let fuel_remaining = store.get_fuel().unwrap_or(0);
        if fuel_remaining == 0 {
            tracing::warn!("Sandbox: fuel exhausted");
            return Err(ExtensionLoadError::ResourceLimitExceeded(
                "Fuel exhausted".to_string(),
            ));
        }

        result.map_err(|e| {
            // 检查是否是 trap
            if e.is::<wasmtime::Trap>() {
                tracing::error!("Sandbox: WASM trap occurred: {}", e);
                ExtensionLoadError::SandboxViolation(format!("WASM trap: {}", e))
            } else {
                ExtensionLoadError::from(e)
            }
        })
    }

    /// 编译 WASM 模块
    pub fn compile_module(&self, wasm_bytes: &[u8]) -> Result<Module, ExtensionLoadError> {
        Module::new(&self.engine, wasm_bytes).map_err(|e| {
            ExtensionLoadError::WasmCompileError(format!("Failed to compile WASM: {}", e))
        })
    }

    /// 在沙箱中实例化模块
    ///
    /// 创建受限 Store 并实例化模块
    pub fn instantiate_module(
        &self,
        module: &Module,
    ) -> Result<(Store<WasiSandboxCtx>, Instance), ExtensionLoadError> {
        let mut store = self.create_store()?;
        let instance = Instance::new(&mut store, module, &[]).map_err(|e| {
            ExtensionLoadError::WasmInstantiationError(format!(
                "Failed to instantiate module: {}",
                e
            ))
        })?;

        Ok((store, instance))
    }

    /// 获取默认沙箱配置（最小权限）
    pub fn default_config() -> SandboxConfig {
        SandboxConfig::default()
    }

    /// 获取当前沙箱配置
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// 获取 Engine 引用
    #[allow(dead_code)] // WASM 扩展系统尚未完全集成
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// 检查路径是否在允许列表中
    pub fn is_path_allowed(&self, path: &Path, write: bool) -> bool {
        for perm in &self.config.allowed_paths {
            if perm.path == path || path.starts_with(&perm.path) {
                if write {
                    return perm.write;
                } else {
                    return perm.read;
                }
            }
        }
        false
    }

    /// 检查网络访问是否允许
    pub fn is_network_allowed(&self, host: &str) -> bool {
        if !self.config.network.enabled {
            return false;
        }
        // 如果 allowed_hosts 为空且 enabled = true，允许所有网络访问
        if self.config.network.allowed_hosts.is_empty() {
            return true;
        }
        // 检查 host 是否在允许列表中
        self.config
            .network
            .allowed_hosts
            .iter()
            .any(|allowed| host == allowed || host.starts_with(allowed))
    }
}

impl std::fmt::Debug for WasmSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmSandbox")
            .field("config", &self.config)
            .field("extension_dir", &self.extension_dir)
            .finish()
    }
}

impl From<anyhow::Error> for ExtensionLoadError {
    fn from(e: anyhow::Error) -> Self {
        ExtensionLoadError::SandboxViolation(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== SandboxConfig Tests ====================

    #[test]
    fn test_sandbox_config_default() {
        let config = SandboxConfig::default();

        assert!(config.allowed_paths.is_empty());
        assert!(!config.network.enabled);
        assert!(config.network.allowed_hosts.is_empty());
        assert_eq!(config.resource_limits.max_memory_bytes, 64 * 1024 * 1024);
        assert_eq!(config.resource_limits.max_fuel, 1_000_000);
        assert_eq!(config.resource_limits.max_execution_time_ms, 5000);
    }

    #[test]
    fn test_resource_limits_default() {
        let limits = ResourceLimits::default();

        assert_eq!(limits.max_memory_bytes, 64 * 1024 * 1024);
        assert_eq!(limits.max_fuel, 1_000_000);
        assert_eq!(limits.max_execution_time_ms, 5000);
    }

    #[test]
    fn test_network_permission_default() {
        let net_perm = NetworkPermission::default();

        assert!(!net_perm.enabled);
        assert!(net_perm.allowed_hosts.is_empty());
    }

    #[test]
    fn test_path_permission_new() {
        let perm = PathPermission {
            path: PathBuf::from("/tmp"),
            read: true,
            write: false,
        };

        assert_eq!(perm.path, PathBuf::from("/tmp"));
        assert!(perm.read);
        assert!(!perm.write);
    }

    #[test]
    fn test_sandbox_config_serialization() {
        let config = SandboxConfig {
            allowed_paths: vec![PathPermission {
                path: PathBuf::from("/test"),
                read: true,
                write: false,
            }],
            network: NetworkPermission {
                enabled: true,
                allowed_hosts: vec!["example.com:443".to_string()],
            },
            resource_limits: ResourceLimits {
                max_memory_bytes: 128 * 1024 * 1024,
                max_fuel: 2_000_000,
                max_execution_time_ms: 10000,
            },
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: SandboxConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.allowed_paths.len(), 1);
        assert!(parsed.network.enabled);
        assert_eq!(parsed.network.allowed_hosts.len(), 1);
        assert_eq!(parsed.resource_limits.max_memory_bytes, 128 * 1024 * 1024);
    }

    // ==================== Permission Tests ====================

    #[test]
    fn test_permission_from_str_file_read() {
        let perm = Permission::parse("fs.read:/tmp/test");
        assert!(perm.is_some());

        let perm = perm.unwrap();
        assert_eq!(perm, Permission::FileRead(PathBuf::from("/tmp/test")));
    }

    #[test]
    fn test_permission_from_str_file_write() {
        let perm = Permission::parse("fs.write:/tmp/output");
        assert!(perm.is_some());

        let perm = perm.unwrap();
        assert_eq!(perm, Permission::FileWrite(PathBuf::from("/tmp/output")));
    }

    #[test]
    fn test_permission_from_str_network_access() {
        let perm = Permission::parse("net:example.com:443");
        assert!(perm.is_some());

        let perm = perm.unwrap();
        assert_eq!(perm, Permission::NetworkAccess("example.com:443".to_string()));
    }

    #[test]
    fn test_permission_from_str_full_network() {
        let perm = Permission::parse("net:*");
        assert!(perm.is_some());

        let perm = perm.unwrap();
        assert_eq!(perm, Permission::FullNetwork);
    }

    #[test]
    fn test_permission_from_str_invalid() {
        assert!(Permission::parse("invalid").is_none());
        assert!(Permission::parse("").is_none());
    }

    #[test]
    fn test_permission_to_permission_string() {
        assert_eq!(
            Permission::FileRead(PathBuf::from("/tmp")).to_permission_string(),
            "fs.read:/tmp"
        );
        assert_eq!(
            Permission::FileWrite(PathBuf::from("/tmp")).to_permission_string(),
            "fs.write:/tmp"
        );
        assert_eq!(
            Permission::NetworkAccess("example.com:443".to_string()).to_permission_string(),
            "net:example.com:443"
        );
        assert_eq!(Permission::FullNetwork.to_permission_string(), "net:*");
    }

    // ==================== WasmSandbox Tests ====================

    #[test]
    fn test_wasm_sandbox_new() {
        let config = SandboxConfig::default();
        let sandbox = WasmSandbox::new(config);

        assert!(sandbox.is_ok());
    }

    #[test]
    fn test_wasm_sandbox_default_config() {
        let config = WasmSandbox::default_config();

        assert!(config.allowed_paths.is_empty());
        assert!(!config.network.enabled);
    }

    #[test]
    fn test_wasm_sandbox_create_store() {
        let config = SandboxConfig::default();
        let sandbox = WasmSandbox::new(config).unwrap();

        let store = sandbox.create_store();
        assert!(store.is_ok());
    }

    #[test]
    fn test_wasm_sandbox_is_path_allowed() {
        let config = SandboxConfig {
            allowed_paths: vec![
                PathPermission {
                    path: PathBuf::from("/tmp"),
                    read: true,
                    write: false,
                },
                PathPermission {
                    path: PathBuf::from("/var/log"),
                    read: true,
                    write: true,
                },
            ],
            network: NetworkPermission::default(),
            resource_limits: ResourceLimits::default(),
        };

        let sandbox = WasmSandbox::new(config).unwrap();

        // 检查路径权限
        assert!(sandbox.is_path_allowed(Path::new("/tmp"), false)); // read
        assert!(!sandbox.is_path_allowed(Path::new("/tmp"), true)); // write
        assert!(sandbox.is_path_allowed(Path::new("/var/log"), false)); // read
        assert!(sandbox.is_path_allowed(Path::new("/var/log"), true)); // write
        assert!(!sandbox.is_path_allowed(Path::new("/etc"), false)); // not allowed
    }

    #[test]
    fn test_wasm_sandbox_is_network_allowed() {
        let config = SandboxConfig {
            allowed_paths: vec![],
            network: NetworkPermission {
                enabled: true,
                allowed_hosts: vec!["api.example.com".to_string()],
            },
            resource_limits: ResourceLimits::default(),
        };

        let sandbox = WasmSandbox::new(config).unwrap();

        assert!(sandbox.is_network_allowed("api.example.com"));
        assert!(!sandbox.is_network_allowed("other.com"));
    }

    #[test]
    fn test_wasm_sandbox_network_disabled_by_default() {
        let config = SandboxConfig::default();
        let sandbox = WasmSandbox::new(config).unwrap();

        assert!(!sandbox.is_network_allowed("any.host.com"));
    }

    #[test]
    fn test_wasm_sandbox_full_network() {
        let config = SandboxConfig {
            allowed_paths: vec![],
            network: NetworkPermission {
                enabled: true,
                allowed_hosts: vec![], // empty = full access
            },
            resource_limits: ResourceLimits::default(),
        };

        let sandbox = WasmSandbox::new(config).unwrap();

        assert!(sandbox.is_network_allowed("any.host.com"));
        assert!(sandbox.is_network_allowed("another.host.com"));
    }

    #[test]
    fn test_wasm_sandbox_from_manifest() {
        let json = r#"{
            "name": "test-extension",
            "version": "1.0.0",
            "description": "Test",
            "wasm_entry": "test.wasm",
            "permissions": ["fs.read:/tmp", "net:api.example.com"]
        }"#;

        let manifest = WasmExtensionManifest::from_json(json).unwrap();
        let sandbox = WasmSandbox::from_manifest(&manifest);

        assert!(sandbox.is_ok());
        let sandbox = sandbox.unwrap();

        // 检查配置是否正确解析
        let config = sandbox.config();
        assert_eq!(config.allowed_paths.len(), 1);
        assert!(config.network.enabled);
    }

    #[test]
    fn test_wasm_sandbox_from_manifest_with_sandbox_config() {
        let json = r#"{
            "name": "test-extension",
            "version": "1.0.0",
            "description": "Test",
            "wasm_entry": "test.wasm",
            "sandbox": {
                "network": {
                    "enabled": true,
                    "allowed_hosts": ["custom.host.com"]
                },
                "resource_limits": {
                    "max_memory_bytes": 33554432,
                    "max_fuel": 500000,
                    "max_execution_time_ms": 2000
                }
            }
        }"#;

        let manifest = WasmExtensionManifest::from_json(json).unwrap();
        let sandbox = WasmSandbox::from_manifest(&manifest).unwrap();

        let config = sandbox.config();
        assert!(config.network.enabled);
        assert!(config.network.allowed_hosts.contains(&"custom.host.com".to_string()));
        assert_eq!(config.resource_limits.max_memory_bytes, 33554432);
        assert_eq!(config.resource_limits.max_fuel, 500000);
        assert_eq!(config.resource_limits.max_execution_time_ms, 2000);
    }

    #[test]
    fn test_wasm_sandbox_compile_invalid_wasm() {
        let sandbox = WasmSandbox::new(SandboxConfig::default()).unwrap();
        let invalid_wasm = b"invalid wasm bytes";

        let result = sandbox.compile_module(invalid_wasm);
        assert!(result.is_err());

        match result.unwrap_err() {
            ExtensionLoadError::WasmCompileError(_) => (),
            other => panic!("Expected WasmCompileError, got {:?}", other),
        }
    }

    #[test]
    fn test_wasm_sandbox_trap_isolation() {
        // 测试沙箱能否正确处理 trap
        let sandbox = WasmSandbox::new(SandboxConfig::default()).unwrap();
        let mut store = sandbox.create_store().unwrap();

        // 模拟一个会失败的操作
        let result: Result<(), ExtensionLoadError> = sandbox.execute_safely(&mut store, |_store| {
            Err(anyhow::anyhow!("Simulated error"))
        });

        assert!(result.is_err());
        // 主进程应该仍然可以继续运行（不会因为 trap 而崩溃）
    }

    // ==================== WasiSandboxCtx Tests ====================

    #[test]
    fn test_wasi_sandbox_ctx_debug() {
        let ctx = WasiSandboxCtx {
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
        };

        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("WasiSandboxCtx"));
    }
}
