//! 扩展热重载实现
//!
//! 监控 ~/.pi/extensions/ 目录变化，自动卸载旧版扩展、加载新版，
//! 实现 WASM 扩展的热重载功能。

use notify::{Watcher, RecommendedWatcher, RecursiveMode, Event, EventKind, Config as NotifyConfig};
use std::sync::mpsc::{self, Receiver, Sender};
use std::path::{Path, PathBuf};
use std::time::Duration;
use anyhow::Result;

/// 热重载事件
#[derive(Debug, Clone)]
#[allow(dead_code)] // 热重载功能尚未完全集成
pub enum HotReloadEvent {
    /// 扩展文件被创建或修改
    ExtensionChanged(PathBuf),
    /// 扩展文件被删除
    ExtensionRemoved(PathBuf),
    /// 重载成功
    ReloadSuccess(String),  // extension_id
    /// 重载失败（保留旧版本）
    ReloadFailed(String, String),  // extension_id, error_message
    /// 监控错误
    WatchError(String),
}

/// 热重载状态
#[derive(Debug, Clone)]
pub struct HotReloadStatus {
    /// 是否正在监控
    pub watching: bool,
    /// 监控路径
    pub watch_path: PathBuf,
    /// 上次重载时间
    pub last_reload: Option<std::time::Instant>,
    /// 重载次数
    pub reload_count: u64,
    /// 上次错误信息
    pub last_error: Option<String>,
}

impl HotReloadStatus {
    fn new(watch_path: PathBuf) -> Self {
        Self {
            watching: false,
            watch_path,
            last_reload: None,
            reload_count: 0,
            last_error: None,
        }
    }
}

/// 扩展热重载器
pub struct HotReloader {
    watcher: Option<RecommendedWatcher>,
    watch_path: PathBuf,
    event_receiver: Option<Receiver<HotReloadEvent>>,
    event_sender: Sender<HotReloadEvent>,
    status: HotReloadStatus,
}

impl HotReloader {
    /// 创建新的热重载器（不立即开始监控）
    pub fn new(watch_path: PathBuf) -> Self {
        let (event_sender, event_receiver) = mpsc::channel();
        let status = HotReloadStatus::new(watch_path.clone());
        
        Self {
            watcher: None,
            watch_path,
            event_receiver: Some(event_receiver),
            event_sender,
            status,
        }
    }

    /// 启动文件系统监控
    /// 
    /// 创建 `notify::RecommendedWatcher`，设置 debounce 间隔为 2 秒
    /// 监控 watch_path 目录（RecursiveMode::Recursive）
    /// 在回调中过滤 .wasm 文件的 Create/Modify/Remove 事件
    /// 通过 channel 发送 HotReloadEvent
    pub fn start_watching(&mut self) -> Result<()> {
        if self.watcher.is_some() {
            return Ok(()); // 已经在监控中
        }

        // 确保监控目录存在
        if !self.watch_path.exists() {
            std::fs::create_dir_all(&self.watch_path)?;
        }

        let sender = self.event_sender.clone();
        let watch_path = self.watch_path.clone();

        // 创建 watcher，设置 2 秒 debounce
        let config = NotifyConfig::default()
            .with_poll_interval(Duration::from_secs(2));

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        Self::handle_notify_event(event, &sender, &watch_path);
                    }
                    Err(e) => {
                        let _ = sender.send(HotReloadEvent::WatchError(e.to_string()));
                    }
                }
            },
            config,
        )?;

        // 开始监控目录（递归模式）
        watcher.watch(&self.watch_path, RecursiveMode::Recursive)?;

        self.watcher = Some(watcher);
        self.status.watching = true;
        
        tracing::info!("Started watching extensions directory: {:?}", self.watch_path);
        
        Ok(())
    }

    /// 处理 notify 事件，过滤 .wasm 文件变更
    fn handle_notify_event(event: Event, sender: &Sender<HotReloadEvent>, _watch_path: &Path) {
        // 只关注 .wasm 文件的创建、修改、删除事件
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                for path in &event.paths {
                    if is_wasm_file(path) {
                        tracing::debug!("WASM file changed: {:?}", path);
                        let _ = sender.send(HotReloadEvent::ExtensionChanged(path.clone()));
                    }
                }
            }
            EventKind::Remove(_) => {
                for path in &event.paths {
                    if is_wasm_file(path) {
                        tracing::debug!("WASM file removed: {:?}", path);
                        let _ = sender.send(HotReloadEvent::ExtensionRemoved(path.clone()));
                    }
                }
            }
            _ => {
                // 忽略其他事件类型
            }
        }
    }

    /// 停止监控（drop watcher）
    pub fn stop_watching(&mut self) {
        if let Some(watcher) = self.watcher.take() {
            drop(watcher);
            self.status.watching = false;
            tracing::info!("Stopped watching extensions directory");
        }
    }

    /// 非阻塞获取待处理事件
    pub fn poll_events(&self) -> Vec<HotReloadEvent> {
        let mut events = Vec::new();
        
        if let Some(ref receiver) = self.event_receiver {
            // 非阻塞地接收所有可用事件
            loop {
                match receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        tracing::warn!("Hot reload event channel disconnected");
                        break;
                    }
                }
            }
        }
        
        events
    }

    /// 获取当前状态
    pub fn status(&self) -> &HotReloadStatus {
        &self.status
    }

    /// 是否正在监控
    pub fn is_watching(&self) -> bool {
        self.status.watching
    }

    /// 获取监控路径
    pub fn watch_path(&self) -> &PathBuf {
        &self.watch_path
    }

    /// 更新状态：记录一次成功的重载
    pub fn record_reload_success(&mut self, extension_id: &str) {
        self.status.last_reload = Some(std::time::Instant::now());
        self.status.reload_count += 1;
        self.status.last_error = None;
        tracing::info!("Hot reload success for extension: {}", extension_id);
    }

    /// 更新状态：记录一次失败的重载
    pub fn record_reload_failure(&mut self, extension_id: &str, error: &str) {
        self.status.last_error = Some(format!("{}: {}", extension_id, error));
        tracing::error!("Hot reload failed for extension {}: {}", extension_id, error);
    }
}

impl Drop for HotReloader {
    fn drop(&mut self) {
        self.stop_watching();
    }
}

/// 检查是否为 .wasm 文件
#[allow(dead_code)]
fn is_wasm_file(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.eq_ignore_ascii_case("wasm"))
        .unwrap_or(false)
}

/// 从路径提取扩展 ID
/// 
/// 从 WASM 文件路径提取扩展标识符，格式为：{name}@{version}
/// 优先从 manifest.json 中读取，如果 manifest 不存在则从目录名推断
#[allow(dead_code)]
pub fn extract_extension_id(path: &Path) -> Option<String> {
    // 首先尝试从同目录的 manifest.json 中提取
    let parent = path.parent()?;
    let manifest_path = parent.join("manifest.json");
    
    if manifest_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                if let (Some(name), Some(version)) = (
                    manifest.get("name").and_then(|v| v.as_str()),
                    manifest.get("version").and_then(|v| v.as_str())
                ) {
                    return Some(format!("{}@{}", name, version));
                }
            }
        }
    }
    
    // 回退：从目录名和文件名推断
    // 例如：/path/to/my-extension-1.0.0/my-extension.wasm -> my-extension@1.0.0
    let dir_name = parent.file_name()?.to_str()?;
    
    // 尝试解析目录名中的版本信息（例如 "my-extension-1.0.0"）
    if let Some(_at_pos) = dir_name.rfind('@') {
        // 目录名已经包含版本信息
        return Some(dir_name.to_string());
    }
    
    // 尝试从文件名推断（例如 "my-extension.wasm"）
    let file_stem = path.file_stem()?.to_str()?;
    Some(format!("{}@unknown", file_stem))
}

/// 从路径提取扩展名称（用于查找已加载的扩展）
/// 
/// 返回扩展名称（不含版本），用于匹配已加载的扩展
pub fn extract_extension_name(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    let manifest_path = parent.join("manifest.json");
    
    if manifest_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(name) = manifest.get("name").and_then(|v| v.as_str()) {
                    return Some(name.to_string());
                }
            }
        }
    }
    
    // 回退：从目录名推断
    let dir_name = parent.file_name()?.to_str()?;
    
    // 移除版本后缀（如果有）
    if let Some(dash_pos) = dir_name.rfind('-') {
        let suffix = &dir_name[dash_pos + 1..];
        // 检查后缀是否像版本号（包含数字和点）
        if suffix.chars().any(|c| c.is_ascii_digit()) && suffix.contains('.') {
            return Some(dir_name[..dash_pos].to_string());
        }
    }
    
    Some(dir_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_hot_reloader_new() {
        let watch_path = PathBuf::from("/tmp/test-extensions");
        let reloader = HotReloader::new(watch_path.clone());
        
        assert!(!reloader.is_watching());
        assert_eq!(reloader.watch_path(), &watch_path);
        assert_eq!(reloader.status().reload_count, 0);
        assert!(reloader.status().last_reload.is_none());
    }

    #[test]
    fn test_hot_reloader_not_watching_initially() {
        let reloader = HotReloader::new(PathBuf::from("/tmp/test"));
        assert!(!reloader.is_watching());
        assert!(!reloader.status().watching);
    }

    #[test]
    fn test_hot_reload_status_initial() {
        let watch_path = PathBuf::from("/tmp/test");
        let status = HotReloadStatus::new(watch_path.clone());
        
        assert!(!status.watching);
        assert_eq!(status.watch_path, watch_path);
        assert!(status.last_reload.is_none());
        assert_eq!(status.reload_count, 0);
        assert!(status.last_error.is_none());
    }

    #[test]
    fn test_is_wasm_file() {
        assert!(is_wasm_file(Path::new("/path/to/extension.wasm")));
        assert!(is_wasm_file(Path::new("extension.WASM")));
        assert!(is_wasm_file(Path::new("extension.Wasm")));
        
        assert!(!is_wasm_file(Path::new("/path/to/extension.js")));
        assert!(!is_wasm_file(Path::new("/path/to/extension")));
        assert!(!is_wasm_file(Path::new("/path/to/manifest.json")));
        assert!(!is_wasm_file(Path::new("")));
    }

    #[test]
    fn test_extract_extension_id_from_manifest() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ext_dir = temp_dir.path().join("test-ext");
        std::fs::create_dir(&ext_dir).unwrap();
        
        // 创建 manifest.json
        let manifest = r#"{
            "name": "my-extension",
            "version": "1.2.3",
            "description": "Test extension",
            "wasm_entry": "test.wasm"
        }"#;
        let mut file = std::fs::File::create(ext_dir.join("manifest.json")).unwrap();
        file.write_all(manifest.as_bytes()).unwrap();
        
        // 创建 .wasm 文件
        let wasm_path = ext_dir.join("test.wasm");
        std::fs::write(&wasm_path, b"dummy").unwrap();
        
        let id = extract_extension_id(&wasm_path).unwrap();
        assert_eq!(id, "my-extension@1.2.3");
    }

    #[test]
    fn test_extract_extension_id_fallback() {
        let temp_dir = tempfile::tempdir().unwrap();
        let wasm_path = temp_dir.path().join("my-ext.wasm");
        std::fs::write(&wasm_path, b"dummy").unwrap();
        
        // 没有 manifest.json，使用文件名回退
        let id = extract_extension_id(&wasm_path).unwrap();
        assert!(id.starts_with("my-ext@"));
    }

    #[test]
    fn test_extract_extension_name_from_manifest() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ext_dir = temp_dir.path().join("test-ext");
        std::fs::create_dir(&ext_dir).unwrap();
        
        // 创建 manifest.json
        let manifest = r#"{
            "name": "my-extension",
            "version": "1.0.0"
        }"#;
        std::fs::write(ext_dir.join("manifest.json"), manifest).unwrap();
        
        let wasm_path = ext_dir.join("test.wasm");
        std::fs::write(&wasm_path, b"dummy").unwrap();
        
        let name = extract_extension_name(&wasm_path).unwrap();
        assert_eq!(name, "my-extension");
    }

    #[test]
    fn test_extract_extension_name_fallback() {
        // 目录名包含版本号
        let path = Path::new("/extensions/my-extension-1.0.0/plugin.wasm");
        let name = extract_extension_name(path).unwrap();
        assert_eq!(name, "my-extension");
        
        // 目录名不包含版本号
        let path2 = Path::new("/extensions/my-extension/plugin.wasm");
        let name2 = extract_extension_name(path2).unwrap();
        assert_eq!(name2, "my-extension");
    }

    #[test]
    fn test_poll_events_empty() {
        let reloader = HotReloader::new(PathBuf::from("/tmp/test"));
        let events = reloader.poll_events();
        assert!(events.is_empty());
    }

    #[test]
    fn test_hot_reloader_start_stop_watching() {
        let temp_dir = tempfile::tempdir().unwrap();
        let watch_path = temp_dir.path().join("extensions");
        
        let mut reloader = HotReloader::new(watch_path.clone());
        
        // 初始状态：未监控
        assert!(!reloader.is_watching());
        
        // 启动监控
        let result = reloader.start_watching();
        assert!(result.is_ok());
        assert!(reloader.is_watching());
        
        // 再次启动应该成功（幂等）
        let result = reloader.start_watching();
        assert!(result.is_ok());
        assert!(reloader.is_watching());
        
        // 停止监控
        reloader.stop_watching();
        assert!(!reloader.is_watching());
        
        // 再次停止应该无问题（幂等）
        reloader.stop_watching();
        assert!(!reloader.is_watching());
    }

    #[test]
    fn test_record_reload_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut reloader = HotReloader::new(temp_dir.path().to_path_buf());
        
        reloader.record_reload_success("test-ext@1.0.0");
        
        assert_eq!(reloader.status().reload_count, 1);
        assert!(reloader.status().last_reload.is_some());
        assert!(reloader.status().last_error.is_none());
    }

    #[test]
    fn test_record_reload_failure() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut reloader = HotReloader::new(temp_dir.path().to_path_buf());
        
        reloader.record_reload_failure("test-ext@1.0.0", "compilation failed");
        
        assert_eq!(reloader.status().reload_count, 0);
        assert!(reloader.status().last_error.is_some());
        let error = reloader.status().last_error.as_ref().unwrap();
        assert!(error.contains("test-ext@1.0.0"));
        assert!(error.contains("compilation failed"));
    }

    #[test]
    fn test_hot_reload_event_variants() {
        let changed = HotReloadEvent::ExtensionChanged(PathBuf::from("/test.wasm"));
        assert!(matches!(changed, HotReloadEvent::ExtensionChanged(_)));
        
        let removed = HotReloadEvent::ExtensionRemoved(PathBuf::from("/test.wasm"));
        assert!(matches!(removed, HotReloadEvent::ExtensionRemoved(_)));
        
        let success = HotReloadEvent::ReloadSuccess("test@1.0.0".to_string());
        assert!(matches!(success, HotReloadEvent::ReloadSuccess(_)));
        
        let failed = HotReloadEvent::ReloadFailed("test@1.0.0".to_string(), "error".to_string());
        assert!(matches!(failed, HotReloadEvent::ReloadFailed(_, _)));
        
        let error = HotReloadEvent::WatchError("watch failed".to_string());
        assert!(matches!(error, HotReloadEvent::WatchError(_)));
    }
}
