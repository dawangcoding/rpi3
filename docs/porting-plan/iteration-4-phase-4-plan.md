# ITERATION-4 Phase 4: 稳定性与协议支持

## 概述

Phase 4 包含 8 个任务，分为 3 个可并行的功能组：
- **A组**: 网络稳定性（P4.1-P4.3）— 在 pi-ai crate 中增强错误处理
- **B组**: MCP 协议支持（P4.4-P4.6）— 新建 pi-mcp crate
- **C组**: 新 Provider（P4.7-P4.8）— 在 pi-ai crate 中新增 Provider

**预估工作量**: 4-5 周 (1 人全职)

**状态**: ✅ 已完成（commit 8aea02a，31 files, +4575/-245 lines，1038 测试通过）

---

## Task 1: 网络稳定性增强（P4.1 + P4.2 + P4.3）

**范围**: `crates/pi-ai/` 内的重试/恢复/退避逻辑

**P4.1 - API 连接超时重试**:
- 在 `crates/pi-ai/src/stream.rs` 中添加 `RetryConfig` 结构体（max_retries、timeout_ms）
- 在 `StreamOptions` 中添加 `retry_config` 字段（`crates/pi-ai/src/types.rs`）
- 为 `stream()` 和 `complete()` 函数添加超时检测和自动重试包装

**P4.2 - 流中断自动恢复**:
- 在 `crates/pi-ai/src/stream.rs` 中新增 `ResilientStream` 包装器
- 检测流中断（bytes_stream 返回错误或意外结束，无 `[DONE]` 标志）
- 自动重新发起请求并拼接已收到的内容
- 添加恢复状态回调（用于 UI 显示"恢复中..."）

**P4.3 - 指数退避重试策略**:
- 新建 `crates/pi-ai/src/retry.rs` 模块，实现 `RetryPolicy`
- 初始延迟 1s，每次翻倍，最大 30s，支持随机抖动（jitter）
- 替换 `openai.rs` 中现有的简单固定间隔重试为新的指数退避
- 对所有 Provider 的 `stream()` trait 实现统一重试包装（在 `stream.rs` 层面）
- 移除所有 Provider 内嵌的重试逻辑（openai/anthropic/google/mistral/azure_openai/openrouter/xai）

**涉及文件**:
- `crates/pi-ai/src/retry.rs` (新建)
- `crates/pi-ai/src/stream.rs` (修改，添加重试包装和 ResilientStream)
- `crates/pi-ai/src/types.rs` (修改，StreamOptions/SimpleStreamOptions 添加 retry_config)
- `crates/pi-ai/src/lib.rs` (修改，导出新模块)
- `crates/pi-ai/src/providers/openai.rs` (修改，移除旧重试逻辑)
- `crates/pi-ai/src/providers/anthropic.rs` (修改，移除内嵌重试)
- `crates/pi-ai/src/providers/google.rs` (修改，移除内嵌重试)
- `crates/pi-ai/src/providers/mistral.rs` (修改，移除内嵌重试)
- `crates/pi-ai/src/providers/azure_openai.rs` (修改，移除内嵌重试)
- `crates/pi-ai/src/providers/openrouter.rs` (修改，移除内嵌重试)
- `crates/pi-ai/src/providers/xai.rs` (修改，移除内嵌重试)

**验收标准**:
- ✅ 网络超时后自动重试，最多 3 次
- ✅ 流中断后自动恢复
- ✅ 重试间隔符合指数退避(1s, 2s, 4s...)，最大 30s
- ✅ 包含单元测试（8 个 retry 测试）

---

## Task 2: MCP 协议客户端（P4.4）

**范围**: 新建 `crates/pi-mcp/` crate

**实现内容**:
- `crates/pi-mcp/Cargo.toml` — 依赖 tokio, serde, serde_json, anyhow, tracing, async-trait
- `crates/pi-mcp/src/lib.rs` — crate 入口，导出公共 API
- `crates/pi-mcp/src/protocol.rs` — MCP 协议类型定义：
  - JSON-RPC 2.0 消息格式（Request, Response, Notification）
  - MCP 方法定义（initialize, tools/list, tools/call, resources/list, resources/read）
  - Capabilities 协商
  - 协议版本: 2024-11-05
- `crates/pi-mcp/src/transport.rs` — 传输层抽象：
  - `Transport` trait（send, receive）
  - `StdioTransport` — 通过子进程 stdin/stdout 通信
  - `SseTransport` — 通过 HTTP SSE 通信
  - `MockTransport` — 用于单元测试
- `crates/pi-mcp/src/client.rs` — MCP 客户端实现：
  - `McpClient` 结构体
  - 握手 + capabilities 协商（initialize / initialized）
  - 工具发现（tools/list）
  - 工具调用（tools/call）
  - 资源发现和读取
  - 请求/响应匹配（JSON-RPC id 跟踪）

**验收标准**:
- ✅ McpClient 可通过 stdio 和 SSE 连接 MCP Server
- ✅ 成功完成 initialize 握手
- ✅ 正确调用 tools/list 获取工具列表
- ✅ 包含单元测试（37 个测试）

---

## Task 3: MCP Server 管理（P4.5）

**范围**: `crates/pi-mcp/src/server.rs`

**实现内容**:
- `McpServerConfig` — Server 配置（command, args, env, cwd, transport_type）
- `McpServerManager` — Server 生命周期管理：
  - 从配置启动 Server 进程
  - 停止/重启 Server
  - 多 Server 管理（HashMap<String, McpServerHandle>）
  - 健康检查（定期 ping）
  - 日志收集（stderr 捕获）
- 配置文件解析 — 支持从 `~/.pi/mcp_servers.json` 或 YAML 读取 Server 配置

**验收标准**:
- ✅ 可通过配置启动/停止 MCP Server
- ✅ 支持多个 Server 并发运行
- ✅ Server 崩溃后可检测并报告
- ✅ 包含单元测试（7 个测试）

---

## Task 4: MCP 工具集成（P4.6）

**范围**: `crates/pi-mcp/` + `crates/pi-coding-agent/`

**实现内容**:
- `crates/pi-mcp/src/tools.rs` — MCP 工具桥接：
  - 将 MCP tools/list 结果转换为 `pi-ai` 的 `Tool` 类型
  - 将 Agent 的 tool call 转发为 MCP tools/call 请求
  - 结果转换回 Agent 格式
  - 命名空间隔离：`mcp_{server}_{tool}` 命名格式
- `crates/pi-coding-agent/src/core/tools/mcp.rs` — MCP 工具适配器：
  - `McpToolManager` — 管理 MCP Server 生命周期和工具调用
  - 在 Agent 启动时从 `~/.pi/mcp_servers.json` 发现并注册 MCP 工具
  - MCP 工具与内置工具共存（namespace 隔离）
  - 非阻塞初始化 — MCP 失败不阻止 Agent 启动

**涉及文件**:
- `crates/pi-mcp/src/tools.rs` (新建)
- `crates/pi-mcp/Cargo.toml` (修改，添加 pi-ai 依赖)
- `crates/pi-coding-agent/src/core/tools/mcp.rs` (新建)
- `crates/pi-coding-agent/src/core/tools/mod.rs` (修改，注册 MCP 工具)
- `crates/pi-coding-agent/src/core/agent_session.rs` (修改，初始化 MCP Manager)
- `crates/pi-coding-agent/Cargo.toml` (修改，添加 pi-mcp 依赖)

**验收标准**:
- ✅ MCP Server 的工具出现在 Agent 可用工具列表
- ✅ Agent 可调用 MCP 工具并获取结果
- ✅ MCP 工具与内置工具共存无冲突
- ✅ 包含单元测试（15 + 3 个测试）

---

## Task 5: Groq 和 Cerebras Provider 直连（P4.7 + P4.8）

**范围**: `crates/pi-ai/`

**实现内容**:
- `crates/pi-ai/src/providers/groq.rs` — Groq Provider：
  - 薄包装器，委托 OpenAiProvider 核心逻辑
  - base_url: `https://api.groq.com/openai/v1`
  - 返回 `Api::Groq`
  - 支持模型：llama-3.3-70b-versatile, llama-3.1-8b-instant, mixtral-8x7b-32768
- `crates/pi-ai/src/providers/cerebras.rs` — Cerebras Provider：
  - 薄包装器，委托 OpenAiProvider 核心逻辑
  - base_url: `https://api.cerebras.ai/v1`
  - 返回 `Api::Cerebras`
  - 支持模型：llama3.1-8b, llama3.1-70b
- 修改 `openai.rs` process_stream 中 Api 类型从硬编码 `Api::OpenAiChatCompletions` 改为 `model.api.clone()`
- 在 `models.rs` 注册 5 个新模型定义
- 在 `providers/mod.rs` 导出新 Provider
- 在 `lib.rs` init_providers() 中注册，Provider 总数 8 → 10

**验收标准**:
- ✅ `GROQ_API_KEY` 设置后可直接调用 Groq 模型
- ✅ `CEREBRAS_API_KEY` 设置后可直接调用 Cerebras 模型
- ✅ 流式响应正确解析
- ✅ 包含单元测试（4 个测试）

---

## 执行依赖和并行策略

```
Task 1 (网络稳定性)  ─────────────────────────→ 完成
Task 2 (MCP 客户端)  ──→ Task 3 (Server管理) ──→ Task 4 (工具集成) ──→ 完成
Task 5 (Groq/Cerebras) ──────────────────────→ 完成
```

- **Task 1、Task 2、Task 5 可并行**（互不依赖，涉及不同文件/模块）
- **Task 3 依赖 Task 2**（Server 管理需要 Client 基础）
- **Task 4 依赖 Task 2 + Task 3**（工具集成需要 Client 和 Server 管理）

## 新增 crate 配置

- `crates/pi-mcp/Cargo.toml` 注册到 workspace `Cargo.toml` 的 `members` 列表
- `pi-coding-agent` 的 `Cargo.toml` 添加 `pi-mcp = { path = "../pi-mcp" }` 依赖
- `pi-mcp` 的 `Cargo.toml` 添加 `pi-ai = { path = "../pi-ai" }` 依赖

## 代码审查修复记录

1. **RetryPolicy Default 无限递归** — `Self::default()` 改为 `Self::new(RetryConfig::default())`
2. **ResilientStream recovery future 生命周期** — 将恢复 future 保存为结构体字段，避免每次 poll 创建新 future 导致异步操作无法完成
