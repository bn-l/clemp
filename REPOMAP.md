<!-- commit: 030d7594927264903637adcb8a9a069a729e7bc1 -->

### Quick Reference
- **Critical Paths**: `run_setup` orchestrates the entire pipeline in 3 phases — (1) clone_dir prep: language resolution → MCP assembly → template rendering → settings/hooks/clarg, (2) pre-flight conflict check against CWD, (3) CWD mutations: gitignore, file copying. All CWD writes are gated behind phase 2 so a conflict aborts cleanly.
- **Architectural Rules**:
  - `COPY_FILES_EXCLUDE` (module-level constant in `src/lib.rs`) must stay in sync with template structure dirs (`commands`, `skills`, `copied`, `hooks`, `mcp`, `clarg`, `claude-md`, etc.)
  - Conflict checking is centralized in `run_setup` phase 2 via `collect_copy_files_sources` + `collect_conditional_dir_sources` + `check_no_conflicts` — individual copy functions (`copy_files`, `copy_conditional_dir`) do **not** check conflicts themselves
  - Language resolution checks 4 conditional dirs: `commands`, `skills`, `copied`, `mcp` — adding a new conditional dir requires updating `resolve_language`
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
- `--clarg <name>` — clarg config profile (single value, maps to `clarg/<name>.yaml` in template)
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
- Output: `.claude/clarg-<name>.yaml` + `PreToolUse` hook entry merged into settings
- PATH check: warns with install instructions if `clarg` binary not found

**Conditional Dirs** (`copy_conditional_dir` in `src/lib.rs`)
- Pattern: `<source_dir>/default/` + `<source_dir>/<lang>/` → merged into dest (lang overrides default)
- Applied to: `commands` → `.claude/commands`, `skills` → `.claude/skills`, `copied` → `.` (project root)

**Filesystem** (`copy_files` in `src/lib.rs`)
- Copies everything from clone dir root to `.` except the `exclude` list
- `check_no_conflicts` prevents overwriting existing files

**Config Persistence** (`load_config` / `save_config` in `src/lib.rs`)
- Path: `~/.config/clemp/clemp.yaml`
- Schema: `{gh-repo: <url>}`

### Naming Conventions
- Hook files: `hooks/default/<name>.json` for always-on, `hooks/<name>.json` for opt-in via `--hooks`
- MCP files: `mcp/default/<name>.json` for always-on, `mcp/<lang>/<name>.json` for language-matched, `mcp/<name>.json` for opt-in via `--mcp`
- Language rules: `claude-md/lang-rules/<canonical>.md`
- MCP rules: `claude-md/mcp-rules/<name>.md`
- Misc template sections: `claude-md/misc/<tag-name>.md` or `<tag-name>.md.jinja` — hyphens become underscores in template variable names
