//! Tests for update_settings_with_hooks.

mod common;

use clemp::update_settings_with_hooks;
use common::Scaffold;
use serde_json::Value;
use std::fs;

#[test]
fn hooks_merge_into_settings() {
    let s = Scaffold::new();
    s.with_settings(r#"{"permissions": {"allow": []}}"#);
    s.with_hooks(&[
        ("sound", r#"{"Notification": [{"command": "play-sound"}]}"#),
        (
            "lint",
            r#"{"PreToolUse": [{"command": "lint"}], "Notification": [{"command": "notify-lint"}]}"#,
        ),
    ]);

    update_settings_with_hooks(&["sound".into(), "lint".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();

    // permissions preserved
    assert!(val["permissions"]["allow"].is_array());

    // hooks merged
    let notif = val["hooks"]["Notification"].as_array().unwrap();
    assert_eq!(notif.len(), 2);
    let pre = val["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 1);
}

#[test]
fn hooks_missing_file_errors() {
    let s = Scaffold::new();
    s.with_settings("{}");
    s.with_hooks(&[]);

    let result = update_settings_with_hooks(&["nonexistent".into()], s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn hooks_creates_settings_if_absent() {
    let s = Scaffold::new();
    fs::create_dir_all(s.path().join(".claude")).unwrap();
    s.with_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);

    update_settings_with_hooks(&["sound".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(val["hooks"]["Notification"].as_array().unwrap().len(), 1);
}

#[test]
fn hooks_empty_list_writes_empty_hooks() {
    let s = Scaffold::new();
    s.with_settings(r#"{"existing": true}"#);
    s.with_hooks(&[]);

    update_settings_with_hooks(&[], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    assert!(val["existing"].as_bool().unwrap());
    assert!(val["hooks"].as_object().unwrap().is_empty());
}
