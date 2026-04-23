//! Tests for assemble_mcp_json.

mod common;

use clemp::assemble_mcp_json;
use common::Scaffold;
use std::collections::HashSet;
use std::fs;

fn empty_excluded() -> HashSet<String> {
    HashSet::new()
}

#[test]
fn default_mcps_always_included() {
    let s = Scaffold::new();
    s.with_default_mcps(&[
        ("context7", r#"{"context7": {"type": "http", "url": "c7"}}"#),
        ("textbelt", r#"{"textbelt": {"type": "stdio", "cmd": "tb"}}"#),
    ]);

    let r = assemble_mcp_json(&[], &[], &[], &empty_excluded(), s.path()).unwrap();
    let servers = r.rendered["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 2);
    assert!(servers.contains_key("context7"));
    assert!(servers.contains_key("textbelt"));
    assert_eq!(r.rendered_keys.len(), 2);
    // Default stems are snapshottable.
    assert!(r.snapshottable_stems.contains(&"context7".to_string()));
    assert!(r.snapshottable_stems.contains(&"textbelt".to_string()));
}

#[test]
fn language_mcps_added() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_lang_mcps("svelte", &[("svelte", r#"{"svelte": {"url": "sv"}}"#)]);

    let r = assemble_mcp_json(&["svelte".into()], &[], &[], &empty_excluded(), s.path()).unwrap();
    let servers = r.rendered["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 2);
    assert!(servers.contains_key("context7"));
    assert!(servers.contains_key("svelte"));
    assert!(r.rendered_keys.contains(&"svelte".to_string()));
    // Language-layer stems are NOT snapshottable (they stay dynamic).
    assert!(!r.snapshottable_stems.contains(&"svelte".to_string()));
    assert!(r.snapshottable_stems.contains(&"context7".to_string()));
}

#[test]
fn named_mcps_added() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_named_mcps(&[("maps", r#"{"maps": {"url": "maps"}}"#)]);

    let r = assemble_mcp_json(&[], &["maps".into()], &[], &empty_excluded(), s.path()).unwrap();
    let servers = r.rendered["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 2);
    assert!(servers.contains_key("context7"));
    assert!(servers.contains_key("maps"));
    assert!(r.rendered_keys.contains(&"maps".to_string()));
    // Named opt-ins ARE snapshottable.
    assert!(r.snapshottable_stems.contains(&"maps".to_string()));
}

#[test]
fn named_mcp_not_found_errors() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {}}"#)]);

    let result = assemble_mcp_json(&[], &["doesnt-exist".into()], &[], &empty_excluded(), s.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("doesnt-exist"));
    assert!(msg.contains("not found"));
}

#[test]
fn no_mcp_dir_is_ok() {
    let s = Scaffold::new();
    let r = assemble_mcp_json(&[], &[], &[], &empty_excluded(), s.path()).unwrap();
    assert!(r.rendered["mcpServers"].as_object().unwrap().is_empty());
    assert!(r.rendered_keys.is_empty());
    assert!(r.snapshottable_stems.is_empty());
}

#[test]
fn no_mcp_dir_with_named_mcp_errors() {
    let s = Scaffold::new();
    let result = assemble_mcp_json(&[], &["maps".into()], &[], &empty_excluded(), s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--mcp specified"));
}

#[test]
fn all_three_sources_merged() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_lang_mcps("svelte", &[("svelte", r#"{"svelte": {"url": "sv"}}"#)]);
    s.with_named_mcps(&[("maps", r#"{"maps": {"url": "maps"}}"#)]);

    let r = assemble_mcp_json(
        &["svelte".into()],
        &["maps".into()],
        &[],
        &empty_excluded(),
        s.path(),
    )
    .unwrap();
    let servers = r.rendered["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 3);
    assert_eq!(r.rendered_keys.len(), 3);
}

#[test]
fn missing_lang_dir_silently_skipped() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);

    let r = assemble_mcp_json(&["rust".into()], &[], &[], &empty_excluded(), s.path()).unwrap();
    let servers = r.rendered["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 1);
}

#[test]
fn empty_mcp_json_when_no_servers() {
    let s = Scaffold::new();
    fs::create_dir_all(s.path().join("mcp/default")).unwrap();

    let r = assemble_mcp_json(&[], &[], &[], &empty_excluded(), s.path()).unwrap();
    assert!(r.rendered["mcpServers"].as_object().unwrap().is_empty());
    assert!(r.rendered_keys.is_empty());
}

#[test]
fn sticky_stem_resolves_from_default_layer() {
    // Historical `--mcp foo` where upstream has since relocated the contributor
    // from mcp/foo.json into mcp/default/foo.json. Sticky resolver must find it.
    let s = Scaffold::new();
    s.with_default_mcps(&[("foo", r#"{"foo": {"url": "x"}}"#)]);

    let r = assemble_mcp_json(&[], &[], &["foo".into()], &empty_excluded(), s.path()).unwrap();
    let servers = r.rendered["mcpServers"].as_object().unwrap();
    assert!(servers.contains_key("foo"));
    assert!(r.snapshottable_stems.contains(&"foo".to_string()));
}

#[test]
fn excluded_stems_filter_default_layer() {
    let s = Scaffold::new();
    s.with_default_mcps(&[
        ("context7", r#"{"context7": {"url": "c7"}}"#),
        ("unwanted", r#"{"unwanted": {"url": "u"}}"#),
    ]);
    let mut excluded = HashSet::new();
    excluded.insert("unwanted".into());

    let r = assemble_mcp_json(&[], &[], &[], &excluded, s.path()).unwrap();
    let servers = r.rendered["mcpServers"].as_object().unwrap();
    assert!(servers.contains_key("context7"));
    assert!(!servers.contains_key("unwanted"));
    assert!(!r.snapshottable_stems.contains(&"unwanted".to_string()));
}

#[test]
fn root_override_replaces_default_keys() {
    // mcp/default/foo.json declares key "foo", root mcp/foo.json overrides it.
    let s = Scaffold::new();
    s.with_default_mcps(&[("foo", r#"{"foo": {"url": "from-default"}}"#)]);
    s.with_named_mcps(&[("foo", r#"{"foo": {"url": "from-root"}}"#)]);

    let r = assemble_mcp_json(&[], &["foo".into()], &[], &empty_excluded(), s.path()).unwrap();
    let servers = r.rendered["mcpServers"].as_object().unwrap();
    assert_eq!(servers["foo"]["url"], "from-root");
}
