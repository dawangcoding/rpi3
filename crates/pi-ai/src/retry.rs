//! 重试策略模块
//!
//! 提供指数退避重试策略和流中断恢复功能

use std::time::Duration;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// 重试配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryConfig {
    /// 最大重试次数（默认 3）
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// 初始延迟（毫秒，默认 1000）
    #[serde(default = "default_initial_delay_ms")]
    pub initial_delay_ms: u64,
    /// 最大延迟（毫秒，默认 30000）
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,
    /// 退避因子（默认 2.0）
    #[serde(default = "default_backoff_factor")]
    pub backoff_factor: f64,
    /// 是否启用抖动（默认 true）
    #[serde(default = "default_jitter")]
    pub jitter: bool,
}

fn default_max_retries() -> u32 {
    3
}

fn default_initial_delay_ms() -> u64 {
    1000
}

fn default_max_delay_ms() -> u64 {
    30000
}

fn default_backoff_factor() -> f64 {
    2.0
}

fn default_jitter() -> bool {
    true
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            initial_delay_ms: default_initial_delay_ms(),
            max_delay_ms: default_max_delay_ms(),
            backoff_factor: default_backoff_factor(),
            jitter: default_jitter(),
        }
    }
}

impl RetryConfig {
    /// 创建一个新的重试配置
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            ..Default::default()
        }
    }

    /// 禁用重试
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }
}

/// 重试策略
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    config: RetryConfig,
}

impl RetryPolicy {
    /// 使用指定配置创建重试策略
    pub fn new(config: RetryConfig) -> Self {
        Self { config }
    }

    /// 使用默认配置创建重试策略
    pub fn with_default_config() -> Self {
        Self {
            config: RetryConfig::default(),
        }
    }
}

impl RetryPolicy {
    /// 获取配置
    pub fn config(&self) -> &RetryConfig {
        &self.config
    }

    /// 计算第 n 次重试的延迟时间
    ///
    /// 公式: min(initial_delay * backoff_factor^attempt + jitter, max_delay)
    /// jitter 范围: 0 到当前延迟的 25%
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::from_millis(0);
        }

        let attempt = attempt.saturating_sub(1) as f64;
        let base_delay = self.config.initial_delay_ms as f64
            * self.config.backoff_factor.powf(attempt);
        
        let delay_ms = if self.config.jitter {
            let jitter_range = base_delay * 0.25;
            let jitter = rand::thread_rng().gen_range(0.0..jitter_range);
            base_delay + jitter
        } else {
            base_delay
        };

        let capped_delay = delay_ms.min(self.config.max_delay_ms as f64) as u64;
        Duration::from_millis(capped_delay)
    }

    /// 判断错误是否可重试
    pub fn is_retryable(error: &anyhow::Error) -> bool {
        let error_string = error.to_string().to_lowercase();
        
        // HTTP 429 Too Many Requests
        if error_string.contains("429") || error_string.contains("too many requests") {
            return true;
        }
        
        // HTTP 500 Internal Server Error
        if error_string.contains("500") || error_string.contains("internal server error") {
            return true;
        }
        
        // HTTP 502 Bad Gateway
        if error_string.contains("502") || error_string.contains("bad gateway") {
            return true;
        }
        
        // HTTP 503 Service Unavailable
        if error_string.contains("503") || error_string.contains("service unavailable") {
            return true;
        }
        
        // HTTP 408 Request Timeout
        if error_string.contains("408") || error_string.contains("request timeout") {
            return true;
        }
        
        // 网络错误
        if error_string.contains("connection") 
            || error_string.contains("timeout")
            || error_string.contains("network")
            || error_string.contains("dns")
            || error_string.contains("reset")
            || error_string.contains("broken pipe") {
            return true;
        }
        
        // 服务器过载
        if error_string.contains("overloaded") 
            || error_string.contains("server is busy")
            || error_string.contains("temporarily unavailable") {
            return true;
        }
        
        false
    }

    /// 执行异步重试操作
    ///
    /// 返回 Ok(T) 如果操作成功，Err(last_error) 如果所有重试都失败
    pub async fn execute<F, Fut, T>(
        &self,
        operation_name: &str,
        operation: F,
    ) -> anyhow::Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<T>>,
    {
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        debug!("{} succeeded after {} retries", operation_name, attempt);
                    }
                    return Ok(result);
                }
                Err(e) => {
                    let should_retry = Self::is_retryable(&e);
                    last_error = Some(e);
                    
                    if should_retry && attempt < self.config.max_retries {
                        let delay = self.delay_for_attempt(attempt + 1);
                        warn!(
                            "{} failed (attempt {}/{}), retrying in {:?}...",
                            operation_name,
                            attempt + 1,
                            self.config.max_retries + 1,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                    } else if !should_retry {
                        debug!("{} failed with non-retryable error", operation_name);
                        break;
                    } else {
                        warn!(
                            "{} failed after {} attempts, giving up",
                            operation_name,
                            attempt + 1
                        );
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("{} failed after retries", operation_name)))
    }

    /// 获取最大重试次数
    pub fn max_retries(&self) -> u32 {
        self.config.max_retries
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(RetryConfig::default())
    }
}

/// 流恢复状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamRecoveryState {
    /// 流正常进行
    Active,
    /// 流已中断，正在恢复
    Recovering,
    /// 恢复成功
    Recovered,
    /// 恢复失败，放弃
    Failed,
}

/// 流恢复统计
#[derive(Debug, Clone, Default)]
pub struct StreamRecoveryStats {
    /// 恢复尝试次数
    pub recovery_attempts: u32,
    /// 成功恢复次数
    pub successful_recoveries: u32,
    /// 失败次数
    pub failed_recoveries: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_backoff_delays() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_factor: 2.0,
            jitter: false,
        };
        let policy = RetryPolicy::new(config);

        // attempt 0 should return 0
        assert_eq!(policy.delay_for_attempt(0).as_millis(), 0);
        
        // attempt 1: 1000 * 2^0 = 1000
        assert_eq!(policy.delay_for_attempt(1).as_millis(), 1000);
        
        // attempt 2: 1000 * 2^1 = 2000
        assert_eq!(policy.delay_for_attempt(2).as_millis(), 2000);
        
        // attempt 3: 1000 * 2^2 = 4000
        assert_eq!(policy.delay_for_attempt(3).as_millis(), 4000);
        
        // attempt 4: 1000 * 2^3 = 8000
        assert_eq!(policy.delay_for_attempt(4).as_millis(), 8000);
        
        // attempt 5: 1000 * 2^4 = 16000
        assert_eq!(policy.delay_for_attempt(5).as_millis(), 16000);
    }

    #[test]
    fn test_max_delay_cap() {
        let config = RetryConfig {
            max_retries: 10,
            initial_delay_ms: 1000,
            max_delay_ms: 5000,
            backoff_factor: 2.0,
            jitter: false,
        };
        let policy = RetryPolicy::new(config);

        // 延迟应该被限制在 max_delay_ms
        assert_eq!(policy.delay_for_attempt(1).as_millis(), 1000);
        assert_eq!(policy.delay_for_attempt(2).as_millis(), 2000);
        assert_eq!(policy.delay_for_attempt(3).as_millis(), 4000);
        assert_eq!(policy.delay_for_attempt(4).as_millis(), 5000); // capped
        assert_eq!(policy.delay_for_attempt(5).as_millis(), 5000); // capped
        assert_eq!(policy.delay_for_attempt(10).as_millis(), 5000); // capped
    }

    #[test]
    fn test_jitter() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_factor: 2.0,
            jitter: true,
        };
        let policy = RetryPolicy::new(config);

        // 测试多次，确保抖动在合理范围内
        for attempt in 1..=5 {
            let delay = policy.delay_for_attempt(attempt).as_millis() as f64;
            let base_delay = 1000.0 * 2.0_f64.powf((attempt - 1) as f64);
            let max_with_jitter = base_delay * 1.25; // base + 25% jitter
            
            assert!(
                delay >= base_delay && delay <= max_with_jitter.min(30000.0),
                "Delay {} for attempt {} should be between {} and {}",
                delay, attempt, base_delay, max_with_jitter
            );
        }
    }

    #[test]
    fn test_is_retryable() {
        // 可重试的错误
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("429 Too Many Requests")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("500 Internal Server Error")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("502 Bad Gateway")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("503 Service Unavailable")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("408 Request Timeout")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("connection refused")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("timeout occurred")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("network error")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("dns resolution failed")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("connection reset")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("broken pipe")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("server overloaded")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("temporarily unavailable")));

        // 不可重试的错误
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("400 Bad Request")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("401 Unauthorized")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("403 Forbidden")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("404 Not Found")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("Invalid API key")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("invalid request")));
    }

    #[test]
    fn test_default_config() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay_ms, 1000);
        assert_eq!(config.max_delay_ms, 30000);
        assert_eq!(config.backoff_factor, 2.0);
        assert!(config.jitter);
    }

    #[test]
    fn test_retry_config_new() {
        let config = RetryConfig::new(5);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.initial_delay_ms, 1000);
        assert_eq!(config.max_delay_ms, 30000);
    }

    #[test]
    fn test_no_retry() {
        let config = RetryConfig::no_retry();
        assert_eq!(config.max_retries, 0);
    }

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries(), 3);
        assert_eq!(policy.config().initial_delay_ms, 1000);
    }

    #[tokio::test]
    async fn test_execute_success() {
        let policy = RetryPolicy::default();
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        
        let result = policy.execute("test_op", || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok::<_, anyhow::Error>(42)
            }
        }).await;
        
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_execute_retry_then_success() {
        let config = RetryConfig {
            max_retries: 3,
            initial_delay_ms: 10, // 短延迟以便测试
            max_delay_ms: 100,
            backoff_factor: 2.0,
            jitter: false,
        };
        let policy = RetryPolicy::new(config);
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        
        let result = policy.execute("test_op", || {
            let counter = counter.clone();
            async move {
                let count = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if count < 2 {
                    Err(anyhow::anyhow!("429 Too Many Requests"))
                } else {
                    Ok::<_, anyhow::Error>(42)
                }
            }
        }).await;
        
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_execute_non_retryable_error() {
        let policy = RetryPolicy::default();
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        
        let result = policy.execute("test_op", || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err::<(), _>(anyhow::anyhow!("400 Bad Request"))
            }
        }).await;
        
        assert!(result.is_err());
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1); // 不重试
    }

    // ============== 新增测试 ==============

    #[test]
    fn test_retry_config_serialization() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 2000,
            max_delay_ms: 60000,
            backoff_factor: 3.0,
            jitter: false,
        };
        
        // 测试序列化
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("max_retries"));
        assert!(json.contains("initial_delay_ms"));
        
        // 测试反序列化
        let deserialized: RetryConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_retries, 5);
        assert_eq!(deserialized.initial_delay_ms, 2000);
    }

    #[test]
    fn test_retry_config_partial_deserialization() {
        // 测试部分字段反序列化（使用默认值）
        let json = r#"{"max_retries": 10}"#;
        let config: RetryConfig = serde_json::from_str(json).unwrap();
        
        assert_eq!(config.max_retries, 10);
        assert_eq!(config.initial_delay_ms, 1000); // 默认值
        assert_eq!(config.max_delay_ms, 30000); // 默认值
        assert_eq!(config.backoff_factor, 2.0); // 默认值
        assert!(config.jitter); // 默认值
    }

    #[test]
    fn test_retry_policy_max_retries_reached() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 10,
            max_delay_ms: 100,
            backoff_factor: 2.0,
            jitter: false,
        };
        let policy = RetryPolicy::new(config);
        
        // 验证最大重试次数
        assert_eq!(policy.max_retries(), 2);
    }

    #[tokio::test]
    async fn test_execute_all_retries_exhausted() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay_ms: 10,
            max_delay_ms: 100,
            backoff_factor: 2.0,
            jitter: false,
        };
        let policy = RetryPolicy::new(config);
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        
        let result = policy.execute("test_op", || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err::<(), _>(anyhow::anyhow!("500 Internal Server Error"))
            }
        }).await;
        
        assert!(result.is_err());
        // 初始调用 + 2 次重试 = 3 次
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn test_is_retryable_4xx_errors() {
        // 4xx 错误大多不可重试
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("400 Bad Request")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("401 Unauthorized")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("403 Forbidden")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("404 Not Found")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("405 Method Not Allowed")));
        assert!(!RetryPolicy::is_retryable(&anyhow::anyhow!("415 Unsupported Media Type")));
        
        // 429 和 408 是例外，可以重试
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("429 Too Many Requests")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("408 Request Timeout")));
    }

    #[test]
    fn test_is_retryable_5xx_errors() {
        // 5xx 错误可重试
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("500 Internal Server Error")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("502 Bad Gateway")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("503 Service Unavailable")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("504 Gateway Timeout")));
    }

    #[test]
    fn test_is_retryable_network_errors() {
        // 网络错误可重试
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("connection refused")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("connection reset by peer")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("connection timed out")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("network is unreachable")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("dns resolution failed")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("broken pipe")));
    }

    #[test]
    fn test_is_retryable_server_overload() {
        // 服务器过载错误可重试
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("server overloaded")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("server is busy")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("temporarily unavailable")));
    }

    #[test]
    fn test_is_retryable_case_insensitive() {
        // 测试大小写不敏感
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("429 too many requests")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("500 INTERNAL SERVER ERROR")));
        assert!(RetryPolicy::is_retryable(&anyhow::anyhow!("CONNECTION REFUSED")));
    }

    #[test]
    fn test_delay_for_attempt_zero() {
        let policy = RetryPolicy::default();
        
        // attempt 0 应该返回 0 延迟
        assert_eq!(policy.delay_for_attempt(0), std::time::Duration::from_millis(0));
    }

    #[test]
    fn test_stream_recovery_state() {
        // 测试流恢复状态枚举
        assert_eq!(StreamRecoveryState::Active, StreamRecoveryState::Active);
        assert_ne!(StreamRecoveryState::Active, StreamRecoveryState::Recovering);
        assert_ne!(StreamRecoveryState::Recovered, StreamRecoveryState::Failed);
    }

    #[test]
    fn test_stream_recovery_stats_default() {
        let stats = StreamRecoveryStats::default();
        assert_eq!(stats.recovery_attempts, 0);
        assert_eq!(stats.successful_recoveries, 0);
        assert_eq!(stats.failed_recoveries, 0);
    }

    #[tokio::test]
    async fn test_execute_with_immediate_success() {
        let policy = RetryPolicy::default();
        
        // 立即成功的情况
        let result = policy.execute("test_op", || async {
            Ok::<_, anyhow::Error>("immediate success".to_string())
        }).await;
        
        assert_eq!(result.unwrap(), "immediate success");
    }

    #[test]
    fn test_backoff_factor_edge_cases() {
        // 测试不同的退避因子
        
        // 退避因子为 1.0（固定延迟）
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 10000,
            backoff_factor: 1.0,
            jitter: false,
        };
        let policy = RetryPolicy::new(config);
        
        // 每次延迟应该相同（1000ms）
        assert_eq!(policy.delay_for_attempt(1).as_millis(), 1000);
        assert_eq!(policy.delay_for_attempt(2).as_millis(), 1000);
        assert_eq!(policy.delay_for_attempt(3).as_millis(), 1000);
        
        // 退避因子为 3.0
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_factor: 3.0,
            jitter: false,
        };
        let policy = RetryPolicy::new(config);
        
        assert_eq!(policy.delay_for_attempt(1).as_millis(), 1000);
        assert_eq!(policy.delay_for_attempt(2).as_millis(), 3000);
        assert_eq!(policy.delay_for_attempt(3).as_millis(), 9000);
        assert_eq!(policy.delay_for_attempt(4).as_millis(), 27000);
        assert_eq!(policy.delay_for_attempt(5).as_millis(), 30000); // 被限制
    }
}
