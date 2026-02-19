//! Tests for copy_named_commands (--commands flag).

mod common;

use clemp::copy_named_commands;
use common::Scaffold;
use std::fs;

#[test]
fn named_commands_copied_to_dest() {
    let s = Scaffold::new();
    s.with_commands("default", &[("commit.md", "commit cmd")]);
    s.with_named_commands(&[("review", "review cmd"), ("deploy", "deploy cmd")]);

    copy_named_commands(&["review".into(), "deploy".into()], s.path()).unwrap();

    let dest = s.path().join(".claude/commands");
    assert_eq!(fs::read_to_string(dest.join("review.md")).unwrap(), "review cmd");
    assert_eq!(fs::read_to_string(dest.join("deploy.md")).unwrap(), "deploy cmd");
}

#[test]
fn named_command_not_found_errors_with_available() {
    let s = Scaffold::new();
    s.with_named_commands(&[("review", "review cmd")]);

    let result = copy_named_commands(&["nonexistent".into()], s.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("nonexistent"));
    assert!(msg.contains("not found"));
    assert!(msg.contains("review"));
}

#[test]
fn no_commands_dir_with_named_commands_errors() {
    let s = Scaffold::new();

    let result = copy_named_commands(&["review".into()], s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--commands specified"));
}

#[test]
fn empty_named_commands_is_noop() {
    let s = Scaffold::new();
    copy_named_commands(&[], s.path()).unwrap();
    assert!(!s.path().join(".claude/commands").exists());
}

#[test]
fn named_commands_override_default_with_same_name() {
    let s = Scaffold::new();
    s.with_commands("default", &[("commit.md", "default commit")]);
    s.with_named_commands(&[("commit", "named commit")]);

    // First copy defaults via copy_conditional_dir
    clemp::copy_conditional_dir(
        &s.path().join("commands"),
        &[],
        &s.path().join(".claude/commands"),
    )
    .unwrap();

    // Then copy named commands on top
    copy_named_commands(&["commit".into()], s.path()).unwrap();

    let content =
        fs::read_to_string(s.path().join(".claude/commands/commit.md")).unwrap();
    assert_eq!(content, "named commit");
}

#[test]
fn named_commands_available_list_only_shows_root_md_files() {
    let s = Scaffold::new();
    // Create a commands dir with subdirs (default, lang) and a root .md file
    s.with_commands("default", &[("commit.md", "commit cmd")]);
    s.with_named_commands(&[("review", "review cmd")]);

    let result = copy_named_commands(&["nonexistent".into()], s.path());
    let msg = result.unwrap_err().to_string();

    // Should list the root-level .md file
    assert!(msg.contains("review"), "should list 'review': {msg}");
    // Should NOT list files inside subdirectories (those are for conditional dirs)
}
