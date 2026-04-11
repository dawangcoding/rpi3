//! CLI 端到端测试

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_help() {
    Command::cargo_bin("pi")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn test_version() {
    Command::cargo_bin("pi")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("0.2.0"));
}

#[test]
fn test_list_models() {
    Command::cargo_bin("pi")
        .unwrap()
        .arg("--list-models")
        .assert()
        .success();
}

// 注意：需要 API key 的测试可以用条件编译跳过
#[test]
#[ignore] // 需要 API key
fn test_print_mode() {
    Command::cargo_bin("pi")
        .unwrap()
        .args(["--mode", "print", "-p", "say hello"])
        .assert()
        .success();
}
