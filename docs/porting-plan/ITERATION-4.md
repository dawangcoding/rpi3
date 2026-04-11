# 四次迭代计划

## 概述

三次迭代（ITERATION-3）完成了 pi-mono 项目的核心功能完善，包括：
- **pi-ai**: 8 个 Provider 实现（Anthropic、OpenAI、Google、Mistral、Bedrock、Azure OpenAI、xAI、OpenRouter），Token 计数基础实现
- **pi-tui**: TUI 框架完整，包含差分渲染引擎、编辑器组件（Emacs 模式）、Markdown 渲染、键盘处理等
- **pi-agent**: Agent 运行时核心 100% 完成，保持稳定
- **pi-coding-agent**: 功能大幅完善，包括完整 TUI 交互模式、OAuth 认证框架、会话压缩、Fork 功能、扩展系统框架、HTML 导出

**ITERATION-3 结束时的状态：**
- **pi-ai**: 完成度 75%（核心 100%，Provider 85%，Token 计数 60%）
- **pi-tui**: 完成度 100%（核心 100%，组件 100%）
- **pi-agent**: 完成度 100%
- **pi-coding-agent**: 完成度 85%（工具 100%，交互 100%，扩展 50%，认证 60%）
- **整体完成度约 88%**

**ITERATION-3 遗留的关键问题：**
1. OAuth Provider 支持不完整（仅 Anthropic，缺少 OpenAI、Google）
2. Token 计数使用比率估算，缺少 Mistral、Gemini 的精确 tokenizer
3. 配置格式单一，仅支持 YAML
4. 缺少 Notebook 交互式代码执行功能
5. 编辑器仅支持 Emacs 快捷键，缺少 Vim 模式
6. 流式 API 缺少错误恢复机制
7. MCP (Model Context Protocol) 协议支持缺失
8. 扩展系统为编译时链接，缺少热重载能力
9. 快捷键不可自定义

**四次迭代的目标是：完善基础设施（OAuth、Token 计数、配置格式），增强核心工具能力（Notebook、Vim 模式），提升系统稳定性（流式错误恢复），支持前沿协议（MCP），最终实现生产级完整体验。**

---

## Phase 1: 基础设施完善（P0 最高优先级）

### 目标
补齐基础设施级别的关键功能，包括 OAuth Provider 完善、Token 计数精度提升、配置格式扩展。

### 任务列表

| 属性 | 值 |
|------|-----|
| **功能名称** | OpenAI OAuth 支持 |
| **当前状态** | 框架代码，未实现具体 Provider |
| **TS 参考** | `packages/coding-agent/src/core/auth/` |
| **预估工作量** | 中 |
| **详细描述** | 实现 OpenAI OAuth 认证支持：<br>1. 配置 OpenAI OAuth 端点（platform.openai.com）<br>2. 实现授权码流程（PKCE）<br>3. 本地回调服务器处理<br>4. Token 存储和刷新<br>5. 注册到 Provider 配置 |

| 属性 | 值 |
|------|-----|
| **功能名称** | Google OAuth 支持 |
| **当前状态** | 框架代码，未实现具体 Provider |
| **TS 参考** | Google Cloud OAuth 2.0 文档 |
| **预估工作量** | 中 |
| **详细描述** | 实现 Google OAuth 认证支持：<br>1. 配置 Google OAuth 端点（Google Cloud Console）<br>2. 支持 Google AI Studio 和 Vertex AI 认证<br>3. 实现授权码流程<br>4. 处理 Google 特有的 scope 配置<br>5. Token 刷新机制 |

| 属性 | 值 |
|------|-----|
| **功能名称** | Token 自动刷新完善 |
| **当前状态** | 基础框架 |
| **TS 参考** | OAuth 2.0 Refresh Token 流程 |
| **预估工作量** | 中 |
| **详细描述** | 完善 Token 自动刷新机制：<br>1. 检测 Token 过期时间（expiry 字段）<br>2. 过期前自动调用 refresh_token 端点<br>3. 更新存储的 Token<br>4. 刷新失败时回退到 API Key 或提示重新登录<br>5. 支持多 Provider 并发刷新 |

| 属性 | 值 |
|------|-----|
| **功能名称** | Mistral Tokenizer 集成 |
| **当前状态** | 使用比率估算（3.8） |
| **TS 参考** | `mistralrs` tokenizer 或 Hugging Face tokenizers |
| **预估工作量** | 中 |
| **详细描述** | 集成 Mistral 模型精确 tokenizer：<br>1. 调研 Mistral tokenizer 方案（SentencePiece 或 Hugging Face）<br>2. 实现 MistralTokenCounter<br>3. 支持 Mistral Small/Medium/Large/Codestral 系列<br>4. 缓存 tokenizer 模型避免重复加载<br>5. 回退策略（加载失败时使用估算） |

| 属性 | 值 |
|------|-----|
| **功能名称** | Gemini Tokenizer 集成 |
| **当前状态** | 使用比率估算（3.8） |
| **TS 参考** | Google `google-token` crate 或 Gemini API |
| **预估工作量** | 中 |
| **详细描述** | 集成 Gemini 模型精确 tokenizer：<br>1. 调研 Gemini tokenizer 方案<br>2. 实现 GeminiTokenCounter<br>3. 支持 Gemini Pro/Flash/Ultra 系列<br>4. 处理多模态内容的 Token 计数<br>5. 与 Google Provider 集成验证 |

| 属性 | 值 |
|------|-----|
| **功能名称** | .env 配置文件支持 |
| **当前状态** | 仅支持 YAML |
| **TS 参考** | `dotenvy` crate |
| **预估工作量** | 小 |
| **详细描述** | 实现 .env 配置文件支持：<br>1. 集成 `dotenvy` crate<br>2. 支持从 ~/.pi/.env 加载环境变量<br>3. 环境变量映射到配置结构<br>4. 优先级：命令行 > .env > 配置文件 > 默认值<br>5. 文档说明支持的变量名 |

| 属性 | 值 |
|------|-----|
| **功能名称** | JSON 配置文件支持 |
| **当前状态** | 仅支持 YAML |
| **TS 参考** | `serde_json` crate |
| **预估工作量** | 小 |
| **详细描述** | 实现 JSON 配置文件支持：<br>1. 配置文件格式自动检测（.json 后缀）<br>2. 复用现有 serde 结构<br>3. 支持 ~/.pi/config.json<br>4. 错误提示（JSON 语法错误定位）<br>5. 与 YAML 配置等效功能 |

| 属性 | 值 |
|------|-----|
| **功能名称** | TOML 配置文件支持 |
| **当前状态** | 仅支持 YAML |
| **TS 参考** | `toml` crate |
| **预估工作量** | 小 |
| **详细描述** | 实现 TOML 配置文件支持（Rust 生态标准）：<br>1. 配置文件格式自动检测（.toml 后缀）<br>2. 复用现有 serde 结构<br>3. 支持 ~/.pi/config.toml<br>4. 错误提示（TOML 语法错误定位）<br>5. 作为 Rust 生态推荐格式重点支持 |

### 涉及文件

**主要修改：**
- `crates/pi-coding-agent/src/core/auth/providers.rs` - 添加 OpenAI、Google OAuth 配置
- `crates/pi-coding-agent/src/core/auth/oauth_server.rs` - 完善回调处理
- `crates/pi-coding-agent/src/core/auth/token_storage.rs` - Token 刷新逻辑
- `crates/pi-ai/src/token_counter.rs` - 添加 Mistral、Gemini TokenCounter
- `crates/pi-coding-agent/src/config.rs` - 多格式配置支持

**新增文件：**
- `crates/pi-ai/src/token_counter/mistral.rs` - Mistral TokenCounter
- `crates/pi-ai/src/token_counter/gemini.rs` - Gemini TokenCounter

**依赖文件：**
- `crates/pi-ai/src/providers/mistral.rs` - Token 计数集成验证
- `crates/pi-ai/src/providers/google.rs` - Token 计数集成验证

### 验证标准

1. `/login openai` 启动 OpenAI OAuth 流程，成功获取 Token
2. `/login google` 启动 Google OAuth 流程，成功获取 Token
3. Token 过期前自动刷新，API 调用无中断
4. Mistral 模型 Token 计数误差 < 5%（相比 API 返回 usage）
5. Gemini 模型 Token 计数误差 < 5%（相比 API 返回 usage）
6. ~/.pi/.env 文件中的环境变量正确加载
7. ~/.pi/config.json 配置文件正确解析
8. ~/.pi/config.toml 配置文件正确解析
9. 配置格式错误时提供清晰的错误提示

### 预估工作量

**总计：4-5 周**（1 人全职）

---

## Phase 2: Notebook 工具（P1）

### 目标
实现交互式代码执行 Notebook 工具，支持 Python 和 Node.js 代码块的执行、输出捕获和状态管理。

### 任务列表

| 属性 | 值 |
|------|-----|
| **功能名称** | Notebook Kernel 管理 |
| **当前状态** | 未实现 |
| **TS 参考** | Jupyter Kernel 协议 |
| **预估工作量** | 大 |
| **详细描述** | 实现 Notebook Kernel 生命周期管理：<br>1. Kernel 发现（检测系统 Python/Node.js）<br>2. Kernel 启动和停止<br>3. 多 Kernel 支持（Python、Node.js 并行）<br>4. Kernel 状态监控（健康检查）<br>5. Kernel 崩溃恢复 |

| 属性 | 值 |
|------|-----|
| **功能名称** | 代码执行沙箱 |
| **当前状态** | 未实现 |
| **TS 参考** | Jupyter Execute 请求 |
| **预估工作量** | 大 |
| **详细描述** | 实现安全的代码执行环境：<br>1. 子进程隔离执行代码<br>2. 超时控制（防止无限循环）<br>3. 内存限制（防止资源耗尽）<br>4. 输出捕获（stdout、stderr、图像）<br>5. 执行中断（Ctrl+C 传递） |

| 属性 | 值 |
|------|-----|
| **功能名称** | 代码块解析与渲染 |
| **当前状态** | 未实现 |
| **TS 参考** | Markdown 代码块扩展 |
| **预估工作量** | 中 |
| **详细描述** | 实现代码块的特殊渲染和交互：<br>1. 识别可执行代码块（带语言标记）<br>2. 代码块工具栏（执行按钮）<br>3. 执行结果显示（文本、表格、图像）<br>4. 错误高亮显示<br>5. 执行历史记录 |

| 属性 | 值 |
|------|-----|
| **功能名称** | Notebook 状态持久化 |
| **当前状态** | 未实现 |
| **TS 参考** | Jupyter Notebook 格式 |
| **预估工作量** | 中 |
| **详细描述** | 实现 Notebook 状态保存和恢复：<br>1. 变量状态序列化<br>2. 执行结果缓存<br>3. Notebook 文件格式（.pinb）<br>4. 导出为 Jupyter Notebook<br>5. 从 Jupyter Notebook 导入 |

| 属性 | 值 |
|------|-----|
| **功能名称** | Notebook 工具集成 |
| **当前状态** | 未实现 |
| **TS 参考** | 工具调用框架 |
| **预估工作量** | 中 |
| **详细描述** | 将 Notebook 作为 Agent 工具集成：<br>1. 注册 notebook_execute 工具<br>2. LLM 自动检测代码执行需求<br>3. 执行结果返回给 LLM<br>4. 支持多轮代码迭代<br>5. 与现有工具（BashTool、FileEditTool）协作 |

### 涉及文件

**新增文件：**
- `crates/pi-coding-agent/src/core/tools/notebook/` - Notebook 工具目录
- `crates/pi-coding-agent/src/core/tools/notebook/mod.rs` - Notebook 模块定义
- `crates/pi-coding-agent/src/core/tools/notebook/kernel.rs` - Kernel 管理
- `crates/pi-coding-agent/src/core/tools/notebook/executor.rs` - 代码执行
- `crates/pi-coding-agent/src/core/tools/notebook/state.rs` - 状态管理
- `crates/pi-coding-agent/src/core/tools/notebook/format.rs` - 文件格式

**修改文件：**
- `crates/pi-coding-agent/src/core/tools/mod.rs` - 注册 Notebook 工具
- `crates/pi-coding-agent/src/core/tools/types.rs` - 扩展工具类型
- `crates/pi-tui/src/components/markdown.rs` - 代码块渲染增强

### 验证标准

1. Agent 检测到代码执行需求时调用 notebook_execute 工具
2. Python 代码在隔离进程中执行，输出正确捕获
3. Node.js 代码在隔离进程中执行，输出正确捕获
4. 无限循环代码被超时中断
5. 代码执行错误正确显示堆栈跟踪
6. Notebook 状态可在会话间保存和恢复
7. 支持导出为 .ipynb 格式
8. 与 FileEditTool 协作（读取文件后在 Notebook 中处理）

### 预估工作量

**总计：2-3 周**（1 人全职）

---

## Phase 3: Vim 编辑器模式（P1）

### 目标
为 pi-tui 的 Editor 组件添加完整的 Vim 编辑模式支持，包括 Normal/Insert/Visual 模式切换和基础命令集。

### 任务列表

| 属性 | 值 |
|------|-----|
| **功能名称** | Vim 模式状态机 |
| **当前状态** | 仅 Emacs 模式 |
| **TS 参考** | Vim 模式定义 |
| **预估工作量** | 大 |
| **详细描述** | 实现 Vim 模式状态管理：<br>1. Normal 模式（默认，命令模式）<br>2. Insert 模式（文本输入）<br>3. Visual 模式（文本选择）<br>4. 模式切换逻辑（i/a/o 进入 Insert，Esc 返回 Normal）<br>5. 模式指示器（状态栏显示当前模式） |

| 属性 | 值 |
|------|-----|
| **功能名称** | 基础移动命令 |
| **当前状态** | 未实现 |
| **TS 参考** | Vim 移动命令 |
| **预估工作量** | 中 |
| **详细描述** | 实现基础光标移动命令：<br>1. h/j/k/l 方向移动<br>2. w/b 词首跳转<br>3. e 词尾跳转<br>4. 0/$ 行首/行尾<br>5. gg/G 文件首/尾<br>6. Ctrl+U/Ctrl+D 半屏滚动 |

| 属性 | 值 |
|------|-----|
| **功能名称** | 编辑命令 |
| **当前状态** | 未实现 |
| **TS 参考** | Vim 编辑命令 |
| **预估工作量** | 中 |
| **详细描述** | 实现基础编辑命令：<br>1. dd 删除行<br>2. yy 复制行<br>3. p/P 粘贴<br>4. x 删除字符<br>5. r 替换字符<br>6. u/Ctrl+R 撤销/重做<br>7. . 重复上次操作 |

| 属性 | 值 |
|------|-----|
| **功能名称** | 命令行模式 |
| **当前状态** | 未实现 |
| **TS 参考** | Vim Ex 命令 |
| **预估工作量** | 中 |
| **详细描述** | 实现命令行模式：<br>1. :w 保存（提交输入）<br>2. :q 退出（取消输入）<br>3. :wq 保存并退出<br>4. / 搜索<br>5. n/N 下一个/上一个匹配<br>6. 命令行历史（Up/Down） |

| 属性 | 值 |
|------|-----|
| **功能名称** | Visual 模式选择 |
| **当前状态** | 未实现 |
| **TS 参考** | Vim Visual 模式 |
| **预估工作量** | 中 |
| **详细描述** | 实现 Visual 模式文本选择：<br>1. v 进入字符 Visual 模式<br>2. V 进入行 Visual 模式<br>3. 选择后执行操作（d/y/>
4. 选择区域高亮显示<br>5. Esc 取消选择 |

| 属性 | 值 |
|------|-----|
| **功能名称** | Vim 配置选项 |
| **当前状态** | 未实现 |
| **TS 参考** | Vim 设置 |
| **预估工作量** | 小 |
| **详细描述** | 实现 Vim 模式配置：<br>1. 默认编辑器模式配置（Emacs/Vim）<br>2. 临时模式切换命令<br>3. 显示行号选项<br>4. 相对行号选项 |

### 涉及文件

**主要修改：**
- `crates/pi-tui/src/components/editor.rs` - 添加 Vim 模式支持
- `crates/pi-tui/src/keybindings.rs` - 添加 Vim 键位映射

**新增文件：**
- `crates/pi-tui/src/components/editor/vim.rs` - Vim 模式实现
- `crates/pi-tui/src/components/editor/vim_commands.rs` - Vim 命令处理

**依赖文件：**
- `crates/pi-tui/src/undo_stack.rs` - 撤销/重做集成
- `crates/pi-tui/src/kill_ring.rs` - 剪贴板集成

### 验证标准

1. 按 Esc 进入 Normal 模式，状态栏显示 "-- NORMAL --"
2. 按 i 进入 Insert 模式，状态栏显示 "-- INSERT --"
3. 按 v 进入 Visual 模式，状态栏显示 "-- VISUAL --"
4. hjkl 正确移动光标
5. dd 删除当前行，p 粘贴
6. /search 正确搜索文本，n 跳转下一个
7. :w 提交输入，:q 取消输入
8. u 撤销操作，Ctrl+R 重做
9. 配置文件中可设置默认编辑器模式

### 预估工作量

**总计：2-3 周**（1 人全职）

---

## Phase 4: 稳定性与协议支持（P1-P2）

### 目标
提升系统稳定性（流式错误恢复），支持前沿协议（MCP），扩展更多 Provider 直连。

### 任务列表

| 属性 | 值 |
|------|-----|
| **功能名称** | API 连接超时重试 |
| **当前状态** | 基础错误处理 |
| **TS 参考** | `reqwest` retry 中间件 |
| **预估工作量** | 中 |
| **详细描述** | 实现 API 连接超时自动重试：<br>1. 配置连接超时（默认 30s）<br>2. 配置请求超时（默认 120s）<br>3. 超时错误检测<br>4. 自动重试机制（最多 3 次）<br>5. 重试间隔递增 |

| 属性 | 值 |
|------|-----|
| **功能名称** | 流中断自动恢复 |
| **当前状态** | 流中断后失败 |
| **TS 参考** | SSE 重连机制 |
| **预估工作量** | 中 |
| **详细描述** | 实现流式响应中断恢复：<br>1. 检测流中断（连接重置、超时）<br>2. 自动重连并恢复流<br>3. 断点续传（从最后接收的消息继续）<br>4. 用户提示（显示恢复中状态）<br>5. 恢复失败时优雅降级 |

| 属性 | 值 |
|------|-----|
| **功能名称** | 指数退避重试策略 |
| **当前状态** | 固定重试间隔 |
| **TS 参考** | Exponential Backoff |
| **预估工作量** | 小 |
| **详细描述** | 实现指数退避重试策略：<br>1. 初始延迟 1s<br>2. 每次重试延迟翻倍（1s, 2s, 4s, 8s...）<br>3. 最大延迟上限（30s）<br>4. 可配置退避参数<br>5. 支持抖动（jitter）避免 thundering herd |

| 属性 | 值 |
|------|-----|
| **功能名称** | MCP 协议客户端 |
| **当前状态** | 未实现 |
| **TS 参考** | Model Context Protocol 规范 |
| **预估工作量** | 大 |
| **详细描述** | 实现 MCP 协议客户端：<br>1. MCP 握手协议<br>2. Context 资源发现<br>3. Context 读取和订阅<br>4. 工具调用协议<br>5. 采样（Sampling）请求处理<br>6. 传输层（stdio、SSE） |

| 属性 | 值 |
|------|-----|
| **功能名称** | MCP Server 管理 |
| **当前状态** | 未实现 |
| **TS 参考** | MCP Server 配置 |
| **预估工作量** | 中 |
| **详细描述** | 实现 MCP Server 生命周期管理：<br>1. Server 配置（命令、参数、环境变量）<br>2. Server 启动和停止<br>3. 多 Server 支持<br>4. Server 健康检查<br>5. Server 日志收集 |

| 属性 | 值 |
|------|-----|
| **功能名称** | MCP 工具集成 |
| **当前状态** | 未实现 |
| **TS 参考** | 工具调用框架 |
| **预估工作量** | 中 |
| **详细描述** | 将 MCP 工具集成到 Agent：<br>1. MCP 工具发现和注册<br>2. 工具描述转换<br>3. 工具调用转发<br>4. 结果返回给 LLM<br>5. 与内置工具共存 |

| 属性 | 值 |
|------|-----|
| **功能名称** | Groq Provider 直连 |
| **当前状态** | 通过 OpenRouter 访问 |
| **TS 参考** | Groq API 文档 |
| **预估工作量** | 小 |
| **详细描述** | 实现 Groq 直连 Provider：<br>1. 配置 Groq base URL（https://api.groq.com）<br>2. 复用 OpenAI 兼容接口逻辑<br>3. 支持 Groq 模型系列（Llama、Mixtral、Gemma）<br>4. 注册到模型注册表<br>5. 流式响应支持 |

| 属性 | 值 |
|------|-----|
| **功能名称** | Cerebras Provider 直连 |
| **当前状态** | 通过 OpenRouter 访问 |
| **TS 参考** | Cerebras API 文档 |
| **预估工作量** | 小 |
| **详细描述** | 实现 Cerebras 直连 Provider：<br>1. 配置 Cerebras base URL<br>2. 复用 OpenAI 兼容接口逻辑<br>3. 支持 Cerebras 模型系列<br>4. 注册到模型注册表<br>5. 流式响应支持 |

### 涉及文件

**主要修改：**
- `crates/pi-ai/src/stream.rs` - 流式错误恢复
- `crates/pi-ai/src/providers/` - 重试逻辑集成
- `crates/pi-coding-agent/src/core/agent_session.rs` - MCP 集成

**新增文件：**
- `crates/pi-mcp/` - 新建 MCP crate
- `crates/pi-mcp/src/lib.rs` - MCP 库入口
- `crates/pi-mcp/src/client.rs` - MCP 客户端
- `crates/pi-mcp/src/protocol.rs` - MCP 协议定义
- `crates/pi-mcp/src/transport.rs` - 传输层
- `crates/pi-mcp/src/server.rs` - Server 管理
- `crates/pi-ai/src/providers/groq.rs` - Groq Provider
- `crates/pi-ai/src/providers/cerebras.rs` - Cerebras Provider

**修改文件：**
- `crates/pi-ai/src/providers/mod.rs` - 注册新 Provider
- `crates/pi-ai/src/models.rs` - 添加新模型
- `Cargo.toml` - 添加 pi-mcp crate

### 验证标准

1. 网络超时后自动重试，最多 3 次
2. 流中断后自动恢复，用户看到 "恢复中..." 提示
3. 重试间隔符合指数退避（1s, 2s, 4s）
4. MCP Server 配置后正确启动
5. MCP 工具出现在 Agent 可用工具列表
6. Agent 可以调用 MCP 工具并获取结果
7. Groq Provider 可直接调用，不经过 OpenRouter
8. Cerebras Provider 可直接调用，不经过 OpenRouter
9. 所有新 Provider 支持流式响应

### 预估工作量

**总计：4-5 周**（1 人全职）

---

## Phase 5: 扩展系统与体验优化（P2）

### 目标
实现扩展热重载能力，添加快捷键自定义功能，提升用户体验。

### 任务列表

| 属性 | 值 |
|------|-----|
| **功能名称** | 扩展动态加载方案调研 |
| **当前状态** | 编译时链接 |
| **TS 参考** | Rust 插件系统方案 |
| **预估工作量** | 中 |
| **详细描述** | 调研并确定扩展动态加载方案：<br>1. dylib 动态库方案（libloading）<br>2. WASM 方案（wasmtime）<br>3. IPC 方案（独立进程通信）<br>4. 各方案安全性、性能、复杂度对比<br>5. 选择最适合的方案 |

| 属性 | 值 |
|------|-----|
| **功能名称** | 扩展动态加载实现 |
| **当前状态** | 编译时链接 |
| **TS 参考** | 选定方案实现 |
| **预估工作量** | 大 |
| **详细描述** | 实现扩展动态加载：<br>1. 扩展发现（~/.pi/extensions/）<br>2. 扩展加载（运行时加载）<br>3. 扩展初始化（调用 init 函数）<br>4. 扩展卸载（清理资源）<br>5. 加载失败隔离（不影响主程序） |

| 属性 | 值 |
|------|-----|
| **功能名称** | 扩展热重载 |
| **当前状态** | 未实现 |
| **TS 参考** | 文件监控 |
| **预估工作量** | 中 |
| **详细描述** | 实现扩展热重载：<br>1. 文件系统监控（notify crate）<br>2. 扩展文件变化检测<br>3. 自动卸载旧版本<br>4. 自动加载新版本<br>5. 重载状态提示 |

| 属性 | 值 |
|------|-----|
| **功能名称** | 扩展安全沙箱 |
| **当前状态** | 未实现 |
| **TS 参考** | WASI 或 seccomp |
| **预估工作量** | 大 |
| **详细描述** | 实现扩展安全隔离：<br>1. 文件系统访问限制（只允许访问工作目录）<br>2. 网络访问限制（需声明权限）<br>3. 资源使用限制（CPU、内存）<br>4. 系统调用过滤<br>5. 崩溃隔离（扩展崩溃不影响主程序） |

| 属性 | 值 |
|------|-----|
| **功能名称** | KeybindingsManager UI |
| **当前状态** | 硬编码快捷键 |
| **TS 参考** | 设置界面 |
| **预估工作量** | 小 |
| **详细描述** | 实现快捷键配置界面：<br>1. 快捷键列表显示<br>2. 快捷键编辑（按键捕获）<br>3. 冲突检测<br>4. 恢复默认<br>5. 与 SettingsList 组件集成 |

| 属性 | 值 |
|------|-----|
| **功能名称** | 快捷键配置文件 |
| **当前状态** | 硬编码 |
| **TS 参考** | 配置系统 |
| **预估工作量** | 小 |
| **详细描述** | 实现快捷键配置文件：<br>1. 快捷键配置结构<br>2. 配置文件加载（~/.pi/keybindings.toml）<br>3. 运行时热加载<br>4. 格式验证<br>5. 错误提示 |

| 属性 | 值 |
|------|-----|
| **功能名称** | 快捷键导入/导出 |
| **当前状态** | 未实现 |
| **TS 参考** | 文件操作 |
| **预估工作量** | 小 |
| **详细描述** | 实现快捷键配置的导入导出：<br>1. 导出为文件（JSON/TOML）<br>2. 从文件导入<br>3. 预设快捷键方案（Emacs、Vim、VSCode）<br>4. 分享和同步 |

### 涉及文件

**主要修改：**
- `crates/pi-coding-agent/src/core/extensions/loader.rs` - 动态加载实现
- `crates/pi-coding-agent/src/core/extensions/runner.rs` - 扩展管理器
- `crates/pi-tui/src/keybindings.rs` - 快捷键配置支持

**新增文件：**
- `crates/pi-coding-agent/src/core/extensions/sandbox.rs` - 沙箱实现
- `crates/pi-coding-agent/src/core/extensions/hot_reload.rs` - 热重载
- `crates/pi-coding-agent/src/modes/keybindings_config.rs` - 快捷键配置界面

**依赖文件：**
- `crates/pi-tui/src/components/settings_list.rs` - 设置列表组件

### 验证标准

1. 扩展文件放入 ~/.pi/extensions/ 后自动加载
2. 修改扩展文件后自动热重载
3. 扩展崩溃后主程序继续运行
4. 扩展无法访问未授权的文件路径
5. /keybindings 命令打开快捷键配置界面
6. 按下快捷键后显示按键捕获界面
7. 快捷键配置保存到 ~/.pi/keybindings.toml
8. 可导入/导出快捷键配置
9. 预设方案（Emacs/Vim）可一键切换

### 预估工作量

**总计：3-4 周**（1 人全职）

---

## Phase 间依赖关系

```
Phase 1 (基础设施完善)
    │
    ├──→ Phase 2 (Notebook 工具) ───┐
    │                                │
    ├──→ Phase 3 (Vim 模式) ─────────┤
    │                                │
    └──→ Phase 4 (稳定性与协议) ─────┤
                                     │
                                     ↓
                            Phase 5 (扩展与体验)
```

**依赖说明：**
- Phase 1 是其他所有 Phase 的基础，提供稳定的基础设施
- Phase 2、3、4 可以并行进行（在 Phase 1 完成后）
- Phase 5 依赖于 Phase 1-4 的稳定性（在核心功能稳定后优化体验）

---

## 总体时间线估算

| Phase | 名称 | 预估时间 | 累计时间 |
|-------|------|----------|----------|
| Phase 1 | 基础设施完善 | 4-5 周 | 4-5 周 |
| Phase 2 | Notebook 工具 | 2-3 周 | 6-8 周 |
| Phase 3 | Vim 编辑器模式 | 2-3 周 | 6-8 周 |
| Phase 4 | 稳定性与协议支持 | 4-5 周 | 8-13 周 |
| Phase 5 | 扩展系统与体验优化 | 3-4 周 | 11-17 周 |

**总计：11-17 周**（约 3-4 个月，1 人全职）

**并行优化：**
- 如果 Phase 2-4 并行开发，可缩短至 **8-12 周**
- 需要 2-3 名开发者协作

---

## 完成后的预期状态

### 各模块完成度目标

| 模块 | 当前完成度 | 目标完成度 | 关键改进 |
|------|-----------|-----------|----------|
| **pi-ai** | 75% | 90% | +2 个 Provider，Token 计数精确化，流式错误恢复 |
| **pi-tui** | 100% | 100% | +Vim 模式，快捷键自定义 |
| **pi-agent** | 100% | 100% | 保持稳定 |
| **pi-coding-agent** | 85% | 95% | +Notebook 工具，OAuth 完善，配置格式扩展 |
| **pi-mcp** | 0% | 80% | 新建 crate，MCP 协议完整支持 |
| **整体** | 88% | **95-100%** | 生产级完整体验 |

### 功能完整性对比

| 功能 | ITERATION-3 | ITERATION-4 目标 | 原版 |
|------|-------------|------------------|------|
| OAuth 认证 | 框架（Anthropic） | 完整（Anthropic、OpenAI、Google） | 100% |
| Token 计数 | 比率估算 | 精确计数（OpenAI、Claude、Mistral、Gemini） | 100% |
| 配置格式 | YAML | YAML、JSON、TOML、.env | 80% |
| Notebook 工具 | 无 | 完整（Python、Node.js） | 100% |
| 编辑器模式 | Emacs | Emacs + Vim | 100% |
| 流式错误恢复 | 基础 | 完整（重试、恢复、退避） | 100% |
| MCP 支持 | 无 | 完整客户端 | 90% |
| Provider 支持 | 8 个 | 10 个（+Groq、Cerebras） | 22+ 个 |
| 扩展热重载 | 编译时链接 | 动态加载 + 热重载 | 80% |
| 快捷键自定义 | 无 | 完整配置界面 | 100% |

### 用户使用场景验证

1. **OAuth 认证完善**
   - `/login openai` 完成 OpenAI OAuth 认证
   - `/login google` 完成 Google OAuth 认证
   - Token 自动刷新，无需手动重新登录

2. **精确 Token 计数**
   - `/stats` 显示精确的 Token 使用统计
   - 成本估算更准确
   - 上下文窗口使用率实时监控

3. **灵活配置**
   - 使用 TOML 格式编写配置文件
   - 使用 .env 文件管理敏感信息
   - 配置格式自动识别

4. **Notebook 交互式编程**
   - Agent 自动执行 Python 代码分析数据
   - 代码执行结果显示表格和图表
   - 变量状态在会话间保持

5. **Vim 高效编辑**
   - Vim 用户可以使用熟悉的快捷键
   - hjkl 移动，dd 删除，yy 复制
   - / 搜索，:w 提交

6. **稳定流式响应**
   - 网络波动时自动重试
   - 流中断后自动恢复
   - 无需手动重新发送请求

7. **MCP 生态接入**
   - 配置 MCP Server 接入外部工具
   - Agent 自动使用 MCP 工具
   - 与内置工具无缝协作

8. **个性化快捷键**
   - 自定义快捷键绑定
   - 导入 Vim/VSCode 预设方案
   - 快捷键配置云端同步

---

## 风险与缓解措施

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Vim 模式复杂度超预期 | Phase 3 延期 | 分阶段实现：先基础命令，后高级功能 |
| Notebook 沙箱安全性 | Phase 2 延期 | 使用成熟方案（Docker、seccomp），安全审计 |
| MCP 协议规范变化 | Phase 4 延期 | 关注规范更新，设计可扩展架构 |
| 扩展动态加载方案不确定 | Phase 5 延期 | 前期技术调研，准备备选方案 |
| Tokenizer 库依赖问题 | Phase 1 延期 | 准备多方案（Hugging Face、原生实现） |

---

## 附录：新增依赖预估

| Crate | 用途 | Phase |
|-------|------|-------|
| `mistralrs-tokenizer` | Mistral Token 计数 | Phase 1 |
| `tokenizers` | Hugging Face Tokenizer | Phase 1 |
| `dotenvy` | .env 文件支持 | Phase 1 |
| `tokio-process` | Notebook 进程管理 | Phase 2 |
| `warp` / `axum` | Notebook 内核通信 | Phase 2 |
| `wasmtime` | 扩展 WASM 运行时 | Phase 5 |
| `libloading` | 扩展 dylib 加载 | Phase 5 |
| `notify` | 文件系统监控（热重载） | Phase 5 |
| `backoff` / `tower-retry` | 指数退避重试 | Phase 4 |
| `reqwest-retry` | HTTP 重试中间件 | Phase 4 |

---

*文档版本: 1.0*
*创建日期: 2026-04-11*
*基于: ITERATION-3 完成状态*
