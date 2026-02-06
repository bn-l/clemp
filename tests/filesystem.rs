//! Tests for filesystem operations: copy, gitignore, conflicts, cleanup, and integration.

mod common;

use clemp::{
    check_no_conflicts, cleanup, copy_dir_recursive, copy_files, copy_lang_files, run_setup,
    update_gitignore, Cli, CLONE_DIR,
};
use common::{setup_gitignore_test, CwdGuard, Scaffold};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ── check_no_conflicts ──────────────────────────────────────────────────

#[test]
fn check_no_conflicts_passes_when_clean() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    let sources = vec![PathBuf::from("/some/dir/nonexistent_file.txt")];
    check_no_conflicts(&sources).unwrap();
}

#[test]
fn check_no_conflicts_errors_on_existing() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    fs::write(workdir.path().join("conflict.txt"), "exists").unwrap();

    let sources = vec![PathBuf::from("/some/dir/conflict.txt")];
    let result = check_no_conflicts(&sources);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("conflict.txt"));
}

#[test]
fn check_no_conflicts_multiple_conflicts() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    fs::write(workdir.path().join("a.txt"), "").unwrap();
    fs::write(workdir.path().join("b.txt"), "").unwrap();

    let sources = vec![
        PathBuf::from("/dir/a.txt"),
        PathBuf::from("/dir/b.txt"),
        PathBuf::from("/dir/c.txt"),
    ];
    let result = check_no_conflicts(&sources);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("a.txt"));
    assert!(msg.contains("b.txt"));
    assert!(!msg.contains("c.txt"));
}

// ── copy_dir_recursive ──────────────────────────────────────────────────

#[test]
fn copy_dir_recursive_nested() {
    let src_dir = TempDir::new().unwrap();
    let dest_dir = TempDir::new().unwrap();
    let dest = dest_dir.path().join("out");

    fs::create_dir_all(src_dir.path().join("sub")).unwrap();
    fs::write(src_dir.path().join("a.txt"), "A").unwrap();
    fs::write(src_dir.path().join("sub/b.txt"), "B").unwrap();

    copy_dir_recursive(src_dir.path(), &dest).unwrap();

    assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "A");
    assert_eq!(fs::read_to_string(dest.join("sub/b.txt")).unwrap(), "B");
}

#[test]
fn copy_dir_recursive_empty_dir() {
    let src_dir = TempDir::new().unwrap();
    let dest_dir = TempDir::new().unwrap();
    let dest = dest_dir.path().join("out");

    copy_dir_recursive(src_dir.path(), &dest).unwrap();
    assert!(dest.exists());
    assert!(dest.is_dir());
}

// ── update_gitignore ────────────────────────────────────────────────────

#[test]
fn gitignore_creates_new_file() {
    let (workdir, _g) = setup_gitignore_test(None, ".claude/\n.clinerules\n");

    update_gitignore().unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains("# Claude related"));
    assert!(content.contains(".claude/"));
    assert!(content.contains(".clinerules"));
}

#[test]
fn gitignore_appends_to_existing() {
    let (workdir, _g) = setup_gitignore_test(Some("node_modules/\n"), ".claude/\nnode_modules/\n");

    update_gitignore().unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.starts_with("node_modules/\n"));
    assert!(content.contains(".claude/"));
    assert_eq!(
        content.matches("node_modules/").count(),
        1,
        "should not duplicate"
    );
}

#[test]
fn gitignore_no_op_when_all_present() {
    let (workdir, _g) =
        setup_gitignore_test(Some(".claude/\n.clinerules\n"), ".claude/\n.clinerules\n");

    update_gitignore().unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(!content.contains("# Claude related"));
}

#[test]
fn gitignore_handles_whitespace_in_additions() {
    let (workdir, _g) = setup_gitignore_test(None, "  .claude/  \n  \n.foo\n");

    update_gitignore().unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains(".claude/"));
    assert!(content.contains(".foo"));
}

#[test]
fn gitignore_appends_newline_if_missing() {
    let (workdir, _g) = setup_gitignore_test(
        Some("node_modules/"), // no trailing newline
        ".claude/\n",
    );

    update_gitignore().unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains("node_modules/\n\n# Claude related"));
}

// ── copy_files ──────────────────────────────────────────────────────────

#[test]
fn copy_files_excludes_reserved_entries() {
    let s = Scaffold::new();
    fs::create_dir_all(s.path().join(".git")).unwrap();
    fs::create_dir_all(s.path().join("hooks-template")).unwrap();
    fs::create_dir_all(s.path().join("rules-templates")).unwrap();
    fs::create_dir_all(s.path().join("lang-files")).unwrap();
    fs::write(s.path().join("README.md"), "readme").unwrap();
    fs::write(s.path().join(".gitignore"), "ignore").unwrap();
    fs::write(s.path().join("gitignore-additions"), "additions").unwrap();

    fs::write(s.path().join("CLAUDE.md"), "claude").unwrap();
    fs::write(s.path().join(".mcp.json"), "mcp").unwrap();

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    copy_files(s.path()).unwrap();

    assert!(workdir.path().join("CLAUDE.md").exists());
    assert!(workdir.path().join(".mcp.json").exists());
    assert!(!workdir.path().join("README.md").exists());
    assert!(!workdir.path().join(".gitignore").exists());
    assert!(!workdir.path().join("gitignore-additions").exists());
    assert!(!workdir.path().join(".git").exists());
    assert!(!workdir.path().join("hooks-template").exists());
    assert!(!workdir.path().join("rules-templates").exists());
    assert!(!workdir.path().join("lang-files").exists());
}

#[test]
fn copy_files_errors_on_conflict() {
    let s = Scaffold::new();
    fs::write(s.path().join("CLAUDE.md"), "claude").unwrap();

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    fs::write(workdir.path().join("CLAUDE.md"), "existing").unwrap();

    let result = copy_files(s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("CLAUDE.md"));
}

// ── copy_lang_files ─────────────────────────────────────────────────────

#[test]
fn copy_lang_files_copies_matching_language() {
    let s = Scaffold::new();
    s.with_lang_files("typescript", &[("tsconfig.json", r#"{"strict": true}"#)]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    copy_lang_files(&["typescript".into()], s.path()).unwrap();

    assert_eq!(
        fs::read_to_string(workdir.path().join("tsconfig.json")).unwrap(),
        r#"{"strict": true}"#
    );
}

#[test]
fn copy_lang_files_ignores_missing_lang_dir() {
    let s = Scaffold::new();
    fs::create_dir_all(s.path().join("lang-files")).unwrap();

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    copy_lang_files(&["rust".into()], s.path()).unwrap();
}

#[test]
fn copy_lang_files_no_lang_files_dir_is_ok() {
    let s = Scaffold::new();

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    copy_lang_files(&["rust".into()], s.path()).unwrap();
}

#[test]
fn copy_lang_files_conflict_errors() {
    let s = Scaffold::new();
    s.with_lang_files("rust", &[("Cargo.toml", "content")]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    fs::write(workdir.path().join("Cargo.toml"), "existing").unwrap();

    let result = copy_lang_files(&["rust".into()], s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Cargo.toml"));
}

#[test]
fn copy_lang_files_multiple_languages() {
    let s = Scaffold::new();
    s.with_lang_files("typescript", &[("tsconfig.json", "ts")]);
    s.with_lang_files("rust", &[("rustfmt.toml", "rs")]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    copy_lang_files(&["typescript".into(), "rust".into()], s.path()).unwrap();

    assert!(workdir.path().join("tsconfig.json").exists());
    assert!(workdir.path().join("rustfmt.toml").exists());
}

// ── cleanup ─────────────────────────────────────────────────────────────

#[test]
fn cleanup_removes_directory() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("to_remove");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("file.txt"), "data").unwrap();

    cleanup(&dir).unwrap();
    assert!(!dir.exists());
}

#[test]
fn cleanup_nonexistent_errors() {
    let result = cleanup(Path::new("/tmp/clemp_test_nonexistent_dir_12345"));
    assert!(result.is_err());
}

// ── Error cleanup: gitignore removal ────────────────────────────────────

#[test]
fn error_cleanup_removes_new_gitignore() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    assert!(!Path::new(".gitignore").exists());
    let gitignore_existed = Path::new(".gitignore").exists();

    fs::write(".gitignore", "# Claude related\n.claude/\n").unwrap();

    // Simulate the error cleanup logic from main()
    if !gitignore_existed {
        let _ = fs::remove_file(".gitignore");
    }

    assert!(!Path::new(".gitignore").exists());
}

#[test]
fn error_cleanup_preserves_existing_gitignore() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    fs::write(".gitignore", "node_modules/\n").unwrap();
    let gitignore_existed = Path::new(".gitignore").exists();

    if !gitignore_existed {
        let _ = fs::remove_file(".gitignore");
    }

    assert!(Path::new(".gitignore").exists());
    assert_eq!(fs::read_to_string(".gitignore").unwrap(), "node_modules/\n");
}

// ── run_setup integration (end-to-end minus git clone) ──────────────────

#[test]
fn run_setup_full_flow() {
    let s = Scaffold::new();
    s.with_gitignore_additions(".claude/\n");
    s.with_rules_template(
        "Hello {{ languages | join(', ') }}\n{{ language_rules }}",
        &[("typescript-rules.md", "strict mode")],
    );
    s.with_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);
    s.with_settings("{}");
    s.with_mcp(r#"{"mcpServers": {"context7": {"cmd": "c7"}}}"#);
    fs::write(s.path().join("somefile.txt"), "hello").unwrap();

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    // Symlink CLONE_DIR in workdir to the scaffold (for update_gitignore)
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    let cli = Cli {
        version: (),
        languages: vec!["ts".into()],
        hooks: vec!["sound".into()],
        mcp: vec!["context7".into()],
    };

    run_setup(&cli, s.path(), &s.path().join("rules-templates")).unwrap();

    // .gitignore created with additions
    let gitignore = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".claude/"));

    // CLAUDE.md rendered in clone dir
    let claude = fs::read_to_string(s.path().join("CLAUDE.md")).unwrap();
    assert!(claude.contains("Hello typescript"));
    assert!(claude.contains("<typescript-rules>"));

    // Settings has hooks
    let settings: Value = serde_json::from_str(
        &fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert!(settings["hooks"]["Notification"].is_array());

    // MCP filtered
    let mcp: Value = serde_json::from_str(
        &fs::read_to_string(s.path().join(".mcp.json")).unwrap(),
    )
    .unwrap();
    assert!(mcp["mcpServers"]["context7"].is_object());

    // somefile.txt copied to workdir
    assert!(workdir.path().join("somefile.txt").exists());
}
