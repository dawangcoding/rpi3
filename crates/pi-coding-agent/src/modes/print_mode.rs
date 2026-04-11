//! 打印模式 - 非交互单次执行
//
//! 用于 `pi -p "prompt"` 或 `pi --mode print "prompt"`
//! 执行单次 prompt 后退出

use std::sync::Arc;
use tokio::sync::mpsc;
use pi_agent::types::*;
use pi_ai::types::*;
use crate::core::agent_session::{AgentSession, AgentSessionConfig};
use crate::config::AppConfig;
use serde::Serialize;

/// 标准化退出码
#[allow(dead_code)] // 预留给未来使用
pub mod exit_codes {
    /// 成功
    pub const SUCCESS: i32 = 0;
    /// 一般错误
    pub const GENERAL_ERROR: i32 = 1;
    /// 认证错误（API key 缺失或无效）
    pub const AUTH_ERROR: i32 = 2;
    /// 模型错误（模型不存在或不可用）
    pub const MODEL_ERROR: i32 = 3;
}

/// JSON 输出格式
#[derive(Debug, Serialize)]
#[allow(dead_code)] // 预留给未来使用
pub struct JsonOutput {
    /// 是否成功
    pub success: bool,
    /// 输出内容
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 使用的模型
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Token 使用情况
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<JsonUsage>,
    /// 退出码
    pub exit_code: i32,
}

/// JSON Token 使用情况
#[derive(Debug, Serialize)]
#[allow(dead_code)] // 预留给未来使用
pub struct JsonUsage {
    /// 输入 Token 数
    pub input_tokens: u64,
    /// 输出 Token 数
    pub output_tokens: u64,
}

/// 将内容写入输出文件
#[allow(dead_code)] // 预留给未来使用
pub fn write_output_file(path: &str, content: &str) -> anyhow::Result<()> {
    std::fs::write(path, content)?;
    Ok(())
}

/// 从输入文件读取提示词
#[allow(dead_code)] // 预留给未来使用
pub fn read_input_file(path: &str) -> anyhow::Result<String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read input file '{}': {}", path, e))?;
    Ok(content.trim().to_string())
}

/// 格式化 JSON 输出
#[allow(dead_code)] // 预留给未来使用
pub fn format_json_output(output: &JsonOutput) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(output)?)
}

/// 打印模式配置
pub struct PrintConfig {
    /// 使用的模型
    pub model: Model,
    /// 思考级别
    pub thinking_level: ThinkingLevel,
    /// 系统提示词
    pub system_prompt: Option<String>,
    /// 追加的系统提示词
    pub append_system_prompt: Option<String>,
    /// 上下文文件列表
    pub context_files: Vec<String>,
    /// 工作目录
    pub cwd: std::path::PathBuf,
    /// 是否禁用 Bash 工具
    pub no_bash: bool,
    /// 是否禁用编辑工具
    pub no_edit: bool,
    /// 应用配置
    pub app_config: AppConfig,
    /// 提示词
    pub prompt: String,
    /// 是否禁用流式输出
    pub no_stream: bool,
}

/// 运行打印模式（非交互，执行单次 prompt 后退出）
pub async fn run(config: PrintConfig) -> anyhow::Result<()> {
    // 1. 创建 AgentSession
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
    
    // 2. 设置事件监听器（流式输出到 stdout）
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();
    
    let tx = event_tx.clone();
    let _ = session.agent().subscribe(Arc::new(move |event: AgentEvent, _cancel| {
        let _ = tx.send(event);
    }));
    
    // 3. 发送 prompt
    session.prompt_text(&config.prompt).await?;
    
    // 4. 消费事件流输出
    while let Some(event) = event_rx.recv().await {
        match event {
            AgentEvent::MessageUpdate { event: msg_event, .. } => {
                match msg_event {
                    AssistantMessageEvent::TextDelta { delta, .. } => {
                        if !config.no_stream {
                            print!("{}", delta);
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                    }
                    AssistantMessageEvent::ThinkingDelta { delta, .. } => {
                        if !config.no_stream {
                            eprint!("{}", delta); // thinking 输出到 stderr
                        }
                    }
                    _ => {}
                }
            }
            AgentEvent::ToolExecutionStart { tool_name, .. } => {
                eprintln!("[tool] Running {}...", tool_name);
            }
            AgentEvent::ToolExecutionEnd { tool_name, is_error, .. } => {
                if is_error {
                    eprintln!("[tool] {} failed", tool_name);
                } else {
                    eprintln!("[tool] {} done", tool_name);
                }
            }
            AgentEvent::AgentEnd { .. } => {
                if !config.no_stream {
                    println!(); // 最后换行
                }
                break;
            }
            _ => {}
        }
    }
    
    // 5. 等待完成
    session.wait_for_idle().await;
    
    // 6. 输出统计到 stderr
    let stats = session.stats().await;
    eprintln!("[stats] tokens: {} in / {} out | cost: ${:.4}",
        stats.tokens.input, stats.tokens.output, stats.cost);
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_codes() {
        assert_eq!(exit_codes::SUCCESS, 0);
        assert_eq!(exit_codes::GENERAL_ERROR, 1);
        assert_eq!(exit_codes::AUTH_ERROR, 2);
        assert_eq!(exit_codes::MODEL_ERROR, 3);
    }

    #[test]
    fn test_json_output_success() {
        let output = JsonOutput {
            success: true,
            content: Some("Hello world".to_string()),
            error: None,
            model: Some("gpt-4o".to_string()),
            usage: Some(JsonUsage { input_tokens: 10, output_tokens: 20 }),
            exit_code: exit_codes::SUCCESS,
        };
        let json = format_json_output(&output).unwrap();
        assert!(json.contains("\"success\": true"));
        assert!(json.contains("Hello world"));
        assert!(!json.contains("error")); // error 为 None 应被跳过
    }

    #[test]
    fn test_json_output_error() {
        let output = JsonOutput {
            success: false,
            content: None,
            error: Some("API key not found".to_string()),
            model: None,
            usage: None,
            exit_code: exit_codes::AUTH_ERROR,
        };
        let json = format_json_output(&output).unwrap();
        assert!(json.contains("\"success\": false"));
        assert!(json.contains("API key not found"));
    }

    #[test]
    fn test_read_input_file() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), "  Hello from file  \n").unwrap();
        let content = read_input_file(temp.path().to_str().unwrap()).unwrap();
        assert_eq!(content, "Hello from file");
    }

    #[test]
    fn test_read_input_file_not_found() {
        let result = read_input_file("/nonexistent/file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_write_output_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output_path = temp_dir.path().join("output.txt");
        write_output_file(output_path.to_str().unwrap(), "Test output").unwrap();
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert_eq!(content, "Test output");
    }
}
