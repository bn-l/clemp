//! Tests for build_settings (hooks merging + enabledMcpjsonServers).

mod common;

use clemp::build_settings;
use common::Scaffold;
use serde_json::Value;
use std::fs;

#[test]
fn default_hooks_always_applied() {
    let s = Scaffold::new();
    s.with_settings(r#"{"permissions": {"allow": []}}"#);
    s.with_default_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);

    build_settings(&[], &[], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(val["hooks"]["Notification"].as_array().unwrap().len(), 1);
    // permissions preserved
    assert!(val["permissions"]["allow"].is_array());
}

#[test]
fn named_hooks_merged_with_defaults() {
    let s = Scaffold::new();
    s.with_settings("{}");
    s.with_default_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);
    s.with_named_hooks(&[(
        "blocker",
        r#"{"PreToolUse": [{"command": "block"}], "Notification": [{"command": "notify-block"}]}"#,
    )]);

    build_settings(&["blocker".into()], &[], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();

    let notif = val["hooks"]["Notification"].as_array().unwrap();
    assert_eq!(notif.len(), 2); // sound + blocker notification
    let pre = val["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 1);
}

#[test]
fn named_hook_not_found_errors() {
    let s = Scaffold::new();
    s.with_settings("{}");
    fs::create_dir_all(s.path().join("hooks")).unwrap();

    let result = build_settings(&["nonexistent".into()], &[], s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn no_hooks_dir_empty_hooks() {
    let s = Scaffold::new();
    s.with_settings(r#"{"existing": true}"#);

    build_settings(&[], &[], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    assert!(val["existing"].as_bool().unwrap());
    assert!(val["hooks"].as_object().unwrap().is_empty());
}

#[test]
fn enabled_mcp_servers_set() {
    let s = Scaffold::new();
    s.with_settings("{}");

    build_settings(&[], &["context7".into(), "textbelt".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    let servers = val["enabledMcpjsonServers"].as_array().unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0], "context7");
    assert_eq!(servers[1], "textbelt");
}

#[test]
fn settings_created_if_no_base_file() {
    let s = Scaffold::new();
    // No settings.local.json at root
    s.with_default_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);

    build_settings(&[], &["ctx7".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(val["hooks"]["Notification"].as_array().unwrap().len(), 1);
    assert_eq!(val["enabledMcpjsonServers"].as_array().unwrap().len(), 1);
}
