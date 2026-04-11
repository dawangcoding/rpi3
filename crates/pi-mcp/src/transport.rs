//! MCP 传输层实现
//!
//! 提供多种传输方式的实现：
//! - StdioTransport: 通过子进程的 stdin/stdout 通信
//! - SseTransport: 通过 HTTP SSE (Server-Sent Events) 通信

use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex};
use tokio_util::codec::{FramedRead, LinesCodec};
use tracing::{debug, error, info, trace, warn};

use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

/// 传输类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// Stdio 传输
    Stdio,
    /// SSE 传输
    Sse,
}

/// 接收到的消息类型
#[derive(Debug, Clone)]
pub enum IncomingMessage {
    /// 响应消息
    Response(JsonRpcResponse),
    /// 通知消息
    Notification(JsonRpcNotification),
    /// 服务端发起的请求
    Request(JsonRpcRequest),
}

/// Transport trait
#[async_trait]
pub trait Transport: Send + Sync {
    /// 发送请求
    async fn send_request(&mut self, request: &JsonRpcRequest) -> anyhow::Result<()>;

    /// 发送通知
    async fn send_notification(&mut self, notification: &JsonRpcNotification) -> anyhow::Result<()>;

    /// 接收消息
    async fn receive(&mut self) -> anyhow::Result<IncomingMessage>;

    /// 关闭连接
    async fn close(&mut self) -> anyhow::Result<()>;
}

/// Stdio 传输实现
///
/// 通过子进程的 stdin/stdout 进行 MCP 通信
pub struct StdioTransport {
    /// 子进程
    child: Option<Child>,
    /// stdin 写入器
    stdin: Option<ChildStdin>,
    /// stdout 读取器
    stdout_reader: Option<FramedRead<BufReader<ChildStdout>, LinesCodec>>,
    /// 子进程 stderr 收集（用于日志）
    stderr_buffer: Arc<Mutex<String>>,
}

impl StdioTransport {
    /// 创建新的 Stdio 传输，启动指定的命令
    pub fn new(command: &str, args: &[&str]) -> anyhow::Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!("Failed to spawn process '{}': {}", command, e)
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            anyhow::anyhow!("Failed to get stdin of child process")
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            anyhow::anyhow!("Failed to get stdout of child process")
        })?;

        let stdout_reader = FramedRead::new(BufReader::new(stdout), LinesCodec::new());

        let stderr_buffer = Arc::new(Mutex::new(String::new()));
        let stderr_buffer_clone = stderr_buffer.clone();

        // 处理 stderr（在后台任务中收集）
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!(stderr = %line, "MCP server stderr");
                    let mut buf = stderr_buffer_clone.lock().await;
                    buf.push_str(&line);
                    buf.push('\n');
                }
            });
        }

        info!(command = %command, args = ?args, "Started MCP server process");

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout_reader: Some(stdout_reader),
            stderr_buffer,
        })
    }

    /// 从命令字符串创建（解析命令和参数）
    pub fn from_command_string(command: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Err(anyhow::anyhow!("Empty command string"));
        }

        let cmd = parts[0];
        let args = &parts[1..];
        Self::new(cmd, args)
    }

    /// 获取 stderr 输出
    pub async fn get_stderr(&self) -> String {
        self.stderr_buffer.lock().await.clone()
    }

    /// 从现有的 stdin/stdout 管道创建 StdioTransport
    /// 
    /// 用于当子进程已经在外部启动，需要包装其管道时使用
    pub fn from_pipes(
        stdin: ChildStdin,
        stdout: ChildStdout,
    ) -> anyhow::Result<Self> {
        let stdout_reader = FramedRead::new(BufReader::new(stdout), LinesCodec::new());
        let stderr_buffer = Arc::new(Mutex::new(String::new()));

        Ok(Self {
            child: None, // 没有子进程，因为进程由外部管理
            stdin: Some(stdin),
            stdout_reader: Some(stdout_reader),
            stderr_buffer,
        })
    }

    /// 发送原始 JSON 行
    async fn send_line(&mut self, line: &str) -> anyhow::Result<()> {
        if let Some(ref mut stdin) = self.stdin {
            stdin.write_all(line.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
            trace!(line = %line, "Sent line to MCP server");
            Ok(())
        } else {
            Err(anyhow::anyhow!("stdin is not available"))
        }
    }

    /// 解析接收到的消息
    fn parse_message(line: &str) -> anyhow::Result<IncomingMessage> {
        let value: serde_json::Value = serde_json::from_str(line)?;

        // 根据 JSON 结构判断消息类型
        // Response: 有 id，有 result 或 error
        // Request: 有 id，有 method，无 result/error
        // Notification: 无 id，有 method
        if let Some(_id) = value.get("id") {
            if value.get("result").is_some() || value.get("error").is_some() {
                // Response
                let response: JsonRpcResponse = serde_json::from_str(line)?;
                Ok(IncomingMessage::Response(response))
            } else if value.get("method").is_some() {
                // Request (server-initiated)
                let request: JsonRpcRequest = serde_json::from_str(line)?;
                Ok(IncomingMessage::Request(request))
            } else {
                Err(anyhow::anyhow!("Invalid message format: {}", line))
            }
        } else if value.get("method").is_some() {
            // Notification
            let notification: JsonRpcNotification = serde_json::from_str(line)?;
            Ok(IncomingMessage::Notification(notification))
        } else {
            Err(anyhow::anyhow!("Invalid message format: {}", line))
        }
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send_request(&mut self, request: &JsonRpcRequest) -> anyhow::Result<()> {
        let json = serde_json::to_string(request)?;
        debug!(id = %request.id, method = %request.method, "Sending request");
        self.send_line(&json).await
    }

    async fn send_notification(&mut self, notification: &JsonRpcNotification) -> anyhow::Result<()> {
        let json = serde_json::to_string(notification)?;
        debug!(method = %notification.method, "Sending notification");
        self.send_line(&json).await
    }

    async fn receive(&mut self) -> anyhow::Result<IncomingMessage> {
        if let Some(ref mut reader) = self.stdout_reader {
            match reader.next().await {
                Some(Ok(line)) => {
                    trace!(line = %line, "Received line from MCP server");
                    Self::parse_message(&line)
                }
                Some(Err(e)) => {
                    error!(error = %e, "Error reading from MCP server");
                    Err(anyhow::anyhow!("Error reading from MCP server: {}", e))
                }
                None => {
                    warn!("MCP server closed stdout");
                    Err(anyhow::anyhow!("MCP server closed stdout"))
                }
            }
        } else {
            Err(anyhow::anyhow!("stdout reader is not available"))
        }
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        // 先关闭 stdin
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.shutdown().await;
        }

        // 然后终止子进程
        if let Some(mut child) = self.child.take() {
            info!("Killing MCP server process");
            child.kill().await.map_err(|e| {
                anyhow::anyhow!("Failed to kill MCP server process: {}", e)
            })?;
        }

        Ok(())
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // 确保子进程被终止
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

/// SSE 传输实现
///
/// 通过 HTTP Server-Sent Events 进行 MCP 通信
pub struct SseTransport {
    /// HTTP 客户端
    client: reqwest::Client,
    /// SSE 端点 URL
    #[allow(dead_code)]
    sse_url: String,
    /// POST 端点 URL
    post_url: String,
    /// SSE 事件接收器
    event_receiver: Option<mpsc::Receiver<IncomingMessage>>,
    /// SSE 任务句柄
    sse_handle: Option<tokio::task::JoinHandle<()>>,
}

impl SseTransport {
    /// 创建新的 SSE 传输
    pub async fn new(base_url: &str) -> anyhow::Result<Self> {
        let client = reqwest::Client::new();
        let sse_url = format!("{}/sse", base_url.trim_end_matches('/'));
        let post_url = format!("{}/message", base_url.trim_end_matches('/'));

        info!(sse_url = %sse_url, post_url = %post_url, "Creating SSE transport");

        let (tx, rx) = mpsc::channel(100);
        let client_clone = client.clone();
        let sse_url_clone = sse_url.clone();

        // 启动 SSE 连接任务
        let handle = tokio::spawn(async move {
            if let Err(e) = Self::run_sse_connection(client_clone, &sse_url_clone, tx).await {
                error!(error = %e, "SSE connection error");
            }
        });

        Ok(Self {
            client,
            sse_url,
            post_url,
            event_receiver: Some(rx),
            sse_handle: Some(handle),
        })
    }

    /// 运行 SSE 连接
    async fn run_sse_connection(
        _client: reqwest::Client,
        url: &str,
        tx: mpsc::Sender<IncomingMessage>,
    ) -> anyhow::Result<()> {
        use reqwest_eventsource::{Event, EventSource};

        let mut event_source = EventSource::get(url);

        while let Some(event) = event_source.next().await {
            match event {
                Ok(Event::Message(message)) => {
                    if let Ok(incoming) = Self::parse_sse_message(&message.data) {
                        if tx.send(incoming).await.is_err() {
                            break;
                        }
                    }
                }
                Ok(Event::Open) => {
                    debug!("SSE connection opened");
                }
                Err(e) => {
                    error!(error = %e, "SSE connection error");
                    break;
                }
            }
        }

        event_source.close();
        Ok(())
    }

    /// 解析 SSE 消息
    fn parse_sse_message(data: &str) -> anyhow::Result<IncomingMessage> {
        // SSE 消息格式与 Stdio 相同
        StdioTransport::parse_message(data)
    }
}

#[async_trait]
impl Transport for SseTransport {
    async fn send_request(&mut self, request: &JsonRpcRequest) -> anyhow::Result<()> {
        let json = serde_json::to_string(request)?;
        debug!(id = %request.id, method = %request.method, "Sending request via SSE");

        let response = self
            .client
            .post(&self.post_url)
            .header("Content-Type", "application/json")
            .body(json)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to send request: status={}, body={}",
                status,
                body
            ));
        }

        Ok(())
    }

    async fn send_notification(&mut self, notification: &JsonRpcNotification) -> anyhow::Result<()> {
        let json = serde_json::to_string(notification)?;
        debug!(method = %notification.method, "Sending notification via SSE");

        let response = self
            .client
            .post(&self.post_url)
            .header("Content-Type", "application/json")
            .body(json)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to send notification: status={}, body={}",
                status,
                body
            ));
        }

        Ok(())
    }

    async fn receive(&mut self) -> anyhow::Result<IncomingMessage> {
        if let Some(ref mut rx) = self.event_receiver {
            rx.recv()
                .await
                .ok_or_else(|| anyhow::anyhow!("SSE event channel closed"))
        } else {
            Err(anyhow::anyhow!("SSE event receiver not available"))
        }
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        // 关闭 SSE 任务
        if let Some(handle) = self.sse_handle.take() {
            handle.abort();
        }
        Ok(())
    }
}

/// Mock 传输实现（用于测试）
#[cfg(test)]
pub struct MockTransport {
    /// 待返回的响应队列
    responses: std::collections::VecDeque<IncomingMessage>,
    /// 已发送的消息记录
    pub sent: Vec<String>,
}

#[cfg(test)]
impl MockTransport {
    /// 创建新的 Mock 传输
    pub fn new() -> Self {
        Self {
            responses: std::collections::VecDeque::new(),
            sent: Vec::new(),
        }
    }

    /// 添加响应到队列
    pub fn push_response(&mut self, response: JsonRpcResponse) {
        self.responses.push_back(IncomingMessage::Response(response));
    }

    /// 添加通知到队列
    pub fn push_notification(&mut self, notification: JsonRpcNotification) {
        self.responses
            .push_back(IncomingMessage::Notification(notification));
    }

    /// 添加请求到队列
    pub fn push_request(&mut self, request: JsonRpcRequest) {
        self.responses.push_back(IncomingMessage::Request(request));
    }

    /// 获取最后发送的消息
    pub fn last_sent(&self) -> Option<&str> {
        self.sent.last().map(|s| s.as_str())
    }

    /// 解析最后发送的请求
    pub fn last_sent_request(&self) -> Option<JsonRpcRequest> {
        self.sent
            .last()
            .and_then(|s| serde_json::from_str(s).ok())
    }
}

#[cfg(test)]
#[async_trait]
impl Transport for MockTransport {
    async fn send_request(&mut self, request: &JsonRpcRequest) -> anyhow::Result<()> {
        let json = serde_json::to_string(request)?;
        self.sent.push(json);
        Ok(())
    }

    async fn send_notification(
        &mut self,
        notification: &JsonRpcNotification,
    ) -> anyhow::Result<()> {
        let json = serde_json::to_string(notification)?;
        self.sent.push(json);
        Ok(())
    }

    async fn receive(&mut self) -> anyhow::Result<IncomingMessage> {
        self.responses
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("No more responses in mock"))
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

// === 单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId};

    #[test]
    fn test_transport_type_serialization() {
        let t = TransportType::Stdio;
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#""stdio""#);

        let t: TransportType = serde_json::from_str(r#""sse""#).unwrap();
        assert!(matches!(t, TransportType::Sse));
    }

    #[tokio::test]
    async fn test_mock_transport() {
        let mut transport = MockTransport::new();

        // 添加响应
        transport.push_response(JsonRpcResponse::success(
            RequestId::Number(1),
            serde_json::json!({ "result": "ok" }),
        ));

        // 发送请求
        let request = JsonRpcRequest::new(RequestId::Number(1), "test", None);
        transport.send_request(&request).await.unwrap();

        // 检查发送的消息
        let sent = transport.last_sent().unwrap();
        assert!(sent.contains(r#""method":"test""#));

        // 接收响应
        let incoming = transport.receive().await.unwrap();
        if let IncomingMessage::Response(response) = incoming {
            assert_eq!(response.id, RequestId::Number(1));
            assert!(!response.is_error());
        } else {
            panic!("Expected Response");
        }
    }

    #[tokio::test]
    async fn test_mock_transport_notification() {
        let mut transport = MockTransport::new();

        transport.push_notification(JsonRpcNotification::new("test/notification", None));

        let incoming = transport.receive().await.unwrap();
        if let IncomingMessage::Notification(notification) = incoming {
            assert_eq!(notification.method, "test/notification");
        } else {
            panic!("Expected Notification");
        }
    }

    #[tokio::test]
    async fn test_mock_transport_multiple_requests() {
        let mut transport = MockTransport::new();

        // 发送多个请求
        transport
            .send_request(&JsonRpcRequest::new(RequestId::Number(1), "method1", None))
            .await
            .unwrap();
        transport
            .send_request(&JsonRpcRequest::new(RequestId::String("abc".to_string()), "method2", None))
            .await
            .unwrap();

        assert_eq!(transport.sent.len(), 2);

        let req1 = serde_json::from_str::<JsonRpcRequest>(&transport.sent[0]).unwrap();
        assert_eq!(req1.method, "method1");

        let req2 = serde_json::from_str::<JsonRpcRequest>(&transport.sent[1]).unwrap();
        assert_eq!(req2.method, "method2");
    }

    #[test]
    fn test_parse_message_response() {
        let line = r#"{"jsonrpc":"2.0","id":1,"result":{"status":"ok"}}"#;
        let msg = StdioTransport::parse_message(line).unwrap();
        match msg {
            IncomingMessage::Response(response) => {
                assert_eq!(response.id, RequestId::Number(1));
                assert!(response.result.is_some());
            }
            _ => panic!("Expected Response"),
        }
    }

    #[test]
    fn test_parse_message_error_response() {
        let line = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid Request"}}"#;
        let msg = StdioTransport::parse_message(line).unwrap();
        match msg {
            IncomingMessage::Response(response) => {
                assert!(response.is_error());
                let error = response.error.unwrap();
                assert_eq!(error.code, -32600);
            }
            _ => panic!("Expected Response"),
        }
    }

    #[test]
    fn test_parse_message_notification() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let msg = StdioTransport::parse_message(line).unwrap();
        match msg {
            IncomingMessage::Notification(notification) => {
                assert_eq!(notification.method, "notifications/initialized");
            }
            _ => panic!("Expected Notification"),
        }
    }

    #[test]
    fn test_parse_message_request() {
        let line = r#"{"jsonrpc":"2.0","id":"server-1","method":"sampling/createMessage","params":{"prompt":"test"}}"#;
        let msg = StdioTransport::parse_message(line).unwrap();
        match msg {
            IncomingMessage::Request(request) => {
                assert_eq!(request.id, RequestId::String("server-1".to_string()));
                assert_eq!(request.method, "sampling/createMessage");
            }
            _ => panic!("Expected Request"),
        }
    }
}
