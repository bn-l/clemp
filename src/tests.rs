// Tests for clemp's core functions.

use super::*;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use tempfile::TempDir;

/// Global mutex to serialize tests that change the process working directory.
static CWD_LOCK: Mutex<()> = Mutex::new(());

// ── split_multi_values ──────────────────────────────────────────────────

#[test]
fn split_multi_values_comma_only() {
    let input = vec!["a,b,c".into()];
    // clap's value_delimiter already splits commas, so each arrives separately.
    // But if a value sneaks through with spaces, split_multi_values handles it.
    assert_eq!(split_multi_values(input), vec!["a,b,c"]);
    // commas aren't split by split_multi_values — clap does that.
}

#[test]
fn split_multi_values_space_separated() {
    let input = vec!["context7 sequential-thinking".into()];
    assert_eq!(
        split_multi_values(input),
        vec!["context7", "sequential-thinking"]
    );
}

#[test]
fn split_multi_values_mixed() {
    // clap splits on comma first, so we might get ["a", "b c"]
    let input = vec!["a".into(), "b c".into()];
    assert_eq!(split_multi_values(input), vec!["a", "b", "c"]);
}

#[test]
fn split_multi_values_extra_whitespace() {
    let input = vec!["  foo   bar  ".into()];
    assert_eq!(split_multi_values(input), vec!["foo", "bar"]);
}

#[test]
fn split_multi_values_empty_string() {
    let input = vec!["".into()];
    assert_eq!(split_multi_values(input), Vec::<String>::new());
}

#[test]
fn split_multi_values_tabs_and_newlines() {
    let input = vec!["a\tb\nc".into()];
    assert_eq!(split_multi_values(input), vec!["a", "b", "c"]);
}

#[test]
fn split_multi_values_single_value_no_split() {
    let input = vec!["context7".into()];
    assert_eq!(split_multi_values(input), vec!["context7"]);
}

// ── normalize_language ──────────────────────────────────────────────────

#[test]
fn normalize_known_aliases() {
    assert_eq!(normalize_language("ts"), Some("typescript"));
    assert_eq!(normalize_language("typescript"), Some("typescript"));
    assert_eq!(normalize_language("TS"), Some("typescript"));
    assert_eq!(normalize_language("js"), Some("javascript"));
    assert_eq!(normalize_language("rs"), Some("rust"));
    assert_eq!(normalize_language("py"), Some("python"));
    assert_eq!(normalize_language("cs"), Some("csharp"));
    assert_eq!(normalize_language("c#"), Some("csharp"));
    assert_eq!(normalize_language("cpp"), Some("cplusplus"));
    assert_eq!(normalize_language("c++"), Some("cplusplus"));
    assert_eq!(normalize_language("rb"), Some("ruby"));
    assert_eq!(normalize_language("golang"), Some("go"));
    assert_eq!(normalize_language("swift"), Some("swift"));
    assert_eq!(normalize_language("html"), Some("html"));
    assert_eq!(normalize_language("svelte"), Some("svelte"));
    assert_eq!(normalize_language("java"), Some("java"));
}

#[test]
fn normalize_case_insensitive() {
    assert_eq!(normalize_language("PYTHON"), Some("python"));
    assert_eq!(normalize_language("GoLang"), Some("go"));
    assert_eq!(normalize_language("RuBy"), Some("ruby"));
}

#[test]
fn normalize_unknown_language() {
    assert_eq!(normalize_language("brainfuck"), None);
    assert_eq!(normalize_language(""), None);
    assert_eq!(normalize_language("zig"), None);
}

// ── template_has_conditional ────────────────────────────────────────────

#[test]
fn template_conditional_double_quotes() {
    let tmpl = r#"{% if "swift" in languages %}swift stuff{% endif %}"#;
    assert!(template_has_conditional(tmpl, "swift"));
    assert!(!template_has_conditional(tmpl, "rust"));
}

#[test]
fn template_conditional_single_quotes() {
    let tmpl = "{% if 'rust' in languages %}rust stuff{% endif %}";
    assert!(template_has_conditional(tmpl, "rust"));
}

#[test]
fn template_conditional_with_dash_syntax() {
    let tmpl = r#"{%- if "go" in languages -%}go stuff{%- endif -%}"#;
    assert!(template_has_conditional(tmpl, "go"));
}

#[test]
fn template_conditional_no_match() {
    assert!(!template_has_conditional("no conditionals here", "rust"));
    assert!(!template_has_conditional("", "rust"));
}

// ── Helper: scaffold a fake clone directory ─────────────────────────────

struct Scaffold {
    dir: TempDir,
}

impl Scaffold {
    fn new() -> Self {
        Self { dir: TempDir::new().unwrap() }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn with_rules_template(&self, template: &str, rules: &[(&str, &str)]) {
        let rules_dir = self.path().join("rules-templates");
        fs::create_dir_all(&rules_dir).unwrap();
        fs::write(rules_dir.join("CLAUDE-template.md"), template).unwrap();
        for (name, content) in rules {
            fs::write(rules_dir.join(name), content).unwrap();
        }
    }

    fn with_hooks(&self, hooks: &[(&str, &str)]) {
        let hooks_dir = self.path().join("hooks-template");
        fs::create_dir_all(&hooks_dir).unwrap();
        for (name, content) in hooks {
            fs::write(hooks_dir.join(format!("{}.json", name)), content).unwrap();
        }
    }

    fn with_settings(&self, content: &str) {
        let settings_dir = self.path().join(".claude");
        fs::create_dir_all(&settings_dir).unwrap();
        fs::write(settings_dir.join("settings.local.json"), content).unwrap();
    }

    fn with_mcp(&self, content: &str) {
        fs::write(self.path().join(".mcp.json"), content).unwrap();
    }

    fn with_gitignore_additions(&self, content: &str) {
        fs::write(self.path().join("gitignore-additions"), content).unwrap();
    }

    fn with_lang_files(&self, lang: &str, files: &[(&str, &str)]) {
        let lang_dir = self.path().join("lang-files").join(lang);
        fs::create_dir_all(&lang_dir).unwrap();
        for (name, content) in files {
            fs::write(lang_dir.join(name), content).unwrap();
        }
    }
}

/// RAII guard that changes cwd under the global lock and restores on drop.
struct CwdGuard {
    prev: PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl CwdGuard {
    fn new(path: &Path) -> Self {
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

// ── resolve_language ────────────────────────────────────────────────────

#[test]
fn resolve_language_with_rules_file() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(rules_dir.join("typescript-rules.md"), "ts rules").unwrap();

    match resolve_language("ts", &rules_dir, "") {
        LanguageResolution::HasRulesFile(name) => assert_eq!(name, "typescript"),
        _ => panic!("Expected HasRulesFile"),
    }
}

#[test]
fn resolve_language_conditional_only() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();

    let template = r#"{% if "swift" in languages %}swift{% endif %}"#;
    match resolve_language("swift", &rules_dir, template) {
        LanguageResolution::ConditionalOnly(name) => assert_eq!(name, "swift"),
        _ => panic!("Expected ConditionalOnly"),
    }
}

#[test]
fn resolve_language_no_match() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();

    match resolve_language("brainfuck", &rules_dir, "") {
        LanguageResolution::NoMatch => {}
        _ => panic!("Expected NoMatch"),
    }
}

#[test]
fn resolve_language_unknown_input_falls_back_to_lowercase() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(rules_dir.join("zig-rules.md"), "zig stuff").unwrap();

    match resolve_language("ZIG", &rules_dir, "") {
        LanguageResolution::HasRulesFile(name) => assert_eq!(name, "zig"),
        _ => panic!("Expected HasRulesFile for unknown-but-present language"),
    }
}

// ── build_language_rules ────────────────────────────────────────────────

#[test]
fn build_language_rules_wraps_in_tags() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(rules_dir.join("rust-rules.md"), "  use iterators  \n").unwrap();
    fs::write(rules_dir.join("go-rules.md"), "handle errors").unwrap();

    let langs = vec!["rust".into(), "go".into()];
    let result = build_language_rules(&langs, &rules_dir).unwrap();

    assert!(result.contains("<rust-rules>\nuse iterators\n</rust-rules>"));
    assert!(result.contains("<go-rules>\nhandle errors\n</go-rules>"));
}

#[test]
fn build_language_rules_empty_list() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();

    let result = build_language_rules(&[], &rules_dir).unwrap();
    assert_eq!(result, "");
}

// ── render_claude_md ────────────────────────────────────────────────────

#[test]
fn render_claude_md_basic() {
    let s = Scaffold::new();
    s.with_rules_template(
        "Languages: {{ languages | join(', ') }}\n{{ language_rules }}",
        &[("typescript-rules.md", "Use strict mode")],
    );

    let rules_dir = s.path().join("rules-templates");
    let (rendered, resolved) = render_claude_md(&["ts".into()], &rules_dir).unwrap();

    assert_eq!(resolved, vec!["typescript"]);
    assert!(rendered.contains("Languages: typescript"));
    assert!(rendered.contains("<typescript-rules>"));
    assert!(rendered.contains("Use strict mode"));
}

#[test]
fn render_claude_md_multiple_languages() {
    let s = Scaffold::new();
    s.with_rules_template(
        "{{ languages | join(', ') }}\n{{ language_rules }}",
        &[
            ("typescript-rules.md", "ts rules"),
            ("rust-rules.md", "rs rules"),
        ],
    );

    let rules_dir = s.path().join("rules-templates");
    let (rendered, resolved) = render_claude_md(
        &["ts".into(), "rs".into()],
        &rules_dir,
    ).unwrap();

    assert_eq!(resolved, vec!["typescript", "rust"]);
    assert!(rendered.contains("typescript, rust"));
    assert!(rendered.contains("<typescript-rules>"));
    assert!(rendered.contains("<rust-rules>"));
}

#[test]
fn render_claude_md_skips_unknown_language() {
    let s = Scaffold::new();
    s.with_rules_template("{{ languages | join(', ') }}", &[]);

    let rules_dir = s.path().join("rules-templates");
    let (rendered, resolved) = render_claude_md(&["nope".into()], &rules_dir).unwrap();

    assert!(resolved.is_empty());
    assert_eq!(rendered.trim(), "");
}

#[test]
fn render_claude_md_conditional_only_language_in_list() {
    let s = Scaffold::new();
    s.with_rules_template(
        r#"{% if "swift" in languages %}HAS_SWIFT{% endif %} {{ languages | join(', ') }}"#,
        &[],
    );

    let rules_dir = s.path().join("rules-templates");
    let (rendered, resolved) = render_claude_md(&["swift".into()], &rules_dir).unwrap();

    assert_eq!(resolved, vec!["swift"]);
    assert!(rendered.contains("HAS_SWIFT"));
}

// ── update_settings_with_hooks ──────────────────────────────────────────

#[test]
fn hooks_merge_into_settings() {
    let s = Scaffold::new();
    s.with_settings(r#"{"permissions": {"allow": []}}"#);
    s.with_hooks(&[
        ("sound", r#"{"Notification": [{"command": "play-sound"}]}"#),
        ("lint", r#"{"PreToolUse": [{"command": "lint"}], "Notification": [{"command": "notify-lint"}]}"#),
    ]);

    update_settings_with_hooks(&["sound".into(), "lint".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();

    // permissions preserved
    assert!(val["permissions"]["allow"].is_array());

    // hooks merged
    let notif = val["hooks"]["Notification"].as_array().unwrap();
    assert_eq!(notif.len(), 2);
    let pre = val["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 1);
}

#[test]
fn hooks_missing_file_errors() {
    let s = Scaffold::new();
    s.with_settings("{}");
    s.with_hooks(&[]);

    let result = update_settings_with_hooks(&["nonexistent".into()], s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn hooks_creates_settings_if_absent() {
    let s = Scaffold::new();
    fs::create_dir_all(s.path().join(".claude")).unwrap();
    s.with_hooks(&[("sound", r#"{"Notification": [{"command": "beep"}]}"#)]);

    update_settings_with_hooks(&["sound".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(val["hooks"]["Notification"].as_array().unwrap().len(), 1);
}

#[test]
fn hooks_empty_list_writes_empty_hooks() {
    let s = Scaffold::new();
    s.with_settings(r#"{"existing": true}"#);
    s.with_hooks(&[]);

    update_settings_with_hooks(&[], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".claude/settings.local.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    assert!(val["existing"].as_bool().unwrap());
    assert!(val["hooks"].as_object().unwrap().is_empty());
}

// ── filter_mcp_json ─────────────────────────────────────────────────────

#[test]
fn filter_mcp_keeps_requested_servers() {
    let s = Scaffold::new();
    s.with_mcp(r#"{"mcpServers": {"context7": {"url": "c7"}, "other": {"url": "x"}, "third": {"url": "y"}}}"#);

    filter_mcp_json(&["context7".into(), "third".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".mcp.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    let servers = val["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 2);
    assert!(servers.contains_key("context7"));
    assert!(servers.contains_key("third"));
    assert!(!servers.contains_key("other"));
}

#[test]
fn filter_mcp_removes_file_when_empty() {
    let s = Scaffold::new();
    s.with_mcp(r#"{"mcpServers": {}}"#);

    filter_mcp_json(&[], s.path()).unwrap();
    assert!(!s.path().join(".mcp.json").exists());
}

#[test]
fn filter_mcp_unknown_server_errors() {
    let s = Scaffold::new();
    s.with_mcp(r#"{"mcpServers": {"context7": {}}}"#);

    let result = filter_mcp_json(&["doesnt-exist".into()], s.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("doesnt-exist"));
    assert!(msg.contains("not found"));
}

#[test]
fn filter_mcp_no_file_is_ok() {
    let s = Scaffold::new();
    filter_mcp_json(&["anything".into()], s.path()).unwrap();
}

#[test]
fn filter_mcp_single_server() {
    let s = Scaffold::new();
    s.with_mcp(r#"{"mcpServers": {"context7": {"cmd": "c7"}, "other": {"cmd": "x"}}}"#);

    filter_mcp_json(&["context7".into()], s.path()).unwrap();

    let content = fs::read_to_string(s.path().join(".mcp.json")).unwrap();
    let val: Value = serde_json::from_str(&content).unwrap();
    let servers = val["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 1);
    assert!(servers.contains_key("context7"));
}

// ── check_no_conflicts (needs cwd lock) ─────────────────────────────────

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

// ── update_gitignore (needs cwd lock) ───────────────────────────────────

fn setup_gitignore_test(existing: Option<&str>, additions: &str) -> (TempDir, CwdGuard) {
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
    let (workdir, _g) = setup_gitignore_test(
        Some("node_modules/\n"),
        ".claude/\nnode_modules/\n",
    );

    update_gitignore().unwrap();

    let content = fs::read_to_string(workdir.path().join(".gitignore")).unwrap();
    assert!(content.starts_with("node_modules/\n"));
    assert!(content.contains(".claude/"));
    assert_eq!(content.matches("node_modules/").count(), 1, "should not duplicate");
}

#[test]
fn gitignore_no_op_when_all_present() {
    let (workdir, _g) = setup_gitignore_test(
        Some(".claude/\n.clinerules\n"),
        ".claude/\n.clinerules\n",
    );

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
    // Should have added a newline before the section header
    assert!(content.contains("node_modules/\n\n# Claude related"));
}

// ── copy_files (needs cwd lock) ─────────────────────────────────────────

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

// ── copy_lang_files (needs cwd lock) ────────────────────────────────────

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
    ).unwrap();
    assert!(settings["hooks"]["Notification"].is_array());

    // MCP filtered
    let mcp: Value = serde_json::from_str(
        &fs::read_to_string(s.path().join(".mcp.json")).unwrap(),
    ).unwrap();
    assert!(mcp["mcpServers"]["context7"].is_object());

    // somefile.txt copied to workdir
    assert!(workdir.path().join("somefile.txt").exists());
}

// ── Error cleanup: gitignore removal ────────────────────────────────────

#[test]
fn error_cleanup_removes_new_gitignore() {
    let workdir = TempDir::new().unwrap();
    let _g = CwdGuard::new(workdir.path());

    assert!(!Path::new(".gitignore").exists());
    let gitignore_existed = Path::new(".gitignore").exists();

    // Simulate update_gitignore having created the file
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

    // Simulate the error cleanup logic from main()
    if !gitignore_existed {
        let _ = fs::remove_file(".gitignore");
    }

    assert!(Path::new(".gitignore").exists());
    assert_eq!(fs::read_to_string(".gitignore").unwrap(), "node_modules/\n");
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
