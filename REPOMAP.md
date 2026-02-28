<!-- commit: 289ba0092be3e26f7b1a282b7fabb330b0f1f7b5 -->

### Quick Reference
- **Critical Paths**: `run_setup` orchestrates the entire pipeline in 3 phases — (1) clone_dir prep: language resolution → MCP assembly → template rendering → settings/hooks/clarg → commands/skills assembly (conditional dirs + named commands), (2) pre-flight conflict check against CWD + `.git/hooks/`, (3) CWD mutations: gitignore, file copying, git hooks installation. All CWD writes are gated behind phase 2 so a conflict aborts cleanly.
- **Architectural Rules**:
  - `COPY_FILES_EXCLUDE` (module-level constant in `src/lib.rs`) must stay in sync with template structure dirs (`commands`, `skills`, `copied`, `hooks`, `mcp`, `githooks`, `clarg`, `claude-md`, etc.)
  - Conflict checking is centralized in `run_setup` phase 2 via `collect_copy_files_sources` + `collect_conditional_dir_sources` + `collect_conflicts` — individual copy functions (`copy_files`, `copy_conditional_dir`) do **not** check conflicts themselves. With `--force`, conflicts are shown, user is prompted for confirmation, and conflicting paths are removed before copying.
  - Language resolution checks 5 conditional dirs: `commands`, `skills`, `copied`, `mcp`, `githooks` — adding a new conditional dir requires updating `resolve_language`
  - MCP assembly merges 3 layers in order: `default/` → language dirs → `--mcp` named files. Later layers override.

### Types & Schemas
- (`src/lib.rs`, CLI argument definition, `Cli`)
- (`src/lib.rs`, persistent config at `~/.config/clemp/clemp.yaml`, `Config`)
- (`src/lib.rs`, language resolution result, `LanguageResolution { HasRulesFile, ConditionalOnly, NoMatch }`)

### Integration Points

**CLI Surface** (`src/main.rs` → `src/lib.rs:Cli`)
- Positional: `[LANGUAGE...]` — language names/aliases
- `--hooks <name,...>` — extra hook names (comma or space separated, post-processed by `split_multi_values`)
- `--mcp <name,...>` — extra MCP server names (comma or space separated, post-processed by `split_multi_values`)
- `--commands <name,...>` — extra command names (comma or space separated, post-processed by `split_multi_values`)
- `--githooks <name,...>` — git hook scripts to install into `.git/hooks/` (comma or space separated, post-processed by `split_multi_values`)
- `--clarg <name>` — clarg config profile (single value, maps to `clarg/<name>.yaml` in template)
- `--force` — overwrite existing files/directories (with confirmation prompt)
- `--list [category]` — list available template files; optional category: `mcp`, `hooks`, `commands`, `githooks`, `clarg`, `languages` (omit for all)
- `-v` / `--version` — prints version from `Cargo.toml`

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
- Applied to: `commands` → `.claude/commands`, `skills` → `.claude/skills`, `copied` → `.` (project root)

**Git Hooks** (`copy_conditional_githooks` + `copy_named_githooks` in `src/lib.rs`)
- Sources: `githooks/default/` + `githooks/<lang>/` (conditional) + `githooks/<name>` (named, extensionless files at root)
- Destination: `.git/hooks/` — all copied files are set executable (0o755)
- Named hooks override conditional hooks with the same name (applied after)
- Skipped with warning if no `.git/` directory exists
- Conflict check includes `.git/hooks/` targets in phase 2

**Named Commands** (`copy_named_commands` in `src/lib.rs`)
- Sources: `commands/<name>.md` — standalone `.md` files at root of commands dir
- Copied into `.claude/commands/` after conditional dir assembly (named files override defaults/lang with same name)
- Mirrors the `--mcp` / `--hooks` named-file pattern

**Filesystem** (`copy_files` in `src/lib.rs`)
- Copies everything from clone dir root to `.` except the `exclude` list
- `collect_conflicts` detects existing files; `--force` with confirmation prompt allows overwriting

**Listing** (`list_category` + `list_available` in `src/lib.rs`)
- `list_category` scans a single category dir for named/opt-in files, returns sorted `Vec<String>`
- `list_available` formats output: "all" mode prints headers per non-empty category; single-category mode prints bare names
- Categories → dir + extension filter: `mcp/*.json`, `hooks/*.json`, `commands/*.md`, `githooks/*` (any file), `clarg/*.yaml|yml`, `claude-md/lang-rules/*.md`
- Only root-level files are listed (subdirs like `default/` and language dirs are excluded via `is_file()` filter)
- In `main.rs`, `--list` early-exits after clone → list → cleanup (skips `run_setup`)

**Config Persistence** (`load_config` / `save_config` in `src/lib.rs`)
- Path: `~/.config/clemp/clemp.yaml`
- Schema: `{gh-repo: <url>}`

### Naming Conventions
- Hook files: `hooks/default/<name>.json` for always-on, `hooks/<name>.json` for opt-in via `--hooks`
- Command files: `commands/default/<name>.md` for always-on, `commands/<lang>/<name>.md` for language-matched, `commands/<name>.md` for opt-in via `--commands`
- MCP files: `mcp/default/<name>.json` for always-on, `mcp/<lang>/<name>.json` for language-matched, `mcp/<name>.json` for opt-in via `--mcp`
- Git hook files: `githooks/default/<name>` for always-on, `githooks/<lang>/<name>` for language-matched, `githooks/<name>` for opt-in via `--githooks` (extensionless)
- Language rules: `claude-md/lang-rules/<canonical>.md`
- MCP rules: `claude-md/mcp-rules/<name>.md`
- Misc template sections: `claude-md/misc/<tag-name>.md` or `<tag-name>.md.jinja` — hyphens become underscores in template variable names
