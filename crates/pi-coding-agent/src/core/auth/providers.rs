use serde::{Serialize, Deserialize};

/// OAuth Provider 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthProviderConfig {
    /// Provider 名称
    pub name: String,
    /// 授权 URL
    pub authorize_url: String,
    /// Token URL
    pub token_url: String,
    /// 客户端 ID
    pub client_id: String,
    /// 权限范围
    pub scopes: Vec<String>,
    /// 是否使用 PKCE
    pub use_pkce: bool,
    /// Provider 特有的额外授权 URL 参数
    pub extra_auth_params: Vec<(String, String)>,
}

/// 获取内置 OAuth 提供商配置
pub fn get_oauth_provider(name: &str) -> Option<OAuthProviderConfig> {
    match name {
        "anthropic" => Some(OAuthProviderConfig {
            name: "anthropic".to_string(),
            authorize_url: "https://console.anthropic.com/oauth/authorize".to_string(),
            token_url: "https://console.anthropic.com/oauth/token".to_string(),
            client_id: "pi-coding-agent".to_string(),
            scopes: vec!["user:inference".to_string()],
            use_pkce: true,
            extra_auth_params: vec![],
        }),
        "github-copilot" => Some(OAuthProviderConfig {
            name: "github-copilot".to_string(),
            authorize_url: "https://github.com/login/device/code".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            client_id: "Iv1.b507a08c87ecfe98".to_string(),
            scopes: vec!["copilot".to_string()],
            use_pkce: false,
            extra_auth_params: vec![],
        }),
        "openai" => Some(OAuthProviderConfig {
            name: "openai".to_string(),
            authorize_url: "https://auth.openai.com/authorize".to_string(),
            token_url: "https://auth.openai.com/oauth/token".to_string(),
            client_id: "app_live_rlRRsAMIvfOyyPxU1gzM4SZQ".to_string(),
            scopes: vec!["openai.public".to_string()],
            use_pkce: true,
            extra_auth_params: vec![
                ("audience".to_string(), "https://api.openai.com/v1".to_string()),
            ],
        }),
        "google" => Some(OAuthProviderConfig {
            name: "google".to_string(),
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            client_id: "764086051850-6qr4p6gpi6hn506pt8ejuq83di341hur.apps.googleusercontent.com".to_string(),
            scopes: vec![
                "https://www.googleapis.com/auth/generative-language".to_string(),
            ],
            use_pkce: true,
            extra_auth_params: vec![
                ("access_type".to_string(), "offline".to_string()),
                ("prompt".to_string(), "consent".to_string()),
            ],
        }),
        "azure-openai" => Some(OAuthProviderConfig {
            name: "azure-openai".to_string(),
            authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".to_string(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".to_string(),
            client_id: "pi-coding-agent-azure".to_string(),
            scopes: vec!["https://cognitiveservices.azure.com/.default".to_string()],
            use_pkce: true,
            extra_auth_params: vec![
                ("response_mode".to_string(), "query".to_string()),
            ],
        }),
        "mistral" => Some(OAuthProviderConfig {
            name: "mistral".to_string(),
            authorize_url: "https://auth.mistral.ai/oauth/authorize".to_string(),
            token_url: "https://auth.mistral.ai/oauth/token".to_string(),
            client_id: "pi-coding-agent-mistral".to_string(),
            scopes: vec!["api".to_string()],
            use_pkce: true,
            extra_auth_params: vec![],
        }),
        "huggingface" => Some(OAuthProviderConfig {
            name: "huggingface".to_string(),
            authorize_url: "https://huggingface.co/oauth/authorize".to_string(),
            token_url: "https://huggingface.co/oauth/token".to_string(),
            client_id: "pi-coding-agent-hf".to_string(),
            scopes: vec!["inference-api".to_string()],
            use_pkce: true,
            extra_auth_params: vec![],
        }),
        "openrouter" => Some(OAuthProviderConfig {
            name: "openrouter".to_string(),
            authorize_url: "https://openrouter.ai/auth".to_string(),
            token_url: "https://openrouter.ai/api/v1/auth/keys".to_string(),
            client_id: "pi-coding-agent-or".to_string(),
            scopes: vec![],
            use_pkce: false,
            extra_auth_params: vec![],
        }),
        _ => None,
    }
}

/// 列出所有支持的 OAuth 提供商
pub fn list_oauth_providers() -> Vec<&'static str> {
    vec!["anthropic", "github-copilot", "openai", "google", "azure-openai", "mistral", "huggingface", "openrouter"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_anthropic_provider() {
        let provider = get_oauth_provider("anthropic");
        assert!(provider.is_some());
        let p = provider.unwrap();
        assert_eq!(p.name, "anthropic");
        assert!(p.use_pkce);
        assert!(!p.scopes.is_empty());
        assert!(p.authorize_url.starts_with("https://"));
        assert!(p.token_url.starts_with("https://"));
    }

    #[test]
    fn test_get_openai_provider() {
        let provider = get_oauth_provider("openai");
        assert!(provider.is_some());
        let p = provider.unwrap();
        assert_eq!(p.name, "openai");
        assert!(p.use_pkce);
    }

    #[test]
    fn test_get_google_provider() {
        let provider = get_oauth_provider("google");
        assert!(provider.is_some());
        let p = provider.unwrap();
        assert_eq!(p.name, "google");
        assert!(p.use_pkce);
        // Google 应该有 extra_auth_params
        assert!(!p.extra_auth_params.is_empty());
        // 验证 access_type=offline
        assert!(p.extra_auth_params.iter().any(|(k, v)| k == "access_type" && v == "offline"));
    }

    #[test]
    fn test_get_github_copilot_provider() {
        let provider = get_oauth_provider("github-copilot");
        assert!(provider.is_some());
    }

    #[test]
    fn test_get_azure_openai_provider() {
        let provider = get_oauth_provider("azure-openai");
        assert!(provider.is_some());
        let p = provider.unwrap();
        assert_eq!(p.name, "azure-openai");
        assert!(p.use_pkce);
        assert!(!p.scopes.is_empty());
        // Azure 应该有 extra_auth_params (response_mode)
        assert!(!p.extra_auth_params.is_empty());
        assert!(p.extra_auth_params.iter().any(|(k, v)| k == "response_mode" && v == "query"));
    }

    #[test]
    fn test_get_mistral_provider() {
        let provider = get_oauth_provider("mistral");
        assert!(provider.is_some());
        let p = provider.unwrap();
        assert_eq!(p.name, "mistral");
        assert!(p.use_pkce);
        assert!(!p.scopes.is_empty());
    }

    #[test]
    fn test_get_huggingface_provider() {
        let provider = get_oauth_provider("huggingface");
        assert!(provider.is_some());
        let p = provider.unwrap();
        assert_eq!(p.name, "huggingface");
        assert!(p.use_pkce);
        assert!(!p.scopes.is_empty());
    }

    #[test]
    fn test_get_openrouter_provider() {
        let provider = get_oauth_provider("openrouter");
        assert!(provider.is_some());
        let p = provider.unwrap();
        assert_eq!(p.name, "openrouter");
        // OpenRouter 不使用 PKCE
        assert!(!p.use_pkce);
        // OpenRouter 没有 scopes
        assert!(p.scopes.is_empty());
    }

    #[test]
    fn test_get_unknown_provider() {
        let provider = get_oauth_provider("nonexistent");
        assert!(provider.is_none());
    }

    #[test]
    fn test_list_oauth_providers() {
        let providers = list_oauth_providers();
        assert!(providers.len() >= 8);
        assert!(providers.contains(&"anthropic"));
        assert!(providers.contains(&"openai"));
        assert!(providers.contains(&"google"));
        assert!(providers.contains(&"azure-openai"));
        assert!(providers.contains(&"mistral"));
        assert!(providers.contains(&"huggingface"));
        assert!(providers.contains(&"openrouter"));
    }

    #[test]
    fn test_provider_urls_are_valid_https() {
        for name in list_oauth_providers() {
            let provider = get_oauth_provider(name).unwrap();
            assert!(
                provider.authorize_url.starts_with("https://"),
                "Provider {} authorize_url should be HTTPS",
                name
            );
            assert!(
                provider.token_url.starts_with("https://"),
                "Provider {} token_url should be HTTPS",
                name
            );
        }
    }

    // ========== OAuth Provider 配置完整性测试 ==========

    #[test]
    fn test_all_providers_have_required_fields() {
        for name in list_oauth_providers() {
            let provider = get_oauth_provider(name).unwrap();
            
            // 验证所有必需字段
            assert!(!provider.name.is_empty(), "Provider {} should have a name", name);
            assert!(!provider.authorize_url.is_empty(), "Provider {} should have authorize_url", name);
            assert!(!provider.token_url.is_empty(), "Provider {} should have token_url", name);
            assert!(!provider.client_id.is_empty(), "Provider {} should have client_id", name);
            // scopes 可以为空（如 openrouter）
        }
    }

    #[test]
    fn test_provider_name_matches_key() {
        for name in list_oauth_providers() {
            let provider = get_oauth_provider(name).unwrap();
            assert_eq!(
                provider.name, name,
                "Provider name should match the key used to retrieve it"
            );
        }
    }

    // ========== URL 格式验证测试 ==========

    #[test]
    fn test_authorize_url_format() {
        for name in list_oauth_providers() {
            let provider = get_oauth_provider(name).unwrap();
            
            // 验证是有效的 HTTPS URL
            assert!(
                provider.authorize_url.starts_with("https://"),
                "Provider {} authorize_url should start with https://",
                name
            );
            
            // 验证包含主机名
            assert!(
                provider.authorize_url.len() > 8,
                "Provider {} authorize_url should have a hostname",
                name
            );
            
            // 验证不包含空格
            assert!(
                !provider.authorize_url.contains(' '),
                "Provider {} authorize_url should not contain spaces",
                name
            );
        }
    }

    #[test]
    fn test_token_url_format() {
        for name in list_oauth_providers() {
            let provider = get_oauth_provider(name).unwrap();
            
            // 验证是有效的 HTTPS URL
            assert!(
                provider.token_url.starts_with("https://"),
                "Provider {} token_url should start with https://",
                name
            );
            
            // 验证包含主机名
            assert!(
                provider.token_url.len() > 8,
                "Provider {} token_url should have a hostname",
                name
            );
            
            // 验证不包含空格
            assert!(
                !provider.token_url.contains(' '),
                "Provider {} token_url should not contain spaces",
                name
            );
        }
    }

    #[test]
    fn test_url_host_consistency() {
        for name in list_oauth_providers() {
            let provider = get_oauth_provider(name).unwrap();
            
            // 提取主机名（简化检查）
            let auth_host = provider.authorize_url
                .trim_start_matches("https://")
                .split('/')
                .next()
                .unwrap_or("");
            let token_host = provider.token_url
                .trim_start_matches("https://")
                .split('/')
                .next()
                .unwrap_or("");
            
            // 大多数 provider 的 authorize 和 token URL 应该在同一域名下
            // 但有些（如 Azure）可能不同，所以只验证都不为空
            assert!(!auth_host.is_empty(), "Provider {} should have valid authorize host", name);
            assert!(!token_host.is_empty(), "Provider {} should have valid token host", name);
        }
    }

    // ========== Scope 配置正确性测试 ==========

    #[test]
    fn test_anthropic_scopes() {
        let provider = get_oauth_provider("anthropic").unwrap();
        assert!(provider.scopes.contains(&"user:inference".to_string()));
        assert!(provider.use_pkce);
    }

    #[test]
    fn test_openai_scopes() {
        let provider = get_oauth_provider("openai").unwrap();
        assert!(provider.scopes.contains(&"openai.public".to_string()));
        assert!(provider.use_pkce);
        // 验证 extra_auth_params
        assert!(!provider.extra_auth_params.is_empty());
        assert!(provider.extra_auth_params.iter().any(|(k, v)| k == "audience" && v == "https://api.openai.com/v1"));
    }

    #[test]
    fn test_google_scopes() {
        let provider = get_oauth_provider("google").unwrap();
        assert!(provider.scopes.iter().any(|s| s.contains("googleapis.com")));
        assert!(provider.use_pkce);
        // 验证 extra_auth_params
        assert!(provider.extra_auth_params.iter().any(|(k, _)| k == "access_type"));
        assert!(provider.extra_auth_params.iter().any(|(k, _)| k == "prompt"));
    }

    #[test]
    fn test_azure_openai_scopes() {
        let provider = get_oauth_provider("azure-openai").unwrap();
        assert!(provider.scopes.iter().any(|s| s.contains("cognitiveservices.azure.com")));
        assert!(provider.use_pkce);
        assert!(provider.extra_auth_params.iter().any(|(k, _)| k == "response_mode"));
    }

    #[test]
    fn test_mistral_scopes() {
        let provider = get_oauth_provider("mistral").unwrap();
        assert!(provider.scopes.contains(&"api".to_string()));
        assert!(provider.use_pkce);
    }

    #[test]
    fn test_huggingface_scopes() {
        let provider = get_oauth_provider("huggingface").unwrap();
        assert!(provider.scopes.contains(&"inference-api".to_string()));
        assert!(provider.use_pkce);
    }

    #[test]
    fn test_openrouter_scopes() {
        let provider = get_oauth_provider("openrouter").unwrap();
        // OpenRouter 不使用 scopes
        assert!(provider.scopes.is_empty());
        // OpenRouter 不使用 PKCE
        assert!(!provider.use_pkce);
    }

    #[test]
    fn test_github_copilot_scopes() {
        let provider = get_oauth_provider("github-copilot").unwrap();
        assert!(provider.scopes.contains(&"copilot".to_string()));
        // GitHub Copilot 不使用 PKCE
        assert!(!provider.use_pkce);
    }

    // ========== PKCE 配置测试 ==========

    #[test]
    fn test_pkce_configuration() {
        // 大多数 provider 应该使用 PKCE
        let pkce_providers = vec!["anthropic", "openai", "google", "azure-openai", "mistral", "huggingface"];
        for name in pkce_providers {
            let provider = get_oauth_provider(name).unwrap();
            assert!(
                provider.use_pkce,
                "Provider {} should use PKCE",
                name
            );
        }
        
        // 不使用 PKCE 的 provider
        let non_pkce_providers = vec!["github-copilot", "openrouter"];
        for name in non_pkce_providers {
            let provider = get_oauth_provider(name).unwrap();
            assert!(
                !provider.use_pkce,
                "Provider {} should not use PKCE",
                name
            );
        }
    }

    // ========== Client ID 验证测试 ==========

    #[test]
    fn test_client_id_not_empty() {
        for name in list_oauth_providers() {
            let provider = get_oauth_provider(name).unwrap();
            assert!(
                !provider.client_id.is_empty(),
                "Provider {} should have a non-empty client_id",
                name
            );
        }
    }

    #[test]
    fn test_client_id_format() {
        for name in list_oauth_providers() {
            let provider = get_oauth_provider(name).unwrap();
            // Client ID 不应该包含空格
            assert!(
                !provider.client_id.contains(' '),
                "Provider {} client_id should not contain spaces",
                name
            );
        }
    }

    // ========== Extra Auth Params 测试 ==========

    #[test]
    fn test_extra_auth_params_format() {
        for name in list_oauth_providers() {
            let provider = get_oauth_provider(name).unwrap();
            for (key, _value) in &provider.extra_auth_params {
                assert!(
                    !key.is_empty(),
                    "Provider {} should not have empty param keys",
                    name
                );
                // 值可以为空字符串，但键不应该
                assert!(
                    !key.contains(' '),
                    "Provider {} param key should not contain spaces",
                    name
                );
            }
        }
    }

    #[test]
    fn test_provider_specific_extra_params() {
        // OpenAI
        let openai = get_oauth_provider("openai").unwrap();
        let has_audience = openai.extra_auth_params.iter().any(|(k, _)| k == "audience");
        assert!(has_audience, "OpenAI should have audience param");
        
        // Google
        let google = get_oauth_provider("google").unwrap();
        let has_access_type = google.extra_auth_params.iter().any(|(k, _)| k == "access_type");
        let has_prompt = google.extra_auth_params.iter().any(|(k, _)| k == "prompt");
        assert!(has_access_type, "Google should have access_type param");
        assert!(has_prompt, "Google should have prompt param");
        
        // Azure
        let azure = get_oauth_provider("azure-openai").unwrap();
        let has_response_mode = azure.extra_auth_params.iter().any(|(k, _)| k == "response_mode");
        assert!(has_response_mode, "Azure should have response_mode param");
    }

    // ========== 序列化测试 ==========

    #[test]
    fn test_oauth_config_serialization() {
        let config = OAuthProviderConfig {
            name: "test".to_string(),
            authorize_url: "https://example.com/auth".to_string(),
            token_url: "https://example.com/token".to_string(),
            client_id: "test-client-id".to_string(),
            scopes: vec!["scope1".to_string(), "scope2".to_string()],
            use_pkce: true,
            extra_auth_params: vec![("param1".to_string(), "value1".to_string())],
        };
        
        let json = serde_json::to_string(&config).unwrap();
        
        assert!(json.contains("\"name\":\"test\""));
        assert!(json.contains("\"authorize_url\""));
        assert!(json.contains("\"token_url\""));
        assert!(json.contains("\"client_id\""));
        assert!(json.contains("\"scopes\""));
        assert!(json.contains("\"use_pkce\":true"));
        assert!(json.contains("\"extra_auth_params\""));
    }

    #[test]
    fn test_oauth_config_deserialization() {
        let json = r#"{
            "name": "test",
            "authorize_url": "https://example.com/auth",
            "token_url": "https://example.com/token",
            "client_id": "test-client",
            "scopes": ["read", "write"],
            "use_pkce": true,
            "extra_auth_params": [["key", "value"]]
        }"#;
        
        let config: OAuthProviderConfig = serde_json::from_str(json).unwrap();
        
        assert_eq!(config.name, "test");
        assert_eq!(config.authorize_url, "https://example.com/auth");
        assert_eq!(config.token_url, "https://example.com/token");
        assert_eq!(config.client_id, "test-client");
        assert_eq!(config.scopes, vec!["read", "write"]);
        assert!(config.use_pkce);
        assert_eq!(config.extra_auth_params, vec![("key".to_string(), "value".to_string())]);
    }

    // ========== 边界情况测试 ==========

    #[test]
    fn test_unknown_provider_returns_none() {
        let unknown_names = vec!["", "unknown", "fake", "not-real", "123"];
        for name in unknown_names {
            assert!(
                get_oauth_provider(name).is_none(),
                "Unknown provider '{}' should return None",
                name
            );
        }
    }

    #[test]
    fn test_provider_list_not_empty() {
        let providers = list_oauth_providers();
        assert!(!providers.is_empty(), "Provider list should not be empty");
        assert!(providers.len() >= 8, "Should have at least 8 providers");
    }

    #[test]
    fn test_provider_list_unique() {
        let providers = list_oauth_providers();
        let unique: std::collections::HashSet<_> = providers.iter().collect();
        assert_eq!(
            providers.len(),
            unique.len(),
            "Provider list should not contain duplicates"
        );
    }
}
