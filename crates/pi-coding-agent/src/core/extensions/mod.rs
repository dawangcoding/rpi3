//! 扩展系统
//!
//! 支持工具注册、命令注册和生命周期管理的扩展框架
//! 支持编译时链接扩展和运行时 WASM 扩展动态加载

/// 扩展类型定义模块
pub mod types;
/// 扩展加载器模块
pub mod loader;
/// 扩展运行器模块
pub mod runner;
/// 扩展 API 模块
pub mod api;
/// 内置扩展模块
pub mod builtin;
#[allow(dead_code)] // WASM 扩展系统尚未完全集成
pub mod sandbox;
#[allow(dead_code)] // 热重载功能尚未完全集成
pub mod hot_reload;
#[allow(dead_code)] // 事件系统尚未完全集成
pub mod events;
#[allow(dead_code)] // 事件分发器尚未完全集成
pub mod dispatcher;
#[allow(dead_code)] // 工具/命令注册表尚未完全集成
pub mod registry;

// 类型重导出
pub use types::SlashCommand;
pub use loader::{ExtensionLoader, ExtensionRegistry};
pub use runner::ExtensionManager;
pub use api::ExtensionContext;

// 内部使用的重导出（不对外公开）
