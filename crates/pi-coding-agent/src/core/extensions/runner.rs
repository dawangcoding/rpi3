#![allow(dead_code)] // 扩展系统尚未完全集成

use super::types::{EventResult, Extension, SlashCommand, ExtensionToolWrapper, WasmExtension};
use super::loader::WasmExtensionLoader;
use super::api::ExtensionContext;
use super::hot_reload::{HotReloader, HotReloadEvent, HotReloadStatus, extract_extension_name};
use super::dispatcher::EventDispatcher;
use super::registry::{ToolRegistry, CommandRegistry};
use super::events::EventSubscription;
use pi_agent::types::{AgentTool, AgentEvent};
use std::sync::Arc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::Result;

/// 扩展管理器 - 管理所有已加载扩展的生命周期
pub struct ExtensionManager {
    extensions: Vec<Box<dyn Extension>>,
    activated: bool,
    /// 扩展动态注册的工具 (extension_name -> tools)
    dynamic_tools: HashMap<String, Vec<Arc<dyn AgentTool>>>,
    /// WASM 扩展加载器
    wasm_loader: WasmExtensionLoader,
    /// 已加载的 WASM 扩展 (extension_id -> WasmExtension)
    wasm_extensions: HashMap<String, WasmExtension>,
    /// 热重载器
    hot_reloader: Option<HotReloader>,
    /// 事件分发器
    event_dispatcher: EventDispatcher,
    /// 统一工具注册表
    tool_registry: ToolRegistry,
    /// 统一命令注册表
    command_registry: CommandRegistry,
}

impl ExtensionManager {
    /// 创建新的扩展管理器
    pub fn new() -> Self {
        Self {
            extensions: Vec::new(),
            activated: false,
            dynamic_tools: HashMap::new(),
            wasm_loader: WasmExtensionLoader::new().expect("Failed to create WASM loader"),
            wasm_extensions: HashMap::new(),
            hot_reloader: None,
            event_dispatcher: EventDispatcher::new(),
            tool_registry: ToolRegistry::new(),
            command_registry: CommandRegistry::new(),
        }
    }

    /// 启动热重载监控
    pub fn start_watching(&mut self, path: PathBuf) -> Result<()> {
        let mut reloader = HotReloader::new(path);
        reloader.start_watching()?;
        self.hot_reloader = Some(reloader);
        Ok(())
    }

    /// 停止热重载监控
    pub fn stop_watching(&mut self) {
        if let Some(ref mut reloader) = self.hot_reloader {
            reloader.stop_watching();
        }
        self.hot_reloader = None;
    }

    /// 处理热重载事件
    /// 
    /// 对于 ExtensionChanged：卸载旧版本 -> 重新加载新版本（使用沙箱）-> 如果加载失败，尝试恢复旧版本
    /// 对于 ExtensionRemoved：卸载扩展
    /// 返回处理结果事件列表
    pub fn process_reload_events(&mut self) -> Vec<HotReloadEvent> {
        let mut results = Vec::new();
        
        if let Some(ref reloader) = self.hot_reloader {
            let events = reloader.poll_events();
            
            for event in events {
                match event {
                    HotReloadEvent::ExtensionChanged(ref path) => {
                        let result = self.handle_extension_changed(path);
                        results.push(result);
                    }
                    HotReloadEvent::ExtensionRemoved(ref path) => {
                        let result = self.handle_extension_removed(path);
                        results.push(result);
                    }
                    _ => {
                        results.push(event);
                    }
                }
            }
        }
        
        results
    }

    /// 处理扩展文件变更事件
    fn handle_extension_changed(&mut self, path: &Path) -> HotReloadEvent {
        // 提取扩展名称（用于匹配已加载的扩展）
        let extension_name = match extract_extension_name(path) {
            Some(name) => name,
            None => {
                return HotReloadEvent::ReloadFailed(
                    "unknown".to_string(),
                    format!("Cannot extract extension name from path: {:?}", path)
                );
            }
        };
        
        // 查找已加载的相同名称的扩展（可能版本不同）
        let existing_id = self.wasm_extensions.iter()
            .find(|(_, ext)| ext.name == extension_name)
            .map(|(id, _)| id.clone());
        
        // 保存旧扩展的备份（用于失败时恢复）
        let old_extension = existing_id.as_ref()
            .and_then(|id| self.wasm_extensions.remove(id));
        
        // 尝试加载新版本
        match self.wasm_loader.load_wasm(path) {
            Ok(new_extension) => {
                let new_id = new_extension.id.clone();
                
                // 如果存在旧扩展，现在可以安全丢弃（因为新扩展已加载成功）
                if old_extension.is_some() {
                    tracing::info!(
                        "Hot reload: Replaced extension {} with new version {}",
                        extension_name, new_extension.version
                    );
                } else {
                    tracing::info!(
                        "Hot reload: Loaded new extension {} v{}",
                        new_extension.name, new_extension.version
                    );
                }
                
                // 插入新扩展
                self.wasm_extensions.insert(new_id.clone(), new_extension);
                
                // 更新热重载状态
                if let Some(ref mut reloader) = self.hot_reloader {
                    reloader.record_reload_success(&new_id);
                }
                
                HotReloadEvent::ReloadSuccess(new_id)
            }
            Err(e) => {
                // 加载失败，恢复旧版本
                if let Some(old_ext) = old_extension {
                    let old_id = old_ext.id.clone();
                    self.wasm_extensions.insert(old_id.clone(), old_ext);
                    
                    let error_msg = format!("Failed to load new version: {}. Old version {} restored.", e, old_id);
                    tracing::error!("{}", error_msg);
                    
                    if let Some(ref mut reloader) = self.hot_reloader {
                        reloader.record_reload_failure(&old_id, &error_msg);
                    }
                    
                    HotReloadEvent::ReloadFailed(old_id, error_msg)
                } else {
                    // 没有旧版本可以恢复
                    let error_msg = format!("Failed to load extension: {}", e);
                    tracing::error!("{}", error_msg);
                    
                    if let Some(ref mut reloader) = self.hot_reloader {
                        reloader.record_reload_failure(&extension_name, &error_msg);
                    }
                    
                    HotReloadEvent::ReloadFailed(extension_name, error_msg)
                }
            }
        }
    }

    /// 处理扩展文件删除事件
    fn handle_extension_removed(&mut self, path: &Path) -> HotReloadEvent {
        // 尝试从路径提取扩展名称
        let extension_name = match extract_extension_name(path) {
            Some(name) => name,
            None => {
                return HotReloadEvent::ReloadFailed(
                    "unknown".to_string(),
                    format!("Cannot extract extension name from path: {:?}", path)
                );
            }
        };
        
        // 查找并卸载匹配的扩展
        let extension_id = self.wasm_extensions.iter()
            .find(|(_, ext)| ext.name == extension_name)
            .map(|(id, _)| id.clone());
        
        if let Some(id) = extension_id {
            if let Some(mut ext) = self.wasm_extensions.remove(&id) {
                if let Err(e) = self.wasm_loader.unload(&mut ext) {
                    tracing::warn!("Failed to unload extension {}: {}", id, e);
                }
            }
            
            tracing::info!("Hot reload: Unloaded extension {} (file removed)", extension_name);
            HotReloadEvent::ReloadSuccess(id)
        } else {
            // 扩展未加载，无需操作
            HotReloadEvent::ReloadSuccess(format!("{} (not loaded)", extension_name))
        }
    }

    /// 获取热重载状态
    pub fn hot_reload_status(&self) -> Option<&HotReloadStatus> {
        self.hot_reloader.as_ref().map(|r| r.status())
    }

    /// 是否正在热重载监控
    pub fn is_hot_reloading(&self) -> bool {
        self.hot_reloader.as_ref().map(|r| r.is_watching()).unwrap_or(false)
    }

    /// 加载单个 WASM 扩展
    pub fn load_wasm_extension(&mut self, path: &Path) -> Result<String> {
        let extension = self.wasm_loader.load_wasm(path)?;
        let id = extension.id.clone();
        
        if self.wasm_extensions.contains_key(&id) {
            return Err(anyhow::anyhow!("WASM extension {} already loaded", id));
        }
        
        tracing::info!("Loaded WASM extension: {} v{}", extension.name, extension.version);
        self.wasm_extensions.insert(id.clone(), extension);
        Ok(id)
    }

    /// 卸载指定 WASM 扩展
    pub fn unload_wasm_extension(&mut self, id: &str) -> Result<()> {
        if let Some(mut extension) = self.wasm_extensions.remove(id) {
            self.wasm_loader.unload(&mut extension)?;
            tracing::info!("Unloaded WASM extension: {}", id);
        } else {
            return Err(anyhow::anyhow!("WASM extension {} not found", id));
        }
        Ok(())
    }

    /// 批量扫描和加载 WASM 扩展
    pub fn load_all_wasm_extensions(&mut self, dir: &Path) -> Vec<(String, Result<String>)> {
        let manifests = self.wasm_loader.scan_wasm_extensions(dir);
        let mut results = Vec::new();

        for (manifest, wasm_path) in manifests {
            let result = self.wasm_loader.load_wasm(&wasm_path);
            match result {
                Ok(extension) => {
                    let id = extension.id.clone();
                    self.wasm_extensions.insert(id.clone(), extension);
                    tracing::info!("Loaded WASM extension: {} v{}", manifest.name, manifest.version);
                    results.push((manifest.name, Ok(id)));
                }
                Err(e) => {
                    tracing::error!("Failed to load WASM extension {}: {}", manifest.name, e);
                    results.push((manifest.name, Err(anyhow::anyhow!("{}", e))));
                }
            }
        }

        results
    }

    /// 获取已加载的 WASM 扩展列表
    pub fn list_wasm_extensions(&self) -> Vec<&WasmExtension> {
        self.wasm_extensions.values().collect()
    }

    /// 获取指定 WASM 扩展
    pub fn get_wasm_extension(&self, id: &str) -> Option<&WasmExtension> {
        self.wasm_extensions.get(id)
    }

    /// 获取 WASM 扩展数量
    pub fn wasm_extension_count(&self) -> usize {
        self.wasm_extensions.len()
    }
    
    /// 注册一个扩展
    pub fn register(&mut self, extension: Box<dyn Extension>) {
        tracing::info!("Registered extension: {}", extension.manifest().name);
        self.extensions.push(extension);
    }
    
    /// 激活所有已注册的扩展（包括 WASM 扩展）
    pub async fn activate_all(&mut self, ctx: &ExtensionContext) -> Result<()> {
        // 激活编译时扩展
        for ext in &mut self.extensions {
            let name = ext.manifest().name.clone();
            match ext.activate(ctx).await {
                Ok(()) => {
                    tracing::info!("Activated extension: {}", name);
                    
                    // 注册扩展的事件订阅
                    let subscriptions = ext.event_subscriptions();
                    if subscriptions.is_empty() {
                        // 默认订阅所有事件
                        self.event_dispatcher.register_extension(
                            name.clone(),
                            vec![EventSubscription::default()],
                        );
                    } else {
                        self.event_dispatcher.register_extension(name.clone(), subscriptions);
                    }
                    
                    // 注册扩展的工具到统一注册表
                    for tool in ext.registered_tools() {
                        if let Err(e) = self.tool_registry.register_tool(&name, tool) {
                            tracing::warn!("Failed to register tool from {}: {}", name, e);
                        }
                    }
                    
                    // 注册扩展的命令到统一注册表
                    for cmd in ext.registered_commands() {
                        if let Err(e) = self.command_registry.register_command(&name, cmd) {
                            tracing::warn!("Failed to register command from {}: {}", name, e);
                        }
                    }
                }
                Err(e) => tracing::error!("Failed to activate extension {}: {}", name, e),
            }
        }
        
        // 激活 WASM 扩展
        for (id, ext) in &mut self.wasm_extensions {
            if !ext.is_active {
                // WASM 扩展激活逻辑（目前只是标记状态）
                ext.is_active = true;
                tracing::info!("Activated WASM extension: {}", id);
            }
        }
        
        self.activated = true;
        Ok(())
    }
    
    /// 停用所有扩展（包括 WASM 扩展）
    pub async fn deactivate_all(&mut self) -> Result<()> {
        // 停用编译时扩展
        for ext in &mut self.extensions {
            let name = ext.manifest().name.clone();
            match ext.deactivate().await {
                Ok(()) => {
                    tracing::debug!("Deactivated extension: {}", name);
                    
                    // 注销扩展的事件订阅
                    self.event_dispatcher.unregister_extension(&name);
                    
                    // 从统一注册表中移除扩展的工具和命令
                    self.tool_registry.unregister_extension_tools(&name);
                    self.command_registry.unregister_extension_commands(&name);
                }
                Err(e) => tracing::warn!("Failed to deactivate extension {}: {}", name, e),
            }
        }
        
        // 停用 WASM 扩展
        for (id, ext) in &mut self.wasm_extensions {
            if ext.is_active {
                ext.is_active = false;
                tracing::debug!("Deactivated WASM extension: {}", id);
            }
        }
        
        self.activated = false;
        Ok(())
    }
    
    /// 收集所有扩展注册的工具
    pub fn get_all_tools(&self) -> Vec<Arc<dyn AgentTool>> {
        let mut tools = Vec::new();
        // 扩展通过 registered_tools() 返回的工具
        for ext in &self.extensions {
            tools.extend(ext.registered_tools());
        }
        // 动态注册的工具
        for ext_tools in self.dynamic_tools.values() {
            tools.extend(ext_tools.iter().cloned());
        }
        tools
    }

    /// 动态注册工具
    #[allow(dead_code)]
    pub fn register_tool(&mut self, extension_name: &str, tool: Arc<dyn AgentTool>) {
        let wrapped = Arc::new(ExtensionToolWrapper::new(tool, extension_name.to_string()));
        self.dynamic_tools
            .entry(extension_name.to_string())
            .or_default()
            .push(wrapped);
        tracing::info!(
            "Extension {} registered dynamic tool: {}",
            extension_name,
            self.dynamic_tools.get(extension_name).unwrap().last().unwrap().name()
        );
    }

    /// 取消注册工具
    #[allow(dead_code)]
    pub fn unregister_tool(&mut self, extension_name: &str, tool_name: &str) {
        if let Some(tools) = self.dynamic_tools.get_mut(extension_name) {
            tools.retain(|t| t.name() != tool_name);
            tracing::info!("Extension {} unregistered tool: {}", extension_name, tool_name);
        }
    }

    /// 获取指定扩展的工具列表
    pub fn get_extension_tools(&self, extension_name: &str) -> Vec<Arc<dyn AgentTool>> {
        self.dynamic_tools.get(extension_name).cloned().unwrap_or_default()
    }

    /// 获取工具的来源扩展名称
    #[allow(dead_code)]
    pub fn get_tool_source(&self, tool_name: &str) -> Option<String> {
        for (ext_name, tools) in &self.dynamic_tools {
            if tools.iter().any(|t| t.name() == tool_name) {
                return Some(ext_name.clone());
            }
        }
        None
    }
    
    /// 收集所有扩展注册的 slash 命令
    pub fn get_all_commands(&self) -> Vec<SlashCommand> {
        let mut commands = Vec::new();
        for ext in &self.extensions {
            commands.extend(ext.registered_commands());
        }
        commands
    }
    
    /// 向所有扩展分发事件
    #[allow(dead_code)]
    pub async fn dispatch_event(&self, event: &AgentEvent) -> Vec<EventResult> {
        let dispatch_result = self.dispatch_event_with_control(event).await;
        dispatch_result.results.into_iter().map(|(_, r)| r).collect()
    }
    
    /// 向所有扩展分发事件（带完整控制信息）
    ///
    /// 返回 DispatchResult，包含是否被 Block、Modified 数据等
    #[allow(dead_code)]
    pub async fn dispatch_event_with_control(&self, event: &AgentEvent) -> super::dispatcher::DispatchResult {
        use std::collections::HashSet;
        use super::dispatcher::DispatchResult;
        
        let mut result = DispatchResult::default();
        
        // 快速路径：如果没有处理器，直接返回
        if !self.event_dispatcher.has_handlers_for(event) {
            return result;
        }
        
        // 获取匹配的处理器（已按优先级排序）
        let handlers = self.event_dispatcher.registry().get_handlers_for_event(event);
        if handlers.is_empty() {
            return result;
        }
        
        let mut processed: HashSet<&str> = HashSet::new();
        
        for record in handlers {
            if processed.contains(record.extension_name.as_str()) {
                continue;
            }
            processed.insert(&record.extension_name);
            
            // 在 self.extensions 中查找对应扩展
            let ext = self.extensions.iter()
                .find(|e| e.manifest().name == record.extension_name);
            
            let ext = match ext {
                Some(e) => e,
                None => continue,
            };
            
            // 带超时调用
            let timeout_result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                ext.on_event(event)
            ).await;
            
            match timeout_result {
                Ok(Ok(event_result)) => {
                    match &event_result {
                        EventResult::StopPropagation => {
                            result.propagation_stopped = true;
                            result.results.push((record.extension_name.clone(), event_result));
                            break;
                        }
                        EventResult::Block(reason) => {
                            result.blocked = true;
                            result.block_reason = Some(reason.clone());
                        }
                        EventResult::Modified(data) => {
                            result.modified_data = Some(data.clone());
                        }
                        EventResult::Continue => {}
                    }
                    result.results.push((record.extension_name.clone(), event_result));
                }
                Ok(Err(e)) => {
                    tracing::warn!("Extension '{}' event handler error: {}", record.extension_name, e);
                }
                Err(_) => {
                    tracing::warn!("Extension '{}' event handler timed out", record.extension_name);
                }
            }
        }
        
        result
    }
    
    /// 获取已注册扩展数量
    #[allow(dead_code)]
    pub fn extension_count(&self) -> usize {
        self.extensions.len()
    }
    
    /// 获取扩展列表信息
    pub fn list_extensions(&self) -> Vec<&super::types::ExtensionManifest> {
        self.extensions.iter().map(|e| e.manifest()).collect()
    }
    
    /// 是否已激活
    #[allow(dead_code)]
    pub fn is_activated(&self) -> bool {
        self.activated
    }
    
    /// 获取工具注册表（只读）
    #[allow(dead_code)]
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }
    
    /// 获取工具注册表（可变）
    #[allow(dead_code)]
    pub fn tool_registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tool_registry
    }
    
    /// 获取命令注册表（只读）
    #[allow(dead_code)]
    pub fn command_registry(&self) -> &CommandRegistry {
        &self.command_registry
    }
    
    /// 获取命令注册表（可变）
    #[allow(dead_code)]
    pub fn command_registry_mut(&mut self) -> &mut CommandRegistry {
        &mut self.command_registry
    }
    
    /// 获取事件分发器（只读）
    #[allow(dead_code)]
    pub fn event_dispatcher(&self) -> &EventDispatcher {
        &self.event_dispatcher
    }
}

impl Default for ExtensionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::core::extensions::types::{ExtensionManifest, Extension, EventResult, SlashCommand};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::path::PathBuf;

    // ==================== Mock Extension ====================

    struct MockExtension {
        manifest: ExtensionManifest,
        activated: Arc<AtomicBool>,
    }

    impl MockExtension {
        fn new(name: &str) -> Self {
            Self {
                manifest: ExtensionManifest {
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    description: "Mock extension for testing".to_string(),
                    author: "test".to_string(),
                    entry_point: PathBuf::new(),
                },
                activated: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    #[async_trait::async_trait]
    impl Extension for MockExtension {
        fn manifest(&self) -> &ExtensionManifest {
            &self.manifest
        }
        
        async fn activate(&mut self, _ctx: &super::super::api::ExtensionContext) -> anyhow::Result<()> {
            self.activated.store(true, Ordering::Relaxed);
            Ok(())
        }
        
        async fn deactivate(&mut self) -> anyhow::Result<()> {
            self.activated.store(false, Ordering::Relaxed);
            Ok(())
        }
        
        fn registered_tools(&self) -> Vec<Arc<dyn AgentTool>> {
            vec![]
        }
        
        fn registered_commands(&self) -> Vec<SlashCommand> {
            vec![]
        }
        
        fn event_subscriptions(&self) -> Vec<super::super::events::EventSubscription> {
            vec![super::super::events::EventSubscription::all()]
        }

        async fn on_event(&self, _event: &AgentEvent) -> anyhow::Result<EventResult> {
            Ok(EventResult::Continue)
        }
    }

    // ==================== Mock Tool ====================

    struct MockTool {
        name: String,
    }

    impl MockTool {
        fn new(name: &str) -> Self {
            Self { name: name.to_string() }
        }
    }

    #[async_trait::async_trait]
    impl AgentTool for MockTool {
        fn name(&self) -> &str {
            &self.name
        }
        
        fn label(&self) -> &str {
            &self.name
        }
        
        fn description(&self) -> &str {
            "Mock tool for testing"
        }
        
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        
        async fn execute(
            &self,
            _tool_call_id: &str,
            _params: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
            _on_update: Option<Box<dyn Fn(pi_agent::types::AgentToolResult) + Send + Sync>>,
        ) -> anyhow::Result<pi_agent::types::AgentToolResult> {
            Ok(pi_agent::types::AgentToolResult {
                content: vec![],
                details: serde_json::Value::Null,
            })
        }
    }

    // ==================== ExtensionManager Tests ====================

    #[test]
    fn test_extension_manager_new() {
        let manager = ExtensionManager::new();
        
        assert_eq!(manager.extension_count(), 0);
        assert!(!manager.is_activated());
    }

    #[test]
    fn test_extension_manager_register() {
        let mut manager = ExtensionManager::new();
        
        manager.register(Box::new(MockExtension::new("ext1")));
        assert_eq!(manager.extension_count(), 1);
        
        manager.register(Box::new(MockExtension::new("ext2")));
        assert_eq!(manager.extension_count(), 2);
    }

    #[test]
    fn test_extension_manager_default() {
        let manager = ExtensionManager::default();
        assert_eq!(manager.extension_count(), 0);
    }

    #[tokio::test]
    async fn test_extension_manager_activate_all() {
        let mut manager = ExtensionManager::new();
        let ext1 = MockExtension::new("ext1");
        let activated1 = ext1.activated.clone();
        manager.register(Box::new(ext1));
        
        let ext2 = MockExtension::new("ext2");
        let activated2 = ext2.activated.clone();
        manager.register(Box::new(ext2));
        
        assert!(!activated1.load(Ordering::Relaxed));
        assert!(!activated2.load(Ordering::Relaxed));
        
        let config = AppConfig::default();
        let ctx = super::super::api::ExtensionContext::new(
            PathBuf::from("."),
            config,
            "test-session".to_string(),
            "test-ext",
        );
        
        let result = manager.activate_all(&ctx).await;
        assert!(result.is_ok());
        assert!(manager.is_activated());
    }

    #[tokio::test]
    async fn test_extension_manager_deactivate_all() {
        let mut manager = ExtensionManager::new();
        let ext = MockExtension::new("ext1");
        let activated = ext.activated.clone();
        manager.register(Box::new(ext));
        
        let config = AppConfig::default();
        let ctx = super::super::api::ExtensionContext::new(
            PathBuf::from("."),
            config,
            "test-session".to_string(),
            "test-ext",
        );
        
        manager.activate_all(&ctx).await.unwrap();
        assert!(activated.load(Ordering::Relaxed));
        
        let result = manager.deactivate_all().await;
        assert!(result.is_ok());
        assert!(!manager.is_activated());
    }

    #[tokio::test]
    async fn test_extension_manager_dispatch_event() {
        let mut manager = ExtensionManager::new();
        manager.register(Box::new(MockExtension::new("ext1")));
        manager.register(Box::new(MockExtension::new("ext2")));
        
        // 需要激活扩展才能注册事件订阅
        let ctx = ExtensionContext::new(
            PathBuf::from("."),
            crate::config::AppConfig::default(),
            "test-session".to_string(),
            "test",
        );
        manager.activate_all(&ctx).await.unwrap();
        
        let event = AgentEvent::AgentStart;
        let results = manager.dispatch_event(&event).await;
        
        assert_eq!(results.len(), 2);
        for result in results {
            assert!(matches!(result, EventResult::Continue));
        }
    }

    #[test]
    fn test_extension_manager_register_tool() {
        let mut manager = ExtensionManager::new();
        let tool = Arc::new(MockTool::new("test-tool"));
        
        manager.register_tool("test-ext", tool);
        
        let tools = manager.get_extension_tools("test-ext");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "test-tool");
    }

    #[test]
    fn test_extension_manager_unregister_tool() {
        let mut manager = ExtensionManager::new();
        let tool = Arc::new(MockTool::new("test-tool"));
        
        manager.register_tool("test-ext", tool);
        assert_eq!(manager.get_extension_tools("test-ext").len(), 1);
        
        manager.unregister_tool("test-ext", "test-tool");
        assert_eq!(manager.get_extension_tools("test-ext").len(), 0);
    }

    #[test]
    fn test_extension_manager_get_extension_tools() {
        let mut manager = ExtensionManager::new();
        
        // Non-existent extension returns empty
        assert!(manager.get_extension_tools("nonexistent").is_empty());
        
        // Add tools
        manager.register_tool("ext1", Arc::new(MockTool::new("tool1")));
        manager.register_tool("ext1", Arc::new(MockTool::new("tool2")));
        manager.register_tool("ext2", Arc::new(MockTool::new("tool3")));
        
        assert_eq!(manager.get_extension_tools("ext1").len(), 2);
        assert_eq!(manager.get_extension_tools("ext2").len(), 1);
    }

    #[test]
    fn test_extension_manager_get_tool_source() {
        let mut manager = ExtensionManager::new();
        manager.register_tool("ext1", Arc::new(MockTool::new("tool1")));
        manager.register_tool("ext2", Arc::new(MockTool::new("tool2")));
        
        assert_eq!(manager.get_tool_source("tool1"), Some("ext1".to_string()));
        assert_eq!(manager.get_tool_source("tool2"), Some("ext2".to_string()));
        assert_eq!(manager.get_tool_source("nonexistent"), None);
    }

    #[test]
    fn test_extension_manager_get_all_tools() {
        let mut manager = ExtensionManager::new();
        
        // Static tools from extensions
        // (MockExtension returns empty registered_tools)
        manager.register(Box::new(MockExtension::new("ext1")));
        
        // Dynamic tools
        manager.register_tool("ext1", Arc::new(MockTool::new("dynamic-tool")));
        
        let all_tools = manager.get_all_tools();
        assert_eq!(all_tools.len(), 1);
        assert_eq!(all_tools[0].name(), "dynamic-tool");
    }

    #[test]
    fn test_extension_manager_list_extensions() {
        let mut manager = ExtensionManager::new();
        manager.register(Box::new(MockExtension::new("ext1")));
        manager.register(Box::new(MockExtension::new("ext2")));
        
        let manifests = manager.list_extensions();
        assert_eq!(manifests.len(), 2);
        
        let names: Vec<&str> = manifests.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"ext1"));
        assert!(names.contains(&"ext2"));
    }

    // ==================== WASM Extension Tests ====================

    #[test]
    fn test_extension_manager_wasm_extension_count() {
        let manager = ExtensionManager::new();
        assert_eq!(manager.wasm_extension_count(), 0);
    }

    #[test]
    fn test_extension_manager_list_wasm_extensions_empty() {
        let manager = ExtensionManager::new();
        let extensions = manager.list_wasm_extensions();
        assert!(extensions.is_empty());
    }

    #[test]
    fn test_extension_manager_get_wasm_extension_not_found() {
        let manager = ExtensionManager::new();
        assert!(manager.get_wasm_extension("nonexistent").is_none());
    }

    #[test]
    fn test_extension_manager_load_wasm_extension_invalid() {
        let mut manager = ExtensionManager::new();
        let temp_dir = tempfile::tempdir().unwrap();
        
        // Create extension directory
        let ext_dir = temp_dir.path().join("test-ext");
        std::fs::create_dir(&ext_dir).unwrap();
        
        // Create manifest.json
        let manifest_content = r#"{
            "name": "test-extension",
            "version": "1.0.0",
            "description": "Test extension"
        }"#;
        std::fs::write(ext_dir.join("manifest.json"), manifest_content).unwrap();
        
        // Create invalid .wasm file
        let wasm_path = ext_dir.join("test.wasm");
        std::fs::write(&wasm_path, b"invalid wasm bytes").unwrap();
        
        // Loading should fail
        let result = manager.load_wasm_extension(&wasm_path);
        assert!(result.is_err());
        
        // No extension should be registered
        assert_eq!(manager.wasm_extension_count(), 0);
    }

    #[test]
    fn test_extension_manager_unload_wasm_extension_not_found() {
        let mut manager = ExtensionManager::new();
        
        // Unloading non-existent extension should fail
        let result = manager.unload_wasm_extension("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_extension_manager_load_all_wasm_extensions_empty() {
        let mut manager = ExtensionManager::new();
        let temp_dir = tempfile::tempdir().unwrap();
        
        let results = manager.load_all_wasm_extensions(temp_dir.path());
        assert!(results.is_empty());
        assert_eq!(manager.wasm_extension_count(), 0);
    }

    #[tokio::test]
    async fn test_extension_manager_activate_deactivate_includes_wasm() {
        let mut manager = ExtensionManager::new();
        let temp_dir = tempfile::tempdir().unwrap();
        
        // Create extension directory
        let ext_dir = temp_dir.path().join("test-ext");
        std::fs::create_dir(&ext_dir).unwrap();
        
        // Create manifest.json
        let manifest_content = r#"{
            "name": "test-extension",
            "version": "1.0.0",
            "description": "Test extension"
        }"#;
        std::fs::write(ext_dir.join("manifest.json"), manifest_content).unwrap();
        std::fs::write(ext_dir.join("test.wasm"), b"invalid wasm").unwrap();
        
        // Manually add a WASM extension (simulating successful load)
        let manifest = super::super::types::WasmExtensionManifest::from_json(r#"{
            "name": "test-wasm",
            "version": "1.0.0",
            "description": "Test",
            "wasm_entry": "test.wasm"
        }"#).unwrap();
        
        let mut wasm_ext = super::super::types::WasmExtension::new(manifest, ext_dir.join("test.wasm"));
        let ext_id = wasm_ext.id.clone();
        wasm_ext.set_instance(super::super::types::WasmInstance::new(
            ext_id.clone(),
            wasm_ext.name.clone(),
            wasm_ext.version.clone(),
            wasm_ext.wasm_path.clone(),
        ));
        
        manager.wasm_extensions.insert(ext_id.clone(), wasm_ext);
        assert_eq!(manager.wasm_extension_count(), 1);
        
        // Activate all
        let config = AppConfig::default();
        let ctx = super::super::api::ExtensionContext::new(
            PathBuf::from("."),
            config,
            "test-session".to_string(),
            "test-ext",
        );
        
        let result = manager.activate_all(&ctx).await;
        assert!(result.is_ok());
        assert!(manager.is_activated());
        
        // Check WASM extension is activated
        let ext = manager.get_wasm_extension(&ext_id).unwrap();
        assert!(ext.is_active);
        
        // Deactivate all
        let result = manager.deactivate_all().await;
        assert!(result.is_ok());
        assert!(!manager.is_activated());
        
        // Check WASM extension is deactivated
        let ext = manager.get_wasm_extension(&ext_id).unwrap();
        assert!(!ext.is_active);
    }

    #[test]
    fn test_extension_manager_wasm_load_failure_isolation() {
        let mut manager = ExtensionManager::new();
        let temp_dir = tempfile::tempdir().unwrap();
        
        // Create first extension directory (valid structure but invalid WASM)
        let ext1_dir = temp_dir.path().join("ext1");
        std::fs::create_dir(&ext1_dir).unwrap();
        let manifest1 = r#"{"name": "ext1", "version": "1.0.0", "description": "Test 1", "wasm_entry": "ext1.wasm"}"#;
        std::fs::write(ext1_dir.join("manifest.json"), manifest1).unwrap();
        std::fs::write(ext1_dir.join("ext1.wasm"), b"invalid wasm").unwrap();
        
        // Create second extension directory (valid structure but invalid WASM)
        let ext2_dir = temp_dir.path().join("ext2");
        std::fs::create_dir(&ext2_dir).unwrap();
        let manifest2 = r#"{"name": "ext2", "version": "1.0.0", "description": "Test 2", "wasm_entry": "ext2.wasm"}"#;
        std::fs::write(ext2_dir.join("manifest.json"), manifest2).unwrap();
        std::fs::write(ext2_dir.join("ext2.wasm"), b"invalid wasm").unwrap();
        
        // Try to load all - both should fail due to invalid WASM
        let results = manager.load_all_wasm_extensions(temp_dir.path());
        
        // Both should be found but fail to load due to invalid WASM
        assert_eq!(results.len(), 2);
        for (_, result) in &results {
            assert!(result.is_err());
        }
        
        // No extensions should be loaded (because WASM compilation fails)
        assert_eq!(manager.wasm_extension_count(), 0);
    }

    // ==================== Hot Reload Tests ====================

    #[test]
    fn test_extension_manager_hot_reload_integration() {
        let mut manager = ExtensionManager::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let watch_path = temp_dir.path().join("extensions");
        std::fs::create_dir(&watch_path).unwrap();
        
        // 初始状态：未启用热重载
        assert!(!manager.is_hot_reloading());
        assert!(manager.hot_reload_status().is_none());
        
        // 启动热重载监控
        let result = manager.start_watching(watch_path.clone());
        assert!(result.is_ok());
        assert!(manager.is_hot_reloading());
        
        let status = manager.hot_reload_status().unwrap();
        assert!(status.watching);
        assert_eq!(status.watch_path, watch_path);
        
        // 停止热重载监控
        manager.stop_watching();
        assert!(!manager.is_hot_reloading());
        
        // 再次停止应该无问题（幂等）
        manager.stop_watching();
        assert!(!manager.is_hot_reloading());
    }

    #[test]
    fn test_extension_manager_hot_reload_process_events_empty() {
        let mut manager = ExtensionManager::new();
        
        // 未启动热重载时，处理事件应该返回空
        let events = manager.process_reload_events();
        assert!(events.is_empty());
    }

    #[test]
    fn test_extension_manager_hot_reload_start_stop_idempotent() {
        let mut manager = ExtensionManager::new();
        let temp_dir = tempfile::tempdir().unwrap();
        
        // 多次启动应该正常工作
        let result1 = manager.start_watching(temp_dir.path().to_path_buf());
        assert!(result1.is_ok());
        assert!(manager.is_hot_reloading());
        
        let result2 = manager.start_watching(temp_dir.path().to_path_buf());
        assert!(result2.is_ok()); // 应该成功（替换旧的 watcher）
        assert!(manager.is_hot_reloading());
        
        // 停止
        manager.stop_watching();
        assert!(!manager.is_hot_reloading());
    }
}
