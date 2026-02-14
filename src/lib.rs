//! clemp library — core logic for cloning and configuring claude-template.
//! Provides template rendering, hook/MCP configuration, file copying, and CLI parsing.

use anyhow::{bail, Context, Result};
use clap::Parser;
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

pub const CLONE_DIR: &str = "claude-template";

// ── Config ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(rename = "gh-repo")]
    pub gh_repo: Option<String>,
}

pub fn config_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".config/clemp/clemp.yaml"))
}

pub fn load_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))
}

pub fn save_config(config: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_yaml::to_string(config)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn prompt_for_repo() -> Result<String> {
    print!("Enter GitHub repo URL for claude-template: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let url = input.trim().to_string();
    if url.is_empty() {
        bail!("Repository URL cannot be empty");
    }
    Ok(url)
}

pub fn get_repo_url() -> Result<String> {
    let mut config = load_config()?;
    if let Some(url) = &config.gh_repo {
        return Ok(url.clone());
    }
    let url = prompt_for_repo()?;
    config.gh_repo = Some(url.clone());
    save_config(&config)?;
    println!("Saved to {}", config_path()?.display());
    Ok(url)
}

// ── CLI ──────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(version, about = "Clone and configure claude-template for your project", disable_version_flag = true)]
pub struct Cli {
    /// Print version
    #[arg(short = 'v', short_alias = 'V', long = "version", action = clap::ArgAction::Version)]
    pub version: (),

    /// Language(s) for rules (e.g., ts, typescript, py, python, swift)
    #[arg(value_name = "LANGUAGE")]
    pub languages: Vec<String>,

    /// Extra hook names to include (comma or space separated)
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub hooks: Vec<String>,

    /// Extra MCP server names to include (comma or space separated)
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub mcp: Vec<String>,

    /// Clarg config profile to enable (name of a YAML file in the template's clarg/ directory)
    #[arg(long)]
    pub clarg: Option<String>,

    /// Overwrite existing files/directories in the working directory
    #[arg(long)]
    pub force: bool,
}

// ── Language handling ────────────────────────────────────────────────────

pub fn normalize_language(input: &str) -> Option<&'static str> {
    match input.to_lowercase().as_str() {
        "ts" | "typescript" => Some("typescript"),
        "js" | "javascript" => Some("javascript"),
        "rs" | "rust" => Some("rust"),
        "py" | "python" => Some("python"),
        "swift" => Some("swift"),
        "cs" | "csharp" | "c#" => Some("csharp"),
        "cpp" | "cplusplus" | "c++" => Some("cplusplus"),
        "html" => Some("html"),
        "svelte" => Some("svelte"),
        "go" | "golang" => Some("go"),
        "java" => Some("java"),
        "rb" | "ruby" => Some("ruby"),
        _ => None,
    }
}

pub enum LanguageResolution {
    HasRulesFile(String),
    ConditionalOnly(String),
    NoMatch,
}

/// Resolve a language input against the template's rules files and conditional directories.
pub fn resolve_language(input: &str, clone_dir: &Path) -> LanguageResolution {
    let canonical = normalize_language(input)
        .map(String::from)
        .unwrap_or_else(|| input.to_lowercase());

    let rules_file = clone_dir
        .join("claude-md/lang-rules")
        .join(format!("{}.md", canonical));

    if rules_file.exists() {
        return LanguageResolution::HasRulesFile(canonical);
    }

    let has_conditional = ["commands", "skills", "copied", "mcp"]
        .iter()
        .any(|dir| clone_dir.join(dir).join(&canonical).is_dir());

    if has_conditional {
        eprintln!(
            "Warning: No rules file for '{}', but has conditional directories for it",
            canonical
        );
        LanguageResolution::ConditionalOnly(canonical)
    } else {
        LanguageResolution::NoMatch
    }
}

/// Resolve all language inputs, erroring on unknown languages.
pub fn resolve_all_languages(inputs: &[String], clone_dir: &Path) -> Result<Vec<String>> {
    let mut resolved = Vec::new();
    for lang in inputs {
        match resolve_language(lang, clone_dir) {
            LanguageResolution::HasRulesFile(canonical) | LanguageResolution::ConditionalOnly(canonical) => {
                resolved.push(canonical);
            }
            LanguageResolution::NoMatch => {
                let canonical = normalize_language(lang)
                    .map(String::from)
                    .unwrap_or_else(|| lang.to_lowercase());
                bail!(
                    "Unknown language '{}': no rules file (claude-md/lang-rules/{}.md) and no conditional directories in template",
                    lang,
                    canonical
                );
            }
        }
    }
    Ok(resolved)
}

// ── Rules building ───────────────────────────────────────────────────────

pub fn build_language_rules(languages: &[String], claude_md_dir: &Path) -> Result<String> {
    let lang_rules_dir = claude_md_dir.join("lang-rules");
    let mut sections = Vec::new();

    for canonical in languages {
        let rules_file = lang_rules_dir.join(format!("{}.md", canonical));
        if !rules_file.exists() {
            continue; // ConditionalOnly languages have no rules file
        }
        let content = fs::read_to_string(&rules_file)
            .with_context(|| format!("Failed to read {}", rules_file.display()))?;

        sections.push(format!(
            "<{}-rules>\n{}\n</{}-rules>",
            canonical,
            content.trim(),
            canonical
        ));
    }

    Ok(sections.join("\n\n"))
}

pub fn build_mcp_rules(active_mcps: &[String], claude_md_dir: &Path) -> Result<String> {
    let mcp_rules_dir = claude_md_dir.join("mcp-rules");
    let mut sections = Vec::new();

    for name in active_mcps {
        let rules_file = mcp_rules_dir.join(format!("{}.md", name));
        if !rules_file.exists() {
            continue; // Not all MCPs have rules — that's fine
        }
        let content = fs::read_to_string(&rules_file)
            .with_context(|| format!("Failed to read {}", rules_file.display()))?;

        sections.push(format!(
            "<{}-mcp-rules>\n{}\n</{}-mcp-rules>",
            name,
            content.trim(),
            name
        ));
    }

    Ok(sections.join("\n\n"))
}

// ── MCP assembly ─────────────────────────────────────────────────────────

/// Read all .json files from a directory and merge their top-level key-value pairs.
fn read_json_dir(dir: &Path) -> Result<Map<String, Value>> {
    let mut merged = Map::new();
    if !dir.is_dir() {
        return Ok(merged);
    }
    let mut entries: Vec<_> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "json")
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let content = fs::read_to_string(&path)?;
        let obj: Map<String, Value> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        merged.extend(obj);
    }
    Ok(merged)
}

/// Assemble .mcp.json from default/, language, and named MCP server files.
/// Returns the assembled JSON and the list of all server names.
pub fn assemble_mcp_json(
    languages: &[String],
    named_mcps: &[String],
    clone_dir: &Path,
) -> Result<(Value, Vec<String>)> {
    let mcp_dir = clone_dir.join("mcp");

    if !mcp_dir.exists() {
        if !named_mcps.is_empty() {
            bail!("--mcp specified but no mcp/ directory in template");
        }
        return Ok((serde_json::json!({"mcpServers": {}}), vec![]));
    }

    let mut servers = Map::new();

    // 1. Default MCPs (always)
    servers.extend(read_json_dir(&mcp_dir.join("default"))?);

    // 2. Language-matched MCPs
    for lang in languages {
        servers.extend(read_json_dir(&mcp_dir.join(lang))?);
    }

    // 3. Named MCPs from --mcp flag
    for name in named_mcps {
        let path = mcp_dir.join(format!("{}.json", name));
        if !path.exists() {
            let available: Vec<_> = fs::read_dir(&mcp_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let p = e.path();
                    p.is_file() && p.extension().map_or(false, |ext| ext == "json")
                })
                .map(|e| e.path().file_stem().unwrap().to_string_lossy().to_string())
                .collect();
            bail!(
                "MCP '{}' not found in {}. Available: {:?}",
                name,
                mcp_dir.display(),
                available
            );
        }
        let content = fs::read_to_string(&path)?;
        let obj: Map<String, Value> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        servers.extend(obj);
    }

    let names: Vec<String> = servers.keys().cloned().collect();
    let mcp_json = serde_json::json!({ "mcpServers": servers });

    Ok((mcp_json, names))
}

// ── Clarg integration ────────────────────────────────────────────────

/// Copy a clarg YAML config from the template and generate a PreToolUse hook entry.
pub fn setup_clarg(name: &str, clone_dir: &Path) -> Result<Value> {
    let clarg_dir = clone_dir.join("clarg");
    let yaml_path = clarg_dir.join(format!("{}.yaml", name));

    if !yaml_path.exists() {
        let available: Vec<_> = if clarg_dir.is_dir() {
            fs::read_dir(&clarg_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map_or(false, |ext| ext == "yaml" || ext == "yml")
                })
                .map(|e| e.path().file_stem().unwrap().to_string_lossy().to_string())
                .collect()
        } else {
            vec![]
        };
        bail!(
            "Clarg config '{}' not found in {}. Available: {:?}",
            name,
            clarg_dir.display(),
            available
        );
    }

    let dest_name = format!("clarg-{}.yaml", name);
    let claude_dir = clone_dir.join(".claude");
    fs::create_dir_all(&claude_dir)?;
    fs::copy(&yaml_path, claude_dir.join(&dest_name))?;

    Ok(serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": format!("clarg .claude/{}", dest_name)
        }]
    }))
}

/// Warn if clarg is not on PATH.
pub fn check_clarg_installed() {
    let found = Command::new("which")
        .arg("clarg")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !found {
        eprintln!();
        eprintln!("Warning: 'clarg' is not installed. Clarg hooks will not work until it is.");
        eprintln!("  Install with: brew install bn-l/tap/clarg");
        eprintln!("  Or:           cargo install --git https://github.com/bn-l/clarg");
        eprintln!();
    }
}

// ── Settings / hooks ─────────────────────────────────────────────────────

/// Merge hook entries from a JSON file into the accumulated hooks map.
fn merge_hook_file(path: &Path, dest: &mut Map<String, Value>) -> Result<()> {
    let content = fs::read_to_string(path)?;
    let hook_json: Value =
        serde_json::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;
    let hook_obj = hook_json
        .as_object()
        .with_context(|| format!("{} is not an object", path.display()))?;

    for (hook_type, hook_entries) in hook_obj {
        let entries = hook_entries
            .as_array()
            .with_context(|| format!("'{}' in {} is not an array", hook_type, path.display()))?;
        dest.entry(hook_type.clone())
            .or_insert_with(|| Value::Array(vec![]))
            .as_array_mut()
            .unwrap()
            .extend(entries.clone());
    }
    Ok(())
}

/// Build .claude/settings.local.json from base settings + hooks + clarg + MCP list.
pub fn build_settings(
    named_hooks: &[String],
    clarg_entries: &[Value],
    active_mcp_names: &[String],
    clone_dir: &Path,
) -> Result<()> {
    let base_path = clone_dir.join("settings.local.json");
    let hooks_dir = clone_dir.join("hooks");

    let mut settings: Value = if base_path.exists() {
        let content = fs::read_to_string(&base_path)?;
        serde_json::from_str(&content).context("Failed to parse settings.local.json")?
    } else {
        serde_json::json!({})
    };

    let settings_obj = settings
        .as_object_mut()
        .context("settings.local.json is not an object")?;

    // Merge hooks: default/ always + named hooks
    let mut merged_hooks: Map<String, Value> = Map::new();

    let default_hooks_dir = hooks_dir.join("default");
    if default_hooks_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&default_hooks_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map_or(false, |ext| ext == "json")
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            merge_hook_file(&entry.path(), &mut merged_hooks)?;
        }
    }

    for name in named_hooks {
        let path = hooks_dir.join(format!("{}.json", name));
        if !path.exists() {
            let available: Vec<_> = fs::read_dir(&hooks_dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let p = e.path();
                    p.is_file() && p.extension().map_or(false, |ext| ext == "json")
                })
                .map(|e| e.path().file_stem().unwrap().to_string_lossy().to_string())
                .collect();
            bail!(
                "Hook '{}' not found in {}. Available: {:?}",
                name,
                hooks_dir.display(),
                available
            );
        }
        merge_hook_file(&path, &mut merged_hooks)?;
    }

    // Merge clarg PreToolUse hook entries
    for entry in clarg_entries {
        merged_hooks
            .entry("PreToolUse".to_string())
            .or_insert_with(|| Value::Array(vec![]))
            .as_array_mut()
            .unwrap()
            .push(entry.clone());
    }

    settings_obj.insert("hooks".to_string(), Value::Object(merged_hooks));

    // Update enabledMcpjsonServers
    let mcp_names: Vec<Value> = active_mcp_names
        .iter()
        .map(|n| Value::String(n.clone()))
        .collect();
    settings_obj.insert("enabledMcpjsonServers".to_string(), Value::Array(mcp_names));

    // Write to .claude/settings.local.json
    let claude_dir = clone_dir.join(".claude");
    fs::create_dir_all(&claude_dir)?;
    fs::write(
        claude_dir.join("settings.local.json"),
        serde_json::to_string_pretty(&settings)?,
    )?;

    Ok(())
}

// ── Template rendering ───────────────────────────────────────────────────

/// Render CLAUDE.md from the template and all its parts.
pub fn render_claude_md(
    languages: &[String],
    active_mcp_names: &[String],
    clone_dir: &Path,
) -> Result<String> {
    let template_path = clone_dir.join("CLAUDE.md.jinja");
    let template_content = fs::read_to_string(&template_path)
        .with_context(|| format!("Failed to read {}", template_path.display()))?;

    let claude_md_dir = clone_dir.join("claude-md");

    // Build lang dict: {"typescript": true, ...} — truthy if non-empty, dot-accessible
    let lang_dict: BTreeMap<&str, bool> = languages.iter().map(|l| (l.as_str(), true)).collect();

    // Build mcp dict: {"context7": true, ...}
    let mcp_dict: BTreeMap<&str, bool> = active_mcp_names.iter().map(|m| (m.as_str(), true)).collect();

    // Build lang_rules and mcp_rules
    let lang_rules = build_language_rules(languages, &claude_md_dir)?;
    let mcp_rules = build_mcp_rules(active_mcp_names, &claude_md_dir)?;

    // Build template context as a dynamic map (supports misc variables with dynamic names)
    let mut ctx = Map::new();
    ctx.insert("lang".into(), serde_json::to_value(&lang_dict)?);
    ctx.insert("mcp".into(), serde_json::to_value(&mcp_dict)?);
    ctx.insert("lang_rules".into(), Value::String(lang_rules));
    ctx.insert("mcp_rules".into(), Value::String(mcp_rules));

    // Render misc files from claude-md/misc/
    let misc_dir = claude_md_dir.join("misc");
    if misc_dir.is_dir() {
        let env = Environment::new();
        let partial_ctx = serde_json::json!({ "lang": &lang_dict, "mcp": &mcp_dict });

        let mut entries: Vec<_> = fs::read_dir(&misc_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let filename = entry.file_name().to_string_lossy().to_string();
            let content = fs::read_to_string(entry.path())?;

            let is_jinja = filename.ends_with(".jinja");

            // Strip .jinja then .md to get tag name (keeps hyphens)
            let base = if is_jinja {
                &filename[..filename.len() - 6]
            } else {
                &filename
            };
            let tag_name = base.strip_suffix(".md").unwrap_or(base);

            // Variable name: hyphens → underscores
            let var_name = tag_name.replace('-', "_");

            let rendered = if is_jinja {
                env.render_str(&content, &partial_ctx)
                    .with_context(|| format!("Failed to render {}", filename))?
            } else {
                content
            };

            let wrapped = format!("<{}>\n{}\n</{}>", tag_name, rendered.trim(), tag_name);
            ctx.insert(var_name, Value::String(wrapped));
        }
    }

    // Render the main template
    let env = Environment::new();
    let rendered = env
        .render_str(&template_content, Value::Object(ctx))
        .context("Failed to render CLAUDE.md.jinja")?;

    Ok(rendered)
}

// ── Git / filesystem ─────────────────────────────────────────────────────

pub fn clone_repo(repo_url: &str) -> Result<()> {
    let clone_path = Path::new(CLONE_DIR);
    if clone_path.exists() {
        eprintln!("Stale '{}' directory found, removing...", CLONE_DIR);
        fs::remove_dir_all(clone_path)
            .with_context(|| format!("Failed to remove stale {}", CLONE_DIR))?;
    }

    let status = Command::new("git")
        .args(["clone", "--depth=1", repo_url, CLONE_DIR])
        .status()
        .context("Failed to execute git clone")?;

    if !status.success() {
        let _ = fs::remove_dir_all(clone_path);
        bail!("git clone failed with status: {}", status);
    }
    Ok(())
}

pub fn update_gitignore() -> Result<()> {
    let gitignore_path = Path::new(".gitignore");

    let additions_path = Path::new(CLONE_DIR).join("gitignore-additions");
    let additions = fs::read_to_string(&additions_path)
        .with_context(|| format!("Failed to read {}", additions_path.display()))?;

    let existing = if gitignore_path.exists() {
        fs::read_to_string(gitignore_path)?
    } else {
        String::new()
    };

    let existing_lines: HashSet<&str> = existing.lines().map(str::trim).collect();

    let new_entries: Vec<&str> = additions
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !existing_lines.contains(line))
        .collect();

    if new_entries.is_empty() {
        return Ok(());
    }

    let mut content = existing;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("\n# Claude related\n");
    for entry in new_entries {
        content.push_str(entry);
        content.push('\n');
    }

    fs::write(gitignore_path, content)?;
    Ok(())
}

/// Collect destination paths that already exist and would be overwritten.
pub fn collect_conflicts(sources: &[PathBuf], dest_dir: &Path) -> Vec<PathBuf> {
    sources
        .iter()
        .filter_map(|src| src.file_name())
        .map(|name| dest_dir.join(name))
        .collect::<HashSet<_>>()
        .into_iter()
        .filter(|dest| dest.exists())
        .collect()
}

/// Prompt the user for confirmation, returns true for y/yes.
pub fn confirm(message: &str) -> Result<bool> {
    print!("{} [y/N] ", message);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

pub fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }

    Ok(())
}

const COPY_FILES_EXCLUDE: &[&str] = &[
    ".git",
    "README.md",
    ".gitignore",
    "gitignore-additions",
    "CLAUDE.md.jinja",
    "claude-md",
    "clarg",
    "commands",
    "skills",
    "copied",
    "hooks",
    "mcp",
    "settings.local.json",
];

/// Collect the source paths that `copy_files` would copy to CWD.
pub fn collect_copy_files_sources(clone_dir: &Path) -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(clone_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| !COPY_FILES_EXCLUDE.contains(&e.file_name().to_string_lossy().as_ref()))
        .map(|e| e.path())
        .collect())
}

/// Collect entries from a conditional dir's default/ + lang/ subdirs.
pub fn collect_conditional_dir_sources(
    source_dir: &Path,
    languages: &[String],
) -> Vec<PathBuf> {
    if !source_dir.exists() {
        return vec![];
    }

    let mut source_dirs = vec![];
    let default_dir = source_dir.join("default");
    if default_dir.is_dir() {
        source_dirs.push(default_dir);
    }
    for lang in languages {
        let lang_dir = source_dir.join(lang);
        if lang_dir.is_dir() {
            source_dirs.push(lang_dir);
        }
    }

    source_dirs
        .iter()
        .flat_map(|dir| {
            fs::read_dir(dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
        })
        .collect()
}

pub fn copy_files(clone_dir: &Path) -> Result<()> {
    let sources = collect_copy_files_sources(clone_dir)?;

    for src in &sources {
        let dest = Path::new(".").join(src.file_name().unwrap());
        if src.is_dir() {
            copy_dir_recursive(src, &dest)?;
        } else {
            fs::copy(src, &dest)
                .with_context(|| format!("Failed to copy {} to {}", src.display(), dest.display()))?;
        }
    }

    Ok(())
}

/// Copy files from source_dir/default/ and source_dir/<lang>/ into dest_dir.
/// Language dirs override default entries with the same name.
pub fn copy_conditional_dir(
    source_dir: &Path,
    languages: &[String],
    dest_dir: &Path,
) -> Result<()> {
    if !source_dir.exists() {
        return Ok(());
    }

    let mut source_dirs = Vec::new();
    let default_dir = source_dir.join("default");
    if default_dir.is_dir() {
        source_dirs.push(default_dir);
    }
    for lang in languages {
        let lang_dir = source_dir.join(lang);
        if lang_dir.is_dir() {
            source_dirs.push(lang_dir);
        }
    }

    if source_dirs.is_empty() {
        return Ok(());
    }

    // Copy (default first, then language dirs — later entries override)
    fs::create_dir_all(dest_dir)?;
    for dir in &source_dirs {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let src = entry.path();
            let dest = dest_dir.join(entry.file_name());
            if src.is_dir() {
                copy_dir_recursive(&src, &dest)?;
            } else {
                fs::copy(&src, &dest)
                    .with_context(|| format!("Failed to copy {} to {}", src.display(), dest.display()))?;
            }
        }
    }

    Ok(())
}

pub fn cleanup(clone_dir: &Path) -> Result<()> {
    fs::remove_dir_all(clone_dir)
        .with_context(|| format!("Failed to remove {}", clone_dir.display()))?;
    Ok(())
}

// ── Orchestration ────────────────────────────────────────────────────────

pub fn run_setup(cli: &Cli, clone_dir: &Path) -> Result<()> {
    // ── Phase 1: clone_dir prep (no CWD mutations) ──────────────────────

    println!("Resolving languages...");
    let resolved_languages = resolve_all_languages(&cli.languages, clone_dir)?;

    println!("Assembling MCP servers...");
    let (mcp_json, active_mcps) = assemble_mcp_json(&resolved_languages, &cli.mcp, clone_dir)?;
    fs::write(
        clone_dir.join(".mcp.json"),
        serde_json::to_string_pretty(&mcp_json)?,
    )?;

    println!("Rendering CLAUDE.md...");
    let claude_md = render_claude_md(&resolved_languages, &active_mcps, clone_dir)?;
    fs::write(clone_dir.join("CLAUDE.md"), claude_md)?;

    let clarg_name = cli.clarg.clone().or_else(|| {
        clone_dir.join("clarg/default.yaml").exists().then(|| "default".into())
    });
    let clarg_entries: Vec<Value> = if let Some(name) = &clarg_name {
        println!("Setting up clarg...");
        vec![setup_clarg(name, clone_dir)?]
    } else {
        vec![]
    };

    println!("Building settings...");
    build_settings(&cli.hooks, &clarg_entries, &active_mcps, clone_dir)?;

    if clarg_name.is_some() {
        check_clarg_installed();
    }

    println!("Assembling commands...");
    copy_conditional_dir(
        &clone_dir.join("commands"),
        &resolved_languages,
        &clone_dir.join(".claude/commands"),
    )?;

    println!("Assembling skills...");
    copy_conditional_dir(
        &clone_dir.join("skills"),
        &resolved_languages,
        &clone_dir.join(".claude/skills"),
    )?;

    // ── Phase 2: pre-flight conflict check (bail before any CWD writes) ─

    println!("Checking for conflicts...");
    let mut all_cwd_targets = collect_copy_files_sources(clone_dir)?;
    all_cwd_targets.extend(collect_conditional_dir_sources(
        &clone_dir.join("copied"),
        &resolved_languages,
    ));
    let conflicts = collect_conflicts(&all_cwd_targets, Path::new("."));

    if !conflicts.is_empty() {
        let names: Vec<_> = conflicts.iter().map(|p| p.display().to_string()).collect();

        if !cli.force {
            bail!(
                "The following files/directories already exist and would be overwritten:\n  {}\nRemove them first, run from a clean directory, or use --force.",
                names.join("\n  ")
            );
        }

        println!(
            "The following files/directories will be overwritten:\n  {}",
            names.join("\n  ")
        );
        if !confirm("Proceed?")? {
            bail!("Aborted.");
        }
        for path in &conflicts {
            if path.is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
    }

    // ── Phase 3: CWD mutations (conflicts already cleared) ──────────────

    println!("Updating .gitignore...");
    update_gitignore()?;

    println!("Copying files...");
    copy_files(clone_dir)?;

    println!("Copying language-specific files...");
    copy_conditional_dir(
        &clone_dir.join("copied"),
        &resolved_languages,
        Path::new("."),
    )?;

    Ok(())
}

/// Split values on whitespace in addition to clap's comma delimiter.
pub fn split_multi_values(values: Vec<String>) -> Vec<String> {
    values
        .iter()
        .flat_map(|v| v.split_whitespace())
        .map(String::from)
        .collect()
}
