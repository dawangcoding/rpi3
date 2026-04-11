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
use crate::core::auth::{TokenStorage, get_oauth_provider, list_oauth_providers, run_oauth_flow};
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
        slash_provider.add_command(SlashCommand::new("forks", "List all forks of current session"));
        slash_provider.add_command(SlashCommand::new("switch", "Switch to a different session or fork"));
        slash_provider.add_command(SlashCommand::new("delete-fork", "Delete a fork and its descendants"));
        slash_provider.add_command(SlashCommand::new("tree", "Show session tree structure"));
        slash_provider.add_command(SlashCommand::new("export", "Export session to HTML file").with_alias("export-html"));
        slash_provider.add_command(SlashCommand::new("compact", "Compact conversation history to save context space"));
        slash_provider.add_command(SlashCommand::new("extensions", "List loaded extensions"));
        slash_provider.add_command(SlashCommand::new("login", "Login with OAuth provider (anthropic, github-copilot, openai, google)"));
        slash_provider.add_command(SlashCommand::new("logout", "Logout from OAuth provider"));
        slash_provider.add_command(SlashCommand::new("auth", "Show current authentication status"));
        slash_provider.add_command(SlashCommand::new("theme", "Switch color theme (dark, light)"));
        slash_provider.add_command(SlashCommand::new("keybindings", "Configure keyboard shortcuts"));
        Self {
            slash_provider,
            file_provider: FileAutocompleteProvider::new(cwd),
            model_provider: ModelAutocompleteProvider::new(),
        }
    }

    /// 添加扩展注册的命令到自动补全列表
    fn add_extension_commands(&mut self, commands: &[crate::core::extensions::SlashCommand]) {
        for cmd in commands {
            let mut slash_cmd = SlashCommand::new(&cmd.name, &cmd.description);
            if !cmd.aliases.is_empty() {
                slash_cmd = slash_cmd.with_aliases(cmd.aliases.clone());
            }
            self.slash_provider.add_command(slash_cmd);
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

    // 保存 keybindings_path 在移动 app_config 之前
    let keybindings_config_path = config.app_config.keybindings_path();

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
    let _ = session.agent().subscribe(Arc::new(move |event: AgentEvent, _cancel| {
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
        editor_mode: pi_tui::components::editor::EditorMode::Emacs,
        relative_line_numbers: false,
    });
    editor.set_focused(true);
    
    // 设置自动完成提供者
    let mut autocomplete_provider = CodingAgentAutocompleteProvider::new(config.cwd.clone());
    // 添加扩展命令到自动补全列表
    let ext_commands = session.extension_manager().get_all_commands();
    autocomplete_provider.add_extension_commands(&ext_commands);
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
                        status_bar.set_cost(stats.cost); // 更新成本显示
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
                        handle_paste(pasted, &mut editor, &mut stdout, &terminal)?;
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
                        let compaction_history = session.compaction_history().await;
                        let context_usage = session.context_usage().await;
                        
                        let mut output = String::new();
                        
                        // 标题
                        output.push_str("\x1b[1mSession Statistics\x1b[0m\n");
                        output.push_str("──────────────────────────\n\n");
                        
                        // 消息统计
                        output.push_str(&format!(
                            "  Messages:    {} (User: {}, Assistant: {})\n",
                            stats.total_messages, stats.user_messages, stats.assistant_messages
                        ));
                        output.push_str(&format!("  Tool Calls:  {}\n\n", stats.tool_calls));
                        
                        // Token 使用
                        output.push_str("\x1b[1mToken Usage:\x1b[0m\n");
                        output.push_str(&format!("  Input:       {:>10} tokens\n", format_number(stats.tokens.input)));
                        output.push_str(&format!("  Output:      {:>10} tokens\n", format_number(stats.tokens.output)));
                        if stats.tokens.cache_read > 0 {
                            output.push_str(&format!("  Cache Read:  {:>10} tokens\n", format_number(stats.tokens.cache_read)));
                        }
                        if stats.tokens.cache_write > 0 {
                            output.push_str(&format!("  Cache Write: {:>10} tokens\n", format_number(stats.tokens.cache_write)));
                        }
                        output.push_str(&format!("  Total:       {:>10} tokens\n\n", format_number(stats.tokens.total)));
                        
                        // 上下文窗口使用率
                        let context_window = context_usage.context_window;
                        if context_window > 0 {
                            let usage_pct = (stats.tokens.total as f64 / context_window as f64) * 100.0;
                            output.push_str(&format!(
                                "  Context Window: {} / {} ({:.1}%)\n\n",
                                format_number(stats.tokens.total), format_number(context_window as u64), usage_pct
                            ));
                        }
                        
                        // 成本
                        if stats.cost > 0.0 {
                            output.push_str(&format!("\x1b[1mEstimated Cost:\x1b[0m ${:.4}\n\n", stats.cost));
                        }
                        
                        // 压缩信息
                        if !compaction_history.is_empty() {
                            output.push_str("\x1b[1mCompaction History:\x1b[0m\n");
                            output.push_str(&format!("  Total compactions: {}\n", compaction_history.len()));
                            if let Some(last) = compaction_history.last() {
                                let saved = last.original_tokens.saturating_sub(last.summary_tokens);
                                output.push_str(&format!(
                                    "  Last: {} msgs → summary, saved {} tokens\n",
                                    last.removed_message_range.1 - last.removed_message_range.0,
                                    saved
                                ));
                            }
                            output.push('\n');
                        }
                        
                        // 会话信息
                        output.push_str(&format!("  Session: {}\n", &stats.session_id[..8.min(stats.session_id.len())]));
                        if let Some(ref file) = stats.session_file {
                            output.push_str(&format!("  File: {}\n", file));
                        }
                        
                        message_history.add_system_message(output);
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
                                let providers = list_oauth_providers().join(", ");
                                message_history.add_system_message(format!("Unknown provider: {}. Available: {}", provider_name, providers));
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
                    } else if prompt == "/extensions" || prompt.starts_with("/extensions ") {
                        // 解析子命令
                        let sub_cmd = if prompt.len() > 12 { prompt[12..].trim() } else { "list" };
                        let sub_cmd_name = sub_cmd.split_whitespace().next().unwrap_or("list");
                        
                        match sub_cmd_name {
                            "list" | "" => {
                                // 列出扩展（名称、版本、描述、工具数）
                                let ext_mgr = session.extension_manager();
                                let extensions = ext_mgr.list_extensions();
                                
                                if extensions.is_empty() {
                                    message_history.add_system_message("No extensions loaded.".to_string());
                                } else {
                                    message_history.add_system_message(format!("Loaded Extensions ({}):", extensions.len()));
                                    for ext in &extensions {
                                        let tool_count = ext_mgr.get_extension_tools(&ext.name).len();
                                        message_history.add_system_message(format!(
                                            "  {} v{} - {} (tools: {})",
                                            ext.name, ext.version, ext.description, tool_count
                                        ));
                                    }
                                }
                            }
                            "info" => {
                                // 显示扩展详情
                                let ext_name = sub_cmd.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                                if ext_name.is_empty() {
                                    message_history.add_system_message("Usage: /extensions info <name>".to_string());
                                } else {
                                    let ext_mgr = session.extension_manager();
                                    let extensions = ext_mgr.list_extensions();
                                    if let Some(ext) = extensions.iter().find(|e| e.name == ext_name) {
                                        message_history.add_system_message(format!("Extension: {}", ext.name));
                                        message_history.add_system_message(format!("  Version: {}", ext.version));
                                        message_history.add_system_message(format!("  Description: {}", ext.description));
                                        message_history.add_system_message(format!("  Author: {}", ext.author));
                                        let tools = ext_mgr.get_extension_tools(&ext.name);
                                        message_history.add_system_message(format!("  Registered tools: {}", tools.len()));
                                        // 显示工具列表
                                        for tool in &tools {
                                            message_history.add_system_message(format!("    - {} : {}", tool.name(), tool.description()));
                                        }
                                    } else {
                                        message_history.add_system_message(format!("Extension '{}' not found", ext_name));
                                    }
                                }
                            }
                            "enable" => {
                                // 启用扩展（下次重启生效）
                                let ext_name = sub_cmd.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                                if ext_name.is_empty() {
                                    message_history.add_system_message("Usage: /extensions enable <name>".to_string());
                                } else {
                                    // TODO: 修改配置文件
                                    message_history.add_system_message(format!(
                                        "Extension '{}' will be enabled on next restart. (TODO: config persistence)",
                                        ext_name
                                    ));
                                }
                            }
                            "disable" => {
                                // 禁用扩展（下次重启生效）
                                let ext_name = sub_cmd.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                                if ext_name.is_empty() {
                                    message_history.add_system_message("Usage: /extensions disable <name>".to_string());
                                } else {
                                    // TODO: 修改配置文件
                                    message_history.add_system_message(format!(
                                        "Extension '{}' will be disabled on next restart. (TODO: config persistence)",
                                        ext_name
                                    ));
                                }
                            }
                            other => {
                                message_history.add_system_message(format!(
                                    "Unknown subcommand: {}. Available: list, info, enable, disable",
                                    other
                                ));
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
                    } else if prompt == "/keybindings" {
                        // 进入快捷键配置模式
                        let mut kb_view = super::keybindings_config::KeybindingsConfigView::new(keybindings_config_path.clone());
                        
                        message_history.add_system_message("Entering keybindings configuration mode...".to_string());
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                        
                        // 快捷键配置循环 - 从 input_rx channel 消费输入
                        loop {
                            // 渲染配置视图
                            let (term_width, _) = terminal.size();
                            kb_view.render(term_width, &mut stdout)?;
                            
                            // 从 input_rx channel 异步接收输入
                            let data = match input_rx.recv().await {
                                Some(d) => d,
                                None => break,
                            };
                            let input = String::from_utf8_lossy(&data);
                            kb_view.handle_input(&input);
                            
                            if kb_view.should_exit() {
                                break;
                            }
                        }
                        
                        // 退出配置模式，恢复主界面
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/forks" {
                        // 列出当前会话的所有 fork
                        let session_id = session.session_id_async().await;
                        if let Some(mgr) = session.session_manager() {
                            match mgr.list_forks(&session_id).await {
                                Ok(forks) => {
                                    if forks.is_empty() {
                                        message_history.add_system_message("No forks found for current session.".to_string());
                                    } else {
                                        let mut output = String::from("Session Forks:\n");
                                        for (i, fork) in forks.iter().enumerate() {
                                            let title = fork.title.as_deref().unwrap_or("Untitled");
                                            let msgs = fork.message_count;
                                            let id_short = &fork.id[..8.min(fork.id.len())];
                                            output.push_str(&format!("  {}. {} ({}) - {} messages\n",
                                                i + 1, title, id_short, msgs));
                                        }
                                        message_history.add_system_message(output);
                                    }
                                }
                                Err(e) => {
                                    message_history.add_system_message(format!("Error listing forks: {}", e));
                                }
                            }
                        } else {
                            message_history.add_system_message("Session manager not available.".to_string());
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/switch" || prompt.starts_with("/switch ") {
                        let target = if prompt.len() > 8 {
                            prompt[8..].trim()
                        } else {
                            ""
                        };

                        if target.is_empty() {
                            message_history.add_system_message("Usage: /switch <session_id or fork number>".to_string());
                            let (term_width, _) = terminal.size();
                            render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                            continue;
                        }

                        let mgr = match session.session_manager() {
                            Some(m) => m,
                            None => {
                                message_history.add_system_message("Session manager not available.".to_string());
                                let (term_width, _) = terminal.size();
                                render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                                continue;
                            }
                        };

                        let current_id = session.session_id_async().await;

                        // 尝试按编号查找（从 /forks 列表）
                        let target_id = if let Ok(num) = target.parse::<usize>() {
                            // 按编号查找
                            match mgr.list_forks(&current_id).await {
                                Ok(forks) => {
                                    if num > 0 && num <= forks.len() {
                                        Some(forks[num - 1].id.clone())
                                    } else {
                                        message_history.add_system_message(format!("Invalid fork number: {}", num));
                                        None
                                    }
                                }
                                Err(e) => {
                                    message_history.add_system_message(format!("Error: {}", e));
                                    None
                                }
                            }
                        } else {
                            // 按 session_id 前缀匹配
                            match mgr.find_session_by_prefix(target).await {
                                Ok(Some(id)) => Some(id),
                                Ok(None) => {
                                    message_history.add_system_message(format!("Session not found: {}", target));
                                    None
                                }
                                Err(e) => {
                                    message_history.add_system_message(format!("Error: {}", e));
                                    None
                                }
                            }
                        };

                        if let Some(id) = target_id {
                            // 保存当前会话
                            if let Err(e) = session.save().await {
                                message_history.add_system_message(format!("Failed to save current session: {}", e));
                                let (term_width, _) = terminal.size();
                                render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                                continue;
                            }

                            // 加载目标会话
                            match mgr.load_session(&id).await {
                                Ok(saved_session) => {
                                    // 重置 agent 状态并加载新会话的消息
                                    session.agent().reset().await;
                                    for msg in saved_session.messages {
                                        session.agent().steer(msg).await;
                                    }

                                    // 更新会话 ID
                                    session.set_session_id(id.clone()).await;

                                    // 清空消息历史并重新加载
                                    message_history.clear();
                                    message_history.add_system_message(format!(
                                        "Switched to session: {} ({})",
                                        saved_session.metadata.title.as_deref().unwrap_or("Untitled"),
                                        &id[..8.min(id.len())]
                                    ));
                                }
                                Err(e) => {
                                    message_history.add_system_message(format!("Failed to load session: {}", e));
                                }
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/delete-fork" || prompt.starts_with("/delete-fork ") {
                        let target = if prompt.len() > 13 {
                            prompt[13..].trim()
                        } else {
                            ""
                        };

                        if target.is_empty() {
                            message_history.add_system_message("Usage: /delete-fork <session_id>".to_string());
                            let (term_width, _) = terminal.size();
                            render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                            continue;
                        }

                        let mgr = match session.session_manager() {
                            Some(m) => m,
                            None => {
                                message_history.add_system_message("Session manager not available.".to_string());
                                let (term_width, _) = terminal.size();
                                render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                                continue;
                            }
                        };

                        // 不允许删除当前活跃会话
                        let current_id = session.session_id_async().await;
                        if target == current_id || current_id.starts_with(target) {
                            message_history.add_system_message("Cannot delete the active session.".to_string());
                            let (term_width, _) = terminal.size();
                            render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                            continue;
                        }

                        // 解析目标 ID（支持前缀匹配）
                        let target_id = match mgr.find_session_by_prefix(target).await {
                            Ok(Some(id)) => id,
                            Ok(None) => {
                                message_history.add_system_message(format!("Session not found: {}", target));
                                let (term_width, _) = terminal.size();
                                render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                                continue;
                            }
                            Err(e) => {
                                message_history.add_system_message(format!("Error: {}", e));
                                let (term_width, _) = terminal.size();
                                render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                                continue;
                            }
                        };

                        match mgr.delete_fork_tree(&target_id).await {
                            Ok(count) => {
                                message_history.add_system_message(format!("Deleted {} session(s).", count));
                            }
                            Err(e) => {
                                message_history.add_system_message(format!("Error: {}", e));
                            }
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/tree" {
                        let session_id = session.session_id_async().await;
                        if let Some(mgr) = session.session_manager() {
                            match mgr.format_session_tree(&session_id).await {
                                Ok(tree) => {
                                    message_history.add_system_message(format!("Session Tree:\n{}", tree));
                                }
                                Err(e) => {
                                    message_history.add_system_message(format!("Error: {}", e));
                                }
                            }
                        } else {
                            message_history.add_system_message("Session manager not available.".to_string());
                        }
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if prompt == "/help" {
                        message_history.add_system_message("Available Commands:".to_string());
                        message_history.add_system_message("  /help          - Show this help message".to_string());
                        message_history.add_system_message("  /clear         - Clear conversation history".to_string());
                        message_history.add_system_message("  /model         - Show or change model".to_string());
                        message_history.add_system_message("  /stats         - Show session statistics".to_string());
                        message_history.add_system_message("  /save          - Save current session".to_string());
                        message_history.add_system_message("  /fork          - Fork from current position".to_string());
                        message_history.add_system_message("  /fork N        - Fork at message index N".to_string());
                        message_history.add_system_message("  /forks         - List all forks of current session".to_string());
                        message_history.add_system_message("  /switch <id>   - Switch to a different session or fork".to_string());
                        message_history.add_system_message("  /delete-fork <id> - Delete a fork and its descendants".to_string());
                        message_history.add_system_message("  /tree          - Show session tree structure".to_string());
                        message_history.add_system_message("  /compact       - Compact conversation history to save context space".to_string());
                        message_history.add_system_message("  /export        - Export session to HTML".to_string());
                        message_history.add_system_message("  /export path.html - Export to specific path".to_string());
                        message_history.add_system_message("  /extensions         - List loaded extensions".to_string());
                        message_history.add_system_message("  /extensions info <name> - Show extension details".to_string());
                        message_history.add_system_message("  /extensions enable <name> - Enable extension (next restart)".to_string());
                        message_history.add_system_message("  /extensions disable <name> - Disable extension (next restart)".to_string());
                        message_history.add_system_message("  /login         - Login with OAuth (anthropic, github-copilot, openai, google)".to_string());
                        message_history.add_system_message("  /logout        - Logout from OAuth provider".to_string());
                        message_history.add_system_message("  /auth          - Show authentication status".to_string());
                        message_history.add_system_message("  /theme         - Show or switch color theme".to_string());
                        message_history.add_system_message("  /theme dark    - Switch to dark theme".to_string());
                        message_history.add_system_message("  /theme light   - Switch to light theme".to_string());
                        message_history.add_system_message("  /keybindings   - Configure keyboard shortcuts".to_string());
                        message_history.add_system_message("  /exit          - Exit the application".to_string());
                        message_history.add_system_message("  /quit          - Alias for /exit".to_string());
                        
                        // 显示扩展注册的命令
                        let ext_commands = session.extension_manager().get_all_commands();
                        if !ext_commands.is_empty() {
                            message_history.add_system_message("".to_string());
                            message_history.add_system_message("Extension Commands:".to_string());
                            for cmd in ext_commands {
                                let usage = cmd.usage.as_deref().unwrap_or(&cmd.name);
                                message_history.add_system_message(format!("  /{} - {}", usage, cmd.description));
                            }
                        }
                        
                        let (term_width, _) = terminal.size();
                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                    } else if let Some(stripped) = prompt.strip_prefix('/') {
                        // 尝试从扩展查找命令
                        let cmd_name = stripped.split_whitespace().next().unwrap_or("");
                        let cmd_args_str = stripped.trim_start().split_once(' ').map(|x| x.1).unwrap_or("").to_string();
                        
                        // 优先从统一命令注册表查找，回退到旧的 get_all_commands
                        let ext_commands = session.extension_manager().get_all_commands();
                        let found_cmd = ext_commands.into_iter().find(|c| c.matches(cmd_name));
                        
                        if let Some(cmd) = found_cmd {
                            // 发出 BeforeCommandExecute 事件
                            let before_event = AgentEvent::BeforeCommandExecute {
                                command: cmd_name.to_string(),
                                args: cmd_args_str.clone(),
                            };
                            let dispatch_result = session.extension_manager().dispatch_event_with_control(&before_event).await;
                            
                            // 检查是否被扩展阻止
                            if dispatch_result.blocked {
                                let reason = dispatch_result.block_reason.unwrap_or_else(|| "Blocked by extension".to_string());
                                message_history.add_system_message(format!("Command blocked: {}", reason));
                                let (term_width, _) = terminal.size();
                                render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                            } else {
                                let args = crate::core::extensions::types::CommandArgs::new(cmd_args_str.clone());
                                match (cmd.handler)(args).await {
                                    Ok(result) => {
                                        if !result.message.is_empty() {
                                            message_history.add_system_message(result.message.clone());
                                        }
                                        if result.should_render {
                                            let (term_width, _) = terminal.size();
                                            render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                                        }
                                        
                                        // 发出 AfterCommandExecute 事件
                                        let after_event = AgentEvent::AfterCommandExecute {
                                            command: cmd_name.to_string(),
                                            result: result.message,
                                        };
                                        let _ = session.extension_manager().dispatch_event_with_control(&after_event).await;
                                    }
                                    Err(e) => {
                                        let error_msg = format!("Command error: {}", e);
                                        message_history.add_system_message(error_msg.clone());
                                        let (term_width, _) = terminal.size();
                                        render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                                        
                                        // 发出 CommandError 事件
                                        let error_event = AgentEvent::CommandError {
                                            command: cmd_name.to_string(),
                                            error: e.to_string(),
                                        };
                                        let _ = session.extension_manager().dispatch_event_with_control(&error_event).await;
                                    }
                                }
                            }
                        } else {
                            message_history.add_system_message(format!("Unknown command: {}", prompt));
                            let (term_width, _) = terminal.size();
                            render_full(&message_history, &status_bar, &editor, term_width, &mut stdout)?;
                        }
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

/// 格式化数字（千分位）
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(100), "100");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1234567), "1,234,567");
        assert_eq!(format_number(1234567890), "1,234,567,890");
    }
}
