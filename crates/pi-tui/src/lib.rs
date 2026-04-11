//! TUI 渲染引擎
#![warn(missing_docs)]
//!
//! pi-tui 是基于 crossterm 的终端用户界面渲染引擎，提供：
//!
//! - **差分渲染**: 最小化终端输出，只更新变化的行
//! - **组件系统**: 通过 [`Component`] trait 构建可组合的 UI 组件
//! - **覆盖层管理**: 支持弹出窗口、对话框等覆盖层
//! - **键盘处理**: 支持标准 ANSI CSI 序列和 Kitty keyboard protocol
//!
//! # 核心组件
//!
//! - [`Component`]: 所有 UI 组件的基础 trait
//! - [`Focusable`]: 可聚焦组件的 trait
//! - [`Container`][]: 容器组件，用于组合多个子组件
//! - [`OverlayHandle`][]: 覆盖层句柄，用于控制覆盖层的显示和隐藏
//!
//! # 内置组件
//!
//! - `Input`: 文本输入框
//! - `Editor`: 多行编辑器
//! - `SelectList`: 选择列表
//! - `Markdown`: Markdown 渲染
//! - `Loader`: 加载动画
//!
//! # 示例
//!
//! ```ignore
//! use pi_tui::{Tui, Component, Container};
//!
//! let mut tui = Tui::new(terminal);
//! tui.root().add_child(Box::new(my_component));
//! tui.render()?;
//! ```

/// TUI 核心模块
pub mod tui;
/// 终端模块
pub mod terminal;
/// 按键模块
pub mod keys;
/// 按键绑定模块
pub mod keybindings;
/// 自动补全模块
pub mod autocomplete;
/// 模糊搜索模块
pub mod fuzzy;
/// 剪切板模块
pub mod kill_ring;
/// 撤销栈模块
pub mod undo_stack;
/// 工具模块
pub mod utils;
/// 终端图片模块
pub mod terminal_image;
/// 组件模块
pub mod components;

// 重导出核心类型
pub use tui::{Tui, Component, Focusable, Container, OverlayHandle, OverlayOptions};
pub use terminal::Terminal;
pub use keys::Key;
