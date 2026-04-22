//! E2E tests for `clemp update`. Each test sets up a fake template (v1),
//! runs `run_setup` + writes a lockfile, mutates the template (v2), and calls
//! `run_update` directly. A fake `claude` shell shim is installed on PATH for
//! merge tests so behavior is deterministic.

mod common;

use clemp::{
    compute_manifest, run_setup, run_update, Lockfile, OriginalCommand, SetupArgs, UpdateArgs,
    LOCKFILE_NAME,
};
use common::{install_fake_claude, CwdGuard, EnvVarGuard, PathGuard, Scaffold};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const REPO_URL: &str = "test://example/template.git";
const V1_SHA: &str = "1111111111111111111111111111111111111111";
const V2_SHA: &str = "2222222222222222222222222222222222222222";

// ── Scaffold builders ───────────────────────────────────────────────────

/// Build a baseline template scaffold with one `lang_rules` slot.
fn build_scaffold(ts_rules: &str) -> Scaffold {
    let s = Scaffold::new();
    s.with_gitignore_additions(".claude/\n");
    s.with_template("intro\n{{ lang_rules }}\n", &[("typescript.md", ts_rules)]);
    s.with_settings(r#"{"permissions": {"allow": []}}"#);
    s.with_default_mcps(&[("context7", r#"{"context7": {"url": "c7"}}"#)]);
    s.with_copied("default", &[(".editorconfig", "root = true\n")]);
    s
}

fn ts_args() -> SetupArgs {
    SetupArgs {
        languages: vec!["ts".into()],
        ..Default::default()
    }
}

fn ts_update(force: bool, prune_stale: bool, restore_deleted: bool) -> UpdateArgs {
    UpdateArgs {
        setup: SetupArgs {
            languages: vec!["ts".into()],
            force,
            ..Default::default()
        },
        prune_stale,
        restore_deleted,
    }
}

/// Run setup against `s` and write a lockfile in CWD pinned to `sha`.
/// Returns the resolved language list (for callers that want to recompute).
fn setup_and_lock(s: &Scaffold, sha: &str) -> Vec<String> {
    let args = ts_args();
    let resolved = run_setup(&args, s.path(), Path::new("."), true, false).unwrap();
    let manifest = compute_manifest(&args, &resolved, s.path(), Path::new(".")).unwrap();
    Lockfile {
        template_repo: REPO_URL.into(),
        template_sha: sha.into(),
        original_command: OriginalCommand::from_setup(&args),
        files: manifest,
    }
    .save(Path::new("."))
    .unwrap();
    resolved
}

// ── 1. Clean update propagates template changes ─────────────────────────

#[test]
fn clean_update_changes_clean_files_and_updates_lockfile() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
        // v1 dropped here; its temp dir is cleaned up.
    }

    let pre = fs::read_to_string("CLAUDE.md").unwrap();
    assert!(pre.contains("v1 ts rules"));

    let v2 = build_scaffold("v2 ts rules\n");
    // Drop a brand-new file in the template that didn't exist in v1.
    fs::write(v2.path().join("NEWFILE.md"), "fresh from template\n").unwrap();

    run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let post = fs::read_to_string("CLAUDE.md").unwrap();
    assert!(post.contains("v2 ts rules"), "CLAUDE.md should be updated, got:\n{post}");
    let new_file = fs::read_to_string("NEWFILE.md").unwrap();
    assert_eq!(new_file, "fresh from template\n");

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V2_SHA);
    assert!(lock.files.contains_key("NEWFILE.md"));
}

// ── 2. No-op fast path: same SHA + same args ────────────────────────────

#[test]
fn no_op_fast_path_leaves_project_byte_for_byte() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    let v1 = build_scaffold("v1 ts rules\n");
    setup_and_lock(&v1, V1_SHA);

    let claude_md_before = fs::read("CLAUDE.md").unwrap();
    let lock_before = fs::read(Path::new(LOCKFILE_NAME)).unwrap();

    // Same SHA, same args, no flags — must short-circuit.
    run_update(&ts_update(false, false, false), v1.path(), V1_SHA, REPO_URL).unwrap();

    let claude_md_after = fs::read("CLAUDE.md").unwrap();
    let lock_after = fs::read(Path::new(LOCKFILE_NAME)).unwrap();
    assert_eq!(claude_md_before, claude_md_after);
    assert_eq!(lock_before, lock_after, "lockfile must not be re-written on no-op");
}

// ── 3. --restore-deleted bypasses fast path (fix #1 regression) ─────────

#[test]
fn restore_deleted_works_on_unchanged_template() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    let v1 = build_scaffold("v1 ts rules\n");
    setup_and_lock(&v1, V1_SHA);

    // User deletes a tracked file.
    fs::remove_file(".editorconfig").unwrap();
    assert!(!Path::new(".editorconfig").exists());

    // Same SHA + same args, but --restore-deleted must bypass the fast path
    // and re-copy the missing file.
    run_update(&ts_update(false, false, true), v1.path(), V1_SHA, REPO_URL).unwrap();

    assert!(
        Path::new(".editorconfig").exists(),
        ".editorconfig should be restored after --restore-deleted on same-SHA update"
    );
    assert_eq!(
        fs::read_to_string(".editorconfig").unwrap(),
        "root = true\n"
    );
}

// ── 4. User-modified, template-unchanged → preserved ────────────────────

#[test]
fn user_modified_file_preserved_when_template_did_not_change_it() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    fs::write(".editorconfig", "USER CUSTOMIZED\n").unwrap();

    // v2 changes lang rules but leaves the .editorconfig overlay alone.
    let v2 = build_scaffold("v2 ts rules\n");

    run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    // user file untouched
    assert_eq!(
        fs::read_to_string(".editorconfig").unwrap(),
        "USER CUSTOMIZED\n",
        "user-modified .editorconfig must be preserved"
    );
    // template file updated
    assert!(fs::read_to_string("CLAUDE.md").unwrap().contains("v2 ts rules"));
}

// ── 5. Conflict + claude success → merged content, lockfile advances ────

#[test]
fn conflict_with_fake_claude_success_merges_and_advances_lockfile() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    // User edits CLAUDE.md
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();

    let v2 = build_scaffold("v2 ts rules\n");

    // Install fake claude that writes "MERGED-OUTPUT" into the target file.
    let bindir = workdir.path().join("bin");
    install_fake_claude(&bindir);
    let _path = PathGuard::replace_with(&bindir);
    let mut env = EnvVarGuard::new();
    env.set("FAKE_CLAUDE_EXIT", "0");
    let target = workdir.path().join("CLAUDE.md");
    env.set("FAKE_CLAUDE_TARGET", target.to_str().unwrap());
    env.set("FAKE_CLAUDE_CONTENT", "MERGED-OUTPUT");

    run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let claude_md = fs::read_to_string("CLAUDE.md").unwrap();
    assert_eq!(claude_md, "MERGED-OUTPUT", "fake claude should have rewritten the file");

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V2_SHA);
}

// ── 6. Conflict + no claude on PATH → clean abort, lockfile intact ──────

#[test]
fn conflict_without_claude_aborts_cleanly_and_keeps_lockfile() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();

    let lock_before = fs::read(LOCKFILE_NAME).unwrap();
    let claude_md_before = fs::read("CLAUDE.md").unwrap();

    let v2 = build_scaffold("v2 ts rules\n");

    let _path = PathGuard::system_only(); // no fake claude on PATH

    let err = run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(err.contains("claude"), "should mention claude: {err}");
    assert!(err.contains("--force"), "should mention --force: {err}");

    let lock_after = fs::read(LOCKFILE_NAME).unwrap();
    let claude_md_after = fs::read("CLAUDE.md").unwrap();
    assert_eq!(lock_before, lock_after, "lockfile must remain pinned to v1");
    assert_eq!(claude_md_before, claude_md_after, "user edit must remain");
}

// ── 7. Collision + claude success ───────────────────────────────────────

#[test]
fn collision_with_fake_claude_success_merges() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }

    // User adds a file at a path the template did NOT produce in v1.
    fs::write("USERFILE.md", "user's own content\n").unwrap();

    // v2 introduces a file at the same path.
    let v2 = build_scaffold("v1 ts rules\n");
    fs::write(v2.path().join("USERFILE.md"), "template version\n").unwrap();

    let bindir = workdir.path().join("bin");
    install_fake_claude(&bindir);
    let _path = PathGuard::replace_with(&bindir);
    let mut env = EnvVarGuard::new();
    let target = workdir.path().join("USERFILE.md");
    env.set("FAKE_CLAUDE_TARGET", target.to_str().unwrap());
    env.set("FAKE_CLAUDE_CONTENT", "MERGED-USERFILE");

    run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    assert_eq!(fs::read_to_string("USERFILE.md").unwrap(), "MERGED-USERFILE");

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(lock.files.contains_key("USERFILE.md"));
}

// ── 8. Collision + no claude → clean abort (fix #2 regression) ──────────

#[test]
fn collision_without_claude_aborts_before_any_writes() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }

    // Plant a user file at a path the template will introduce in v2.
    fs::write("USERFILE.md", "user's own content\n").unwrap();

    let v2 = build_scaffold("v2 ts rules\n");
    fs::write(v2.path().join("USERFILE.md"), "template version\n").unwrap();

    let claude_md_before = fs::read("CLAUDE.md").unwrap();
    let lock_before = fs::read(LOCKFILE_NAME).unwrap();
    let userfile_before = fs::read("USERFILE.md").unwrap();

    let _path = PathGuard::system_only();

    let err = run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(err.contains("claude"), "should mention claude: {err}");

    // Critically: before fix #2, clean/new files would have been copied even
    // though we abort on collision. Verify everything is byte-for-byte intact.
    assert_eq!(claude_md_before, fs::read("CLAUDE.md").unwrap());
    assert_eq!(lock_before, fs::read(LOCKFILE_NAME).unwrap());
    assert_eq!(userfile_before, fs::read("USERFILE.md").unwrap());
}

// ── 9. Claude exits non-zero → fail, lockfile intact (fix #3 regression) ─

#[test]
fn claude_failure_aborts_update_without_advancing_lockfile() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();

    let v2 = build_scaffold("v2 ts rules\n");

    let bindir = workdir.path().join("bin");
    install_fake_claude(&bindir);
    let _path = PathGuard::replace_with(&bindir);
    let mut env = EnvVarGuard::new();
    env.set("FAKE_CLAUDE_EXIT", "1");

    let lock_before = fs::read(LOCKFILE_NAME).unwrap();
    let userfile_before = fs::read("CLAUDE.md").unwrap();

    let err = run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("aborting update"),
        "should propagate claude failure: {err}"
    );

    let lock_after = fs::read(LOCKFILE_NAME).unwrap();
    assert_eq!(
        lock_before, lock_after,
        "lockfile must NOT advance when claude exits non-zero"
    );
    // The user's edit must remain since claude (in our shim) didn't write to it
    assert_eq!(userfile_before, fs::read("CLAUDE.md").unwrap());
}

// ── 9b. Claude failure must not have touched clean/new files either ─────

#[test]
fn claude_failure_leaves_clean_and_new_files_untouched() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    // Conflict target
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();

    // v2: different lang rules (so CLAUDE.md is a conflict because both user and
    // template changed it), plus a brand-new file that previously did not exist
    // anywhere. If the apply phase wrote clean/new files BEFORE attempting the
    // merge, NEWFILE.md would land even though the merge later fails. That's
    // the non-atomic state the review flagged.
    let v2 = build_scaffold("v2 ts rules\n");
    fs::write(v2.path().join("NEWFILE.md"), "fresh from template\n").unwrap();

    let bindir = workdir.path().join("bin");
    install_fake_claude(&bindir);
    let _path = PathGuard::replace_with(&bindir);
    let mut env = EnvVarGuard::new();
    env.set("FAKE_CLAUDE_EXIT", "1");

    let lock_before = fs::read(LOCKFILE_NAME).unwrap();
    let mcp_before = fs::read(".mcp.json").unwrap();

    let err = run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(err.contains("aborting update"), "claude failure should bubble: {err}");

    assert!(
        !Path::new("NEWFILE.md").exists(),
        "`new` file must NOT be written when a later merge fails — clean/new writes should come AFTER merges"
    );
    assert_eq!(
        lock_before,
        fs::read(LOCKFILE_NAME).unwrap(),
        "lockfile must stay pinned to v1"
    );
    // .mcp.json was a clean-classified file (template may have re-serialized
    // it). It should ALSO be untouched since clean writes come after merges.
    assert_eq!(
        mcp_before,
        fs::read(".mcp.json").unwrap(),
        "`clean` files must NOT be written when a later merge fails"
    );
}

// ── 10. --force overrides both conflicts and collisions ─────────────────

#[test]
fn force_overrides_conflicts_and_collisions_without_claude() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    // Conflict: user-edited CLAUDE.md
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();
    // Collision: brand-new user file at a future template path
    fs::write("USERFILE.md", "user's own\n").unwrap();

    let v2 = build_scaffold("v2 ts rules\n");
    fs::write(v2.path().join("USERFILE.md"), "template-USERFILE\n").unwrap();

    // No claude on PATH at all — --force must skip the gate entirely.
    let _path = PathGuard::system_only();

    run_update(&ts_update(true, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    // Both files are now the template's version
    assert!(fs::read_to_string("CLAUDE.md").unwrap().contains("v2 ts rules"));
    assert_eq!(fs::read_to_string("USERFILE.md").unwrap(), "template-USERFILE\n");
}

// ── 11/12. Stale prompt n (via EOF) leaves file but drops it from lockfile ─

#[test]
fn stale_no_response_keeps_file_but_drops_from_lockfile() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    // v1 has a "extra-tool.sh" at template root that gets copied to CWD
    {
        let v1 = build_scaffold("v1 ts rules\n");
        fs::write(v1.path().join("extra-tool.sh"), "#!/bin/sh\necho v1\n").unwrap();
        setup_and_lock(&v1, V1_SHA);
    }
    assert!(Path::new("extra-tool.sh").exists());

    // v2 no longer has extra-tool.sh
    let v2 = build_scaffold("v2 ts rules\n");

    // No --prune-stale; stdin is EOF in the test runner so confirm() returns false.
    run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    // File remains on disk (user said "n")
    assert!(
        Path::new("extra-tool.sh").exists(),
        "stale file should remain when user declines deletion"
    );
    // But it's no longer tracked in the new lockfile
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(
        !lock.files.contains_key("extra-tool.sh"),
        "stale file must drop out of the new manifest"
    );
}

// ── 13. --prune-stale deletes without prompting ─────────────────────────

#[test]
fn prune_stale_deletes_without_prompt() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        fs::write(v1.path().join("extra-tool.sh"), "#!/bin/sh\n").unwrap();
        setup_and_lock(&v1, V1_SHA);
    }
    assert!(Path::new("extra-tool.sh").exists());

    let v2 = build_scaffold("v2 ts rules\n");

    run_update(&ts_update(false, true, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    assert!(
        !Path::new("extra-tool.sh").exists(),
        "--prune-stale must delete files no longer in template"
    );
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(!lock.files.contains_key("extra-tool.sh"));
}

// ── 13b. --prune-stale + later Claude failure must not delete stale files ─

#[test]
fn prune_stale_preserves_files_when_later_merge_fails() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    // v1 has a stale-candidate at template root
    {
        let v1 = build_scaffold("v1 ts rules\n");
        fs::write(v1.path().join("extra-tool.sh"), "#!/bin/sh\necho v1\n").unwrap();
        setup_and_lock(&v1, V1_SHA);
    }
    assert!(Path::new("extra-tool.sh").exists());

    // User edits CLAUDE.md so v2 produces a conflict that routes through Claude.
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();

    // v2: changed lang rules (so CLAUDE.md is a conflict) and dropped extra-tool.sh.
    let v2 = build_scaffold("v2 ts rules\n");

    // Fake claude that fails — simulates the merge blowing up mid-update.
    let bindir = workdir.path().join("bin");
    install_fake_claude(&bindir);
    let _path = PathGuard::replace_with(&bindir);
    let mut env = EnvVarGuard::new();
    env.set("FAKE_CLAUDE_EXIT", "1");

    let lock_before = fs::read(LOCKFILE_NAME).unwrap();

    let err = run_update(&ts_update(false, true, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(err.contains("aborting update"), "claude failure should bubble: {err}");

    assert!(
        Path::new("extra-tool.sh").exists(),
        "--prune-stale must NOT delete stale files when a merge later fails — \
         stale handling has to run AFTER merges"
    );
    assert_eq!(
        lock_before,
        fs::read(LOCKFILE_NAME).unwrap(),
        "lockfile must stay pinned to v1 on abort"
    );
}

// ── 14. Missing tracked file without --restore-deleted stays missing ────

#[test]
fn missing_tracked_file_without_restore_stays_missing() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    fs::remove_file(".editorconfig").unwrap();

    let v2 = build_scaffold("v2 ts rules\n");

    run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    assert!(
        !Path::new(".editorconfig").exists(),
        ".editorconfig should remain missing when --restore-deleted not set"
    );
}

// ── 15. Git hook update preserves executable bit ────────────────────────

#[cfg(unix)]
#[test]
fn git_hook_update_preserves_executable_bit() {
    use std::os::unix::fs::PermissionsExt;

    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    fs::create_dir_all(workdir.path().join(".git/hooks")).unwrap();

    {
        let v1 = build_scaffold("v1 ts rules\n");
        fs::create_dir_all(v1.path().join("githooks/default")).unwrap();
        fs::write(
            v1.path().join("githooks/default/pre-commit"),
            "#!/bin/sh\necho v1\n",
        )
        .unwrap();

        let args = SetupArgs {
            languages: vec!["ts".into()],
            ..Default::default()
        };
        let resolved = run_setup(&args, v1.path(), Path::new("."), true, true).unwrap();
        let manifest = compute_manifest(&args, &resolved, v1.path(), Path::new(".")).unwrap();
        Lockfile {
            template_repo: REPO_URL.into(),
            template_sha: V1_SHA.into(),
            original_command: OriginalCommand::from_setup(&args),
            files: manifest,
        }
        .save(Path::new("."))
        .unwrap();
    }

    let mode_before =
        fs::metadata(".git/hooks/pre-commit").unwrap().permissions().mode() & 0o777;
    assert_eq!(mode_before & 0o100, 0o100, "v1 hook should be executable");

    let v2 = build_scaffold("v2 ts rules\n");
    fs::create_dir_all(v2.path().join("githooks/default")).unwrap();
    fs::write(
        v2.path().join("githooks/default/pre-commit"),
        "#!/bin/sh\necho v2\n",
    )
    .unwrap();

    run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let body = fs::read_to_string(".git/hooks/pre-commit").unwrap();
    assert!(body.contains("v2"), "hook should be updated to v2 content");

    let mode_after =
        fs::metadata(".git/hooks/pre-commit").unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode_after & 0o100,
        0o100,
        "executable bit must survive update, got mode={:o}",
        mode_after
    );
}

// ── 16a. file → directory template transition ───────────────────────────

#[test]
fn file_to_directory_transition_handled_with_prune_stale() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    // v1: template ships `tool.sh` as a file.
    {
        let v1 = build_scaffold("v1 ts rules\n");
        fs::write(v1.path().join("tool.sh"), "#!/bin/sh\necho v1\n").unwrap();
        setup_and_lock(&v1, V1_SHA);
    }
    assert!(Path::new("tool.sh").is_file());

    // v2: template ships `tool.sh/inner.sh` (file refactored into a directory).
    let v2 = build_scaffold("v2 ts rules\n");
    fs::create_dir_all(v2.path().join("tool.sh")).unwrap();
    fs::write(v2.path().join("tool.sh/inner.sh"), "inner content\n").unwrap();

    // --prune-stale removes the old file so the new directory tree can land.
    run_update(&ts_update(false, true, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    assert!(
        Path::new("tool.sh").is_dir(),
        "tool.sh should now be a directory"
    );
    assert_eq!(
        fs::read_to_string("tool.sh/inner.sh").unwrap(),
        "inner content\n"
    );
}

// ── 16a-bis. file→dir transition WITHOUT --prune-stale bails in preflight ─

#[test]
fn file_to_directory_transition_without_prune_stale_bails_before_merges() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    // v1: template ships `tool.sh` as a file at root.
    {
        let v1 = build_scaffold("v1 ts rules\n");
        fs::write(v1.path().join("tool.sh"), "#!/bin/sh\necho v1\n").unwrap();
        setup_and_lock(&v1, V1_SHA);
    }
    // User edits CLAUDE.md so v2 produces a conflict that would route through Claude.
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();

    // v2: tool.sh is refactored to tool.sh/inner.sh (blocker-stale) AND lang rules changed.
    let v2 = build_scaffold("v2 ts rules\n");
    fs::create_dir_all(v2.path().join("tool.sh")).unwrap();
    fs::write(v2.path().join("tool.sh/inner.sh"), "inner content\n").unwrap();

    // Install a fake `claude` that would succeed if called. The preflight must
    // bail BEFORE reaching the merge step — otherwise the merge would run and
    // edit CLAUDE.md even though the later clean/new phase is doomed.
    let bindir = workdir.path().join("bin");
    install_fake_claude(&bindir);
    let _path = PathGuard::replace_with(&bindir);

    let lock_before = fs::read(LOCKFILE_NAME).unwrap();
    let claude_md_before = fs::read_to_string("CLAUDE.md").unwrap();

    let err = run_update(&ts_update(false, false, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("--prune-stale") && err.contains("directory"),
        "error should point at --prune-stale and the file→dir case: {err}"
    );

    // No writes happened: stale file, conflict file, lockfile all untouched.
    assert!(Path::new("tool.sh").is_file(), "stale file must remain");
    assert_eq!(
        claude_md_before,
        fs::read_to_string("CLAUDE.md").unwrap(),
        "conflict file must NOT be merged — preflight gate ran first"
    );
    assert_eq!(
        lock_before,
        fs::read(LOCKFILE_NAME).unwrap(),
        "lockfile must stay pinned to v1"
    );
}

// ── 16b. directory → file template transition (shape collision + --force) ─

#[test]
fn directory_to_file_transition_requires_force_and_replaces_dir() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    // v1: template ships `tool.sh/inner.sh` (file inside a dir).
    {
        let v1 = build_scaffold("v1 ts rules\n");
        fs::create_dir_all(v1.path().join("tool.sh")).unwrap();
        fs::write(v1.path().join("tool.sh/inner.sh"), "inner v1\n").unwrap();
        setup_and_lock(&v1, V1_SHA);
    }
    assert!(Path::new("tool.sh").is_dir());

    // v2: tool.sh is now a single file (was a dir).
    let v2 = build_scaffold("v2 ts rules\n");
    fs::write(v2.path().join("tool.sh"), "#!/bin/sh\necho v2\n").unwrap();

    // Without --force the shape collision must abort cleanly.
    let _path = PathGuard::system_only();
    let err = run_update(&ts_update(false, true, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("shape") || err.contains("directories"),
        "should explain shape mismatch: {err}"
    );
    // tool.sh remains a directory
    assert!(Path::new("tool.sh").is_dir(), "dir untouched on abort");

    // With --force, the directory is replaced by the file.
    run_update(&ts_update(true, true, false), v2.path(), V2_SHA, REPO_URL).unwrap();
    assert!(Path::new("tool.sh").is_file(), "tool.sh should now be a file");
    assert!(fs::read_to_string("tool.sh").unwrap().contains("v2"));
}
