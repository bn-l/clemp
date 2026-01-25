<p align="center">
  <img src="./media/clemp-logo.png" alt="clemp" width="300">
</p>

A CLI tool for scaffolding [Claude Code](https://docs.anthropic.com/en/docs/claude-code) config files. It clones a repo and and sets up the various files (CLAUDE.md, .mcp.json, .claude dir) depening on the arguments you give it.

## Installation

With homebrew:
```bash
brew install bn-l/clemp/clemp
```

Or cargo: clone this then:
```bash
cargo install --path .
```

## Usage

```bash
clemp [LANGUAGES]... [OPTIONS]
```

On first run, you'll be prompted to provide a url to your repo. This is saved to `~/.config/clemp/clemp.yaml`.

### Examples

```bash
# Configure for a TypeScript project
clemp ts

# Multiple languages
clemp rust typescript

# With specific hooks and MCP servers
clemp python --hooks sound,lint --mcp context7,filesystem
```

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `--hooks` | `sound` | Hook names to include (comma-separated) |
| `--mcp` | `context7` | MCP server names to keep (comma-separated) |

## Template Repository Structure

Your `claude-template` repo should contain:

```
claude-template/
├── .claude/
│   └── settings.local.json
├── .mcp.json
├── rules-templates/
│   ├── CLAUDE-template.md      # Jinja2 template
│   ├── typescript-rules.md
│   ├── python-rules.md
│   └── ...
├── hooks-template/
│   ├── sound.json
│   └── ...
└── gitignore-additions
```

The `CLAUDE-template.md` uses [MiniJinja](https://github.com/mitsuhiko/minijinja) syntax with access to:
- `languages` - list of canonical language names
- `language_rules` - rendered language rule sections