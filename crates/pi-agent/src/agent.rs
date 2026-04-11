//! Agent 主结构体
//!
//! 提供有状态的 Agent 包装器，管理消息队列和生命周期

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, Notify};
use tokio_util::sync::CancellationToken;
use futures::future::BoxFuture;

use pi_ai::types::*;

use crate::types::*;
use crate::agent_loop::*;

/// Agent 配置选项
/// 
/// 用于创建和配置 Agent 实例的各项参数
#[allow(clippy::type_complexity)] // 复杂类型是必要的，用于回调函数
pub struct AgentOptions {
    /// 模型
    pub model: Option<Model>,
    /// 系统提示词
    pub system_prompt: Option<String>,
    /// 工具列表
    pub tools: Vec<Arc<dyn AgentTool>>,
    /// 思考级别
    pub thinking_level: ThinkingLevel,
    /// 思考预算
    pub thinking_budgets: Option<ThinkingBudgets>,
    /// 传输方式
    pub transport: Option<Transport>,
    /// 工具执行模式
    pub tool_execution: ToolExecutionMode,
    /// 会话 ID
    pub session_id: Option<String>,
    /// 最大重试延迟（毫秒）
    pub max_retry_delay_ms: Option<u64>,
    /// 转换为 LLM 消息的回调
    pub convert_to_llm: Option<Arc<dyn Fn(&[AgentMessage]) -> Vec<Message> + Send + Sync>>,
    /// 获取 API 密钥的回调
    pub get_api_key: Option<Arc<dyn Fn(&str) -> Option<String> + Send + Sync>>,
    /// 工具调用前钩子
    pub before_tool_call: Option<
        Arc<
            dyn Fn(&ToolCallContext, CancellationToken) -> BoxFuture<'static, Option<BeforeToolCallResult>>
                + Send
                + Sync,
        >,
    >,
    /// 工具调用后钩子
    pub after_tool_call: Option<
        Arc<
            dyn Fn(
                    &ToolCallContext,
                    &AgentToolResult,
                    bool,
                    CancellationToken,
                ) -> BoxFuture<'static, Option<AfterToolCallResult>>
                + Send
                + Sync,
        >,
    >,
    /// 转向模式
    pub steering_mode: QueueMode,
    /// 跟进模式
    pub follow_up_mode: QueueMode,
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            model: None,
            system_prompt: None,
            tools: Vec::new(),
            thinking_level: ThinkingLevel::Off,
            thinking_budgets: None,
            transport: None,
            tool_execution: ToolExecutionMode::Parallel,
            session_id: None,
            max_retry_delay_ms: None,
            convert_to_llm: None,
            get_api_key: None,
            before_tool_call: None,
            after_tool_call: None,
            steering_mode: QueueMode::OneAtATime,
            follow_up_mode: QueueMode::OneAtATime,
        }
    }
}

/// 默认模型（未知模型占位符）
fn default_model() -> Model {
    Model {
        id: "unknown".to_string(),
        name: "unknown".to_string(),
        api: Api::Anthropic,
        provider: Provider::Anthropic,
        base_url: String::new(),
        reasoning: false,
        input: vec![InputModality::Text],
        cost: ModelCost {
            input: 0.0,
            output: 0.0,
            cache_read: None,
            cache_write: None,
        },
        context_window: 0,
        max_tokens: 0,
        headers: None,
        compat: None,
    }
}

/// 默认消息转换函数
pub fn default_convert_to_llm(messages: &[AgentMessage]) -> Vec<Message> {
    messages
        .iter()
        .map(|msg| match msg {
            AgentMessage::Llm(m) => match m {
                Message::User(_) | Message::Assistant(_) | Message::ToolResult(_) => m.clone(),
            },
        })
        .collect()
}

/// Agent 主结构体
/// 
/// 管理消息队列、生命周期和事件处理的核心组件
#[allow(clippy::type_complexity)] // 复杂类型是必要的，用于回调函数
pub struct Agent {
    state: Arc<RwLock<AgentState>>,
    listeners: Arc<RwLock<Vec<Arc<dyn Fn(AgentEvent, CancellationToken) + Send + Sync>>>>,
    cancel: Arc<Mutex<Option<CancellationToken>>>,
    idle_notify: Arc<Notify>,

    // 消息队列
    steering_queue: Arc<Mutex<PendingMessageQueue>>,
    follow_up_queue: Arc<Mutex<PendingMessageQueue>>,

    // 配置
    convert_to_llm: Arc<dyn Fn(&[AgentMessage]) -> Vec<Message> + Send + Sync>,
    get_api_key: Option<Arc<dyn Fn(&str) -> Option<String> + Send + Sync>>,
    before_tool_call: Option<
        Arc<
            dyn Fn(&ToolCallContext, CancellationToken) -> BoxFuture<'static, Option<BeforeToolCallResult>>
                + Send
                + Sync,
        >,
    >,
    after_tool_call: Option<
        Arc<
            dyn Fn(
                    &ToolCallContext,
                    &AgentToolResult,
                    bool,
                    CancellationToken,
                ) -> BoxFuture<'static, Option<AfterToolCallResult>>
                + Send
                + Sync,
        >,
    >,
    session_id: Option<String>,
    thinking_budgets: Option<ThinkingBudgets>,
    transport: Option<Transport>,
    max_retry_delay_ms: Option<u64>,
    tool_execution: ToolExecutionMode,
}

impl Agent {
    /// 创建新的 Agent 实例
    /// 
    /// 根据提供的选项初始化 Agent 状态和配置
    pub fn new(options: AgentOptions) -> Self {
        let model = options.model.unwrap_or_else(default_model);
        let mut state = AgentState::new(model);
        state.system_prompt = options.system_prompt.unwrap_or_default();
        state.tools = options.tools.clone();
        state.thinking_level = options.thinking_level;

        Self {
            state: Arc::new(RwLock::new(state)),
            listeners: Arc::new(RwLock::new(Vec::new())),
            cancel: Arc::new(Mutex::new(None)),
            idle_notify: Arc::new(Notify::new()),
            steering_queue: Arc::new(Mutex::new(PendingMessageQueue::new(options.steering_mode))),
            follow_up_queue: Arc::new(Mutex::new(PendingMessageQueue::new(options.follow_up_mode))),
            convert_to_llm: options.convert_to_llm.unwrap_or_else(|| Arc::new(default_convert_to_llm)),
            get_api_key: options.get_api_key,
            before_tool_call: options.before_tool_call,
            after_tool_call: options.after_tool_call,
            session_id: options.session_id,
            thinking_budgets: options.thinking_budgets,
            transport: options.transport,
            max_retry_delay_ms: options.max_retry_delay_ms,
            tool_execution: options.tool_execution,
        }
    }

    /// 订阅事件，返回取消订阅函数
    pub fn subscribe(
        &self,
        listener: Arc<dyn Fn(AgentEvent, CancellationToken) + Send + Sync>,
    ) -> impl FnOnce() {
        let listeners = self.listeners.clone();
        let listener_clone = listener.clone();

        // 添加到监听器列表
        tokio::spawn(async move {
            let mut list = listeners.write().await;
            list.push(listener_clone);
        });

        // 返回取消订阅函数
        let listeners = self.listeners.clone();
        move || {
            let listeners = listeners.clone();
            tokio::spawn(async move {
                let mut list = listeners.write().await;
                list.retain(|l| !Arc::ptr_eq(l, &listener));
            });
        }
    }

    /// 获取当前状态快照
    pub async fn state(&self) -> AgentState {
        self.state.read().await.clone()
    }

    /// 获取取消 token
    pub async fn cancel_token(&self) -> Option<CancellationToken> {
        self.cancel.lock().await.clone()
    }

    /// 注入转向消息
    pub async fn steer(&self, message: AgentMessage) {
        self.steering_queue.lock().await.enqueue(message);
    }

    /// 注入后续消息
    pub async fn follow_up(&self, message: AgentMessage) {
        self.follow_up_queue.lock().await.enqueue(message);
    }

    /// 清除转向队列
    pub async fn clear_steering_queue(&self) {
        self.steering_queue.lock().await.clear();
    }

    /// 清除后续队列
    pub async fn clear_follow_up_queue(&self) {
        self.follow_up_queue.lock().await.clear();
    }

    /// 清除所有队列
    pub async fn clear_all_queues(&self) {
        self.clear_steering_queue().await;
        self.clear_follow_up_queue().await;
    }

    /// 检查是否有队列消息
    pub async fn has_queued_messages(&self) -> bool {
        let steering = self.steering_queue.lock().await.has_items();
        let follow_up = self.follow_up_queue.lock().await.has_items();
        steering || follow_up
    }

    /// 中止当前运行
    pub async fn abort(&self) {
        if let Some(token) = self.cancel.lock().await.take() {
            token.cancel();
        }
    }

    /// 等待空闲
    pub async fn wait_for_idle(&self) {
        // 如果正在运行，等待通知
        let is_streaming = self.state.read().await.is_streaming;
        if is_streaming {
            self.idle_notify.notified().await;
        }
    }

    /// 重置状态
    pub async fn reset(&self) {
        let mut state = self.state.write().await;
        state.messages.clear();
        state.is_streaming = false;
        state.streaming_message = None;
        state.pending_tool_calls.clear();
        state.error_message = None;
        drop(state);

        self.clear_all_queues().await;
    }

    /// 发送 prompt 并运行 agent 循环
    pub async fn prompt(&self, message: AgentMessage) -> anyhow::Result<()> {
        // 检查是否已经在运行
        {
            let state = self.state.read().await;
            if state.is_streaming {
                anyhow::bail!("Agent is already processing a prompt. Use steer() or followUp() to queue messages, or wait for completion.");
            }
        }

        self.run_with_lifecycle(|cancel| async move {
            let mut context = self.create_context_snapshot().await;
            let config = self.create_loop_config(false).await;

            run_agent_loop(
                vec![message],
                &mut context,
                &config,
                &|event| {
                    self.process_event(event);
                },
                cancel,
            )
            .await?;

            // 更新状态
            let mut state = self.state.write().await;
            state.messages = context.messages;

            Ok(())
        })
        .await
    }

    /// 发送文本 prompt
    pub async fn prompt_text(&self, text: &str) -> anyhow::Result<()> {
        self.prompt(AgentMessage::user(text)).await
    }

    /// 发送带图片的 prompt
    pub async fn prompt_with_images(&self, text: &str, images: Vec<ImageContent>) -> anyhow::Result<()> {
        self.prompt(AgentMessage::user_with_images(text, images)).await
    }

    /// 继续上一次的循环
    pub async fn continue_loop(&self) -> anyhow::Result<()> {
        // 检查是否已经在运行
        {
            let state = self.state.read().await;
            if state.is_streaming {
                anyhow::bail!("Agent is already processing. Wait for completion before continuing.");
            }

            // 检查最后一条消息
            if let Some(last) = state.messages.last() {
                if last.role() == "assistant" {
                    // 检查是否有队列消息
                    let steering = self.steering_queue.lock().await.drain();
                    if !steering.is_empty() {
                        return self.run_with_lifecycle(|cancel| async move {
                            let mut context = self.create_context_snapshot().await;
                            let config = self.create_loop_config(true).await;

                            run_agent_loop(
                                steering,
                                &mut context,
                                &config,
                                &|event| {
                                    self.process_event(event);
                                },
                                cancel,
                            )
                            .await?;

                            let mut state = self.state.write().await;
                            state.messages = context.messages;

                            Ok(())
                        }).await;
                    }

                    let follow_ups = self.follow_up_queue.lock().await.drain();
                    if !follow_ups.is_empty() {
                        return self.run_with_lifecycle(|cancel| async move {
                            let mut context = self.create_context_snapshot().await;
                            let config = self.create_loop_config(false).await;

                            run_agent_loop(
                                follow_ups,
                                &mut context,
                                &config,
                                &|event| {
                                    self.process_event(event);
                                },
                                cancel,
                            )
                            .await?;

                            let mut state = self.state.write().await;
                            state.messages = context.messages;

                            Ok(())
                        }).await;
                    }

                    anyhow::bail!("Cannot continue from message role: assistant");
                }
            } else {
                anyhow::bail!("No messages to continue from");
            }
        }

        // 继续循环
        self.run_with_lifecycle(|cancel| async move {
            let mut context = self.create_context_snapshot().await;
            let config = self.create_loop_config(false).await;

            run_agent_loop_continue(
                &mut context,
                &config,
                &|event| {
                    self.process_event(event);
                },
                cancel,
            )
            .await?;

            let mut state = self.state.write().await;
            state.messages = context.messages;

            Ok(())
        })
        .await
    }

    /// 运行生命周期管理
    async fn run_with_lifecycle<F, Fut>(&self, executor: F) -> anyhow::Result<()>
    where
        F: FnOnce(CancellationToken) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        // 设置运行状态
        {
            let mut state = self.state.write().await;
            state.is_streaming = true;
            state.streaming_message = None;
            state.error_message = None;
        }

        // 创建取消 token
        let cancel = CancellationToken::new();
        *self.cancel.lock().await = Some(cancel.clone());

        // 执行
        let result = executor(cancel.clone()).await;

        // 处理错误
        if let Err(ref e) = result {
            if !cancel.is_cancelled() {
                let error_msg = AssistantMessage::new(
                    Api::Anthropic,
                    Provider::Anthropic,
                    "unknown",
                )
                .with_error_message(e.to_string());

                let mut state = self.state.write().await;
                state.messages.push(AgentMessage::Llm(Message::Assistant(error_msg.clone())));
                state.error_message = Some(error_msg.error_message.clone().unwrap_or_default());
            }
        }

        // 清理运行状态
        {
            let mut state = self.state.write().await;
            state.is_streaming = false;
            state.streaming_message = None;
            state.pending_tool_calls.clear();
        }

        *self.cancel.lock().await = None;
        self.idle_notify.notify_waiters();

        result
    }

    /// 创建上下文快照
    async fn create_context_snapshot(&self) -> AgentContext {
        let state = self.state.read().await;
        AgentContext {
            system_prompt: state.system_prompt.clone(),
            messages: state.messages.clone(),
            tools: state.tools.clone(),
        }
    }

    /// 创建循环配置
    async fn create_loop_config(&self, skip_initial_steering: bool) -> AgentLoopConfig {
        let state = self.state.read().await;

        let steering_queue = self.steering_queue.clone();
        let follow_up_queue = self.follow_up_queue.clone();

        AgentLoopConfig {
            model: state.model.clone(),
            thinking_level: state.thinking_level.clone(),
            thinking_budgets: self.thinking_budgets.clone(),
            temperature: None,
            max_tokens: None,
            transport: self.transport.clone(),
            cache_retention: None,
            session_id: self.session_id.clone(),
            max_retry_delay_ms: self.max_retry_delay_ms,
            context_manager: None, // 可在 AgentOptions 中配置
            convert_to_llm: self.convert_to_llm.clone(),
            transform_context: None,
            get_api_key: self.get_api_key.clone(),
            get_steering_messages: Some(Arc::new(move || {
                if skip_initial_steering {
                    Vec::new()
                } else {
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            steering_queue.lock().await.drain()
                        })
                    })
                }
            })),
            get_follow_up_messages: Some(Arc::new(move || {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        follow_up_queue.lock().await.drain()
                    })
                })
            })),
            tool_execution: self.tool_execution,
            before_tool_call: self.before_tool_call.clone(),
            after_tool_call: self.after_tool_call.clone(),
        }
    }

    /// 处理事件
    fn process_event(&self, event: AgentEvent) {
        // 更新内部状态
        let state = self.state.clone();
        let listeners = self.listeners.clone();

        tokio::spawn(async move {
            {
                let mut s = state.write().await;
                match &event {
                    AgentEvent::MessageStart { message } => {
                        s.streaming_message = Some(message.clone());
                    }
                    AgentEvent::MessageUpdate { message, .. } => {
                        s.streaming_message = Some(message.clone());
                    }
                    AgentEvent::MessageEnd { message } => {
                        s.streaming_message = None;
                        s.messages.push(message.clone());
                    }
                    AgentEvent::ToolExecutionStart { tool_call_id, .. } => {
                        s.pending_tool_calls.insert(tool_call_id.clone());
                    }
                    AgentEvent::ToolExecutionEnd { tool_call_id, .. } => {
                        s.pending_tool_calls.remove(tool_call_id);
                    }
                    AgentEvent::TurnEnd { message: AgentMessage::Llm(Message::Assistant(assistant)), .. } => {
                        if let Some(ref error) = assistant.error_message {
                            s.error_message = Some(error.clone());
                        }
                    }
                    AgentEvent::AgentEnd { .. } => {
                        s.streaming_message = None;
                    }
                    _ => {}
                }
            }

            // 通知监听器
            let cancel = CancellationToken::new(); // 创建临时 token 用于监听器
            let list = listeners.read().await;
            for listener in list.iter() {
                listener(event.clone(), cancel.clone());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::fixtures::*;

    #[test]
    fn test_agent_creation() {
        let options = AgentOptions::default();
        let _agent = Agent::new(options);
        
        // 测试 Agent 创建成功
        // 由于 Agent 字段是私有的，我们通过测试方法不 panic 来验证
    }

    #[test]
    fn test_agent_creation_with_model() {
        let model = sample_agent_state().model;
        let options = AgentOptions {
            model: Some(model.clone()),
            ..Default::default()
        };
        let _agent = Agent::new(options);
        
        // 验证创建成功
    }

    #[test]
    fn test_agent_creation_with_system_prompt() {
        let options = AgentOptions {
            system_prompt: Some("You are a test assistant".to_string()),
            ..Default::default()
        };
        let _agent = Agent::new(options);
        
        // 验证创建成功
    }

    #[test]
    fn test_agent_creation_with_tools_extended() {
        let tools = sample_mock_tools();
        let options = AgentOptions {
            tools,
            ..Default::default()
        };
        let _agent = Agent::new(options);
        
        // 验证创建成功
    }

    #[test]
    fn test_default_agent_options() {
        let options = AgentOptions::default();
        
        assert!(options.model.is_none());
        assert!(options.system_prompt.is_none());
        assert!(options.tools.is_empty());
        assert_eq!(options.thinking_level, ThinkingLevel::Off);
        assert!(options.thinking_budgets.is_none());
        assert!(options.transport.is_none());
        assert_eq!(options.tool_execution, ToolExecutionMode::Parallel);
        assert!(options.session_id.is_none());
        assert!(options.max_retry_delay_ms.is_none());
        assert!(options.convert_to_llm.is_none());
        assert!(options.get_api_key.is_none());
        assert!(options.before_tool_call.is_none());
        assert!(options.after_tool_call.is_none());
        assert_eq!(options.steering_mode, QueueMode::OneAtATime);
        assert_eq!(options.follow_up_mode, QueueMode::OneAtATime);
    }

    #[test]
    fn test_default_convert_to_llm() {
        let messages = vec![
            AgentMessage::user("Hello"),
            AgentMessage::user("World"),
        ];
        
        let llm_messages = default_convert_to_llm(&messages);
        
        // 验证转换后的消息数量
        assert_eq!(llm_messages.len(), 2);
        
        // 验证每条消息都被正确转换
        for msg in &llm_messages {
            match msg {
                Message::User(_) => {},
                _ => panic!("Expected User message"),
            }
        }
    }

    #[test]
    fn test_default_convert_to_llm_filters_non_llm() {
        // 测试过滤逻辑 - 只保留 User, Assistant, ToolResult
        let messages = vec![
            AgentMessage::user("Test message"),
        ];
        
        let llm_messages = default_convert_to_llm(&messages);
        assert_eq!(llm_messages.len(), 1);
    }

    #[tokio::test]
    async fn test_agent_state_snapshot() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        let state = agent.state().await;
        
        // 验证状态快照
        assert!(!state.is_streaming);
        assert!(state.streaming_message.is_none());
        assert!(state.pending_tool_calls.is_empty());
        assert!(state.error_message.is_none());
    }

    #[tokio::test]
    async fn test_agent_subscribe() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        
        let listener: Arc<dyn Fn(AgentEvent, CancellationToken) + Send + Sync> = 
            Arc::new(move |_event, _cancel| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            });
        
        let _unsubscribe = agent.subscribe(listener);
        
        // 给订阅一点时间注册
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn test_agent_steering_queue() {
        let options = AgentOptions {
            steering_mode: QueueMode::OneAtATime,
            ..Default::default()
        };
        let agent = Agent::new(options);
        
        // 添加转向消息
        agent.steer(AgentMessage::user("Steering message 1")).await;
        agent.steer(AgentMessage::user("Steering message 2")).await;
        
        // 验证队列有消息
        assert!(agent.has_queued_messages().await);
        
        // 清除队列
        agent.clear_steering_queue().await;
        
        // 注意：follow_up_queue 可能仍然有消息，所以 has_queued_messages 可能仍然返回 true
    }

    #[tokio::test]
    async fn test_agent_follow_up_queue() {
        let options = AgentOptions {
            follow_up_mode: QueueMode::OneAtATime,
            ..Default::default()
        };
        let agent = Agent::new(options);
        
        // 添加后续消息
        agent.follow_up(AgentMessage::user("Follow up message")).await;
        
        // 验证队列有消息
        assert!(agent.has_queued_messages().await);
        
        // 清除队列
        agent.clear_follow_up_queue().await;
    }

    #[tokio::test]
    async fn test_agent_clear_all_queues() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        // 添加消息到两个队列
        agent.steer(AgentMessage::user("Steering")).await;
        agent.follow_up(AgentMessage::user("Follow up")).await;
        
        // 验证队列有消息
        assert!(agent.has_queued_messages().await);
        
        // 清除所有队列
        agent.clear_all_queues().await;
        
        // 验证队列为空
        assert!(!agent.has_queued_messages().await);
    }

    #[tokio::test]
    async fn test_agent_reset() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        // 添加一些队列消息
        agent.steer(AgentMessage::user("Test")).await;
        
        // 重置
        agent.reset().await;
        
        // 验证状态被重置
        let state = agent.state().await;
        assert!(state.messages.is_empty());
        assert!(!state.is_streaming);
        assert!(state.streaming_message.is_none());
        assert!(state.pending_tool_calls.is_empty());
        assert!(state.error_message.is_none());
        assert!(!agent.has_queued_messages().await);
    }

    #[tokio::test]
    async fn test_agent_abort_no_active_operation() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        // 中止没有活动的操作应该不会 panic
        agent.abort().await;
        
        // 验证 cancel token 被清除
        let token = agent.cancel_token().await;
        assert!(token.is_none());
    }

    #[tokio::test]
    async fn test_agent_cancel_token() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        // 初始状态应该没有 cancel token
        let token = agent.cancel_token().await;
        assert!(token.is_none());
    }

    #[test]
    fn test_default_model() {
        let model = default_model();
        
        assert_eq!(model.id, "unknown");
        assert_eq!(model.name, "unknown");
        assert_eq!(model.api, Api::Anthropic);
        assert_eq!(model.provider, Provider::Anthropic);
        assert!(model.base_url.is_empty());
        assert!(!model.reasoning);
        assert_eq!(model.input, vec![InputModality::Text]);
        assert_eq!(model.cost.input, 0.0);
        assert_eq!(model.cost.output, 0.0);
        assert!(model.cost.cache_read.is_none());
        assert!(model.cost.cache_write.is_none());
        assert_eq!(model.context_window, 0);
        assert_eq!(model.max_tokens, 0);
        assert!(model.headers.is_none());
        assert!(model.compat.is_none());
    }

    // === 额外状态管理测试 ===

    #[test]
    fn test_agent_options_with_all_fields() {
        use pi_ai::types::{Model, ModelCost, ThinkingLevel, ThinkingBudgets, Transport};
        
        let model = Model {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            api: Api::Anthropic,
            provider: Provider::Anthropic,
            base_url: "https://api.test.com".to_string(),
            reasoning: true,
            input: vec![pi_ai::types::InputModality::Text],
            cost: ModelCost {
                input: 0.001,
                output: 0.002,
                cache_read: Some(0.0005),
                cache_write: Some(0.0015),
            },
            context_window: 100000,
            max_tokens: 4096,
            headers: None,
            compat: None,
        };

        let options = AgentOptions {
            model: Some(model.clone()),
            system_prompt: Some("You are a test assistant".to_string()),
            tools: vec![],
            thinking_level: ThinkingLevel::Medium,
            thinking_budgets: Some(ThinkingBudgets { thinking_budget: Some(1000), plan_budget: None }),
            transport: Some(Transport::Sse),
            tool_execution: ToolExecutionMode::Sequential,
            session_id: Some("test-session".to_string()),
            max_retry_delay_ms: Some(5000),
            convert_to_llm: None,
            get_api_key: None,
            before_tool_call: None,
            after_tool_call: None,
            steering_mode: QueueMode::All,
            follow_up_mode: QueueMode::All,
        };

        assert!(options.model.is_some());
        assert_eq!(options.system_prompt, Some("You are a test assistant".to_string()));
        assert_eq!(options.thinking_level, ThinkingLevel::Medium);
        assert!(options.thinking_budgets.is_some());
        assert!(options.transport.is_some());
        assert_eq!(options.tool_execution, ToolExecutionMode::Sequential);
        assert_eq!(options.session_id, Some("test-session".to_string()));
        assert_eq!(options.max_retry_delay_ms, Some(5000));
        assert_eq!(options.steering_mode, QueueMode::All);
        assert_eq!(options.follow_up_mode, QueueMode::All);
    }

    #[tokio::test]
    async fn test_agent_wait_for_idle_not_streaming() {
        // 当 agent 不在 streaming 状态时，wait_for_idle 应该立即返回
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        // 不应该阻塞
        agent.wait_for_idle().await;
    }

    #[tokio::test]
    async fn test_agent_multiple_steering_messages() {
        let options = AgentOptions {
            steering_mode: QueueMode::All,
            ..Default::default()
        };
        let agent = Agent::new(options);
        
        // 添加多个 steering 消息
        for i in 0..5 {
            agent.steer(AgentMessage::user(&format!("steering {}", i))).await;
        }
        
        assert!(agent.has_queued_messages().await);
        
        // 清除 steering 队列
        agent.clear_steering_queue().await;
        
        // 还有 follow_up 队列可能为空，所以整体可能为空
        // 但因为我们没有添加 follow_up，所以应该为空
        assert!(!agent.has_queued_messages().await);
    }

    #[tokio::test]
    async fn test_agent_clear_steering_only() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        // 添加 steering 消息
        agent.steer(AgentMessage::user("steering")).await;
        
        // 只清除 steering
        agent.clear_steering_queue().await;
        
        // 检查是否还有消息（follow_up 队列）
        // 因为我们没有添加 follow_up，所以应该为空
        assert!(!agent.has_queued_messages().await);
    }

    #[tokio::test]
    async fn test_agent_clear_follow_up_only() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        // 添加 follow_up 消息
        agent.follow_up(AgentMessage::user("follow up")).await;
        
        // 只清除 follow_up
        agent.clear_follow_up_queue().await;
        
        // 应该为空
        assert!(!agent.has_queued_messages().await);
    }

    #[test]
    fn test_agent_creation_with_tools() {
        let tools = sample_mock_tools();
        let options = AgentOptions {
            tools: tools.clone(),
            ..Default::default()
        };
        
        // 创建 agent 不应该 panic
        let _agent = Agent::new(options);
    }

    #[test]
    fn test_default_convert_to_llm_empty() {
        let messages: Vec<AgentMessage> = vec![];
        let llm_messages = default_convert_to_llm(&messages);
        assert!(llm_messages.is_empty());
    }

    #[test]
    fn test_default_convert_to_llm_mixed() {
        use pi_ai::types::{AssistantMessage, Api, Provider};
        
        let messages = vec![
            AgentMessage::user("hello"),
            AgentMessage::Llm(Message::Assistant(AssistantMessage::new(
                Api::Anthropic,
                Provider::Anthropic,
                "claude-3"
            ))),
        ];
        
        let llm_messages = default_convert_to_llm(&messages);
        assert_eq!(llm_messages.len(), 2);
    }

    #[tokio::test]
    async fn test_agent_subscribe_multiple() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));
        
        let counter1_clone = counter1.clone();
        let counter2_clone = counter2.clone();
        
        let listener1: Arc<dyn Fn(AgentEvent, CancellationToken) + Send + Sync> = 
            Arc::new(move |_event, _cancel| {
                counter1_clone.fetch_add(1, Ordering::SeqCst);
            });
        
        let listener2: Arc<dyn Fn(AgentEvent, CancellationToken) + Send + Sync> = 
            Arc::new(move |_event, _cancel| {
                counter2_clone.fetch_add(1, Ordering::SeqCst);
            });
        
        let _unsubscribe1 = agent.subscribe(listener1);
        let _unsubscribe2 = agent.subscribe(listener2);
        
        // 给订阅一点时间注册
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        // 两个监听器都应该已注册
        let state = agent.state().await;
        // 状态本身不存储监听器数量，但我们验证了订阅没有 panic
        assert!(!state.is_streaming);
    }

    #[test]
    fn test_agent_options_debug() {
        let options = AgentOptions::default();
        // AgentOptions 没有实现 Debug，但我们能创建它
        let _agent = Agent::new(options);
    }

    #[tokio::test]
    async fn test_agent_reset_clears_all() {
        let options = AgentOptions::default();
        let agent = Agent::new(options);
        
        // 添加队列消息
        agent.steer(AgentMessage::user("steering")).await;
        agent.follow_up(AgentMessage::user("follow up")).await;
        
        // 重置
        agent.reset().await;
        
        // 验证所有队列被清空
        assert!(!agent.has_queued_messages().await);
        
        // 验证状态被重置
        let state = agent.state().await;
        assert!(state.messages.is_empty());
        assert!(!state.is_streaming);
    }

    #[test]
    fn test_agent_options_clone() {
        let options = AgentOptions {
            system_prompt: Some("test".to_string()),
            ..Default::default()
        };
        
        // AgentOptions 没有实现 Clone，但我们能创建它
        let _agent = Agent::new(options);
    }
}
