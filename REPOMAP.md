<!-- commit: 5b20954bcbc4be5734fb920d4e18f79b5bff9e2d -->

### Quick Reference
- **Critical Paths**:
  - **Initial setup** (`main.rs::run_setup_cmd`): clone → `run_setup(args, clone_dir, ".", check_conflicts=true, install_git_hooks=<CWD has .git>)` → `compute_manifest(".")` → write `.clemp-lock.yaml` → cleanup. Errors mid-setup roll back clone_dir and any created `.gitignore`.
  - **Update** (`main.rs::run_update_cmd` → `lib::run_update`): read `.clemp-lock.yaml` → clone → merge CLI args additively into stored `OriginalCommand` → early-exit if SHA+command unchanged AND `--restore-deleted` is NOT set → `run_setup` into a `env::temp_dir()/clemp-update-<pid>` staging dir → `compute_manifest(staging)` to get new template hashes → classify each path via `classify_update_path` (clean / new / skipped / conflict / collision / shape-collision / stale / missing / identical) → preflight: bail if any `shape_collisions` without `--force`, bail if `claude` missing on PATH and any conflicts/collisions exist without `--force` → Claude merge or `--force` for collisions then conflicts → `--force` apply for shape collisions → stale prune (prompt-or-`--prune-stale`) → apply clean+new writes → rewrite lockfile with new template manifest.
  - **List** (`main.rs::run_list`): clone → `list_available(category, clone_dir)` → print → remove clone dir. Early-exits, never touches CWD.
- **Architectural Rules**:
  - `COPY_FILES_EXCLUDE` (module-level constant in `src/lib.rs`) must stay in sync with template structure dirs (`commands`, `skills`, `copied`, `hooks`, `mcp`, `githooks`, `clarg`, `claude-md`, etc.). `compute_manifest` uses the same list to enumerate which dest_dir paths are clemp-owned.
  - Conflict checking in `run_setup` is gated on the `check_conflicts` parameter; initial setup passes `true`, update render passes `false` (staging dir is always empty).
  - Git-hook installation in `run_setup` is gated on `install_git_hooks`; initial setup derives this from `.git/` existence in CWD, update render always passes `true` (renders into `staging/.git/hooks/` for diffing). Applying to a real `.git/hooks/` at CWD for update happens per-path in `run_update::apply_one`.
  - `.gitignore` is **never** hash-tracked in the lockfile — it's append-only and user-owned. `update_gitignore` is idempotent and called separately in the update apply phase.
  - Lockfile keys are normalized to forward-slash form via `lockfile_key`. All manifest lookups go through this normalization.
  - New lockfile manifest after `clemp update` uses **template-render hashes** (from the staging dir), not on-disk CWD hashes. This preserves the invariant: "lockfile hash = what clemp last wrote; any deviation on disk = user modification."
  - Language resolution in `resolve_language` checks 5 conditional dirs (`commands`, `skills`, `copied`, `mcp`, `githooks`) plus the file `gitignore-additions/<canonical>.gitignore`. `resolve_all_languages` dedupes by canonical name so `ts` and `typescript` collapse to a single `typescript` entry; `OriginalCommand::merge_additive` applies the same alias-aware dedup when unioning stored language lists across updates.
  - MCP assembly merges 3 layers in order: `default/` → language dirs → `--mcp` named files. Later layers override.

### Types & Schemas
- (`src/lib.rs`, shared arg set for `clemp` and `clemp update`, `SetupArgs` — clap `Args`-derived, field-for-field mirror of `OriginalCommand` + `force`)
- (`src/lib.rs`, top-level CLI, `Cli { command: Option<CliCommand>, setup: SetupArgs, version }`)
- (`src/lib.rs`, subcommands, `CliCommand::{Update(UpdateArgs), List { category: Option<String> }}`)
- (`src/lib.rs`, update-only args, `UpdateArgs { setup: SetupArgs, prune_stale: bool, restore_deleted: bool }`)
- (`src/lib.rs`, persisted invocation for update's additive-merge, `OriginalCommand { languages, hooks, mcp, commands, githooks, clarg }`)
- (`src/lib.rs`, project-root lockfile at `.clemp-lock.yaml`, `Lockfile { template_repo, template_sha, original_command, files: BTreeMap<String, String> }`)
- (`src/lib.rs`, persistent config at `~/.config/clemp/clemp.yaml`, `Config { gh_repo: Option<String> }`)
- (`src/lib.rs`, language resolution result, `LanguageResolution { HasRulesFile, ConditionalOnly, NoMatch }`)
- (`src/lib.rs`, update classification result for one manifest entry, `UpdateClass { Clean, New, Collision, Conflict, Skipped, Missing, ShapeCollision, Identical }`)

### Integration Points

**CLI Surface** (`src/main.rs` → `src/lib.rs::Cli`)
- Default (no subcommand) — initial setup:
  - Positional: `[LANGUAGE...]` — language names/aliases
  - `--hooks <name,...>`, `--mcp <name,...>`, `--commands <name,...>`, `--githooks <name,...>` — comma or space separated (post-processed by `split_multi_values` via `normalize_setup_args`)
  - `--clarg <name>` — single value, maps to `clarg/<name>.yaml` in template
  - `--force` — overwrite existing files with confirmation prompt
- `clemp update [LANGUAGE...] [OPTIONS]` — additive update. Same flags as setup, plus:
  - `--prune-stale` — delete files the template no longer produces without prompting
  - `--restore-deleted` — re-copy files the user removed from disk
  - `--force` — skip interactive Claude merge, overwrite conflicts with template version
- `clemp list [CATEGORY]` — list available template files. `CATEGORY` is one of `mcp`, `hooks`, `commands`, `githooks`, `clarg`, `gitignore`, `languages`; omit for all categories with headers.
- `-v` / `--version` — top-level, prints version from `Cargo.toml`

**Template Rendering** (`render_claude_md` in `src/lib.rs`)
- Input: `CLAUDE.md.jinja` from clone dir
- Context variables: `lang` (dict), `mcp` (dict), `lang_rules` (string), `mcp_rules` (string), plus dynamic vars from `claude-md/misc/` files (hyphens → underscores)
- Misc files: plain `.md` → static content; `.md.jinja` → rendered with `{lang, mcp}` context before injection
- Output tags: `<tag-name>...</tag-name>` wrapping each section

**MCP Assembly** (`assemble_mcp_json` in `src/lib.rs`)
- Sources: `mcp/default/*.json` + `mcp/<lang>/*.json` + `mcp/<name>.json`
- Output: `{"mcpServers": {...}}` JSON + flat list of server names
- Server names feed into `settings.local.json` → `enabledMcpjsonServers`

**Hooks/Settings** (`build_settings` in `src/lib.rs`)
- Sources: `hooks/default/*.json` + `hooks/<name>.json`
- Hook JSON format: `{"HookType": [entries...]}` — arrays merge across files
- Output: `.claude/settings.local.json` with `hooks` and `enabledMcpjsonServers` keys

**Clarg Integration** (`setup_clarg` + `check_clarg_installed` in `src/lib.rs`)
- Source: `clarg/<name>.yaml` in clone dir
- Auto-apply: `clarg/default.yaml` is used automatically when present and no `--clarg` flag is given
- Output: `.claude/clarg-<name>.yaml` + `PreToolUse` hook entry merged into settings
- PATH check: warns with install instructions if `clarg` binary not found

**Conditional Dirs** (`copy_conditional_dir` in `src/lib.rs`)
- Pattern: `<source_dir>/default/` + `<source_dir>/<lang>/` → merged into dest (lang overrides default)
- Applied to: `commands` → `<dest>/.claude/commands`, `skills` → `<dest>/.claude/skills`, `copied` → `<dest>` (dest root)

**Gitignore Additions** (`update_gitignore` in `src/lib.rs`)
- Sources: `gitignore-additions/default.gitignore` (always) + `gitignore-additions/<lang>.gitignore` for each resolved language (in user-provided order)
- Merge: concat default + lang fragments → trim → drop blanks → dedupe against existing `.gitignore` lines AND against earlier fragments → append remaining lines under a `# Claude related` header
- Missing dir, missing files, and empty fragments are silent no-ops
- `.gitignore` is **not** hash-tracked in the lockfile (see Architectural Rules)

**Git Hooks** (`copy_conditional_githooks` + `copy_named_githooks` in `src/lib.rs`)
- Sources: `githooks/default/` + `githooks/<lang>/` (conditional) + `githooks/<name>` (named, extensionless files at root)
- Destination: `<dest>/.git/hooks/` — all copied files chmod 0o755 on Unix
- Named hooks override conditional hooks with the same name (applied after)
- `run_setup` installs only when caller passes `install_git_hooks = true`; warns otherwise
- Per-file apply in `run_update::apply_one` re-chmods 0o755 whenever the path starts with `.git/hooks/`

**Named Commands** (`copy_named_commands` in `src/lib.rs`)
- Sources: `commands/<name>.md` — standalone `.md` files at root of commands dir
- Copied into `<clone_dir>/.claude/commands/` (staging), later flushed via `copy_files` — named files override defaults/lang with same name

**Filesystem** (`copy_files` in `src/lib.rs`)
- Copies everything from clone dir root to `<dest_dir>` except `COPY_FILES_EXCLUDE`
- `collect_conflicts` detects existing files in dest; `--force` prompts + clears before write

**Lockfile / Manifest** (`Lockfile`, `compute_manifest`, `hash_file`, `lockfile_key` in `src/lib.rs`)
- `Lockfile::load(dest_dir)` → `Ok(None)` if absent, `Ok(Some(Lockfile))` otherwise
- `Lockfile::save(dest_dir)` → writes `dest_dir/.clemp-lock.yaml`
- `compute_manifest(args, resolved_languages, clone_dir, dest_dir)` enumerates every path clemp would write (clone-root minus exclude + `copied/{default,<lang>}/` flattened + `.git/hooks/` entries), hashes whatever exists under `dest_dir`, and returns a `BTreeMap<String, String>` keyed by forward-slash paths. Always excludes `.gitignore` and `.clemp-lock.yaml` from the manifest.

**Update Flow** (`run_update` in `src/lib.rs`)
- Driven by `run_update(args, clone_dir, template_sha, template_repo)` after `main.rs` clones the template
- Loads lockfile; bails if missing with a hint to run `clemp` for initial setup instead
- Merges `args.setup` into `lockfile.original_command` additively (union of vectors, replaces `clarg` only if the update specifies it)
- No-op fast path when SHA and merged-command both unchanged AND `--restore-deleted` is NOT set (the flag must inspect the working tree even when nothing template-side has changed)
- Stages full template render into `env::temp_dir()/clemp-update-<pid>` via `run_setup(merged, clone_dir, staging, check_conflicts=false, install_git_hooks=true)`
- Classifies each manifest entry via `classify_update_path(old_hash, cur_hash, new_hash, cwd_is_dir) -> UpdateClass`. A directory at a path where the template wants a file becomes `ShapeCollision` regardless of lockfile state.
- Preflight gates BEFORE any writes:
  - `shape_collisions` non-empty without `--force` → bail (Claude can't merge into a directory)
  - `conflicts` OR `collisions` non-empty without `--force` AND `claude` missing on PATH → bail
  - Any stale path that is a FILE on disk AND whose path is a strict prefix of some new/clean entry (file→directory template transition) AND `--prune-stale` not set → bail. Otherwise declining the later stale prompt would leave clean/new `create_dir_all` failing after merges had already landed.
- Apply order is: collisions (Claude or `--force`) → conflicts (Claude or `--force`) → shape-collisions (`--force` only) → stale prune (prompt-or-`--prune-stale`) → clean+new writes. Merges (the fail-prone step) run FIRST so a failed `merge_with_claude` leaves the project untouched — stale files still on disk, clean files at their old hashes, lockfile pinned to old SHA. Stale runs AFTER merges (so `--prune-stale` can't delete files that a later aborted merge would have rolled back) but BEFORE clean/new so file→directory template transitions unblock the new directory writes.
- `merge_with_claude` returns an error on non-zero `claude` exit; `run_update` propagates it without saving a new lockfile so a failed merge cannot advance the baseline.
- `apply_one` removes any directory present at the destination path before copying (handles `--force` shape-collision overwrites).
- Always re-runs `update_gitignore(clone_dir, ".")` at end of apply
- Persists a new lockfile using the staging-derived `new_manifest` (not on-disk CWD hashes) so future updates can detect user modifications

**Listing** (`list_category` + `list_available` in `src/lib.rs`)
- `list_category` scans a single category dir for named/opt-in files, returns sorted `Vec<String>`
- `list_available` formats output: "all" mode prints headers per non-empty category; single-category mode prints bare names
- Categories → dir + extension filter: `mcp/*.json`, `hooks/*.json`, `commands/*.md`, `githooks/*` (any file), `clarg/*.yaml|yml`, `gitignore-additions/*.gitignore` (excluding `default`), `claude-md/lang-rules/*.md`
- Only root-level files are listed (subdirs like `default/` and language dirs are excluded via `is_file()` filter); the gitignore category additionally drops the `default` stem so only per-language fragments are surfaced
- Invoked via `clemp list [CATEGORY]` (subcommand); `main.rs::run_list` clones, lists, and removes the clone dir — never touches CWD

**Config Persistence** (`load_config` / `save_config` in `src/lib.rs`)
- Path: `~/.config/clemp/clemp.yaml`
- Schema: `{gh-repo: <url>}`

**Clone + SHA capture** (`clone_repo` in `src/lib.rs`)
- `git clone --depth=1 <repo_url> claude-template/` after removing any stale prior clone
- Runs `git -C claude-template rev-parse HEAD` and returns the resulting SHA; both `run_setup_cmd` and `run_update_cmd` persist this SHA into the lockfile

### Naming Conventions
- Hook files: `hooks/default/<name>.json` for always-on, `hooks/<name>.json` for opt-in via `--hooks`
- Command files: `commands/default/<name>.md` for always-on, `commands/<lang>/<name>.md` for language-matched, `commands/<name>.md` for opt-in via `--commands`
- MCP files: `mcp/default/<name>.json` for always-on, `mcp/<lang>/<name>.json` for language-matched, `mcp/<name>.json` for opt-in via `--mcp`
- Git hook files: `githooks/default/<name>` for always-on, `githooks/<lang>/<name>` for language-matched, `githooks/<name>` for opt-in via `--githooks` (extensionless)
- Gitignore fragments: `gitignore-additions/default.gitignore` (always applied), `gitignore-additions/<canonical>.gitignore` (applied when that language resolves)
- Language rules: `claude-md/lang-rules/<canonical>.md`
- MCP rules: `claude-md/mcp-rules/<name>.md`
- Misc template sections: `claude-md/misc/<tag-name>.md` or `<tag-name>.md.jinja` — hyphens become underscores in template variable names
- Lockfile path: `.clemp-lock.yaml` at project root (defined by `LOCKFILE_NAME` constant)
