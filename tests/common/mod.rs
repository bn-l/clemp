//! Shared test helpers: scaffold for fake clone directories and CWD guard.

use clemp::CLONE_DIR;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::TempDir;

/// Global mutex to serialize tests that change the process working directory.
pub static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Scaffolds a fake clone directory with configurable template files.
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

    pub fn with_rules_template(&self, template: &str, rules: &[(&str, &str)]) {
        let rules_dir = self.path().join("rules-templates");
        fs::create_dir_all(&rules_dir).unwrap();
        fs::write(rules_dir.join("CLAUDE-template.md"), template).unwrap();
        for (name, content) in rules {
            fs::write(rules_dir.join(name), content).unwrap();
        }
    }

    pub fn with_hooks(&self, hooks: &[(&str, &str)]) {
        let hooks_dir = self.path().join("hooks-template");
        fs::create_dir_all(&hooks_dir).unwrap();
        for (name, content) in hooks {
            fs::write(hooks_dir.join(format!("{}.json", name)), content).unwrap();
        }
    }

    pub fn with_settings(&self, content: &str) {
        let settings_dir = self.path().join(".claude");
        fs::create_dir_all(&settings_dir).unwrap();
        fs::write(settings_dir.join("settings.local.json"), content).unwrap();
    }

    pub fn with_mcp(&self, content: &str) {
        fs::write(self.path().join(".mcp.json"), content).unwrap();
    }

    pub fn with_gitignore_additions(&self, content: &str) {
        fs::write(self.path().join("gitignore-additions"), content).unwrap();
    }

    pub fn with_lang_files(&self, lang: &str, files: &[(&str, &str)]) {
        let lang_dir = self.path().join("lang-files").join(lang);
        fs::create_dir_all(&lang_dir).unwrap();
        for (name, content) in files {
            fs::write(lang_dir.join(name), content).unwrap();
        }
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
