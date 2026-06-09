//! E2E tests for `clemp update`. Each test sets up a fake template (v1),
//! runs `run_setup` + writes a lockfile, mutates the template (v2), and calls
//! `run_update` directly. A fake `claude` shell shim is installed on PATH for
//! merge tests so behavior is deterministic.

mod common;

use clemp::{
    compute_manifest, run_setup, run_update, Lockfile, OriginalCommand, RenderInputs, Resolved,
    SetupArgs, UpdateArgs, LOCKFILE_NAME,
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

fn ts_update(force: bool, merge: bool, prune_stale: bool, restore_deleted: bool) -> UpdateArgs {
    UpdateArgs {
        setup: SetupArgs {
            languages: vec!["ts".into()],
            force,
            ..Default::default()
        },
        prune_stale,
        restore_deleted,
        merge,
        only: false,
    }
}

/// Run setup against `s` and write a lockfile in CWD pinned to `sha`.
/// Returns the resolved language list (for callers that want to recompute).
fn setup_and_lock(s: &Scaffold, sha: &str) -> Vec<String> {
    let args = ts_args();
    let outcome = run_setup(
        &RenderInputs { setup: &args, sticky_mcp: &[], sticky_hooks: &[] },
        s.path(),
        Path::new("."),
        true,
        false,
    )
    .unwrap();
    let manifest = compute_manifest(&args, &outcome.resolved_languages, s.path(), Path::new(".")).unwrap();
    Lockfile {
        template_repo: REPO_URL.into(),
        template_sha: sha.into(),
        original_command: OriginalCommand::from_setup(&args),
        resolved: Some(Resolved {
            mcp: outcome.mcp_snapshottable_stems,
            hooks: outcome.hooks_snapshottable_stems,
        }),
        files: manifest,
    }
    .save(Path::new("."))
    .unwrap();
    outcome.resolved_languages
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

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

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
    run_update(&ts_update(false, false, false, false), v1.path(), V1_SHA, REPO_URL).unwrap();

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
    run_update(&ts_update(false, false, false, true), v1.path(), V1_SHA, REPO_URL).unwrap();

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

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    // user file untouched
    assert_eq!(
        fs::read_to_string(".editorconfig").unwrap(),
        "USER CUSTOMIZED\n",
        "user-modified .editorconfig must be preserved"
    );
    // template file updated
    assert!(fs::read_to_string("CLAUDE.md").unwrap().contains("v2 ts rules"));
}

// ── 5. Keep-own lifecycle: conflict + collision + clean + chained updates ─

const V3_SHA: &str = "3333333333333333333333333333333333333333";
const V4_SHA: &str = "4444444444444444444444444444444444444444";

#[test]
fn keep_own_lifecycle() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    // No claude on PATH for the entire test — keep-own must never invoke it.
    let _path = PathGuard::system_only();

    // ── v1 setup ──
    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }

    // User edits → conflict target.
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();
    // User plants file at a path v2 will introduce → collision target.
    fs::write("USERFILE.md", "user's USERFILE\n").unwrap();
    // .editorconfig left untouched → clean target (v2 will change it).

    // ── Cycle 1: default update to v2 ──
    let v2 = build_scaffold("v2 ts rules\n");
    // Change .editorconfig so it's Clean (template changed, user didn't).
    fs::write(
        v2.path().join("copied/default/.editorconfig"),
        "v2 editor config\n",
    )
    .unwrap();
    // Template introduces USERFILE.md → collision with user's file.
    fs::write(v2.path().join("USERFILE.md"), "template USERFILE\n").unwrap();

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    // Conflict → kept.
    assert_eq!(
        fs::read_to_string("CLAUDE.md").unwrap(),
        "USER EDIT\n",
        "conflict must keep user's version by default"
    );
    // Collision → kept.
    assert_eq!(
        fs::read_to_string("USERFILE.md").unwrap(),
        "user's USERFILE\n",
        "collision must keep user's version by default"
    );
    // Clean → updated.
    assert_eq!(
        fs::read_to_string(".editorconfig").unwrap(),
        "v2 editor config\n",
        "clean file must be updated even when conflicts are kept"
    );
    // Lockfile advances.
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V2_SHA);
    assert!(lock.files.contains_key("USERFILE.md"));

    // ── Cycle 2: same template content, different SHA (bypass fast path) ──
    // Template content identical to v2, but V3_SHA forces classification to run.
    // Kept-own files should classify as Skipped (template unchanged, user modified).
    run_update(&ts_update(false, false, false, false), v2.path(), V3_SHA, REPO_URL).unwrap();

    assert_eq!(
        fs::read_to_string("CLAUDE.md").unwrap(),
        "USER EDIT\n",
        "cycle 2: kept file must classify as Skipped"
    );
    assert_eq!(
        fs::read_to_string("USERFILE.md").unwrap(),
        "user's USERFILE\n",
        "cycle 2: collision-kept file must classify as Skipped"
    );
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V3_SHA);

    // ── Cycle 3: template changes again → re-Conflict, still kept ──
    let v4 = build_scaffold("v4 ts rules\n");
    fs::write(
        v4.path().join("copied/default/.editorconfig"),
        "v2 editor config\n",
    )
    .unwrap();
    fs::write(v4.path().join("USERFILE.md"), "template USERFILE v4\n").unwrap();

    run_update(&ts_update(false, false, false, false), v4.path(), V4_SHA, REPO_URL).unwrap();

    assert_eq!(
        fs::read_to_string("CLAUDE.md").unwrap(),
        "USER EDIT\n",
        "cycle 3: re-conflict must still keep user's version"
    );
    assert_eq!(
        fs::read_to_string("USERFILE.md").unwrap(),
        "user's USERFILE\n",
        "cycle 3: re-conflict on collision path must keep user's version"
    );
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V4_SHA);
}

// ── 6. --merge lifecycle: failure aborts atomically, success merges ─────

#[test]
fn merge_flag_lifecycle() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    // Conflict target.
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();

    // v2: changed template + brand-new file (tests merge-before-write ordering).
    let v2 = build_scaffold("v2 ts rules\n");
    fs::write(v2.path().join("NEWFILE.md"), "fresh from template\n").unwrap();

    let bindir = workdir.path().join("bin");
    install_fake_claude(&bindir);
    let _path = PathGuard::replace_with(&bindir);
    let mut env = EnvVarGuard::new();

    // ── Attempt 1: --merge with Claude failure ──
    env.set("FAKE_CLAUDE_EXIT", "1");
    // Don't set FAKE_CLAUDE_TARGET so the shim doesn't write anything.

    let lock_before = fs::read(LOCKFILE_NAME).unwrap();
    let mcp_before = fs::read(".mcp.json").unwrap();

    let err = run_update(&ts_update(false, true, false, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(err.contains("aborting update"), "claude failure should bubble: {err}");

    // User file untouched.
    assert_eq!(fs::read_to_string("CLAUDE.md").unwrap(), "USER EDIT\n");
    // New/clean files must NOT be written (merges run before clean/new writes).
    assert!(
        !Path::new("NEWFILE.md").exists(),
        "new file must NOT appear when merge fails — proves merge-before-write ordering"
    );
    assert_eq!(
        mcp_before,
        fs::read(".mcp.json").unwrap(),
        "clean files must NOT be written when merge fails"
    );
    // Lockfile pinned to v1.
    assert_eq!(lock_before, fs::read(LOCKFILE_NAME).unwrap());

    // ── Attempt 2: --merge with Claude success ──
    unsafe { std::env::set_var("FAKE_CLAUDE_EXIT", "0"); }
    let target = workdir.path().join("CLAUDE.md");
    unsafe { std::env::set_var("FAKE_CLAUDE_TARGET", target.to_str().unwrap()); }
    unsafe { std::env::set_var("FAKE_CLAUDE_CONTENT", "MERGED-OUTPUT"); }

    run_update(&ts_update(false, true, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    // Merged by Claude.
    assert_eq!(
        fs::read_to_string("CLAUDE.md").unwrap(),
        "MERGED-OUTPUT",
        "successful --merge must apply Claude's output"
    );
    // New file now written (merges succeeded, clean/new phase ran).
    assert_eq!(
        fs::read_to_string("NEWFILE.md").unwrap(),
        "fresh from template\n"
    );
    // Lockfile advanced.
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V2_SHA);
}

// ── 7. --merge without claude on PATH → clean abort ────────────────────

#[test]
fn merge_flag_without_claude_aborts() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    fs::write("CLAUDE.md", "USER EDIT\n").unwrap();

    let v2 = build_scaffold("v2 ts rules\n");
    let _path = PathGuard::system_only();

    let lock_before = fs::read(LOCKFILE_NAME).unwrap();

    let err = run_update(&ts_update(false, true, false, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(err.contains("claude"), "should mention claude: {err}");
    assert!(err.contains("--merge"), "should mention --merge: {err}");

    // Nothing changed.
    assert_eq!(fs::read_to_string("CLAUDE.md").unwrap(), "USER EDIT\n");
    assert_eq!(lock_before, fs::read(LOCKFILE_NAME).unwrap());
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

    run_update(&ts_update(true, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

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
    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

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

    run_update(&ts_update(false, false, true, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    assert!(
        !Path::new("extra-tool.sh").exists(),
        "--prune-stale must delete files no longer in template"
    );
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(!lock.files.contains_key("extra-tool.sh"));
}

// ── 13b. --prune-stale + --merge + Claude failure must not delete stale files ─

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

    // --merge is required to trigger the Claude merge path; without it,
    // keep-own (the default) would skip the merge and the test couldn't
    // exercise the merge-before-stale ordering.
    let err = run_update(&ts_update(false, true, true, false), v2.path(), V2_SHA, REPO_URL)
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

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

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
        let outcome = run_setup(
            &RenderInputs { setup: &args, sticky_mcp: &[], sticky_hooks: &[] },
            v1.path(),
            Path::new("."),
            true,
            true,
        )
        .unwrap();
        let manifest =
            compute_manifest(&args, &outcome.resolved_languages, v1.path(), Path::new("."))
                .unwrap();
        Lockfile {
            template_repo: REPO_URL.into(),
            template_sha: V1_SHA.into(),
            original_command: OriginalCommand::from_setup(&args),
            resolved: Some(Resolved {
                mcp: outcome.mcp_snapshottable_stems,
                hooks: outcome.hooks_snapshottable_stems,
            }),
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

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

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
    run_update(&ts_update(false, false, true, false), v2.path(), V2_SHA, REPO_URL).unwrap();

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

    let err = run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL)
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
    let err = run_update(&ts_update(false, false, true, false), v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("shape") || err.contains("directories"),
        "should explain shape mismatch: {err}"
    );
    // tool.sh remains a directory
    assert!(Path::new("tool.sh").is_dir(), "dir untouched on abort");

    // With --force, the directory is replaced by the file.
    run_update(&ts_update(true, false, true, false), v2.path(), V2_SHA, REPO_URL).unwrap();
    assert!(Path::new("tool.sh").is_file(), "tool.sh should now be a file");
    assert!(fs::read_to_string("tool.sh").unwrap().contains("v2"));
}

// ── Gitignore behavior across updates ───────────────────────────────────

#[test]
fn update_reapplies_gitignore_when_only_fragment_changed() {
    // `.gitignore` is not manifest-tracked, so a template-only change to a
    // gitignore fragment slips past the classify_update_path machinery. The
    // explicit re-apply in run_update must still pick it up.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        // v1: default fragment contains `.claude/` only.
        setup_and_lock(&v1, V1_SHA);
    }

    let pre = fs::read_to_string(".gitignore").unwrap();
    assert!(pre.contains(".claude/"));
    assert!(!pre.contains(".DS_Store"));

    // v2: default fragment grew — nothing else changed shape-wise.
    let v2 = build_scaffold("v1 ts rules\n");
    // Overwrite the default fragment written by build_scaffold.
    fs::write(
        v2.path().join("gitignore-additions/default.gitignore"),
        ".claude/\n.DS_Store\n",
    )
    .unwrap();

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let post = fs::read_to_string(".gitignore").unwrap();
    assert!(post.contains(".claude/"), "default line preserved:\n{post}");
    assert!(
        post.contains(".DS_Store"),
        "new fragment line must be appended on update:\n{post}"
    );
    // Idempotent — second update adds nothing new.
    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();
    let post2 = fs::read_to_string(".gitignore").unwrap();
    assert_eq!(post, post2, "second update must be a no-op for .gitignore");
}

#[test]
fn update_adds_language_with_only_gitignore_fragment() {
    // User updates with a new language whose only template surface is a
    // gitignore fragment. resolve_language must accept it (ConditionalOnly via
    // gitignore fragment) and the new fragment lines must land in .gitignore.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }

    // v2: adds a `python.gitignore` fragment. No rules file, no commands dir,
    // no MCP — python's sole surface is this fragment.
    let v2 = build_scaffold("v1 ts rules\n");
    fs::write(
        v2.path().join("gitignore-additions/python.gitignore"),
        "__pycache__/\n*.pyc\n",
    )
    .unwrap();

    // Update invocation adds `python` to the stored languages.
    let update = UpdateArgs {
        setup: SetupArgs {
            languages: vec!["python".into()],
            ..Default::default()
        },
        prune_stale: false,
        restore_deleted: false,
        merge: false,
        only: false,
    };
    run_update(&update, v2.path(), V2_SHA, REPO_URL).unwrap();

    let post = fs::read_to_string(".gitignore").unwrap();
    assert!(post.contains("__pycache__/"), "{post}");
    assert!(post.contains("*.pyc"), "{post}");

    // Lockfile should now record both languages (additive merge).
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    let langs = &lock.original_command.languages;
    assert!(langs.iter().any(|l| l == "ts"), "ts kept: {langs:?}");
    assert!(langs.iter().any(|l| l == "python"), "python added: {langs:?}");
}

// ── Snapshot / sticky reproducibility coverage ──────────────────────────

/// Set up v1 with `extra_named_mcps` additionally present at `mcp/<stem>.json`,
/// opt into them via `--mcp`, and persist a lockfile pinned to `V1_SHA`.
fn setup_and_lock_with_named_mcps(
    s: &Scaffold,
    named: &[(&str, &str)],
    args: &SetupArgs,
) {
    s.with_named_mcps(named);
    let outcome = run_setup(
        &RenderInputs { setup: args, sticky_mcp: &[], sticky_hooks: &[] },
        s.path(),
        Path::new("."),
        true,
        false,
    )
    .unwrap();
    let manifest =
        compute_manifest(args, &outcome.resolved_languages, s.path(), Path::new(".")).unwrap();
    Lockfile {
        template_repo: REPO_URL.into(),
        template_sha: V1_SHA.into(),
        original_command: OriginalCommand::from_setup(args),
        resolved: Some(Resolved {
            mcp: outcome.mcp_snapshottable_stems,
            hooks: outcome.hooks_snapshottable_stems,
        }),
        files: manifest,
    }
    .save(Path::new("."))
    .unwrap();
}

#[test]
fn prune_stale_drops_opt_in_from_original_command_and_snapshot() {
    // Regression for P1: when `--prune-stale` removes a stale contributor,
    // its stem must also disappear from `original_command.<kind>` so the next
    // render's assembler doesn't try to resolve it via the user_named path
    // (which would bail "MCP not found" since it's gone from the template).
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        let args = SetupArgs {
            languages: vec!["ts".into()],
            mcp: vec!["foo".into()],
            ..Default::default()
        };
        setup_and_lock_with_named_mcps(
            &v1,
            &[("foo", r#"{"foo": {"url": "foo-v1"}}"#)],
            &args,
        );
    }

    let lock_pre = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(
        lock_pre.original_command.mcp.contains(&"foo".to_string()),
        "pre: foo is historical in original_command.mcp"
    );
    assert!(
        lock_pre.resolved.as_ref().unwrap().mcp.contains(&"foo".to_string()),
        "pre: foo is pinned in the snapshot"
    );

    // v2 removes foo from the template entirely.
    let v2 = build_scaffold("v2 ts rules\n");

    run_update(&ts_update(false, false, true, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let lock_post = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(
        !lock_post.original_command.mcp.contains(&"foo".to_string()),
        "--prune-stale must strip foo from original_command.mcp, got {:?}",
        lock_post.original_command.mcp
    );
    let resolved_post = lock_post.resolved.unwrap().mcp;
    assert!(
        !resolved_post.contains(&"foo".to_string()),
        "--prune-stale must strip foo from resolved.mcp, got {resolved_post:?}"
    );
    assert_eq!(lock_post.template_sha, V2_SHA);
}

#[test]
fn fresh_mcp_flag_rejected_when_stem_only_exists_under_language_dir() {
    // Regression for P2: fresh positive --mcp validation must be root-only.
    // Accepting a language-layer stem would pin it as sticky and defeat the
    // "language layers stay dynamic" invariant.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }

    // v2 introduces `bar` ONLY under mcp/typescript/bar.json.
    let v2 = build_scaffold("v2 ts rules\n");
    v2.with_lang_mcps("typescript", &[("bar", r#"{"bar": {"url": "b"}}"#)]);

    let update = UpdateArgs {
        setup: SetupArgs {
            languages: vec!["ts".into()],
            mcp: vec!["bar".into()],
            ..Default::default()
        },
        prune_stale: false,
        restore_deleted: false,
        merge: false,
        only: false,
    };

    let err = run_update(&update, v2.path(), V2_SHA, REPO_URL)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("MCP 'bar'") && err.contains("mcp/bar.json"),
        "fresh --mcp on language-only stem must complain about missing root opt-in: {err}"
    );

    // Lockfile must not have advanced.
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V1_SHA);
    assert!(lock.original_command.mcp.is_empty());
}

#[test]
fn old_lockfile_without_resolved_bypasses_fast_path_and_writes_snapshot() {
    // Pre-snapshot lockfiles have `resolved: None`. The same-SHA + same-args
    // no-op fast path must NOT trigger for them — otherwise those projects
    // could never capture their snapshot without a template change.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    let v1 = build_scaffold("v1 ts rules\n");
    setup_and_lock(&v1, V1_SHA);

    // Downgrade the lockfile to pre-snapshot schema.
    {
        let mut lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
        lock.resolved = None;
        lock.save(Path::new(".")).unwrap();
    }
    assert!(
        Lockfile::load(Path::new(".")).unwrap().unwrap().resolved.is_none(),
        "pre: lockfile has no resolved block"
    );

    // Same SHA, same args, no restore flag. Fast-path would short-circuit
    // unless `snapshot_missing` forces a full pass.
    run_update(&ts_update(false, false, false, false), v1.path(), V1_SHA, REPO_URL).unwrap();

    let after = Lockfile::load(Path::new(".")).unwrap().unwrap();
    let resolved = after
        .resolved
        .expect("resolved must be populated after migration update");
    assert!(
        resolved.mcp.contains(&"context7".to_string()),
        "default-layer MCP must land in snapshot on first post-migration update, got {resolved:?}"
    );
}

#[test]
fn sticky_opt_in_preserved_when_contributor_moves_root_to_default() {
    // User opted into `extra` when it lived at the root layer. Template
    // relocates it to the default layer. The aggregation still includes it
    // (now via default) and the snapshot continues to pin the opt-in so a
    // later template flip back wouldn't lose the user's intent.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        let args = SetupArgs {
            languages: vec!["ts".into()],
            mcp: vec!["extra".into()],
            ..Default::default()
        };
        setup_and_lock_with_named_mcps(
            &v1,
            &[("extra", r#"{"extra": {"url": "e1"}}"#)],
            &args,
        );
    }

    let mcp_pre = fs::read_to_string(".mcp.json").unwrap();
    assert!(mcp_pre.contains("\"extra\""), "pre: {mcp_pre}");

    // v2 moves extra: mcp/extra.json → mcp/default/extra.json.
    let v2 = build_scaffold("v2 ts rules\n");
    v2.with_default_mcps(&[("extra", r#"{"extra": {"url": "e2"}}"#)]);

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let mcp_post = fs::read_to_string(".mcp.json").unwrap();
    assert!(
        mcp_post.contains("\"extra\""),
        "extra must survive the root→default move: {mcp_post}"
    );

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    let resolved_mcp = lock.resolved.unwrap().mcp;
    assert!(
        resolved_mcp.contains(&"extra".to_string()),
        "explicit opt-in must stay sticky across root→default move, got {resolved_mcp:?}"
    );
    assert!(
        lock.original_command.mcp.contains(&"extra".to_string()),
        "original_command.mcp must still hold the opt-in"
    );
}

/// Setup helper that seeds a root-level named hook, opts into it, and writes
/// the resulting lockfile pinned to `V1_SHA`.
fn setup_and_lock_with_named_hooks(
    s: &Scaffold,
    named: &[(&str, &str)],
    args: &SetupArgs,
) {
    s.with_named_hooks(named);
    let outcome = run_setup(
        &RenderInputs { setup: args, sticky_mcp: &[], sticky_hooks: &[] },
        s.path(),
        Path::new("."),
        true,
        false,
    )
    .unwrap();
    let manifest =
        compute_manifest(args, &outcome.resolved_languages, s.path(), Path::new(".")).unwrap();
    Lockfile {
        template_repo: REPO_URL.into(),
        template_sha: V1_SHA.into(),
        original_command: OriginalCommand::from_setup(args),
        resolved: Some(Resolved {
            mcp: outcome.mcp_snapshottable_stems,
            hooks: outcome.hooks_snapshottable_stems,
        }),
        files: manifest,
    }
    .save(Path::new("."))
    .unwrap();
}

// ── Drop-flag e2e coverage ──────────────────────────────────────────────

#[test]
fn update_with_drop_mcp_excludes_default_contributor() {
    // `--drop-mcp context7` on a default always-on contributor must remove it
    // from the rendered .mcp.json AND keep it out of the snapshot so the
    // exclusion survives future updates.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        setup_and_lock(&v1, V1_SHA);
    }
    let pre = fs::read_to_string(".mcp.json").unwrap();
    assert!(pre.contains("\"context7\""), "pre: default mcp present");

    let v2 = build_scaffold("v2 ts rules\n");

    let update = UpdateArgs {
        setup: SetupArgs {
            languages: vec!["ts".into()],
            drop_mcp: vec!["context7".into()],
            ..Default::default()
        },
        prune_stale: false,
        restore_deleted: false,
        merge: false,
        only: false,
    };
    run_update(&update, v2.path(), V2_SHA, REPO_URL).unwrap();

    let post = fs::read_to_string(".mcp.json").unwrap();
    assert!(
        !post.contains("\"context7\""),
        "--drop-mcp context7 must exclude the default contributor: {post}"
    );

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(
        lock.original_command.drop_mcp.contains(&"context7".to_string()),
        "drop must persist: {:?}",
        lock.original_command.drop_mcp
    );
    let resolved_mcp = lock.resolved.unwrap().mcp;
    assert!(
        !resolved_mcp.contains(&"context7".to_string()),
        "dropped default must NOT land in snapshot: {resolved_mcp:?}"
    );
}

#[test]
fn update_mcp_flag_undrops_previously_dropped_default_contributor() {
    // Exercises the full persisted-drop → newer-add undrop cycle end-to-end.
    // Also guards the `FRESH_POSITIVE_LAYERS` widening: if validation stayed
    // root-only, `--mcp context7` would bail here even though merge_additive
    // is documented to clear the persisted drop.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        let args = SetupArgs {
            languages: vec!["ts".into()],
            drop_mcp: vec!["context7".into()],
            ..Default::default()
        };
        let outcome = run_setup(
            &RenderInputs { setup: &args, sticky_mcp: &[], sticky_hooks: &[] },
            v1.path(),
            Path::new("."),
            true,
            false,
        )
        .unwrap();
        let manifest =
            compute_manifest(&args, &outcome.resolved_languages, v1.path(), Path::new(".")).unwrap();
        Lockfile {
            template_repo: REPO_URL.into(),
            template_sha: V1_SHA.into(),
            original_command: OriginalCommand::from_setup(&args),
            resolved: Some(Resolved {
                mcp: outcome.mcp_snapshottable_stems,
                hooks: outcome.hooks_snapshottable_stems,
            }),
            files: manifest,
        }
        .save(Path::new("."))
        .unwrap();
    }
    let pre = fs::read_to_string(".mcp.json").unwrap();
    assert!(
        !pre.contains("\"context7\""),
        "pre: dropped default must be absent, got {pre}"
    );

    let v2 = build_scaffold("v2 ts rules\n");
    let update = UpdateArgs {
        setup: SetupArgs {
            languages: vec!["ts".into()],
            mcp: vec!["context7".into()],
            ..Default::default()
        },
        prune_stale: false,
        restore_deleted: false,
        merge: false,
        only: false,
    };
    run_update(&update, v2.path(), V2_SHA, REPO_URL).unwrap();

    let post = fs::read_to_string(".mcp.json").unwrap();
    assert!(
        post.contains("\"context7\""),
        "--mcp must undrop the default contributor, got {post}"
    );

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(
        lock.original_command.mcp.contains(&"context7".to_string()),
        "newer --mcp must land in original_command.mcp"
    );
    assert!(
        !lock.original_command.drop_mcp.contains(&"context7".to_string()),
        "persisted drop must be cleared after undrop, got {:?}",
        lock.original_command.drop_mcp
    );
    assert!(
        lock.resolved.unwrap().mcp.contains(&"context7".to_string()),
        "undropped contributor must reappear in the snapshot"
    );
}

// ── Hook snapshot coverage ──────────────────────────────────────────────

#[test]
fn prune_stale_drops_hook_opt_in_from_original_command_and_snapshot() {
    // Hook parallel to the MCP prune-stale opt-in test. Guards the symmetric
    // half of the P1 fix — the stale retain pass for hooks.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        let args = SetupArgs {
            languages: vec!["ts".into()],
            hooks: vec!["notify".into()],
            ..Default::default()
        };
        setup_and_lock_with_named_hooks(
            &v1,
            &[(
                "notify",
                r#"{"PreToolUse": [{"type":"command","command":"echo v1"}]}"#,
            )],
            &args,
        );
    }

    let lock_pre = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(
        lock_pre.original_command.hooks.contains(&"notify".to_string()),
        "pre: notify persisted in original_command.hooks"
    );
    assert!(
        lock_pre
            .resolved
            .as_ref()
            .unwrap()
            .hooks
            .contains(&"notify".to_string()),
        "pre: notify pinned in snapshot"
    );

    // v2 removes hooks/notify.json entirely.
    let v2 = build_scaffold("v2 ts rules\n");

    run_update(&ts_update(false, false, true, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let lock_post = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(
        !lock_post.original_command.hooks.contains(&"notify".to_string()),
        "--prune-stale must strip notify from original_command.hooks, got {:?}",
        lock_post.original_command.hooks
    );
    let resolved_hooks = lock_post.resolved.unwrap().hooks;
    assert!(
        !resolved_hooks.contains(&"notify".to_string()),
        "--prune-stale must strip notify from resolved.hooks, got {resolved_hooks:?}"
    );
    assert_eq!(lock_post.template_sha, V2_SHA);
}

#[test]
fn sticky_hook_opt_in_preserved_when_contributor_moves_root_to_default() {
    // Hook parallel to the MCP root→default sticky test. Exercises the
    // already-satisfied branch of assemble_hooks_json.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        let args = SetupArgs {
            languages: vec!["ts".into()],
            hooks: vec!["notify".into()],
            ..Default::default()
        };
        setup_and_lock_with_named_hooks(
            &v1,
            &[(
                "notify",
                r#"{"PreToolUse": [{"type":"command","command":"echo v1"}]}"#,
            )],
            &args,
        );
    }

    let settings_pre =
        fs::read_to_string(".claude/settings.local.json").unwrap();
    assert!(
        settings_pre.contains("echo v1"),
        "pre: notify hook entry present: {settings_pre}"
    );

    // v2 relocates the hook from hooks/notify.json → hooks/default/notify.json.
    let v2 = build_scaffold("v2 ts rules\n");
    v2.with_default_hooks(&[(
        "notify",
        r#"{"PreToolUse": [{"type":"command","command":"echo v2"}]}"#,
    )]);

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let settings_post =
        fs::read_to_string(".claude/settings.local.json").unwrap();
    assert!(
        settings_post.contains("echo v2"),
        "hook must survive root→default relocation: {settings_post}"
    );

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    let resolved_hooks = lock.resolved.unwrap().hooks;
    assert!(
        resolved_hooks.contains(&"notify".to_string()),
        "explicit hook opt-in must stay sticky across root→default move, got {resolved_hooks:?}"
    );
    assert!(
        lock.original_command.hooks.contains(&"notify".to_string()),
        "original_command.hooks must still hold the opt-in"
    );
}

#[test]
fn sticky_default_hook_preserved_when_relocated_to_root() {
    // Symmetric to the MCP default→root case for hooks. Exercises
    // assemble_hooks_json's move-fallback branch: default-layer hook was
    // snapshotted without any --hooks flag; template relocates it to root.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        v1.with_default_hooks(&[(
            "watchdog",
            r#"{"PreToolUse": [{"type":"command","command":"echo v1"}]}"#,
        )]);
        setup_and_lock(&v1, V1_SHA);
    }
    let lock_pre = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(
        lock_pre
            .resolved
            .unwrap()
            .hooks
            .contains(&"watchdog".to_string()),
        "pre: watchdog snapshotted from default layer"
    );
    assert!(
        lock_pre.original_command.hooks.is_empty(),
        "pre: watchdog was never flagged — original_command.hooks empty"
    );

    // v2 moves watchdog to the root opt-in layer.
    let v2 = build_scaffold("v2 ts rules\n");
    v2.with_named_hooks(&[(
        "watchdog",
        r#"{"PreToolUse": [{"type":"command","command":"echo v2"}]}"#,
    )]);

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let settings_post =
        fs::read_to_string(".claude/settings.local.json").unwrap();
    assert!(
        settings_post.contains("echo v2"),
        "watchdog must survive default→root relocation via sticky snapshot: {settings_post}"
    );

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    let resolved_hooks = lock.resolved.unwrap().hooks;
    assert!(
        resolved_hooks.contains(&"watchdog".to_string()),
        "snapshot must still pin watchdog after move-fallback resolved it at root, got {resolved_hooks:?}"
    );
}

#[test]
fn sticky_default_contributor_preserved_when_relocated_to_root() {
    // Symmetric to the root→default case, but exercises the move-fallback
    // branch of the assembler. A default-layer contributor gets snapshotted
    // on initial setup without any --mcp flag; when the template relocates
    // it to the root opt-in layer, the snapshot should keep it live even
    // though no user_named flag ever pinned it.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        v1.with_default_mcps(&[("widget", r#"{"widget": {"url": "w1"}}"#)]);
        setup_and_lock(&v1, V1_SHA);
    }
    let lock_pre = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert!(
        lock_pre.resolved.unwrap().mcp.contains(&"widget".to_string()),
        "pre: widget snapshotted from default layer"
    );
    assert!(
        lock_pre.original_command.mcp.is_empty(),
        "pre: widget was never flagged by the user — original_command.mcp empty"
    );

    // v2 moves widget to the root opt-in layer (no longer auto-applied).
    let v2 = build_scaffold("v2 ts rules\n");
    v2.with_named_mcps(&[("widget", r#"{"widget": {"url": "w2"}}"#)]);

    run_update(&ts_update(false, false, false, false), v2.path(), V2_SHA, REPO_URL).unwrap();

    let mcp_post = fs::read_to_string(".mcp.json").unwrap();
    assert!(
        mcp_post.contains("\"widget\""),
        "widget must survive default→root relocation via sticky snapshot: {mcp_post}"
    );

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    let resolved_mcp = lock.resolved.unwrap().mcp;
    assert!(
        resolved_mcp.contains(&"widget".to_string()),
        "snapshot must still pin widget after move-fallback resolved it at root, got {resolved_mcp:?}"
    );
}

// ── --only flag: add without syncing template ──────────────────────────

#[test]
fn only_adds_mcp_without_syncing_template_changes() {
    // Simulates `--only` by passing the lockfile's SHA with the original
    // template content.  The new --mcp should land in .mcp.json but
    // CLAUDE.md and other files must stay at v1 content.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    let v1 = build_scaffold("v1 ts rules\n");
    v1.with_named_mcps(&[("extra", r#"{"extra": {"url": "e1"}}"#)]);
    setup_and_lock(&v1, V1_SHA);

    let claude_md_before = fs::read_to_string("CLAUDE.md").unwrap();
    assert!(claude_md_before.contains("v1 ts rules"));

    // --only: same scaffold (v1), same SHA, just adding --mcp extra.
    let update = UpdateArgs {
        setup: SetupArgs {
            mcp: vec!["extra".into()],
            ..Default::default()
        },
        prune_stale: false,
        restore_deleted: false,
        merge: false,
        only: false,
    };
    run_update(&update, v1.path(), V1_SHA, REPO_URL).unwrap();

    // MCP was added.
    let mcp_post = fs::read_to_string(".mcp.json").unwrap();
    assert!(
        mcp_post.contains("\"extra\""),
        "extra MCP must be present: {mcp_post}"
    );

    // CLAUDE.md still has v1 content (template wasn't synced).
    let claude_md_after = fs::read_to_string("CLAUDE.md").unwrap();
    assert!(
        claude_md_after.contains("v1 ts rules"),
        "CLAUDE.md must still have v1 rules under --only, got:\n{claude_md_after}"
    );

    // SHA stays at V1 (pinned).
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V1_SHA);
    assert!(lock.original_command.mcp.contains(&"extra".to_string()));
}

#[test]
fn only_adds_hooks_without_syncing_template_changes() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    let v1 = build_scaffold("v1 ts rules\n");
    v1.with_named_hooks(&[(
        "notify",
        r#"{"PreToolUse": [{"type":"command","command":"echo notify"}]}"#,
    )]);
    setup_and_lock(&v1, V1_SHA);

    let claude_md_before = fs::read_to_string("CLAUDE.md").unwrap();
    assert!(claude_md_before.contains("v1 ts rules"));

    let update = UpdateArgs {
        setup: SetupArgs {
            hooks: vec!["notify".into()],
            ..Default::default()
        },
        prune_stale: false,
        restore_deleted: false,
        merge: false,
        only: false,
    };
    run_update(&update, v1.path(), V1_SHA, REPO_URL).unwrap();

    let settings = fs::read_to_string(".claude/settings.local.json").unwrap();
    assert!(
        settings.contains("echo notify"),
        "hook must be added: {settings}"
    );

    // Template content unchanged.
    let claude_md_after = fs::read_to_string("CLAUDE.md").unwrap();
    assert!(
        claude_md_after.contains("v1 ts rules"),
        "CLAUDE.md must stay at v1 under --only: {claude_md_after}"
    );
    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V1_SHA);
}

#[test]
fn normal_update_syncs_template_changes_unlike_only() {
    // Contrast test: a normal update (not --only) at V2_SHA DOES pick up
    // template changes.  This proves the --only simulation above is meaningful.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    {
        let v1 = build_scaffold("v1 ts rules\n");
        v1.with_named_mcps(&[("extra", r#"{"extra": {"url": "e1"}}"#)]);
        setup_and_lock(&v1, V1_SHA);
    }

    let claude_md_before = fs::read_to_string("CLAUDE.md").unwrap();
    assert!(claude_md_before.contains("v1 ts rules"));

    // Normal update: v2 scaffold with changed rules, at V2_SHA.
    let v2 = build_scaffold("v2 ts rules\n");
    v2.with_named_mcps(&[("extra", r#"{"extra": {"url": "e2"}}"#)]);

    let update = UpdateArgs {
        setup: SetupArgs {
            mcp: vec!["extra".into()],
            ..Default::default()
        },
        prune_stale: false,
        restore_deleted: false,
        merge: false,
        only: false,
    };
    run_update(&update, v2.path(), V2_SHA, REPO_URL).unwrap();

    // MCP landed.
    let mcp_post = fs::read_to_string(".mcp.json").unwrap();
    assert!(mcp_post.contains("\"extra\""));

    // CLAUDE.md picked up v2 rules — proving a non-pinned update syncs.
    let claude_md_after = fs::read_to_string("CLAUDE.md").unwrap();
    assert!(
        claude_md_after.contains("v2 ts rules"),
        "normal update must sync template changes, got:\n{claude_md_after}"
    );

    let lock = Lockfile::load(Path::new(".")).unwrap().unwrap();
    assert_eq!(lock.template_sha, V2_SHA);
}

#[test]
fn only_with_no_additive_args_is_noop() {
    // --only with no new args: SHA unchanged, command unchanged → fast path.
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    let v1 = build_scaffold("v1 ts rules\n");
    setup_and_lock(&v1, V1_SHA);

    let claude_md_before = fs::read("CLAUDE.md").unwrap();
    let lock_before = fs::read(LOCKFILE_NAME).unwrap();

    let update = UpdateArgs {
        setup: SetupArgs::default(),
        prune_stale: false,
        restore_deleted: false,
        merge: false,
        only: false,
    };
    run_update(&update, v1.path(), V1_SHA, REPO_URL).unwrap();

    assert_eq!(fs::read("CLAUDE.md").unwrap(), claude_md_before);
    assert_eq!(
        fs::read(LOCKFILE_NAME).unwrap(),
        lock_before,
        "lockfile must not be re-written on no-op --only"
    );
}
