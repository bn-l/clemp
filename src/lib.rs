//! clemp library — core logic for cloning and configuring claude-template.
//! Provides template rendering, hook/MCP configuration, file copying, lockfile
//! tracking for `clemp update`, and CLI parsing.

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use minijinja::Environment;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

pub const CLONE_DIR: &str = "claude-template";
pub const LOCKFILE_NAME: &str = ".clemp-lock.yaml";

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

/// Shared argument set used by both the default setup command and `clemp update`.
/// Fields map 1:1 onto `OriginalCommand` fields stored in the lockfile.
#[derive(Args, Clone, Debug, Default)]
pub struct SetupArgs {
    /// Language(s) for rules (e.g., ts, typescript, py, python, swift)
    #[arg(value_name = "LANGUAGE")]
    pub languages: Vec<String>,

    /// Extra hook names to include (comma or space separated)
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub hooks: Vec<String>,

    /// Extra MCP server names to include (comma or space separated)
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub mcp: Vec<String>,

    /// Extra command names to include (comma or space separated)
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub commands: Vec<String>,

    /// Git hook scripts to install into .git/hooks/ (comma or space separated)
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub githooks: Vec<String>,

    /// MCP server file stems to exclude from `.mcp.json` (opts out of a default
    /// or a previously-sticky contributor). Comma or space separated.
    #[arg(long = "drop-mcp", value_delimiter = ',', num_args = 1..)]
    pub drop_mcp: Vec<String>,

    /// Hook file stems to exclude from `.claude/settings.local.json`. Comma or space separated.
    #[arg(long = "drop-hooks", value_delimiter = ',', num_args = 1..)]
    pub drop_hooks: Vec<String>,

    /// Clarg config profile to enable (name of a YAML file in the template's clarg/ directory)
    #[arg(long)]
    pub clarg: Option<String>,

    /// Overwrite existing files/directories without prompting for merge
    #[arg(long)]
    pub force: bool,
}

#[derive(Parser)]
#[command(version, about = "Clone and configure claude-template for your project", disable_version_flag = true)]
pub struct Cli {
    /// Print version
    #[arg(short = 'v', short_alias = 'V', long = "version", action = clap::ArgAction::Version)]
    pub version: (),

    #[command(subcommand)]
    pub command: Option<CliCommand>,

    /// Default setup args (used when no subcommand is given)
    #[command(flatten)]
    pub setup: SetupArgs,
}

#[derive(Subcommand)]
pub enum CliCommand {
    /// Update an existing clemp-configured project from the template (additive).
    /// Arguments are unioned into the command stored in .clemp-lock.yaml.
    Update(UpdateArgs),

    /// List available template files for a category
    /// (mcp, hooks, commands, githooks, clarg, gitignore, languages)
    List {
        /// Category to list; omit to list every category
        category: Option<String>,
    },
}

#[derive(Args, Clone, Debug)]
pub struct UpdateArgs {
    #[command(flatten)]
    pub setup: SetupArgs,

    /// Delete files the template no longer produces without prompting
    #[arg(long)]
    pub prune_stale: bool,

    /// Re-copy files that were removed from the working directory
    #[arg(long)]
    pub restore_deleted: bool,
}

// ── Lockfile ─────────────────────────────────────────────────────────────

/// Captures the invocation that produced a clemp-configured project. Mirrors the
/// public fields of `SetupArgs` minus `force` (which is runtime-only).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct OriginalCommand {
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub mcp: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub githooks: Vec<String>,
    /// MCP contributor stems the user has explicitly excluded. Persisted so the
    /// exclusion survives subsequent `clemp update` runs.
    #[serde(default, rename = "drop-mcp")]
    pub drop_mcp: Vec<String>,
    /// Hook contributor stems the user has explicitly excluded.
    #[serde(default, rename = "drop-hooks")]
    pub drop_hooks: Vec<String>,
    #[serde(default)]
    pub clarg: Option<String>,
}

impl OriginalCommand {
    pub fn from_setup(args: &SetupArgs) -> Self {
        Self {
            languages: args.languages.clone(),
            hooks: args.hooks.clone(),
            mcp: args.mcp.clone(),
            commands: args.commands.clone(),
            githooks: args.githooks.clone(),
            drop_mcp: args.drop_mcp.clone(),
            drop_hooks: args.drop_hooks.clone(),
            clarg: args.clarg.clone(),
        }
    }

    /// Produce a `SetupArgs` from this stored command. `force` is always `false`
    /// — runtime flag, not persisted.
    pub fn into_setup(self) -> SetupArgs {
        SetupArgs {
            languages: self.languages,
            hooks: self.hooks,
            mcp: self.mcp,
            commands: self.commands,
            githooks: self.githooks,
            drop_mcp: self.drop_mcp,
            drop_hooks: self.drop_hooks,
            clarg: self.clarg,
            force: false,
        }
    }

    /// Additive union with another command (used by `clemp update [args]`).
    /// `clarg` is replaced only if `other.clarg` is `Some`. Vectors are unioned
    /// preserving insertion order, skipping duplicates. Languages are deduped
    /// against their canonical form so `ts` + `typescript` don't both land in
    /// the merged command.
    ///
    /// Positive/negative reconciliation: for each (`<kind>`, `drop_<kind>`)
    /// pair, the **newer** invocation wins per stem. A stem in `other.mcp`
    /// clears any existing `drop_mcp` entry for that stem and then unions into
    /// `self.mcp` (and symmetrically the other way). Within a single
    /// invocation, the same stem appearing in both `<kind>` and `drop_<kind>`
    /// is a hard error.
    pub fn merge_additive(&mut self, other: &OriginalCommand) -> Result<()> {
        fn union(a: &mut Vec<String>, b: &[String]) {
            for item in b {
                if !a.contains(item) {
                    a.push(item.clone());
                }
            }
        }
        fn canonical_key(s: &str) -> String {
            normalize_language(s)
                .map(String::from)
                .unwrap_or_else(|| s.to_lowercase())
        }
        fn union_languages(a: &mut Vec<String>, b: &[String]) {
            let mut seen: HashSet<String> = a.iter().map(|s| canonical_key(s)).collect();
            for item in b {
                if seen.insert(canonical_key(item)) {
                    a.push(item.clone());
                }
            }
        }
        reject_add_drop_overlap(other)?;

        // Newer flag clears the opposing entry before the union.
        self.drop_mcp.retain(|s| !other.mcp.contains(s));
        self.mcp.retain(|s| !other.drop_mcp.contains(s));
        self.drop_hooks.retain(|s| !other.hooks.contains(s));
        self.hooks.retain(|s| !other.drop_hooks.contains(s));

        union_languages(&mut self.languages, &other.languages);
        union(&mut self.hooks, &other.hooks);
        union(&mut self.mcp, &other.mcp);
        union(&mut self.commands, &other.commands);
        union(&mut self.githooks, &other.githooks);
        union(&mut self.drop_mcp, &other.drop_mcp);
        union(&mut self.drop_hooks, &other.drop_hooks);
        if other.clarg.is_some() {
            self.clarg = other.clarg.clone();
        }
        Ok(())
    }
}

/// Reject invocations where the same stem appears in both `<kind>` and
/// `drop_<kind>`. Called from both initial setup (on the `SetupArgs`-derived
/// command) and `merge_additive` (on the incoming side of the merge) so the
/// "same-invocation add+drop is illegal" rule holds for every entry point.
pub fn reject_add_drop_overlap(cmd: &OriginalCommand) -> Result<()> {
    fn check(add: &[String], drop: &[String], label: &str) -> Result<()> {
        for s in add {
            if drop.contains(s) {
                bail!(
                    "'{s}' appears in both --{label} and --drop-{label} in the same invocation"
                );
            }
        }
        Ok(())
    }
    check(&cmd.mcp, &cmd.drop_mcp, "mcp")?;
    check(&cmd.hooks, &cmd.drop_hooks, "hooks")?;
    Ok(())
}

/// Symbolic snapshot of contributor *file stems* that fed aggregation outputs
/// (`.mcp.json`, `.claude/settings.local.json`) during the last setup/update.
/// Each entry is the `<stem>` of a `<kind>/**/<stem>.json` source file, not the
/// top-level server/hook key inside the rendered JSON. Used by `clemp update`
/// to keep existing projects pointed at sticky contributors even when a
/// template author reorganises where the contributor is filed.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolved {
    #[serde(default)]
    pub mcp: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<String>,
}

/// Persisted at `.clemp-lock.yaml` in the project root after a successful
/// `clemp` or `clemp update` run.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Lockfile {
    #[serde(rename = "template-repo")]
    pub template_repo: String,
    #[serde(rename = "template-sha")]
    pub template_sha: String,
    #[serde(rename = "original-command")]
    pub original_command: OriginalCommand,
    /// `None` on pre-snapshot lockfiles (missing field). Forces a full update
    /// pass the first time an old lockfile is touched so `resolved` can be
    /// populated. See `Resolved` and the migration fast-path guard in
    /// `run_update`.
    #[serde(default)]
    pub resolved: Option<Resolved>,
    /// Relative path → sha256 hex digest of the file clemp wrote there.
    /// Paths are normalized to forward-slash form for cross-platform stability.
    pub files: BTreeMap<String, String>,
}

impl Lockfile {
    pub fn path(dest_dir: &Path) -> PathBuf {
        dest_dir.join(LOCKFILE_NAME)
    }

    pub fn load(dest_dir: &Path) -> Result<Option<Self>> {
        let path = Self::path(dest_dir);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let lock: Self = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(Some(lock))
    }

    pub fn save(&self, dest_dir: &Path) -> Result<()> {
        let path = Self::path(dest_dir);
        let content = serde_yaml::to_string(self)?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }
}

/// SHA-256 hex digest of a file's bytes.
pub fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(hash_bytes(&bytes))
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Normalize a path to forward-slash form for stable lockfile keys.
pub fn lockfile_key(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            std::path::Component::CurDir => None,
            std::path::Component::ParentDir => Some("..".into()),
            std::path::Component::RootDir => None,
            std::path::Component::Prefix(_) => None,
        })
        .collect::<Vec<_>>()
        .join("/")
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

    let has_conditional_dir = ["commands", "skills", "copied", "mcp", "githooks"]
        .iter()
        .any(|dir| clone_dir.join(dir).join(&canonical).is_dir());

    let has_gitignore_fragment = clone_dir
        .join("gitignore-additions")
        .join(format!("{}.gitignore", canonical))
        .is_file();

    if has_conditional_dir || has_gitignore_fragment {
        let surface = if has_conditional_dir && has_gitignore_fragment {
            "conditional directories and a gitignore fragment"
        } else if has_conditional_dir {
            "conditional directories"
        } else {
            "a gitignore fragment"
        };
        eprintln!(
            "Warning: No rules file for '{}', but has {} for it",
            canonical, surface
        );
        LanguageResolution::ConditionalOnly(canonical)
    } else {
        LanguageResolution::NoMatch
    }
}

/// Resolve all language inputs, erroring on unknown languages. Output is
/// deduplicated by canonical name so callers never see the same language twice
/// (e.g. `ts` and `typescript` collapse to a single `typescript` entry).
pub fn resolve_all_languages(inputs: &[String], clone_dir: &Path) -> Result<Vec<String>> {
    let mut resolved = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for lang in inputs {
        match resolve_language(lang, clone_dir) {
            LanguageResolution::HasRulesFile(canonical) | LanguageResolution::ConditionalOnly(canonical) => {
                if seen.insert(canonical.clone()) {
                    resolved.push(canonical);
                }
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

// ── Contributor resolution ───────────────────────────────────────────────

/// Which directory layers a kind's template authors use to file contributors.
/// The resolver walks enabled layers in `default → lang → root` order.
pub struct LayerSpec {
    pub default: bool,
    pub languages: bool,
    pub root: bool,
}

pub const MCP_LAYERS: LayerSpec = LayerSpec { default: true, languages: true, root: true };
/// Hooks today have no `hooks/<lang>/` dir in the template; keep the resolver
/// honest. If lang-dirs for hooks are added later, flip `languages` to `true`.
pub const HOOKS_LAYERS: LayerSpec = LayerSpec { default: true, languages: false, root: true };

/// Layers consulted when validating a **fresh** positive add (a stem newly
/// appearing in `merged.<kind>`). Default and root are allowed; language is
/// blocked. Default is in because it supports the documented undrop semantics
/// of `merge_additive`: after a persisted `--drop-<kind> context7`, a newer
/// `--<kind> context7` clears the drop — but only makes sense if `context7`
/// actually exists somewhere non-language in the template, which is usually
/// `<kind>/default/`. Default-layer stems are already sticky via the default
/// loop, so accepting them via user_named doesn't introduce a new snapshot
/// surprise. Language is out because pinning a language-scoped stem as sticky
/// would survive a later language drop and contradict the "language layers
/// stay dynamic" invariant. Historical entries still use the broader
/// `MCP_LAYERS` / `HOOKS_LAYERS` so the move-fallback story for
/// previously-persisted opt-ins keeps working.
pub const FRESH_POSITIVE_LAYERS: LayerSpec =
    LayerSpec { default: true, languages: false, root: true };

/// Locate the file that contributes `stem` under `<kind>/` for the enabled
/// layers, in `default → lang → root` order. Returns `None` if no layer
/// produces a hit. `kind` is the subdirectory name (`"mcp"` or `"hooks"`) and
/// `ext` is the file extension without dot (`"json"` for both current kinds).
pub fn resolve_contributor(
    kind: &str,
    ext: &str,
    layers: &LayerSpec,
    stem: &str,
    languages: &[String],
    clone_dir: &Path,
) -> Option<PathBuf> {
    let base = clone_dir.join(kind);
    let filename = format!("{}.{}", stem, ext);
    if layers.default {
        let p = base.join("default").join(&filename);
        if p.is_file() {
            return Some(p);
        }
    }
    if layers.languages {
        for lang in languages {
            let p = base.join(lang).join(&filename);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if layers.root {
        let p = base.join(&filename);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// All contributor stems available under any enabled layer of `kind`, used for
/// error messages and drop-target validation. Deduped, insertion order: default
/// stems first, then each resolved language's stems, then root-level stems.
pub fn available_contributor_stems(
    kind: &str,
    ext: &str,
    layers: &LayerSpec,
    languages: &[String],
    clone_dir: &Path,
) -> Vec<String> {
    let base = clone_dir.join(kind);
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push_stems_from = |dir: &Path| {
        let Ok(entries) = fs::read_dir(dir) else { return };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let p = e.path();
                p.is_file() && p.extension().and_then(|e| e.to_str()) == Some(ext)
            })
            .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        names.sort();
        for n in names {
            if seen.insert(n.clone()) {
                out.push(n);
            }
        }
    };
    if layers.default {
        push_stems_from(&base.join("default"));
    }
    if layers.languages {
        for lang in languages {
            push_stems_from(&base.join(lang));
        }
    }
    if layers.root {
        push_stems_from(&base);
    }
    out
}

/// Enforce typo-safety on **fresh** contributor flags — entries in `merged.<kind>`
/// / `merged.drop_<kind>` that weren't already in `previous`. Historical entries
/// carried through from the lockfile are not re-validated (they rely on the
/// assembler's move-fallback path and, failing that, the pre-flight name-stale
/// pass).
///
/// For fresh positive adds: must resolve via `resolve_contributor` in the
/// current template.  For fresh drops: must match some known stem in the
/// current template layers, the snapshot, or the prior `original_command`.
pub fn validate_fresh_additions(
    previous: &OriginalCommand,
    merged: &OriginalCommand,
    resolved_languages: &[String],
    snapshot: Option<&Resolved>,
    clone_dir: &Path,
) -> Result<()> {
    fn fresh<'a>(a: &'a [String], b: &[String]) -> Vec<&'a str> {
        a.iter().filter(|s| !b.contains(s)).map(String::as_str).collect()
    }

    let snap_mcp: &[String] = snapshot.map(|r| r.mcp.as_slice()).unwrap_or(&[]);
    let snap_hooks: &[String] = snapshot.map(|r| r.hooks.as_slice()).unwrap_or(&[]);

    // Fresh positive adds must resolve at the default or root layer — see
    // `FRESH_POSITIVE_LAYERS` for the rationale. Language-only contributors
    // stay rejected so `--<kind> foo` can't silently pin a language-scoped
    // stem as sticky.
    let check_positive = |label: &str,
                          kind: &str,
                          ext: &str,
                          stems: &[&str]|
     -> Result<()> {
        for stem in stems {
            if resolve_contributor(
                kind,
                ext,
                &FRESH_POSITIVE_LAYERS,
                stem,
                resolved_languages,
                clone_dir,
            )
            .is_none()
            {
                let available = available_contributor_stems(
                    kind,
                    ext,
                    &FRESH_POSITIVE_LAYERS,
                    resolved_languages,
                    clone_dir,
                );
                bail!(
                    "{label} '{stem}' not found at {kind}/{stem}.{ext} or {kind}/default/{stem}.{ext}. \
                     Language-layer contributors can't be pinned via --{kind} — \
                     re-file the source at the root or default layer instead. Available: {available:?}"
                );
            }
        }
        Ok(())
    };

    let check_drop = |label: &str,
                      kind: &str,
                      ext: &str,
                      layers: &LayerSpec,
                      stems: &[&str],
                      snap: &[String],
                      prior: &[String]|
     -> Result<()> {
        for stem in stems {
            let in_template =
                resolve_contributor(kind, ext, layers, stem, resolved_languages, clone_dir)
                    .is_some();
            let in_snap = snap.iter().any(|s| s == stem);
            let in_prior = prior.iter().any(|s| s == stem);
            if !(in_template || in_snap || in_prior) {
                let mut available =
                    available_contributor_stems(kind, ext, layers, resolved_languages, clone_dir);
                for s in snap.iter().chain(prior.iter()) {
                    if !available.contains(s) {
                        available.push(s.clone());
                    }
                }
                bail!(
                    "--drop-{} '{}' does not match any known {} contributor. Known: {:?}",
                    label,
                    stem,
                    kind,
                    available
                );
            }
        }
        Ok(())
    };

    let fresh_mcp = fresh(&merged.mcp, &previous.mcp);
    let fresh_hooks = fresh(&merged.hooks, &previous.hooks);
    let fresh_drop_mcp = fresh(&merged.drop_mcp, &previous.drop_mcp);
    let fresh_drop_hooks = fresh(&merged.drop_hooks, &previous.drop_hooks);

    check_positive("MCP", "mcp", "json", &fresh_mcp)?;
    check_positive("Hook", "hooks", "json", &fresh_hooks)?;
    check_drop(
        "mcp",
        "mcp",
        "json",
        &MCP_LAYERS,
        &fresh_drop_mcp,
        snap_mcp,
        &previous.mcp,
    )?;
    check_drop(
        "hooks",
        "hooks",
        "json",
        &HOOKS_LAYERS,
        &fresh_drop_hooks,
        snap_hooks,
        &previous.hooks,
    )?;
    Ok(())
}

// ── MCP assembly ─────────────────────────────────────────────────────────

/// Result of assembling an aggregation output (`.mcp.json` or
/// `.claude/settings.local.json`'s hooks block) from layered contributors.
#[derive(Debug, Clone)]
pub struct AssemblyResult {
    /// The merged JSON value (the caller decides where to write it).
    pub rendered: Value,
    /// Top-level keys in `rendered` — for MCP this is the server names that
    /// feed `enabledMcpjsonServers` and the `mcp` jinja dict.
    pub rendered_keys: Vec<String>,
    /// Stems eligible for persistence into `lockfile.resolved.<kind>`.
    /// Excludes language-layer contributions (they re-resolve from
    /// `original_command.languages` every render) so dropping a language
    /// implicitly drops its contributors without going through the stale
    /// prompt. Default-layer and explicit-layer stems are included.
    pub snapshottable_stems: Vec<String>,
}

/// Merge the JSON body of a single contributor file into a running accumulator.
fn merge_json_file_into(path: &Path, dest: &mut Map<String, Value>) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let obj: Map<String, Value> = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    dest.extend(obj);
    Ok(())
}

/// Stems of `<ext>` files directly in `dir`, sorted lexicographically, filtered
/// through `exclude`.
fn layer_stems(dir: &Path, ext: &str, exclude: &HashSet<String>) -> Result<Vec<String>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            p.is_file() && p.extension().and_then(|e| e.to_str()) == Some(ext)
        })
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .filter(|s| !exclude.contains(s))
        .collect();
    names.sort();
    Ok(names)
}

/// Assemble `.mcp.json` from `mcp/default/`, `mcp/<lang>/`, and the explicit
/// (`user_named` ∪ `sticky_stems`) layer, filtering every layer through
/// `excluded_stems`. Returns the merged JSON, its top-level keys (server names
/// for `enabledMcpjsonServers`), and the stems eligible for persistence into
/// `lockfile.resolved.mcp`.
pub fn assemble_mcp_json(
    languages: &[String],
    user_named: &[String],
    sticky_stems: &[String],
    excluded_stems: &HashSet<String>,
    clone_dir: &Path,
) -> Result<AssemblyResult> {
    let mcp_dir = clone_dir.join("mcp");

    if !mcp_dir.exists() {
        if !user_named.is_empty() || !sticky_stems.is_empty() {
            bail!("--mcp specified but no mcp/ directory in template");
        }
        return Ok(AssemblyResult {
            rendered: serde_json::json!({"mcpServers": {}}),
            rendered_keys: Vec::new(),
            snapshottable_stems: Vec::new(),
        });
    }

    let mut servers: Map<String, Value> = Map::new();
    let mut snapshottable: Vec<String> = Vec::new();
    let mut snapshot_seen: HashSet<String> = HashSet::new();
    let push_snap = |stem: &str, out: &mut Vec<String>, seen: &mut HashSet<String>| {
        if seen.insert(stem.to_string()) {
            out.push(stem.to_string());
        }
    };

    // Layer 1: default/
    let default_dir = mcp_dir.join("default");
    let mut default_stems_present: HashSet<String> = HashSet::new();
    for stem in layer_stems(&default_dir, "json", excluded_stems)? {
        merge_json_file_into(&default_dir.join(format!("{stem}.json")), &mut servers)?;
        default_stems_present.insert(stem.clone());
        push_snap(&stem, &mut snapshottable, &mut snapshot_seen);
    }

    // Layer 2: per-language
    let mut lang_stems_present: HashSet<String> = HashSet::new();
    for lang in languages {
        let lang_dir = mcp_dir.join(lang);
        for stem in layer_stems(&lang_dir, "json", excluded_stems)? {
            merge_json_file_into(&lang_dir.join(format!("{stem}.json")), &mut servers)?;
            lang_stems_present.insert(stem);
            // Intentionally NOT pushed into snapshottable — language stems stay
            // dynamic so dropping a language transitively drops them.
        }
    }

    // Layer 3: explicit/sticky (user_named ∪ sticky_stems) − excluded
    let mut explicit: Vec<String> = Vec::new();
    let mut explicit_seen: HashSet<String> = HashSet::new();
    for stem in user_named.iter().chain(sticky_stems.iter()) {
        if excluded_stems.contains(stem) {
            continue;
        }
        if explicit_seen.insert(stem.clone()) {
            explicit.push(stem.clone());
        }
    }
    let user_named_set: HashSet<&str> = user_named.iter().map(|s| s.as_str()).collect();
    for stem in &explicit {
        let is_user_named = user_named_set.contains(stem.as_str());
        let root_path = mcp_dir.join(format!("{stem}.json"));

        if is_user_named && root_path.is_file() {
            // Root-override path: user-typed --mcp whose root file exists.
            // Merge unconditionally (may overwrite default/lang keys).
            merge_json_file_into(&root_path, &mut servers)?;
            push_snap(stem, &mut snapshottable, &mut snapshot_seen);
        } else if default_stems_present.contains(stem) || lang_stems_present.contains(stem) {
            // Already-satisfied: contributor was merged by layer 1 or 2.
            // Skip the re-read. Record in snapshottable if user explicitly
            // opted in (keeps explicit opt-ins sticky even when they move
            // to default later).
            if is_user_named {
                push_snap(stem, &mut snapshottable, &mut snapshot_seen);
            }
        } else {
            // Move-fallback: stem isn't in layers 1–2 and has no root file
            // directly. Search the full layer spec so historical opt-ins
            // whose contributor was relocated upstream still resolve.
            match resolve_contributor("mcp", "json", &MCP_LAYERS, stem, languages, clone_dir) {
                Some(path) => {
                    merge_json_file_into(&path, &mut servers)?;
                    push_snap(stem, &mut snapshottable, &mut snapshot_seen);
                }
                None => {
                    let available = available_contributor_stems(
                        "mcp",
                        "json",
                        &MCP_LAYERS,
                        languages,
                        clone_dir,
                    );
                    bail!(
                        "MCP '{}' not found in {}. Available: {:?}",
                        stem,
                        mcp_dir.display(),
                        available
                    );
                }
            }
        }
    }

    let rendered_keys: Vec<String> = servers.keys().cloned().collect();
    let rendered = serde_json::json!({ "mcpServers": servers });

    Ok(AssemblyResult {
        rendered,
        rendered_keys,
        snapshottable_stems: snapshottable,
    })
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

/// Assemble the `hooks` block that feeds `.claude/settings.local.json`. Shape
/// parallels `assemble_mcp_json` but with `HOOKS_LAYERS` (no language dirs in
/// today's template). Arrays inside hook-type buckets are concatenated across
/// contributor files; `rendered_keys` is the merged map's top-level hook-type
/// names (not contributor stems).
pub fn assemble_hooks_json(
    user_named: &[String],
    sticky_stems: &[String],
    excluded_stems: &HashSet<String>,
    clone_dir: &Path,
) -> Result<AssemblyResult> {
    let hooks_dir = clone_dir.join("hooks");
    let mut merged: Map<String, Value> = Map::new();
    let mut snapshottable: Vec<String> = Vec::new();
    let mut snapshot_seen: HashSet<String> = HashSet::new();

    // Layer 1: default/
    let default_dir = hooks_dir.join("default");
    let mut default_stems_present: HashSet<String> = HashSet::new();
    for stem in layer_stems(&default_dir, "json", excluded_stems)? {
        merge_hook_file(&default_dir.join(format!("{stem}.json")), &mut merged)?;
        default_stems_present.insert(stem.clone());
        if snapshot_seen.insert(stem.clone()) {
            snapshottable.push(stem);
        }
    }

    // Layer 2: languages — disabled for hooks per HOOKS_LAYERS. Intentionally
    // omitted to avoid silently picking up `hooks/<lang>/` dirs that no
    // template uses today.

    // Layer 3: explicit/sticky
    let mut explicit: Vec<String> = Vec::new();
    let mut explicit_seen: HashSet<String> = HashSet::new();
    for stem in user_named.iter().chain(sticky_stems.iter()) {
        if excluded_stems.contains(stem) {
            continue;
        }
        if explicit_seen.insert(stem.clone()) {
            explicit.push(stem.clone());
        }
    }
    let user_named_set: HashSet<&str> = user_named.iter().map(|s| s.as_str()).collect();
    for stem in &explicit {
        let is_user_named = user_named_set.contains(stem.as_str());
        let root_path = hooks_dir.join(format!("{stem}.json"));

        if is_user_named && root_path.is_file() {
            merge_hook_file(&root_path, &mut merged)?;
            if snapshot_seen.insert(stem.clone()) {
                snapshottable.push(stem.clone());
            }
        } else if default_stems_present.contains(stem) {
            if is_user_named && snapshot_seen.insert(stem.clone()) {
                snapshottable.push(stem.clone());
            }
        } else {
            match resolve_contributor("hooks", "json", &HOOKS_LAYERS, stem, &[], clone_dir) {
                Some(path) => {
                    merge_hook_file(&path, &mut merged)?;
                    if snapshot_seen.insert(stem.clone()) {
                        snapshottable.push(stem.clone());
                    }
                }
                None => {
                    let available =
                        available_contributor_stems("hooks", "json", &HOOKS_LAYERS, &[], clone_dir);
                    bail!(
                        "Hook '{}' not found in {}. Available: {:?}",
                        stem,
                        hooks_dir.display(),
                        available
                    );
                }
            }
        }
    }

    let rendered_keys: Vec<String> = merged.keys().cloned().collect();
    Ok(AssemblyResult {
        rendered: Value::Object(merged),
        rendered_keys,
        snapshottable_stems: snapshottable,
    })
}

/// Build `.claude/settings.local.json` from the pre-assembled hooks block,
/// clarg PreToolUse entries, and the MCP `rendered_keys` list that populates
/// `enabledMcpjsonServers`.
pub fn build_settings(
    hooks_result: &AssemblyResult,
    clarg_entries: &[Value],
    active_mcp_names: &[String],
    clone_dir: &Path,
) -> Result<()> {
    let base_path = clone_dir.join("settings.local.json");

    let mut settings: Value = if base_path.exists() {
        let content = fs::read_to_string(&base_path)?;
        serde_json::from_str(&content).context("Failed to parse settings.local.json")?
    } else {
        serde_json::json!({})
    };

    let settings_obj = settings
        .as_object_mut()
        .context("settings.local.json is not an object")?;

    let mut merged_hooks: Map<String, Value> = hooks_result
        .rendered
        .as_object()
        .cloned()
        .unwrap_or_default();

    // Merge clarg PreToolUse hook entries on top of the assembled hooks.
    for entry in clarg_entries {
        merged_hooks
            .entry("PreToolUse".to_string())
            .or_insert_with(|| Value::Array(vec![]))
            .as_array_mut()
            .unwrap()
            .push(entry.clone());
    }

    settings_obj.insert("hooks".to_string(), Value::Object(merged_hooks));

    let mcp_names: Vec<Value> = active_mcp_names
        .iter()
        .map(|n| Value::String(n.clone()))
        .collect();
    settings_obj.insert("enabledMcpjsonServers".to_string(), Value::Array(mcp_names));

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

/// Clone the template repo to `CLONE_DIR`, removing any stale prior clone.
/// Returns the HEAD commit SHA of the cloned tree.
pub fn clone_repo(repo_url: &str) -> Result<String> {
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

    let output = Command::new("git")
        .args(["-C", CLONE_DIR, "rev-parse", "HEAD"])
        .output()
        .context("Failed to read template HEAD sha")?;
    if !output.status.success() {
        bail!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Append template gitignore fragments to `<dest_dir>/.gitignore`, skipping any
/// lines already present. Idempotent.
///
/// Sources, merged in order:
///   1. `<clone_dir>/gitignore-additions/default.gitignore` (always applied if present)
///   2. `<clone_dir>/gitignore-additions/<lang>.gitignore` for each resolved language,
///      in the order provided.
///
/// Silent no-op when the directory or all referenced files are missing.
pub fn update_gitignore(clone_dir: &Path, dest_dir: &Path, langs: &[String]) -> Result<()> {
    let additions_dir = clone_dir.join("gitignore-additions");
    if !additions_dir.is_dir() {
        return Ok(());
    }

    let mut fragment_sources: Vec<PathBuf> = Vec::new();
    let default_path = additions_dir.join("default.gitignore");
    if default_path.is_file() {
        fragment_sources.push(default_path);
    }
    for lang in langs {
        let lang_path = additions_dir.join(format!("{}.gitignore", lang));
        if lang_path.is_file() {
            fragment_sources.push(lang_path);
        }
    }

    if fragment_sources.is_empty() {
        return Ok(());
    }

    let mut additions = String::new();
    for path in &fragment_sources {
        let frag = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        additions.push_str(&frag);
        if !additions.ends_with('\n') {
            additions.push('\n');
        }
    }

    let gitignore_path = dest_dir.join(".gitignore");
    let existing = if gitignore_path.exists() {
        fs::read_to_string(&gitignore_path)?
    } else {
        String::new()
    };

    let existing_lines: HashSet<&str> = existing.lines().map(str::trim).collect();

    let mut seen_new: HashSet<String> = HashSet::new();
    let new_entries: Vec<&str> = additions
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !existing_lines.contains(line)
                && seen_new.insert((*line).to_string())
        })
        .collect();

    if new_entries.is_empty() {
        return Ok(());
    }

    let mut content = existing;
    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push_str("\n# Claude related\n");
    for entry in new_entries {
        content.push_str(entry);
        content.push('\n');
    }

    if let Some(parent) = gitignore_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&gitignore_path, content)?;
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
    "githooks",
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

pub fn copy_files(clone_dir: &Path, dest_dir: &Path) -> Result<()> {
    let sources = collect_copy_files_sources(clone_dir)?;
    fs::create_dir_all(dest_dir)?;

    for src in &sources {
        let dest = dest_dir.join(src.file_name().unwrap());
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

/// Set a file as executable (0o755) on Unix.
#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
        .with_context(|| format!("Failed to set executable on {}", path.display()))
}

/// Copy git hooks from source_dir/default/ and source_dir/<lang>/ into dest_dir,
/// setting each copied file as executable. Like `copy_conditional_dir` but with chmod.
pub fn copy_conditional_githooks(
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

    fs::create_dir_all(dest_dir)?;
    for dir in &source_dirs {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let src = entry.path();
            let dest = dest_dir.join(entry.file_name());
            fs::copy(&src, &dest)
                .with_context(|| format!("Failed to copy {} to {}", src.display(), dest.display()))?;
            #[cfg(unix)]
            set_executable(&dest)?;
        }
    }

    Ok(())
}

/// Copy named git hook files (extensionless at githooks/ root) into dest_dir, setting executable.
pub fn copy_named_githooks(
    named: &[String],
    clone_dir: &Path,
    dest_dir: &Path,
) -> Result<()> {
    if named.is_empty() {
        return Ok(());
    }

    let githooks_dir = clone_dir.join("githooks");
    if !githooks_dir.exists() {
        bail!("--githooks specified but no githooks/ directory in template");
    }

    fs::create_dir_all(dest_dir)?;

    for name in named {
        let src = githooks_dir.join(name);
        if !src.is_file() {
            let available: Vec<_> = fs::read_dir(&githooks_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            bail!(
                "Git hook '{}' not found in {}. Available: {:?}",
                name,
                githooks_dir.display(),
                available
            );
        }
        let dest = dest_dir.join(name);
        fs::copy(&src, &dest)
            .with_context(|| format!("Failed to copy git hook {}", name))?;
        #[cfg(unix)]
        set_executable(&dest)?;
    }

    Ok(())
}

/// Copy named command files from commands/<name>.md into .claude/commands/.
pub fn copy_named_commands(named_commands: &[String], clone_dir: &Path) -> Result<()> {
    if named_commands.is_empty() {
        return Ok(());
    }

    let commands_dir = clone_dir.join("commands");
    if !commands_dir.exists() {
        bail!("--commands specified but no commands/ directory in template");
    }

    let dest_dir = clone_dir.join(".claude/commands");
    fs::create_dir_all(&dest_dir)?;

    for name in named_commands {
        let src = commands_dir.join(format!("{}.md", name));
        if !src.exists() {
            let available: Vec<_> = fs::read_dir(&commands_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let p = e.path();
                    p.is_file() && p.extension().map_or(false, |ext| ext == "md")
                })
                .map(|e| e.path().file_stem().unwrap().to_string_lossy().to_string())
                .collect();
            bail!(
                "Command '{}' not found in {}. Available: {:?}",
                name,
                commands_dir.display(),
                available
            );
        }
        fs::copy(&src, dest_dir.join(format!("{}.md", name)))
            .with_context(|| format!("Failed to copy command {}", name))?;
    }

    Ok(())
}

pub fn cleanup(clone_dir: &Path) -> Result<()> {
    fs::remove_dir_all(clone_dir)
        .with_context(|| format!("Failed to remove {}", clone_dir.display()))?;
    Ok(())
}

// ── Listing ──────────────────────────────────────────────────────────────

const LIST_CATEGORIES: &[&str] = &["mcp", "hooks", "commands", "githooks", "clarg", "gitignore", "languages"];

/// List available named files for a template category.
pub fn list_category(category: &str, clone_dir: &Path) -> Result<Vec<String>> {
    let (subdir, extensions): (&str, &[&str]) = match category {
        "mcp" => ("mcp", &["json"]),
        "hooks" => ("hooks", &["json"]),
        "commands" => ("commands", &["md"]),
        "githooks" => ("githooks", &[]),
        "clarg" => ("clarg", &["yaml", "yml"]),
        "gitignore" => ("gitignore-additions", &["gitignore"]),
        "languages" => ("claude-md/lang-rules", &["md"]),
        _ => bail!(
            "Unknown category '{}'. Valid categories: {}",
            category,
            LIST_CATEGORIES.join(", ")
        ),
    };

    let dir = clone_dir.join(subdir);
    if !dir.is_dir() {
        return Ok(vec![]);
    }

    let mut names: Vec<String> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            if extensions.is_empty() {
                Some(e.file_name().to_string_lossy().into_owned())
            } else {
                let path = e.path();
                let ext = path.extension()?.to_str()?;
                extensions
                    .contains(&ext)
                    .then(|| path.file_stem().unwrap().to_string_lossy().into_owned())
            }
        })
        .filter(|name| !(category == "gitignore" && name == "default"))
        .collect();

    names.sort();
    Ok(names)
}

/// Format available template files for display. "all" lists every category with headers;
/// a specific category lists just its names.
pub fn list_available(category: &str, clone_dir: &Path) -> Result<String> {
    let mut output = String::new();

    if category == "all" {
        let mut first = true;
        for &cat in LIST_CATEGORIES {
            let names = list_category(cat, clone_dir)?;
            if names.is_empty() {
                continue;
            }
            if !first {
                output.push('\n');
            }
            first = false;
            output.push_str(cat);
            output.push_str(":\n");
            for name in &names {
                output.push_str("  ");
                output.push_str(name);
                output.push('\n');
            }
        }
    } else {
        let names = list_category(category, clone_dir)?;
        for name in &names {
            output.push_str(name);
            output.push('\n');
        }
    }

    Ok(output)
}

// ── Orchestration ────────────────────────────────────────────────────────

/// Inputs to the render pipeline. `setup` carries the user's latest-merged
/// command (positive flags + drops). `sticky_mcp` / `sticky_hooks` carry
/// contributor stems pulled from `lockfile.resolved` that survived the
/// pre-render name-stale pass (empty for initial setup).
pub struct RenderInputs<'a> {
    pub setup: &'a SetupArgs,
    pub sticky_mcp: &'a [String],
    pub sticky_hooks: &'a [String],
}

/// What `run_setup` produces in addition to writing files, for the caller to
/// persist into the new lockfile.
#[derive(Debug, Clone)]
pub struct SetupOutcome {
    pub resolved_languages: Vec<String>,
    pub mcp_snapshottable_stems: Vec<String>,
    pub hooks_snapshottable_stems: Vec<String>,
}

/// Drive the full clemp pipeline: clone-dir prep → conflict check → write to
/// `dest_dir`. Returns a `SetupOutcome` carrying the resolved language list
/// and the contributor stems eligible for `lockfile.resolved`.
///
/// * `check_conflicts` — when `true`, aborts (or prompts with `--force`) if
///   existing files in `dest_dir` would be overwritten. Set `false` for the
///   update render pass, which writes into an empty temp dir.
/// * `install_git_hooks` — when `true`, git hooks are written under
///   `dest_dir/.git/hooks`. Callers that target a real CWD should gate this on
///   the presence of a `.git/` directory themselves.
pub fn run_setup(
    inputs: &RenderInputs,
    clone_dir: &Path,
    dest_dir: &Path,
    check_conflicts: bool,
    install_git_hooks: bool,
) -> Result<SetupOutcome> {
    let args = inputs.setup;

    // ── Phase 1: clone_dir prep (no dest_dir mutations) ─────────────────

    println!("Resolving languages...");
    let resolved_languages = resolve_all_languages(&args.languages, clone_dir)?;

    let mcp_excluded: HashSet<String> = args.drop_mcp.iter().cloned().collect();
    let hooks_excluded: HashSet<String> = args.drop_hooks.iter().cloned().collect();

    println!("Assembling MCP servers...");
    let mcp_result = assemble_mcp_json(
        &resolved_languages,
        &args.mcp,
        inputs.sticky_mcp,
        &mcp_excluded,
        clone_dir,
    )?;
    fs::write(
        clone_dir.join(".mcp.json"),
        serde_json::to_string_pretty(&mcp_result.rendered)?,
    )?;

    println!("Rendering CLAUDE.md...");
    let claude_md = render_claude_md(&resolved_languages, &mcp_result.rendered_keys, clone_dir)?;
    fs::write(clone_dir.join("CLAUDE.md"), claude_md)?;

    let clarg_name = args.clarg.clone().or_else(|| {
        clone_dir.join("clarg/default.yaml").exists().then(|| "default".into())
    });
    let clarg_entries: Vec<Value> = if let Some(name) = &clarg_name {
        println!("Setting up clarg...");
        vec![setup_clarg(name, clone_dir)?]
    } else {
        vec![]
    };

    println!("Assembling hooks...");
    let hooks_result = assemble_hooks_json(
        &args.hooks,
        inputs.sticky_hooks,
        &hooks_excluded,
        clone_dir,
    )?;

    println!("Building settings...");
    build_settings(&hooks_result, &clarg_entries, &mcp_result.rendered_keys, clone_dir)?;

    if clarg_name.is_some() {
        check_clarg_installed();
    }

    println!("Assembling commands...");
    copy_conditional_dir(
        &clone_dir.join("commands"),
        &resolved_languages,
        &clone_dir.join(".claude/commands"),
    )?;
    copy_named_commands(&args.commands, clone_dir)?;

    println!("Assembling skills...");
    copy_conditional_dir(
        &clone_dir.join("skills"),
        &resolved_languages,
        &clone_dir.join(".claude/skills"),
    )?;

    // ── Phase 2: pre-flight conflict check ──────────────────────────────

    let githooks_dir = clone_dir.join("githooks");
    let git_hooks_dest = dest_dir.join(".git/hooks");

    if check_conflicts {
        println!("Checking for conflicts...");
        let mut all_cwd_targets = collect_copy_files_sources(clone_dir)?;
        all_cwd_targets.extend(collect_conditional_dir_sources(
            &clone_dir.join("copied"),
            &resolved_languages,
        ));
        let mut conflicts = collect_conflicts(&all_cwd_targets, dest_dir);

        let mut githooks_sources =
            collect_conditional_dir_sources(&githooks_dir, &resolved_languages);
        for name in &args.githooks {
            let src = githooks_dir.join(name);
            if src.is_file() {
                githooks_sources.push(src);
            }
        }
        if install_git_hooks && git_hooks_dest.is_dir() {
            conflicts.extend(collect_conflicts(&githooks_sources, &git_hooks_dest));
        }

        if !conflicts.is_empty() {
            let names: Vec<_> = conflicts.iter().map(|p| p.display().to_string()).collect();

            if !args.force {
                bail!(
                    "The following files/directories already exist and would be overwritten:\n  {}\nRemove them first, run from a clean directory, or use --force.\n\nIf this is a previously clemp-configured project, try `clemp update` instead.",
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
    }

    // ── Phase 3: dest_dir mutations ─────────────────────────────────────

    println!("Updating .gitignore...");
    update_gitignore(clone_dir, dest_dir, &resolved_languages)?;

    println!("Copying files...");
    copy_files(clone_dir, dest_dir)?;

    println!("Copying language-specific files...");
    copy_conditional_dir(&clone_dir.join("copied"), &resolved_languages, dest_dir)?;

    if install_git_hooks {
        if githooks_dir.exists() || !args.githooks.is_empty() {
            println!("Installing git hooks...");
            copy_conditional_githooks(&githooks_dir, &resolved_languages, &git_hooks_dest)?;
            copy_named_githooks(&args.githooks, clone_dir, &git_hooks_dest)?;
        }
    } else if !args.githooks.is_empty() || githooks_dir.join("default").is_dir() {
        eprintln!("Warning: git hooks not installed (no .git/ directory in target)");
    }

    Ok(SetupOutcome {
        resolved_languages,
        mcp_snapshottable_stems: mcp_result.snapshottable_stems,
        hooks_snapshottable_stems: hooks_result.snapshottable_stems,
    })
}

/// Enumerate every file clemp wrote under `dest_dir` (derived from `clone_dir`'s
/// staged tree + conditional dirs + git hooks) and compute its SHA-256.
/// Excludes `.gitignore` and the lockfile itself. Paths in the returned map are
/// normalized to forward-slash form.
pub fn compute_manifest(
    args: &SetupArgs,
    resolved_languages: &[String],
    clone_dir: &Path,
    dest_dir: &Path,
) -> Result<BTreeMap<String, String>> {
    let mut manifest = BTreeMap::new();

    // 1. Anything copy_files would have placed in dest_dir (recursively).
    for entry in fs::read_dir(clone_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if COPY_FILES_EXCLUDE.contains(&name.to_string_lossy().as_ref()) {
            continue;
        }
        let rel_start = PathBuf::from(&name);
        hash_tree_into_manifest(dest_dir, &rel_start, &mut manifest)?;
    }

    // 2. copied/default/ and copied/<lang>/ flattened under dest_dir.
    let copied_dir = clone_dir.join("copied");
    let mut overlay_dirs: Vec<PathBuf> = Vec::new();
    if copied_dir.join("default").is_dir() {
        overlay_dirs.push(copied_dir.join("default"));
    }
    for lang in resolved_languages {
        let ld = copied_dir.join(lang);
        if ld.is_dir() {
            overlay_dirs.push(ld);
        }
    }
    for overlay in &overlay_dirs {
        for entry in fs::read_dir(overlay)? {
            let entry = entry?;
            let rel_start = PathBuf::from(entry.file_name());
            hash_tree_into_manifest(dest_dir, &rel_start, &mut manifest)?;
        }
    }

    // 3. Git hooks (flat filenames) under dest_dir/.git/hooks/.
    let githooks_src = clone_dir.join("githooks");
    let mut git_hook_names: BTreeMap<String, ()> = BTreeMap::new();
    let mut collect_names = |dir: &Path| -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let e = entry?;
            if e.path().is_file() {
                git_hook_names.insert(e.file_name().to_string_lossy().into_owned(), ());
            }
        }
        Ok(())
    };
    collect_names(&githooks_src.join("default"))?;
    for lang in resolved_languages {
        collect_names(&githooks_src.join(lang))?;
    }
    for name in &args.githooks {
        git_hook_names.insert(name.clone(), ());
    }
    for name in git_hook_names.keys() {
        let rel = PathBuf::from(".git/hooks").join(name);
        let full = dest_dir.join(&rel);
        if full.is_file() {
            manifest.insert(lockfile_key(&rel), hash_file(&full)?);
        }
    }

    // Never track these — user-owned or clemp-meta.
    manifest.remove(".gitignore");
    manifest.remove(LOCKFILE_NAME);

    Ok(manifest)
}

fn hash_tree_into_manifest(
    dest_dir: &Path,
    rel_start: &Path,
    manifest: &mut BTreeMap<String, String>,
) -> Result<()> {
    let full = dest_dir.join(rel_start);
    if !full.exists() {
        return Ok(());
    }
    if full.is_file() {
        manifest.insert(lockfile_key(rel_start), hash_file(&full)?);
        return Ok(());
    }
    for entry in fs::read_dir(&full)? {
        let entry = entry?;
        let sub_rel = rel_start.join(entry.file_name());
        hash_tree_into_manifest(dest_dir, &sub_rel, manifest)?;
    }
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

/// Normalize all multi-value fields in-place (replaces clap's comma-only split
/// with comma+whitespace splitting).
pub fn normalize_setup_args(args: &mut SetupArgs) {
    args.hooks = split_multi_values(std::mem::take(&mut args.hooks));
    args.mcp = split_multi_values(std::mem::take(&mut args.mcp));
    args.commands = split_multi_values(std::mem::take(&mut args.commands));
    args.githooks = split_multi_values(std::mem::take(&mut args.githooks));
    args.drop_mcp = split_multi_values(std::mem::take(&mut args.drop_mcp));
    args.drop_hooks = split_multi_values(std::mem::take(&mut args.drop_hooks));
}

// ── Update flow ──────────────────────────────────────────────────────────

/// Check whether the `claude` CLI is on PATH.
pub fn claude_available() -> bool {
    Command::new("which")
        .arg("claude")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Launch an interactive `claude` session to merge template changes into a
/// user-modified file. Uses `--model sonnet --permission-mode acceptEdits` so
/// file edits proceed without additional prompting inside Claude. Returns an
/// error on non-zero exit so the caller can abort before persisting a new
/// lockfile baseline.
pub fn merge_with_claude(rel_path: &str, staging: &Path, cwd: &Path) -> Result<()> {
    let new_file = staging.join(rel_path);
    let cur_file = cwd.join(rel_path);

    let prompt = format!(
        "Merge the template update at @{new} into @{cur}. \
         The user has customized @{cur} and the template has also changed independently. \
         Preserve the user's customizations while incorporating the template's updates. \
         Edit @{cur} in place. Do not create any new files.",
        new = new_file.display(),
        cur = cur_file.display(),
    );

    println!("\n— Merging {rel_path} —");
    let status = Command::new("claude")
        .args([
            "--model",
            "sonnet",
            "--permission-mode",
            "acceptEdits",
            &prompt,
        ])
        .status()
        .context("Failed to invoke `claude`")?;

    if !status.success() {
        bail!(
            "claude exited with {} while merging {} — aborting update so the lockfile baseline stays intact.\n\
             Re-run with `--force` to overwrite your edits with the template version.",
            status,
            rel_path
        );
    }
    Ok(())
}

/// Apply a single manifest entry from `staging_dir` to `cwd`, creating parents
/// and (on Unix) preserving executable bit for `.git/hooks/` entries. If `dest`
/// currently exists as a directory (shape collision resolved via `--force`), it
/// is removed before the file is written.
fn apply_one(key: &str, staging_dir: &Path, cwd: &Path) -> Result<()> {
    let src = staging_dir.join(key);
    let dest = cwd.join(key);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.is_dir() {
        fs::remove_dir_all(&dest)
            .with_context(|| format!("Failed to remove directory at {}", dest.display()))?;
    }
    fs::copy(&src, &dest)
        .with_context(|| format!("Failed to copy {} to {}", src.display(), dest.display()))?;
    #[cfg(unix)]
    if key.starts_with(".git/hooks/") {
        set_executable(&dest)?;
    }
    Ok(())
}

/// Classification for a single manifest entry during `clemp update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateClass {
    /// Old == current on disk, new differs — safe template refresh.
    Clean,
    /// Not tracked, not on disk — straight copy.
    New,
    /// Not tracked, but user has a file at this path with different content — needs merge.
    Collision,
    /// Tracked, user modified it, and template changed it — needs merge.
    Conflict,
    /// Tracked, user modified it, template unchanged — leave user's version.
    Skipped,
    /// Tracked, user deleted it — restore only with `--restore-deleted`.
    Missing,
    /// A directory exists at a path where the template now wants a file.
    /// Resolvable only by `--force` (Claude cannot merge into a directory).
    ShapeCollision,
    /// Template hash matches what's on disk — already in sync, no-op.
    Identical,
}

/// Classify a single update path from its (old, current, new) hash triple plus
/// the on-disk "shape" at that path. Extracted for direct unit testing.
pub fn classify_update_path(
    old_hash: Option<&str>,
    cur_hash: Option<&str>,
    new_hash: &str,
    cwd_is_dir: bool,
) -> UpdateClass {
    if cwd_is_dir {
        return UpdateClass::ShapeCollision;
    }
    match (old_hash, cur_hash) {
        (None, None) => UpdateClass::New,
        (None, Some(cur)) => {
            if cur == new_hash {
                UpdateClass::Identical
            } else {
                UpdateClass::Collision
            }
        }
        (Some(_), None) => UpdateClass::Missing,
        (Some(old), Some(cur)) => {
            if cur == old {
                if new_hash == old {
                    UpdateClass::Identical
                } else {
                    UpdateClass::Clean
                }
            } else if new_hash == old {
                UpdateClass::Skipped
            } else {
                UpdateClass::Conflict
            }
        }
    }
}

/// Drive `clemp update`: diff the new template render against the lockfile +
/// current working tree, apply non-conflicting changes, route conflicts to
/// Claude (or `--force` overwrite), and persist an updated lockfile.
pub fn run_update(
    args: &UpdateArgs,
    clone_dir: &Path,
    template_sha: &str,
    template_repo: &str,
) -> Result<()> {
    let cwd = Path::new(".");

    let lockfile = Lockfile::load(cwd)?.with_context(|| format!(
        "No {LOCKFILE_NAME} found in current directory.\nThis doesn't look like a clemp-configured project — run `clemp <args>` to set one up first."
    ))?;

    let mut merged_command = {
        let mut m = lockfile.original_command.clone();
        m.merge_additive(&OriginalCommand::from_setup(&args.setup))?;
        m
    };

    let sha_unchanged = template_sha == lockfile.template_sha;
    let command_unchanged = merged_command == lockfile.original_command;
    // Pre-snapshot lockfiles must do a full re-render even when nothing has
    // otherwise changed, so `resolved` can be captured. Pins reproducibility
    // for aggregation-output contributors (see PLAN_snapshot.md).
    let snapshot_missing = lockfile.resolved.is_none();
    // `--restore-deleted` must inspect the working tree even when nothing in the
    // template has changed, so it cannot share the unchanged-template fast path.
    if sha_unchanged && command_unchanged && !args.restore_deleted && !snapshot_missing {
        println!("Already up to date.");
        return Ok(());
    }

    // Resolve languages once up front — drives fresh-addition validation, the
    // name-stale pass, and `resolve_contributor` layer walks.
    let resolved_languages = resolve_all_languages(&merged_command.languages, clone_dir)?;

    validate_fresh_additions(
        &lockfile.original_command,
        &merged_command,
        &resolved_languages,
        lockfile.resolved.as_ref(),
        clone_dir,
    )?;

    // Name-level stale pass: stems from the lockfile snapshot plus historical
    // opt-ins from original_command.<kind>, minus anything the merged command
    // drops, probed against the current template. Stems with no current
    // contributor become `stale_<kind>`; survivors become `sticky_<kind>` fed
    // into assembly.
    let drop_mcp_set: HashSet<String> = merged_command.drop_mcp.iter().cloned().collect();
    let drop_hooks_set: HashSet<String> = merged_command.drop_hooks.iter().cloned().collect();

    let candidate_mcp: Vec<String> = {
        let snap = lockfile.resolved.as_ref().map(|r| r.mcp.as_slice()).unwrap_or(&[]);
        let mut seen: HashSet<String> = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        for s in snap.iter().chain(merged_command.mcp.iter()) {
            if drop_mcp_set.contains(s) { continue; }
            if seen.insert(s.clone()) { out.push(s.clone()); }
        }
        out
    };
    let candidate_hooks: Vec<String> = {
        let snap = lockfile.resolved.as_ref().map(|r| r.hooks.as_slice()).unwrap_or(&[]);
        let mut seen: HashSet<String> = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        for s in snap.iter().chain(merged_command.hooks.iter()) {
            if drop_hooks_set.contains(s) { continue; }
            if seen.insert(s.clone()) { out.push(s.clone()); }
        }
        out
    };

    let (sticky_mcp, stale_mcp): (Vec<String>, Vec<String>) = candidate_mcp
        .into_iter()
        .partition(|s| {
            resolve_contributor("mcp", "json", &MCP_LAYERS, s, &resolved_languages, clone_dir)
                .is_some()
        });
    let (sticky_hooks, stale_hooks): (Vec<String>, Vec<String>) = candidate_hooks
        .into_iter()
        .partition(|s| {
            resolve_contributor("hooks", "json", &HOOKS_LAYERS, s, &[], clone_dir).is_some()
        });

    // Single confirm-or-bail UX for name-stale contributors. Declining bails
    // with zero side effects so the user can re-run with `--drop-<kind>` or
    // restore the contributor upstream.
    for (kind, stale, output) in [
        ("MCP", &stale_mcp, ".mcp.json"),
        ("hook", &stale_hooks, ".claude/settings.local.json"),
    ] {
        if stale.is_empty() {
            continue;
        }
        if args.prune_stale {
            println!(
                "Dropping {} stale {} contributors: {}",
                stale.len(),
                kind,
                stale.join(", ")
            );
        } else {
            println!(
                "The template no longer provides these {} contributors: {}",
                kind,
                stale.join(", ")
            );
            if !confirm(&format!("They will be removed from {output}. Continue?"))? {
                bail!("Aborted. Re-run with --prune-stale, or --drop-{kind} the stems explicitly, or restore them upstream.",
                    kind = if kind == "MCP" { "mcp" } else { "hooks" });
            }
        }
    }

    // Strip accepted/pruned stale stems from the persisted command so
    // `setup_args.mcp`/`hooks` (derived from merged_command below) no longer
    // carries them into the assembler's user_named path — where they would
    // bail with "MCP/Hook not found" after their contributor disappeared
    // upstream. The stems are already gone from sticky_mcp/hooks above; this
    // closes the symmetric hole in the explicit/historical list.
    if !stale_mcp.is_empty() {
        merged_command.mcp.retain(|s| !stale_mcp.contains(s));
    }
    if !stale_hooks.is_empty() {
        merged_command.hooks.retain(|s| !stale_hooks.contains(s));
    }

    // Render the new template into a temp staging directory.
    let staging = env::temp_dir().join(format!("clemp-update-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    let setup_args = {
        let mut s = merged_command.clone().into_setup();
        s.force = args.setup.force;
        s
    };

    let render_inputs = RenderInputs {
        setup: &setup_args,
        sticky_mcp: &sticky_mcp,
        sticky_hooks: &sticky_hooks,
    };

    let outcome = run_setup(&render_inputs, clone_dir, &staging, false, true)?;
    let resolved = outcome.resolved_languages;
    let new_manifest = compute_manifest(&setup_args, &resolved, clone_dir, &staging)?;

    // Classify every file in the new render.
    let mut clean: Vec<String> = Vec::new();
    let mut new_files: Vec<String> = Vec::new();
    let mut collisions: Vec<String> = Vec::new(); // template-new, user already has something there
    let mut conflicts: Vec<String> = Vec::new();  // user modified AND template changed
    let mut skipped: Vec<String> = Vec::new();    // user modified, template unchanged
    let mut restore_pending: Vec<String> = Vec::new(); // user deleted clemp-managed file
    let mut shape_collisions: Vec<String> = Vec::new(); // dir exists at file-target path

    for (path, new_hash) in &new_manifest {
        let cwd_path = cwd.join(path);
        let cwd_is_dir = cwd_path.is_dir();
        let cur_hash = if cwd_path.is_file() { Some(hash_file(&cwd_path)?) } else { None };
        let old_hash = lockfile.files.get(path).map(String::as_str);

        match classify_update_path(old_hash, cur_hash.as_deref(), new_hash, cwd_is_dir) {
            UpdateClass::Identical => {}
            UpdateClass::Clean => clean.push(path.clone()),
            UpdateClass::New => new_files.push(path.clone()),
            UpdateClass::Collision => collisions.push(path.clone()),
            UpdateClass::Conflict => conflicts.push(path.clone()),
            UpdateClass::Skipped => skipped.push(path.clone()),
            UpdateClass::Missing => {
                if args.restore_deleted {
                    new_files.push(path.clone());
                } else {
                    restore_pending.push(path.clone());
                }
            }
            UpdateClass::ShapeCollision => shape_collisions.push(path.clone()),
        }
    }

    let stale: Vec<String> = lockfile
        .files
        .keys()
        .filter(|p| !new_manifest.contains_key(*p))
        .cloned()
        .collect();

    println!("\nUpdate plan:");
    let report = |paths: &[String], label: &str| {
        if paths.is_empty() { return; }
        println!("  {:>3} {}", paths.len(), label);
        for p in paths {
            println!("        {p}");
        }
    };
    report(&clean, "cleanly updated");
    report(&new_files, "new");
    report(&skipped, "preserved (user-modified, template unchanged)");
    report(&conflicts, "conflicting (user + template both changed)");
    report(&collisions, "collisions (template introduced file, you already have one)");
    report(&shape_collisions, "shape collisions (directory exists where template wants a file)");
    report(&stale, "stale (template no longer produces)");
    report(&restore_pending, "missing (use --restore-deleted to re-add)");

    // Shape collisions can only be resolved by --force (Claude can't merge into a directory).
    if !shape_collisions.is_empty() && !args.setup.force {
        let _ = fs::remove_dir_all(&staging);
        bail!(
            "The following paths exist as directories but the template now wants a file there:\n  {}\n\n\
             Re-run with `--force` to replace the directories with the template's file, \
             or remove/move the directories yourself.",
            shape_collisions.join("\n  ")
        );
    }

    // Blocker-stale: a stale path that exists on disk as a FILE and whose
    // path is a parent of some new/clean write. If left in place, `create_dir_all`
    // during the clean/new phase would fail after merges had already been
    // applied. Gate this in preflight so declining deletion can't produce a
    // half-applied update.
    let blocker_stale: Vec<String> = stale
        .iter()
        .filter(|path| {
            if !cwd.join(path).is_file() { return false; }
            let prefix = format!("{}/", path);
            clean.iter().chain(new_files.iter()).any(|p| p.starts_with(&prefix))
        })
        .cloned()
        .collect();
    if !blocker_stale.is_empty() && !args.prune_stale {
        let _ = fs::remove_dir_all(&staging);
        bail!(
            "The following files must be removed because the template now produces \
             a directory at their path:\n  {}\n\n\
             Re-run with `--prune-stale` to delete them, or remove them yourself.",
            blocker_stale.join("\n  ")
        );
    }

    // Conflicts AND collisions both route through Claude. Gate before any writes.
    let needs_claude = (!conflicts.is_empty() || !collisions.is_empty()) && !args.setup.force;
    if needs_claude && !claude_available() {
        let _ = fs::remove_dir_all(&staging);
        let affected: Vec<String> = conflicts.iter().chain(collisions.iter()).cloned().collect();
        bail!(
            "The following files need an interactive merge (you've changed them and so has the template, \
             or the template now wants a path you already use):\n  {}\n\n\
             `claude` CLI not found on PATH — interactive merging isn't possible.\n\
             Options:\n  \
             - Install Claude Code and re-run `clemp update`\n  \
             - Run `clemp update --force` to overwrite your edits with the template version",
            affected.join("\n  ")
        );
    }

    // Run the fail-prone work (Claude merges) BEFORE any destructive step
    // (stale deletions) or clean/new writes. If a merge fails we bail via `?`
    // with the working tree still classifiable:
    // - lockfile still pinned to the old SHA
    // - stale files still on disk (not yet deleted)
    // - clean files still at their old hashes (so next retry classifies them as
    //   `clean` instead of bogus `conflict` via old != cur == new).

    // Collisions: path is new from template's perspective. Overwrite with --force,
    // else route through Claude merge (treat like a conflict).
    for path in &collisions {
        if args.setup.force {
            apply_one(path, &staging, cwd)?;
        } else {
            merge_with_claude(path, &staging, cwd)?;
        }
    }

    // Conflicts: --force overwrites, else Claude merges.
    for path in &conflicts {
        if args.setup.force {
            apply_one(path, &staging, cwd)?;
        } else {
            merge_with_claude(path, &staging, cwd)?;
        }
    }

    // Shape collisions reach here only with --force.
    for path in &shape_collisions {
        apply_one(path, &staging, cwd)?;
    }

    // Stale handling runs AFTER merges (so a failed merge can't lose stale
    // files under `--prune-stale`) but BEFORE clean/new writes (so file→dir
    // template transitions have the old file gone before the new directory
    // tree is written).
    for path in &stale {
        let target = cwd.join(path);
        if !target.exists() {
            continue;
        }
        let delete = if args.prune_stale {
            true
        } else {
            confirm(&format!(
                "Template no longer produces {path}. Delete this file?"
            ))?
        };
        if delete {
            if target.is_dir() {
                fs::remove_dir_all(&target)?;
            } else {
                fs::remove_file(&target)?;
            }
        }
    }

    // Finally, apply non-conflicting writes. These are straight copies and
    // shouldn't fail on a healthy filesystem; doing them last means the rest of
    // the update is already committed before we touch paths the user considers
    // "clean".
    for path in clean.iter().chain(new_files.iter()) {
        apply_one(path, &staging, cwd)?;
    }

    // Always re-apply gitignore additions to the real CWD.
    update_gitignore(clone_dir, cwd, &resolved)?;

    // Persist new lockfile. Use the template-side manifest (template hashes) as
    // the source of truth so future updates can detect user modifications.
    let new_lockfile = Lockfile {
        template_repo: template_repo.to_string(),
        template_sha: template_sha.to_string(),
        original_command: merged_command,
        resolved: Some(Resolved {
            mcp: outcome.mcp_snapshottable_stems,
            hooks: outcome.hooks_snapshottable_stems,
        }),
        files: new_manifest,
    };
    new_lockfile.save(cwd)?;

    // Cleanup staging.
    let _ = fs::remove_dir_all(&staging);

    println!("\nUpdate complete.");
    Ok(())
}
