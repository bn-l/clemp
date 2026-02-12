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

    // ── Clarg configs ────────────────────────────────────────────────

    pub fn with_clarg_configs(&self, configs: &[(&str, &str)]) {
        let dir = self.path().join("clarg");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in configs {
            fs::write(dir.join(format!("{}.yaml", name)), content).unwrap();
        }
    }

    // ── Gitignore ────────────────────────────────────────────────────

    pub fn with_gitignore_additions(&self, content: &str) {
        fs::write(self.path().join("gitignore-additions"), content).unwrap();
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

/// Sets up a temp workdir with a fake CLONE_DIR containing gitignore-additions.
pub fn setup_gitignore_test(existing: Option<&str>, additions: &str) -> (TempDir, CwdGuard) {
    let workdir = TempDir::new().unwrap();

    let clone = workdir.path().join(CLONE_DIR);
    fs::create_dir_all(&clone).unwrap();
    fs::write(clone.join("gitignore-additions"), additions).unwrap();

    if let Some(content) = existing {
        fs::write(workdir.path().join(".gitignore"), content).unwrap();
    }

    let guard = CwdGuard::new(workdir.path());
    (workdir, guard)
}
