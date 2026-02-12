//! Tests for clarg integration (setup_clarg + build_settings merging).

mod common;

use clemp::{build_settings, setup_clarg};
use common::Scaffold;
use serde_json::Value;
use std::fs;

#[test]
fn setup_clarg_copies_yaml_and_returns_hook_entry() {
    let s = Scaffold::new();
    s.with_clarg_configs(&[(
        "strict",
        "block_access_to:\n  - '.env'\ncommands_forbidden:\n  - 'rm -rf'\n",
    )]);

    let entry = setup_clarg("strict", s.path()).unwrap();

    // YAML copied to .claude/clarg-strict.yaml
    let dest = s.path().join(".claude/clarg-strict.yaml");
    assert!(dest.exists());
    let content = fs::read_to_string(&dest).unwrap();
    assert!(content.contains("block_access_to"));

    // Hook entry points to the correct path
    let command = entry["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(command, "clarg .claude/clarg-strict.yaml");
    assert_eq!(entry["hooks"][0]["type"].as_str().unwrap(), "command");
}

#[test]
fn setup_clarg_missing_config_errors_with_available_list() {
    let s = Scaffold::new();
    s.with_clarg_configs(&[("strict", "block_access_to: ['.env']")]);

    let result = setup_clarg("nonexistent", s.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not found"));
    assert!(err.contains("strict"));
}

#[test]
fn setup_clarg_no_clarg_dir_errors() {
    let s = Scaffold::new();
    // No clarg/ directory at all

    let result = setup_clarg("anything", s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn clarg_entry_merged_into_pretooluse_hooks() {
    let s = Scaffold::new();
    s.with_settings("{}");
    s.with_clarg_configs(&[(
        "strict",
        "block_access_to:\n  - '.env'\n",
    )]);

    let entry = setup_clarg("strict", s.path()).unwrap();
    build_settings(&[], &[entry], &[], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();

    let pre = val["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 1);
    let command = pre[0]["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(command, "clarg .claude/clarg-strict.yaml");
}

#[test]
fn clarg_merges_with_existing_pretooluse_hooks() {
    let s = Scaffold::new();
    s.with_settings("{}");
    s.with_default_hooks(&[(
        "guard",
        r#"{"PreToolUse": [{"hooks": [{"type": "command", "command": "other-guard"}]}]}"#,
    )]);
    s.with_clarg_configs(&[("strict", "internal_access_only: true")]);

    let entry = setup_clarg("strict", s.path()).unwrap();
    build_settings(&[], &[entry], &[], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();

    let pre = val["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 2);
    // Default hook first, then clarg
    assert_eq!(pre[0]["hooks"][0]["command"].as_str().unwrap(), "other-guard");
    assert!(pre[1]["hooks"][0]["command"].as_str().unwrap().starts_with("clarg"));
}

#[test]
fn clarg_without_clarg_entry_leaves_hooks_unchanged() {
    let s = Scaffold::new();
    s.with_settings("{}");
    s.with_default_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);

    // No clarg entries
    build_settings(&[], &[], &[], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();

    assert!(val["hooks"]["PreToolUse"].is_null());
    assert_eq!(val["hooks"]["Notification"].as_array().unwrap().len(), 1);
}

#[test]
fn clarg_yaml_content_preserved_exactly() {
    let s = Scaffold::new();
    let yaml = "block_access_to:\n  - '.env'\n  - '*.secret'\ncommands_forbidden:\n  - 'rm -rf'\n  - 'sudo'\ninternal_access_only: true\nlog_to: /tmp/clarg.log\n";
    s.with_clarg_configs(&[("full", yaml)]);

    setup_clarg("full", s.path()).unwrap();

    let copied = fs::read_to_string(s.path().join(".claude/clarg-full.yaml")).unwrap();
    assert_eq!(copied, yaml);
}
