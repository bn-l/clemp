//! Tests for --list: listing available template files by category.

mod common;

use clemp::{list_available, list_category};
use common::Scaffold;
use std::fs;

// ── list_category unit tests ────────────────────────────────────────────

#[test]
fn list_mcp_root_json_only() {
    let s = Scaffold::new();
    s.with_default_mcps(&[("context7", r#"{"context7": {}}"#)]);
    s.with_named_mcps(&[
        ("maps", r#"{"maps": {}}"#),
        ("github", r#"{"github": {}}"#),
    ]);

    let names = list_category("mcp", s.path()).unwrap();
    assert_eq!(names, vec!["github", "maps"]);
}

#[test]
fn list_hooks_root_json_only() {
    let s = Scaffold::new();
    s.with_default_hooks(&[("sound", r#"{"Notification": []}"#)]);
    s.with_named_hooks(&[("blocker", r#"{"PreToolUse": []}"#)]);

    let names = list_category("hooks", s.path()).unwrap();
    assert_eq!(names, vec!["blocker"]);
}

#[test]
fn list_commands_root_md_only() {
    let s = Scaffold::new();
    s.with_commands("default", &[("commit.md", "cmd")]);
    s.with_commands("svelte", &[("svelte.md", "cmd")]);
    s.with_named_commands(&[("review-pr", "review pr cmd")]);

    let names = list_category("commands", s.path()).unwrap();
    assert_eq!(names, vec!["review-pr"]);
}

#[test]
fn list_githooks_root_files_only() {
    let s = Scaffold::new();
    let dir = s.path().join("githooks");
    fs::create_dir_all(dir.join("default")).unwrap();
    fs::write(dir.join("default/pre-commit"), "#!/bin/sh").unwrap();
    fs::write(dir.join("commit-msg"), "#!/bin/sh").unwrap();
    fs::write(dir.join("pre-push"), "#!/bin/sh").unwrap();

    let names = list_category("githooks", s.path()).unwrap();
    assert_eq!(names, vec!["commit-msg", "pre-push"]);
}

#[test]
fn list_clarg_yaml() {
    let s = Scaffold::new();
    s.with_clarg_configs(&[("default", "x: y"), ("strict", "a: b")]);

    let names = list_category("clarg", s.path()).unwrap();
    assert_eq!(names, vec!["default", "strict"]);
}

#[test]
fn list_languages() {
    let s = Scaffold::new();
    s.with_template(
        "base",
        &[("typescript.md", "ts rules"), ("python.md", "py rules")],
    );

    let names = list_category("languages", s.path()).unwrap();
    assert_eq!(names, vec!["python", "typescript"]);
}

#[test]
fn list_empty_dir_returns_empty() {
    let s = Scaffold::new();
    // Dir exists with only a subdirectory, no root-level named files
    s.with_default_mcps(&[("context7", r#"{"context7": {}}"#)]);

    let names = list_category("mcp", s.path()).unwrap();
    assert!(names.is_empty());
}

#[test]
fn list_missing_dir_returns_empty() {
    let s = Scaffold::new();

    let names = list_category("mcp", s.path()).unwrap();
    assert!(names.is_empty());
}

#[test]
fn list_invalid_category_errors() {
    let s = Scaffold::new();
    let result = list_category("bogus", s.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("Unknown category"), "{msg}");
    assert!(msg.contains("mcp"), "should list valid categories: {msg}");
    assert!(
        msg.contains("hooks"),
        "should list valid categories: {msg}"
    );
}

// ── list_available integration tests ────────────────────────────────────

#[test]
fn list_all_prints_headers() {
    let s = Scaffold::new();
    s.with_named_mcps(&[("maps", r#"{"maps": {}}"#)]);
    s.with_named_hooks(&[("blocker", r#"{"PreToolUse": []}"#)]);
    s.with_named_commands(&[("review-pr", "cmd")]);

    let output = list_available("all", s.path()).unwrap();
    assert!(
        output.contains("mcp:\n"),
        "should have mcp header:\n{output}"
    );
    assert!(
        output.contains("  maps\n"),
        "should list maps indented:\n{output}"
    );
    assert!(
        output.contains("hooks:\n"),
        "should have hooks header:\n{output}"
    );
    assert!(
        output.contains("  blocker\n"),
        "should list blocker indented:\n{output}"
    );
    assert!(
        output.contains("commands:\n"),
        "should have commands header:\n{output}"
    );
    assert!(
        output.contains("  review-pr\n"),
        "should list review-pr indented:\n{output}"
    );
}

#[test]
fn list_single_category_no_header() {
    let s = Scaffold::new();
    s.with_named_mcps(&[
        ("maps", r#"{"maps": {}}"#),
        ("github", r#"{"github": {}}"#),
    ]);

    let output = list_available("mcp", s.path()).unwrap();
    assert_eq!(output, "github\nmaps\n");
}

#[test]
fn list_all_skips_empty_categories() {
    let s = Scaffold::new();
    s.with_named_mcps(&[("maps", r#"{"maps": {}}"#)]);

    let output = list_available("all", s.path()).unwrap();
    assert!(output.contains("mcp:"), "should show mcp:\n{output}");
    assert!(
        !output.contains("hooks:"),
        "should skip empty hooks:\n{output}"
    );
    assert!(
        !output.contains("commands:"),
        "should skip empty commands:\n{output}"
    );
    assert!(
        !output.contains("githooks:"),
        "should skip empty githooks:\n{output}"
    );
    assert!(
        !output.contains("clarg:"),
        "should skip empty clarg:\n{output}"
    );
    assert!(
        !output.contains("languages:"),
        "should skip empty languages:\n{output}"
    );
}

#[test]
fn list_invalid_category_via_list_available() {
    let s = Scaffold::new();
    let result = list_available("bogus", s.path());
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unknown category"));
}
