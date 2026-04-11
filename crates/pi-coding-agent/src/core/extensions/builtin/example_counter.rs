//! 示例扩展：计数器
//!
//! 统计会话中的工具调用次数和消息数，演示扩展系统的完整功能

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::pin::Pin;
use std::future::Future;
use async_trait::async_trait;
use pi_agent::types::{AgentTool, AgentToolResult, AgentEvent};

use crate::core::extensions::types::*;
use crate::core::extensions::api::ExtensionContext;
use crate::core::extensions::loader::ExtensionFactory;

/// 计数器扩展
pub struct CounterExtension {
    manifest: ExtensionManifest,
    tool_call_count: Arc<AtomicUsize>,
    message_count: Arc<AtomicUsize>,
}

impl Default for CounterExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl CounterExtension {
    pub fn new() -> Self {
        Self {
            manifest: ExtensionManifest {
                name: "example-counter".to_string(),
                version: "1.0.0".to_string(),
                description: "Counts tool calls and messages in the session".to_string(),
                author: "pi-coding-agent".to_string(),
                entry_point: std::path::PathBuf::new(),
            },
            tool_call_count: Arc::new(AtomicUsize::new(0)),
            message_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl Extension for CounterExtension {
    fn manifest(&self) -> &ExtensionManifest {
        &self.manifest
    }
    
    async fn activate(&mut self, ctx: &ExtensionContext) -> anyhow::Result<()> {
        ctx.logger().info("Counter extension activated");
        // 尝试从持久化数据恢复计数
        if let Ok(Some(count_str)) = ctx.read_data("tool_calls") {
            if let Ok(count) = count_str.parse::<usize>() {
                self.tool_call_count.store(count, Ordering::Relaxed);
            }
        }
        if let Ok(Some(count_str)) = ctx.read_data("messages") {
            if let Ok(count) = count_str.parse::<usize>() {
                self.message_count.store(count, Ordering::Relaxed);
            }
        }
        Ok(())
    }
    
    async fn deactivate(&mut self) -> anyhow::Result<()> {
        tracing::info!("[example-counter] Counter extension deactivated. Final counts - tools: {}, messages: {}",
            self.tool_call_count.load(Ordering::Relaxed),
            self.message_count.load(Ordering::Relaxed));
        Ok(())
    }
    
    fn registered_tools(&self) -> Vec<Arc<dyn AgentTool>> {
        vec![Arc::new(CounterResetTool {
            tool_call_count: self.tool_call_count.clone(),
            message_count: self.message_count.clone(),
        })]
    }
    
    fn registered_commands(&self) -> Vec<SlashCommand> {
        let tc = self.tool_call_count.clone();
        let mc = self.message_count.clone();
        
        vec![
            SlashCommand::from_extension(
                "counter-stats",
                "Show counter statistics (tool calls and messages)",
                "example-counter",
                Arc::new(move |_args: CommandArgs| -> Pin<Box<dyn Future<Output = anyhow::Result<CommandResult>> + Send>> {
                    let tc = tc.clone();
                    let mc = mc.clone();
                    Box::pin(async move {
                        let tool_calls = tc.load(Ordering::Relaxed);
                        let messages = mc.load(Ordering::Relaxed);
                        Ok(CommandResult::new(format!(
                            "Counter Stats:\n  Tool calls: {}\n  Messages: {}\n  Total events: {}",
                            tool_calls, messages, tool_calls + messages
                        )))
                    })
                }),
            ),
        ]
    }
    
    fn event_subscriptions(&self) -> Vec<super::super::events::EventSubscription> {
        use super::super::events::{EventSubscription, EventTypeFilter, EventPriority};
        vec![
            // 高优先级订阅工具执行事件（用于统计工具调用）
            EventSubscription {
                filter: EventTypeFilter::ToolExecution,
                priority: EventPriority::High,
            },
            // 普通优先级订阅消息事件（用于统计消息）
            EventSubscription {
                filter: EventTypeFilter::Message,
                priority: EventPriority::Normal,
            },
            // 低优先级订阅 Agent 生命周期事件
            EventSubscription {
                filter: EventTypeFilter::AgentLifecycle,
                priority: EventPriority::Low,
            },
        ]
    }

    async fn on_event(&self, event: &AgentEvent) -> anyhow::Result<EventResult> {
        match event {
            AgentEvent::ToolExecutionEnd { .. } => {
                self.tool_call_count.fetch_add(1, Ordering::Relaxed);
            }
            AgentEvent::MessageEnd { .. } => {
                self.message_count.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        // 示例：如果需要阻止后续处理器处理此事件，可以返回 StopPropagation
        // if some_condition {
        //     return Ok(EventResult::StopPropagation);
        // }

        Ok(EventResult::Continue)
    }
}

/// 计数器重置工具
struct CounterResetTool {
    tool_call_count: Arc<AtomicUsize>,
    message_count: Arc<AtomicUsize>,
}

#[async_trait]
impl AgentTool for CounterResetTool {
    fn name(&self) -> &str { "counter_reset" }
    fn label(&self) -> &str { "Counter Reset" }
    fn description(&self) -> &str { "Reset the counter extension statistics to zero" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }
    
    async fn execute(
        &self,
        _tool_call_id: &str,
        _params: serde_json::Value,
        _cancel: tokio_util::sync::CancellationToken,
        _on_update: Option<Box<dyn Fn(AgentToolResult) + Send + Sync>>,
    ) -> anyhow::Result<AgentToolResult> {
        let old_tools = self.tool_call_count.swap(0, Ordering::Relaxed);
        let old_msgs = self.message_count.swap(0, Ordering::Relaxed);
        
        Ok(AgentToolResult {
            content: vec![pi_ai::types::ContentBlock::Text(pi_ai::types::TextContent::new(
                format!("Counters reset. Previous values - tool calls: {}, messages: {}", old_tools, old_msgs)
            ))],
            details: serde_json::json!({
                "previous_tool_calls": old_tools,
                "previous_messages": old_msgs,
            }),
        })
    }
}

/// 计数器扩展工厂
pub struct CounterExtensionFactory;

impl ExtensionFactory for CounterExtensionFactory {
    fn name(&self) -> &str { "example-counter" }
    
    fn create(&self) -> Box<dyn Extension> {
        Box::new(CounterExtension::new())
    }
    
    fn description(&self) -> &str {
        "Counts tool calls and messages in the session"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::core::extensions::api::ExtensionContext;
    use crate::core::extensions::types::{Extension, EventResult, CommandArgs};
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    fn create_test_context() -> ExtensionContext {
        ExtensionContext::new(
            PathBuf::from("."),
            AppConfig::default(),
            "test-session".to_string(),
            "example-counter",
        )
    }

    #[test]
    fn test_counter_extension_new() {
        let ext = CounterExtension::new();
        
        assert_eq!(ext.manifest().name, "example-counter");
        assert_eq!(ext.manifest().version, "1.0.0");
        assert_eq!(ext.tool_call_count.load(Ordering::Relaxed), 0);
        assert_eq!(ext.message_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_counter_extension_lifecycle() {
        let mut ext = CounterExtension::new();
        let ctx = create_test_context();
        
        // Test activate
        let result = ext.activate(&ctx).await;
        assert!(result.is_ok());
        
        // Test deactivate
        let result = ext.deactivate().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_counter_extension_on_event_tool_execution() {
        let ext = CounterExtension::new();
        
        assert_eq!(ext.tool_call_count.load(Ordering::Relaxed), 0);
        
        // Send ToolExecutionEnd event
        let event = AgentEvent::ToolExecutionEnd {
            tool_call_id: "test-id".to_string(),
            tool_name: "test-tool".to_string(),
            result: AgentToolResult {
                content: vec![],
                details: serde_json::Value::Null,
            },
            is_error: false,
        };
        
        let result = ext.on_event(&event).await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), EventResult::Continue));
        assert_eq!(ext.tool_call_count.load(Ordering::Relaxed), 1);
        
        // Send another event
        let _ = ext.on_event(&event).await;
        assert_eq!(ext.tool_call_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_counter_extension_on_event_message() {
        let ext = CounterExtension::new();
        
        assert_eq!(ext.message_count.load(Ordering::Relaxed), 0);
        
        // Send MessageEnd event
        let event = AgentEvent::MessageEnd {
            message: pi_agent::types::AgentMessage::user("test"),
        };
        
        let result = ext.on_event(&event).await;
        assert!(result.is_ok());
        assert_eq!(ext.message_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_counter_extension_on_event_other() {
        let ext = CounterExtension::new();
        
        // Other events should not change counters
        let event = AgentEvent::AgentStart;
        let result = ext.on_event(&event).await;
        
        assert!(result.is_ok());
        assert_eq!(ext.tool_call_count.load(Ordering::Relaxed), 0);
        assert_eq!(ext.message_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_counter_extension_registered_tools() {
        let ext = CounterExtension::new();
        let tools = ext.registered_tools();
        
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "counter_reset");
        assert_eq!(tools[0].label(), "Counter Reset");
    }

    #[test]
    fn test_counter_extension_registered_commands() {
        let ext = CounterExtension::new();
        let commands = ext.registered_commands();
        
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "counter-stats");
        assert!(commands[0].matches("counter-stats"));
    }

    #[tokio::test]
    async fn test_counter_reset_tool() {
        let tool_call_count = Arc::new(AtomicUsize::new(5));
        let message_count = Arc::new(AtomicUsize::new(3));
        
        let tool = CounterResetTool {
            tool_call_count: tool_call_count.clone(),
            message_count: message_count.clone(),
        };
        
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = tool.execute(
            "call-id",
            serde_json::json!({}),
            cancel,
            None,
        ).await;
        
        assert!(result.is_ok());
        let result = result.unwrap();
        
        // Verify counters were reset
        assert_eq!(tool_call_count.load(Ordering::Relaxed), 0);
        assert_eq!(message_count.load(Ordering::Relaxed), 0);
        
        // Verify result contains previous values
        assert_eq!(result.details["previous_tool_calls"], 5);
        assert_eq!(result.details["previous_messages"], 3);
    }

    #[tokio::test]
    async fn test_counter_extension_full_lifecycle() {
        // Create extension
        let mut ext = CounterExtension::new();
        let ctx = create_test_context();
        
        // Activate
        ext.activate(&ctx).await.unwrap();
        
        // Simulate events
        let tool_event = AgentEvent::ToolExecutionEnd {
            tool_call_id: "1".to_string(),
            tool_name: "tool1".to_string(),
            result: AgentToolResult {
                content: vec![],
                details: serde_json::Value::Null,
            },
            is_error: false,
        };
        ext.on_event(&tool_event).await.unwrap();
        ext.on_event(&tool_event).await.unwrap();
        
        let msg_event = AgentEvent::MessageEnd {
            message: pi_agent::types::AgentMessage::user("test"),
        };
        ext.on_event(&msg_event).await.unwrap();
        
        // Verify counts
        assert_eq!(ext.tool_call_count.load(Ordering::Relaxed), 2);
        assert_eq!(ext.message_count.load(Ordering::Relaxed), 1);
        
        // Deactivate
        ext.deactivate().await.unwrap();
    }

    #[test]
    fn test_counter_extension_factory() {
        let factory = CounterExtensionFactory;
        
        assert_eq!(factory.name(), "example-counter");
        assert_eq!(factory.description(), "Counts tool calls and messages in the session");
        
        let ext = factory.create();
        assert_eq!(ext.manifest().name, "example-counter");
    }

    #[tokio::test]
    async fn test_counter_stats_command() {
        let ext = CounterExtension::new();
        
        // Increment counters
        ext.tool_call_count.fetch_add(5, Ordering::Relaxed);
        ext.message_count.fetch_add(3, Ordering::Relaxed);
        
        // Get command and execute
        let commands = ext.registered_commands();
        let cmd = &commands[0];
        
        let args = CommandArgs::new("");
        let result = (cmd.handler)(args).await;
        
        assert!(result.is_ok());
        let result = result.unwrap();
        
        assert!(result.message.contains("Tool calls: 5"));
        assert!(result.message.contains("Messages: 3"));
        assert!(result.message.contains("Total events: 8"));
        assert!(result.should_render);
    }
}
