//! 代码执行沙箱
//!
//! 实现隔离子进程中的安全代码执行，支持超时控制、输出捕获、取消信号。

#![allow(dead_code)] // Notebook 功能尚未完全集成

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::kernel::KernelType;

/// 敏感环境变量前缀/包含模式
const SENSITIVE_ENV_PATTERNS: &[&str] = &[
    "KEY",
    "SECRET",
    "TOKEN",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "AUTH",
    "PRIVATE",
    "API_KEY",
    "ACCESS_KEY",
    "SECRET_KEY",
];

/// 代码执行配置
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// 超时时间（秒），默认 30s，最大 300s
    pub timeout_secs: u64,
    /// 最大输出行数，默认 500
    pub max_output_lines: usize,
    /// 最大输出字节数，默认 1MB
    pub max_output_bytes: usize,
    /// 工作目录
    pub cwd: PathBuf,
}

impl ExecutionConfig {
    /// 创建新的执行配置
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            timeout_secs: 30,
            max_output_lines: 500,
            max_output_bytes: 1_000_000,
            cwd,
        }
    }

    /// 设置超时时间（秒）
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs.min(300); // 最大 300 秒
        self
    }

    /// 设置最大输出行数
    #[allow(dead_code)]
    pub fn with_max_output_lines(mut self, lines: usize) -> Self {
        self.max_output_lines = lines;
        self
    }

    /// 设置最大输出字节数
    #[allow(dead_code)]
    pub fn with_max_output_bytes(mut self, bytes: usize) -> Self {
        self.max_output_bytes = bytes;
        self
    }
}

/// 图像输出
#[derive(Debug, Clone)]
pub struct ImageOutput {
    /// MIME 类型
    pub mime_type: String,
    /// 图像数据
    pub data: Vec<u8>,
    /// 文件名
    pub filename: String,
}

/// 代码执行输出
#[derive(Debug, Clone)]
pub struct ExecutionOutput {
    /// 标准输出
    pub stdout: String,
    /// 标准错误
    pub stderr: String,
    /// 退出码
    pub exit_code: Option<i32>,
    /// 执行时间（毫秒）
    pub execution_time_ms: u64,
    /// 是否超时
    pub is_timeout: bool,
    /// 是否被取消
    pub is_cancelled: bool,
    /// 图像输出列表
    pub images: Vec<ImageOutput>,
}

impl ExecutionOutput {
    /// 是否执行成功（exit_code == 0 且未超时未取消）
    pub fn is_success(&self) -> bool {
        !self.is_timeout && !self.is_cancelled && self.exit_code == Some(0)
    }

    /// 获取格式化的输出文本（合并 stdout 和 stderr）
    pub fn formatted_output(&self) -> String {
        let mut output = String::new();
        if !self.stdout.is_empty() {
            output.push_str(&self.stdout);
        }
        if !self.stderr.is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str("[stderr]\n");
            output.push_str(&self.stderr);
        }
        if self.is_timeout {
            output.push_str(&format!(
                "\n\n[Execution timed out after {}ms]",
                self.execution_time_ms
            ));
        }
        if self.is_cancelled {
            output.push_str("\n\n[Execution cancelled]");
        }
        if let Some(code) = self.exit_code {
            if code != 0 {
                output.push_str(&format!("\n\n[Exit code: {}]", code));
            }
        }
        output
    }
}

/// 代码执行器
pub struct CodeExecutor {
    config: ExecutionConfig,
}

impl CodeExecutor {
    /// 创建新的代码执行器
    pub fn new(config: ExecutionConfig) -> Self {
        Self { config }
    }

    /// 统一执行入口
    ///
    /// # Arguments
    /// * `kernel_type` - Kernel 类型（Python 或 NodeJs）
    /// * `code` - 要执行的代码
    /// * `executable` - 可执行文件路径（python3 或 node）
    /// * `cancel` - 取消令牌
    /// * `on_update` - 实时输出回调
    pub async fn execute(
        &self,
        kernel_type: KernelType,
        code: &str,
        executable: &Path,
        cancel: CancellationToken,
        on_update: Option<&(dyn Fn(String) + Send + Sync)>,
    ) -> Result<ExecutionOutput> {
        // 1. 创建临时目录（用于代码文件和图像输出）
        let temp_dir = tempfile::TempDir::new()?;
        let code_file = temp_dir
            .path()
            .join(format!("notebook_exec{}", kernel_type.file_extension()));

        // 2. 准备代码（Python 需要注入 matplotlib 后端配置）
        let prepared_code = self.prepare_code(kernel_type, code, temp_dir.path());
        std::fs::write(&code_file, &prepared_code)?;

        // 3. 构建命令
        let mut cmd = Command::new(executable.as_os_str());
        cmd.arg(&code_file)
            .current_dir(&self.config.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .kill_on_drop(true);

        // 4. 环境变量过滤（复用 BashTool 的模式）
        let filtered_env = self.filter_sensitive_env_vars();
        cmd.env_clear();
        for (key, value) in &filtered_env {
            cmd.env(key, value);
        }

        // 添加 PYTHONUNBUFFERED=1 确保 Python 无缓冲输出
        if kernel_type == KernelType::Python {
            cmd.env("PYTHONUNBUFFERED", "1");
            cmd.env("PYTHONDONTWRITEBYTECODE", "1");
        }

        // 添加 NODE_NO_WARNINGS=1
        if kernel_type == KernelType::NodeJs {
            cmd.env("NODE_NO_WARNINGS", "1");
        }

        // 5. 启动进程
        let start_time = Instant::now();
        let mut child = cmd.spawn()?;

        // 6. 捕获输出（参考 BashTool 的 tokio::select! 模式）
        let stdout_handle = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout"))?;
        let stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stderr"))?;

        let mut stdout_reader = BufReader::new(stdout_handle).lines();
        let mut stderr_reader = BufReader::new(stderr_handle).lines();

        let mut stdout_lines: Vec<String> = Vec::new();
        let mut stderr_lines: Vec<String> = Vec::new();
        let mut is_cancelled = false;
        let mut is_timeout = false;
        
        // 输出计数器（用于内存限制）
        let mut stdout_total_lines: usize = 0;
        let mut stderr_total_lines: usize = 0;
        let mut stdout_bytes: usize = 0;
        let mut stderr_bytes: usize = 0;

        let timeout_duration = Duration::from_secs(self.config.timeout_secs);

        let result = timeout(timeout_duration, async {
            loop {
                tokio::select! {
                    // 检查取消信号
                    _ = cancel.cancelled() => {
                        is_cancelled = true;
                        let _ = child.kill().await;
                        break;
                    }
                    // 读取 stdout
                    line = stdout_reader.next_line() => {
                        match line {
                            Ok(Some(line)) => {
                                stdout_total_lines += 1;
                                let line_bytes = line.len() + 1; // +1 for newline
                                stdout_bytes += line_bytes;
                                
                                // 限制内存中的行数（滑动窗口）
                                if stdout_lines.len() >= self.config.max_output_lines {
                                    stdout_lines.remove(0); // 移除最早的行，保留最新
                                }
                                if stdout_bytes <= self.config.max_output_bytes {
                                    stdout_lines.push(line.clone());
                                }
                                
                                if let Some(callback) = on_update {
                                    callback(line);
                                }
                            }
                            Ok(None) => {
                                // stdout 结束，继续读 stderr 直到结束
                                while let Ok(Some(line)) = stderr_reader.next_line().await {
                                    stderr_total_lines += 1;
                                    let line_bytes = line.len() + 1;
                                    stderr_bytes += line_bytes;
                                    
                                    if stderr_lines.len() >= self.config.max_output_lines {
                                        stderr_lines.remove(0);
                                    }
                                    if stderr_bytes <= self.config.max_output_bytes {
                                        stderr_lines.push(line);
                                    }
                                }
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    // 读取 stderr
                    line = stderr_reader.next_line() => {
                        match line {
                            Ok(Some(line)) => {
                                stderr_total_lines += 1;
                                let line_bytes = line.len() + 1;
                                stderr_bytes += line_bytes;
                                
                                if stderr_lines.len() >= self.config.max_output_lines {
                                    stderr_lines.remove(0);
                                }
                                if stderr_bytes <= self.config.max_output_bytes {
                                    stderr_lines.push(line);
                                }
                            }
                            Ok(None) => {
                                // stderr 结束，继续读 stdout 直到结束
                                while let Ok(Some(line)) = stdout_reader.next_line().await {
                                    stdout_total_lines += 1;
                                    let line_bytes = line.len() + 1;
                                    stdout_bytes += line_bytes;
                                    
                                    if stdout_lines.len() >= self.config.max_output_lines {
                                        stdout_lines.remove(0);
                                    }
                                    if stdout_bytes <= self.config.max_output_bytes {
                                        stdout_lines.push(line.clone());
                                    }
                                    
                                    if let Some(callback) = on_update {
                                        callback(line);
                                    }
                                }
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
            child.wait().await
        })
        .await;

        let exit_status = match result {
            Ok(status) => status,
            Err(_) => {
                is_timeout = true;
                let _ = child.kill().await;
                child.wait().await
            }
        };

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        // 7. 收集图像输出
        let images = self.collect_images(temp_dir.path()).await;

        // 8. 构建输出（截断过长输出）
        let stdout = self.format_output_with_truncation(
            stdout_lines.join("\n"),
            stdout_total_lines,
            stdout_bytes,
        );
        let stderr = self.format_output_with_truncation(
            stderr_lines.join("\n"),
            stderr_total_lines,
            stderr_bytes,
        );

        Ok(ExecutionOutput {
            stdout,
            stderr,
            exit_code: exit_status.ok().and_then(|s| s.code()),
            execution_time_ms,
            is_timeout,
            is_cancelled,
            images,
        })
        // temp_dir 在这里 drop，自动清理临时文件
    }

    /// 准备代码（注入必要的前置配置）
    fn prepare_code(
        &self,
        kernel_type: KernelType,
        code: &str,
        output_dir: &std::path::Path,
    ) -> String {
        match kernel_type {
            KernelType::Python => {
                // 注入 matplotlib 非交互式后端 + 自动保存图像
                let output_path = output_dir.to_string_lossy().replace('\\', "/");
                format!(
                    r#"import os as _os
_os.environ['MPLBACKEND'] = 'Agg'
_PI_OUTPUT_DIR = '{output_dir}'

# Hook matplotlib savefig
try:
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as _plt
    _original_show = _plt.show
    _pi_fig_counter = [0]
    def _pi_show(*args, **kwargs):
        for _fig_num in _plt.get_fignums():
            _fig = _plt.figure(_fig_num)
            _pi_fig_counter[0] += 1
            _fig.savefig(_os.path.join(_PI_OUTPUT_DIR, f'figure_{{_pi_fig_counter[0]}}.png'), dpi=150, bbox_inches='tight')
        _plt.close('all')
    _plt.show = _pi_show
except ImportError:
    pass

{code}
"#,
                    output_dir = output_path,
                    code = code
                )
            }
            KernelType::NodeJs => {
                // Node.js 不需要特殊前置代码
                code.to_string()
            }
        }
    }

    /// 过滤敏感环境变量（参考 BashTool）
    fn filter_sensitive_env_vars(&self) -> Vec<(String, String)> {
        std::env::vars()
            .filter(|(key, _)| {
                let key_upper = key.to_uppercase();
                // 保留非敏感变量
                !SENSITIVE_ENV_PATTERNS
                    .iter()
                    .any(|pattern| key_upper.contains(pattern))
            })
            .collect()
    }

    /// 截断过长的输出
    fn truncate_output(&self, output: String) -> String {
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() > self.config.max_output_lines {
            let truncated: Vec<&str> =
                lines[lines.len() - self.config.max_output_lines..].to_vec();
            format!(
                "[Output truncated: showing last {} of {} lines]\n{}",
                self.config.max_output_lines,
                lines.len(),
                truncated.join("\n")
            )
        } else if output.len() > self.config.max_output_bytes {
            let truncated = &output[output.len() - self.config.max_output_bytes..];
            format!(
                "[Output truncated: showing last {} bytes]\n{}",
                self.config.max_output_bytes,
                truncated
            )
        } else {
            output
        }
    }

    /// 格式化输出并添加截断提示（用于已在读取循环中进行截断的情况）
    fn format_output_with_truncation(
        &self,
        output: String,
        total_lines: usize,
        total_bytes: usize,
    ) -> String {
        if total_lines > self.config.max_output_lines || total_bytes > self.config.max_output_bytes {
            let truncated_lines = output.lines().count();
            format!(
                "[Output truncated: showing last {} of {} lines]\n{}",
                truncated_lines,
                total_lines,
                output
            )
        } else {
            output
        }
    }

    /// 收集执行过程中产生的图像文件
    async fn collect_images(&self, output_dir: &std::path::Path) -> Vec<ImageOutput> {
        let mut images = Vec::new();
        if let Ok(mut entries) = tokio::fs::read_dir(output_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if ext_str == "png" || ext_str == "jpg" || ext_str == "jpeg" || ext_str == "svg"
                    {
                        if let Ok(data) = tokio::fs::read(&path).await {
                            let mime_type = match ext_str.as_str() {
                                "png" => "image/png",
                                "jpg" | "jpeg" => "image/jpeg",
                                "svg" => "image/svg+xml",
                                _ => "application/octet-stream",
                            };
                            images.push(ImageOutput {
                                mime_type: mime_type.to_string(),
                                data,
                                filename: path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                            });
                        }
                    }
                }
            }
        }
        // 按文件名排序确保顺序一致
        images.sort_by(|a, b| a.filename.cmp(&b.filename));
        images
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ExecutionConfig {
        ExecutionConfig::new(PathBuf::from("/tmp"))
    }

    #[test]
    fn test_execution_config_new() {
        let config = ExecutionConfig::new(PathBuf::from("/tmp"));
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_output_lines, 500);
        assert_eq!(config.max_output_bytes, 1_000_000);
    }

    #[test]
    fn test_execution_config_with_timeout() {
        let config = ExecutionConfig::new(PathBuf::from("/tmp")).with_timeout(60);
        assert_eq!(config.timeout_secs, 60);

        // 超过最大值被截断
        let config = ExecutionConfig::new(PathBuf::from("/tmp")).with_timeout(500);
        assert_eq!(config.timeout_secs, 300);
    }

    #[test]
    fn test_execution_output_is_success() {
        let output = ExecutionOutput {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            execution_time_ms: 100,
            is_timeout: false,
            is_cancelled: false,
            images: vec![],
        };
        assert!(output.is_success());

        let failed = ExecutionOutput {
            exit_code: Some(1),
            ..output.clone()
        };
        assert!(!failed.is_success());

        let timeout_output = ExecutionOutput {
            is_timeout: true,
            ..output.clone()
        };
        assert!(!timeout_output.is_success());

        let cancelled_output = ExecutionOutput {
            is_cancelled: true,
            ..output.clone()
        };
        assert!(!cancelled_output.is_success());
    }

    #[test]
    fn test_execution_output_formatted() {
        // 只有 stdout
        let output = ExecutionOutput {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            execution_time_ms: 100,
            is_timeout: false,
            is_cancelled: false,
            images: vec![],
        };
        assert_eq!(output.formatted_output(), "hello");

        // stdout + stderr
        let output = ExecutionOutput {
            stdout: "hello".to_string(),
            stderr: "error".to_string(),
            exit_code: Some(0),
            execution_time_ms: 100,
            is_timeout: false,
            is_cancelled: false,
            images: vec![],
        };
        let formatted = output.formatted_output();
        assert!(formatted.contains("hello"));
        assert!(formatted.contains("[stderr]"));
        assert!(formatted.contains("error"));

        // 超时
        let output = ExecutionOutput {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            execution_time_ms: 1000,
            is_timeout: true,
            is_cancelled: false,
            images: vec![],
        };
        let formatted = output.formatted_output();
        assert!(formatted.contains("timed out"));

        // 取消
        let output = ExecutionOutput {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            execution_time_ms: 100,
            is_timeout: false,
            is_cancelled: true,
            images: vec![],
        };
        let formatted = output.formatted_output();
        assert!(formatted.contains("cancelled"));

        // 非零退出码
        let output = ExecutionOutput {
            stdout: "hello".to_string(),
            stderr: String::new(),
            exit_code: Some(1),
            execution_time_ms: 100,
            is_timeout: false,
            is_cancelled: false,
            images: vec![],
        };
        let formatted = output.formatted_output();
        assert!(formatted.contains("Exit code: 1"));
    }

    #[test]
    fn test_prepare_code_python() {
        let executor = CodeExecutor::new(test_config());
        let code = "print('hello')";
        let prepared =
            executor.prepare_code(KernelType::Python, code, std::path::Path::new("/tmp"));
        assert!(prepared.contains("MPLBACKEND"));
        assert!(prepared.contains("print('hello')"));
    }

    #[test]
    fn test_prepare_code_nodejs() {
        let executor = CodeExecutor::new(test_config());
        let code = "console.log('hello')";
        let prepared =
            executor.prepare_code(KernelType::NodeJs, code, std::path::Path::new("/tmp"));
        assert_eq!(prepared, code);
    }

    #[test]
    fn test_truncate_output_short() {
        let executor = CodeExecutor::new(test_config());
        let output = "short output".to_string();
        assert_eq!(executor.truncate_output(output.clone()), output);
    }

    #[test]
    fn test_truncate_output_long_lines() {
        let mut config = test_config();
        config.max_output_lines = 5;
        let executor = CodeExecutor::new(config);
        let lines: Vec<String> = (0..10).map(|i| format!("line {}", i)).collect();
        let output = lines.join("\n");
        let truncated = executor.truncate_output(output);
        assert!(truncated.contains("[Output truncated"));
        assert!(truncated.contains("line 9"));
        assert!(!truncated.contains("line 0"));
    }

    #[test]
    fn test_truncate_output_long_bytes() {
        let mut config = test_config();
        config.max_output_bytes = 100;
        let executor = CodeExecutor::new(config);
        let long_output = "x".repeat(200);
        let truncated = executor.truncate_output(long_output);
        assert!(truncated.contains("[Output truncated"));
        assert!(truncated.contains("bytes"));
    }

    #[test]
    fn test_filter_sensitive_env_vars() {
        let executor = CodeExecutor::new(test_config());

        // 设置一些测试环境变量
        std::env::set_var("TEST_NORMAL_VAR", "normal");
        std::env::set_var("TEST_SECRET_KEY", "secret");
        std::env::set_var("TEST_API_TOKEN", "token");

        let vars = executor.filter_sensitive_env_vars();

        // 验证不包含敏感变量
        for (key, _) in &vars {
            let key_upper = key.to_uppercase();
            assert!(
                !key_upper.contains("SECRET"),
                "Found sensitive key: {}",
                key
            );
            assert!(
                !key_upper.contains("TOKEN"),
                "Found sensitive key: {}",
                key
            );
            assert!(
                !key_upper.contains("PASSWORD"),
                "Found sensitive key: {}",
                key
            );
        }

        // 验证普通变量被保留
        let keys: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"TEST_NORMAL_VAR"));

        // 清理
        std::env::remove_var("TEST_NORMAL_VAR");
        std::env::remove_var("TEST_SECRET_KEY");
        std::env::remove_var("TEST_API_TOKEN");
    }

    // 集成测试（需要系统环境）
    #[tokio::test]
    #[ignore = "Depends on system environment - requires python3 to be installed"]
    async fn test_execute_python_hello() {
        let config = ExecutionConfig::new(PathBuf::from("/tmp"));
        let executor = CodeExecutor::new(config);
        let cancel = CancellationToken::new();
        let python = PathBuf::from("python3");

        let result = executor
            .execute(KernelType::Python, "print('hello world')", &python, cancel, None)
            .await
            .unwrap();

        assert!(result.is_success());
        assert!(result.stdout.contains("hello world"));
    }

    #[tokio::test]
    #[ignore = "Depends on system environment - requires node to be installed"]
    async fn test_execute_nodejs_hello() {
        let config = ExecutionConfig::new(PathBuf::from("/tmp"));
        let executor = CodeExecutor::new(config);
        let cancel = CancellationToken::new();
        let node = PathBuf::from("node");

        let result = executor
            .execute(
                KernelType::NodeJs,
                "console.log('hello world')",
                &node,
                cancel,
                None,
            )
            .await
            .unwrap();

        assert!(result.is_success());
        assert!(result.stdout.contains("hello world"));
    }

    #[tokio::test]
    #[ignore = "Depends on system environment - requires python3 to be installed"]
    async fn test_execute_timeout() {
        let config = ExecutionConfig::new(PathBuf::from("/tmp")).with_timeout(2);
        let executor = CodeExecutor::new(config);
        let cancel = CancellationToken::new();
        let python = PathBuf::from("python3");

        let result = executor
            .execute(
                KernelType::Python,
                "import time; time.sleep(100)",
                &python,
                cancel,
                None,
            )
            .await
            .unwrap();

        assert!(result.is_timeout);
        assert!(!result.is_success());
    }

    #[tokio::test]
    #[ignore = "Depends on system environment - requires python3 to be installed"]
    async fn test_execute_cancel() {
        let config = ExecutionConfig::new(PathBuf::from("/tmp")).with_timeout(30);
        let executor = CodeExecutor::new(config);
        let cancel = CancellationToken::new();
        let python = PathBuf::from("python3");

        // 在另一个任务中取消
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cancel_clone.cancel();
        });

        let result = executor
            .execute(
                KernelType::Python,
                "import time; time.sleep(100)",
                &python,
                cancel,
                None,
            )
            .await
            .unwrap();

        assert!(result.is_cancelled);
        assert!(!result.is_success());
    }

    #[tokio::test]
    #[ignore = "Depends on system environment - requires python3 to be installed"]
    async fn test_execute_stderr() {
        let config = ExecutionConfig::new(PathBuf::from("/tmp"));
        let executor = CodeExecutor::new(config);
        let cancel = CancellationToken::new();
        let python = PathBuf::from("python3");

        let result = executor
            .execute(
                KernelType::Python,
                "import sys; print('error', file=sys.stderr)",
                &python,
                cancel,
                None,
            )
            .await
            .unwrap();

        assert!(result.is_success());
        assert!(result.stderr.contains("error"));
    }
}
