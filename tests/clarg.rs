//! Tests for clarg integration (setup_clarg + build_settings merging + default auto-detection).

mod common;

use clemp::{build_settings, run_setup, setup_clarg, Cli, CLONE_DIR};
use common::{CwdGuard, Scaffold};
use serde_json::Value;
use std::fs;
use tempfile::TempDir;

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

// ── default.yaml auto-detection via run_setup ───────────────────────

fn scaffold_for_run_setup(s: &Scaffold) {
    s.with_template("{{ lang_rules }}", &[("typescript.md", "ts rules")]);
    s.with_gitignore_additions(".claude/\n");
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
}

#[test]
fn default_yaml_applied_without_clarg_flag() {
    let s = Scaffold::new();
    scaffold_for_run_setup(&s);
    s.with_clarg_configs(&[("default", "internal_access_only: true\n")]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    let cli = Cli {
        version: (),
        languages: vec!["ts".into()],
        hooks: vec![],
        mcp: vec![],
        commands: vec![],
        githooks: vec![],
        clarg: None,
        force: false,
        list: None,
    };

    run_setup(&cli, s.path()).unwrap();

    // clarg-default.yaml copied
    assert!(s.path().join(".claude/clarg-default.yaml").exists());

    // PreToolUse hook registered
    let settings: Value = serde_json::from_str(
        &fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    let pre = settings["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(
        pre[0]["hooks"][0]["command"].as_str().unwrap(),
        "clarg .claude/clarg-default.yaml"
    );
}

#[test]
fn explicit_clarg_flag_overrides_default() {
    let s = Scaffold::new();
    scaffold_for_run_setup(&s);
    s.with_clarg_configs(&[
        ("default", "internal_access_only: true\n"),
        ("strict", "block_access_to:\n  - '.env'\n"),
    ]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    let cli = Cli {
        version: (),
        languages: vec!["ts".into()],
        hooks: vec![],
        mcp: vec![],
        commands: vec![],
        githooks: vec![],
        clarg: Some("strict".into()),
        force: false,
        list: None,
    };

    run_setup(&cli, s.path()).unwrap();

    // Only strict copied, not default
    assert!(s.path().join(".claude/clarg-strict.yaml").exists());
    assert!(!s.path().join(".claude/clarg-default.yaml").exists());

    let settings: Value = serde_json::from_str(
        &fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    let pre = settings["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 1);
    assert!(pre[0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .contains("clarg-strict.yaml"));
}

#[test]
fn no_default_yaml_and_no_flag_skips_clarg() {
    let s = Scaffold::new();
    scaffold_for_run_setup(&s);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    let cli = Cli {
        version: (),
        languages: vec!["ts".into()],
        hooks: vec![],
        mcp: vec![],
        commands: vec![],
        githooks: vec![],
        clarg: None,
        force: false,
        list: None,
    };

    run_setup(&cli, s.path()).unwrap();

    let settings: Value = serde_json::from_str(
        &fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert!(settings["hooks"]["PreToolUse"].is_null());
}
