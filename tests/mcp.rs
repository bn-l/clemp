//! Tests for assemble_mcp_json.

mod common;

use clemp::assemble_mcp_json;
use common::Scaffold;
use std::fs;

#[test]
fn default_mcps_always_included() {
    let s = Scaffold::new();
    s.with_default_mcps(&[
        ("context7", r#"{"context7": {"type": "http", "url": "c7"}}"#),
        ("textbelt", r#"{"textbelt": {"type": "stdio", "cmd": "tb"}}"#),
    ]);

    let (json, names) = assemble_mcp_json(&[], &[], s.path()).unwrap();
    let servers = json["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 2);
    assert!(servers.contains_key("context7"));
    assert!(servers.contains_key("textbelt"));
    assert_eq!(names.len(), 2);
}

#[test]
fn language_mcps_added() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_lang_mcps("svelte", &[("svelte", r#"{"svelte": {"url": "sv"}}"#)]);

    let (json, names) = assemble_mcp_json(&["svelte".into()], &[], s.path()).unwrap();
    let servers = json["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 2);
    assert!(servers.contains_key("context7"));
    assert!(servers.contains_key("svelte"));
    assert!(names.contains(&"svelte".to_string()));
}

#[test]
fn named_mcps_added() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_named_mcps(&[("maps", r#"{"maps": {"url": "maps"}}"#)]);

    let (json, names) = assemble_mcp_json(&[], &["maps".into()], s.path()).unwrap();
    let servers = json["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 2);
    assert!(servers.contains_key("context7"));
    assert!(servers.contains_key("maps"));
    assert!(names.contains(&"maps".to_string()));
}

#[test]
fn named_mcp_not_found_errors() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {}}"#)]);

    let result = assemble_mcp_json(&[], &["doesnt-exist".into()], s.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("doesnt-exist"));
    assert!(msg.contains("not found"));
}

#[test]
fn no_mcp_dir_is_ok() {
    let s = Scaffold::new();
    let (json, names) = assemble_mcp_json(&[], &[], s.path()).unwrap();
    assert!(json["mcpServers"].as_object().unwrap().is_empty());
    assert!(names.is_empty());
}

#[test]
fn no_mcp_dir_with_named_mcp_errors() {
    let s = Scaffold::new();
    let result = assemble_mcp_json(&[], &["maps".into()], s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--mcp specified"));
}

#[test]
fn all_three_sources_merged() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_lang_mcps("svelte", &[("svelte", r#"{"svelte": {"url": "sv"}}"#)]);
    s.with_named_mcps(&[("maps", r#"{"maps": {"url": "maps"}}"#)]);

    let (json, names) =
        assemble_mcp_json(&["svelte".into()], &["maps".into()], s.path()).unwrap();
    let servers = json["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 3);
    assert_eq!(names.len(), 3);
}

#[test]
fn missing_lang_dir_silently_skipped() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);

    let (json, _) = assemble_mcp_json(&["rust".into()], &[], s.path()).unwrap();
    let servers = json["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 1);
}

#[test]
fn empty_mcp_json_when_no_servers() {
    let s = Scaffold::new();
    fs::create_dir_all(s.path().join("mcp/default")).unwrap();

    let (json, names) = assemble_mcp_json(&[], &[], s.path()).unwrap();
    assert!(json["mcpServers"].as_object().unwrap().is_empty());
    assert!(names.is_empty());
}
