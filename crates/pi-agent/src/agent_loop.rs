//! Agent 循环核心
//!
//! 处理 Agent 的主要循环逻辑，包括消息流、工具执行等

use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use futures::future::BoxFuture;
use futures::StreamExt;
use pi_ai::types::*;

use crate::types::*;
use crate::context_manager::ContextWindowManager;

/// Agent 循环配置
/// 
/// 定义 Agent 主循环的行为参数和回调函数
#[allow(clippy::type_complexity)] // 复杂类型是必要的，用于回调函数
pub struct AgentLoopConfig {
    /// 模型
    pub model: Model,
    /// 思考级别
    pub thinking_level: ThinkingLevel,
    /// 思考预算
    pub thinking_budgets: Option<ThinkingBudgets>,
    /// 温度参数
    pub temperature: Option<f64>,
    /// 最大 token 数
    pub max_tokens: Option<u32>,
    /// 传输方式
    pub transport: Option<Transport>,
    /// 缓存保留策略
    pub cache_retention: Option<CacheRetention>,
    /// 会话 ID
    pub session_id: Option<String>,
    /// 最大重试延迟（毫秒）
    pub max_retry_delay_ms: Option<u64>,

    /// 上下文窗口管理器
    pub context_manager: Option<Arc<ContextWindowManager>>,

    /// 消息转换函数（AgentMessage -> LLM Message）
    pub convert_to_llm: Arc<dyn Fn(&[AgentMessage]) -> Vec<Message> + Send + Sync>,

    /// 上下文变换（可选，在每次 LLM 调用前执行）
    pub transform_context: Option<
        Arc<
            dyn Fn(Vec<AgentMessage>, CancellationToken) -> BoxFuture<'static, Vec<AgentMessage>>
                + Send
                + Sync,
        >,
    >,

    /// API Key 获取
    pub get_api_key: Option<Arc<dyn Fn(&str) -> Option<String> + Send + Sync>>,

    /// 转向消息（mid-turn注入）
    pub get_steering_messages: Option<Arc<dyn Fn() -> Vec<AgentMessage> + Send + Sync>>,

    /// 后续消息
    pub get_follow_up_messages: Option<Arc<dyn Fn() -> Vec<AgentMessage> + Send + Sync>>,

    /// 工具执行模式
    pub tool_execution: ToolExecutionMode,

    /// beforeToolCall 钩子
    pub before_tool_call: Option<
        Arc<
            dyn Fn(&ToolCallContext, CancellationToken) -> BoxFuture<'static, Option<BeforeToolCallResult>>
                + Send
                + Sync,
        >,
    >,

    /// afterToolCall 钩子
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
}

/// 从新提示启动 Agent 循环
/// 
/// 处理用户输入并启动完整的 Agent 交互循环
pub async fn run_agent_loop(
    prompts: Vec<AgentMessage>,
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    emit: &dyn Fn(AgentEvent),
    cancel: CancellationToken,
) -> anyhow::Result<Vec<AgentMessage>> {
    let mut new_messages = prompts.clone();

    // 将 prompts 添加到 context.messages
    context.messages.extend(prompts.clone());

    // 发出 before_agent_start（可被扩展拦截）
    emit(AgentEvent::BeforeAgentStart);

    // 发出 agent_start
    emit(AgentEvent::AgentStart);

    // 发出 turn_start
    emit(AgentEvent::TurnStart);

    // 为每个 prompt 发出 message_start 和 message_end
    for prompt in &prompts {
        emit(AgentEvent::MessageStart {
            message: prompt.clone(),
        });
        emit(AgentEvent::MessageEnd {
            message: prompt.clone(),
        });
    }

    // 进入主循环
    run_loop(context, &mut new_messages, config, emit, cancel.clone()).await?;

    // 发出 before_agent_end（可被扩展拦截）
    emit(AgentEvent::BeforeAgentEnd {
        messages: new_messages.clone(),
    });

    // 发出 agent_end
    emit(AgentEvent::AgentEnd {
        messages: new_messages.clone(),
    });

    Ok(new_messages)
}

/// 从已有消息继续循环（用于重试）
/// 
/// 在已有对话上下文基础上继续 Agent 循环
pub async fn run_agent_loop_continue(
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    emit: &dyn Fn(AgentEvent),
    cancel: CancellationToken,
) -> anyhow::Result<Vec<AgentMessage>> {
    let mut new_messages: Vec<AgentMessage> = Vec::new();

    // 检查上下文有效性
    if context.messages.is_empty() {
        anyhow::bail!("Cannot continue: no messages in context");
    }

    // 检查最后一条消息不是 assistant
    if let Some(AgentMessage::Llm(Message::Assistant(_))) = context.messages.last() {
        anyhow::bail!("Cannot continue from message role: assistant");
    }

    // 发出 before_agent_start（可被扩展拦截）
    emit(AgentEvent::BeforeAgentStart);

    // 发出 agent_start
    emit(AgentEvent::AgentStart);

    // 发出 turn_start
    emit(AgentEvent::TurnStart);

    // 进入主循环
    run_loop(context, &mut new_messages, config, emit, cancel.clone()).await?;

    // 发出 before_agent_end（可被扩展拦截）
    emit(AgentEvent::BeforeAgentEnd {
        messages: new_messages.clone(),
    });

    // 发出 agent_end
    emit(AgentEvent::AgentEnd {
        messages: new_messages.clone(),
    });

    Ok(new_messages)
}

/// 主循环逻辑
async fn run_loop(
    context: &mut AgentContext,
    new_messages: &mut Vec<AgentMessage>,
    config: &AgentLoopConfig,
    emit: &dyn Fn(AgentEvent),
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let mut first_turn = true;
    let mut pending_messages: Vec<AgentMessage> = config
        .get_steering_messages
        .as_ref()
        .map(|f| f())
        .unwrap_or_default();

    // 外层循环：处理后续消息
    loop {
        let mut has_more_tool_calls = true;

        // 内层循环：处理工具调用和转向消息
        while has_more_tool_calls || !pending_messages.is_empty() {
            if !first_turn {
                emit(AgentEvent::TurnStart);
            } else {
                first_turn = false;
            }

            // 处理待处理消息
            if !pending_messages.is_empty() {
                for message in &pending_messages {
                    emit(AgentEvent::MessageStart {
                        message: message.clone(),
                    });
                    emit(AgentEvent::MessageEnd {
                        message: message.clone(),
                    });
                    context.messages.push(message.clone());
                    new_messages.push(message.clone());
                }
                pending_messages.clear();
            }

            // 流式获取助手响应
            let message = stream_assistant_response(context, config, emit, cancel.clone()).await?;
            new_messages.push(message.clone());

            // 检查是否是错误/中止
            let stop_reason = match &message {
                AgentMessage::Llm(Message::Assistant(assistant)) => Some(assistant.stop_reason.clone()),
                _ => None,
            };

            if matches!(stop_reason, Some(StopReason::Error) | Some(StopReason::Aborted)) {
                emit(AgentEvent::TurnEnd {
                    message,
                    tool_results: Vec::new(),
                });
                // 发出 TurnError 事件
                if matches!(stop_reason, Some(StopReason::Error)) {
                    emit(AgentEvent::TurnError {
                        error: "Turn ended with error".to_string(),
                        turn_index: 0,  // TODO: 跟踪实际 turn 索引
                    });
                }
                emit(AgentEvent::BeforeAgentEnd {
                    messages: new_messages.clone(),
                });
                emit(AgentEvent::AgentEnd {
                    messages: new_messages.clone(),
                });
                return Ok(());
            }

            // 检查工具调用
            let tool_calls = extract_tool_calls(&message);
            has_more_tool_calls = !tool_calls.is_empty();

            let mut tool_results: Vec<ToolResultMessage> = Vec::new();
            if has_more_tool_calls {
                tool_results = execute_tool_calls(context, &message, tool_calls, config, emit, cancel.clone()).await?;

                for result in &tool_results {
                    let tool_result_msg = AgentMessage::Llm(Message::ToolResult(result.clone()));
                    context.messages.push(tool_result_msg.clone());
                    new_messages.push(tool_result_msg);
                }
            }

            emit(AgentEvent::TurnEnd {
                message,
                tool_results,
            });

            // 获取转向消息
            pending_messages = config
                .get_steering_messages
                .as_ref()
                .map(|f| f())
                .unwrap_or_default();
        }

        // 检查后续消息
        let follow_up = config
            .get_follow_up_messages
            .as_ref()
            .map(|f| f())
            .unwrap_or_default();

        if !follow_up.is_empty() {
            pending_messages = follow_up;
            continue;
        }

        // 没有更多消息，退出循环
        break;
    }

    Ok(())
}

/// 流式获取助手响应
async fn stream_assistant_response(
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    emit: &dyn Fn(AgentEvent),
    cancel: CancellationToken,
) -> anyhow::Result<AgentMessage> {
    // 应用上下文变换
    let messages = if let Some(transform) = &config.transform_context {
        transform(context.messages.clone(), cancel.clone()).await
    } else {
        context.messages.clone()
    };

    // 上下文窗口管理：检查使用情况并裁剪
    let mut messages = messages;
    if let Some(ref context_manager) = config.context_manager {
        let usage = context_manager.estimate_usage(&messages);

        // 发出警告
        if context_manager.should_warn(&usage) {
            emit(AgentEvent::ContextWarning {
                usage_percent: usage.usage_percent,
                total_tokens: usage.total_tokens,
                context_window: usage.context_window,
            });
        }

        // 如果需要裁剪
        if context_manager.needs_trimming(&usage) {
            let target_tokens = context_manager.context_window_size() - context_manager.reserve_for_output();
            context_manager.trim_messages(&mut messages, target_tokens);
        }
    }

    // 转换为 LLM 消息
    let llm_messages = (config.convert_to_llm)(&messages);

    // 转换工具为 LLM 格式
    let llm_tools: Vec<Tool> = context
        .tools
        .iter()
        .map(|t| agent_tool_to_llm_tool(t.as_ref()))
        .collect();

    // 构建 LLM 上下文
    let llm_context = Context {
        system_prompt: Some(context.system_prompt.clone()),
        messages: llm_messages,
        tools: if llm_tools.is_empty() {
            None
        } else {
            Some(llm_tools)
        },
    };

    // 解析 API key
    let api_key = config
        .get_api_key
        .as_ref()
        .and_then(|f| f(&format!("{:?}", config.model.provider)))
        .or_else(|| pi_ai::get_api_key_from_env(&config.model.provider));

    // 构建 StreamOptions
    let stream_options = StreamOptions {
        temperature: config.temperature.map(|t| t as f32),
        max_tokens: config.max_tokens.map(|t| t as u64),
        api_key,
        transport: config.transport.clone(),
        cache_retention: config.cache_retention.clone(),
        session_id: config.session_id.clone(),
        headers: None,
        max_retry_delay_ms: config.max_retry_delay_ms,
        metadata: None,
        retry_config: None,
    };

    // 调用 pi_ai::stream() 获取事件流
    let mut event_stream = pi_ai::stream(&llm_context, &config.model, &stream_options).await?;

    // 消费事件流
    #[allow(unused_assignments)] // final_message 在多个分支中被赋值
    let mut final_message: Option<AssistantMessage> = None;
    let mut current_partial: Option<AssistantMessage> = None;
    let mut started = false;

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let aborted = current_partial
                    .unwrap_or_else(|| {
                        AssistantMessage::new(
                            config.model.api.clone(),
                            config.model.provider.clone(),
                            &config.model.id,
                        )
                    })
                    .with_stop_reason(StopReason::Aborted);
                final_message = Some(aborted);
                break;
            }
            next = event_stream.next() => {
                match next {
                    Some(Ok(event)) => {
                        match &event {
                            AssistantMessageEvent::Start { partial } => {
                                current_partial = Some(partial.clone());
                                if !started {
                                    started = true;
                                    let agent_msg = AgentMessage::Llm(Message::Assistant(partial.clone()));
                                    emit(AgentEvent::MessageStart { message: agent_msg });
                                }
                            }
                            AssistantMessageEvent::Done { message, .. } => {
                                final_message = Some(message.clone());
                                break;
                            }
                            AssistantMessageEvent::Error { error, .. } => {
                                // 发出 MessageError 事件
                                emit(AgentEvent::MessageError {
                                    error: format!("Stream error: {:?}", error.error_message),
                                });
                                final_message = Some(error.clone());
                                break;
                            }
                            AssistantMessageEvent::TextStart { partial, .. }
                            | AssistantMessageEvent::TextDelta { partial, .. }
                            | AssistantMessageEvent::TextEnd { partial, .. }
                            | AssistantMessageEvent::ThinkingStart { partial, .. }
                            | AssistantMessageEvent::ThinkingDelta { partial, .. }
                            | AssistantMessageEvent::ThinkingEnd { partial, .. }
                            | AssistantMessageEvent::ToolCallStart { partial, .. }
                            | AssistantMessageEvent::ToolCallDelta { partial, .. }
                            | AssistantMessageEvent::ToolCallEnd { partial, .. } => {
                                current_partial = Some(partial.clone());
                            }
                        }

                        // 对每个非终结事件发出 MessageUpdate
                        if !matches!(event, AssistantMessageEvent::Done { .. } | AssistantMessageEvent::Error { .. }) {
                            if let Some(ref partial) = current_partial {
                                let agent_msg = AgentMessage::Llm(Message::Assistant(partial.clone()));
                                emit(AgentEvent::MessageUpdate {
                                    message: agent_msg,
                                    event: event.clone(),
                                });
                            }
                        }
                    }
                    Some(Err(e)) => {
                        tracing::error!("Stream error: {}", e);
                        let error_msg = AssistantMessage::new(
                            config.model.api.clone(),
                            config.model.provider.clone(),
                            &config.model.id,
                        )
                        .with_stop_reason(StopReason::Error)
                        .with_error_message(e.to_string());
                        final_message = Some(error_msg);
                        break;
                    }
                    None => {
                        let msg = current_partial.unwrap_or_else(|| {
                            AssistantMessage::new(
                                config.model.api.clone(),
                                config.model.provider.clone(),
                                &config.model.id,
                            )
                        });
                        final_message = Some(msg);
                        break;
                    }
                }
            }
        }
    }

    let assistant = final_message.expect("should have a final message after stream loop");
    let agent_message = AgentMessage::Llm(Message::Assistant(assistant));

    // 添加到 context
    context.messages.push(agent_message.clone());

    // 发出 MessageEnd
    emit(AgentEvent::MessageEnd {
        message: agent_message.clone(),
    });

    Ok(agent_message)
}

/// 从消息中提取工具调用
fn extract_tool_calls(message: &AgentMessage) -> Vec<ToolCall> {
    match message {
        AgentMessage::Llm(Message::Assistant(assistant)) => assistant
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolCall(tc) => Some(tc.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// 执行工具调用
async fn execute_tool_calls(
    context: &AgentContext,
    assistant_message: &AgentMessage,
    tool_calls: Vec<ToolCall>,
    config: &AgentLoopConfig,
    emit: &dyn Fn(AgentEvent),
    cancel: CancellationToken,
) -> anyhow::Result<Vec<ToolResultMessage>> {
    match config.tool_execution {
        ToolExecutionMode::Sequential => {
            execute_tools_sequential(context, assistant_message, tool_calls, config, emit, cancel).await
        }
        ToolExecutionMode::Parallel => {
            execute_tools_parallel(context, assistant_message, tool_calls, config, emit, cancel).await
        }
    }
}

/// 串行执行工具
async fn execute_tools_sequential(
    context: &AgentContext,
    assistant_message: &AgentMessage,
    tool_calls: Vec<ToolCall>,
    config: &AgentLoopConfig,
    emit: &dyn Fn(AgentEvent),
    cancel: CancellationToken,
) -> anyhow::Result<Vec<ToolResultMessage>> {
    let mut results = Vec::new();

    let assistant = match assistant_message {
        AgentMessage::Llm(Message::Assistant(a)) => a.clone(),
        _ => anyhow::bail!("Expected assistant message"),
    };

    for tool_call in tool_calls {
        // 发出 BeforeToolCall 事件（工具调用前，可拦截）
        emit(AgentEvent::BeforeToolCall {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            args: tool_call.arguments.clone(),
        });

        emit(AgentEvent::ToolExecutionStart {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            args: tool_call.arguments.clone(),
        });

        let (result_msg, is_error) = execute_tool_call(
            context,
            &assistant,
            &tool_call,
            config,
            emit,
            cancel.clone(),
        )
        .await?;

        // 发出 ToolExecutionEnd 事件
        emit(AgentEvent::ToolExecutionEnd {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            result: AgentToolResult {
                content: result_msg.content.clone(),
                details: result_msg.details.clone().unwrap_or_default(),
            },
            is_error,
        });

        // 如果有错误，发出 ToolError 事件
        if is_error {
            emit(AgentEvent::ToolError {
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                error: format!("{:?}", result_msg.content),
            });
        }

        // 发出 AfterToolCall 事件（工具调用后，可修改结果）
        emit(AgentEvent::AfterToolCall {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            result: AgentToolResult {
                content: result_msg.content.clone(),
                details: result_msg.details.clone().unwrap_or_default(),
            },
            is_error,
        });

        results.push(result_msg);
    }

    Ok(results)
}

/// 并行执行工具
async fn execute_tools_parallel(
    context: &AgentContext,
    assistant_message: &AgentMessage,
    tool_calls: Vec<ToolCall>,
    config: &AgentLoopConfig,
    emit: &dyn Fn(AgentEvent),
    cancel: CancellationToken,
) -> anyhow::Result<Vec<ToolResultMessage>> {
    let mut results = Vec::new();
    let mut pending_futures = Vec::new();

    let assistant = match assistant_message {
        AgentMessage::Llm(Message::Assistant(a)) => a.clone(),
        _ => anyhow::bail!("Expected assistant message"),
    };

    for tool_call in tool_calls {
        // 发出 BeforeToolCall 事件（工具调用前，可拦截）
        emit(AgentEvent::BeforeToolCall {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            args: tool_call.arguments.clone(),
        });

        emit(AgentEvent::ToolExecutionStart {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            args: tool_call.arguments.clone(),
        });

        // 准备工具调用
        let tool = context.tools.iter().find(|t| t.name() == tool_call.name);

        if tool.is_none() {
            // 工具未找到，创建错误结果
            let error_result = AgentToolResult::error(format!("Tool {} not found", tool_call.name));
            let tool_result = ToolResultMessage::new(
                &tool_call.id,
                &tool_call.name,
                error_result.content.clone(),
            )
            .with_error(true)
            .with_details(error_result.details.clone());

            emit(AgentEvent::ToolExecutionEnd {
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                result: error_result.clone(),
                is_error: true,
            });

            // 发出 AfterToolCall 事件
            emit(AgentEvent::AfterToolCall {
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                result: error_result,
                is_error: true,
            });

            results.push(tool_result);
            continue;
        }

        // 创建异步任务
        let tool = tool.unwrap().clone();
        let tool_call_clone = tool_call.clone();
        let assistant_clone = assistant.clone();
        let _config_clone = AgentContext {
            system_prompt: context.system_prompt.clone(),
            messages: context.messages.clone(),
            tools: context.tools.clone(),
        };
        let cancel_clone = cancel.clone();

        let future = async move {
            let ctx = ToolCallContext {
                assistant_message: assistant_clone,
                tool_call: tool_call_clone.clone(),
                args: tool_call_clone.arguments.clone(),
            };

            // 执行工具
            let result = tool
                .execute(
                    &tool_call_clone.id,
                    tool_call_clone.arguments.clone(),
                    cancel_clone.clone(),
                    None,
                )
                .await;

            (tool_call_clone, result, ctx)
        };

        pending_futures.push(future);
    }

    // 等待所有工具执行完成
    let executions = futures::future::join_all(pending_futures).await;

    for (tool_call, result, ctx) in executions {
        let (tool_result, _is_error) = match result {
            Ok(agent_result) => {
                // 应用 afterToolCall 钩子
                let (final_result, final_is_error) = if let Some(after_hook) = &config.after_tool_call {
                    if let Some(after_result) = after_hook(&ctx, &agent_result, false, cancel.clone()).await {
                        let content = after_result.content.unwrap_or(agent_result.content);
                        let details = after_result.details.unwrap_or(agent_result.details);
                        let is_err = after_result.is_error.unwrap_or(false);
                        (
                            AgentToolResult { content, details },
                            is_err,
                        )
                    } else {
                        (agent_result, false)
                    }
                } else {
                    (agent_result, false)
                };

                emit(AgentEvent::ToolExecutionEnd {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    result: final_result.clone(),
                    is_error: final_is_error,
                });

                // 发出 AfterToolCall 事件
                emit(AgentEvent::AfterToolCall {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    result: final_result.clone(),
                    is_error: final_is_error,
                });

                let msg = ToolResultMessage::new(&tool_call.id, &tool_call.name, final_result.content)
                    .with_error(final_is_error)
                    .with_details(final_result.details);

                (msg, final_is_error)
            }
            Err(e) => {
                let error_result = AgentToolResult::error(e.to_string());

                emit(AgentEvent::ToolExecutionEnd {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    result: error_result.clone(),
                    is_error: true,
                });

                // 发出 AfterToolCall 事件
                emit(AgentEvent::AfterToolCall {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    result: error_result.clone(),
                    is_error: true,
                });

                let msg = ToolResultMessage::new(&tool_call.id, &tool_call.name, error_result.content)
                    .with_error(true)
                    .with_details(error_result.details);

                (msg, true)
            }
        };

        // 发出 tool result 消息事件
        let agent_msg = AgentMessage::Llm(Message::ToolResult(tool_result.clone()));
        emit(AgentEvent::MessageStart {
            message: agent_msg.clone(),
        });
        emit(AgentEvent::MessageEnd {
            message: agent_msg,
        });

        results.push(tool_result);
    }

    Ok(results)
}

/// 执行单个工具调用
async fn execute_tool_call(
    context: &AgentContext,
    assistant_message: &AssistantMessage,
    tool_call: &ToolCall,
    config: &AgentLoopConfig,
    emit: &dyn Fn(AgentEvent),
    cancel: CancellationToken,
) -> anyhow::Result<(ToolResultMessage, bool)> {
    // 查找工具
    let tool = context.tools.iter().find(|t| t.name() == tool_call.name);

    if let Some(tool) = tool {
        let tool = tool.clone();
        let ctx = ToolCallContext {
            assistant_message: assistant_message.clone(),
            tool_call: tool_call.clone(),
            args: tool_call.arguments.clone(),
        };

        // 应用 beforeToolCall 钩子
        if let Some(before_hook) = &config.before_tool_call {
            if let Some(before_result) = before_hook(&ctx, cancel.clone()).await {
                if before_result.block {
                    let error_result = AgentToolResult::error(
                        before_result.reason.unwrap_or_else(|| "Tool execution was blocked".to_string()),
                    );

                    emit(AgentEvent::ToolExecutionEnd {
                        tool_call_id: tool_call.id.clone(),
                        tool_name: tool_call.name.clone(),
                        result: error_result.clone(),
                        is_error: true,
                    });

                    let msg = ToolResultMessage::new(&tool_call.id, &tool_call.name, error_result.content)
                        .with_error(true)
                        .with_details(error_result.details);

                    // 发出 tool result 消息事件
                    let agent_msg = AgentMessage::Llm(Message::ToolResult(msg.clone()));
                    emit(AgentEvent::MessageStart {
                        message: agent_msg.clone(),
                    });
                    emit(AgentEvent::MessageEnd { message: agent_msg });

                    return Ok((msg, true));
                }
            }
        }

        // 执行工具
        let result = tool
            .execute(&tool_call.id, tool_call.arguments.clone(), cancel.clone(), None)
            .await;

        match result {
            Ok(agent_result) => {
                // 应用 afterToolCall 钩子
                let (final_result, final_is_error) = if let Some(after_hook) = &config.after_tool_call {
                    if let Some(after_result) = after_hook(&ctx, &agent_result, false, cancel.clone()).await {
                        let content = after_result.content.unwrap_or(agent_result.content);
                        let details = after_result.details.unwrap_or(agent_result.details);
                        let is_err = after_result.is_error.unwrap_or(false);
                        (
                            AgentToolResult { content, details },
                            is_err,
                        )
                    } else {
                        (agent_result, false)
                    }
                } else {
                    (agent_result, false)
                };

                emit(AgentEvent::ToolExecutionEnd {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    result: final_result.clone(),
                    is_error: final_is_error,
                });

                let msg = ToolResultMessage::new(&tool_call.id, &tool_call.name, final_result.content)
                    .with_error(final_is_error)
                    .with_details(final_result.details);

                // 发出 tool result 消息事件
                let agent_msg = AgentMessage::Llm(Message::ToolResult(msg.clone()));
                emit(AgentEvent::MessageStart {
                    message: agent_msg.clone(),
                });
                emit(AgentEvent::MessageEnd { message: agent_msg });

                Ok((msg, final_is_error))
            }
            Err(e) => {
                let error_result = AgentToolResult::error(e.to_string());

                emit(AgentEvent::ToolExecutionEnd {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    result: error_result.clone(),
                    is_error: true,
                });

                let msg = ToolResultMessage::new(&tool_call.id, &tool_call.name, error_result.content)
                    .with_error(true)
                    .with_details(error_result.details);

                // 发出 tool result 消息事件
                let agent_msg = AgentMessage::Llm(Message::ToolResult(msg.clone()));
                emit(AgentEvent::MessageStart {
                    message: agent_msg.clone(),
                });
                emit(AgentEvent::MessageEnd { message: agent_msg });

                Ok((msg, true))
            }
        }
    } else {
        // 工具未找到
        let error_result = AgentToolResult::error(format!("Tool {} not found", tool_call.name));

        emit(AgentEvent::ToolExecutionEnd {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            result: error_result.clone(),
            is_error: true,
        });

        let msg = ToolResultMessage::new(&tool_call.id, &tool_call.name, error_result.content)
            .with_error(true)
            .with_details(error_result.details);

        // 发出 tool result 消息事件
        let agent_msg = AgentMessage::Llm(Message::ToolResult(msg.clone()));
        emit(AgentEvent::MessageStart {
            message: agent_msg.clone(),
        });
        emit(AgentEvent::MessageEnd { message: agent_msg });

        Ok((msg, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::fixtures::*;
    use crate::default_convert_to_llm;

    #[test]
    fn test_agent_tool_to_llm_tool_conversion() {
        let mock_tool = MockTool::new("test_tool", "test result");
        let llm_tool = agent_tool_to_llm_tool(&mock_tool);
        
        assert_eq!(llm_tool.name, "test_tool");
        assert_eq!(llm_tool.description, "Mock tool for testing");
        assert!(llm_tool.parameters.is_object());
        
        // 验证参数结构
        let params = llm_tool.parameters.as_object().unwrap();
        assert!(params.contains_key("type"));
        assert!(params.contains_key("properties"));
        assert!(params.contains_key("required"));
    }

    #[test]
    fn test_extract_tool_calls_from_assistant_message() {
        // 创建包含工具调用的消息
        let tool_call = ToolCall::new("call_123", "search", serde_json::json!({"query": "test"}));
        let assistant = AssistantMessage::new(Api::Anthropic, Provider::Anthropic, "claude-3-sonnet")
            .with_content(vec![ContentBlock::ToolCall(tool_call.clone())]);
        let message = AgentMessage::Llm(Message::Assistant(assistant));
        
        let tool_calls = extract_tool_calls(&message);
        
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_123");
        assert_eq!(tool_calls[0].name, "search");
    }

    #[test]
    fn test_extract_tool_calls_from_non_assistant_message() {
        // 从用户消息中提取应该返回空
        let message = AgentMessage::user("Hello");
        let tool_calls = extract_tool_calls(&message);
        
        assert!(tool_calls.is_empty());
    }

    #[test]
    fn test_extract_tool_calls_no_tool_calls() {
        // 助手消息但没有工具调用
        let assistant = AssistantMessage::new(Api::Anthropic, Provider::Anthropic, "claude-3-sonnet")
            .with_content(vec![ContentBlock::Text(TextContent::new("Hello"))]);
        let message = AgentMessage::Llm(Message::Assistant(assistant));
        
        let tool_calls = extract_tool_calls(&message);
        
        assert!(tool_calls.is_empty());
    }

    #[test]
    fn test_agent_loop_config_creation() {
        let model = sample_agent_state().model;
        let config = AgentLoopConfig {
            model,
            thinking_level: ThinkingLevel::Off,
            thinking_budgets: None,
            temperature: None,
            max_tokens: None,
            transport: None,
            cache_retention: None,
            session_id: None,
            max_retry_delay_ms: None,
            context_manager: None,
            convert_to_llm: Arc::new(default_convert_to_llm),
            transform_context: None,
            get_api_key: None,
            get_steering_messages: None,
            get_follow_up_messages: None,
            tool_execution: ToolExecutionMode::Parallel,
            before_tool_call: None,
            after_tool_call: None,
        };
        
        // 验证配置创建成功
        assert_eq!(config.thinking_level, ThinkingLevel::Off);
        assert_eq!(config.tool_execution, ToolExecutionMode::Parallel);
        assert!(config.temperature.is_none());
        assert!(config.max_tokens.is_none());
    }

    #[test]
    fn test_agent_loop_config_with_tools() {
        let model = sample_agent_state().model;
        let _tools = sample_mock_tools();
        
        let config = AgentLoopConfig {
            model,
            thinking_level: ThinkingLevel::Medium,
            thinking_budgets: None,
            temperature: Some(0.7),
            max_tokens: Some(4096),
            transport: None,
            cache_retention: None,
            session_id: Some("test-session".to_string()),
            max_retry_delay_ms: Some(5000),
            context_manager: None,
            convert_to_llm: Arc::new(default_convert_to_llm),
            transform_context: None,
            get_api_key: None,
            get_steering_messages: None,
            get_follow_up_messages: None,
            tool_execution: ToolExecutionMode::Sequential,
            before_tool_call: None,
            after_tool_call: None,
        };
        
        assert_eq!(config.thinking_level, ThinkingLevel::Medium);
        assert_eq!(config.tool_execution, ToolExecutionMode::Sequential);
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.max_tokens, Some(4096));
        assert_eq!(config.session_id, Some("test-session".to_string()));
        assert_eq!(config.max_retry_delay_ms, Some(5000));
    }

    #[test]
    fn test_default_convert_to_llm_in_config() {
        let messages = vec![
            AgentMessage::user("Hello"),
            AgentMessage::user("World"),
        ];
        
        let convert_fn = default_convert_to_llm;
        let llm_messages = convert_fn(&messages);
        
        assert_eq!(llm_messages.len(), 2);
    }

    #[test]
    fn test_tool_execution_mode_enum() {
        assert_eq!(ToolExecutionMode::Sequential as i32, ToolExecutionMode::Sequential as i32);
        assert_eq!(ToolExecutionMode::Parallel as i32, ToolExecutionMode::Parallel as i32);
        assert_ne!(
            std::mem::discriminant(&ToolExecutionMode::Sequential),
            std::mem::discriminant(&ToolExecutionMode::Parallel)
        );
    }

    #[tokio::test]
    async fn test_mock_tool_execution() {
        let mock_tool = MockTool::new("test_tool", "test result");
        let result = mock_tool.execute(
            "call_1",
            serde_json::json!({"input": "test"}),
            CancellationToken::new(),
            None,
        ).await;
        
        assert!(result.is_ok());
        let tool_result = result.unwrap();
        assert!(!tool_result.content.is_empty());
        
        // 验证内容包含预期的结果
        let text_content = tool_result.content.iter()
            .filter_map(|c| if let ContentBlock::Text(t) = c { Some(t.text.clone()) } else { None })
            .collect::<String>();
        assert_eq!(text_content, "test result");
    }

    #[tokio::test]
    async fn test_mock_error_tool_execution() {
        let error_tool = MockErrorTool::new("error_tool", "Something went wrong");
        let result = error_tool.execute(
            "call_1",
            serde_json::json!({}),
            CancellationToken::new(),
            None,
        ).await;
        
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Something went wrong"));
    }

    #[tokio::test]
    async fn test_mock_tool_with_cancellation() {
        let mock_tool = MockTool::new("test_tool", "result");
        let cancel = CancellationToken::new();
        
        // 取消 token
        cancel.cancel();
        
        // MockTool 不检查取消状态，所以仍然会返回结果
        // 这个测试主要验证接口兼容性
        let result = mock_tool.execute(
            "call_1",
            serde_json::json!({"input": "test"}),
            cancel,
            None,
        ).await;
        
        // MockTool 的实现不检查取消，所以应该成功
        assert!(result.is_ok());
    }

    #[test]
    fn test_agent_context_creation() {
        let context = AgentContext {
            system_prompt: "You are a helpful assistant".to_string(),
            messages: vec![AgentMessage::user("Hello")],
            tools: vec![],
        };
        
        assert_eq!(context.system_prompt, "You are a helpful assistant");
        assert_eq!(context.messages.len(), 1);
        assert!(context.tools.is_empty());
    }

    #[test]
    fn test_agent_context_clone() {
        let context = AgentContext {
            system_prompt: "Test".to_string(),
            messages: vec![AgentMessage::user("Hello")],
            tools: vec![],
        };
        
        let cloned = context.clone();
        assert_eq!(cloned.system_prompt, context.system_prompt);
        assert_eq!(cloned.messages.len(), context.messages.len());
    }

    #[test]
    fn test_agent_context_debug() {
        let context = AgentContext {
            system_prompt: "Test".to_string(),
            messages: vec![],
            tools: vec![],
        };
        
        let debug_str = format!("{:?}", context);
        assert!(debug_str.contains("AgentContext"));
        assert!(debug_str.contains("Test"));
    }

    #[test]
    fn test_empty_tool_list_handling() {
        // 测试空工具列表的处理
        let context = AgentContext {
            system_prompt: "You are helpful".to_string(),
            messages: vec![AgentMessage::user("Hello")],
            tools: vec![], // 空工具列表
        };
        
        assert!(context.tools.is_empty());
        assert_eq!(context.messages.len(), 1);
    }

    #[test]
    fn test_message_boundary_conditions() {
        // 测试消息边界条件
        let mut messages = vec![];
        
        // 添加大量消息测试边界
        for i in 0..100 {
            messages.push(AgentMessage::user(&format!("Message {}", i)));
        }
        
        let context = AgentContext {
            system_prompt: "Test".to_string(),
            messages: messages.clone(),
            tools: vec![],
        };
        
        assert_eq!(context.messages.len(), 100);
        
        // 测试消息转换
        let llm_messages = default_convert_to_llm(&messages);
        assert_eq!(llm_messages.len(), 100);
    }

    #[test]
    fn test_agent_message_variants() {
        // 测试不同类型的 AgentMessage
        let user_msg = AgentMessage::user("User message");
        
        // 验证消息类型 - AgentMessage 包装的是 Llm(Message)
        match &user_msg {
            AgentMessage::Llm(Message::User(_)) => {},
            _ => panic!("Expected User message wrapped in Llm"),
        }
        
        // 验证 user 方法创建的消息内容
        if let AgentMessage::Llm(Message::User(user)) = &user_msg {
            match &user.content {
                UserContent::Text(text) => assert_eq!(text, "User message"),
                _ => panic!("Expected text content"),
            }
        }
    }

    #[test]
    fn test_agent_loop_config_default() {
        use pi_ai::ModelCost;
        
        // 测试 AgentLoopConfig 的默认值
        let config = AgentLoopConfig {
            model: Model {
                id: "test-model".to_string(),
                name: "Test Model".to_string(),
                api: Api::Anthropic,
                provider: Provider::Anthropic,
                base_url: "https://test.com".to_string(),
                reasoning: false,
                input: vec![InputModality::Text],
                cost: ModelCost {
                    input: 0.0,
                    output: 0.0,
                    cache_read: None,
                    cache_write: None,
                },
                context_window: 100000,
                max_tokens: 4096,
                headers: None,
                compat: None,
            },
            thinking_level: ThinkingLevel::Off,
            thinking_budgets: None,
            temperature: None,
            max_tokens: None,
            transport: None,
            cache_retention: None,
            session_id: None,
            max_retry_delay_ms: None,
            context_manager: None,
            convert_to_llm: Arc::new(default_convert_to_llm),
            transform_context: None,
            get_api_key: None,
            get_steering_messages: None,
            get_follow_up_messages: None,
            tool_execution: ToolExecutionMode::Sequential,
            before_tool_call: None,
            after_tool_call: None,
        };
        
        assert_eq!(config.model.id, "test-model");
        assert_eq!(config.thinking_level, ThinkingLevel::Off);
        assert_eq!(config.tool_execution, ToolExecutionMode::Sequential);
    }

    #[test]
    fn test_tool_execution_mode_variants() {
        // 测试工具执行模式的所有变体
        let sequential = ToolExecutionMode::Sequential;
        let parallel = ToolExecutionMode::Parallel;
        
        // 验证它们是不同的值
        assert_ne!(
            std::mem::discriminant(&sequential),
            std::mem::discriminant(&parallel)
        );
    }

    #[test]
    fn test_thinking_level_variants() {
        // 测试所有 ThinkingLevel 变体
        let levels = [
            ThinkingLevel::Off,
            ThinkingLevel::Minimal,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::XHigh,
        ];
        
        // 验证每个变体都是唯一的
        for i in 0..levels.len() {
            for j in (i+1)..levels.len() {
                assert_ne!(
                    std::mem::discriminant(&levels[i]),
                    std::mem::discriminant(&levels[j]),
                    "ThinkingLevel variants should be unique"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_empty_context_handling() {
        // 测试空上下文处理
        let context = AgentContext {
            system_prompt: "".to_string(),
            messages: vec![],
            tools: vec![],
        };
        
        assert!(context.messages.is_empty());
        assert!(context.system_prompt.is_empty());
        
        // 转换空消息列表
        let llm_messages = default_convert_to_llm(&context.messages);
        assert!(llm_messages.is_empty());
    }
}
