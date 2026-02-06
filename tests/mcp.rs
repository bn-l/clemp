//! Tests for filter_mcp_json.

mod common;

use clemp::filter_mcp_json;
use common::Scaffold;
use serde_json::Value;
use std::fs;

#[test]
fn filter_mcp_keeps_requested_servers() {
    let s = Scaffold::new();
    s.with_mcp(
        r#"{"mcpServers": {"context7": {"url": "c7"}, "other": {"url": "x"}, "third": {"url": "y"}}}"#,
    );

    filter_mcp_json(&["context7".into(), "third".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".mcp.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    let servers = val["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 2);
    assert!(servers.contains_key("context7"));
    assert!(servers.contains_key("third"));
    assert!(!servers.contains_key("other"));
}

#[test]
fn filter_mcp_removes_file_when_empty() {
    let s = Scaffold::new();
    s.with_mcp(r#"{"mcpServers": {}}"#);

    filter_mcp_json(&[], s.path()).unwrap();
    assert!(!s.path().join(".mcp.json").exists());
}

#[test]
fn filter_mcp_unknown_server_errors() {
    let s = Scaffold::new();
    s.with_mcp(r#"{"mcpServers": {"context7": {}}}"#);

    let result = filter_mcp_json(&["doesnt-exist".into()], s.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("doesnt-exist"));
    assert!(msg.contains("not found"));
}

#[test]
fn filter_mcp_no_file_is_ok() {
    let s = Scaffold::new();
    filter_mcp_json(&["anything".into()], s.path()).unwrap();
}

#[test]
fn filter_mcp_single_server() {
    let s = Scaffold::new();
    s.with_mcp(r#"{"mcpServers": {"context7": {"cmd": "c7"}, "other": {"cmd": "x"}}}"#);

    filter_mcp_json(&["context7".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".mcp.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    let servers = val["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 1);
    assert!(servers.contains_key("context7"));
}
