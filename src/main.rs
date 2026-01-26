//! clemp - CLI tool to clone and configure the claude-template repository.
//! Clones a configurable GitHub repo, renders CLAUDE.md with language-specific
//! rules, configures hooks and MCP servers, then copies files to the working directory.

use anyhow::{bail, Context, Result};
use clap::Parser;
use minijinja::{context, Environment};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::{env, fs};
use std::process::Command;

const CLONE_DIR: &str = "claude-template";

#[derive(Serialize, Deserialize, Default)]
struct Config {
    #[serde(rename = "gh-repo")]
    gh_repo: Option<String>,
}

fn config_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".config/clemp/clemp.yaml"))
}

fn load_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))
}

fn save_config(config: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_yaml::to_string(config)?;
    fs::write(&path, content)?;
    Ok(())
}

fn prompt_for_repo() -> Result<String> {
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

fn get_repo_url() -> Result<String> {
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

#[derive(Parser)]
#[command(version, about = "Clone and configure claude-template for your project", disable_version_flag = true)]
struct Cli {
    /// Print version
    #[arg(short = 'v', short_alias = 'V', long = "version", action = clap::ArgAction::Version)]
    version: (),

    /// Language(s) for rules (e.g., ts, typescript, py, python, swift)
    #[arg(value_name = "LANGUAGE")]
    languages: Vec<String>,

    /// Hook names to include (comma or space separated)
    #[arg(long, value_delimiter = ',', default_value = "sound")]
    hooks: Vec<String>,

    /// MCP server names to keep (comma or space separated)
    #[arg(long, value_delimiter = ',', default_value = "context7")]
    mcp: Vec<String>,
}

fn normalize_language(input: &str) -> Option<&'static str> {
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

fn clone_repo(repo_url: &str) -> Result<()> {
    if Path::new(CLONE_DIR).exists() {
        bail!("Directory '{}' already exists. Remove it first.", CLONE_DIR);
    }

    let status = Command::new("git")
        .args(["clone", "--depth=1", repo_url, CLONE_DIR])
        .status()
        .context("Failed to execute git clone")?;

    if !status.success() {
        bail!("git clone failed with status: {}", status);
    }
    Ok(())
}

fn update_gitignore() -> Result<()> {
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

fn template_has_conditional(template_content: &str, lang: &str) -> bool {
    // Check for patterns like {% if "swift" in languages %} or {%- if "swift" in languages %}
    let patterns = [
        format!(r#""{}" in languages"#, lang),
        format!(r#"'{}' in languages"#, lang),
    ];
    patterns.iter().any(|p| template_content.contains(p))
}

enum LanguageResolution {
    HasRulesFile(String),      // canonical name, has rules file
    ConditionalOnly(String),   // canonical name, only has conditional in template
    NoMatch,                   // no rules file or conditional - skip with warning
}

fn resolve_language(input: &str, rules_dir: &Path, template_content: &str) -> LanguageResolution {
    let canonical = normalize_language(input)
        .map(String::from)
        .unwrap_or_else(|| input.to_lowercase());

    let rules_file = rules_dir.join(format!("{}-rules.md", canonical));

    if rules_file.exists() {
        LanguageResolution::HasRulesFile(canonical)
    } else if template_has_conditional(template_content, &canonical) {
        eprintln!(
            "Warning: No rules file for '{}', but template has conditional sections for it",
            canonical
        );
        LanguageResolution::ConditionalOnly(canonical)
    } else {
        eprintln!(
            "Warning: Language '{}' has no rules file ({}-rules.md) and no conditional sections in template, skipping",
            input,
            canonical
        );
        LanguageResolution::NoMatch
    }
}

fn build_language_rules(languages_with_rules: &[String], rules_dir: &Path) -> Result<String> {
    let mut sections = Vec::new();

    for canonical in languages_with_rules {
        let rules_file = rules_dir.join(format!("{}-rules.md", canonical));
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

fn render_claude_md(languages: &[String], rules_dir: &Path) -> Result<String> {
    let template_path = rules_dir.join("CLAUDE-template.md");
    let template_content = fs::read_to_string(&template_path)
        .with_context(|| format!("Failed to read {}", template_path.display()))?;

    // Resolve all languages and separate those with rules files
    let mut all_languages = Vec::new();
    let mut languages_with_rules = Vec::new();

    for lang in languages {
        match resolve_language(lang, rules_dir, &template_content) {
            LanguageResolution::HasRulesFile(canonical) => {
                all_languages.push(canonical.clone());
                languages_with_rules.push(canonical);
            }
            LanguageResolution::ConditionalOnly(canonical) => {
                all_languages.push(canonical);
            }
            LanguageResolution::NoMatch => {
                // Skip - warning already printed
            }
        }
    }

    let language_rules = build_language_rules(&languages_with_rules, rules_dir)?;

    let mut env = Environment::new();
    env.add_template("claude", &template_content)
        .context("Failed to add template")?;

    let tmpl = env.get_template("claude").context("Failed to get template")?;

    let rendered = tmpl
        .render(context! {
            languages => all_languages,
            language_rules => language_rules,
        })
        .context("Failed to render template")?;

    Ok(rendered)
}

fn update_settings_with_hooks(hooks: &[String], clone_dir: &Path) -> Result<()> {
    let settings_path = clone_dir.join(".claude/settings.local.json");
    let hooks_dir = clone_dir.join("hooks-template");

    let mut settings: Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).context("Failed to parse settings.local.json")?
    } else {
        serde_json::json!({})
    };

    let settings_obj = settings
        .as_object_mut()
        .context("settings.local.json is not an object")?;

    // Start with empty hooks object
    let mut merged_hooks: Map<String, Value> = Map::new();

    for hook_name in hooks {
        let hook_file = hooks_dir.join(format!("{}.json", hook_name));
        if !hook_file.exists() {
            bail!(
                "Hook '{}' not found in {}",
                hook_name,
                hooks_dir.display()
            );
        }

        let hook_content = fs::read_to_string(&hook_file)?;
        let hook_json: Value =
            serde_json::from_str(&hook_content).with_context(|| format!("Failed to parse {}", hook_file.display()))?;

        let hook_obj = hook_json
            .as_object()
            .with_context(|| format!("{} is not an object", hook_file.display()))?;

        // Merge each hook type (e.g., PreToolUse, Notification)
        for (hook_type, hook_entries) in hook_obj {
            let entries = hook_entries
                .as_array()
                .with_context(|| format!("'{}' in {} is not an array", hook_type, hook_file.display()))?;

            merged_hooks
                .entry(hook_type.clone())
                .or_insert_with(|| Value::Array(vec![]))
                .as_array_mut()
                .unwrap()
                .extend(entries.clone());
        }
    }

    settings_obj.insert("hooks".to_string(), Value::Object(merged_hooks));

    fs::write(&settings_path, serde_json::to_string_pretty(&settings)?)?;
    Ok(())
}

fn filter_mcp_json(mcp_servers: &[String], clone_dir: &Path) -> Result<()> {
    let mcp_path = clone_dir.join(".mcp.json");
    if !mcp_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&mcp_path)?;
    let mut mcp_json: Value =
        serde_json::from_str(&content).context("Failed to parse .mcp.json")?;

    let mcp_obj = mcp_json
        .as_object_mut()
        .context(".mcp.json is not an object")?;

    let servers = mcp_obj
        .get_mut("mcpServers")
        .and_then(|v| v.as_object_mut())
        .context(".mcp.json missing mcpServers object")?;

    // Verify all requested servers exist
    for server in mcp_servers {
        if !servers.contains_key(server) {
            bail!(
                "MCP server '{}' not found in .mcp.json. Available: {:?}",
                server,
                servers.keys().collect::<Vec<_>>()
            );
        }
    }

    // Keep only requested servers
    let to_keep: HashSet<&str> = mcp_servers.iter().map(String::as_str).collect();
    servers.retain(|k, _| to_keep.contains(k.as_str()));

    if servers.is_empty() {
        fs::remove_file(&mcp_path)?;
    } else {
        fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_json)?)?;
    }

    Ok(())
}

fn copy_files(clone_dir: &Path) -> Result<()> {
    let exclude = [
        ".git",
        "hooks-template",
        "rules-templates",
        "README.md",
        "gitignore-additions",
    ];

    for entry in fs::read_dir(clone_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if exclude.contains(&name_str.as_ref()) {
            continue;
        }

        let src = entry.path();
        let dest = Path::new(".").join(&name);

        if src.is_dir() {
            copy_dir_recursive(&src, &dest)?;
        } else {
            fs::copy(&src, &dest)
                .with_context(|| format!("Failed to copy {} to {}", src.display(), dest.display()))?;
        }
    }

    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
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

fn cleanup(clone_dir: &Path) -> Result<()> {
    fs::remove_dir_all(clone_dir)
        .with_context(|| format!("Failed to remove {}", clone_dir.display()))?;
    Ok(())
}

fn run_setup(cli: &Cli, clone_dir: &Path, rules_dir: &Path) -> Result<()> {
    // Update .gitignore
    println!("Updating .gitignore...");
    update_gitignore()?;

    // Render CLAUDE.md
    println!("Rendering CLAUDE.md...");
    let claude_md = render_claude_md(&cli.languages, rules_dir)?;
    let claude_path = clone_dir.join("CLAUDE.md");
    fs::write(&claude_path, claude_md)?;

    // Update settings with hooks
    println!("Configuring hooks: {:?}", cli.hooks);
    update_settings_with_hooks(&cli.hooks, clone_dir)?;

    // Filter .mcp.json
    println!("Configuring MCP servers: {:?}", cli.mcp);
    filter_mcp_json(&cli.mcp, clone_dir)?;

    // Copy files to working directory
    println!("Copying files...");
    copy_files(clone_dir)?;

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let clone_dir = Path::new(CLONE_DIR);
    let rules_dir = clone_dir.join("rules-templates");

    // 1. Get repo URL from config (prompt if not set)
    let repo_url = get_repo_url()?;

    // 2. Clone repository
    println!("Cloning {}...", repo_url);
    clone_repo(&repo_url)?;

    // 3-7. Run setup steps (with cleanup on error)
    if let Err(e) = run_setup(&cli, clone_dir, &rules_dir) {
        eprintln!("Removing {} due to error...", clone_dir.display());
        let _ = fs::remove_dir_all(clone_dir);
        return Err(e);
    }

    // 8. Cleanup (success path)
    println!("Cleaning up...");
    cleanup(clone_dir)?;

    println!("Done! Claude template configured for your project.");
    Ok(())
}