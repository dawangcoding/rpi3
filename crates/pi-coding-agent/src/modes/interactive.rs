//! 交互模式 - TUI 集成的交互式会话
//!
//! 使用 Markdown 渲染、流式差分更新和状态栏。
//! 输入使用 pi-tui 的 Editor 组件。

use std::io::Write;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use std::path::PathBuf;
use pi_agent::types::*;
use pi_ai::types::*;
use pi_tui::terminal::{ProcessTerminal, Terminal};
use pi_tui::components::editor::{Editor, EditorConfig};
use pi_tui::autocomplete::{AutocompleteProvider, AutocompleteSuggestions, SlashCommand, SlashCommandProvider};
use pi_tui::tui::{Component, Focusable};
use crate::core::agent_session::{AgentSession, AgentSessionConfig};
use crate::core::export::HtmlExporter;
use crate::core::session_manager::SessionManager;
use crate::core::auth::{TokenStorage, get_oauth_provider, run_oauth_flow};
use crate::config::AppConfig;
use super::input_history::InputHistory;
use super::autocomplete_providers::{FileAutocompleteProvider, ModelAutocompleteProvider};
use super::interactive_components::{StreamingBlock, render_editor_area, render_status_and_editor};
use super::message_history::MessageHistory;
use super::message_components::StatusBarComponent;
use super::theme::Theme;

/// 最小渲染间隔 (~60fps = 16ms)
const MIN_RENDER_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

/// 交互模式配置
pub struct InteractiveConfig {
    pub model: Model,
    pub thinking_level: ThinkingLevel,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub context_files: Vec<String>,
    pub cwd: std::path::PathBuf,
    pub no_bash: bool,
    pub no_edit: bool,
    pub app_config: AppConfig,
    pub initial_prompt: Option<String>,
}

/// CodingAgent 自动完成提供者
/// 支持 slash 命令补全、@文件路径补全和模型名称补全
struct CodingAgentAutocompleteProvider {
    slash_provider: SlashCommandProvider,
    file_provider: FileAutocompleteProvider,
    model_provider: ModelAutocompleteProvider,
}

impl CodingAgentAutocompleteProvider {
    fn new(cwd: PathBuf) -> Self {
        let mut slash_provider = SlashCommandProvider::new();
        slash_provider.add_command(SlashCommand::new("help", "Show help information"));
        slash_provider.add_command(SlashCommand::new("clear", "Clear the conversation history"));
        slash_provider.add_command(SlashCommand::new("model", "Show or change the current model"));
        slash_provider.add_command(SlashCommand::new("exit", "Exit the application").with_alias("quit"));
        slash_provider.add_command(SlashCommand::new("stats", "Show session statistics"));
        slash_provider.add_command(SlashCommand::new("save", "Save the current session"));
        slash_provider.add_command(SlashCommand::new("fork", "Fork the current session at a message index"));
        slash_provider.add_command(SlashCommand::new("export", "Export session to HTML file").with_alias("export-html"));
        slash_provider.add_command(SlashCommand::new("compact", "Compact conversation history to save context space"));
        slash_provider.add_command(SlashCommand::new("extensions", "List loaded extensions"));
        slash_provider.add_command(SlashCommand::new("login", "Login with OAuth provider (anthropic, github-copilot)"));
        slash_provider.add_command(SlashCommand::new("logout", "Logout from OAuth provider"));
        slash_provider.add_command(SlashCommand::new("auth", "Show current authentication status"));
        slash_provider.add_command(SlashCommand::new("theme", "Switch color theme (dark, light)"));
        Self {
            slash_provider,
            file_provider: FileAutocompleteProvider::new(cwd),
            model_provider: ModelAutocompleteProvider::new(),
        }
    }
}

impl AutocompleteProvider for CodingAgentAutocompleteProvider {
    fn provide(&self, input: &str, cursor_pos: usize) -> Option<AutocompleteSuggestions> {
        // 优先级: model > slash > file
        if let Some(suggestions) = self.model_provider.provide(input, cursor_pos) {
            return Some(suggestions);
        }
        if let Some(suggestions) = self.slash_provider.provide(input, cursor_pos) {
            return Some(suggestions);
        }
        if let Some(suggestions) = self.file_provider.provide(input, cursor_pos) {
            return Some(suggestions);
        }
        None
    }
}

/// 运行交互模式
pub async fn run(config: InteractiveConfig) -> anyhow::Result<()> {
    // 1. 初始化终端
    let mut terminal = ProcessTerminal::new();
    terminal.enable_raw_mode()?;
    let mut stdout = std::io::stdout();

    // 启用 bracketed paste mode
    write!(stdout, "\x1b[?2004h")?;
    stdout.flush()?;

    // 2. 创建 AgentSession
    let session = AgentSession::new(AgentSessionConfig {
        model: config.model.clone(),
        thinking_level: config.thinking_level.clone(),
        system_prompt: config.system_prompt,
        append_system_prompt: config.append_system_prompt,
        context_files: config.context_files,
        cwd: config.cwd.clone(),
        no_bash: config.no_bash,
        no_edit: config.no_edit,
        app_config: config.app_config,
        session_id: None,
    }).await?;

    // 3. 设置事件通道（agent 事件 -> UI 更新）
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let tx = event_tx.clone();
    session.agent().subscribe(Arc::new(move |event: AgentEvent, _cancel| {
        let _ = tx.send(event);
    }));

    // 4. 设置 stdin 异步读取
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 1024];
        loop {
            match stdin.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if input_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // 5. 创建组件
    let mut message_history = MessageHistory::new();
    let mut status_bar = StatusBarComponent::new();
    let mut input_history = InputHistory::new(100);
    status_bar.set_model(config.model.name.clone());
    
    let mut editor = Editor::new(EditorConfig {
        placeholder: Some("> Ask anything...".to_string()),
        max_lines: None,
        read_only: false,
        line_numbers: false,
        wrap: true,
    });
    editor.set_focused(true);
    
    // 设置自动完成提供者
    let autocomplete_provider = CodingAgentAutocompleteProvider::new(config.cwd.clone());
    editor.set_autocomplete_provider(Box::new(autocomplete_provider));
    
    // StreamingBlock 仍保留用于差分渲染当前流式消息
    let mut streaming = StreamingBlock::new();
    let mut is_streaming = false;
    let mut should_exit = false;
    let mut last_render_time = Instant::now();
    let mut _current_theme = Theme::dark(); // 暂时下划线前缀，因为尚未在组件中使用
    let mut paste_buffer: Option<String> = None;

    // 6. 添加欢迎信息到消息历史
    message_history.add_system_message(format!(
        "pi v0.1.0 | Model: {} | Thinking: {:?}",
        config.model.name, config.thinking_level
    ));
    message_history.add_system_message(
        "Type your message and press Enter to send. Ctrl+C to cancel, Ctrl+D to exit.".to_string()
    );
    message_history.add_system_message(
        "Use Shift+Enter for new line. /help for commands.".to_string()
    );
    
    // 7. 初始渲染
    let (term_width, _) = terminal.size();
    render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;

    // 8. 如果有初始 prompt，直接发送
    if let Some(prompt) = &config.initial_prompt {
        message_history.add_user_message(prompt.clone());
        // 重新渲染以显示用户消息
        let (term_width, _) = terminal.size();
        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
        session.prompt_text(prompt).await?;
    }

    // 9. 主事件循环

    loop {
        tokio::select! {
            // 处理 Agent 事件
            Some(event) = event_rx.recv() => {
                let (term_width, _) = terminal.size();

                match event {
                    AgentEvent::AgentStart => {
                        is_streaming = true;
                        streaming = StreamingBlock::new();
                        let _assistant = message_history.start_assistant_message();
                        status_bar.set_loading(true);
                    }
                    AgentEvent::MessageStart { .. } => {
                        // 消息流开始，准备接收 delta
                    }
                    AgentEvent::MessageUpdate { event: msg_event, .. } => {
                        match msg_event {
                            AssistantMessageEvent::TextDelta { delta, .. } => {
                                // 更新消息组件
                                if let Some(assistant) = message_history.current_streaming() {
                                    assistant.push_text(&delta);
                                }
                                // 也更新 StreamingBlock 用于流式差分渲染
                                streaming.push_text(&delta);
                                // 渲染节流
                                let now = Instant::now();
                                if now.duration_since(last_render_time) >= MIN_RENDER_INTERVAL {
                                    let update = streaming.diff_update(term_width);
                                    write!(stdout, "{}", update)?;
                                    stdout.flush()?;
                                    last_render_time = now;
                                }
                            }
                            AssistantMessageEvent::ThinkingDelta { delta, .. } => {
                                if let Some(assistant) = message_history.current_streaming() {
                                    assistant.push_thinking(&delta);
                                }
                                streaming.push_thinking(&delta);
                                let now = Instant::now();
                                if now.duration_since(last_render_time) >= MIN_RENDER_INTERVAL {
                                    let update = streaming.diff_update(term_width);
                                    write!(stdout, "{}", update)?;
                                    stdout.flush()?;
                                    last_render_time = now;
                                }
                            }
                            AssistantMessageEvent::ToolCallEnd { tool_call, .. } => {
                                if let Some(assistant) = message_history.current_streaming() {
                                    assistant.add_tool_call(tool_call.name.clone(), tool_call.id.clone());
                                }
                                // flush streaming
                                if streaming.has_content() {
                                    let update = streaming.diff_update(term_width);
                                    write!(stdout, "{}", update)?;
                                }
                                write!(stdout, "\r\n\x1b[33m⚡ Tool: {} ({})\x1b[0m",
                                    tool_call.name, tool_call.id)?;
                                stdout.flush()?;
                                streaming.finish();
                                last_render_time = Instant::now();
                            }
                            _ => {}
                        }
                    }
                    AgentEvent::MessageEnd { .. } => {
                        // 最终渲染一次确保完整（强制渲染，无视节流）
                        if streaming.has_content() {
                            let (term_w, _) = terminal.size();
                            let update = streaming.diff_update(term_w);
                            write!(stdout, "{}", update)?;
                        }
                        streaming.finish();
                        write!(stdout, "\r\n")?;
                        stdout.flush()?;
                        last_render_time = Instant::now();
                    }
                    AgentEvent::ToolExecutionStart { tool_name, .. } => {
                        write!(stdout, "\x1b[2m  Running {}...\x1b[0m\r\n", tool_name)?;
                        stdout.flush()?;
                    }
                    AgentEvent::ToolExecutionEnd { tool_name, is_error, .. } => {
                        if let Some(assistant) = message_history.current_streaming() {
                            // 更新最后一个匹配 tool_name 且仍在运行的工具调用
                            assistant.update_last_tool_call(&tool_name, is_error, None);
                        }
                        if is_error {
                            write!(stdout, "\x1b[31m  ✗ {} failed\x1b[0m\r\n", tool_name)?;
                        } else {
                            write!(stdout, "\x1b[32m  ✓ {} done\x1b[0m\r\n", tool_name)?;
                        }
                        stdout.flush()?;
                    }
                    AgentEvent::TurnEnd { .. } => {
                        // Turn 结束
                    }
                    AgentEvent::AgentEnd { .. } => {
                        is_streaming = false;
                        status_bar.set_loading(false);
                        // 完成当前消息
                        message_history.finish_assistant_message();
                        // 最终 flush
                        if streaming.has_content() {
                            let update = streaming.diff_update(term_width);
                            write!(stdout, "{}", update)?;
                        }
                        streaming.finish();
                        write!(stdout, "\r\n")?;
                        // 渲染状态栏和输入区域
                        let stats = session.stats().await;
                        status_bar.set_tokens(stats.tokens.total as usize, 200000); // context window
                        let (term_width, _) = terminal.size();
                        render_status_and_editor(&status_bar, &editor, term_width, &mut stdout)?;
                    }
                    _ => {}
                }
            }

            // 处理键盘输入
            Some(data) = input_rx.recv() => {
                if is_streaming {
                    // 流式输出时只处理 Ctrl+C 中断
                    let text = String::from_utf8_lossy(&data);
                    if text.contains('\x03') {
                        session.abort().await;
                        streaming.finish();
                        write!(stdout, "\r\n\x1b[33m[cancelled]\x1b[0m\r\n\r\n")?;
                        is_streaming = false;
                        status_bar.set_loading(false);
                        message_history.finish_assistant_message();
                        let (term_width, _) = terminal.size();
                        render_status_and_editor(&status_bar, &editor, term_width, &mut stdout)?;
                    }
                    continue;
                }

                let text = String::from_utf8_lossy(&data);

                // 粘贴模式检测
                if text.contains("\x1b[200~") {
                    // 粘贴开始
                    let start_idx = text.find("\x1b[200~").unwrap();
                    let content_start = start_idx + 6; // "\x1b[200~" 长度为 6

                    if let Some(end_idx) = text.find("\x1b[201~") {
                        // 粘贴在单个数据包中完成
                        let pasted = &text[content_start..end_idx];
                        handle_paste(&pasted.to_string(), &mut editor, &mut stdout, &terminal)?;
                    } else {
                        // 粘贴跨多个数据包，开始缓冲
                        paste_buffer = Some(text[content_start..].to_string());
                    }
                    continue;
                }

                if let Some(ref mut buffer) = paste_buffer {
                    if let Some(end_idx) = text.find("\x1b[201~") {
                        // 粘贴结束
                        buffer.push_str(&text[..end_idx]);
                        let pasted = buffer.clone();
                        paste_buffer = None;
                        handle_paste(&pasted, &mut editor, &mut stdout, &terminal)?;
                    } else {
                        // 继续缓冲
                        buffer.push_str(&text);
                    }
                    continue;
                }
                
                // 检查 Ctrl+C - 取消/清空
                if text.contains('\x03') {
                    editor.set_text("");
                    let (term_width, _) = terminal.size();
                    render_editor_area(&editor, term_width, &mut stdout)?;
                    continue;
                }
                
                // 检查 Ctrl+D - 退出（仅在编辑器为空时）
                if text.contains('\x04') && editor.is_empty() {
                    should_exit = true;
                    continue;
                }
                
                // 检查是否是 Enter 键（需要特殊处理提交）
                // Enter 键的序列：\r, \n, \r\n, 或 Kitty 协议的 CSI 序列
                let is_enter = text == "\r" || text == "\n" || text == "\r\n";
                let is_shift_enter = text == "\x1b\r" || text == "\x1b\n"; // Shift+Enter 在 Kitty 协议下
                
                if is_enter && !is_shift_enter && !editor.is_empty() {
                    // 提交输入
                    let prompt = editor.get_text().trim().to_string();
                    input_history.push(prompt.clone());
                    editor.set_text("");
                    write!(stdout, "\r\n")?;
                    
                    // 特殊命令处理
                    if prompt == "/exit" || prompt == "/quit" {
                        should_exit = true;
                    } else if prompt == "/clear" {
                        // 清空消息历史
                        message_history.clear();
                        // 重新添加欢迎信息
                        message_history.add_system_message(format!(
                            "pi v0.1.0 | Model: {} | Thinking: {:?}",
                            config.model.name, config.thinking_level
                        ));
                        message_history.add_system_message("Conversation cleared.".to_string());
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/model" {
                        message_history.add_system_message(format!("Current model: {}", config.model.name));
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/stats" {
                        let stats = session.stats().await;
                        message_history.add_system_message("Session stats:".to_string());
                        message_history.add_system_message(format!(
                            "  Messages: {} user, {} assistant",
                            stats.user_messages, stats.assistant_messages
                        ));
                        message_history.add_system_message(format!(
                            "  Tool calls: {}", stats.tool_calls
                        ));
                        message_history.add_system_message(format!(
                            "  Tokens: {} total ({} in, {} out)",
                            stats.tokens.total, stats.tokens.input, stats.tokens.output
                        ));
                        message_history.add_system_message(format!(
                            "  Cost: ${:.4}", stats.cost
                        ));
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/save" {
                        match session.save().await {
                            Ok(()) => message_history.add_system_message("Session saved.".to_string()),
                            Err(e) => message_history.add_system_message(format!("Failed to save: {}", e)),
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/fork" || prompt.starts_with("/fork ") {
                        // 解析可选的消息索引参数
                        let fork_at_index = if prompt.len() > 6 {
                            match prompt[6..].trim().parse::<usize>() {
                                Ok(index) => Some(index),
                                Err(_) => {
                                    message_history.add_system_message("Invalid index. Usage: /fork or /fork N".to_string());
                                    let (term_width, _) = terminal.size();
                                    render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                                    continue;
                                }
                            }
                        } else {
                            None
                        };
                        
                        // 先保存当前会话
                        if let Err(e) = session.save().await {
                            message_history.add_system_message(format!("Failed to save session: {}", e));
                            let (term_width, _) = terminal.size();
                            render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                            continue;
                        }
                        
                        // 执行 fork
                        match session.fork(fork_at_index).await {
                            Ok(new_session_id) => {
                                if let Some(index) = fork_at_index {
                                    message_history.add_system_message(format!("Forked at message {}. New session ID: {}", index, new_session_id));
                                } else {
                                    message_history.add_system_message(format!("Forked session. New session ID: {}", new_session_id));
                                }
                            }
                            Err(e) => {
                                message_history.add_system_message(format!("Failed to fork session: {}", e));
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/export" || prompt.starts_with("/export ") {
                        // 解析可选的输出路径参数
                        let output_path = if prompt.len() > 7 {
                            let path_str = prompt[7..].trim();
                            if path_str.is_empty() {
                                None
                            } else {
                                Some(PathBuf::from(path_str))
                            }
                        } else {
                            None
                        };
                        
                        // 先保存当前会话
                        if let Err(e) = session.save().await {
                            message_history.add_system_message(format!("Failed to save session: {}", e));
                            let (term_width, _) = terminal.size();
                            render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                            continue;
                        }
                        
                        // 获取会话 ID 并加载会话数据
                        let session_id = session.session_id_async().await;
                        let sessions_dir = session.sessions_dir()
                            .ok_or_else(|| anyhow::anyhow!("Session manager not available"))?;
                        let session_manager = SessionManager::with_dir(sessions_dir)?;
                        
                        match session_manager.load_session(&session_id).await {
                            Ok(saved_session) => {
                                // 确定输出路径
                                let output = match output_path {
                                    Some(path) => path,
                                    None => {
                                        // 默认路径：当前目录/会话标题.html
                                        let title = saved_session.metadata.title.as_deref()
                                            .unwrap_or(&session_id);
                                        let safe_title = sanitize_filename(title);
                                        PathBuf::from(format!("{}.html", safe_title))
                                    }
                                };
                                
                                // 导出为 HTML
                                let exporter = HtmlExporter::new();
                                match exporter.export_session(&saved_session, &output) {
                                    Ok(()) => {
                                        let abs_path = std::fs::canonicalize(&output)
                                            .unwrap_or(output.clone());
                                        message_history.add_system_message(format!("Session exported to: {}", abs_path.display()));
                                    }
                                    Err(e) => {
                                        message_history.add_system_message(format!("Failed to export session: {}", e));
                                    }
                                }
                            }
                            Err(e) => {
                                message_history.add_system_message(format!("Failed to load session: {}", e));
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/compact" {
                        // 检查是否需要压缩
                        if !session.needs_compaction().await {
                            message_history.add_system_message("No need to compact. Context usage is below threshold.".to_string());
                            let (term_width, _) = terminal.size();
                            render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                            continue;
                        }
                        
                        // 执行压缩
                        message_history.add_system_message("Compacting conversation history...".to_string());
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                        
                        match session.compact().await {
                            Ok(result) => {
                                let saved_tokens = result.original_tokens.saturating_sub(result.compacted_tokens);
                                message_history.add_system_message(format!("✓ Compacted {} messages into summary", result.removed_count));
                                message_history.add_system_message(format!(
                                    "  Original: {} tokens → Summary: {} tokens (saved: {})", 
                                    result.original_tokens, result.compacted_tokens, saved_tokens
                                ));
                            }
                            Err(e) => {
                                message_history.add_system_message(format!("✗ Failed to compact: {}", e));
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/login" || prompt.starts_with("/login ") {
                        let provider_name = if prompt.len() > 7 {
                            prompt[7..].trim()
                        } else {
                            "anthropic"
                        };
                        
                        match get_oauth_provider(provider_name) {
                            Some(provider_config) => {
                                let token_storage = TokenStorage::new();
                                match run_oauth_flow(&provider_config, &token_storage).await {
                                    Ok(_) => {
                                        message_history.add_system_message(format!("✓ Successfully logged in with {}", provider_name));
                                    }
                                    Err(e) => {
                                        message_history.add_system_message(format!("✗ Login failed: {}", e));
                                    }
                                }
                            }
                            None => {
                                message_history.add_system_message(format!("Unknown provider: {}. Available: anthropic, github-copilot", provider_name));
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/logout" || prompt.starts_with("/logout ") {
                        let provider_name = if prompt.len() > 8 {
                            prompt[8..].trim()
                        } else {
                            "anthropic"
                        };
                        
                        let token_storage = TokenStorage::new();
                        match token_storage.remove_token(provider_name) {
                            Ok(_) => {
                                message_history.add_system_message(format!("✓ Successfully logged out from {}", provider_name));
                            }
                            Err(e) => {
                                message_history.add_system_message(format!("✗ Logout failed: {}", e));
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/auth" {
                        let token_storage = TokenStorage::new();
                        let providers = token_storage.list_providers();
                        
                        message_history.add_system_message("Authentication Status:".to_string());
                        if providers.is_empty() {
                            message_history.add_system_message("  No OAuth tokens stored.".to_string());
                            message_history.add_system_message("  Use /login [provider] to authenticate.".to_string());
                        } else {
                            for provider in providers {
                                if let Some(token) = token_storage.get_token(&provider) {
                                    let status = if token.is_expired() {
                                        "expired"
                                    } else if token.is_expiring_soon() {
                                        "expiring soon"
                                    } else {
                                        "valid"
                                    };
                                    message_history.add_system_message(format!("  {} - {}", provider, status));
                                }
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/extensions" {
                        // 显示已加载的扩展列表
                        let ext_mgr = session.extension_manager();
                        let extensions = ext_mgr.list_extensions();
                                            
                        if extensions.is_empty() {
                            message_history.add_system_message("No extensions loaded.".to_string());
                        } else {
                            message_history.add_system_message(format!("Loaded Extensions ({}):", extensions.len()));
                            for ext in extensions {
                                message_history.add_system_message(format!("  {} v{} - {}", 
                                    ext.name, ext.version, ext.description));
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/theme" || prompt.starts_with("/theme ") {
                        let theme_name = if prompt.len() > 7 {
                            prompt[7..].trim()
                        } else {
                            ""
                        };
                        
                        if theme_name.is_empty() {
                            // 显示当前主题和可用主题
                            message_history.add_system_message(format!(
                                "Current theme: {}. Available: {}",
                                _current_theme.name,
                                Theme::available_themes().join(", ")
                            ));
                        } else {
                            match Theme::from_name(theme_name) {
                                Some(theme) => {
                                    _current_theme = theme;
                                    message_history.add_system_message(format!("Theme changed to: {}", theme_name));
                                }
                                None => {
                                    message_history.add_system_message(format!(
                                        "Unknown theme: {}. Available: {}",
                                        theme_name,
                                        Theme::available_themes().join(", ")
                                    ));
                                }
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/help" {
                        message_history.add_system_message("Available Commands:".to_string());
                        message_history.add_system_message("  /help        - Show this help message".to_string());
                        message_history.add_system_message("  /clear       - Clear conversation history".to_string());
                        message_history.add_system_message("  /model       - Show or change model".to_string());
                        message_history.add_system_message("  /stats       - Show session statistics".to_string());
                        message_history.add_system_message("  /save        - Save current session".to_string());
                        message_history.add_system_message("  /fork        - Fork from current position".to_string());
                        message_history.add_system_message("  /fork N      - Fork at message index N".to_string());
                        message_history.add_system_message("  /compact     - Compact conversation history to save context space".to_string());
                        message_history.add_system_message("  /export      - Export session to HTML".to_string());
                        message_history.add_system_message("  /export path.html - Export to specific path".to_string());
                        message_history.add_system_message("  /extensions  - List loaded extensions".to_string());
                        message_history.add_system_message("  /login       - Login with OAuth (anthropic, github-copilot)".to_string());
                        message_history.add_system_message("  /logout      - Logout from OAuth provider".to_string());
                        message_history.add_system_message("  /auth        - Show authentication status".to_string());
                        message_history.add_system_message("  /theme       - Show or switch color theme".to_string());
                        message_history.add_system_message("  /theme dark  - Switch to dark theme".to_string());
                        message_history.add_system_message("  /theme light - Switch to light theme".to_string());
                        message_history.add_system_message("  /exit        - Exit the application".to_string());
                        message_history.add_system_message("  /quit        - Alias for /exit".to_string());
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if !prompt.is_empty() {
                        // 记录用户消息到历史
                        message_history.add_user_message(prompt.clone());
                        // 重新渲染以显示用户消息
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                        // 发送到 agent
                        if let Err(e) = session.prompt_text(&prompt).await {
                            message_history.add_system_message(format!("Error: {}", e));
                            let (term_width, _) = terminal.size();
                            render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                        }
                    } else {
                        // 空输入，重新渲染
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    }
                } else {
                    // 检查 Up/Down 键用于输入历史导航
                    // Up: \x1b[A 或 \x1bOA
                    // Down: \x1b[B 或 \x1bOB
                    let is_up = text == "\x1b[A" || text == "\x1bOA";
                    let is_down = text == "\x1b[B" || text == "\x1bOB";

                    let (cursor_row, _) = editor.cursor_position();
                    let line_count = editor.line_count();

                    if is_up && cursor_row == 0 {
                        // 在第一行按 Up，触发历史上翻
                        let current = editor.get_text().to_string();
                        if let Some(entry) = input_history.prev(&current) {
                            let entry = entry.to_string();
                            editor.set_text(&entry);
                        }
                        // 重新渲染编辑器
                        let (term_width, _) = terminal.size();
                        render_editor_area(&editor, term_width, &mut stdout)?;
                        stdout.flush()?;
                    } else if is_down && cursor_row + 1 >= line_count {
                        // 在最后一行按 Down，触发历史下翻
                        if let Some(entry) = input_history.next() {
                            let entry = entry.to_string();
                            editor.set_text(&entry);
                        }
                        // 重新渲染编辑器
                        let (term_width, _) = terminal.size();
                        render_editor_area(&editor, term_width, &mut stdout)?;
                        stdout.flush()?;
                    } else {
                        // 非历史导航，正常传给 Editor
                        let _handled = editor.handle_input(&text);
                        
                        // 重新渲染输入区域
                        let (term_width, _) = terminal.size();
                        render_editor_area(&editor, term_width, &mut stdout)?;
                    }
                }
            }
        }

        if should_exit {
            break;
        }
    }

    // 8. 清理终端状态
    // 禁用 bracketed paste mode
    write!(stdout, "\x1b[?2004l")?;
    stdout.flush()?;
    terminal.disable_raw_mode()?;
    write!(stdout, "\r\nGoodbye!\r\n")?;
    stdout.flush()?;

    Ok(())
}

/// 完整渲染：消息历史 + 状态栏 + 编辑器
fn render_full(
    history: &MessageHistory, 
    status_bar: &StatusBarComponent,
    editor: &Editor,
    width: u16,
    stdout: &mut impl Write,
) -> std::io::Result<()> {
    // 清屏
    write!(stdout, "\x1b[2J\x1b[H")?;
    
    // 渲染消息历史
    let history_lines = history.render(width);
    for line in &history_lines {
        write!(stdout, "{}\r\n", line)?;
    }
    
    // 渲染状态栏
    let bar_lines = status_bar.render(width);
    for line in &bar_lines {
        write!(stdout, "{}\r\n", line)?;
    }
    write!(stdout, "\r\n")?;
    
    // 渲染编辑器
    render_editor_area(editor, width, stdout)
}

/// 清理文件名中的非法字符
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .take(100) // 限制长度
        .collect::<String>()
        .trim()
        .replace(' ', "_")
}

/// 处理粘贴内容
fn handle_paste(
    pasted: &str,
    editor: &mut Editor,
    stdout: &mut impl Write,
    terminal: &ProcessTerminal,
) -> anyhow::Result<()> {
    let line_count = pasted.lines().count();
    let char_count = pasted.len();

    // 大粘贴检测阈值
    const LARGE_PASTE_LINES: usize = 10;
    const LARGE_PASTE_CHARS: usize = 500;

    if line_count > LARGE_PASTE_LINES || char_count > LARGE_PASTE_CHARS {
        // 大粘贴：显示提示后插入
        let preview_lines: Vec<&str> = pasted.lines().take(3).collect();
        let preview = preview_lines.join("\n");
        let remaining = line_count.saturating_sub(3);

        // 显示折叠提示
        write!(stdout, "\r\n\x1b[2m[Pasted {} lines, {} chars]\x1b[0m\r\n", line_count, char_count)?;
        if remaining > 0 {
            write!(stdout, "\x1b[2m  {}...\x1b[0m\r\n", preview.lines().next().unwrap_or(""))?;
            write!(stdout, "\x1b[2m  ... ({} more lines)\x1b[0m\r\n", remaining)?;
        }
        stdout.flush()?;
    }

    // 将粘贴内容插入编辑器
    editor.insert_text(pasted);

    // 重新渲染编辑器
    let (term_width, _) = terminal.size();
    render_editor_area(editor, term_width, stdout)?;
    stdout.flush()?;

    Ok(())
}
