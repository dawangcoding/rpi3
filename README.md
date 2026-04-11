# pi-mono (Rust 移植版)

一个高性能的 AI Coding Agent，从 TypeScript 项目 [pi-mono](https://github.com/pi-mono/pi-mono) 复刻而来，使用 Rust 语言重新实现。

## 简介

pi 是一个智能编程助手，通过自然语言对话帮助开发者完成代码编写、文件操作、项目搜索等任务。它支持多种 LLM Provider（Anthropic、OpenAI、Google），具备强大的工具调用能力和优雅的终端交互体验。

## 特性亮点

- **19+ Provider 支持**: 统一抽象层支持 Anthropic Claude、OpenAI GPT、Google Gemini、DeepSeek、Qwen、Minimax、Huggingface、Moonshot、Vertex AI、GitHub Copilot 等 19+ 个平台
- **流式响应**: 实时显示 LLM 输出，支持 SSE 流式解析、中断自动恢复
- **强大的工具系统**: 7 个内置工具（Bash、Read、Write、Edit、Grep、Find、Ls）+ Notebook 工具，支持串行/并行执行
- **自定义 TUI 引擎**: 基于 crossterm 的差分渲染引擎，支持 Kitty 键盘协议、多行编辑器、Markdown 渲染
- **Vim/Emacs 编辑模式**: 完整的 Normal/Insert/Command/Visual Vim 模式 + Emacs 快捷键
- **技能系统**: 预设技能库（代码审查、重构、文档生成等），支持自定义技能导入
- **RPC 服务模式**: JSON-RPC 2.0 服务，便于集成到第三方工具
- **设置管理 TUI**: 可视化配置管理，支持热重载
- **会话管理**: 支持会话保存、恢复、Fork、压缩、导出 HTML
- **扩展系统**: WASM 动态加载 + 安全沙箱 + 热重载
- **OAuth 认证**: 完整 OAuth 流程 + 系统密钥链存储 + Token 自动刷新
- **MCP 协议**: Model Context Protocol 客户端支持
- **异步架构**: 基于 tokio 的全异步运行时，支持 CancellationToken 取消操作
- **性能基准测试**: criterion 基准测试，持续性能监控

## 项目结构

```
rpi3/
├── Cargo.toml              # Workspace 配置
├── CHANGELOG.md            # 变更日志
├── crates/
│   ├── pi-ai/              # LLM 统一 API 层 (19+ Provider)
│   ├── pi-tui/             # 终端 UI 引擎 (Vim/Emacs 模式)
│   ├── pi-agent/           # Agent 核心循环
│   ├── pi-coding-agent/    # CLI 入口和交互模式
│   └── pi-mcp/             # MCP 协议客户端
└── docs/
    ├── pi-mono-research.md
    └── porting-plan/
```

### Crate 依赖关系

```
pi-coding-agent (CLI 入口)
    ├── pi-agent (Agent 核心)
    │   └── pi-ai (LLM API)
    ├── pi-tui (TUI 引擎)
    ├── pi-ai (LLM API)
    └── pi-mcp (MCP 协议)
```

### 各 Crate 职责

| Crate | 职责 | 主要功能 |
|-------|------|----------|
| **pi-ai** | LLM 统一 API 层 | 19+ Provider 注册系统、流式响应、Token 计数、SSE 解析、JSON 流式解析、ResilientStream |
| **pi-tui** | TUI 引擎 | Component trait 系统、差分渲染、Vim/Emacs 编辑模式、Markdown 渲染、快捷键自定义 |
| **pi-agent** | Agent 核心 | AgentTool trait、事件系统、串行/并行工具执行、CancellationToken、Agent 循环 |
| **pi-coding-agent** | CLI 入口 | 参数解析、7 个内置工具 + Notebook、技能系统、RPC 服务、设置管理、会话管理 |
| **pi-mcp** | MCP 协议 | MCP 握手、Context 资源发现、工具调用协议、Server 管理 |

## 快速开始

### 环境要求

- Rust 1.75+ (推荐最新稳定版)
- 对应 LLM Provider 的 API Key

### 编译

```bash
# 克隆仓库
cd /Users/lzmcoding/Code/rpi3

# 开发构建
cargo build

# 发布构建（推荐）
cargo build --release
```

编译后的二进制文件位于 `target/release/pi`。

### 配置 API Key

根据你使用的 LLM Provider，设置对应的环境变量：

```bash
# Anthropic (Claude)
export ANTHROPIC_API_KEY="your-api-key"

# OpenAI (GPT)
export OPENAI_API_KEY="your-api-key"

# Google (Gemini)
export GOOGLE_API_KEY="your-api-key"
```

### 运行

```bash
# 交互模式（默认）
./target/release/pi

# 查看帮助
./target/release/pi --help

# 列出支持的模型
./target/release/pi --list-models

# 使用特定模型
./target/release/pi --model anthropic:claude-sonnet-4-20250514

# 直接执行命令
./target/release/pi "帮我写一个 Rust Hello World"
```

## CLI 用法

### 基本用法

```bash
pi [OPTIONS] [PROMPT]...
```

### 常用选项

| 选项 | 说明 | 示例 |
|------|------|------|
| `-m, --model <MODEL>` | 指定模型 (provider:model 格式) | `--model anthropic:claude-opus-4` |
| `--provider <PROVIDER>` | 指定 Provider | `--provider anthropic` |
| `--api-key <KEY>` | 直接传入 API Key | `--api-key sk-xxx` |
| `-t, --thinking <LEVEL>` | 思考级别 (off/minimal/low/medium/high/xhigh) | `--thinking high` |
| `-s, --session <PATH>` | 指定会话文件 | `--session ./my-session.json` |
| `-c, --continue` | 继续之前的会话 | `--continue` |
| `-r, --resume` | 从列表中选择会话恢复 | `--resume` |
| `-p, --print` | 非交互模式（直接输出） | `--print` |
| `--file <FILE>` | 添加文件到初始消息 | `--file src/main.rs` |
| `--list-models` | 列出所有支持的模型 | `--list-models` |
| `-v, --verbose` | 详细输出 | `--verbose` |
| `--offline` | 离线模式 | `--offline` |
| `--rpc` | 启动 JSON-RPC 服务模式 | `--rpc --rpc-port 8080` |
| `--input-file <FILE>` | 从文件读取输入 | `--input-file prompt.txt` |
| `--output-file <FILE>` | 输出到文件 | `--output-file result.txt` |
| `--json` | JSON 格式输出 | `--json` |
| `--batch` | 批处理模式 | `--batch` |

### 工具控制选项

| 选项 | 说明 |
|------|------|
| `--no-bash` | 禁用 Bash 工具 |
| `--no-edit` | 禁用 Edit 工具 |
| `--no-tools` | 禁用所有工具 |
| `--tools <LIST>` | 启用指定工具（逗号分隔） |
| `--no-stream` | 禁用流式输出 |

### 会话管理选项

| 选项 | 说明 |
|------|------|
| `--session-id <ID>` | 指定会话 ID |
| `--session-dir <DIR>` | 指定会话目录 |
| `--no-session` | 临时模式（不保存会话） |
| `--fork <ID>` | 从指定会话 Fork |
| `--export <PATH>` | 导出会话为 HTML |

### 系统提示词选项

| 选项 | 说明 |
|------|------|
| `--system-prompt <TEXT>` | 自定义系统提示词 |
| `--append-system-prompt <TEXT>` | 追加到系统提示词 |

## 内置工具说明

pi 内置 7 个工具供 Agent 调用：

| 工具 | 功能 | 用途 |
|------|------|------|
| **Bash** | 执行 Shell 命令 | 运行编译、测试、Git 操作等 |
| **Read** | 读取文件内容 | 查看源代码、配置文件 |
| **Write** | 写入文件 | 创建新文件 |
| **Edit** | 编辑文件 | 修改现有文件内容 |
| **Grep** | 文本搜索 | 在文件中搜索特定模式 |
| **Find** | 文件查找 | 按名称或模式查找文件 |
| **Ls** | 目录列表 | 列出目录内容 |

工具支持以下执行模式：
- **串行执行**: 按顺序逐个执行，适合有依赖关系的工具调用
- **并行执行**: 同时执行多个独立工具，提高效率

## 架构概览

### pi-ai: LLM 统一 API 层

```rust
// 核心抽象
- Provider trait: 统一不同 LLM Provider 的接口
- ApiRegistry: Provider 注册和管理系统
- Stream/Complete API: 流式和非流式调用
- SSE 解析器: 处理 Server-Sent Events 流
- JSON 流式解析器: 处理不完整的 JSON 数据
- ResilientStream: 流中断自动恢复
- TokenCounter: 精确 Token 计数
```

支持的 Provider (19+):

| Provider | 模型系列 | 认证方式 |
|----------|---------|----------|
| Anthropic | Claude 3.5/4 (Sonnet, Opus) | API Key, OAuth |
| OpenAI | GPT-4o, GPT-4-turbo, o1, o3 | API Key, OAuth |
| Google | Gemini 1.5/2.0 (Pro, Flash) | API Key, OAuth |
| Mistral | Mistral Small/Medium/Large, Codestral | API Key, OAuth |
| Bedrock | Claude, Llama (via AWS) | AWS Credentials |
| Azure OpenAI | GPT-4o, GPT-4-turbo | API Key, Azure AD |
| xAI | Grok-2, Grok-beta | API Key |
| OpenRouter | 多后端路由 | API Key, OAuth |
| Groq | Llama, Mixtral, Gemma | API Key |
| Cerebras | Llama, Mistral (高速) | API Key |
| DeepSeek | DeepSeek V3, R1 | API Key |
| Qwen | Qwen Max, Plus, Turbo | API Key |
| Minimax | abab6, abab5.5 | API Key |
| Huggingface | Llama, Mistral, Qwen (Inference) | Token, OAuth |
| Moonshot | Kimi k1, moonshot-v1 | API Key |
| OpenCode | OpenCode 模型 | API Key |
| Vertex AI | Gemini (Google Cloud) | Service Account, OAuth |
| Gemini CLI | Gemini (CLI) | CLI Auth |
| GitHub Copilot | GPT-4, Claude | GitHub Token, OAuth |

### pi-tui: 终端 UI 引擎

```rust
// 核心组件
- Component trait: 可组合 UI 组件系统
- Terminal: 差分渲染引擎，只更新变化的部分
- Editor: 多行文本编辑器，支持 Vim/Emacs 风格快捷键
- Markdown: Markdown 渲染组件
- Fuzzy: 模糊匹配引擎
- Keys: 键盘输入解析（支持 Kitty 协议）
- KeybindingsManager: 快捷键自定义系统
```

特性:
- 差分渲染：只重绘变化的区域，性能优异
- Kitty 键盘协议：支持组合键、修饰键
- Vim 模式：Normal/Insert/Command/Visual 完整支持
- Emacs 模式：Ctrl+A/E/K/Y 等
- 多行编辑：支持语法高亮、自动缩进
- 撤销/重做：完整的编辑历史管理
- 快捷键自定义：配置文件支持，导入/导出预设

### pi-agent: Agent 核心循环

```rust
// 核心抽象
- AgentTool trait: 工具接口定义
- AgentEvent: 事件系统（消息、工具调用、状态变化）
- AgentLoop: 主事件循环
- ToolExecutionMode: 串行/并行执行策略
- CancellationToken: 取消长时间运行的操作
```

Agent 循环流程:
1. 接收用户输入
2. 调用 LLM 获取响应
3. 解析工具调用请求
4. 执行工具（串行/并行）
5. 将结果返回给 LLM
6. 生成最终回复

### pi-coding-agent: CLI 入口

```rust
// 主要模块
- CLI Args: 命令行参数解析 (clap)
- Tools: 7 个内置工具 + Notebook 工具实现
- Skills: 技能系统框架 + 5 个内置技能
- RPC: JSON-RPC 2.0 服务模式
- Settings: TUI 设置管理界面
- Extensions: WASM 扩展系统 (动态加载+沙箱+热重载)
- Auth: OAuth 认证 + 系统密钥链
- Session Manager: 会话持久化管理
- System Prompt: 动态系统提示词生成
- Modes: 交互模式和非交互模式
```

## 开发指南

### 添加新的 LLM Provider

1. 在 `pi-ai/src/providers/` 创建新的 Provider 实现
2. 实现 `Provider` trait
3. 在 `pi-ai/src/api_registry.rs` 注册 Provider

### 添加新的工具

1. 在 `pi-coding-agent/src/core/tools/` 创建工具实现
2. 实现 `AgentTool` trait
3. 在 `pi-coding-agent/src/core/tools/mod.rs` 导出

### 添加新的 TUI 组件

1. 在 `pi-tui/src/components/` 创建组件
2. 实现 `Component` trait
3. 在 `pi-tui/src/components/mod.rs` 导出

### 运行测试

```bash
# 运行所有测试
cargo test

# 运行特定 crate 的测试
cargo test -p pi-ai
cargo test -p pi-tui
cargo test -p pi-agent
cargo test -p pi-coding-agent
```

### 代码检查

```bash
# 格式化代码
cargo fmt

# 运行 Clippy
cargo clippy --all-targets --all-features
```

## 技术栈

- **Rust** (Edition 2021)
- **tokio** - 异步运行时
- **reqwest** - HTTP 客户端（支持流式）
- **serde/serde_json** - 序列化/反序列化
- **crossterm** - 跨平台终端控制
- **clap** - 命令行参数解析
- **pulldown-cmark** - Markdown 渲染
- **futures/async-stream** - 异步流处理

## 致谢

本项目是对原始 TypeScript 项目 [pi-mono](https://github.com/pi-mono/pi-mono) 的完整 Rust 移植。

感谢 pi-mono 项目的开创性工作，为我们提供了优秀的产品设计和功能参考。Rust 版本在保持原有功能的基础上，充分利用了 Rust 的类型安全、零成本抽象和卓越性能特性。

---

**License**: MIT
