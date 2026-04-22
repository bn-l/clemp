//! Shared test helpers: scaffold for fake clone directories and CWD guard.

use clemp::CLONE_DIR;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::TempDir;

/// Global mutex to serialize tests that change the process working directory.
pub static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Scaffolds a fake clone directory matching the v2 template structure.
pub struct Scaffold {
    pub dir: TempDir,
}

impl Scaffold {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().unwrap(),
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    // ── CLAUDE.md template ───────────────────────────────────────────

    /// Write CLAUDE.md.jinja and optional lang-rules files.
    pub fn with_template(&self, template: &str, lang_rules: &[(&str, &str)]) {
        fs::write(self.path().join("CLAUDE.md.jinja"), template).unwrap();
        if !lang_rules.is_empty() {
            let dir = self.path().join("claude-md/lang-rules");
            fs::create_dir_all(&dir).unwrap();
            for (name, content) in lang_rules {
                fs::write(dir.join(name), content).unwrap();
            }
        }
    }

    /// Write misc files into claude-md/misc/.
    pub fn with_misc_files(&self, files: &[(&str, &str)]) {
        let dir = self.path().join("claude-md/misc");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            fs::write(dir.join(name), content).unwrap();
        }
    }

    /// Write MCP rules files into claude-md/mcp-rules/.
    pub fn with_mcp_rules(&self, files: &[(&str, &str)]) {
        let dir = self.path().join("claude-md/mcp-rules");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            fs::write(dir.join(name), content).unwrap();
        }
    }

    // ── Hooks ────────────────────────────────────────────────────────

    /// Write default and/or named hook files.
    pub fn with_default_hooks(&self, hooks: &[(&str, &str)]) {
        let dir = self.path().join("hooks/default");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in hooks {
            fs::write(dir.join(format!("{}.json", name)), content).unwrap();
        }
    }

    pub fn with_named_hooks(&self, hooks: &[(&str, &str)]) {
        let dir = self.path().join("hooks");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in hooks {
            fs::write(dir.join(format!("{}.json", name)), content).unwrap();
        }
    }

    // ── Settings ─────────────────────────────────────────────────────

    pub fn with_settings(&self, content: &str) {
        fs::write(self.path().join("settings.local.json"), content).unwrap();
    }

    // ── MCP servers ──────────────────────────────────────────────────

    pub fn with_default_mcps(&self, servers: &[(&str, &str)]) {
        let dir = self.path().join("mcp/default");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in servers {
            fs::write(dir.join(format!("{}.json", name)), content).unwrap();
        }
    }

    pub fn with_lang_mcps(&self, lang: &str, servers: &[(&str, &str)]) {
        let dir = self.path().join("mcp").join(lang);
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in servers {
            fs::write(dir.join(format!("{}.json", name)), content).unwrap();
        }
    }

    pub fn with_named_mcps(&self, servers: &[(&str, &str)]) {
        let dir = self.path().join("mcp");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in servers {
            fs::write(dir.join(format!("{}.json", name)), content).unwrap();
        }
    }

    // ── Commands / Skills / Copied ───────────────────────────────────

    pub fn with_commands(&self, subdir: &str, files: &[(&str, &str)]) {
        let dir = self.path().join("commands").join(subdir);
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            fs::write(dir.join(name), content).unwrap();
        }
    }

    pub fn with_skills(&self, subdir: &str, files: &[(&str, &str)]) {
        let dir = self.path().join("skills").join(subdir);
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            fs::write(dir.join(name), content).unwrap();
        }
    }

    pub fn with_copied(&self, subdir: &str, files: &[(&str, &str)]) {
        let dir = self.path().join("copied").join(subdir);
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            fs::write(dir.join(name), content).unwrap();
        }
    }

    pub fn with_named_commands(&self, files: &[(&str, &str)]) {
        let dir = self.path().join("commands");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            fs::write(dir.join(format!("{}.md", name)), content).unwrap();
        }
    }

    // ── Clarg configs ────────────────────────────────────────────────

    pub fn with_clarg_configs(&self, configs: &[(&str, &str)]) {
        let dir = self.path().join("clarg");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in configs {
            fs::write(dir.join(format!("{}.yaml", name)), content).unwrap();
        }
    }

    // ── Gitignore ────────────────────────────────────────────────────

    /// Write the always-on fragment at `gitignore-additions/default.gitignore`.
    pub fn with_gitignore_additions(&self, content: &str) {
        let dir = self.path().join("gitignore-additions");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("default.gitignore"), content).unwrap();
    }

    /// Write a per-language fragment at `gitignore-additions/<lang>.gitignore`.
    pub fn with_gitignore_for_lang(&self, lang: &str, content: &str) {
        let dir = self.path().join("gitignore-additions");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{}.gitignore", lang)), content).unwrap();
    }
}

/// RAII guard that changes cwd under the global lock and restores on drop.
pub struct CwdGuard {
    prev: PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl CwdGuard {
    pub fn new(path: &Path) -> Self {
        let lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();
        Self { prev, _lock: lock }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.prev);
    }
}

/// Sets up a temp workdir with a fake CLONE_DIR containing a
/// `gitignore-additions/default.gitignore` fragment.
pub fn setup_gitignore_test(existing: Option<&str>, additions: &str) -> (TempDir, CwdGuard) {
    setup_gitignore_test_with_langs(existing, Some(additions), &[])
}

/// Full-control variant: optionally seed a default fragment and any number of
/// per-language fragments inside `CLONE_DIR/gitignore-additions/`.
pub fn setup_gitignore_test_with_langs(
    existing: Option<&str>,
    default: Option<&str>,
    lang_fragments: &[(&str, &str)],
) -> (TempDir, CwdGuard) {
    let workdir = TempDir::new().unwrap();

    let additions_dir = workdir.path().join(CLONE_DIR).join("gitignore-additions");
    fs::create_dir_all(&additions_dir).unwrap();

    if let Some(content) = default {
        fs::write(additions_dir.join("default.gitignore"), content).unwrap();
    }
    for (lang, content) in lang_fragments {
        fs::write(
            additions_dir.join(format!("{}.gitignore", lang)),
            content,
        )
        .unwrap();
    }

    if let Some(content) = existing {
        fs::write(workdir.path().join(".gitignore"), content).unwrap();
    }

    let guard = CwdGuard::new(workdir.path());
    (workdir, guard)
}

/// RAII guard that overwrites `PATH` for the test (must be created while a
/// `CwdGuard` is held — both share the same global lock for serialization).
pub struct PathGuard {
    prev: Option<String>,
}

impl PathGuard {
    /// Replace PATH with `dir:/usr/bin:/bin` so subprocesses see only the test's
    /// fake shims plus the standard system tools (`which`, `git`, etc.).
    pub fn replace_with(dir: &Path) -> Self {
        let prev = env::var("PATH").ok();
        // Safety: tests serialize global state via `CWD_LOCK` (held by `CwdGuard`).
        unsafe { env::set_var("PATH", format!("{}:/usr/bin:/bin", dir.display())); }
        Self { prev }
    }

    /// Replace PATH with just `/usr/bin:/bin` (no test shim) so `claude` cannot
    /// be located even by `which`.
    pub fn system_only() -> Self {
        let prev = env::var("PATH").ok();
        unsafe { env::set_var("PATH", "/usr/bin:/bin"); }
        Self { prev }
    }
}

impl Drop for PathGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.prev {
                Some(p) => env::set_var("PATH", p),
                None => env::remove_var("PATH"),
            }
        }
    }
}

/// RAII guard for environment variables. Restores prior values on drop.
pub struct EnvVarGuard {
    keys: Vec<(String, Option<String>)>,
}

impl EnvVarGuard {
    pub fn new() -> Self {
        Self { keys: vec![] }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let prev = env::var(key).ok();
        self.keys.push((key.to_string(), prev));
        unsafe { env::set_var(key, value); }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        for (k, v) in self.keys.drain(..) {
            unsafe {
                match v {
                    Some(val) => env::set_var(&k, val),
                    None => env::remove_var(&k),
                }
            }
        }
    }
}

/// Install a fake `claude` shell script into `bindir`. Behavior is controlled
/// at run time by environment variables:
///   - `FAKE_CLAUDE_EXIT`: exit code (default `0`)
///   - `FAKE_CLAUDE_TARGET`: file path to overwrite (optional)
///   - `FAKE_CLAUDE_CONTENT`: bytes to write into TARGET (default `MERGED`)
pub fn install_fake_claude(bindir: &Path) {
    fs::create_dir_all(bindir).unwrap();
    let script = bindir.join("claude");
    let body = r#"#!/bin/sh
if [ -n "$FAKE_CLAUDE_TARGET" ]; then
    printf '%s' "${FAKE_CLAUDE_CONTENT:-MERGED}" > "$FAKE_CLAUDE_TARGET"
fi
exit "${FAKE_CLAUDE_EXIT:-0}"
"#;
    fs::write(&script, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
}
