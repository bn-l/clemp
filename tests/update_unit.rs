//! Unit tests for the `clemp update` building blocks: command merging, lockfile
//! round-trips, manifest computation, classifier table, the merge_with_claude
//! error contract, and CLI parsing for the `update`/`list` subcommands.

mod common;

use clap::Parser;
use clemp::{
    classify_update_path, compute_manifest, hash_bytes, lockfile_key, merge_with_claude,
    normalize_setup_args, run_setup, Cli, CliCommand, Lockfile, OriginalCommand, SetupArgs,
    UpdateClass,
};
use common::{CwdGuard, PathGuard, Scaffold};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ── OriginalCommand::merge_additive ─────────────────────────────────────

#[test]
fn merge_additive_unions_vectors_without_duplicates() {
    let mut a = OriginalCommand {
        languages: vec!["typescript".into()],
        hooks: vec!["sound".into()],
        mcp: vec!["context7".into()],
        commands: vec!["review".into()],
        githooks: vec!["pre-push".into()],
        clarg: Some("default".into()),
    };
    let b = OriginalCommand {
        languages: vec!["typescript".into(), "python".into()],
        hooks: vec!["lint".into(), "sound".into()],
        mcp: vec![],
        commands: vec!["review".into(), "deploy".into()],
        githooks: vec!["commit-msg".into()],
        clarg: None,
    };
    a.merge_additive(&b);

    assert_eq!(a.languages, vec!["typescript", "python"]);
    assert_eq!(a.hooks, vec!["sound", "lint"]);
    assert_eq!(a.mcp, vec!["context7"]);
    assert_eq!(a.commands, vec!["review", "deploy"]);
    assert_eq!(a.githooks, vec!["pre-push", "commit-msg"]);
    // None on the right keeps the existing clarg.
    assert_eq!(a.clarg.as_deref(), Some("default"));
}

#[test]
fn merge_additive_replaces_clarg_only_when_other_set() {
    let mut a = OriginalCommand {
        clarg: Some("default".into()),
        ..Default::default()
    };
    let b = OriginalCommand {
        clarg: Some("strict".into()),
        ..Default::default()
    };
    a.merge_additive(&b);
    assert_eq!(a.clarg.as_deref(), Some("strict"));
}

#[test]
fn merge_additive_clarg_unset_on_other_preserves_existing() {
    let mut a = OriginalCommand {
        clarg: Some("default".into()),
        ..Default::default()
    };
    let b = OriginalCommand {
        clarg: None,
        ..Default::default()
    };
    a.merge_additive(&b);
    assert_eq!(a.clarg.as_deref(), Some("default"));
}

// ── Additive language canonicalization ──────────────────────────────────
//
// merge_additive itself is purely string-set union, so `ts` and `typescript`
// would both end up in the languages vector. The canonicalization happens at
// language resolution time inside run_setup, which dedupes via canonical name.

#[test]
fn language_aliases_dedupe_through_resolution() {
    let s = Scaffold::new();
    s.with_template("{{ lang_rules }}", &[("typescript.md", "ts rules")]);

    let resolved = clemp::resolve_all_languages(
        &["ts".into(), "typescript".into(), "TS".into()],
        s.path(),
    )
    .unwrap();

    // Strict: all three alias-inputs must collapse to exactly one canonical entry.
    assert_eq!(
        resolved,
        vec!["typescript".to_string()],
        "aliases must dedupe to a single canonical entry, got {resolved:?}"
    );
}

#[test]
fn resolve_all_languages_preserves_order_across_distinct_canonicals() {
    let s = Scaffold::new();
    s.with_template(
        "{{ lang_rules }}",
        &[("typescript.md", "ts"), ("python.md", "py"), ("rust.md", "rs")],
    );

    let resolved = clemp::resolve_all_languages(
        &["py".into(), "ts".into(), "typescript".into(), "rust".into()],
        s.path(),
    )
    .unwrap();

    assert_eq!(resolved, vec!["python", "typescript", "rust"]);
}

#[test]
fn merge_additive_dedupes_language_aliases() {
    let mut a = OriginalCommand {
        languages: vec!["ts".into()],
        ..Default::default()
    };
    let b = OriginalCommand {
        languages: vec!["typescript".into(), "python".into(), "TS".into()],
        ..Default::default()
    };
    a.merge_additive(&b);
    // "typescript" and "TS" both canonicalize to "typescript" (already present
    // via "ts"), so only "python" should be appended.
    assert_eq!(
        a.languages,
        vec!["ts", "python"],
        "language aliases must dedupe by canonical form, got {:?}",
        a.languages
    );
}

#[test]
fn merge_additive_dedupes_languages_in_other_side_too() {
    let mut a = OriginalCommand::default();
    let b = OriginalCommand {
        languages: vec!["ts".into(), "typescript".into(), "python".into(), "py".into()],
        ..Default::default()
    };
    a.merge_additive(&b);
    assert_eq!(
        a.languages,
        vec!["ts", "python"],
        "dedup must also apply within the incoming list"
    );
}

// ── CLI parsing: update / list subcommands ──────────────────────────────

#[test]
fn cli_update_subcommand_parses() {
    let cli =
        Cli::try_parse_from(["clemp", "update", "ts", "--mcp", "context7", "--prune-stale"])
            .unwrap();
    match cli.command {
        Some(CliCommand::Update(args)) => {
            assert_eq!(args.setup.languages, vec!["ts"]);
            assert_eq!(args.setup.mcp, vec!["context7"]);
            assert!(args.prune_stale);
            assert!(!args.restore_deleted);
            assert!(!args.setup.force);
        }
        other => panic!("expected Update subcommand, got {:?}", other.is_some()),
    }
}

#[test]
fn cli_update_restore_deleted_force_flags() {
    let cli =
        Cli::try_parse_from(["clemp", "update", "--restore-deleted", "--force"]).unwrap();
    match cli.command {
        Some(CliCommand::Update(args)) => {
            assert!(args.restore_deleted);
            assert!(args.setup.force);
        }
        _ => panic!("expected Update"),
    }
}

#[test]
fn cli_list_no_category() {
    let cli = Cli::try_parse_from(["clemp", "list"]).unwrap();
    match cli.command {
        Some(CliCommand::List { category }) => assert!(category.is_none()),
        _ => panic!("expected List"),
    }
}

#[test]
fn cli_list_with_category() {
    let cli = Cli::try_parse_from(["clemp", "list", "mcp"]).unwrap();
    match cli.command {
        Some(CliCommand::List { category }) => assert_eq!(category.as_deref(), Some("mcp")),
        _ => panic!("expected List"),
    }
}

#[test]
fn cli_default_command_is_setup() {
    let cli = Cli::try_parse_from(["clemp", "ts", "--hooks", "sound"]).unwrap();
    assert!(cli.command.is_none());
    assert_eq!(cli.setup.languages, vec!["ts"]);
    assert_eq!(cli.setup.hooks, vec!["sound"]);
}

// ── normalize_setup_args ────────────────────────────────────────────────

#[test]
fn normalize_setup_args_splits_whitespace_in_update_args() {
    let mut args = SetupArgs {
        hooks: vec!["sound lint".into(), "format".into()],
        mcp: vec!["a,b".into(), "c d".into()],
        commands: vec!["review deploy".into()],
        githooks: vec!["pre-push commit-msg".into()],
        ..Default::default()
    };
    normalize_setup_args(&mut args);
    // value_delimiter handles commas in the parser; normalize splits whitespace.
    assert_eq!(args.hooks, vec!["sound", "lint", "format"]);
    assert_eq!(args.mcp, vec!["a,b", "c", "d"]);
    assert_eq!(args.commands, vec!["review", "deploy"]);
    assert_eq!(args.githooks, vec!["pre-push", "commit-msg"]);
}

// ── Lockfile round-trip ─────────────────────────────────────────────────

#[test]
fn lockfile_round_trip_preserves_all_fields() {
    let dir = TempDir::new().unwrap();
    let mut files = BTreeMap::new();
    files.insert("CLAUDE.md".into(), "abc123".into());
    files.insert(".claude/settings.local.json".into(), "def456".into());

    let lock = Lockfile {
        template_repo: "https://example.test/template.git".into(),
        template_sha: "deadbeef".into(),
        original_command: OriginalCommand {
            languages: vec!["typescript".into()],
            hooks: vec!["sound".into()],
            mcp: vec!["context7".into()],
            commands: vec!["review".into()],
            githooks: vec!["pre-push".into()],
            clarg: Some("default".into()),
        },
        files: files.clone(),
    };

    lock.save(dir.path()).unwrap();
    let loaded = Lockfile::load(dir.path()).unwrap().unwrap();

    assert_eq!(loaded.template_repo, lock.template_repo);
    assert_eq!(loaded.template_sha, lock.template_sha);
    assert_eq!(loaded.original_command, lock.original_command);
    assert_eq!(loaded.files, files);
}

#[test]
fn lockfile_load_returns_none_when_missing() {
    let dir = TempDir::new().unwrap();
    assert!(Lockfile::load(dir.path()).unwrap().is_none());
}

// ── lockfile_key normalization ──────────────────────────────────────────

#[test]
fn lockfile_key_strips_curdir_and_uses_forward_slashes() {
    assert_eq!(lockfile_key(Path::new("./.claude/settings.local.json")), ".claude/settings.local.json");
    assert_eq!(lockfile_key(Path::new(".claude/settings.local.json")), ".claude/settings.local.json");
    assert_eq!(lockfile_key(Path::new("foo/bar/baz.txt")), "foo/bar/baz.txt");
}

#[test]
fn lockfile_key_handles_root_relative_segments() {
    // A leading `/` is a RootDir component on Unix and is stripped.
    #[cfg(unix)]
    assert_eq!(lockfile_key(Path::new("/foo/bar")), "foo/bar");
    assert_eq!(lockfile_key(Path::new("foo/./bar")), "foo/bar");
}

// ── compute_manifest ────────────────────────────────────────────────────

fn manifest_scaffold() -> Scaffold {
    let s = Scaffold::new();
    s.with_template("{{ lang_rules }}", &[("typescript.md", "ts")]);
    s.with_settings("{}");
    s.with_default_mcps(&[("context7", r#"{"context7":{}}"#)]);
    s.with_copied("default", &[(".editorconfig", "root = true")]);
    s.with_gitignore_additions(".claude/\n");
    s
}

#[test]
fn compute_manifest_includes_root_files_overlays_excludes_meta() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    let s = manifest_scaffold();

    let args = SetupArgs {
        languages: vec!["ts".into()],
        ..Default::default()
    };
    let resolved = run_setup(&args, s.path(), Path::new("."), true, false).unwrap();
    let manifest = compute_manifest(&args, &resolved, s.path(), Path::new(".")).unwrap();

    assert!(manifest.contains_key("CLAUDE.md"), "manifest must contain CLAUDE.md");
    assert!(manifest.contains_key(".mcp.json"), "manifest must contain .mcp.json");
    assert!(
        manifest.contains_key(".claude/settings.local.json"),
        "manifest must contain settings"
    );
    assert!(
        manifest.contains_key(".editorconfig"),
        "manifest must include copied/default overlay"
    );
    assert!(!manifest.contains_key(".gitignore"), "must never track .gitignore");
    assert!(
        !manifest.contains_key(".clemp-lock.yaml"),
        "must never track its own lockfile"
    );

    // Hashes must be 64-char sha256 hex
    for (k, v) in &manifest {
        assert_eq!(v.len(), 64, "{k} hash should be sha256 hex, got {v}");
    }
}

#[test]
fn compute_manifest_no_git_hooks_entries_when_no_git_dir() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    let s = manifest_scaffold();

    // Add some githook files to the template, but DON'T install them
    // (install_git_hooks=false). The manifest must still not include phantom
    // hook entries because hash_tree only records files that actually exist.
    fs::create_dir_all(s.path().join("githooks/default")).unwrap();
    fs::write(s.path().join("githooks/default/pre-commit"), "#!/bin/sh").unwrap();

    let args = SetupArgs {
        languages: vec!["ts".into()],
        ..Default::default()
    };
    let resolved = run_setup(&args, s.path(), Path::new("."), true, false).unwrap();
    let manifest = compute_manifest(&args, &resolved, s.path(), Path::new(".")).unwrap();

    assert!(
        !manifest.keys().any(|k| k.starts_with(".git/hooks/")),
        "should not invent git-hook entries when none were installed: {:?}",
        manifest.keys().collect::<Vec<_>>()
    );
}

#[test]
fn compute_manifest_includes_git_hooks_when_installed() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());
    let s = manifest_scaffold();

    fs::create_dir_all(s.path().join("githooks/default")).unwrap();
    fs::write(s.path().join("githooks/default/pre-commit"), "#!/bin/sh").unwrap();
    // Pretend we have a .git/ so install_git_hooks=true would be valid in real flow
    fs::create_dir_all(workdir.path().join(".git/hooks")).unwrap();

    let args = SetupArgs {
        languages: vec!["ts".into()],
        ..Default::default()
    };
    let resolved = run_setup(&args, s.path(), Path::new("."), true, true).unwrap();
    let manifest = compute_manifest(&args, &resolved, s.path(), Path::new(".")).unwrap();

    assert!(
        manifest.contains_key(".git/hooks/pre-commit"),
        "should track installed git hooks: {:?}",
        manifest.keys().collect::<Vec<_>>()
    );
}

// ── classify_update_path table ──────────────────────────────────────────

#[test]
fn classifier_table() {
    use UpdateClass::*;
    // (label, old, cur, new, cwd_is_dir, expected)
    let cases: &[(&str, Option<&str>, Option<&str>, &str, bool, UpdateClass)] = &[
        ("clean (template moved, user untouched)", Some("a"), Some("a"), "b", false, Clean),
        ("identical (everything matches)",        Some("a"), Some("a"), "a", false, Identical),
        ("new (not tracked, not on disk)",        None,     None,     "b", false, New),
        ("collision (untracked, user has different file)", None, Some("c"), "b", false, Collision),
        ("identical-coincidence (untracked, user file matches new template)", None, Some("b"), "b", false, Identical),
        ("missing (tracked, user deleted)",       Some("a"), None,     "b", false, Missing),
        ("skipped (user-modified, template unchanged)", Some("a"), Some("c"), "a", false, Skipped),
        ("conflict (user + template both changed)", Some("a"), Some("c"), "b", false, Conflict),
        ("shape: dir at file path (no lockfile)", None,     None,     "b", true,  ShapeCollision),
        ("shape: dir at file path (was tracked)", Some("a"), None,     "b", true,  ShapeCollision),
    ];

    for (label, old, cur, new, cwd_is_dir, expected) in cases {
        let got = classify_update_path(*old, *cur, new, *cwd_is_dir);
        assert_eq!(
            got, *expected,
            "case `{label}`: expected {expected:?}, got {got:?}"
        );
    }
}

#[test]
fn classifier_treats_directory_as_distinct_from_missing() {
    // Same hash triple, only `cwd_is_dir` differs.
    let triple = (None, None, "deadbeef");
    assert_eq!(
        classify_update_path(triple.0, triple.1, triple.2, false),
        UpdateClass::New
    );
    assert_eq!(
        classify_update_path(triple.0, triple.1, triple.2, true),
        UpdateClass::ShapeCollision
    );
}

// ── merge_with_claude error contract ────────────────────────────────────

#[test]
fn merge_with_claude_errors_on_non_zero_exit() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    let bindir = workdir.path().join("bin");
    common::install_fake_claude(&bindir);
    let _path = PathGuard::replace_with(&bindir);

    // Force the fake claude to exit non-zero.
    let mut env = common::EnvVarGuard::new();
    env.set("FAKE_CLAUDE_EXIT", "7");

    let staging = workdir.path().join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("foo.md"), "new").unwrap();
    fs::write(workdir.path().join("foo.md"), "cur").unwrap();

    let result = merge_with_claude("foo.md", &staging, Path::new("."));
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("aborting update"),
        "expected aborting-update message, got: {err}"
    );
    assert!(err.contains("--force"), "should mention --force escape: {err}");
}

#[test]
fn merge_with_claude_succeeds_when_claude_exits_zero() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    let bindir = workdir.path().join("bin");
    common::install_fake_claude(&bindir);
    let _path = PathGuard::replace_with(&bindir);

    let staging = workdir.path().join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("foo.md"), "new").unwrap();
    fs::write(workdir.path().join("foo.md"), "cur").unwrap();

    merge_with_claude("foo.md", &staging, Path::new(".")).unwrap();
}

// ── hash_bytes sanity ───────────────────────────────────────────────────

#[test]
fn hash_bytes_is_lowercase_64char_hex() {
    let h = hash_bytes(b"hello");
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_eq!(
        h,
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

// Helper used in language test above to keep imports clean.
#[allow(dead_code)]
fn _unused_to_silence_pathbuf_warning(_p: PathBuf) {}
