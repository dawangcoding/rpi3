//! 配置模块测试
//!
//! 测试多格式配置加载（YAML/JSON/TOML）和 .env 文件支持
//!
//! 注意：由于 dotenvy 修改的是进程全局环境变量，且测试在同一进程中顺序执行，
//! 测试之间会相互影响。我们使用以下策略：
//! 1. 每个测试使用独立的临时目录
//! 2. 测试顺序很重要：先执行不修改环境变量的测试，再执行修改环境变量的测试
//! 3. 使用 --test-threads=1 确保测试顺序执行

use tempfile::TempDir;
use serial_test::serial;

// 使用 pi_coding_agent 的 config 模块
use pi_coding_agent::config::AppConfig;

/// 设置配置目录环境变量
fn set_config_dir(temp_dir: &TempDir) {
    std::env::set_var("PI_CODING_AGENT_DIR", temp_dir.path().to_str().unwrap());
}

/// 清理环境变量
#[allow(dead_code)]
fn clean_env() {
    std::env::remove_var("PI_CODING_AGENT_DIR");
}

/// 测试 YAML 配置加载（回归测试）
#[test]
#[serial]
fn test_yaml_config_loading() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.yaml");

    let yaml_content = r#"
default_model: "gpt-4o"
default_thinking: "medium"
api_keys:
  openai: "sk-test-yaml"
  anthropic: "sk-ant-test"
sessions_dir: "/tmp/sessions"
"#;

    std::fs::write(&config_path, yaml_content).unwrap();
    set_config_dir(&temp_dir);

    let config = AppConfig::load().unwrap();

    assert_eq!(config.default_model, Some("gpt-4o".to_string()));
    assert_eq!(config.default_thinking, Some("medium".to_string()));
    assert_eq!(
        config.api_keys.get("openai"),
        Some(&"sk-test-yaml".to_string())
    );
    assert_eq!(
        config.api_keys.get("anthropic"),
        Some(&"sk-ant-test".to_string())
    );
    assert_eq!(config.sessions_dir, Some("/tmp/sessions".to_string()));
}

/// 测试 JSON 配置加载
#[test]
#[serial]
fn test_json_config_loading() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.json");

    let json_content = r#"{
  "default_model": "gpt-4o",
  "api_keys": {
    "openai": "sk-test-json"
  }
}"#;

    std::fs::write(&config_path, json_content).unwrap();
    set_config_dir(&temp_dir);

    let config = AppConfig::load().unwrap();

    assert_eq!(config.default_model, Some("gpt-4o".to_string()));
    assert_eq!(
        config.api_keys.get("openai"),
        Some(&"sk-test-json".to_string())
    );
}

/// 测试 TOML 配置加载
#[test]
#[serial]
fn test_toml_config_loading() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    let toml_content = r#"
default_model = "gpt-4o"
default_thinking = "high"

[api_keys]
openai = "sk-test-toml"
anthropic = "sk-ant-toml"
"#;

    std::fs::write(&config_path, toml_content).unwrap();
    set_config_dir(&temp_dir);

    let config = AppConfig::load().unwrap();

    assert_eq!(config.default_model, Some("gpt-4o".to_string()));
    assert_eq!(config.default_thinking, Some("high".to_string()));
    assert_eq!(
        config.api_keys.get("openai"),
        Some(&"sk-test-toml".to_string())
    );
    assert_eq!(
        config.api_keys.get("anthropic"),
        Some(&"sk-ant-toml".to_string())
    );
}

/// 测试配置格式优先级（YAML 优先于 JSON 优先于 TOML）
#[test]
#[serial]
fn test_config_format_priority() {
    let temp_dir = TempDir::new().unwrap();

    // 创建所有三种格式的配置文件
    let yaml_path = temp_dir.path().join("config.yaml");
    let json_path = temp_dir.path().join("config.json");
    let toml_path = temp_dir.path().join("config.toml");

    std::fs::write(&yaml_path, "default_model: \"yaml-model\"\n").unwrap();
    std::fs::write(&json_path, r#"{"default_model": "json-model"}"#).unwrap();
    std::fs::write(&toml_path, r#"default_model = "toml-model""#).unwrap();
    set_config_dir(&temp_dir);

    let config = AppConfig::load().unwrap();

    // YAML 应该优先
    assert_eq!(config.default_model, Some("yaml-model".to_string()));
}

/// 测试 JSON 优先于 TOML（当 YAML 不存在时）
#[test]
#[serial]
fn test_json_priority_over_toml() {
    let temp_dir = TempDir::new().unwrap();

    // 只创建 JSON 和 TOML
    let json_path = temp_dir.path().join("config.json");
    let toml_path = temp_dir.path().join("config.toml");

    std::fs::write(&json_path, r#"{"default_model": "json-model"}"#).unwrap();
    std::fs::write(&toml_path, r#"default_model = "toml-model""#).unwrap();
    set_config_dir(&temp_dir);

    let config = AppConfig::load().unwrap();

    // JSON 应该优先于 TOML
    assert_eq!(config.default_model, Some("json-model".to_string()));
}

/// 测试 .env 文件加载
#[test]
#[serial]
fn test_env_file_loading() {
    // 清理可能的残留环境变量
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("CUSTOM_VAR");
    let temp_dir = TempDir::new().unwrap();
    let env_path = temp_dir.path().join(".env");

    let env_content = r#"
OPENAI_API_KEY=sk-from-env-file
ANTHROPIC_API_KEY=sk-ant-from-env
CUSTOM_VAR=test-value
"#;

    std::fs::write(&env_path, env_content).unwrap();
    set_config_dir(&temp_dir);

    // 加载配置（会触发 .env 加载）
    let _config = AppConfig::load().unwrap();

    // 验证环境变量被加载
    assert_eq!(
        std::env::var("OPENAI_API_KEY").ok(),
        Some("sk-from-env-file".to_string())
    );
    assert_eq!(
        std::env::var("ANTHROPIC_API_KEY").ok(),
        Some("sk-ant-from-env".to_string())
    );
}

/// 测试 .env 文件不覆盖已有环境变量
///
/// 注意：由于 dotenvy 使用 std::env::set_var，且测试在同一个进程中运行，
/// 这个测试必须在 test_env_file_loading 之前运行才能有效测试不覆盖行为。
/// 或者使用独立的进程来测试（使用 std::process::Command）。
/// 这里我们单独测试这个行为。
#[test]
#[serial]
fn test_env_file_no_override() {
    // 注意：由于测试在同一进程中运行，且 dotenvy 设置的环境变量会保留，
    // 我们需要确保 OPENAI_API_KEY 在调用 load() 之前已经存在
    std::env::set_var("OPENAI_API_KEY", "sk-existing");

    let temp_dir = TempDir::new().unwrap();
    let env_path = temp_dir.path().join(".env");

    let env_content = r#"OPENAI_API_KEY=sk-from-env-file"#;
    std::fs::write(&env_path, env_content).unwrap();
    set_config_dir(&temp_dir);

    // 加载配置（会触发 .env 加载）
    let _config = AppConfig::load().unwrap();

    // 验证已有环境变量未被覆盖
    assert_eq!(
        std::env::var("OPENAI_API_KEY").ok(),
        Some("sk-existing".to_string())
    );
}

/// 测试 YAML 格式错误时的错误提示
#[test]
#[serial]
fn test_yaml_parse_error_message() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.yaml");

    // 故意写错误的 YAML
    let invalid_yaml = r#"
default_model: "gpt-4o"
api_keys:
  openai: "test"
  invalid yaml here: [unclosed
"#;

    std::fs::write(&config_path, invalid_yaml).unwrap();
    set_config_dir(&temp_dir);

    let result = AppConfig::load();
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    // 错误信息应该包含文件路径
    assert!(error_msg.contains("config.yaml"), "Error should contain file path");
    assert!(error_msg.contains("YAML"), "Error should indicate YAML format");
}

/// 测试 JSON 格式错误时的错误提示
#[test]
#[serial]
fn test_json_parse_error_message() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.json");

    // 故意写错误的 JSON
    let invalid_json = r#"{"default_model": "gpt-4o", invalid}"#;

    std::fs::write(&config_path, invalid_json).unwrap();
    set_config_dir(&temp_dir);

    let result = AppConfig::load();
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("config.json"), "Error should contain file path");
    assert!(error_msg.contains("JSON"), "Error should indicate JSON format");
}

/// 测试 TOML 格式错误时的错误提示
#[test]
#[serial]
fn test_toml_parse_error_message() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // 故意写错误的 TOML
    let invalid_toml = r#"
default_model = "gpt-4o"
[api_keys
openai = "test"
"#;

    std::fs::write(&config_path, invalid_toml).unwrap();
    set_config_dir(&temp_dir);

    let result = AppConfig::load();
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("config.toml"), "Error should contain file path");
    assert!(error_msg.contains("TOML"), "Error should indicate TOML format");
}

/// 测试空目录返回默认配置
#[test]
#[serial]
fn test_empty_config_dir_returns_default() {
    let temp_dir = TempDir::new().unwrap();

    set_config_dir(&temp_dir);

    let config = AppConfig::load().unwrap();

    // 应该返回默认配置
    assert_eq!(config.default_model, None);
    assert!(config.api_keys.is_empty());
}

/// 测试部分配置文件（缺少可选字段）
#[test]
#[serial]
fn test_partial_config_loading() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.yaml");

    // 只提供部分字段
    let partial_yaml = r#"
default_model: "gpt-4o"
"#;

    std::fs::write(&config_path, partial_yaml).unwrap();
    set_config_dir(&temp_dir);

    let config = AppConfig::load().unwrap();

    assert_eq!(config.default_model, Some("gpt-4o".to_string()));
    // 其他字段应该是默认值
    assert_eq!(config.default_thinking, None);
    assert!(config.api_keys.is_empty());
    assert!(config.custom_models.is_empty());
}
