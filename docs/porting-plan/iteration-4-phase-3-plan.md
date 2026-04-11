# Vim 编辑器模式实现计划

## 背景

当前 `crates/pi-tui/src/components/editor.rs` 仅支持 Emacs 模式快捷键。Phase 3 要求添加完整的 Vim 编辑模式，包含 6 个核心功能模块。

## 架构决策

**模块化重构**: 当前 `editor.rs` 是单文件（1731 行）。需要将其转为模块目录结构：
```
crates/pi-tui/src/components/editor/
  mod.rs          -- 原 editor.rs 内容 + Vim 集成入口
  vim.rs          -- Vim 模式状态机 + 模式切换逻辑
  vim_commands.rs -- Vim 命令处理（移动/编辑/搜索/命令行）
```

**模式切换策略**: 在 `Editor` 结构体中添加 `vim_state: Option<VimState>` 字段。`None` 表示 Emacs 模式，`Some(...)` 表示 Vim 模式。`handle_input()` 根据此字段分发到不同处理路径。

---

## Task 1: Editor 模块化重构 + Vim 状态机基础

**范围**: 将 `editor.rs` 转为模块目录，创建 Vim 核心类型定义

**修改文件:**
- `crates/pi-tui/src/components/editor.rs` -> 移为 `editor/mod.rs`
- 新建 `crates/pi-tui/src/components/editor/vim.rs`

**实现内容:**

1. 将 `editor.rs` 重命名为 `editor/mod.rs`，添加 `mod vim; mod vim_commands;`
2. 在 `vim.rs` 中定义核心类型：
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum VimMode { Normal, Insert, Visual, VisualLine, Command }

   pub struct VimState {
       pub mode: VimMode,
       pub pending_keys: String,        // 多键命令缓冲（如 gg, dd）
       pub last_command: Option<VimCommand>, // 用于 . 重复
       pub command_line: String,        // : 命令行内容
       pub search_pattern: Option<String>,
       pub search_direction: SearchDirection,
       pub visual_start: Option<(usize, usize)>, // Visual 模式起点
       pub status_message: String,      // 状态栏消息
   }
   ```
3. 在 `Editor` 结构体中添加 `vim_state: Option<VimState>` 字段
4. 修改 `EditorConfig` 添加 `pub editor_mode: EditorMode` 枚举字段
5. 修改 `handle_input()` 添加 Vim 分发入口：
   ```rust
   fn handle_input(&mut self, data: &str) -> bool {
       if let Some(ref mut vim) = self.vim_state {
           self.handle_vim_input(data)
       } else {
           // 原有 Emacs 逻辑不变
       }
   }
   ```
6. 实现基础模式切换：Esc -> Normal, i/a/o -> Insert, v/V -> Visual

**验证**: 编译通过，现有测试全部通过，Vim 模式下 Esc/i 可切换 Normal/Insert

---

## Task 2: 基础移动命令

**范围**: Normal 模式下的光标移动命令

**新建/修改文件:**
- `crates/pi-tui/src/components/editor/vim_commands.rs`

**实现内容:**

| 按键 | 功能 | 复用方法 |
|------|------|---------|
| h/j/k/l | 方向移动 | `move_left/down/up/right()` |
| w | 下一个词首 | 新增 `vim_move_word_forward()` (Vim 语义) |
| b | 上一个词首 | `move_word_left()` |
| e | 词尾 | 新增 `vim_move_word_end()` |
| 0 | 行首 | `move_home()` |
| $ | 行尾 | `move_end()` |
| gg | 文件首 | `move_to_start()` |
| G | 文件尾 | `move_to_end()` |
| Ctrl+U | 半屏上滚 | 新增 `scroll_half_page_up()` |
| Ctrl+D | 半屏下滚 | 新增 `scroll_half_page_down()` |

注意：Vim 的 w/b/e 语义与 Emacs 的 `move_word_left/right` 略有不同（Vim 区分 word 和 WORD），需要独立实现。

**多键命令处理**: `gg` 需要通过 `pending_keys` 缓冲实现。输入 `g` 时缓存，再输入 `g` 时执行。

**验证**: 单元测试覆盖所有移动命令，包括边界情况（文件首尾、空行）

---

## Task 3: 编辑命令

**范围**: Normal 模式下的文本编辑命令

**修改文件:**
- `crates/pi-tui/src/components/editor/vim_commands.rs`
- `crates/pi-tui/src/components/editor/mod.rs` (添加辅助方法)

**实现内容:**

| 按键 | 功能 | 实现方式 |
|------|------|---------|
| dd | 删除当前行 | 调用 `delete_line()` + 将内容推入 kill_ring |
| yy | 复制当前行 | 读取当前行内容推入 kill_ring |
| p | 粘贴到光标后/下方 | 从 kill_ring 获取内容，按行/字符插入 |
| P | 粘贴到光标前/上方 | 类似 p 但位置不同 |
| x | 删除光标处字符 | 调用 `delete_char_after()` |
| r{char} | 替换光标字符 | 新增 `replace_char()` 方法 |
| u | 撤销 | 调用 `undo()` |
| Ctrl+R | 重做 | 调用 `redo()` |
| . | 重复上次命令 | 从 `last_command` 重新执行 |

**关键设计**: 
- Vim 的 yank/delete 需要区分"行模式"和"字符模式"粘贴。在 kill_ring 中存储时需标记类型。
- `.` 重复命令需要在 `VimState.last_command` 中记录可重复的命令类型和参数。

**验证**: 单元测试覆盖 dd/yy/p/x/r/u/Ctrl+R/.，包括行模式粘贴和字符模式粘贴

---

## Task 4: 命令行模式 + 搜索

**范围**: `:` 命令行和 `/` 搜索功能

**修改文件:**
- `crates/pi-tui/src/components/editor/vim_commands.rs`
- `crates/pi-tui/src/components/editor/vim.rs`
- `crates/pi-tui/src/components/editor/mod.rs` (渲染命令行/搜索)

**实现内容:**

命令行模式：
- `:` 进入 Command 模式，状态栏显示 `:` 前缀
- 输入命令字符，Backspace 删除，Enter 执行，Esc 取消
- `:w` -> 返回提交信号（类似 submit）
- `:q` -> 返回取消信号
- `:wq` -> 提交并退出

搜索模式：
- `/` 进入搜索输入，输入搜索串
- Enter 执行搜索，高亮第一个匹配
- `n` 跳到下一个匹配
- `N` 跳到上一个匹配  
- Esc 取消搜索

**搜索实现**: 在 `VimState` 中维护 `search_pattern` 和 `search_matches: Vec<(usize, usize)>`，每次搜索时扫描全文构建匹配列表。

**渲染集成**: `render()` 方法底部显示命令行/搜索输入；搜索匹配项高亮显示。

**验证**: 测试 :w/:q/:wq 返回正确信号，/search + n/N 正确跳转

---

## Task 5: Visual 模式选择

**范围**: v/V 进入 Visual 模式，选择区域操作

**修改文件:**
- `crates/pi-tui/src/components/editor/vim_commands.rs`
- `crates/pi-tui/src/components/editor/vim.rs`
- `crates/pi-tui/src/components/editor/mod.rs` (选择渲染)

**实现内容:**

- `v` 进入字符 Visual 模式 -> 设置 `visual_start` 为当前光标位置
- `V` 进入行 Visual 模式 -> 选择整行
- hjkl/w/b/e 扩展选择范围（更新 Editor 的 `selection` 字段）
- `d` 删除选择内容（调用 `delete_selection()`，内容入 kill_ring）
- `y` 复制选择内容（内容入 kill_ring，退出 Visual）
- `>` / `<` 缩进/反缩进选择行
- Esc 取消选择，返回 Normal 模式

**复用**: 现有 `Selection` 结构体和 `delete_selection()`/`get_selected_text()` 方法可直接复用。Visual 模式主要是维护 selection 的动态更新。

**验证**: 测试 v 选择 + d 删除、V 行选择 + y 复制、选择后 > 缩进

---

## Task 6: Vim 配置选项 + 状态栏 + 收尾

**范围**: 配置持久化、模式指示器、keybindings 集成

**修改文件:**
- `crates/pi-tui/src/components/editor/mod.rs`
- `crates/pi-tui/src/keybindings.rs`

**实现内容:**

1. `EditorConfig` 扩展：
   ```rust
   pub enum EditorMode { Emacs, Vim }
   pub struct EditorConfig {
       // 现有字段...
       pub editor_mode: EditorMode,
       pub relative_line_numbers: bool,
   }
   ```

2. 模式指示器：`render()` 底部显示当前模式文本
   - Normal: "-- NORMAL --"
   - Insert: "-- INSERT --"
   - Visual: "-- VISUAL --" / "-- VISUAL LINE --"
   - Command: 显示 `:` + 命令内容

3. `keybindings.rs` 添加 Vim 上下文键位定义：
   - context = "editor/vim/normal"
   - context = "editor/vim/insert"
   - context = "editor/vim/visual"

4. 新增 `get_mode_indicator()` 公共方法供外部组件查询模式状态

5. 补充完整的单元测试和集成测试

**验证**: 配置 `editor_mode: Vim` 后默认进入 Vim 模式，状态栏正确显示模式

---

## Task 7: 验证与代码审查

**范围**: 全量编译、测试、代码审查

1. `cargo build` 编译通过
2. `cargo test -p pi-tui` 全部测试通过
3. `cargo clippy -p pi-tui` 无警告
4. 代码审查：检查模式切换完整性、边界条件、与 Emacs 模式的隔离性

---

## 依赖关系

```
Task 1 (模块化+状态机) 
  -> Task 2 (移动命令) + Task 3 (编辑命令)  [可并行]
     -> Task 4 (命令行+搜索) + Task 5 (Visual模式)  [可并行]
        -> Task 6 (配置+收尾)
           -> Task 7 (验证+审查)
```

## 风险

- **模块化重构**: 将单文件转为目录模块需要修改 `mod.rs` 的 re-export，可能影响外部引用
- **快捷键冲突**: Vim 的 Esc 键与现有自动完成取消冲突，需在 Vim 模式下优先处理为模式切换
- **多键命令**: `gg`、`dd` 等双键命令需要超时或确认机制，避免卡在 pending 状态
