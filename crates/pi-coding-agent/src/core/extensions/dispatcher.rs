//! 事件分发器
//!
//! 负责将事件按优先级分发给已注册的扩展处理器

use super::events::{
    EventHandlerRegistry, EventSubscription, DEFAULT_EVENT_TIMEOUT,
};
use super::types::{EventResult, Extension};
use pi_agent::types::AgentEvent;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// 事件分发结果
///
/// 记录事件分发的完整结果信息
#[derive(Debug, Clone, Default)]
pub struct DispatchResult {
    /// 各处理器的结果列表
    pub results: Vec<(String, EventResult)>,
    /// 事件是否被阻止（任一处理器返回 Block）
    pub blocked: bool,
    /// 阻止原因（如果有处理器返回 Block）
    pub block_reason: Option<String>,
    /// 最终修改后的数据（如果有 Modified 结果）
    pub modified_data: Option<serde_json::Value>,
    /// 是否因 StopPropagation 提前终止
    pub propagation_stopped: bool,
}

impl DispatchResult {
    /// 创建新的分发结果
    pub fn new() -> Self {
        Self::default()
    }

    /// 检查是否有处理器处理了事件
    pub fn has_results(&self) -> bool {
        !self.results.is_empty()
    }

    /// 获取处理器数量
    pub fn handler_count(&self) -> usize {
        self.results.len()
    }
}

/// 扩展查找器 trait
///
/// 用于在分发时查找扩展实例
pub trait ExtensionFinder: Send + Sync {
    /// 根据名称查找扩展
    fn find_extension(&self, name: &str) -> Option<Arc<dyn Extension>>;
}

/// 扩展名称到扩展实例的映射查找器
pub struct ExtensionMapFinder {
    extensions: HashMap<String, Arc<dyn Extension>>,
}

impl ExtensionMapFinder {
    /// 创建新的映射查找器
    pub fn new(extensions: HashMap<String, Arc<dyn Extension>>) -> Self {
        Self { extensions }
    }
}

impl ExtensionFinder for ExtensionMapFinder {
    fn find_extension(&self, name: &str) -> Option<Arc<dyn Extension>> {
        self.extensions.get(name).cloned()
    }
}

/// 事件分发器
///
/// 负责将事件分发给已注册的处理器，支持：
/// - 按优先级顺序分发
/// - 超时保护
/// - 事件传播控制
/// - 事件修改
pub struct EventDispatcher {
    /// 处理器注册表
    handler_registry: EventHandlerRegistry,
    /// 处理超时时间
    timeout: Duration,
}

impl Default for EventDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl EventDispatcher {
    /// 创建新的事件分发器
    pub fn new() -> Self {
        Self {
            handler_registry: EventHandlerRegistry::new(),
            timeout: DEFAULT_EVENT_TIMEOUT,
        }
    }

    /// 创建指定超时时间的事件分发器
    ///
    /// # 参数
    /// - `timeout`: 单个处理器的最大执行时间
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            handler_registry: EventHandlerRegistry::new(),
            timeout,
        }
    }

    /// 注册扩展的事件订阅
    ///
    /// # 参数
    /// - `name`: 扩展名称
    /// - `subscriptions`: 订阅配置列表
    pub fn register_extension(&mut self, name: String, subscriptions: Vec<EventSubscription>) {
        self.handler_registry.register(name, subscriptions);
    }

    /// 注销扩展的事件订阅
    ///
    /// # 参数
    /// - `name`: 扩展名称
    pub fn unregister_extension(&mut self, name: &str) {
        self.handler_registry.unregister(name);
    }

    /// 获取处理器注册表（只读）
    pub fn registry(&self) -> &EventHandlerRegistry {
        &self.handler_registry
    }

    /// 分发事件给所有匹配的处理器
    ///
    /// 按优先级从高到低依次调用处理器：
    /// - 如果返回 `Continue`：继续下一个处理器
    /// - 如果返回 `Block(reason)`：标记 blocked 并继续（不立即终止）
    /// - 如果返回 `Modified(data)`：记录修改数据并继续
    /// - 单个处理器出错时记录日志并继续
    /// - 每个处理器调用都有超时保护
    ///
    /// # 参数
    /// - `event`: 要分发的事件
    /// - `finder`: 扩展查找器
    ///
    /// # 返回
    /// 分发结果
    pub async fn dispatch(&self, event: &AgentEvent, finder: &dyn ExtensionFinder) -> DispatchResult {
        let mut result = DispatchResult::new();
        
        // 获取匹配的处理器（已按优先级排序）
        let handlers = self.handler_registry.get_handlers_for_event(event);

        if handlers.is_empty() {
            return result;
        }

        // 记录已处理的扩展，避免重复调用
        let mut processed_extensions: std::collections::HashSet<&str> = std::collections::HashSet::new();

        for record in handlers {
            // 跳过已处理的扩展（一个扩展可能注册多个订阅）
            if processed_extensions.contains(record.extension_name.as_str()) {
                continue;
            }
            processed_extensions.insert(&record.extension_name);

            // 查找扩展实例
            let extension = match finder.find_extension(&record.extension_name) {
                Some(ext) => ext,
                None => {
                    tracing::warn!(
                        "Extension '{}' not found for event dispatch",
                        record.extension_name
                    );
                    continue;
                }
            };

            // 带超时保护调用处理器
            let timeout_result = tokio::time::timeout(
                self.timeout,
                extension.on_event(event)
            ).await;

            match timeout_result {
                Ok(handler_result) => {
                    match handler_result {
                        Ok(event_result) => {
                            // 处理 StopPropagation
                            if matches!(&event_result, EventResult::StopPropagation) {
                                result.propagation_stopped = true;
                                result.results.push((record.extension_name.clone(), event_result));
                                break;
                            }

                            // 处理 Block 结果
                            if let EventResult::Block(reason) = &event_result {
                                result.blocked = true;
                                result.block_reason = Some(reason.clone());
                            }

                            // 处理 Modified 结果
                            if let EventResult::Modified(data) = &event_result {
                                result.modified_data = Some(data.clone());
                            }

                            result.results.push((record.extension_name.clone(), event_result));
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Extension '{}' event handler error: {}",
                                record.extension_name,
                                e
                            );
                            // 出错时继续处理下一个处理器
                        }
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        "Extension '{}' event handler timed out after {:?}",
                        record.extension_name,
                        self.timeout
                    );
                    // 超时时继续处理下一个处理器
                }
            }
        }

        result
    }

    /// 快速检查是否有处理器订阅了指定事件
    ///
    /// 这是一个快速路径检查，用于避免不必要的分发开销
    ///
    /// # 参数
    /// - `event`: 要检查的事件
    ///
    /// # 返回
    /// 如果有处理器订阅了此事件则返回 true
    pub fn has_handlers_for(&self, event: &AgentEvent) -> bool {
        self.handler_registry.has_handlers_for(event)
    }

    /// 获取已注册扩展数量
    #[allow(dead_code)]
    pub fn extension_count(&self) -> usize {
        self.handler_registry.extension_count()
    }

    /// 获取当前超时时间
    #[allow(dead_code)]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::extensions::types::ExtensionManifest;
    use async_trait::async_trait;
    use std::path::PathBuf;

    // ==================== Mock Extension ====================

    struct MockExtension {
        manifest: ExtensionManifest,
        result: EventResult,
        should_error: bool,
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
                result: EventResult::Continue,
                should_error: false,
            }
        }

        fn with_result(name: &str, result: EventResult) -> Self {
            Self {
                manifest: ExtensionManifest {
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    description: "Mock extension for testing".to_string(),
                    author: "test".to_string(),
                    entry_point: PathBuf::new(),
                },
                result,
                should_error: false,
            }
        }

        fn with_error(name: &str) -> Self {
            Self {
                manifest: ExtensionManifest {
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    description: "Mock extension for testing".to_string(),
                    author: "test".to_string(),
                    entry_point: PathBuf::new(),
                },
                result: EventResult::Continue,
                should_error: true,
            }
        }
    }

    #[async_trait]
    impl Extension for MockExtension {
        fn manifest(&self) -> &ExtensionManifest {
            &self.manifest
        }

        async fn activate(
            &mut self,
            _ctx: &super::super::api::ExtensionContext,
        ) -> anyhow::Result<()> {
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

        async fn on_event(&self, _event: &AgentEvent) -> anyhow::Result<EventResult> {
            if self.should_error {
                Err(anyhow::anyhow!("Mock error"))
            } else {
                Ok(self.result.clone())
            }
        }
    }

    // ==================== DispatchResult Tests ====================

    #[test]
    fn test_dispatch_result_default() {
        let result = DispatchResult::default();
        assert!(result.results.is_empty());
        assert!(!result.blocked);
        assert!(result.block_reason.is_none());
        assert!(result.modified_data.is_none());
        assert!(!result.propagation_stopped);
    }

    #[test]
    fn test_dispatch_result_new() {
        let result = DispatchResult::new();
        assert!(result.results.is_empty());
    }

    #[test]
    fn test_dispatch_result_has_results() {
        let mut result = DispatchResult::new();
        assert!(!result.has_results());

        result.results.push(("ext1".to_string(), EventResult::Continue));
        assert!(result.has_results());
    }

    #[test]
    fn test_dispatch_result_handler_count() {
        let mut result = DispatchResult::new();
        assert_eq!(result.handler_count(), 0);

        result.results.push(("ext1".to_string(), EventResult::Continue));
        result.results.push(("ext2".to_string(), EventResult::Continue));
        assert_eq!(result.handler_count(), 2);
    }

    // ==================== EventDispatcher Tests ====================

    #[test]
    fn test_event_dispatcher_new() {
        let dispatcher = EventDispatcher::new();
        assert_eq!(dispatcher.extension_count(), 0);
        assert_eq!(dispatcher.timeout(), DEFAULT_EVENT_TIMEOUT);
    }

    #[test]
    fn test_event_dispatcher_default() {
        let dispatcher = EventDispatcher::default();
        assert_eq!(dispatcher.extension_count(), 0);
    }

    #[test]
    fn test_event_dispatcher_with_timeout() {
        let timeout = Duration::from_secs(10);
        let dispatcher = EventDispatcher::with_timeout(timeout);
        assert_eq!(dispatcher.timeout(), timeout);
    }

    #[test]
    fn test_event_dispatcher_register_extension() {
        let mut dispatcher = EventDispatcher::new();
        
        let subscriptions = vec![EventSubscription::all()];
        dispatcher.register_extension("ext1".to_string(), subscriptions);
        
        assert_eq!(dispatcher.extension_count(), 1);
    }

    #[test]
    fn test_event_dispatcher_unregister_extension() {
        let mut dispatcher = EventDispatcher::new();
        
        let subscriptions = vec![EventSubscription::all()];
        dispatcher.register_extension("ext1".to_string(), subscriptions);
        assert_eq!(dispatcher.extension_count(), 1);
        
        dispatcher.unregister_extension("ext1");
        assert_eq!(dispatcher.extension_count(), 0);
    }

    #[test]
    fn test_event_dispatcher_has_handlers_for() {
        let mut dispatcher = EventDispatcher::new();
        
        let event = AgentEvent::AgentStart;
        assert!(!dispatcher.has_handlers_for(&event));
        
        let subscriptions = vec![EventSubscription::with_filter(
            super::super::events::EventTypeFilter::AgentLifecycle
        )];
        dispatcher.register_extension("ext1".to_string(), subscriptions);
        
        assert!(dispatcher.has_handlers_for(&event));
        assert!(!dispatcher.has_handlers_for(&AgentEvent::TurnStart));
    }

    #[tokio::test]
    async fn test_event_dispatcher_dispatch_no_handlers() {
        let dispatcher = EventDispatcher::new();
        
        let extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        let finder = ExtensionMapFinder::new(extensions);
        
        let event = AgentEvent::AgentStart;
        let result = dispatcher.dispatch(&event, &finder).await;
        
        assert!(!result.has_results());
        assert!(!result.blocked);
    }

    #[tokio::test]
    async fn test_event_dispatcher_dispatch_continue() {
        let mut dispatcher = EventDispatcher::new();
        
        let subscriptions = vec![EventSubscription::all()];
        dispatcher.register_extension("ext1".to_string(), subscriptions);
        
        let mut extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        extensions.insert("ext1".to_string(), Arc::new(MockExtension::new("ext1")));
        let finder = ExtensionMapFinder::new(extensions);
        
        let event = AgentEvent::AgentStart;
        let result = dispatcher.dispatch(&event, &finder).await;
        
        assert_eq!(result.handler_count(), 1);
        assert!(!result.blocked);
        assert!(matches!(result.results[0].1, EventResult::Continue));
    }

    #[tokio::test]
    async fn test_event_dispatcher_dispatch_block() {
        let mut dispatcher = EventDispatcher::new();
        
        let subscriptions = vec![EventSubscription::all()];
        dispatcher.register_extension("ext1".to_string(), subscriptions);
        
        let mut extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        extensions.insert(
            "ext1".to_string(),
            Arc::new(MockExtension::with_result("ext1", EventResult::Block("blocked".to_string()))),
        );
        let finder = ExtensionMapFinder::new(extensions);
        
        let event = AgentEvent::AgentStart;
        let result = dispatcher.dispatch(&event, &finder).await;
        
        assert!(result.blocked);
        assert_eq!(result.block_reason, Some("blocked".to_string()));
    }

    #[tokio::test]
    async fn test_event_dispatcher_dispatch_modified() {
        let mut dispatcher = EventDispatcher::new();
        
        let subscriptions = vec![EventSubscription::all()];
        dispatcher.register_extension("ext1".to_string(), subscriptions);
        
        let modified_data = serde_json::json!({"key": "value"});
        let mut extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        extensions.insert(
            "ext1".to_string(),
            Arc::new(MockExtension::with_result("ext1", EventResult::Modified(modified_data.clone()))),
        );
        let finder = ExtensionMapFinder::new(extensions);
        
        let event = AgentEvent::AgentStart;
        let result = dispatcher.dispatch(&event, &finder).await;
        
        assert_eq!(result.modified_data, Some(modified_data));
    }

    #[tokio::test]
    async fn test_event_dispatcher_dispatch_priority_order() {
        let mut dispatcher = EventDispatcher::new();
        
        // 注册低优先级扩展
        dispatcher.register_extension(
            "ext-low".to_string(),
            vec![EventSubscription::new(
                super::super::events::EventTypeFilter::All,
                super::super::events::EventPriority::Low,
            )],
        );
        
        // 注册高优先级扩展
        dispatcher.register_extension(
            "ext-high".to_string(),
            vec![EventSubscription::new(
                super::super::events::EventTypeFilter::All,
                super::super::events::EventPriority::High,
            )],
        );
        
        // 注册普通优先级扩展
        dispatcher.register_extension(
            "ext-normal".to_string(),
            vec![EventSubscription::new(
                super::super::events::EventTypeFilter::All,
                super::super::events::EventPriority::Normal,
            )],
        );
        
        let mut extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        extensions.insert("ext-low".to_string(), Arc::new(MockExtension::new("ext-low")));
        extensions.insert("ext-high".to_string(), Arc::new(MockExtension::new("ext-high")));
        extensions.insert("ext-normal".to_string(), Arc::new(MockExtension::new("ext-normal")));
        let finder = ExtensionMapFinder::new(extensions);
        
        let event = AgentEvent::AgentStart;
        let result = dispatcher.dispatch(&event, &finder).await;
        
        assert_eq!(result.handler_count(), 3);
        
        // 验证按优先级排序执行
        assert_eq!(result.results[0].0, "ext-high");
        assert_eq!(result.results[1].0, "ext-normal");
        assert_eq!(result.results[2].0, "ext-low");
    }

    #[tokio::test]
    async fn test_event_dispatcher_dispatch_error_handling() {
        let mut dispatcher = EventDispatcher::new();
        
        let subscriptions = vec![EventSubscription::all()];
        dispatcher.register_extension("ext1".to_string(), subscriptions);
        
        let mut extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        extensions.insert("ext1".to_string(), Arc::new(MockExtension::with_error("ext1")));
        let finder = ExtensionMapFinder::new(extensions);
        
        let event = AgentEvent::AgentStart;
        let result = dispatcher.dispatch(&event, &finder).await;
        
        // 错误时不应有结果（但也不应 panic）
        assert_eq!(result.handler_count(), 0);
    }

    #[tokio::test]
    async fn test_event_dispatcher_dispatch_extension_not_found() {
        let mut dispatcher = EventDispatcher::new();
        
        let subscriptions = vec![EventSubscription::all()];
        dispatcher.register_extension("nonexistent".to_string(), subscriptions);
        
        let extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        let finder = ExtensionMapFinder::new(extensions);
        
        let event = AgentEvent::AgentStart;
        let result = dispatcher.dispatch(&event, &finder).await;
        
        // 找不到扩展时不应有结果
        assert_eq!(result.handler_count(), 0);
    }

    #[tokio::test]
    async fn test_event_dispatcher_dispatch_filter_mismatch() {
        let mut dispatcher = EventDispatcher::new();
        
        // 只订阅命令事件
        let subscriptions = vec![EventSubscription::with_filter(
            super::super::events::EventTypeFilter::Command
        )];
        dispatcher.register_extension("ext1".to_string(), subscriptions);
        
        let mut extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        extensions.insert("ext1".to_string(), Arc::new(MockExtension::new("ext1")));
        let finder = ExtensionMapFinder::new(extensions);
        
        // 发送 AgentStart 事件（非命令事件）
        let event = AgentEvent::AgentStart;
        let result = dispatcher.dispatch(&event, &finder).await;
        
        // 过滤器不匹配，不应调用处理器
        assert_eq!(result.handler_count(), 0);
    }

    #[tokio::test]
    async fn test_dispatch_stop_propagation() {
        let mut dispatcher = EventDispatcher::new();
        
        // 注册返回 StopPropagation 的扩展（高优先级）
        dispatcher.register_extension(
            "ext-stopper".to_string(),
            vec![EventSubscription::new(
                super::super::events::EventTypeFilter::All,
                super::super::events::EventPriority::High,
            )],
        );
        
        // 注册普通扩展（低优先级）
        dispatcher.register_extension(
            "ext-normal".to_string(),
            vec![EventSubscription::new(
                super::super::events::EventTypeFilter::All,
                super::super::events::EventPriority::Low,
            )],
        );
        
        let mut extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        extensions.insert(
            "ext-stopper".to_string(),
            Arc::new(MockExtension::with_result("ext-stopper", EventResult::StopPropagation)),
        );
        extensions.insert(
            "ext-normal".to_string(),
            Arc::new(MockExtension::new("ext-normal")),
        );
        let finder = ExtensionMapFinder::new(extensions);
        
        let event = AgentEvent::AgentStart;
        let result = dispatcher.dispatch(&event, &finder).await;
        
        // 验证 StopPropagation 后，只有 ext-stopper 被调用
        assert_eq!(result.handler_count(), 1);
        assert!(result.propagation_stopped);
        assert_eq!(result.results[0].0, "ext-stopper");
        assert!(matches!(result.results[0].1, EventResult::StopPropagation));
    }

    // ==================== ExtensionMapFinder Tests ====================

    #[test]
    fn test_extension_map_finder_new() {
        let extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        let finder = ExtensionMapFinder::new(extensions);
        assert!(finder.find_extension("any").is_none());
    }

    #[test]
    fn test_extension_map_finder_find() {
        let mut extensions: HashMap<String, Arc<dyn Extension>> = HashMap::new();
        let ext = Arc::new(MockExtension::new("test-ext"));
        extensions.insert("test-ext".to_string(), ext);
        
        let finder = ExtensionMapFinder::new(extensions);
        
        assert!(finder.find_extension("test-ext").is_some());
        assert!(finder.find_extension("nonexistent").is_none());
    }
}
