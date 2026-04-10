# ITERATION-3 Phase 1: 交互模式 TUI 完整集成开发计划

## 现状分析

当前 `interactive.rs`（659行）使用简化的 raw mode：
- **输入**: 已集成 pi-tui Editor，支持多行编辑和 Slash 命令补全
- **输出**: 通过 `StreamingBlock` 对当前流式消息做差分更新，但历史消息仅用 `write!()` 逐行输出
- **渲染**: 未使用 pi-tui 的 `Tui` 差分渲染引擎和虚拟视口
- **缺失**: 消息历史管理、虚拟滚动、@文件补全、输入历史、粘贴折叠、主题系统

pi-tui 框架已具备完整能力：
- `Tui` 差分渲染引擎（1053行），含 `VirtualViewport`、覆盖层系统
- `Editor` 组件（1638行），含撤销/重做、Kill ring、自动完成集成
- `Markdown` 组件（598行），含缓存优化
- `Component` trait：`render()` / `handle_input()` / `invalidate()`

## 目标架构

```
Tui (差分渲染引擎)
├─ Container (根组件)
│  ├─ MessageHistory (消息历史容器, 实现 Component)
│  │  ├─ UserMessageComponent (用户消息)
│  │  ├─ AssistantMessageComponent (助手消息)
│  │  │  ├─ ThinkingBlock (可折叠思考内容)
│  │  │  ├─ Markdown (文本内容)
│  │  │  └─ ToolCallDisplay (工具调用列表)
│  │  ├─ SystemMessageComponent (系统消息/状态)
│  │  └─ SeparatorComponent (消息分隔线)
│  ├─ StatusBar (状态栏: 模型名、Token数、会话名)
│  └─ EditorArea (输入区域, 包装 Editor)
└─ Overlays
   └─ AutocompletePopup (自动完成弹出层)
```

---

## Task 1: 消息组件体系创建（P0）

**目标**: 创建消息显示的组件化体系，为后续 Tui 集成打基础。

**新增文件**: `crates/pi-coding-agent/src/modes/message_components.rs`

**实现内容**:

1. **UserMessageComponent** - 用户消息组件
   - 显示用户头像/标识 + 消息内容
   - 支持编辑标记（已编辑的消息）
   - 实现 `Component` trait

2. **AssistantMessageComponent** - 助手消息组件
   - ThinkingBlock: 可折叠的思考内容（dim 样式 + 折叠/展开控制）
   - 文本内容: 使用 `Markdown` 组件渲染
   - 工具调用列表: 每个工具调用显示名称、参数摘要、状态（运行中/成功/失败）
   - 流式状态: 支持增量追加文本（`push_text`/`push_thinking`）

3. **ToolCallDisplayComponent** - 工具调用显示
   - 可折叠的工具调用详情
   - 显示工具名、耗时、结果摘要
   - 错误工具调用高亮显示

4. **StatusBarComponent** - 状态栏
   - 当前模型名称
   - Token 使用量 / 上下文窗口百分比
   - 会话名称
   - 流式状态指示器（加载动画）

5. **SeparatorComponent** - 消息间分隔线

**依赖**: `pi-tui::Component`, `pi-tui::Markdown`, `pi-agent::types::AgentMessage`

---

## Task 2: MessageHistory 消息历史容器（P0）

**目标**: 创建管理所有消息组件的容器，支持滚动和虚拟视口。

**新增文件**: `crates/pi-coding-agent/src/modes/message_history.rs`

**实现内容**:

1. **MessageHistory 结构体** - 管理消息列表和滚动状态
2. **MessageEntry 枚举** - User / Assistant / System / Separator
3. **核心方法**: add_user_message, add_assistant_message, current_streaming, scroll_up/down, clear
4. **实现 Component trait**: render() 从 scroll_offset 渲染可见消息

---

## Task 3: Tui 主循环集成（P0 核心）

**目标**: 将交互模式从直接 stdout 写入重构为使用 Tui 差分渲染引擎。

**修改文件**: `crates/pi-coding-agent/src/modes/interactive.rs`

**实现内容**:

1. 创建 InteractiveApp 结构体封装整个交互模式状态
2. 重构 run() 使用 tui.render() 代替 write!()
3. 事件处理更新 MessageHistory 组件
4. 命令处理保持不变

---

## Task 4: 自动完成增强（P1）

**目标**: 扩展自动完成系统，支持 @文件引用和模型名称补全。

- FileAutocompleteProvider: @文件路径补全
- ModelAutocompleteProvider: 模型名称补全
- 使用 CombinedAutocompleteProvider 组合

---

## Task 5: 输入历史管理（P1）

**目标**: 实现输入历史记录，支持 Up/Down 键导航。

- InputHistory 结构体（entries, cursor, draft）
- 集成到 Editor 键盘处理

---

## Task 6: 粘贴处理优化（P1）

**目标**: 实现大粘贴内容的折叠显示。

- Bracketed Paste 模式检测
- 大粘贴折叠（>10行）
- 粘贴期间防抖

---

## Task 7: 主题系统基础（P2）

**目标**: 实现基础的 Light/Dark 主题系统。

- Theme 结构体定义
- Dark/Light 预定义主题
- /theme 命令切换

---

## Task 间依赖关系

```
Task 1 (消息组件) ──→ Task 2 (消息历史) ──→ Task 3 (Tui集成) ──→ Task 4 (自动完成)
                                                              ──→ Task 5 (输入历史)
                                                              ──→ Task 6 (粘贴处理)
Task 1 ──────────────────────────────────────────────────────→ Task 7 (主题系统)
                                              Task 3 ────────→ Task 7
```

## 新增/修改文件汇总

**新增文件（5个）**:
- `crates/pi-coding-agent/src/modes/message_components.rs`
- `crates/pi-coding-agent/src/modes/message_history.rs`
- `crates/pi-coding-agent/src/modes/input_history.rs`
- `crates/pi-coding-agent/src/modes/theme.rs`
- `crates/pi-coding-agent/src/modes/autocomplete_providers.rs`

**重构文件（2个）**:
- `crates/pi-coding-agent/src/modes/interactive.rs` (659行 -> ~1500行)
- `crates/pi-coding-agent/src/modes/interactive_components.rs` (简化)

**小修改文件（1个）**:
- `crates/pi-coding-agent/src/modes/mod.rs`

## 验证标准

1. `cargo check` 和 `cargo clippy` 通过
2. 交互模式启动后显示完整 TUI 界面
3. 流式消息通过 Tui 差分渲染引擎更新
4. 多行输入支持 Shift+Enter 换行
5. `/` 触发 Slash 命令补全
6. `@` 触发文件引用补全
7. Up/Down 键浏览输入历史
8. 大粘贴内容自动折叠
9. Ctrl+C 中断 Agent、Ctrl+D 退出
10. `/theme dark` / `/theme light` 切换主题
11. 终端 resize 时布局正确调整
12. 所有现有命令继续正常工作

---

*文档版本: 1.0*
*创建日期: 2026-04-10*
*基于: ITERATION-3.md Phase 1*
