# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-04-12

### Added - ITERATION-5

#### Phase 1: 扩展系统完善
- **EventDispatcher** 事件分发器，支持优先级排序和 StopPropagation
- **ToolRegistry/CommandRegistry** 动态注册表，运行时注册工具和命令
- **20+ AgentEvent** 事件类型（AgentStart/End, TurnStart/End, MessageSend/Receive/Chunk, ToolCall/Result, CommandExecute/Result 等）
- **Extension trait** 升级（`event_subscriptions()` 方法，向后兼容默认实现）
- **SubscriptionConfig** 订阅配置（优先级 + 类型过滤器）

#### Phase 2: Provider 补全
- **9 个新 Provider**: DeepSeek, Qwen, Minimax, Huggingface, Moonshot, OpenCode, Vertex AI, Gemini CLI, GitHub Copilot
- **13+ 新模型定义**: DeepSeek V3/R1, Qwen Max/Plus, Minimax abab6, Kimi k1, Huggingface Inference 等
- **Provider 总数从 10 增至 19**，覆盖国内外主流 LLM 平台

#### Phase 3: OAuth 完整化
- **4 个新 OAuth 配置**: Azure OpenAI, Mistral, Huggingface, OpenRouter
- **RefreshScheduler** Token 自动刷新调度器（过期前 5 分钟自动刷新）
- **系统密钥链集成** (keyring crate): macOS Keychain, Windows DPAPI, Linux Secret Service
- **加密文件降级方案**: AES-GCM 加密 + Argon2 密钥派生
- **Token 版本化存储**: v1→v2 自动迁移
- **Device Code Flow 支持**: 无浏览器环境认证

#### Phase 4: 功能特性增强
- **技能系统框架**: SkillRegistry + 5 个内置技能（code-review, refactor, document, test, explain）
- **JSON-RPC 2.0 服务模式**: hyper 1.x 实现，10 个方法（initialize, sendMessage, getMessages, executeTool, getTools, setModel, getModels, compactSession, getSkills, shutdown）
- **TUI 设置管理界面**: 12 个设置项，4 个分类（General, Provider, Editor, Extensions）
- **ConfigWatcher 配置热重载**: notify crate 文件监控，自动重载配置变更
- **CLI 增强**: `--rpc` 启动 RPC 服务，`--input-file`/`--output-file` 文件 I/O，`--json` JSON 输出，`--batch` 批处理模式

#### Phase 5: 质量保障
- **编译警告清零**: 99 → 0（dead_code, unused, clippy warnings）
- **Rustdoc 文档补全**: 760 个警告 → 0，所有公共 API 有文档注释
- **性能基准测试**: criterion benches（token_counter, markdown_render, editor_ops）
- **测试覆盖大幅提升**: 226+ 测试用例

### Changed - ITERATION-5
- ExtensionManager 集成 EventDispatcher/ToolRegistry/CommandRegistry
- agent_loop.rs 添加完整事件触发点
- Interactive 模式集成命令事件系统
- 示例扩展演示事件优先级和 StopPropagation

---

## [0.1.0] - 2026-03-XX

### Added - ITERATION-1~4

#### pi-ai: LLM 统一 API 层
- **10 个 Provider 实现**: Anthropic, OpenAI, Google, Mistral, Bedrock, Azure OpenAI, xAI, OpenRouter, Groq, Cerebras
- **Token 计数系统**: tiktoken-rs (OpenAI), tokenizers (Hugging Face), 字符估算
- **流式响应处理**: SSE 解析, JSON 流式解析, ResilientStream 中断恢复
- **重试机制**: 指数退避策略, 超时控制
- **统一 Provider trait**: stream() 和 complete() 方法

#### pi-tui: TUI 渲染引擎
- **差分渲染引擎**: 只更新变化区域，性能优异
- **11+ 组件**: Editor, Markdown, Input, Select, List, Spinner, Help, CodeBlock, Table, Image, SettingsList
- **Vim 编辑模式**: Normal/Insert/Command/Visual 模式，基础命令集
- **Emacs 编辑模式**: Ctrl+A/E/K/Y 等
- **Kitty 键盘协议**: 支持组合键、修饰键
- **快捷键自定义系统**: KeybindingsManager, 配置文件支持
- **模糊搜索**: FuzzyMatcher 引擎

#### pi-agent: Agent 运行时核心
- **AgentTool trait**: 工具接口定义
- **AgentLoop**: 主事件循环
- **AgentEvent**: 事件系统（消息、工具调用、状态变化）
- **ToolExecutionMode**: 串行/并行执行策略
- **CancellationToken**: 取消长时间运行的操作
- **上下文管理**: ContextManager 消息历史管理

#### pi-coding-agent: CLI 入口
- **7 个内置工具**: Bash, Read, Write, Edit, Grep, Find, Ls
- **OAuth 认证框架**: 本地回调服务器, PKCE 支持
- **会话管理**: 保存、恢复、Fork、压缩 (Compaction)
- **HTML 导出**: ANSI 转 HTML, Markdown 渲染, 独立文件
- **扩展系统框架**: Extension trait, ExtensionManager, WASM 动态加载
- **权限系统**: PermissionsController 细粒度控制
- **Notebook 工具**: Python/Node.js 代码执行, 状态持久化
- **多格式配置**: YAML, JSON, TOML, .env

#### pi-mcp: MCP 协议客户端
- **MCP 握手协议**: initialize, initialized
- **Context 资源发现**: 资源读取和订阅
- **工具调用协议**: tools/list, tools/call
- **传输层**: stdio, SSE
- **Server 管理**: 生命周期管理, 健康检查

### Changed - ITERATION-1~4
- 从 TypeScript pi-mono 项目完整移植到 Rust
- 充分利用 Rust 类型安全、零成本抽象、卓越性能特性
- 新增超越原版功能: Vim 模式, MCP 协议, 权限系统, WASM 扩展, ResilientStream

---

## 版本说明

### 版本号规则
- **主版本号**: 不兼容的 API 修改
- **次版本号**: 向后兼容的功能性新增
- **修订号**: 向后兼容的问题修正

### 发布周期
- **Major/Minor**: 每个 ITERATION 完成后发布
- **Patch**: Bug 修复和小的改进

[0.2.0]: https://github.com/pi-mono/rpi3/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/pi-mono/rpi3/releases/tag/v0.1.0
