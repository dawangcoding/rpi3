//! 扩展系统
//!
//! 支持工具注册、命令注册和生命周期管理的扩展框架
//! 支持编译时链接扩展和运行时 WASM 扩展动态加载

pub mod types;
pub mod loader;
pub mod runner;
pub mod api;
pub mod builtin;
pub mod sandbox;
pub mod hot_reload;
pub mod events;
pub mod dispatcher;
pub mod registry;

pub use types::{
    SlashCommand,
    WasmExtension,
    WasmInstance,
    WasmExtensionManifest,
    ExtensionLoadError,
    // 沙箱相关类型
    SandboxConfig,
    PathPermission,
    NetworkPermission,
    ResourceLimits,
    Permission,
};
pub use loader::{ExtensionLoader, ExtensionRegistry};
pub use runner::ExtensionManager;
pub use api::ExtensionContext;
pub use sandbox::{WasmSandbox, WasiSandboxCtx};
pub use hot_reload::{HotReloader, HotReloadEvent, HotReloadStatus};
pub use events::{EventPriority, EventTypeFilter, EventSubscription, EventHandlerRegistry};
pub use dispatcher::{EventDispatcher, DispatchResult, ExtensionFinder, ExtensionMapFinder};
pub use registry::{ToolRegistry, CommandRegistry, ToolRegistration, CommandRegistration};

// 内部使用的重导出（不对外公开）
pub(crate) use loader::WasmExtensionLoader;
