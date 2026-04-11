use super::types::{ExtensionManifest, Extension, WasmExtension, WasmInstance, WasmExtensionManifest, ExtensionLoadError, SandboxConfig};
use super::sandbox::WasmSandbox;
use crate::config::AppConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::Result;

/// 扩展加载器 - 从文件系统扫描和加载扩展
pub struct ExtensionLoader {
    /// 扩展目录路径
    extensions_dir: PathBuf,
}

impl ExtensionLoader {
    /// 创建新的扩展加载器
    pub fn new() -> Self {
        let extensions_dir = directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().join(".pi").join("extensions"))
            .unwrap_or_else(|| PathBuf::from(".pi/extensions"));
        
        Self { extensions_dir }
    }
    
    /// 使用指定目录创建扩展加载器
    #[allow(dead_code)]
    pub fn with_dir(extensions_dir: PathBuf) -> Self {
        Self { extensions_dir }
    }
    
    /// 扫描扩展目录，返回所有找到的扩展 manifest
    pub fn scan_extensions(&self) -> Vec<ExtensionManifest> {
        let mut manifests = Vec::new();
        
        if !self.extensions_dir.exists() {
            tracing::debug!("Extensions directory does not exist: {:?}", self.extensions_dir);
            return manifests;
        }
        
        let entries = match std::fs::read_dir(&self.extensions_dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Failed to read extensions directory: {}", e);
                return manifests;
            }
        };
        
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let manifest_path = path.join("manifest.json");
                if manifest_path.exists() {
                    match self.load_manifest(&manifest_path) {
                        Ok(manifest) => {
                            tracing::info!("Found extension: {} v{}", manifest.name, manifest.version);
                            manifests.push(manifest);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to load manifest {:?}: {}", manifest_path, e);
                        }
                    }
                }
            }
        }
        
        manifests
    }
    
    fn load_manifest(&self, path: &std::path::Path) -> Result<ExtensionManifest> {
        let content = std::fs::read_to_string(path)?;
        let manifest: ExtensionManifest = serde_json::from_str(&content)?;
        Ok(manifest)
    }
    
    /// 获取扩展目录路径
    pub fn extensions_dir(&self) -> &PathBuf {
        &self.extensions_dir
    }
}

impl ExtensionLoader {
    /// 从注册表和配置加载扩展
    pub fn load_extensions(&self, registry: &ExtensionRegistry, config: &AppConfig) -> Vec<Box<dyn Extension>> {
        registry.load_enabled(config)
    }
}

impl Default for ExtensionLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// 扩展工厂 trait - 用于编译时链接
pub trait ExtensionFactory: Send + Sync {
    /// 工厂名称（对应扩展名称）
    fn name(&self) -> &str;
    /// 创建扩展实例
    fn create(&self) -> Box<dyn Extension>;
    /// 扩展描述
    #[allow(dead_code)]
    fn description(&self) -> &str { "" }
}

/// 扩展注册表 - 管理可用的扩展工厂
pub struct ExtensionRegistry {
    /// 扩展工厂映射表
    factories: HashMap<String, Box<dyn ExtensionFactory>>,
}

impl ExtensionRegistry {
    /// 创建新的扩展注册表
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }
    
    /// 注册一个扩展工厂
    pub fn register_factory(&mut self, factory: Box<dyn ExtensionFactory>) {
        let name = factory.name().to_string();
        tracing::info!("Registered extension factory: {}", name);
        self.factories.insert(name, factory);
    }
    
    /// 列出所有可用的扩展名称
    #[allow(dead_code)]
    pub fn available_extensions(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }
    
    /// 从配置加载启用的扩展
    pub fn load_enabled(&self, config: &AppConfig) -> Vec<Box<dyn Extension>> {
        let mut extensions = Vec::new();
        
        let enabled = config.extensions_config()
            .map(|c| c.enabled.clone())
            .unwrap_or_default();
        let disabled = config.extensions_config()
            .map(|c| c.disabled.clone())
            .unwrap_or_default();
        
        // 如果 enabled 为空，默认加载所有非 disabled 的扩展
        if enabled.is_empty() {
            for (name, factory) in &self.factories {
                if !disabled.contains(name) {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| factory.create())) {
                        Ok(ext) => {
                            tracing::info!("Loaded extension: {}", name);
                            extensions.push(ext);
                        }
                        Err(_) => {
                            tracing::error!("Failed to create extension: {} (panic)", name);
                        }
                    }
                }
            }
        } else {
            for name in &enabled {
                if disabled.contains(name) {
                    continue;
                }
                if let Some(factory) = self.factories.get(name) {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| factory.create())) {
                        Ok(ext) => {
                            tracing::info!("Loaded extension: {}", name);
                            extensions.push(ext);
                        }
                        Err(_) => {
                            tracing::error!("Failed to create extension: {} (panic)", name);
                        }
                    }
                } else {
                    tracing::warn!("Extension factory not found: {}", name);
                }
            }
        }
        
        extensions
    }
    
    /// 获取注册的工厂数量
    #[allow(dead_code)]
    pub fn factory_count(&self) -> usize {
        self.factories.len()
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// WASM 扩展加载器 - 使用 wasmtime 动态加载 WASM 扩展
///
/// 集成沙箱，提供安全的 WASM 执行环境
#[allow(dead_code)] // WASM 扩展系统尚未完全集成
pub struct WasmExtensionLoader {
    /// 默认沙箱配置（用于没有指定沙箱配置的扩展）
    #[allow(dead_code)]
    default_sandbox_config: SandboxConfig,
}

impl WasmExtensionLoader {
    /// 创建新的 WASM 扩展加载器
    pub fn new() -> Result<Self, ExtensionLoadError> {
        Ok(Self {
            default_sandbox_config: SandboxConfig::default(),
        })
    }

    /// 使用自定义默认沙箱配置创建加载器
    #[allow(dead_code)] // 预留给未来使用
    pub fn with_sandbox_config(config: SandboxConfig) -> Result<Self, ExtensionLoadError> {
        Ok(Self {
            default_sandbox_config: config,
        })
    }

    /// 扫描目录下的 WASM 扩展
    /// 查找 .wasm 文件和对应的 manifest.json
    pub fn scan_wasm_extensions(&self, dir: &Path) -> Vec<(WasmExtensionManifest, PathBuf)> {
        let mut results = Vec::new();

        if !dir.exists() {
            tracing::debug!("WASM extensions directory does not exist: {:?}", dir);
            return results;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Failed to read WASM extensions directory: {}", e);
                return results;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            
            // 检查是否是包含 manifest.json 的目录
            if path.is_dir() {
                let manifest_path = path.join("manifest.json");
                let wasm_file = Self::find_wasm_file(&path);
                
                if manifest_path.exists() {
                    if let Some(wasm) = wasm_file {
                        match WasmExtensionManifest::from_file(&manifest_path) {
                            Ok(manifest) => {
                                tracing::info!(
                                    "Found WASM extension: {} v{} at {:?}",
                                    manifest.name,
                                    manifest.version,
                                    path
                                );
                                results.push((manifest, wasm));
                            }
                            Err(e) => {
                                tracing::warn!("Failed to load WASM manifest {:?}: {}", manifest_path, e);
                            }
                        }
                    }
                }
            }
            // 或者直接是 .wasm 文件
            else if path.extension().map(|e| e == "wasm").unwrap_or(false) {
                // 尝试查找同目录下的 manifest.json
                let manifest_path = path.with_extension("json");
                if manifest_path.exists() {
                    match WasmExtensionManifest::from_file(&manifest_path) {
                        Ok(manifest) => {
                            tracing::info!(
                                "Found WASM extension: {} v{} at {:?}",
                                manifest.name,
                                manifest.version,
                                path
                            );
                            results.push((manifest, path.clone()));
                        }
                        Err(e) => {
                            tracing::warn!("Failed to load WASM manifest {:?}: {}", manifest_path, e);
                        }
                    }
                }
            }
        }

        results
    }

    /// 在目录中查找 .wasm 文件
    fn find_wasm_file(dir: &Path) -> Option<PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "wasm").unwrap_or(false) {
                return Some(path);
            }
        }
        None
    }

    /// 加载单个 WASM 扩展
    /// 编译 .wasm 文件，创建沙箱化的 Store 和 Instance
    ///
    /// 安全特性:
    /// - 文件系统访问仅限 manifest 中声明的路径
    /// - 网络访问默认禁止
    /// - 内存和 CPU 资源受限
    /// - 扩展崩溃不影响主程序
    pub fn load_wasm(&self, path: &Path) -> Result<WasmExtension, ExtensionLoadError> {
        // 1. 查找并加载 manifest
        // 先检查同目录下的 manifest.json，再检查同名 .json 文件
        let manifest_path = {
            let candidate_dir = path
                .parent()
                .map(|p| p.join("manifest.json"))
                .filter(|p| p.exists());

            if let Some(p) = candidate_dir {
                p
            } else {
                let json_sidecar = path.with_extension("json");
                if json_sidecar.exists() {
                    json_sidecar
                } else {
                    return Err(ExtensionLoadError::ManifestError(
                        "manifest.json not found".to_string(),
                    ));
                }
            }
        };
    
        let manifest = WasmExtensionManifest::from_file(&manifest_path)?;
    
        // 2. 获取扩展目录（用于解析相对路径）
        let extension_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    
        // 3. 根据 manifest 创建沙箱
        let sandbox = if let Some(ref sandbox_config) = manifest.sandbox {
            // 使用 manifest 中指定的沙箱配置
            WasmSandbox::with_extension_dir(sandbox_config.clone(), extension_dir)?
        } else {
            // 从权限声明构建沙箱配置
            WasmSandbox::from_manifest_with_dir(&manifest, extension_dir)?
        };
    
        tracing::info!(
            "Loading WASM extension {} v{} with sandbox config: {:?}",
            manifest.name,
            manifest.version,
            sandbox.config()
        );
    
        // 4. 编译 WASM 模块
        let wasm_bytes = std::fs::read(path)
            .map_err(|e| ExtensionLoadError::IoError(e.to_string()))?;
    
        let module = sandbox.compile_module(&wasm_bytes)?;
    
        // 5. 在沙箱中实例化模块
        let (mut store, instance) = sandbox.instantiate_module(&module)?;
    
        // 6. 调用初始化函数（如果存在）- 使用安全执行
        if let Ok(init_func) = instance.get_typed_func::<(), ()>(&mut store, "init") {
            match sandbox.execute_safely(&mut store, |s| {
                init_func.call(s, ())?;
                Ok(())
            }) {
                Ok(()) => {
                    tracing::info!("WASM init function succeeded for {}", manifest.name);
                }
                Err(e) => {
                    tracing::warn!("WASM init function failed for {}: {}", manifest.name, e);
                    // 初始化失败不阻止加载，只是记录警告
                }
            }
        }
    
        // 7. 尝试调用 _start（WASI 命令默认入口）
        if let Ok(start_func) = instance.get_typed_func::<(), ()>(&mut store, "_start") {
            match sandbox.execute_safely(&mut store, |s| {
                start_func.call(s, ())?;
                Ok(())
            }) {
                Ok(()) => {
                    tracing::debug!("WASM _start function succeeded for {}", manifest.name);
                }
                Err(e) => {
                    tracing::debug!("WASM _start function call: {}", e);
                }
            }
        }
    
        // 8. 创建 WasmExtension
        let mut extension = WasmExtension::new(manifest, path.to_path_buf());
        let instance_info = WasmInstance::new(
            extension.id.clone(),
            extension.name.clone(),
            extension.version.clone(),
            path.to_path_buf(),
        );
        extension.set_instance(instance_info);
    
        tracing::info!(
            "Successfully loaded WASM extension: {} v{} (sandboxed)",
            extension.name,
            extension.version
        );
    
        Ok(extension)
    }

    /// 卸载 WASM 扩展
    /// 通过 drop Store/Instance 来释放资源
    pub fn unload(&self, extension: &mut WasmExtension) -> Result<(), ExtensionLoadError> {
        if extension.instance.is_none() {
            return Err(ExtensionLoadError::ExtensionNotFound(
                extension.id.clone()
            ));
        }

        // 调用 deinit 函数（如果存在）
        // 注意：由于 Store 和 Instance 被封装，这里只是标记为已卸载
        // 实际的资源释放由 drop 处理
        extension.instance = None;
        extension.is_active = false;
        
        tracing::info!(
            "Unloaded WASM extension: {} v{}",
            extension.name,
            extension.version
        );

        Ok(())
    }

    /// 获取默认沙箱配置
    #[allow(dead_code)] // WASM 扩展系统尚未完全集成
    pub fn default_sandbox_config(&self) -> &SandboxConfig {
        &self.default_sandbox_config
    }
}

impl Default for WasmExtensionLoader {
    fn default() -> Self {
        Self::new().expect("Failed to create WasmExtensionLoader")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ExtensionsConfig};
    use std::sync::Arc;
    use std::path::PathBuf;

    // ==================== Mock Extension ====================

    struct MockExtension {
        manifest: ExtensionManifest,
    }

    impl MockExtension {
        fn new(name: &str) -> Self {
            Self {
                manifest: ExtensionManifest {
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    description: "Mock extension".to_string(),
                    author: "test".to_string(),
                    entry_point: PathBuf::new(),
                },
            }
        }
    }

    #[async_trait::async_trait]
    impl super::super::types::Extension for MockExtension {
        fn manifest(&self) -> &ExtensionManifest {
            &self.manifest
        }
        
        async fn activate(&mut self, _ctx: &super::super::api::ExtensionContext) -> anyhow::Result<()> {
            Ok(())
        }
        
        async fn deactivate(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
        
        fn registered_tools(&self) -> Vec<Arc<dyn pi_agent::types::AgentTool>> {
            vec![]
        }
        
        fn registered_commands(&self) -> Vec<super::super::types::SlashCommand> {
            vec![]
        }
    }

    // ==================== Mock Factory ====================

    struct MockFactory {
        name: String,
    }

    impl MockFactory {
        fn new(name: &str) -> Self {
            Self { name: name.to_string() }
        }
    }

    impl ExtensionFactory for MockFactory {
        fn name(&self) -> &str {
            &self.name
        }
        
        fn create(&self) -> Box<dyn super::super::types::Extension> {
            Box::new(MockExtension::new(&self.name))
        }
        
        fn description(&self) -> &str {
            "Mock factory for testing"
        }
    }

    // ==================== ExtensionRegistry Tests ====================

    #[test]
    fn test_extension_registry_new() {
        let registry = ExtensionRegistry::new();
        assert_eq!(registry.factory_count(), 0);
    }

    #[test]
    fn test_extension_registry_default() {
        let registry = ExtensionRegistry::default();
        assert_eq!(registry.factory_count(), 0);
    }

    #[test]
    fn test_extension_registry_register_factory() {
        let mut registry = ExtensionRegistry::new();
        
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        assert_eq!(registry.factory_count(), 1);
        
        registry.register_factory(Box::new(MockFactory::new("ext2")));
        assert_eq!(registry.factory_count(), 2);
        
        // Same name should overwrite
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        assert_eq!(registry.factory_count(), 2);
    }

    #[test]
    fn test_extension_registry_available_extensions() {
        let mut registry = ExtensionRegistry::new();
        
        let available = registry.available_extensions();
        assert!(available.is_empty());
        
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        registry.register_factory(Box::new(MockFactory::new("ext2")));
        
        let available = registry.available_extensions();
        assert_eq!(available.len(), 2);
        assert!(available.contains(&"ext1"));
        assert!(available.contains(&"ext2"));
    }

    #[test]
    fn test_extension_registry_factory_count() {
        let mut registry = ExtensionRegistry::new();
        
        assert_eq!(registry.factory_count(), 0);
        
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        assert_eq!(registry.factory_count(), 1);
        
        registry.register_factory(Box::new(MockFactory::new("ext2")));
        assert_eq!(registry.factory_count(), 2);
    }

    // ==================== ExtensionRegistry load_enabled Tests ====================

    fn create_config_with_extensions(enabled: Vec<String>, disabled: Vec<String>) -> AppConfig {
        AppConfig {
            extensions: Some(ExtensionsConfig {
                enabled,
                disabled,
                settings: std::collections::HashMap::new(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_load_enabled_empty_enabled_loads_all() {
        let mut registry = ExtensionRegistry::new();
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        registry.register_factory(Box::new(MockFactory::new("ext2")));
        registry.register_factory(Box::new(MockFactory::new("ext3")));
        
        // Empty enabled list -> load all
        let config = create_config_with_extensions(vec![], vec![]);
        let extensions = registry.load_enabled(&config);
        
        assert_eq!(extensions.len(), 3);
    }

    #[test]
    fn test_load_enabled_specific_enabled() {
        let mut registry = ExtensionRegistry::new();
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        registry.register_factory(Box::new(MockFactory::new("ext2")));
        registry.register_factory(Box::new(MockFactory::new("ext3")));
        
        // Only load ext1 and ext2
        let config = create_config_with_extensions(
            vec!["ext1".to_string(), "ext2".to_string()],
            vec![]
        );
        let extensions = registry.load_enabled(&config);
        
        assert_eq!(extensions.len(), 2);
    }

    #[test]
    fn test_load_enabled_disabled_excludes() {
        let mut registry = ExtensionRegistry::new();
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        registry.register_factory(Box::new(MockFactory::new("ext2")));
        registry.register_factory(Box::new(MockFactory::new("ext3")));
        
        // Empty enabled -> load all, but exclude ext3 via disabled
        let config = create_config_with_extensions(
            vec![],
            vec!["ext3".to_string()]
        );
        let extensions = registry.load_enabled(&config);
        
        assert_eq!(extensions.len(), 2);
    }

    #[test]
    fn test_load_enabled_enabled_and_disabled_conflict() {
        let mut registry = ExtensionRegistry::new();
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        registry.register_factory(Box::new(MockFactory::new("ext2")));
        registry.register_factory(Box::new(MockFactory::new("ext3")));
        
        // ext2 is both enabled and disabled -> disabled wins
        let config = create_config_with_extensions(
            vec!["ext1".to_string(), "ext2".to_string()],
            vec!["ext2".to_string()]
        );
        let extensions = registry.load_enabled(&config);
        
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].manifest().name, "ext1");
    }

    #[test]
    fn test_load_enabled_nonexistent_enabled() {
        let mut registry = ExtensionRegistry::new();
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        
        // Request non-existent extension
        let config = create_config_with_extensions(
            vec!["ext1".to_string(), "nonexistent".to_string()],
            vec![]
        );
        let extensions = registry.load_enabled(&config);
        
        // Only ext1 should be loaded
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].manifest().name, "ext1");
    }

    // ==================== ExtensionLoader Tests ====================

    #[test]
    fn test_extension_loader_new() {
        let loader = ExtensionLoader::new();
        // Default path should be ~/.pi/extensions
        assert!(loader.extensions_dir().to_string_lossy().contains("extensions"));
    }

    #[test]
    fn test_extension_loader_with_dir() {
        let custom_path = PathBuf::from("/custom/extensions");
        let loader = ExtensionLoader::with_dir(custom_path.clone());
        
        assert_eq!(*loader.extensions_dir(), custom_path);
    }

    #[test]
    fn test_extension_loader_scan_extensions_nonexistent_dir() {
        let loader = ExtensionLoader::with_dir(PathBuf::from("/nonexistent/path"));
        let manifests = loader.scan_extensions();
        
        assert!(manifests.is_empty());
    }

    #[test]
    fn test_extension_loader_default() {
        let loader = ExtensionLoader::default();
        assert!(loader.extensions_dir().to_string_lossy().contains("extensions"));
    }

    #[test]
    fn test_extension_loader_load_extensions() {
        let loader = ExtensionLoader::new();
        let mut registry = ExtensionRegistry::new();
        registry.register_factory(Box::new(MockFactory::new("ext1")));
        
        let config = AppConfig::default();
        let extensions = loader.load_extensions(&registry, &config);
        
        assert_eq!(extensions.len(), 1);
    }

    // ==================== WasmExtensionLoader Tests ====================

    #[test]
    fn test_wasm_extension_loader_new() {
        let loader = WasmExtensionLoader::new();
        assert!(loader.is_ok());
        
        let loader = loader.unwrap();
        // Default sandbox config should be available
        let _ = loader.default_sandbox_config();
    }

    #[test]
    fn test_wasm_extension_loader_default() {
        let loader = WasmExtensionLoader::default();
        // Default sandbox config should be available
        let _ = loader.default_sandbox_config();
    }

    #[test]
    fn test_wasm_extension_loader_scan_empty_dir() {
        let loader = WasmExtensionLoader::new().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        
        let results = loader.scan_wasm_extensions(temp_dir.path());
        assert!(results.is_empty());
    }

    #[test]
    fn test_wasm_extension_loader_scan_nonexistent_dir() {
        let loader = WasmExtensionLoader::new().unwrap();
        
        let results = loader.scan_wasm_extensions(Path::new("/nonexistent/path/that/does/not/exist"));
        assert!(results.is_empty());
    }

    #[test]
    fn test_wasm_extension_loader_scan_with_manifest() {
        let loader = WasmExtensionLoader::new().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        
        // Create extension directory structure
        let ext_dir = temp_dir.path().join("test-ext");
        std::fs::create_dir(&ext_dir).unwrap();
        
        // Create manifest.json
        let manifest_content = r#"{
            "name": "test-extension",
            "version": "1.0.0",
            "description": "Test extension",
            "author": "test",
            "wasm_entry": "test.wasm",
            "permissions": ["fs.read"]
        }"#;
        std::fs::write(ext_dir.join("manifest.json"), manifest_content).unwrap();
        
        // Create dummy .wasm file (not valid WASM, just for scanning)
        std::fs::write(ext_dir.join("test.wasm"), b"not valid wasm").unwrap();
        
        let results = loader.scan_wasm_extensions(temp_dir.path());
        assert_eq!(results.len(), 1);
        
        let (manifest, wasm_path) = &results[0];
        assert_eq!(manifest.name, "test-extension");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(wasm_path.file_name().unwrap(), "test.wasm");
    }

    #[test]
    fn test_wasm_extension_loader_load_invalid_wasm() {
        let loader = WasmExtensionLoader::new().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        
        // Create extension directory
        let ext_dir = temp_dir.path().join("test-ext");
        std::fs::create_dir(&ext_dir).unwrap();
        
        // Create manifest.json
        let manifest_content = r#"{
            "name": "test-extension",
            "version": "1.0.0",
            "description": "Test extension",
            "author": "test",
            "wasm_entry": "test.wasm"
        }"#;
        std::fs::write(ext_dir.join("manifest.json"), manifest_content).unwrap();
        
        // Create invalid .wasm file
        let wasm_path = ext_dir.join("test.wasm");
        std::fs::write(&wasm_path, b"this is not valid wasm").unwrap();
        
        // Loading should fail with compile error
        let result = loader.load_wasm(&wasm_path);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            ExtensionLoadError::WasmCompileError(_) => (), // Expected
            other => panic!("Expected WasmCompileError, got {:?}", other),
        }
    }

    #[test]
    fn test_wasm_extension_loader_load_missing_manifest() {
        let loader = WasmExtensionLoader::new().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        
        // Create just a .wasm file without manifest
        let wasm_path = temp_dir.path().join("test.wasm");
        std::fs::write(&wasm_path, b"not valid wasm").unwrap();
        
        // Loading should fail with manifest error
        let result = loader.load_wasm(&wasm_path);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            ExtensionLoadError::ManifestError(_) => (), // Expected
            other => panic!("Expected ManifestError, got {:?}", other),
        }
    }

    #[test]
    fn test_wasm_extension_loader_unload() {
        let loader = WasmExtensionLoader::new().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        
        // Create extension directory
        let ext_dir = temp_dir.path().join("test-ext");
        std::fs::create_dir(&ext_dir).unwrap();
        
        // Create manifest.json with required wasm_entry field
        let manifest_content = r#"{
            "name": "test-extension",
            "version": "1.0.0",
            "description": "Test extension",
            "wasm_entry": "test.wasm"
        }"#;
        std::fs::write(ext_dir.join("manifest.json"), manifest_content).unwrap();
        std::fs::write(ext_dir.join("test.wasm"), b"not valid wasm").unwrap();
        
        // Create a WasmExtension manually
        let manifest = WasmExtensionManifest::from_file(&ext_dir.join("manifest.json")).unwrap();
        let mut extension = WasmExtension::new(manifest, ext_dir.join("test.wasm"));
        
        // Set a mock instance
        extension.set_instance(WasmInstance::new(
            extension.id.clone(),
            extension.name.clone(),
            extension.version.clone(),
            extension.wasm_path.clone(),
        ));
        
        // Unload should succeed
        let result = loader.unload(&mut extension);
        assert!(result.is_ok());
        assert!(extension.instance.is_none());
        assert!(!extension.is_active);
    }

    #[test]
    fn test_wasm_extension_loader_unload_not_loaded() {
        let loader = WasmExtensionLoader::new().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        
        // Create extension directory
        let ext_dir = temp_dir.path().join("test-ext");
        std::fs::create_dir(&ext_dir).unwrap();
        
        // Create manifest.json with required wasm_entry field
        let manifest_content = r#"{
            "name": "test-extension",
            "version": "1.0.0",
            "description": "Test extension",
            "wasm_entry": "test.wasm"
        }"#;
        std::fs::write(ext_dir.join("manifest.json"), manifest_content).unwrap();
        
        // Create extension without instance
        let manifest = WasmExtensionManifest::from_file(&ext_dir.join("manifest.json")).unwrap();
        let mut extension = WasmExtension::new(manifest, ext_dir.join("test.wasm"));
        
        // Unload should fail because instance is None
        let result = loader.unload(&mut extension);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            ExtensionLoadError::ExtensionNotFound(_) => (), // Expected
            other => panic!("Expected ExtensionNotFound, got {:?}", other),
        }
    }
}
