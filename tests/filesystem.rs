//! Tests for filesystem operations: copy, gitignore, conflicts, cleanup,
//! copy_conditional_dir, and run_setup integration.

mod common;

use clemp::{
    cleanup, collect_conditional_dir_sources, collect_conflicts, collect_copy_files_sources,
    copy_conditional_dir, copy_dir_recursive, copy_files, run_setup, update_gitignore, SetupArgs,
    CLONE_DIR,
};
use common::{setup_gitignore_test, setup_gitignore_test_with_langs, CwdGuard, Scaffold};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ── collect_conflicts ────────────────────────────────────────────────────

#[test]
fn collect_conflicts_empty_when_clean() {
    let workdir = TempDir::new().unwrap();
    let sources = vec![PathBuf::from("/some/dir/nonexistent_file.txt")];
    assert!(collect_conflicts(&sources, workdir.path()).is_empty());
}

#[test]
fn collect_conflicts_finds_existing() {
    let workdir = TempDir::new().unwrap();
    fs::write(workdir.path().join("conflict.txt"), "exists").unwrap();

    let sources = vec![PathBuf::from("/some/dir/conflict.txt")];
    let conflicts = collect_conflicts(&sources, workdir.path());
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].ends_with("conflict.txt"));
}

#[test]
fn collect_conflicts_multiple() {
    let workdir = TempDir::new().unwrap();
    fs::write(workdir.path().join("a.txt"), "").unwrap();
    fs::write(workdir.path().join("b.txt"), "").unwrap();

    let sources = vec![
        PathBuf::from("/dir/a.txt"),
        PathBuf::from("/dir/b.txt"),
        PathBuf::from("/dir/c.txt"),
    ];
    let conflicts = collect_conflicts(&sources, workdir.path());
    let names: Vec<_> = conflicts.iter().map(|p| p.display().to_string()).collect();
    assert!(names.iter().any(|n| n.contains("a.txt")));
    assert!(names.iter().any(|n| n.contains("b.txt")));
    assert!(!names.iter().any(|n| n.contains("c.txt")));
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

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &[]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains("# Claude related"));
    assert!(content.contains(".claude/"));
    assert!(content.contains(".clinerules"));
}

#[test]
fn gitignore_appends_to_existing() {
    let (workdir, _g) = setup_gitignore_test(Some("node_modules/\n"), ".claude/\nnode_modules/\n");

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &[]).unwrap();

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

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &[]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(!content.contains("# Claude related"));
}

#[test]
fn gitignore_handles_whitespace_in_additions() {
    let (workdir, _g) = setup_gitignore_test(None, "  .claude/  \n  \n.foo\n");

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &[]).unwrap();

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

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &[]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains("node_modules/\n\n# Claude related"));
}

// ── update_gitignore: language-conditional fragments ────────────────────

#[test]
fn gitignore_default_only_applied_when_no_langs() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        Some(".claude/\n"),
        &[("js", "node_modules/\n")],
    );

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &[]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains(".claude/"));
    assert!(!content.contains("node_modules/"));
}

#[test]
fn gitignore_lang_fragment_applied_when_lang_passed() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        Some(".claude/\n"),
        &[("js", "node_modules/\ndist/\n")],
    );

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains(".claude/"));
    assert!(content.contains("node_modules/"));
    assert!(content.contains("dist/"));
}

#[test]
fn gitignore_lang_fragment_skipped_when_lang_not_passed() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        Some(".claude/\n"),
        &[
            ("js", "node_modules/\n"),
            ("python", "__pycache__/\n"),
        ],
    );

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["python".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains(".claude/"));
    assert!(content.contains("__pycache__/"));
    assert!(!content.contains("node_modules/"));
}

#[test]
fn gitignore_multiple_langs_merged_in_order() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        Some(".claude/\n"),
        &[("js", "node_modules/\n"), ("python", "__pycache__/\n")],
    );

    update_gitignore(
        Path::new(CLONE_DIR),
        Path::new("."),
        &["js".into(), "python".into()],
    )
    .unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    let claude_pos = content.find(".claude/").unwrap();
    let js_pos = content.find("node_modules/").unwrap();
    let py_pos = content.find("__pycache__/").unwrap();
    assert!(claude_pos < js_pos);
    assert!(js_pos < py_pos);
}

#[test]
fn gitignore_default_and_lang_dedupe() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        Some(".DS_Store\n.claude/\n"),
        &[("js", ".DS_Store\nnode_modules/\n")],
    );

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert_eq!(content.matches(".DS_Store").count(), 1);
    assert!(content.contains(".claude/"));
    assert!(content.contains("node_modules/"));
}

#[test]
fn gitignore_existing_and_fragment_dedupe_no_header() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        Some("node_modules/\n"),
        None,
        &[("js", "node_modules/\n")],
    );

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(!content.contains("# Claude related"));
    assert_eq!(content.matches("node_modules/").count(), 1);
}

#[test]
fn gitignore_missing_default_file_ok() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        None,
        &[("js", "node_modules/\n")],
    );

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains("node_modules/"));
}

#[test]
fn gitignore_missing_lang_file_is_noop() {
    let (workdir, _g) =
        setup_gitignore_test_with_langs(None, Some(".claude/\n"), &[]);

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains(".claude/"));
    assert!(!content.contains("node_modules/"));
}

#[test]
fn gitignore_empty_dir_is_noop() {
    let workdir = TempDir::new().unwrap();
    let clone = workdir.path().join(CLONE_DIR);
    fs::create_dir_all(clone.join("gitignore-additions")).unwrap();
    let _g = CwdGuard::new(workdir.path());

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    assert!(!workdir.path().join(".gitignore").exists());
}

#[test]
fn gitignore_missing_dir_is_noop() {
    let workdir = TempDir::new().unwrap();
    let clone = workdir.path().join(CLONE_DIR);
    fs::create_dir_all(&clone).unwrap();
    let _g = CwdGuard::new(workdir.path());

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    assert!(!workdir.path().join(".gitignore").exists());
}

#[test]
fn gitignore_whitespace_trimmed_across_fragments() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        Some("  .claude/  \n"),
        &[("js", "   node_modules/   \n")],
    );

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains("\n.claude/\n"));
    assert!(content.contains("\nnode_modules/\n"));
}

#[test]
fn gitignore_blank_lines_skipped_across_fragments() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        Some(".claude/\n\n\n"),
        &[("javascript", "\n\nnode_modules/\n\n")],
    );

    update_gitignore(
        Path::new(CLONE_DIR),
        Path::new("."),
        &["javascript".into()],
    )
    .unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    // The Claude-related block must be exactly default → lang, one per line,
    // with no pass-through blank lines from the fragments.
    assert_eq!(
        content,
        "\n# Claude related\n.claude/\nnode_modules/\n"
    );
}

#[test]
fn gitignore_all_present_no_op() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        Some(".claude/\nnode_modules/\n"),
        Some(".claude/\n"),
        &[("js", "node_modules/\n")],
    );

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(!content.contains("# Claude related"));
    assert_eq!(content, ".claude/\nnode_modules/\n");
}

#[test]
fn gitignore_reads_file_by_exact_lang_string() {
    // `update_gitignore` is a leaf function: it takes the already-canonicalized
    // language list from `resolve_all_languages` and reads files by that exact
    // string — no further normalization. Alias→canonical resolution is covered
    // end-to-end in `e2e_gitignore_alias_resolves_to_canonical_fragment`.
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        None,
        &[("typescript", "*.tsbuildinfo\n")],
    );

    update_gitignore(
        Path::new(CLONE_DIR),
        Path::new("."),
        &["typescript".into()],
    )
    .unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains("*.tsbuildinfo"));
}

#[test]
fn gitignore_order_preserves_user_lang_order() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        None,
        &[("js", "node_modules/\n"), ("python", "__pycache__/\n")],
    );

    update_gitignore(
        Path::new(CLONE_DIR),
        Path::new("."),
        &["python".into(), "js".into()],
    )
    .unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    let py_pos = content.find("__pycache__/").unwrap();
    let js_pos = content.find("node_modules/").unwrap();
    assert!(py_pos < js_pos, "python should appear before js in output");
}

#[test]
fn gitignore_fragment_with_comments_preserved() {
    let (workdir, _g) = setup_gitignore_test_with_langs(
        None,
        None,
        &[("js", "# node\nnode_modules/\n")],
    );

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert_eq!(content.matches("# node").count(), 1);
    assert!(content.contains("node_modules/"));
}

#[test]
fn gitignore_lang_file_with_wrong_extension_ignored() {
    let workdir = TempDir::new().unwrap();
    let additions = workdir.path().join(CLONE_DIR).join("gitignore-additions");
    fs::create_dir_all(&additions).unwrap();
    // Wrong extension — must not be read.
    fs::write(additions.join("js.txt"), "node_modules/\n").unwrap();
    fs::write(additions.join("default.gitignore"), ".claude/\n").unwrap();
    let _g = CwdGuard::new(workdir.path());

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains(".claude/"));
    assert!(!content.contains("node_modules/"));
}

#[test]
fn gitignore_subdirectory_ignored() {
    let workdir = TempDir::new().unwrap();
    let additions = workdir.path().join(CLONE_DIR).join("gitignore-additions");
    fs::create_dir_all(additions.join("subdir")).unwrap();
    fs::write(
        additions.join("subdir").join("js.gitignore"),
        "node_modules/\n",
    )
    .unwrap();
    fs::write(additions.join("default.gitignore"), ".claude/\n").unwrap();
    let _g = CwdGuard::new(workdir.path());

    update_gitignore(Path::new(CLONE_DIR), Path::new("."), &["js".into()]).unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.contains(".claude/"));
    assert!(!content.contains("node_modules/"));
}

// ── copy_files ──────────────────────────────────────────────────────────

#[test]
fn copy_files_excludes_reserved_entries() {
    let s = Scaffold::new();
    // Create all entries that should be excluded
    fs::create_dir_all(s.path().join(".git")).unwrap();
    fs::create_dir_all(s.path().join("commands")).unwrap();
    fs::create_dir_all(s.path().join("skills")).unwrap();
    fs::create_dir_all(s.path().join("copied")).unwrap();
    fs::create_dir_all(s.path().join("hooks")).unwrap();
    fs::create_dir_all(s.path().join("mcp")).unwrap();
    fs::create_dir_all(s.path().join("githooks")).unwrap();
    fs::create_dir_all(s.path().join("claude-md")).unwrap();
    fs::write(s.path().join("README.md"), "readme").unwrap();
    fs::write(s.path().join(".gitignore"), "ignore").unwrap();
    fs::create_dir_all(s.path().join("gitignore-additions")).unwrap();
    fs::write(
        s.path().join("gitignore-additions/default.gitignore"),
        "additions",
    )
    .unwrap();
    fs::write(s.path().join("CLAUDE.md.jinja"), "template").unwrap();
    fs::write(s.path().join("settings.local.json"), "settings").unwrap();

    // Files that SHOULD be copied
    fs::write(s.path().join("CLAUDE.md"), "claude").unwrap();
    fs::write(s.path().join(".mcp.json"), "mcp").unwrap();
    fs::create_dir_all(s.path().join(".claude")).unwrap();
    fs::write(s.path().join(".claude/settings.local.json"), "s").unwrap();

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    copy_files(s.path(), Path::new(".")).unwrap();

    // Should be copied
    assert!(workdir.path().join("CLAUDE.md").exists());
    assert!(workdir.path().join(".mcp.json").exists());
    assert!(workdir.path().join(".claude/settings.local.json").exists());

    // Should NOT be copied
    assert!(!workdir.path().join("README.md").exists());
    assert!(!workdir.path().join(".gitignore").exists());
    assert!(!workdir.path().join("gitignore-additions").exists());
    assert!(!workdir.path().join("CLAUDE.md.jinja").exists());
    assert!(!workdir.path().join("settings.local.json").exists());
    assert!(!workdir.path().join(".git").exists());
    assert!(!workdir.path().join("commands").exists());
    assert!(!workdir.path().join("skills").exists());
    assert!(!workdir.path().join("copied").exists());
    assert!(!workdir.path().join("hooks").exists());
    assert!(!workdir.path().join("mcp").exists());
    assert!(!workdir.path().join("githooks").exists());
    assert!(!workdir.path().join("claude-md").exists());
}

// ── copy_conditional_dir ────────────────────────────────────────────────

#[test]
fn copy_conditional_default_only() {
    let s = Scaffold::new();
    s.with_commands("default", &[("commit.md", "commit cmd"), ("isolated.md", "iso cmd")]);

    let dest = TempDir::new().unwrap();
    copy_conditional_dir(&s.path().join("commands"), &[], dest.path()).unwrap();

    assert_eq!(
        fs::read_to_string(dest.path().join("commit.md")).unwrap(),
        "commit cmd"
    );
    assert_eq!(
        fs::read_to_string(dest.path().join("isolated.md")).unwrap(),
        "iso cmd"
    );
}

#[test]
fn copy_conditional_with_language() {
    let s = Scaffold::new();
    s.with_commands("default", &[("commit.md", "commit cmd")]);
    s.with_commands("svelte", &[("svelte.md", "svelte cmd")]);

    let dest = TempDir::new().unwrap();
    copy_conditional_dir(
        &s.path().join("commands"),
        &["svelte".into()],
        dest.path(),
    )
    .unwrap();

    assert!(dest.path().join("commit.md").exists());
    assert!(dest.path().join("svelte.md").exists());
}

#[test]
fn copy_conditional_language_overrides_default() {
    let s = Scaffold::new();
    s.with_commands("default", &[("shared.md", "default version")]);
    s.with_commands("svelte", &[("shared.md", "svelte version")]);

    let dest = TempDir::new().unwrap();
    copy_conditional_dir(
        &s.path().join("commands"),
        &["svelte".into()],
        dest.path(),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(dest.path().join("shared.md")).unwrap(),
        "svelte version"
    );
}

#[test]
fn copy_conditional_missing_lang_dir_ok() {
    let s = Scaffold::new();
    s.with_commands("default", &[("commit.md", "cmd")]);

    let dest = TempDir::new().unwrap();
    copy_conditional_dir(
        &s.path().join("commands"),
        &["rust".into()],
        dest.path(),
    )
    .unwrap();

    assert!(dest.path().join("commit.md").exists());
}

#[test]
fn copy_conditional_missing_source_dir_ok() {
    let s = Scaffold::new();
    let dest = TempDir::new().unwrap();

    copy_conditional_dir(&s.path().join("commands"), &[], dest.path()).unwrap();
}

#[test]
fn copy_conditional_skills_recursive() {
    let s = Scaffold::new();
    let skill_dir = s.path().join("skills/default/my-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "skill content").unwrap();
    fs::write(skill_dir.join("README.md"), "readme").unwrap();

    let dest = TempDir::new().unwrap();
    copy_conditional_dir(&s.path().join("skills"), &[], dest.path()).unwrap();

    assert!(dest.path().join("my-skill/SKILL.md").exists());
    assert!(dest.path().join("my-skill/README.md").exists());
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

// ── run_setup integration ───────────────────────────────────────────────

#[test]
fn run_setup_full_flow() {
    let s = Scaffold::new();

    // Template
    s.with_gitignore_additions(".claude/\n");
    s.with_template(
        "{%- if lang %}Languages: {% for l in lang %}{{ l }} {% endfor %}\n{{ lang_rules }}{%- endif %}\n{{ mcp_rules }}",
        &[("typescript.md", "strict mode")],
    );

    // Settings + hooks
    s.with_settings(r#"{"permissions": {"allow": []}}"#);
    s.with_default_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);

    // MCP
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);

    // Commands
    s.with_commands("default", &[("commit.md", "commit command")]);

    // Skills
    let skill_dir = s.path().join("skills/default/my-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "skill").unwrap();

    // Copied
    s.with_copied("default", &[(".editorconfig", "config")]);

    // An extra file at clone root that should be copied
    fs::write(s.path().join("somefile.txt"), "hello").unwrap();

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    // Symlink CLONE_DIR in workdir to the scaffold (for update_gitignore)
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    let args = SetupArgs {
        languages: vec!["ts".into()],
        hooks: vec![],
        mcp: vec![],
        commands: vec![],
        githooks: vec![],
        clarg: None,
        force: false,
    };

    run_setup(&args, s.path(), Path::new("."), true, false).unwrap();

    // .gitignore created with additions
    let gitignore = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".claude/"));

    // CLAUDE.md rendered in clone dir then copied to workdir
    let claude = fs::read_to_string(workdir.path().join("CLAUDE.md")).unwrap();
    assert!(claude.contains("typescript"));
    assert!(claude.contains("<typescript-rules>"));

    // Settings has hooks + enabledMcpjsonServers
    let settings: Value = serde_json::from_str(
        &fs::read_to_string(workdir.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert!(settings["hooks"]["Notification"].is_array());
    assert!(settings["permissions"]["allow"].is_array());
    let enabled = settings["enabledMcpjsonServers"].as_array().unwrap();
    assert!(enabled.contains(&Value::String("context7".into())));

    // .mcp.json assembled
    let mcp: Value = serde_json::from_str(
        &fs::read_to_string(workdir.path().join(".mcp.json")).unwrap(),
    )
    .unwrap();
    assert!(mcp["mcpServers"]["context7"].is_object());

    // Commands copied
    assert!(workdir.path().join(".claude/commands/commit.md").exists());

    // Skills copied
    assert!(workdir.path().join(".claude/skills/my-skill/SKILL.md").exists());

    // Copied files
    assert!(workdir.path().join(".editorconfig").exists());

    // Extra file copied
    assert!(workdir.path().join("somefile.txt").exists());
}

// ── copy_conditional_dir (multiple languages) ───────────────────────────

#[test]
fn copy_conditional_multiple_languages() {
    let s = Scaffold::new();
    s.with_commands("default", &[("base.md", "base cmd")]);
    s.with_commands("svelte", &[("sv.md", "svelte cmd")]);
    s.with_commands("typescript", &[("ts.md", "ts cmd")]);

    let dest = TempDir::new().unwrap();
    copy_conditional_dir(
        &s.path().join("commands"),
        &["svelte".into(), "typescript".into()],
        dest.path(),
    )
    .unwrap();

    assert!(dest.path().join("base.md").exists());
    assert!(dest.path().join("sv.md").exists());
    assert!(dest.path().join("ts.md").exists());
}

#[test]
fn copy_conditional_copied_with_lang_files() {
    let s = Scaffold::new();
    s.with_copied("default", &[("editor.cfg", "editor config")]);
    s.with_copied("swift", &[("swift-lint.yml", "swift lint config")]);

    let dest = TempDir::new().unwrap();
    copy_conditional_dir(
        &s.path().join("copied"),
        &["swift".into()],
        dest.path(),
    )
    .unwrap();

    assert!(dest.path().join("editor.cfg").exists());
    assert!(dest.path().join("swift-lint.yml").exists());
}

// ── run_setup pre-flight conflict check (no dirty state) ────────────────

#[test]
fn run_setup_aborts_cleanly_on_copy_files_conflict() {
    let s = Scaffold::new();
    s.with_gitignore_additions(".claude/\n");
    s.with_template("base", &[]);
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_copied("default", &[(".editorconfig", "config")]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    // Pre-existing CLAUDE.md in CWD — will conflict with copy_files
    fs::write(workdir.path().join("CLAUDE.md"), "existing").unwrap();

    let args = SetupArgs {
        languages: vec![],
        hooks: vec![],
        mcp: vec![],
        commands: vec![],
        githooks: vec![],
        clarg: None,
        force: false,
    };

    let result = run_setup(&args, s.path(), Path::new("."), true, false);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("CLAUDE.md"));

    // CWD must be untouched — no .gitignore created, no .mcp.json, no .editorconfig
    assert!(!workdir.path().join(".gitignore").exists());
    assert!(!workdir.path().join(".mcp.json").exists());
    assert!(!workdir.path().join(".editorconfig").exists());
    assert!(!workdir.path().join(".claude").exists());
    // Original file still intact
    assert_eq!(
        fs::read_to_string(workdir.path().join("CLAUDE.md")).unwrap(),
        "existing"
    );
}

#[test]
fn run_setup_aborts_cleanly_on_copied_dir_conflict() {
    let s = Scaffold::new();
    s.with_gitignore_additions(".claude/\n");
    s.with_template("base", &[]);
    s.with_copied("default", &[("AGENTS.md", "agents content")]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    // Pre-existing AGENTS.md — will conflict with copy_conditional_dir(copied/)
    fs::write(workdir.path().join("AGENTS.md"), "existing").unwrap();

    let args = SetupArgs {
        languages: vec![],
        hooks: vec![],
        mcp: vec![],
        commands: vec![],
        githooks: vec![],
        clarg: None,
        force: false,
    };

    let result = run_setup(&args, s.path(), Path::new("."), true, false);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("AGENTS.md"));

    // CWD must be untouched — no .gitignore, no CLAUDE.md, no .mcp.json
    assert!(!workdir.path().join(".gitignore").exists());
    assert!(!workdir.path().join("CLAUDE.md").exists());
    assert!(!workdir.path().join(".mcp.json").exists());
    assert!(!workdir.path().join(".claude").exists());
    // Original file still intact
    assert_eq!(
        fs::read_to_string(workdir.path().join("AGENTS.md")).unwrap(),
        "existing"
    );
}

// ── collect_copy_files_sources / collect_conditional_dir_sources ─────────

#[test]
fn collect_copy_files_sources_excludes_reserved() {
    let s = Scaffold::new();
    fs::create_dir_all(s.path().join(".git")).unwrap();
    fs::create_dir_all(s.path().join("commands")).unwrap();
    fs::write(s.path().join("README.md"), "readme").unwrap();
    fs::write(s.path().join("CLAUDE.md"), "claude").unwrap();
    fs::write(s.path().join(".mcp.json"), "mcp").unwrap();

    let sources = collect_copy_files_sources(s.path()).unwrap();
    let names: Vec<_> = sources
        .iter()
        .filter_map(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .collect();

    assert!(names.contains(&"CLAUDE.md".to_string()));
    assert!(names.contains(&".mcp.json".to_string()));
    assert!(!names.contains(&".git".to_string()));
    assert!(!names.contains(&"README.md".to_string()));
    assert!(!names.contains(&"commands".to_string()));
}

#[test]
fn collect_conditional_dir_sources_gathers_entries() {
    let s = Scaffold::new();
    s.with_copied("default", &[("a.txt", "a"), ("b.txt", "b")]);
    s.with_copied("swift", &[("c.txt", "c")]);

    let sources =
        collect_conditional_dir_sources(&s.path().join("copied"), &["swift".into()]);
    let names: Vec<_> = sources
        .iter()
        .filter_map(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .collect();

    assert!(names.contains(&"a.txt".to_string()));
    assert!(names.contains(&"b.txt".to_string()));
    assert!(names.contains(&"c.txt".to_string()));
}

#[test]
fn collect_conditional_dir_sources_missing_dir_returns_empty() {
    let s = Scaffold::new();
    let sources =
        collect_conditional_dir_sources(&s.path().join("nonexistent"), &["swift".into()]);
    assert!(sources.is_empty());
}

// ── collect_conflicts (dedup) ────────────────────────────────────────────

#[test]
fn collect_conflicts_dedup() {
    let workdir = TempDir::new().unwrap();
    fs::write(workdir.path().join("shared.md"), "exists").unwrap();

    let sources = vec![
        PathBuf::from("/dir1/shared.md"),
        PathBuf::from("/dir2/shared.md"),
    ];
    let conflicts = collect_conflicts(&sources, workdir.path());
    assert_eq!(conflicts.len(), 1, "should dedup to one entry");
}

// ── run_setup integration (named hooks + MCPs) ──────────────────────────

#[test]
fn run_setup_with_named_hooks_and_mcps() {
    let s = Scaffold::new();

    s.with_gitignore_additions(".claude/\n");
    s.with_template(
        "{{ lang_rules }}\n{{ mcp_rules }}",
        &[("typescript.md", "ts rules")],
    );
    s.with_settings(r#"{"permissions": {"allow": []}}"#);

    // Default + named hook
    s.with_default_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);
    s.with_named_hooks(&[("blocker", r#"{"PreToolUse": [{"command": "block-tool"}]}"#)]);

    // Default + named MCP
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_named_mcps(&[("maps", r#"{"maps": {"url": "maps"}}"#)]);

    // Commands
    s.with_commands("default", &[("commit.md", "commit cmd")]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    let args = SetupArgs {
        languages: vec!["ts".into()],
        hooks: vec!["blocker".into()],
        mcp: vec!["maps".into()],
        commands: vec![],
        githooks: vec![],
        clarg: None,
        force: false,
    };

    run_setup(&args, s.path(), Path::new("."), true, false).unwrap();

    // Settings has both default + blocker hooks merged
    let settings: Value = serde_json::from_str(
        &fs::read_to_string(workdir.path().join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert!(settings["hooks"]["Notification"].is_array());
    assert!(settings["hooks"]["PreToolUse"].is_array());

    // enabledMcpjsonServers includes both context7 and maps
    let enabled = settings["enabledMcpjsonServers"].as_array().unwrap();
    assert!(enabled.contains(&Value::String("context7".into())));
    assert!(enabled.contains(&Value::String("maps".into())));

    // .mcp.json has both servers
    let mcp: Value = serde_json::from_str(
        &fs::read_to_string(workdir.path().join(".mcp.json")).unwrap(),
    )
    .unwrap();
    assert!(mcp["mcpServers"]["context7"].is_object());
    assert!(mcp["mcpServers"]["maps"].is_object());
}

// ── run_setup integration (language conditionals) ───────────────────────

#[test]
fn run_setup_with_lang_conditionals() {
    let s = Scaffold::new();

    s.with_gitignore_additions(".claude/\n");
    s.with_template("base", &[]);

    // Commands: default + svelte
    s.with_commands("default", &[("base.md", "base cmd")]);
    s.with_commands("svelte", &[("sv.md", "svelte cmd")]);

    // Skills: svelte
    let skill_dir = s.path().join("skills/svelte/sv-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "svelte skill").unwrap();

    // Copied: svelte
    s.with_copied("svelte", &[("sv-lint.yml", "svelte lint config")]);

    // MCP: default + svelte
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_lang_mcps("svelte", &[("svelte-mcp", r#"{"svelte-mcp": {"url": "sv"}}"#)]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    let args = SetupArgs {
        languages: vec!["svelte".into()],
        hooks: vec![],
        mcp: vec![],
        commands: vec![],
        githooks: vec![],
        clarg: None,
        force: false,
    };

    run_setup(&args, s.path(), Path::new("."), true, false).unwrap();

    // Commands
    assert!(workdir.path().join(".claude/commands/base.md").exists());
    assert!(workdir.path().join(".claude/commands/sv.md").exists());

    // Skills
    assert!(workdir
        .path()
        .join(".claude/skills/sv-skill/SKILL.md")
        .exists());

    // Copied to workdir root
    assert!(workdir.path().join("sv-lint.yml").exists());

    // .mcp.json has both servers
    let mcp: Value = serde_json::from_str(
        &fs::read_to_string(workdir.path().join(".mcp.json")).unwrap(),
    )
    .unwrap();
    assert!(mcp["mcpServers"]["context7"].is_object());
    assert!(mcp["mcpServers"]["svelte-mcp"].is_object());
}

// ── run_setup integration (multiple languages) ──────────────────────────

#[test]
fn run_setup_multiple_languages() {
    let s = Scaffold::new();

    s.with_gitignore_additions(".claude/\n");
    s.with_template(
        "Languages: {% for l in lang %}{{ l }} {% endfor %}\n{{ lang_rules }}",
        &[("svelte.md", "svelte rules"), ("typescript.md", "ts rules")],
    );

    // Commands: default + svelte + typescript
    s.with_commands("default", &[("base.md", "base cmd")]);
    s.with_commands("svelte", &[("sv.md", "svelte cmd")]);
    s.with_commands("typescript", &[("ts.md", "ts cmd")]);

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();

    let args = SetupArgs {
        languages: vec!["ts".into(), "svelte".into()],
        hooks: vec![],
        mcp: vec![],
        commands: vec![],
        githooks: vec![],
        clarg: None,
        force: false,
    };

    run_setup(&args, s.path(), Path::new("."), true, false).unwrap();

    // CLAUDE.md contains both language rules
    let claude = fs::read_to_string(workdir.path().join("CLAUDE.md")).unwrap();
    assert!(claude.contains("<typescript-rules>"));
    assert!(claude.contains("<svelte-rules>"));

    // Commands dir has all three files
    assert!(workdir.path().join(".claude/commands/base.md").exists());
    assert!(workdir.path().join(".claude/commands/sv.md").exists());
    assert!(workdir.path().join(".claude/commands/ts.md").exists());
}
