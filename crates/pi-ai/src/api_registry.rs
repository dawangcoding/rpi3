//! API Provider 注册系统
//!
//! 提供全局的 Provider 注册表，用于管理不同 LLM 提供商的实现

use std::collections::HashMap;
use std::sync::{Arc, RwLock, OnceLock};
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::types::*;

/// API Provider trait - 所有 LLM 提供商实现此 trait
#[async_trait]
pub trait ApiProvider: Send + Sync {
    /// 返回此 provider 支持的 API 类型
    fn api(&self) -> Api;
    
    /// 流式调用 LLM
    async fn stream(
        &self,
        context: &Context,
        model: &Model,
        options: &StreamOptions,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>>;
}

/// API Provider 注册表
pub struct ApiRegistry {
    providers: HashMap<Api, Arc<dyn ApiProvider>>,
}

impl ApiRegistry {
    /// 创建新的注册表
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }
    
    /// 注册一个 provider
    pub fn register(&mut self, provider: Arc<dyn ApiProvider>) {
        let api = provider.api();
        self.providers.insert(api, provider);
    }
    
    /// 获取指定 API 类型的 provider
    pub fn get(&self, api: &Api) -> Option<Arc<dyn ApiProvider>> {
        self.providers.get(api).cloned()
    }
    
    /// 检查是否已注册指定 API 类型的 provider
    pub fn has(&self, api: &Api) -> bool {
        self.providers.contains_key(api)
    }
    
    /// 获取所有已注册的 provider
    pub fn get_all(&self) -> Vec<Arc<dyn ApiProvider>> {
        self.providers.values().cloned().collect()
    }
    
    /// 清除所有注册的 provider
    pub fn clear(&mut self) {
        self.providers.clear();
    }
}

impl Default for ApiRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// 全局注册表
static GLOBAL_REGISTRY: OnceLock<RwLock<ApiRegistry>> = OnceLock::new();

/// 获取全局注册表
fn get_global_registry() -> &'static RwLock<ApiRegistry> {
    GLOBAL_REGISTRY.get_or_init(|| RwLock::new(ApiRegistry::new()))
}

/// 注册一个 API provider 到全局注册表
pub fn register_api_provider(provider: Arc<dyn ApiProvider>) {
    let registry = get_global_registry();
    if let Ok(mut reg) = registry.write() {
        reg.register(provider);
    }
}

/// 从全局注册表获取指定 API 类型的 provider
pub fn get_api_provider(api: &Api) -> Option<Arc<dyn ApiProvider>> {
    let registry = get_global_registry();
    registry.read().ok().and_then(|reg| reg.get(api))
}

/// 检查全局注册表是否包含指定 API 类型的 provider
pub fn has_api_provider(api: &Api) -> bool {
    let registry = get_global_registry();
    registry.read().ok().map(|reg| reg.has(api)).unwrap_or(false)
}

/// 获取全局注册表中所有 provider
pub fn get_all_api_providers() -> Vec<Arc<dyn ApiProvider>> {
    let registry = get_global_registry();
    registry.read().ok().map(|reg| reg.get_all()).unwrap_or_default()
}

/// 清除全局注册表中的所有 provider
pub fn clear_api_providers() {
    let registry = get_global_registry();
    if let Ok(mut reg) = registry.write() {
        reg.clear();
    }
}

/// 解析 API provider，如果未找到则返回错误
pub fn resolve_api_provider(api: &Api) -> anyhow::Result<Arc<dyn ApiProvider>> {
    get_api_provider(api)
        .ok_or_else(|| anyhow::anyhow!("No API provider registered for api: {:?}", api))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use serial_test::serial;

    // 测试用的 Mock Provider
    struct MockProvider {
        api_type: Api,
    }

    impl MockProvider {
        fn new(api_type: Api) -> Self {
            Self { api_type }
        }
    }

    #[async_trait]
    impl ApiProvider for MockProvider {
        fn api(&self) -> Api {
            self.api_type.clone()
        }

        async fn stream(
            &self,
            _context: &Context,
            _model: &Model,
            _options: &StreamOptions,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<AssistantMessageEvent>> + Send>>> {
            Ok(Box::pin(stream::empty()))
        }
    }

    #[test]
    fn test_api_registry_new() {
        let registry = ApiRegistry::new();
        assert!(registry.get_all().is_empty());
    }

    #[test]
    fn test_api_registry_default() {
        let registry = ApiRegistry::default();
        assert!(registry.get_all().is_empty());
    }

    #[test]
    fn test_api_registry_register_and_get() {
        let mut registry = ApiRegistry::new();
        let provider = Arc::new(MockProvider::new(Api::Anthropic));
        
        registry.register(provider);
        
        assert!(registry.has(&Api::Anthropic));
        assert!(registry.get(&Api::Anthropic).is_some());
    }

    #[test]
    fn test_api_registry_get_nonexistent() {
        let registry = ApiRegistry::new();
        
        assert!(!registry.has(&Api::Anthropic));
        assert!(registry.get(&Api::Anthropic).is_none());
    }

    #[test]
    fn test_api_registry_register_replaces() {
        let mut registry = ApiRegistry::new();
        
        let provider1 = Arc::new(MockProvider::new(Api::Anthropic));
        let provider2 = Arc::new(MockProvider::new(Api::Anthropic));
        
        registry.register(provider1);
        registry.register(provider2);
        
        assert_eq!(registry.get_all().len(), 1);
    }

    #[test]
    fn test_api_registry_get_all() {
        let mut registry = ApiRegistry::new();
        
        registry.register(Arc::new(MockProvider::new(Api::Anthropic)));
        registry.register(Arc::new(MockProvider::new(Api::OpenAiChatCompletions)));
        registry.register(Arc::new(MockProvider::new(Api::Google)));
        
        let all = registry.get_all();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_api_registry_clear() {
        let mut registry = ApiRegistry::new();
        
        registry.register(Arc::new(MockProvider::new(Api::Anthropic)));
        registry.register(Arc::new(MockProvider::new(Api::OpenAiChatCompletions)));
        
        assert_eq!(registry.get_all().len(), 2);
        
        registry.clear();
        
        assert!(registry.get_all().is_empty());
        assert!(!registry.has(&Api::Anthropic));
    }

    #[test]
    fn test_api_registry_multiple_apis() {
        let mut registry = ApiRegistry::new();
        
        registry.register(Arc::new(MockProvider::new(Api::Anthropic)));
        registry.register(Arc::new(MockProvider::new(Api::OpenAiChatCompletions)));
        registry.register(Arc::new(MockProvider::new(Api::Google)));
        registry.register(Arc::new(MockProvider::new(Api::Mistral)));
        registry.register(Arc::new(MockProvider::new(Api::Groq)));
        
        assert!(registry.has(&Api::Anthropic));
        assert!(registry.has(&Api::OpenAiChatCompletions));
        assert!(registry.has(&Api::Google));
        assert!(registry.has(&Api::Mistral));
        assert!(registry.has(&Api::Groq));
        assert!(!registry.has(&Api::Xai));
    }

    #[test]
    #[serial]
    fn test_global_registry_functions() {
        clear_api_providers();
    
        register_api_provider(Arc::new(MockProvider::new(Api::Anthropic)));
    
        assert!(has_api_provider(&Api::Anthropic));
    
        let provider = get_api_provider(&Api::Anthropic);
        assert!(provider.is_some());
    
        clear_api_providers();
        assert!(!has_api_provider(&Api::Anthropic));
    }

    #[test]
    #[serial]
    fn test_resolve_api_provider_success() {
        clear_api_providers();
    
        register_api_provider(Arc::new(MockProvider::new(Api::Anthropic)));
    
        let result = resolve_api_provider(&Api::Anthropic);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn test_resolve_api_provider_not_found() {
        clear_api_providers();
    
        let result = resolve_api_provider(&Api::Other("nonexistent-api-for-test".to_string()));
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("No API provider registered"));
    }

    #[test]
    fn test_api_provider_trait() {
        let provider = MockProvider::new(Api::Anthropic);
        assert_eq!(provider.api(), Api::Anthropic);
    }
}