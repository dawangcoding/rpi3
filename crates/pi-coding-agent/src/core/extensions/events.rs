//! 事件优先级和订阅管理
//!
//! 定义事件处理器优先级、事件类型过滤器和处理器注册表

use pi_agent::types::AgentEvent;
use std::collections::HashMap;
use std::time::Duration;

/// 默认事件处理超时时间（5秒）
pub const DEFAULT_EVENT_TIMEOUT: Duration = Duration::from_secs(5);

/// 事件优先级
///
/// 数值越小优先级越高，处理器按优先级从高到低依次执行
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventPriority {
    /// 高优先级（最先执行）
    High = 0,
    /// 普通优先级（默认）
    Normal = 50,
    /// 低优先级（最后执行）
    Low = 100,
}

impl Default for EventPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// 事件类型过滤器
///
/// 用于扩展订阅特定类型的事件
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventTypeFilter {
    /// 订阅所有事件
    All,
    /// Agent 生命周期事件（BeforeAgentStart, AgentStart, AgentEnd）
    AgentLifecycle,
    /// Turn 生命周期事件（TurnStart, TurnEnd）
    TurnLifecycle,
    /// 消息事件（MessageStart, MessageUpdate, MessageEnd）
    Message,
    /// 工具执行事件（BeforeToolCall, ToolExecution*, AfterToolCall）
    ToolExecution,
    /// 命令事件（BeforeCommandExecute, AfterCommandExecute）
    Command,
    /// 扩展事件（ExtensionLoaded, ExtensionError）
    Extension,
    /// 特定事件名称匹配
    Specific(String),
}

impl EventTypeFilter {
    /// 检查事件是否匹配此过滤器
    ///
    /// # 参数
    /// - `event`: 要检查的 Agent 事件
    ///
    /// # 返回
    /// 如果事件匹配过滤器则返回 true
    pub fn matches(&self, event: &AgentEvent) -> bool {
        match self {
            EventTypeFilter::All => true,
            EventTypeFilter::AgentLifecycle => matches!(
                event,
                AgentEvent::BeforeAgentStart
                    | AgentEvent::AgentStart
                    | AgentEvent::AgentEnd { .. }
                    | AgentEvent::BeforeAgentEnd { .. }
                    | AgentEvent::ContextWarning { .. }
            ),
            EventTypeFilter::TurnLifecycle => matches!(
                event,
                AgentEvent::TurnStart | AgentEvent::TurnEnd { .. } | AgentEvent::TurnError { .. }
            ),
            EventTypeFilter::Message => matches!(
                event,
                AgentEvent::MessageStart { .. }
                    | AgentEvent::MessageUpdate { .. }
                    | AgentEvent::MessageEnd { .. }
                    | AgentEvent::MessageChunk { .. }
                    | AgentEvent::MessageError { .. }
            ),
            EventTypeFilter::ToolExecution => matches!(
                event,
                AgentEvent::BeforeToolCall { .. }
                    | AgentEvent::ToolExecutionStart { .. }
                    | AgentEvent::ToolExecutionUpdate { .. }
                    | AgentEvent::ToolExecutionEnd { .. }
                    | AgentEvent::AfterToolCall { .. }
                    | AgentEvent::ToolError { .. }
            ),
            EventTypeFilter::Command => matches!(
                event,
                AgentEvent::BeforeCommandExecute { .. }
                    | AgentEvent::AfterCommandExecute { .. }
                    | AgentEvent::CommandError { .. }
            ),
            EventTypeFilter::Extension => matches!(
                event,
                AgentEvent::ExtensionLoaded { .. } | AgentEvent::ExtensionError { .. }
            ),
            EventTypeFilter::Specific(name) => {
                let event_name = match event {
                    AgentEvent::BeforeAgentStart => "BeforeAgentStart",
                    AgentEvent::AgentStart => "AgentStart",
                    AgentEvent::AgentEnd { .. } => "AgentEnd",
                    AgentEvent::BeforeAgentEnd { .. } => "BeforeAgentEnd",
                    AgentEvent::ContextWarning { .. } => "ContextWarning",
                    AgentEvent::TurnStart => "TurnStart",
                    AgentEvent::TurnEnd { .. } => "TurnEnd",
                    AgentEvent::TurnError { .. } => "TurnError",
                    AgentEvent::MessageStart { .. } => "MessageStart",
                    AgentEvent::MessageUpdate { .. } => "MessageUpdate",
                    AgentEvent::MessageEnd { .. } => "MessageEnd",
                    AgentEvent::MessageChunk { .. } => "MessageChunk",
                    AgentEvent::MessageError { .. } => "MessageError",
                    AgentEvent::BeforeToolCall { .. } => "BeforeToolCall",
                    AgentEvent::ToolExecutionStart { .. } => "ToolExecutionStart",
                    AgentEvent::ToolExecutionUpdate { .. } => "ToolExecutionUpdate",
                    AgentEvent::ToolExecutionEnd { .. } => "ToolExecutionEnd",
                    AgentEvent::AfterToolCall { .. } => "AfterToolCall",
                    AgentEvent::ToolError { .. } => "ToolError",
                    AgentEvent::BeforeCommandExecute { .. } => "BeforeCommandExecute",
                    AgentEvent::AfterCommandExecute { .. } => "AfterCommandExecute",
                    AgentEvent::CommandError { .. } => "CommandError",
                    AgentEvent::ExtensionLoaded { .. } => "ExtensionLoaded",
                    AgentEvent::ExtensionError { .. } => "ExtensionError",
                };
                event_name == name
            }
        }
    }
}

impl Default for EventTypeFilter {
    fn default() -> Self {
        Self::All
    }
}

/// 事件订阅配置
///
/// 定义扩展对事件的订阅方式和优先级
#[derive(Debug, Clone)]
pub struct EventSubscription {
    /// 事件类型过滤器
    pub filter: EventTypeFilter,
    /// 处理优先级
    pub priority: EventPriority,
}

impl Default for EventSubscription {
    fn default() -> Self {
        Self {
            filter: EventTypeFilter::default(),
            priority: EventPriority::default(),
        }
    }
}

impl EventSubscription {
    /// 创建新的订阅配置
    ///
    /// # 参数
    /// - `filter`: 事件类型过滤器
    /// - `priority`: 处理优先级
    #[allow(dead_code)]
    pub fn new(filter: EventTypeFilter, priority: EventPriority) -> Self {
        Self { filter, priority }
    }

    /// 创建订阅所有事件的配置
    #[allow(dead_code)]
    pub fn all() -> Self {
        Self {
            filter: EventTypeFilter::All,
            priority: EventPriority::Normal,
        }
    }

    /// 创建订阅特定类型事件的配置
    #[allow(dead_code)]
    pub fn with_filter(filter: EventTypeFilter) -> Self {
        Self {
            filter,
            priority: EventPriority::Normal,
        }
    }

    /// 设置优先级
    #[allow(dead_code)]
    pub fn with_priority(mut self, priority: EventPriority) -> Self {
        self.priority = priority;
        self
    }
}

/// 事件处理器记录
///
/// 存储已注册的事件处理器信息
pub struct EventHandlerRecord {
    /// 扩展名称
    pub extension_name: String,
    /// 处理优先级
    pub priority: EventPriority,
    /// 事件过滤器
    pub filter: EventTypeFilter,
}

impl std::fmt::Debug for EventHandlerRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventHandlerRecord")
            .field("extension_name", &self.extension_name)
            .field("priority", &self.priority)
            .field("filter", &self.filter)
            .finish()
    }
}

/// 事件处理器注册表
///
/// 管理所有扩展的事件处理器注册信息
#[derive(Default)]
pub struct EventHandlerRegistry {
    /// 扩展名称 -> 处理器记录列表
    handlers: HashMap<String, Vec<EventHandlerRecord>>,
}

impl EventHandlerRegistry {
    /// 创建新的处理器注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册扩展的事件处理器
    ///
    /// # 参数
    /// - `extension_name`: 扩展名称
    /// - `subscriptions`: 订阅配置列表
    pub fn register(&mut self, extension_name: String, subscriptions: Vec<EventSubscription>) {
        let records: Vec<EventHandlerRecord> = subscriptions
            .into_iter()
            .map(|sub| EventHandlerRecord {
                extension_name: extension_name.clone(),
                priority: sub.priority,
                filter: sub.filter,
            })
            .collect();

        self.handlers.insert(extension_name, records);
    }

    /// 注销扩展的所有事件处理器
    ///
    /// # 参数
    /// - `extension_name`: 扩展名称
    pub fn unregister(&mut self, extension_name: &str) {
        self.handlers.remove(extension_name);
    }

    /// 获取匹配指定事件的所有处理器记录
    ///
    /// 返回的列表按优先级排序（优先级高的在前）
    ///
    /// # 参数
    /// - `event`: 要匹配的事件
    ///
    /// # 返回
    /// 按优先级排序的处理器记录列表
    pub fn get_handlers_for_event(&self, event: &AgentEvent) -> Vec<&EventHandlerRecord> {
        let mut matching_handlers: Vec<&EventHandlerRecord> = self
            .handlers
            .values()
            .flat_map(|records| records.iter())
            .filter(|record| record.filter.matches(event))
            .collect();

        // 按优先级排序（数值小的优先级高，排在前面）
        matching_handlers.sort_by_key(|record| record.priority);
        matching_handlers
    }

    /// 检查是否有处理器订阅了指定事件
    ///
    /// 这是一个快速路径检查，用于避免不必要的分发开销
    ///
    /// # 参数
    /// - `event`: 要检查的事件
    ///
    /// # 返回
    /// 如果有处理器订阅了此事件则返回 true
    pub fn has_handlers_for(&self, event: &AgentEvent) -> bool {
        self.handlers
            .values()
            .flat_map(|records| records.iter())
            .any(|record| record.filter.matches(event))
    }

    /// 获取已注册扩展数量
    #[allow(dead_code)]
    pub fn extension_count(&self) -> usize {
        self.handlers.len()
    }

    /// 获取指定扩展的处理器数量
    #[allow(dead_code)]
    pub fn handler_count(&self, extension_name: &str) -> usize {
        self.handlers
            .get(extension_name)
            .map(|records| records.len())
            .unwrap_or(0)
    }

    /// 清空所有注册
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.handlers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== EventPriority Tests ====================

    #[test]
    fn test_event_priority_default() {
        let priority = EventPriority::default();
        assert_eq!(priority, EventPriority::Normal);
    }

    #[test]
    fn test_event_priority_ordering() {
        // 数值越小优先级越高
        assert!(EventPriority::High < EventPriority::Normal);
        assert!(EventPriority::Normal < EventPriority::Low);
        assert!(EventPriority::High < EventPriority::Low);
    }

    #[test]
    fn test_event_priority_values() {
        assert_eq!(EventPriority::High as i32, 0);
        assert_eq!(EventPriority::Normal as i32, 50);
        assert_eq!(EventPriority::Low as i32, 100);
    }

    // ==================== EventTypeFilter Tests ====================

    #[test]
    fn test_event_type_filter_default() {
        let filter = EventTypeFilter::default();
        assert_eq!(filter, EventTypeFilter::All);
    }

    #[test]
    fn test_event_type_filter_matches_all() {
        let filter = EventTypeFilter::All;

        // All 应该匹配所有事件
        assert!(filter.matches(&AgentEvent::AgentStart));
        assert!(filter.matches(&AgentEvent::TurnStart));
        assert!(filter.matches(&AgentEvent::MessageStart { message: pi_agent::types::AgentMessage::user("test") }));
        assert!(filter.matches(&AgentEvent::BeforeToolCall {
            tool_call_id: "1".to_string(),
            tool_name: "test".to_string(),
            args: serde_json::Value::Null,
        }));
        assert!(filter.matches(&AgentEvent::BeforeCommandExecute {
            command: "test".to_string(),
            args: "".to_string(),
        }));
        assert!(filter.matches(&AgentEvent::ExtensionLoaded {
            name: "test".to_string(),
            version: "1.0".to_string(),
        }));
    }

    #[test]
    fn test_event_type_filter_matches_agent_lifecycle() {
        let filter = EventTypeFilter::AgentLifecycle;

        // Agent 生命周期事件
        assert!(filter.matches(&AgentEvent::BeforeAgentStart));
        assert!(filter.matches(&AgentEvent::AgentStart));
        assert!(filter.matches(&AgentEvent::AgentEnd { messages: vec![] }));
        assert!(filter.matches(&AgentEvent::ContextWarning {
            usage_percent: 50.0,
            total_tokens: 1000,
            context_window: 2000,
        }));

        // 非生命周期事件
        assert!(!filter.matches(&AgentEvent::TurnStart));
        assert!(!filter.matches(&AgentEvent::MessageStart { message: pi_agent::types::AgentMessage::user("test") }));
    }

    #[test]
    fn test_event_type_filter_matches_turn_lifecycle() {
        let filter = EventTypeFilter::TurnLifecycle;

        // Turn 生命周期事件
        assert!(filter.matches(&AgentEvent::TurnStart));
        assert!(filter.matches(&AgentEvent::TurnEnd {
            message: pi_agent::types::AgentMessage::user("test"),
            tool_results: vec![],
        }));

        // 非 Turn 事件
        assert!(!filter.matches(&AgentEvent::AgentStart));
        assert!(!filter.matches(&AgentEvent::MessageStart { message: pi_agent::types::AgentMessage::user("test") }));
    }

    #[test]
    fn test_event_type_filter_matches_message() {
        use pi_ai::types::AssistantMessage;
        
        let filter = EventTypeFilter::Message;

        // 消息事件
        let msg = pi_agent::types::AgentMessage::user("test");
        assert!(filter.matches(&AgentEvent::MessageStart { message: msg.clone() }));
        
        // MessageUpdate 事件测试
        let partial = AssistantMessage::default();
        let event = pi_ai::types::AssistantMessageEvent::TextDelta {
            content_index: 0,
            delta: "test".to_string(),
            partial,
        };
        assert!(filter.matches(&AgentEvent::MessageUpdate {
            message: msg.clone(),
            event,
        }));
        assert!(filter.matches(&AgentEvent::MessageEnd { message: msg }));

        // 非消息事件
        assert!(!filter.matches(&AgentEvent::AgentStart));
        assert!(!filter.matches(&AgentEvent::TurnStart));
    }

    #[test]
    fn test_event_type_filter_matches_tool_execution() {
        let filter = EventTypeFilter::ToolExecution;

        // 工具执行事件
        assert!(filter.matches(&AgentEvent::BeforeToolCall {
            tool_call_id: "1".to_string(),
            tool_name: "test".to_string(),
            args: serde_json::Value::Null,
        }));
        assert!(filter.matches(&AgentEvent::ToolExecutionStart {
            tool_call_id: "1".to_string(),
            tool_name: "test".to_string(),
            args: serde_json::Value::Null,
        }));
        assert!(filter.matches(&AgentEvent::ToolExecutionUpdate {
            tool_call_id: "1".to_string(),
            tool_name: "test".to_string(),
            args: serde_json::Value::Null,
            partial_result: pi_agent::types::AgentToolResult {
                content: vec![],
                details: serde_json::Value::Null,
            },
        }));
        assert!(filter.matches(&AgentEvent::ToolExecutionEnd {
            tool_call_id: "1".to_string(),
            tool_name: "test".to_string(),
            result: pi_agent::types::AgentToolResult {
                content: vec![],
                details: serde_json::Value::Null,
            },
            is_error: false,
        }));
        assert!(filter.matches(&AgentEvent::AfterToolCall {
            tool_call_id: "1".to_string(),
            tool_name: "test".to_string(),
            result: pi_agent::types::AgentToolResult {
                content: vec![],
                details: serde_json::Value::Null,
            },
            is_error: false,
        }));

        // 非工具事件
        assert!(!filter.matches(&AgentEvent::AgentStart));
        assert!(!filter.matches(&AgentEvent::BeforeCommandExecute {
            command: "test".to_string(),
            args: "".to_string(),
        }));
    }

    #[test]
    fn test_event_type_filter_matches_command() {
        let filter = EventTypeFilter::Command;

        // 命令事件
        assert!(filter.matches(&AgentEvent::BeforeCommandExecute {
            command: "test".to_string(),
            args: "".to_string(),
        }));
        assert!(filter.matches(&AgentEvent::AfterCommandExecute {
            command: "test".to_string(),
            result: "ok".to_string(),
        }));

        // 非命令事件
        assert!(!filter.matches(&AgentEvent::AgentStart));
        assert!(!filter.matches(&AgentEvent::BeforeToolCall {
            tool_call_id: "1".to_string(),
            tool_name: "test".to_string(),
            args: serde_json::Value::Null,
        }));
    }

    #[test]
    fn test_event_type_filter_matches_extension() {
        let filter = EventTypeFilter::Extension;

        // 扩展事件
        assert!(filter.matches(&AgentEvent::ExtensionLoaded {
            name: "test".to_string(),
            version: "1.0".to_string(),
        }));
        assert!(filter.matches(&AgentEvent::ExtensionError {
            name: "test".to_string(),
            error: "error".to_string(),
        }));

        // 非扩展事件
        assert!(!filter.matches(&AgentEvent::AgentStart));
        assert!(!filter.matches(&AgentEvent::TurnStart));
    }

    #[test]
    fn test_event_type_filter_matches_specific() {
        let filter = EventTypeFilter::Specific("AgentStart".to_string());

        // 匹配特定名称
        assert!(filter.matches(&AgentEvent::AgentStart));
        assert!(!filter.matches(&AgentEvent::AgentEnd { messages: vec![] }));
        assert!(!filter.matches(&AgentEvent::TurnStart));
    }

    // ==================== EventSubscription Tests ====================

    #[test]
    fn test_event_subscription_default() {
        let sub = EventSubscription::default();
        assert_eq!(sub.filter, EventTypeFilter::All);
        assert_eq!(sub.priority, EventPriority::Normal);
    }

    #[test]
    fn test_event_subscription_new() {
        let sub = EventSubscription::new(EventTypeFilter::Command, EventPriority::High);
        assert_eq!(sub.filter, EventTypeFilter::Command);
        assert_eq!(sub.priority, EventPriority::High);
    }

    #[test]
    fn test_event_subscription_all() {
        let sub = EventSubscription::all();
        assert_eq!(sub.filter, EventTypeFilter::All);
        assert_eq!(sub.priority, EventPriority::Normal);
    }

    #[test]
    fn test_event_subscription_with_filter() {
        let sub = EventSubscription::with_filter(EventTypeFilter::ToolExecution);
        assert_eq!(sub.filter, EventTypeFilter::ToolExecution);
        assert_eq!(sub.priority, EventPriority::Normal);
    }

    #[test]
    fn test_event_subscription_with_priority() {
        let sub = EventSubscription::with_filter(EventTypeFilter::Message)
            .with_priority(EventPriority::High);
        assert_eq!(sub.filter, EventTypeFilter::Message);
        assert_eq!(sub.priority, EventPriority::High);
    }

    // ==================== EventHandlerRegistry Tests ====================

    #[test]
    fn test_event_handler_registry_new() {
        let registry = EventHandlerRegistry::new();
        assert_eq!(registry.extension_count(), 0);
    }

    #[test]
    fn test_event_handler_registry_default() {
        let registry = EventHandlerRegistry::default();
        assert_eq!(registry.extension_count(), 0);
    }

    #[test]
    fn test_event_handler_registry_register() {
        let mut registry = EventHandlerRegistry::new();

        let subscriptions = vec![
            EventSubscription::with_filter(EventTypeFilter::AgentLifecycle),
            EventSubscription::with_filter(EventTypeFilter::ToolExecution),
        ];

        registry.register("ext1".to_string(), subscriptions);

        assert_eq!(registry.extension_count(), 1);
        assert_eq!(registry.handler_count("ext1"), 2);
        assert_eq!(registry.handler_count("nonexistent"), 0);
    }

    #[test]
    fn test_event_handler_registry_unregister() {
        let mut registry = EventHandlerRegistry::new();

        let subscriptions = vec![EventSubscription::all()];
        registry.register("ext1".to_string(), subscriptions);

        assert_eq!(registry.extension_count(), 1);

        registry.unregister("ext1");
        assert_eq!(registry.extension_count(), 0);
    }

    #[test]
    fn test_event_handler_registry_get_handlers_for_event() {
        let mut registry = EventHandlerRegistry::new();

        // 注册高优先级的工具处理器
        registry.register(
            "ext1".to_string(),
            vec![EventSubscription::new(EventTypeFilter::ToolExecution, EventPriority::High)],
        );

        // 注册普通优先级的全局处理器
        registry.register(
            "ext2".to_string(),
            vec![EventSubscription::new(EventTypeFilter::All, EventPriority::Normal)],
        );

        // 注册低优先级的工具处理器
        registry.register(
            "ext3".to_string(),
            vec![EventSubscription::new(EventTypeFilter::ToolExecution, EventPriority::Low)],
        );

        let event = AgentEvent::BeforeToolCall {
            tool_call_id: "1".to_string(),
            tool_name: "test".to_string(),
            args: serde_json::Value::Null,
        };

        let handlers = registry.get_handlers_for_event(&event);

        // 应该匹配 3 个处理器
        assert_eq!(handlers.len(), 3);

        // 验证排序：高优先级在前
        assert_eq!(handlers[0].extension_name, "ext1");
        assert_eq!(handlers[0].priority, EventPriority::High);
        assert_eq!(handlers[1].extension_name, "ext2");
        assert_eq!(handlers[1].priority, EventPriority::Normal);
        assert_eq!(handlers[2].extension_name, "ext3");
        assert_eq!(handlers[2].priority, EventPriority::Low);
    }

    #[test]
    fn test_event_handler_registry_get_handlers_no_match() {
        let mut registry = EventHandlerRegistry::new();

        registry.register(
            "ext1".to_string(),
            vec![EventSubscription::with_filter(EventTypeFilter::Command)],
        );

        let event = AgentEvent::AgentStart;
        let handlers = registry.get_handlers_for_event(&event);

        // Command 过滤器不应匹配 AgentStart
        assert!(handlers.is_empty());
    }

    #[test]
    fn test_event_handler_registry_has_handlers_for() {
        let mut registry = EventHandlerRegistry::new();

        assert!(!registry.has_handlers_for(&AgentEvent::AgentStart));

        registry.register(
            "ext1".to_string(),
            vec![EventSubscription::with_filter(EventTypeFilter::AgentLifecycle)],
        );

        assert!(registry.has_handlers_for(&AgentEvent::AgentStart));
        assert!(!registry.has_handlers_for(&AgentEvent::TurnStart));
    }

    #[test]
    fn test_event_handler_registry_clear() {
        let mut registry = EventHandlerRegistry::new();

        registry.register("ext1".to_string(), vec![EventSubscription::all()]);
        registry.register("ext2".to_string(), vec![EventSubscription::all()]);

        assert_eq!(registry.extension_count(), 2);

        registry.clear();

        assert_eq!(registry.extension_count(), 0);
    }

    #[test]
    fn test_event_handler_record_debug() {
        let record = EventHandlerRecord {
            extension_name: "test-ext".to_string(),
            priority: EventPriority::High,
            filter: EventTypeFilter::ToolExecution,
        };

        let debug_str = format!("{:?}", record);
        assert!(debug_str.contains("EventHandlerRecord"));
        assert!(debug_str.contains("test-ext"));
        assert!(debug_str.contains("High"));
    }

    // ==================== DEFAULT_EVENT_TIMEOUT Test ====================

    #[test]
    fn test_default_event_timeout() {
        assert_eq!(DEFAULT_EVENT_TIMEOUT, Duration::from_secs(5));
    }
}
