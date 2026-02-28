//! Tests for git hook installation (--githooks flag).

mod common;

use clemp::{copy_conditional_githooks, copy_named_githooks};
use common::Scaffold;
use std::fs;
use tempfile::TempDir;

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

// ── Scaffold helper ─────────────────────────────────────────────────────

impl Scaffold {
    fn with_default_githooks(&self, hooks: &[(&str, &str)]) {
        let dir = self.path().join("githooks/default");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in hooks {
            fs::write(dir.join(name), content).unwrap();
        }
    }

    fn with_lang_githooks(&self, lang: &str, hooks: &[(&str, &str)]) {
        let dir = self.path().join("githooks").join(lang);
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in hooks {
            fs::write(dir.join(name), content).unwrap();
        }
    }

    fn with_named_githooks(&self, hooks: &[(&str, &str)]) {
        let dir = self.path().join("githooks");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in hooks {
            fs::write(dir.join(name), content).unwrap();
        }
    }
}

// ── copy_named_githooks ─────────────────────────────────────────────────

#[test]
fn named_githooks_copied_to_dest_and_executable() {
    let s = Scaffold::new();
    s.with_named_githooks(&[("commit-msg", "#!/bin/sh\nexit 0"), ("prepare-commit-msg", "#!/bin/sh\nexit 0")]);

    let dest = TempDir::new().unwrap();
    copy_named_githooks(
        &["commit-msg".into(), "prepare-commit-msg".into()],
        s.path(),
        dest.path(),
    )
    .unwrap();

    let cm = dest.path().join("commit-msg");
    let pcm = dest.path().join("prepare-commit-msg");
    assert_eq!(fs::read_to_string(&cm).unwrap(), "#!/bin/sh\nexit 0");
    assert_eq!(fs::read_to_string(&pcm).unwrap(), "#!/bin/sh\nexit 0");

    #[cfg(unix)]
    {
        assert!(is_executable(&cm));
        assert!(is_executable(&pcm));
    }
}

#[test]
fn named_githook_not_found_errors_with_available() {
    let s = Scaffold::new();
    s.with_named_githooks(&[("commit-msg", "#!/bin/sh")]);

    let dest = TempDir::new().unwrap();
    let result = copy_named_githooks(&["nonexistent".into()], s.path(), dest.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("nonexistent"), "should mention missing hook: {msg}");
    assert!(msg.contains("not found"), "should say not found: {msg}");
    assert!(msg.contains("commit-msg"), "should list available hooks: {msg}");
}

#[test]
fn named_githook_available_list_excludes_dirs() {
    let s = Scaffold::new();
    s.with_default_githooks(&[("pre-commit", "#!/bin/sh")]);
    s.with_named_githooks(&[("commit-msg", "#!/bin/sh")]);

    let dest = TempDir::new().unwrap();
    let result = copy_named_githooks(&["nonexistent".into()], s.path(), dest.path());
    let msg = result.unwrap_err().to_string();

    assert!(msg.contains("commit-msg"), "should list root-level files: {msg}");
    assert!(!msg.contains("default"), "should not list subdirectories: {msg}");
}

#[test]
fn no_githooks_dir_with_named_githooks_errors() {
    let s = Scaffold::new();

    let dest = TempDir::new().unwrap();
    let result = copy_named_githooks(&["pre-commit".into()], s.path(), dest.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--githooks specified"));
}

#[test]
fn empty_named_githooks_is_noop() {
    let s = Scaffold::new();
    let dest = TempDir::new().unwrap();
    copy_named_githooks(&[], s.path(), dest.path()).unwrap();
    // dest should remain empty (no files created)
    assert_eq!(fs::read_dir(dest.path()).unwrap().count(), 0);
}

// ── copy_conditional_githooks ───────────────────────────────────────────

#[test]
fn conditional_default_only() {
    let s = Scaffold::new();
    s.with_default_githooks(&[("pre-commit", "#!/bin/sh\necho default")]);

    let dest = TempDir::new().unwrap();
    copy_conditional_githooks(&s.path().join("githooks"), &[], dest.path()).unwrap();

    let hook = dest.path().join("pre-commit");
    assert_eq!(fs::read_to_string(&hook).unwrap(), "#!/bin/sh\necho default");

    #[cfg(unix)]
    assert!(is_executable(&hook));
}

#[test]
fn conditional_language_overrides_default() {
    let s = Scaffold::new();
    s.with_default_githooks(&[("pre-commit", "#!/bin/sh\necho default")]);
    s.with_lang_githooks("rust", &[("pre-commit", "#!/bin/sh\necho rust")]);

    let dest = TempDir::new().unwrap();
    copy_conditional_githooks(
        &s.path().join("githooks"),
        &["rust".into()],
        dest.path(),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(dest.path().join("pre-commit")).unwrap(),
        "#!/bin/sh\necho rust"
    );
}

#[test]
fn conditional_missing_source_dir_ok() {
    let s = Scaffold::new();
    let dest = TempDir::new().unwrap();
    copy_conditional_githooks(&s.path().join("githooks"), &[], dest.path()).unwrap();
}

#[test]
fn named_overrides_conditional() {
    let s = Scaffold::new();
    s.with_default_githooks(&[("pre-commit", "#!/bin/sh\necho default")]);
    s.with_named_githooks(&[("pre-commit", "#!/bin/sh\necho named")]);

    let dest = TempDir::new().unwrap();

    // Conditional first
    copy_conditional_githooks(&s.path().join("githooks"), &[], dest.path()).unwrap();
    // Named on top
    copy_named_githooks(&["pre-commit".into()], s.path(), dest.path()).unwrap();

    assert_eq!(
        fs::read_to_string(dest.path().join("pre-commit")).unwrap(),
        "#!/bin/sh\necho named"
    );
}
