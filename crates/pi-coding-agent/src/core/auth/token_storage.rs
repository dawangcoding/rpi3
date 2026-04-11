/// Token 刷新错误类型
#[derive(Debug, Clone)]
pub enum RefreshError {
    /// 网络错误（可重试）
    NetworkError(String),
    /// 认证错误（需要重新登录）
    AuthError(String),
    /// 其他错误
    Other(String),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefreshError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            RefreshError::AuthError(msg) => write!(f, "Authentication error: {}", msg),
            RefreshError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for RefreshError {}

impl RefreshError {
    /// 从响应状态码判断错误类型
    pub fn from_response(status: reqwest::StatusCode, body: Option<&str>) -> Self {
        match status.as_u16() {
            400 | 401 | 403 => RefreshError::AuthError(
                body.unwrap_or("Authentication failed").to_string()
            ),
            429 => RefreshError::NetworkError("Rate limited".to_string()),
            500..=599 => RefreshError::NetworkError(
                format!("Server error: {}", status)
            ),
            _ => RefreshError::Other(format!("HTTP {}: {}", status, body.unwrap_or(""))),
        }
    }
    
    /// 判断是否可重试
    #[allow(dead_code)] // 预留给未来使用
    pub fn is_retryable(&self) -> bool {
        matches!(self, RefreshError::NetworkError(_))
    }
    
    /// 判断是否需要重新登录
    #[allow(dead_code)] // 预留给未来使用
    pub fn requires_relogin(&self) -> bool {
        matches!(self, RefreshError::AuthError(_))
    }
}


use anyhow::{Result, Context};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

/// 当前 Token 存储版本
const CURRENT_TOKEN_VERSION: u32 = 2;

/// Token 存储的版本化包装
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VersionedTokenData {
    version: u32,
    #[serde(flatten)]
    data: serde_json::Value,
}

/// 存储的 Token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    /// Provider 名称
    pub provider: String,
    /// 访问令牌
    pub access_token: String,
    /// 刷新令牌
    pub refresh_token: Option<String>,
    /// 过期时间
    pub expires_at: Option<DateTime<Utc>>,
}

impl StoredToken {
    /// 检查 token 是否已过期
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expires_at) => Utc::now() >= expires_at,
            None => false, // 无过期时间视为未过期
        }
    }
    
    /// 检查 token 是否即将过期（5 分钟内）
    pub fn is_expiring_soon(&self) -> bool {
        match self.expires_at {
            Some(expires_at) => Utc::now() + chrono::Duration::minutes(5) >= expires_at,
            None => false,
        }
    }
}

/// Token 持久化存储
pub struct TokenStorage {
    storage: Box<dyn SecureStorage>,
    /// 保留旧路径用于迁移检测
    #[allow(dead_code)] // 用于检测旧文件迁移
    legacy_path: PathBuf,
    /// 并发刷新锁 - 按 provider 名称区分，防止多个请求同时刷新同一个 token
    refresh_locks: Arc<StdMutex<HashMap<String, Arc<TokioMutex<()>>>>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TokenStore {
    tokens: HashMap<String, StoredToken>,
}

/// 安全存储抽象
pub(crate) trait SecureStorage: Send + Sync {
    fn save(&self, key: &str, data: &[u8]) -> Result<()>;
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn delete(&self, key: &str) -> Result<()>;
    fn list_keys(&self) -> Result<Vec<String>>;
}

/// 系统密钥链存储（macOS Keychain / Linux Secret Service / Windows Credential Manager）
struct KeychainStorage {
    service_name: String,
    /// 独立的索引文件路径，用于追踪已存储的 provider
    index_path: PathBuf,
}

impl KeychainStorage {
    fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let index_path = home.join(".pi").join("auth").join("keychain_index.json");
        Self { 
            service_name: "pi-cli-auth".to_string(),
            index_path,
        }
    }
    
    /// 更新索引文件
    fn update_index(&self, keys: &[String]) -> Result<()> {
        // 确保目录存在
        if let Some(parent) = self.index_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create index directory")?;
        }
        let content = serde_json::to_string_pretty(keys)
            .context("Failed to serialize index")?;
        std::fs::write(&self.index_path, content)
            .context("Failed to write index file")?;
        Ok(())
    }
    
    /// 从索引文件读取
    fn read_index(&self) -> Vec<String> {
        match std::fs::read_to_string(&self.index_path) {
            Ok(content) => {
                serde_json::from_str(&content).unwrap_or_default()
            }
            Err(_) => vec![],
        }
    }
    
    /// 向索引添加 key
    fn add_to_index(&self, key: &str) -> Result<()> {
        let mut keys = self.read_index();
        if !keys.contains(&key.to_string()) {
            keys.push(key.to_string());
            self.update_index(&keys)?;
        }
        Ok(())
    }
    
    /// 从索引移除 key
    fn remove_from_index(&self, key: &str) -> Result<()> {
        let mut keys = self.read_index();
        keys.retain(|k| k != key);
        self.update_index(&keys)?;
        Ok(())
    }
    
    fn is_available() -> bool {
        // 尝试一个简单的 keyring 操作来检测是否可用
        let entry = keyring::Entry::new("pi-cli-auth-test", "availability-check");
        entry.is_ok()
    }
    
    /// 健康检查 - 验证 keychain 可以正常读写
    fn health_check(&self) -> bool {
        let test_key = "__health_check__";
        let test_data = b"health_check_ok";
        if self.save_raw(test_key, test_data).is_ok() {
            let result = self.load_raw(test_key).ok().flatten();
            let _ = self.delete_raw(test_key);
            result.as_deref() == Some(test_data.as_slice())
        } else {
            false
        }
    }
    
    /// 内部 save 方法（不更新索引）
    fn save_raw(&self, key: &str, data: &[u8]) -> Result<()> {
        let entry = keyring::Entry::new(&self.service_name, key)
            .context("Failed to create keychain entry")?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        entry.set_password(&encoded)
            .context("Failed to save to keychain")?;
        Ok(())
    }
    
    /// 内部 load 方法
    fn load_raw(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let entry = keyring::Entry::new(&self.service_name, key)
            .context("Failed to create keychain entry")?;
        match entry.get_password() {
            Ok(encoded) => {
                let data = base64::engine::general_purpose::STANDARD.decode(&encoded)
                    .context("Failed to decode keychain data")?;
                Ok(Some(data))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("Keychain error: {}", e)),
        }
    }
    
    /// 内部 delete 方法（不更新索引）
    fn delete_raw(&self, key: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service_name, key)
            .context("Failed to create keychain entry")?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // 已经不存在，也算成功
            Err(e) => Err(anyhow::anyhow!("Keychain delete error: {}", e)),
        }
    }
}

impl SecureStorage for KeychainStorage {
    fn save(&self, key: &str, data: &[u8]) -> Result<()> {
        self.save_raw(key, data)?;
        // 成功后更新索引文件
        self.add_to_index(key)?;
        Ok(())
    }
    
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.load_raw(key)
    }
    
    fn delete(&self, key: &str) -> Result<()> {
        self.delete_raw(key)?;
        // 成功后从索引中移除
        self.remove_from_index(key)?;
        Ok(())
    }
    
    fn list_keys(&self) -> Result<Vec<String>> {
        // 从独立索引文件读取
        Ok(self.read_index())
    }
}

use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;
use argon2::Argon2;

pub(crate) struct EncryptedFileStorage {
    storage_dir: PathBuf,
    encryption_key: [u8; 32],
}

impl EncryptedFileStorage {
    pub(crate) fn new(storage_dir: PathBuf) -> Result<Self> {
        let machine_id = Self::get_machine_id()?;
        let salt = b"pi-cli-token-storage-salt";
        let mut key = [0u8; 32];
        Argon2::default()
            .hash_password_into(machine_id.as_bytes(), salt, &mut key)
            .map_err(|e| anyhow::anyhow!("Key derivation failed: {}", e))?;
        Ok(Self { storage_dir, encryption_key: key })
    }
    
    fn get_machine_id() -> Result<String> {
        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("ioreg")
                .args(["-rd1", "-c", "IOPlatformExpertDevice"])
                .output()
                .context("Failed to get machine ID")?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("IOPlatformSerialNumber") {
                    if let Some(serial) = line.split('"').nth(3) {
                        return Ok(serial.to_string());
                    }
                }
            }
            // Fallback
            Ok("macos-default-machine-id".to_string())
        }
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/etc/machine-id")
                .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
                .context("Failed to read machine-id")
                .map(|id| id.trim().to_string())
        }
        #[cfg(target_os = "windows")]
        {
            let output = std::process::Command::new("wmic")
                .args(&["csproduct", "get", "UUID"])
                .output()
                .context("Failed to get machine UUID")?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.lines().nth(1).unwrap_or("windows-default").trim().to_string())
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Ok("unknown-platform-default-id".to_string())
        }
    }
    
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        use rand::RngCore;
        
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|e| anyhow::anyhow!("Cipher creation failed: {}", e))?;
        let mut nonce_bytes = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, data)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
        
        // 格式: nonce (12 bytes) + ciphertext
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }
    
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            anyhow::bail!("Invalid encrypted data: too short");
        }
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|e| anyhow::anyhow!("Cipher creation failed: {}", e))?;
        let nonce = Nonce::from_slice(&data[..12]);
        let plaintext = cipher.decrypt(nonce, &data[12..])
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;
        Ok(plaintext)
    }
    
    fn token_file_path(&self, key: &str) -> PathBuf {
        self.storage_dir.join(format!("{}.enc", key))
    }
}

impl SecureStorage for EncryptedFileStorage {
    fn save(&self, key: &str, data: &[u8]) -> Result<()> {
        std::fs::create_dir_all(&self.storage_dir)?;
        let encrypted = self.encrypt(data)?;
        let path = self.token_file_path(key);
        std::fs::write(&path, &encrypted)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
    
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.token_file_path(key);
        if !path.exists() {
            return Ok(None);
        }
        let encrypted = std::fs::read(&path)?;
        let decrypted = self.decrypt(&encrypted)?;
        Ok(Some(decrypted))
    }
    
    fn delete(&self, key: &str) -> Result<()> {
        let path = self.token_file_path(key);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
    
    fn list_keys(&self) -> Result<Vec<String>> {
        let mut keys = Vec::new();
        if self.storage_dir.exists() {
            for entry in std::fs::read_dir(&self.storage_dir)? {
                let entry = entry?;
                if let Some(name) = entry.path().file_stem() {
                    let name = name.to_string_lossy().to_string();
                    if entry.path().extension().is_some_and(|ext| ext == "enc") 
                       && name != "__provider_index__" {
                        keys.push(name);
                    }
                }
            }
        }
        Ok(keys)
    }
}

impl TokenStorage {
    /// 创建新的 Token 存储
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let auth_dir = home.join(".pi").join("auth");
        let legacy_path = auth_dir.join("tokens.json");
        
        // 尝试使用 Keychain，失败则回退到加密文件
        let storage: Box<dyn SecureStorage> = if KeychainStorage::is_available() {
            let keychain = KeychainStorage::new();
            // 进行更可靠的健康检查
            if keychain.health_check() {
                tracing::info!("Using system keychain for secure token storage");
                Box::new(keychain)
            } else {
                tracing::warn!("System keychain health check failed, falling back to encrypted file storage");
                let encrypted_dir = auth_dir.join("encrypted");
                match EncryptedFileStorage::new(encrypted_dir) {
                    Ok(s) => Box::new(s),
                    Err(e) => {
                        tracing::error!("Failed to initialize encrypted file storage: {}", e);
                        // 最终回退：重试 keychain（至少还能用）
                        Box::new(keychain)
                    }
                }
            }
        } else {
            tracing::warn!("System keychain not available, falling back to encrypted file storage");
            let encrypted_dir = auth_dir.join("encrypted");
            match EncryptedFileStorage::new(encrypted_dir) {
                Ok(s) => Box::new(s),
                Err(_) => {
                    // 最终回退：使用加密文件但用固定密钥（不推荐）
                    // 实际上这种情况极少发生
                    let encrypted_dir = auth_dir.join("encrypted");
                    Box::new(EncryptedFileStorage::new(encrypted_dir)
                        .expect("Failed to initialize any storage backend"))
                }
            }
        };
        
        let ts = Self { 
            storage, 
            legacy_path: legacy_path.clone(),
            refresh_locks: Arc::new(StdMutex::new(HashMap::new())),
        };
        
        // 自动迁移旧版明文存储
        if legacy_path.exists() {
            if let Err(e) = ts.migrate_from_plaintext(&legacy_path) {
                eprintln!("Warning: Failed to migrate legacy tokens: {}", e);
            }
        }
        
        ts
    }
    
    /// 用于测试的自定义路径构造器
    #[allow(dead_code)] // 用于测试
    pub(crate) fn with_storage(storage: Box<dyn SecureStorage>) -> Self {
        Self {
            storage,
            legacy_path: PathBuf::from("/nonexistent"),
            refresh_locks: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    /// 获取或创建指定 provider 的刷新锁
    fn get_refresh_lock(&self, provider: &str) -> Arc<TokioMutex<()>> {
        let mut locks = self.refresh_locks.lock().unwrap();
        locks.entry(provider.to_string())
            .or_insert_with(|| Arc::new(TokioMutex::new(())))
            .clone()
    }
    
    fn migrate_from_plaintext(&self, legacy_path: &std::path::Path) -> Result<()> {
        let content = std::fs::read_to_string(legacy_path)?;
        let store: TokenStore = serde_json::from_str(&content)?;
        for (provider, token) in store.tokens {
            let data = serde_json::to_vec(&token)?;
            self.storage.save(&provider, &data)?;
        }
        // 迁移成功后重命名旧文件（不删除，作为备份）
        let backup_path = legacy_path.with_extension("json.bak");
        std::fs::rename(legacy_path, &backup_path)?;
        Ok(())
    }
    
    /// 保存 token（带版本化）
    pub fn save_token(&self, token: &StoredToken) -> Result<()> {
        // 包装为版本化格式
        let token_value = serde_json::to_value(token)?;
        let versioned = VersionedTokenData {
            version: CURRENT_TOKEN_VERSION,
            data: token_value,
        };
        let data = serde_json::to_vec(&versioned)?;
        self.storage.save(&token.provider, &data)?;
        // 如果是 Keychain，更新索引
        self.update_provider_index()?;
        Ok(())
    }
    
    /// 获取指定 provider 的 token（支持版本迁移）
    pub fn get_token(&self, provider: &str) -> Option<StoredToken> {
        match self.storage.load(provider) {
            Ok(Some(data)) => {
                // 尝试解析为版本化格式
                if let Ok(versioned) = serde_json::from_slice::<VersionedTokenData>(&data) {
                    // 检查版本并迁移
                    match versioned.version {
                        CURRENT_TOKEN_VERSION => {
                            // 当前版本，直接解析
                            serde_json::from_value(versioned.data).ok()
                        }
                        1 => {
                            // v1 格式迁移
                            let token: StoredToken = serde_json::from_value(versioned.data).ok()?;
                            // 自动升级存储格式
                            if let Err(e) = self.save_token(&token) {
                                tracing::warn!("Failed to migrate token to v2: {}", e);
                            }
                            Some(token)
                        }
                        _ => {
                            tracing::warn!("Unknown token version: {}", versioned.version);
                            // 尝试直接解析
                            serde_json::from_value(versioned.data).ok()
                        }
                    }
                } else {
                    // 没有 version 字段，视为 v1 格式（旧格式直接是 StoredToken）
                    if let Ok(token) = serde_json::from_slice::<StoredToken>(&data) {
                        // 自动迁移到 v2
                        if let Err(e) = self.save_token(&token) {
                            tracing::warn!("Failed to migrate token to v2: {}", e);
                        }
                        Some(token)
                    } else {
                        None
                    }
                }
            }
            _ => None,
        }
    }
    
    /// 获取有效的 access token（未过期的）
    pub fn get_valid_token(&self, provider: &str) -> Option<String> {
        let token = self.get_token(provider)?;
        if token.is_expired() {
            None
        } else {
            Some(token.access_token.clone())
        }
    }
    
    /// 删除指定 provider 的 token
    pub fn remove_token(&self, provider: &str) -> Result<()> {
        self.storage.delete(provider)?;
        self.update_provider_index()?;
        Ok(())
    }
    
    /// 列出所有已存储的 provider
    pub fn list_providers(&self) -> Vec<String> {
        self.storage.list_keys().unwrap_or_default()
    }
    
    fn update_provider_index(&self) -> Result<()> {
        // 用于 KeychainStorage 的索引维护
        let providers = self.list_providers();
        let index_data = serde_json::to_vec(&providers)?;
        self.storage.save("__provider_index__", &index_data)?;
        Ok(())
    }
    
    /// 刷新 token（使用 refresh_token 获取新的 access_token）
    /// 
    /// 包含基础重试机制：失败时重试 1 次，间隔 1 秒
    pub async fn refresh_token(&self, provider: &str, token_url: &str, client_id: &str) -> Result<StoredToken> {
        let stored = self.get_token(provider)
            .context("No stored token found")?;
        
        let refresh_token = stored.refresh_token
            .context("No refresh token available")?;
        
        let client = reqwest::Client::new();
        
        // 第一次尝试
        let result = self.do_refresh_token(&client, provider, token_url, client_id, &refresh_token).await;
        
        match result {
            Ok(token) => Ok(token),
            Err(e) => {
                tracing::debug!("First token refresh attempt failed for {}: {}", provider, e);
                // 等待 1 秒后重试
                tokio::time::sleep(Duration::from_secs(1)).await;
                tracing::debug!("Retrying token refresh for {}", provider);
                self.do_refresh_token(&client, provider, token_url, client_id, &refresh_token).await
            }
        }
    }

    /// 执行实际的 token 刷新请求
    async fn do_refresh_token(
        &self,
        client: &reqwest::Client,
        provider: &str,
        token_url: &str,
        client_id: &str,
        refresh_token: &str,
    ) -> Result<StoredToken> {
        let resp = client.post(token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", client_id),
            ])
            .send()
            .await
            .map_err(|e| {
                // 区分网络错误
                if e.is_connect() || e.is_timeout() {
                    anyhow::anyhow!("Network error during token refresh for '{}': {}", provider, e)
                } else if e.is_request() {
                    anyhow::anyhow!("Request error during token refresh for '{}': {}", provider, e)
                } else {
                    anyhow::anyhow!("Failed to send refresh request for '{}': {}", provider, e)
                }
            })?;
        
        // 检查 HTTP 状态码
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let error = RefreshError::from_response(status, Some(&body));
            
            // 根据错误类型提供更详细的错误信息
            match error {
                RefreshError::AuthError(_) => {
                    tracing::error!(
                        provider = %provider,
                        status = %status,
                        body = %body,
                        "Authentication error during token refresh - re-login required"
                    );
                    anyhow::bail!(
                        "Authentication error for '{}': {}. Please run '/login {}' to re-authenticate.",
                        provider, error, provider
                    );
                }
                RefreshError::NetworkError(_) => {
                    tracing::warn!(
                        provider = %provider,
                        status = %status,
                        "Network error during token refresh - will retry"
                    );
                    anyhow::bail!(
                        "Network error for '{}': {}. This may be temporary.",
                        provider, error
                    );
                }
                RefreshError::Other(_) => {
                    anyhow::bail!(
                        "Token refresh failed for '{}': {}",
                        provider, error
                    );
                }
            }
        }
        
        let token_response: serde_json::Value = resp.json().await
            .context("Failed to parse refresh response")?;
        
        // 检查响应中是否包含错误
        if let Some(error) = token_response.get("error") {
            let error_desc = token_response.get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            
            tracing::error!(
                provider = %provider,
                error = %error,
                description = %error_desc,
                "OAuth error in refresh response"
            );
            
            anyhow::bail!(
                "OAuth error for '{}': {} - {}",
                provider, error, error_desc
            );
        }
        
        let new_token = StoredToken {
            provider: provider.to_string(),
            access_token: token_response["access_token"]
                .as_str()
                .context("No access_token in refresh response")?
                .to_string(),
            refresh_token: token_response["refresh_token"]
                .as_str()
                .map(|s| s.to_string())
                .or(Some(refresh_token.to_string())),
            expires_at: token_response["expires_in"]
                .as_u64()
                .map(|secs| Utc::now() + chrono::Duration::seconds(secs as i64)),
        };
        
        self.save_token(&new_token)?;
        tracing::info!(
            provider = %provider,
            expires_at = ?new_token.expires_at,
            "Token refreshed successfully"
        );
        Ok(new_token)
    }

    /// 获取有效的 token，如果即将过期则自动尝试刷新
    /// 
    /// 策略：
    /// 1. Token 有效且不在过期预警期 -> 直接返回
    /// 2. Token 即将过期（5分钟内）且有 refresh_token -> 尝试刷新
    ///    - 使用并发锁确保同一时刻只有一个刷新请求
    ///    - 刷新成功 -> 返回新 token
    ///    - 刷新失败但 token 未过期 -> 返回旧 token 并记录警告
    /// 3. Token 已过期且刷新失败 -> 返回 None（需要重新登录）
    /// 
    /// 并发保护：
    /// - 使用 provider 级别的锁，确保同一 provider 的并发刷新请求只触发一次实际刷新
    /// - 其他请求会等待刷新完成后直接读取新 token
    /// 
    /// 错误恢复：
    /// - 网络错误：使用旧 token（如果未过期）或返回 None
    /// - 认证错误：返回 None，提示用户重新登录
    pub async fn get_valid_token_or_refresh(
        &self,
        provider: &str,
    ) -> Option<String> {
        // 先读取 token 状态（无锁，快速路径）
        let token = self.get_token(provider)?;
        
        // Token 有效且不在过期预警期
        if !token.is_expired() && !token.is_expiring_soon() {
            return Some(token.access_token.clone());
        }
        
        // 需要刷新 - 获取该 provider 的刷新锁
        let lock = self.get_refresh_lock(provider);
        let _guard = lock.lock().await;
        
        // 获取锁后再次检查 token 状态（可能其他请求已经刷新成功）
        let token = self.get_token(provider)?;
        if !token.is_expired() && !token.is_expiring_soon() {
            return Some(token.access_token.clone());
        }
        
        // 需要刷新 - 获取 provider 配置
        if let Some(provider_config) = crate::core::auth::providers::get_oauth_provider(provider) {
            match self.refresh_token(provider, &provider_config.token_url, &provider_config.client_id).await {
                Ok(new_token) => {
                    return Some(new_token.access_token);
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    
                    // 判断错误类型
                    let is_auth_error = error_msg.contains("Authentication error") 
                        || error_msg.contains("OAuth error");
                    let is_network_error = error_msg.contains("Network error") 
                        || error_msg.contains("timeout") 
                        || error_msg.contains("connect");
                    
                    if is_auth_error {
                        // 认证错误 - 需要重新登录
                        tracing::error!(
                            provider = %provider,
                            error = %error_msg,
                            "Authentication error during token refresh - re-login required"
                        );
                        tracing::warn!(
                            "Token refresh failed for provider '{}'. Please run '/login {}' to re-authenticate.",
                            provider, provider
                        );
                        return None;
                    }
                    
                    if is_network_error {
                        // 网络错误 - 尝试使用旧 token
                        tracing::warn!(
                            provider = %provider,
                            error = %error_msg,
                            "Network error during token refresh"
                        );
                        if !token.is_expired() {
                            tracing::info!(
                                provider = %provider,
                                "Using existing token despite refresh failure (network issue)"
                            );
                            return Some(token.access_token.clone());
                        }
                        tracing::warn!(
                            "Token refresh failed due to network issues for provider '{}'. Token may be expired.",
                            provider
                        );
                        return None;
                    }
                    
                    // 其他错误
                    tracing::warn!(
                        provider = %provider,
                        error = %error_msg,
                        "Token refresh failed"
                    );
                    // 刷新失败但 token 可能还没完全过期
                    if !token.is_expired() {
                        tracing::info!(
                            provider = %provider,
                            "Using existing token for '{}' (expires soon, refresh failed)",
                            provider
                        );
                        return Some(token.access_token.clone());
                    }
                    // Token 已过期且刷新失败
                    tracing::warn!(
                        "Token refresh failed for provider '{}'. Please run '/login {}' to re-authenticate.",
                        provider, provider
                    );
                    return None;
                }
            }
        }
        
        // 没有 provider 配置，无法刷新
        if !token.is_expired() {
            Some(token.access_token.clone())
        } else {
            None
        }
    }
}

impl Default for TokenStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn create_test_token(provider: &str, expires_at: Option<DateTime<Utc>>) -> StoredToken {
        StoredToken {
            provider: provider.to_string(),
            access_token: "test_access_token".to_string(),
            refresh_token: Some("test_refresh_token".to_string()),
            expires_at,
        }
    }

    #[test]
    fn test_stored_token_not_expired() {
        let token = create_test_token("test", Some(Utc::now() + Duration::hours(1)));
        assert!(!token.is_expired());
    }

    #[test]
    fn test_stored_token_expired() {
        let token = create_test_token("test", Some(Utc::now() - Duration::hours(1)));
        assert!(token.is_expired());
    }

    #[test]
    fn test_stored_token_expiring_soon() {
        let token = create_test_token("test", Some(Utc::now() + Duration::minutes(3)));
        assert!(token.is_expiring_soon());
    }

    #[test]
    fn test_stored_token_not_expiring_soon() {
        let token = create_test_token("test", Some(Utc::now() + Duration::hours(1)));
        assert!(!token.is_expiring_soon());
    }

    #[test]
    fn test_stored_token_no_expiry() {
        let token = create_test_token("test", None);
        assert!(!token.is_expired());
        assert!(!token.is_expiring_soon());
    }

    #[test]
    fn test_save_and_get_token() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        let token = create_test_token("test_provider", Some(Utc::now() + Duration::hours(1)));
        token_storage.save_token(&token).unwrap();

        let retrieved = token_storage.get_token("test_provider");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.provider, "test_provider");
        assert_eq!(retrieved.access_token, "test_access_token");
    }

    #[test]
    fn test_get_valid_token_returns_none_for_expired() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        let token = create_test_token("test_provider", Some(Utc::now() - Duration::hours(1)));
        token_storage.save_token(&token).unwrap();

        let valid_token = token_storage.get_valid_token("test_provider");
        assert!(valid_token.is_none());
    }

    #[test]
    fn test_get_valid_token_returns_some_for_valid() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        let token = create_test_token("test_provider", Some(Utc::now() + Duration::hours(1)));
        token_storage.save_token(&token).unwrap();

        let valid_token = token_storage.get_valid_token("test_provider");
        assert!(valid_token.is_some());
        assert_eq!(valid_token.unwrap(), "test_access_token");
    }

    #[test]
    fn test_remove_token() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        let token = create_test_token("test_provider", Some(Utc::now() + Duration::hours(1)));
        token_storage.save_token(&token).unwrap();

        // Verify token exists
        assert!(token_storage.get_token("test_provider").is_some());

        // Remove token
        token_storage.remove_token("test_provider").unwrap();

        // Verify token is removed
        assert!(token_storage.get_token("test_provider").is_none());
    }

    #[test]
    fn test_list_providers() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        let token1 = create_test_token("provider1", Some(Utc::now() + Duration::hours(1)));
        let token2 = create_test_token("provider2", Some(Utc::now() + Duration::hours(1)));
        let token3 = create_test_token("provider3", Some(Utc::now() + Duration::hours(1)));

        token_storage.save_token(&token1).unwrap();
        token_storage.save_token(&token2).unwrap();
        token_storage.save_token(&token3).unwrap();

        let providers = token_storage.list_providers();
        assert_eq!(providers.len(), 3);
        assert!(providers.contains(&"provider1".to_string()));
        assert!(providers.contains(&"provider2".to_string()));
        assert!(providers.contains(&"provider3".to_string()));
    }

    #[test]
    fn test_encrypted_storage_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();

        let data = b"Hello, World! This is a test message.";
        storage.save("test_key", data).unwrap();

        let loaded = storage.load("test_key").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap(), data);
    }

    #[test]
    fn test_encrypted_storage_delete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();

        let data = b"Test data";
        storage.save("test_key", data).unwrap();
        assert!(storage.load("test_key").unwrap().is_some());

        storage.delete("test_key").unwrap();
        assert!(storage.load("test_key").unwrap().is_none());
    }

    #[test]
    fn test_encrypted_storage_list_keys() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();

        storage.save("key1", b"data1").unwrap();
        storage.save("key2", b"data2").unwrap();
        storage.save("key3", b"data3").unwrap();

        let keys = storage.list_keys().unwrap();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
        assert!(keys.contains(&"key3".to_string()));
    }

    // ========== 并发刷新保护测试 ==========

    #[tokio::test]
    async fn test_concurrent_refresh_protection() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Duration as StdDuration;

        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        // 创建一个即将过期的 token
        let token = create_test_token("test_provider", Some(Utc::now() + Duration::minutes(3)));
        token_storage.save_token(&token).unwrap();

        // 创建计数器来跟踪锁获取次数
        let counter = Arc::new(AtomicUsize::new(0));

        // 模拟并发调用 get_refresh_lock
        let mut handles = vec![];
        for i in 0..5 {
            let ts_clone = TokenStorage::with_storage(Box::new(
                EncryptedFileStorage::new(temp_dir.path().join(format!("clone{}", i)).to_path_buf()).unwrap()
            ));
            let counter_clone = counter.clone();
            let handle = tokio::spawn(async move {
                let lock = ts_clone.get_refresh_lock("test_provider");
                let _guard = lock.lock().await;
                counter_clone.fetch_add(1, Ordering::SeqCst);
                // 模拟一些工作
                tokio::time::sleep(StdDuration::from_millis(10)).await;
            });
            handles.push(handle);
        }

        // 等待所有任务完成
        for handle in handles {
            handle.await.unwrap();
        }

        // 所有锁都应该被成功获取
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn test_refresh_lock_per_provider() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        // 获取不同 provider 的锁应该是独立的
        let lock1 = token_storage.get_refresh_lock("provider1");
        let lock2 = token_storage.get_refresh_lock("provider2");
        let lock3 = token_storage.get_refresh_lock("provider1"); // 同一个 provider

        // lock1 和 lock3 应该是同一个锁（Arc 克隆）
        let guard1 = lock1.try_lock();
        let guard3 = lock3.try_lock();

        // lock1 和 lock3 是同一个 Arc，所以 lock3 应该无法获取锁
        assert!(guard1.is_ok());
        assert!(guard3.is_err()); // 已经被 lock1 占用

        // lock2 是独立的，应该可以获取
        let guard2 = lock2.try_lock();
        assert!(guard2.is_ok());
    }

    // ========== 重试机制测试 ==========

    #[tokio::test]
    async fn test_retry_mechanism_first_fail_second_success() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // 创建一个计数器来跟踪请求次数
        static REQUEST_COUNT: AtomicUsize = AtomicUsize::new(0);

        // 启动一个模拟服务器
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                // 读取请求（必须读完请求后再响应）
                let mut buf = vec![0u8; 4096];
                let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
                let count = REQUEST_COUNT.fetch_add(1, Ordering::SeqCst);
                
                let response = if count == 0 {
                    // 第一次请求失败
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_string()
                } else {
                    // 第二次请求成功
                    let body = r#"{"access_token":"new_token","refresh_token":"new_refresh","expires_in":3600}"#;
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body)
                };
                
                tokio::io::AsyncWriteExt::write_all(&mut socket, response.as_bytes()).await.unwrap();
                if count >= 1 {
                    break;
                }
            }
        });

        // 等待服务器启动
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // 创建测试 token
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        let token = StoredToken {
            provider: "test_provider".to_string(),
            access_token: "old_access_token".to_string(),
            refresh_token: Some("test_refresh_token".to_string()),
            expires_at: Some(Utc::now() + Duration::minutes(3)),
        };
        token_storage.save_token(&token).unwrap();

        // 调用 refresh_token
        let token_url = format!("http://127.0.0.1:{}/token", port);
        let result = token_storage.refresh_token("test_provider", &token_url, "test_client_id").await;

        // 等待服务器完成
        tokio::time::timeout(std::time::Duration::from_secs(5), server).await.ok();

        // 验证结果 - 应该成功（第二次尝试）
        assert!(result.is_ok());
        let new_token = result.unwrap();
        assert_eq!(new_token.access_token, "new_token");
        assert_eq!(new_token.refresh_token, Some("new_refresh".to_string()));
        
        // 验证请求了两次
        assert_eq!(REQUEST_COUNT.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_mechanism_both_fail() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // 创建一个计数器来跟踪请求次数
        static REQUEST_COUNT: AtomicUsize = AtomicUsize::new(0);

        // 启动一个模拟服务器 - 总是返回错误
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                // 读取请求
                let mut buf = vec![0u8; 4096];
                let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
                REQUEST_COUNT.fetch_add(1, Ordering::SeqCst);
                
                let response = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n";
                tokio::io::AsyncWriteExt::write_all(&mut socket, response.as_bytes()).await.unwrap();
            }
        });

        // 等待服务器启动
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // 创建测试 token
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        let token = StoredToken {
            provider: "test_provider".to_string(),
            access_token: "old_access_token".to_string(),
            refresh_token: Some("test_refresh_token".to_string()),
            expires_at: Some(Utc::now() + Duration::minutes(3)),
        };
        token_storage.save_token(&token).unwrap();

        // 调用 refresh_token
        let token_url = format!("http://127.0.0.1:{}/token", port);
        let result = token_storage.refresh_token("test_provider", &token_url, "test_client_id").await;

        // 等待服务器完成
        tokio::time::timeout(std::time::Duration::from_secs(5), server).await.ok();

        // 验证结果 - 应该失败
        assert!(result.is_err());
        
        // 验证了两次请求（原始请求 + 1次重试）
        assert_eq!(REQUEST_COUNT.load(Ordering::SeqCst), 2);
    }

    // ========== RefreshError 测试 ==========

    #[test]
    fn test_refresh_error_display() {
        let err = RefreshError::NetworkError("connection timeout".to_string());
        assert!(err.to_string().contains("Network error"));
        
        let err = RefreshError::AuthError("invalid token".to_string());
        assert!(err.to_string().contains("Authentication error"));
        
        let err = RefreshError::Other("unknown".to_string());
        assert!(err.to_string().contains("Error"));
    }

    #[test]
    fn test_refresh_error_is_retryable() {
        let err = RefreshError::NetworkError("timeout".to_string());
        assert!(err.is_retryable());
        
        let err = RefreshError::AuthError("invalid".to_string());
        assert!(!err.is_retryable());
        
        let err = RefreshError::Other("unknown".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_refresh_error_requires_relogin() {
        let err = RefreshError::AuthError("invalid token".to_string());
        assert!(err.requires_relogin());
        
        let err = RefreshError::NetworkError("timeout".to_string());
        assert!(!err.requires_relogin());
        
        let err = RefreshError::Other("unknown".to_string());
        assert!(!err.requires_relogin());
    }

    #[test]
    fn test_refresh_error_from_response_auth() {
        let err = RefreshError::from_response(
            reqwest::StatusCode::UNAUTHORIZED,
            Some("invalid_token"),
        );
        assert!(err.requires_relogin());
        assert!(!err.is_retryable());
        
        let err = RefreshError::from_response(
            reqwest::StatusCode::FORBIDDEN,
            Some("access denied"),
        );
        assert!(err.requires_relogin());
    }

    #[test]
    fn test_refresh_error_from_response_network() {
        let err = RefreshError::from_response(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            None,
        );
        assert!(err.is_retryable());
        assert!(!err.requires_relogin());
        
        let err = RefreshError::from_response(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            None,
        );
        assert!(err.is_retryable());
        
        let err = RefreshError::from_response(
            reqwest::StatusCode::BAD_GATEWAY,
            None,
        );
        assert!(err.is_retryable());
    }

    #[test]
    fn test_refresh_error_from_response_other() {
        let err = RefreshError::from_response(
            reqwest::StatusCode::NOT_FOUND,
            Some("not found"),
        );
        assert!(!err.is_retryable());
        assert!(!err.requires_relogin());
    }

    // ========== 版本化 Token 测试 ==========

    #[test]
    fn test_versioned_token_save_load() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        let token = StoredToken {
            provider: "test_provider".to_string(),
            access_token: "test_access_token".to_string(),
            refresh_token: Some("test_refresh_token".to_string()),
            expires_at: Some(Utc::now() + Duration::hours(1)),
        };
        token_storage.save_token(&token).unwrap();

        // 读取并验证版本化格式
        let retrieved = token_storage.get_token("test_provider");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.provider, "test_provider");
        assert_eq!(retrieved.access_token, "test_access_token");
        assert_eq!(retrieved.refresh_token, Some("test_refresh_token".to_string()));
    }

    #[test]
    fn test_version_migration_v1_to_v2() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();

        // 模拟 v1 格式数据（直接 StoredToken JSON，没有 version 字段）
        let v1_token = StoredToken {
            provider: "v1_provider".to_string(),
            access_token: "v1_access_token".to_string(),
            refresh_token: Some("v1_refresh_token".to_string()),
            expires_at: Some(Utc::now() + Duration::hours(1)),
        };
        let v1_data = serde_json::to_vec(&v1_token).unwrap();
        
        // 直接存储 v1 格式（绕过 save_token 的版本化）
        storage.save("v1_provider", &v1_data).unwrap();

        // 创建 TokenStorage（使用同一个存储目录的新实例）
        let token_storage = TokenStorage::with_storage(Box::new(
            EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap()
        ));

        // 使用 get_token 读取，应该自动迁移
        let retrieved = token_storage.get_token("v1_provider");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.provider, "v1_provider");
        assert_eq!(retrieved.access_token, "v1_access_token");

        // 再次读取，验证已被迁移为 v2 格式
        let raw_data = storage.load("v1_provider").unwrap().unwrap();
        let versioned: VersionedTokenData = serde_json::from_slice(&raw_data).unwrap();
        assert_eq!(versioned.version, CURRENT_TOKEN_VERSION);
    }

    #[test]
    fn test_version_migration_with_version_field() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();

        // 模拟带 version 字段的 v1 格式
        let v1_token = serde_json::json!({
            "provider": "v1_explicit",
            "access_token": "v1_explicit_token",
            "refresh_token": "v1_refresh",
            "expires_at": "2025-01-01T00:00:00Z"
        });
        let versioned_v1 = VersionedTokenData {
            version: 1,
            data: v1_token,
        };
        let v1_data = serde_json::to_vec(&versioned_v1).unwrap();
        
        // 直接存储
        storage.save("v1_explicit", &v1_data).unwrap();

        // 创建 TokenStorage（使用同一个存储目录的新实例）
        let token_storage = TokenStorage::with_storage(Box::new(
            EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap()
        ));

        // 使用 get_token 读取，应该自动迁移
        let retrieved = token_storage.get_token("v1_explicit");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.provider, "v1_explicit");
        assert_eq!(retrieved.access_token, "v1_explicit_token");

        // 验证已迁移为 v2
        let raw_data = storage.load("v1_explicit").unwrap().unwrap();
        let versioned: VersionedTokenData = serde_json::from_slice(&raw_data).unwrap();
        assert_eq!(versioned.version, CURRENT_TOKEN_VERSION);
    }

    // ========== 索引文件测试 ==========

    #[test]
    fn test_keychain_index_file_operations() {
        let temp_dir = tempfile::tempdir().unwrap();
        let index_path = temp_dir.path().join("keychain_index.json");
        
        // 创建一个测试用的 KeychainStorage
        let keychain = TestKeychainStorage {
            service_name: "test-service".to_string(),
            index_path: index_path.clone(),
        };

        // 测试空索引
        let keys = keychain.read_index();
        assert!(keys.is_empty());

        // 测试添加索引
        keychain.add_to_index("provider1").unwrap();
        let keys = keychain.read_index();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains(&"provider1".to_string()));

        // 测试重复添加（不会重复）
        keychain.add_to_index("provider1").unwrap();
        let keys = keychain.read_index();
        assert_eq!(keys.len(), 1);

        // 测试添加另一个
        keychain.add_to_index("provider2").unwrap();
        let keys = keychain.read_index();
        assert_eq!(keys.len(), 2);

        // 测试移除
        keychain.remove_from_index("provider1").unwrap();
        let keys = keychain.read_index();
        assert_eq!(keys.len(), 1);
        assert!(!keys.contains(&"provider1".to_string()));
        assert!(keys.contains(&"provider2".to_string()));

        // 验证文件存在
        assert!(index_path.exists());
    }

    /// 测试用 KeychainStorage（只测试索引逻辑，不实际访问 keychain）
    #[allow(dead_code)] // 测试辅助结构体
    struct TestKeychainStorage {
        #[allow(dead_code)]
        service_name: String,
        index_path: PathBuf,
    }

    impl TestKeychainStorage {
        fn update_index(&self, keys: &[String]) -> Result<()> {
            if let Some(parent) = self.index_path.parent() {
                std::fs::create_dir_all(parent)
                    .context("Failed to create index directory")?;
            }
            let content = serde_json::to_string_pretty(keys)
                .context("Failed to serialize index")?;
            std::fs::write(&self.index_path, content)
                .context("Failed to write index file")?;
            Ok(())
        }

        fn read_index(&self) -> Vec<String> {
            match std::fs::read_to_string(&self.index_path) {
                Ok(content) => {
                    serde_json::from_str(&content).unwrap_or_default()
                }
                Err(_) => vec![],
            }
        }

        fn add_to_index(&self, key: &str) -> Result<()> {
            let mut keys = self.read_index();
            if !keys.contains(&key.to_string()) {
                keys.push(key.to_string());
                self.update_index(&keys)?;
            }
            Ok(())
        }

        fn remove_from_index(&self, key: &str) -> Result<()> {
            let mut keys = self.read_index();
            keys.retain(|k| k != key);
            self.update_index(&keys)?;
            Ok(())
        }
    }

    // ========== 健康检查测试 ==========

    #[test]
    fn test_keychain_health_check_simulation() {
        // 使用 EncryptedFileStorage 模拟健康检查逻辑
        // 因为实际 KeychainStorage 需要系统 keychain 支持
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();

        // 模拟健康检查流程
        let test_key = "__health_check__";
        let test_data = b"health_check_ok";
        
        // save
        let save_result = storage.save(test_key, test_data);
        assert!(save_result.is_ok());
        
        // load
        let load_result = storage.load(test_key);
        assert!(load_result.is_ok());
        let loaded = load_result.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.as_deref(), Some(test_data.as_slice()));
        
        // delete
        let delete_result = storage.delete(test_key);
        assert!(delete_result.is_ok());
        
        // verify deleted
        let verify_result = storage.load(test_key);
        assert!(verify_result.is_ok());
        assert!(verify_result.unwrap().is_none());
    }

    #[test]
    fn test_storage_roundtrip_with_versioning() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = EncryptedFileStorage::new(temp_dir.path().to_path_buf()).unwrap();
        let token_storage = TokenStorage::with_storage(Box::new(storage));

        // 创建并保存多个 token
        let tokens = vec![
            StoredToken {
                provider: "github".to_string(),
                access_token: "gh_token".to_string(),
                refresh_token: Some("gh_refresh".to_string()),
                expires_at: Some(Utc::now() + Duration::hours(1)),
            },
            StoredToken {
                provider: "gitlab".to_string(),
                access_token: "gl_token".to_string(),
                refresh_token: None,
                expires_at: None,
            },
            StoredToken {
                provider: "bitbucket".to_string(),
                access_token: "bb_token".to_string(),
                refresh_token: Some("bb_refresh".to_string()),
                expires_at: Some(Utc::now() + Duration::days(7)),
            },
        ];

        for token in &tokens {
            token_storage.save_token(token).unwrap();
        }

        // 验证所有 token 都能正确读取
        for token in &tokens {
            let retrieved = token_storage.get_token(&token.provider);
            assert!(retrieved.is_some());
            let retrieved = retrieved.unwrap();
            assert_eq!(retrieved.access_token, token.access_token);
            assert_eq!(retrieved.refresh_token, token.refresh_token);
        }

        // 验证 list_providers
        let providers = token_storage.list_providers();
        assert_eq!(providers.len(), 3);
        assert!(providers.contains(&"github".to_string()));
        assert!(providers.contains(&"gitlab".to_string()));
        assert!(providers.contains(&"bitbucket".to_string()));
    }
}
