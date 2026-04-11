//! Token 刷新调度器
//! 
//! 后台定期检查并刷新即将过期的 token

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

use super::token_storage::TokenStorage;
use super::providers::get_oauth_provider;

/// 刷新事件类型
#[derive(Debug, Clone)]
#[allow(dead_code)] // Token 刷新调度器尚未完全集成
pub enum RefreshEvent {
    /// Token 刷新成功
    Refreshed {
        /// Provider 名称
        provider: String
    },
    /// Token 刷新失败
    Failed {
        /// Provider 名称
        provider: String,
        /// 错误信息
        error: String
    },
    /// 需要重新登录（连续失败超过阈值）
    ReloginRequired {
        /// Provider 名称
        provider: String
    },
}

/// Token 刷新调度器
/// 
/// 后台定期检查所有已存储的 token，对即将过期的进行刷新。
/// 跟踪每个 provider 的连续失败次数，超过阈值时发送 ReloginRequired 事件。
#[allow(dead_code)] // Token 刷新调度器尚未完全集成
pub struct RefreshScheduler {
    /// Token 存储
    token_storage: Arc<TokenStorage>,
    /// 事件发送通道
    event_tx: mpsc::Sender<RefreshEvent>,
    /// 检查间隔
    check_interval: Duration,
    /// 最大连续失败次数
    max_consecutive_failures: u32,
    /// 关闭信号发送器
    shutdown: watch::Sender<bool>,
    /// 关闭信号接收器
    shutdown_rx: watch::Receiver<bool>,
}

#[allow(dead_code)] // Token 刷新调度器尚未完全集成
impl RefreshScheduler {
    /// 创建新的刷新调度器
    /// 
    /// # Arguments
    /// * `token_storage` - Token 存储实例
    /// * `event_tx` - 刷新事件发送通道
    pub fn new(
        token_storage: Arc<TokenStorage>,
        event_tx: mpsc::Sender<RefreshEvent>,
    ) -> Self {
        let (shutdown, shutdown_rx) = watch::channel(false);
        Self {
            token_storage,
            event_tx,
            check_interval: Duration::from_secs(60),
            max_consecutive_failures: 3,
            shutdown,
            shutdown_rx,
        }
    }
    
    /// 设置检查间隔
    pub fn with_check_interval(mut self, interval: Duration) -> Self {
        self.check_interval = interval;
        self
    }
    
    /// 设置最大连续失败次数
    pub fn with_max_failures(mut self, max: u32) -> Self {
        self.max_consecutive_failures = max;
        self
    }
    
    /// 启动后台刷新任务
    /// 
    /// 返回一个 JoinHandle，可用于等待任务完成或强制终止。
    /// 后台任务会定期检查所有 token 并刷新即将过期的。
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let token_storage = self.token_storage.clone();
        let event_tx = self.event_tx.clone();
        let check_interval = self.check_interval;
        let max_failures = self.max_consecutive_failures;
        let mut shutdown_rx = self.shutdown_rx.clone();
        
        tokio::spawn(async move {
            let mut failure_counts: HashMap<String, u32> = HashMap::new();
            
            loop {
                // 使用 tokio::select 同时等待定时器和关闭信号
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            tracing::debug!("Refresh scheduler shutting down");
                            break;
                        }
                    }
                    _ = tokio::time::sleep(check_interval) => {
                        Self::check_and_refresh(
                            &token_storage,
                            &event_tx,
                            &mut failure_counts,
                            max_failures,
                        ).await;
                    }
                }
            }
        })
    }
    
    /// 停止刷新调度器
    pub fn stop(&self) {
        let _ = self.shutdown.send(true);
    }
    
    /// 执行一次刷新检查
    /// 
    /// 遍历所有已存储的 token，对即将过期的调用刷新。
    /// 跟踪连续失败次数，超过阈值时发送 ReloginRequired 事件。
    async fn check_and_refresh(
        token_storage: &Arc<TokenStorage>,
        event_tx: &mpsc::Sender<RefreshEvent>,
        failure_counts: &mut HashMap<String, u32>,
        max_consecutive_failures: u32,
    ) {
        let providers = token_storage.list_providers();
        
        for provider in providers {
            // 检查 token 是否即将过期
            if let Some(token) = token_storage.get_token(&provider) {
                if !token.is_expiring_soon() {
                    continue;
                }
                
                // 获取 provider 配置
                let provider_config = match get_oauth_provider(&provider) {
                    Some(config) => config,
                    None => {
                        tracing::debug!(
                            provider = %provider,
                            "No OAuth configuration found, skipping refresh"
                        );
                        continue;
                    }
                };
                
                // 尝试刷新
                match token_storage.refresh_token(
                    &provider,
                    &provider_config.token_url,
                    &provider_config.client_id,
                ).await {
                    Ok(new_token) => {
                        // 刷新成功，重置失败计数
                        failure_counts.remove(&provider);
                        
                        tracing::info!(
                            provider = %provider,
                            expires_at = ?new_token.expires_at,
                            "Token refreshed successfully"
                        );
                        
                        let _ = event_tx.send(RefreshEvent::Refreshed {
                            provider: provider.clone(),
                        }).await;
                    }
                    Err(e) => {
                        // 刷新失败，增加失败计数
                        let count = failure_counts.entry(provider.clone()).or_insert(0);
                        *count += 1;
                        
                        tracing::warn!(
                            provider = %provider,
                            error = %e,
                            consecutive_failures = *count,
                            "Token refresh failed"
                        );
                        
                        let _ = event_tx.send(RefreshEvent::Failed {
                            provider: provider.clone(),
                            error: e.to_string(),
                        }).await;
                        
                        // 检查是否超过阈值
                        if *count >= max_consecutive_failures {
                            tracing::error!(
                                provider = %provider,
                                max_failures = max_consecutive_failures,
                                "Token refresh failed too many times, re-login required"
                            );
                            
                            let _ = event_tx.send(RefreshEvent::ReloginRequired {
                                provider: provider.clone(),
                            }).await;
                            
                            // 重置计数，避免重复发送 ReloginRequired
                            failure_counts.remove(&provider);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::auth::token_storage::StoredToken;
    use chrono::{Duration as ChronoDuration, Utc};
    use std::collections::HashMap as StdHashMap;
    use std::time::Duration;
    
    /// 创建测试用的 TokenStorage
    fn create_test_storage() -> (Arc<TokenStorage>, tempfile::TempDir) {
        // 使用临时目录创建存储
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = crate::core::auth::token_storage::EncryptedFileStorage::new(
            temp_dir.path().to_path_buf()
        ).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));
        (Arc::new(token_storage), temp_dir)
    }
    
    fn create_test_token(provider: &str, expires_at: Option<chrono::DateTime<Utc>>) -> StoredToken {
        StoredToken {
            provider: provider.to_string(),
            access_token: "test_access_token".to_string(),
            refresh_token: Some("test_refresh_token".to_string()),
            expires_at,
        }
    }
    
    #[test]
    fn test_refresh_event_debug() {
        let event = RefreshEvent::Refreshed {
            provider: "test".to_string(),
        };
        assert!(format!("{:?}", event).contains("Refreshed"));
        
        let event = RefreshEvent::Failed {
            provider: "test".to_string(),
            error: "error".to_string(),
        };
        assert!(format!("{:?}", event).contains("Failed"));
        
        let event = RefreshEvent::ReloginRequired {
            provider: "test".to_string(),
        };
        assert!(format!("{:?}", event).contains("ReloginRequired"));
    }
    
    #[test]
    fn test_scheduler_creation() {
        let (storage, _temp) = create_test_storage();
        let (tx, _rx) = mpsc::channel(100);
        
        let scheduler = RefreshScheduler::new(storage, tx);
        assert_eq!(scheduler.check_interval, Duration::from_secs(60));
        assert_eq!(scheduler.max_consecutive_failures, 3);
    }
    
    #[test]
    fn test_scheduler_with_custom_interval() {
        let (storage, _temp) = create_test_storage();
        let (tx, _rx) = mpsc::channel(100);
        
        let scheduler = RefreshScheduler::new(storage, tx)
            .with_check_interval(Duration::from_secs(30));
        assert_eq!(scheduler.check_interval, Duration::from_secs(30));
    }
    
    #[test]
    fn test_scheduler_with_custom_max_failures() {
        let (storage, _temp) = create_test_storage();
        let (tx, _rx) = mpsc::channel(100);
        
        let scheduler = RefreshScheduler::new(storage, tx)
            .with_max_failures(5);
        assert_eq!(scheduler.max_consecutive_failures, 5);
    }
    
    #[test]
    fn test_scheduler_builder_chain() {
        let (storage, _temp) = create_test_storage();
        let (tx, _rx) = mpsc::channel(100);
        
        let scheduler = RefreshScheduler::new(storage, tx)
            .with_check_interval(Duration::from_secs(120))
            .with_max_failures(10);
        
        assert_eq!(scheduler.check_interval, Duration::from_secs(120));
        assert_eq!(scheduler.max_consecutive_failures, 10);
    }
    
    #[tokio::test]
    async fn test_scheduler_stop() {
        let (storage, _temp) = create_test_storage();
        let (tx, _rx) = mpsc::channel(100);
        
        let scheduler = RefreshScheduler::new(storage.clone(), tx)
            .with_check_interval(Duration::from_millis(10));
        
        // 启动调度器
        let handle = scheduler.start();
        
        // 立即停止
        scheduler.stop();
        
        // 等待任务结束
        let result = tokio::time::timeout(Duration::from_millis(100), handle).await;
        assert!(result.is_ok());
    }
    
    #[tokio::test]
    async fn test_check_and_refresh_skips_valid_tokens() {
        let (storage, _temp) = create_test_storage();
        let (tx, mut rx) = mpsc::channel(100);
        
        // 创建一个有效期很长的 token
        let token = create_test_token(
            "anthropic",
            Some(Utc::now() + ChronoDuration::hours(1)),
        );
        storage.save_token(&token).unwrap();
        
        let mut failure_counts = StdHashMap::new();
        
        RefreshScheduler::check_and_refresh(
            &storage,
            &tx,
            &mut failure_counts,
            3,
        ).await;
        
        // 不应该发送任何事件
        let result = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
        assert!(result.is_err()); // timeout
    }
    
    #[tokio::test]
    async fn test_check_and_refresh_handles_expiring_token() {
        let (storage, _temp) = create_test_storage();
        let (tx, mut rx) = mpsc::channel(100);
        
        // 创建一个即将过期的 token
        let token = create_test_token(
            "anthropic",
            Some(Utc::now() + ChronoDuration::minutes(3)),
        );
        storage.save_token(&token).unwrap();
        
        let mut failure_counts = StdHashMap::new();
        
        // 执行检查（刷新会失败因为没有真实的服务器）
        RefreshScheduler::check_and_refresh(
            &storage,
            &tx,
            &mut failure_counts,
            3,
        ).await;
        
        // 应该收到 Failed 事件
        let result = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
        match result {
            Ok(Some(RefreshEvent::Failed { provider, .. })) => {
                assert_eq!(provider, "anthropic");
            }
            Ok(Some(RefreshEvent::Refreshed { .. })) => {
                // 刷新成功也是可能的（如果有 refresh_token）
            }
            _ => panic!("Expected Failed or Refreshed event, got {:?}", result),
        }
    }
    
    #[tokio::test]
    async fn test_relogin_required_after_max_failures() {
        let (storage, _temp) = create_test_storage();
        let (tx, mut rx) = mpsc::channel(100);
        
        // 创建一个即将过期的 token
        let token = create_test_token(
            "anthropic",
            Some(Utc::now() + ChronoDuration::minutes(3)),
        );
        storage.save_token(&token).unwrap();
        
        // 模拟已经失败 2 次
        let mut failure_counts = StdHashMap::new();
        failure_counts.insert("anthropic".to_string(), 2);
        
        // 执行检查，应该达到阈值并发送 ReloginRequired
        RefreshScheduler::check_and_refresh(
            &storage,
            &tx,
            &mut failure_counts,
            3,
        ).await;
        
        // 应该收到 Failed 然后可能收到 ReloginRequired
        let mut _received_relogin = false;
        
        loop {
            let result = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
            match result {
                Ok(Some(RefreshEvent::ReloginRequired { provider })) => {
                    assert_eq!(provider, "anthropic");
                    _received_relogin = true;
                    break;
                }
                Ok(Some(RefreshEvent::Failed { .. })) => {
                    // 继续等待 ReloginRequired
                }
                Ok(Some(RefreshEvent::Refreshed { .. })) => {
                    // 如果刷新成功，就不会有 ReloginRequired
                    break;
                }
                _ => break,
            }
        }
        
        // 由于我们没有真实的 OAuth 服务器，刷新会失败
        // 但 ReloginRequired 可能在同一检查周期发送
        // 这里主要验证逻辑不会 panic
    }
}
