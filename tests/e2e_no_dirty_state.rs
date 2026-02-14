//! E2E tests ensuring run_setup never leaves a dirty half-state on conflict.
//! Every test scaffolds a realistic template, plants a conflict in CWD, calls
//! run_setup, and asserts the CWD is byte-for-byte identical to before the call.

mod common;

use clemp::{run_setup, Cli, CLONE_DIR};
use common::{CwdGuard, Scaffold};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────────

/// Recursively snapshot every file under `root` as relative-path → contents.
/// Excludes the clone dir symlink (temp scratch space, not user CWD state).
fn snapshot_dir(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut map = BTreeMap::new();
    collect_entries(root, root, &mut map);
    map
}

fn collect_entries(base: &Path, current: &Path, map: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path.strip_prefix(base).unwrap().to_string_lossy().to_string();
        // Skip the clone dir — it's temp scratch space, not user state
        if rel == CLONE_DIR {
            continue;
        }
        if path.is_dir() {
            map.insert(format!("{}/", rel), vec![]);
            collect_entries(base, &path, map);
        } else {
            map.insert(rel, fs::read(&path).unwrap());
        }
    }
}

/// Build a fully-featured scaffold (template, MCP, hooks, commands, skills, copied).
fn full_scaffold() -> Scaffold {
    let s = Scaffold::new();
    s.with_gitignore_additions(".claude/\n");
    s.with_template(
        "{% if lang %}{{ lang_rules }}{% endif %}\n{{ mcp_rules }}",
        &[("typescript.md", "ts rules")],
    );
    s.with_settings(r#"{"permissions": {"allow": []}}"#);
    s.with_default_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_commands("default", &[("commit.md", "commit cmd")]);
    s.with_skills("default", &[("my-skill.md", "skill content")]);
    s.with_copied("default", &[(".editorconfig", "root = true")]);
    s
}

fn default_cli() -> Cli {
    Cli {
        version: (),
        languages: vec!["ts".into()],
        hooks: vec![],
        mcp: vec![],
        clarg: None,
        force: false,
    }
}

fn setup_workdir(s: &Scaffold) -> (TempDir, CwdGuard) {
    let workdir = TempDir::new().unwrap();
    std::os::unix::fs::symlink(s.path(), workdir.path().join(CLONE_DIR)).unwrap();
    let guard = CwdGuard::new(workdir.path());
    (workdir, guard)
}

/// Assert run_setup errors, the message contains `expected_substr`, and CWD is
/// byte-for-byte identical to `before`.
fn assert_clean_abort(
    cli: &Cli,
    clone_dir: &Path,
    before: &BTreeMap<String, Vec<u8>>,
    workdir: &Path,
    expected_substr: &str,
) {
    let result = run_setup(cli, clone_dir);
    assert!(result.is_err(), "expected run_setup to fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains(expected_substr),
        "error should mention '{}', got: {}",
        expected_substr,
        msg,
    );

    let after = snapshot_dir(workdir);
    assert_eq!(
        before, &after,
        "CWD must be unchanged after failed run_setup"
    );
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn conflict_claude_md_leaves_cwd_clean() {
    let s = full_scaffold();
    let (workdir, _g) = setup_workdir(&s);

    fs::write(workdir.path().join("CLAUDE.md"), "existing").unwrap();
    let before = snapshot_dir(workdir.path());

    assert_clean_abort(&default_cli(), s.path(), &before, workdir.path(), "CLAUDE.md");
}

#[test]
fn conflict_mcp_json_leaves_cwd_clean() {
    let s = full_scaffold();
    let (workdir, _g) = setup_workdir(&s);

    fs::write(workdir.path().join(".mcp.json"), r#"{"old": true}"#).unwrap();
    let before = snapshot_dir(workdir.path());

    assert_clean_abort(&default_cli(), s.path(), &before, workdir.path(), ".mcp.json");
}

#[test]
fn conflict_claude_dir_leaves_cwd_clean() {
    let s = full_scaffold();
    let (workdir, _g) = setup_workdir(&s);

    fs::create_dir_all(workdir.path().join(".claude")).unwrap();
    fs::write(
        workdir.path().join(".claude/settings.local.json"),
        "{}",
    )
    .unwrap();
    let before = snapshot_dir(workdir.path());

    assert_clean_abort(&default_cli(), s.path(), &before, workdir.path(), ".claude");
}

#[test]
fn conflict_copied_default_file_leaves_cwd_clean() {
    let s = full_scaffold();
    let (workdir, _g) = setup_workdir(&s);

    // .editorconfig comes from copied/default — plant a conflict
    fs::write(workdir.path().join(".editorconfig"), "mine").unwrap();
    let before = snapshot_dir(workdir.path());

    assert_clean_abort(
        &default_cli(),
        s.path(),
        &before,
        workdir.path(),
        ".editorconfig",
    );
}

#[test]
fn conflict_copied_lang_file_leaves_cwd_clean() {
    let s = full_scaffold();
    s.with_copied("typescript", &[("tsconfig.json", r#"{"strict": true}"#)]);
    let (workdir, _g) = setup_workdir(&s);

    fs::write(workdir.path().join("tsconfig.json"), "{}").unwrap();
    let before = snapshot_dir(workdir.path());

    assert_clean_abort(
        &default_cli(),
        s.path(),
        &before,
        workdir.path(),
        "tsconfig.json",
    );
}

#[test]
fn multiple_conflicts_all_reported_cwd_clean() {
    let s = full_scaffold();
    let (workdir, _g) = setup_workdir(&s);

    // Plant conflicts for both copy_files and copied/ targets
    fs::write(workdir.path().join("CLAUDE.md"), "x").unwrap();
    fs::write(workdir.path().join(".editorconfig"), "x").unwrap();
    let before = snapshot_dir(workdir.path());

    let result = run_setup(&default_cli(), s.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("CLAUDE.md"), "should mention CLAUDE.md: {msg}");
    assert!(
        msg.contains(".editorconfig"),
        "should mention .editorconfig: {msg}"
    );

    let after = snapshot_dir(workdir.path());
    assert_eq!(before, after, "CWD must be unchanged");
}

#[test]
fn existing_gitignore_untouched_on_conflict() {
    let s = full_scaffold();
    let (workdir, _g) = setup_workdir(&s);

    let original_gitignore = "node_modules/\ndist/\n";
    fs::write(workdir.path().join(".gitignore"), original_gitignore).unwrap();
    fs::write(workdir.path().join("CLAUDE.md"), "conflict").unwrap();
    let before = snapshot_dir(workdir.path());

    assert_clean_abort(&default_cli(), s.path(), &before, workdir.path(), "CLAUDE.md");

    // Extra explicit check: gitignore content unchanged
    assert_eq!(
        fs::read_to_string(workdir.path().join(".gitignore")).unwrap(),
        original_gitignore
    );
}

#[test]
fn no_gitignore_created_on_conflict() {
    let s = full_scaffold();
    let (workdir, _g) = setup_workdir(&s);

    // No .gitignore exists — make sure one isn't created
    fs::write(workdir.path().join("CLAUDE.md"), "conflict").unwrap();
    let before = snapshot_dir(workdir.path());

    assert_clean_abort(&default_cli(), s.path(), &before, workdir.path(), "CLAUDE.md");
    assert!(!workdir.path().join(".gitignore").exists());
}

#[test]
fn second_run_after_success_aborts_cleanly() {
    let s = full_scaffold();
    let (workdir, _g) = setup_workdir(&s);

    // First run succeeds
    run_setup(&default_cli(), s.path()).unwrap();

    // Snapshot CWD after successful first run
    let before = snapshot_dir(workdir.path());

    // Second run must fail (everything exists now) and leave CWD identical
    let result = run_setup(&default_cli(), s.path());
    assert!(result.is_err());

    let after = snapshot_dir(workdir.path());
    assert_eq!(before, after, "CWD must be unchanged after second run");
}

#[test]
fn conflict_with_extra_root_file_leaves_cwd_clean() {
    let s = full_scaffold();
    // Add an extra file at template root that copy_files would pick up
    fs::write(s.path().join("extra-tool.sh"), "#!/bin/sh").unwrap();
    let (workdir, _g) = setup_workdir(&s);

    // Conflict on the extra file
    fs::write(workdir.path().join("extra-tool.sh"), "existing").unwrap();
    let before = snapshot_dir(workdir.path());

    assert_clean_abort(
        &default_cli(),
        s.path(),
        &before,
        workdir.path(),
        "extra-tool.sh",
    );
}

#[test]
fn conflict_does_not_leave_clone_dir_artifacts_in_cwd() {
    let s = full_scaffold();
    let (workdir, _g) = setup_workdir(&s);

    fs::write(workdir.path().join(".mcp.json"), "conflict").unwrap();

    let _ = run_setup(&default_cli(), s.path());

    // Verify no artifacts leaked — these are built inside clone_dir during
    // phase 1 but must never reach CWD when phase 2 aborts.
    assert!(!workdir.path().join("CLAUDE.md").exists());
    assert!(!workdir.path().join(".claude").exists());
    assert!(!workdir.path().join(".editorconfig").exists());
}
