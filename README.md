<p align="center">
  <img src="./media/clemp-logo.png" alt="clemp" width="300">
</p>

A CLI tool for scaffolding [Claude Code](https://docs.anthropic.com/en/docs/claude-code) config files. It clones a repo and and sets up the various files (CLAUDE.md, .mcp.json, .claude dir) depening on the arguments you give it.

## Installation

With homebrew:
```bash
brew install bn-l/tap/clemp
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

# With clarg argument guard
clemp ts --clarg strict
```

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `--hooks` | `sound` | Hook names to include (comma-separated) |
| `--mcp` | `context7` | MCP server names to keep (comma-separated) |
| `--clarg` | — | Clarg config profile to enable (see below) |

## Clarg Integration

[clarg](https://github.com/bn-l/clarg) is a `PreToolUse` hook that blocks risky commands, arguments, and file access in Claude Code. clemp can set it up automatically.

### Setup

1. Add a `clarg/` directory to your template repo with YAML config files:

```
claude-template/
└── clarg/
    ├── strict.yaml
    └── permissive.yaml
```

Each YAML file is a clarg config (see [clarg docs](https://github.com/bn-l/clarg) for the schema):

```yaml
block_access_to:
  - ".env"
  - "*.secret"
commands_forbidden:
  - "rm -rf"
  - "sudo"
internal_access_only: true
```

2. Pass the config name to clemp:

```bash
clemp ts --clarg strict
```

This copies `clarg/strict.yaml` to `.claude/clarg-strict.yaml` and registers a `PreToolUse` hook in `.claude/settings.local.json` that runs `clarg .claude/clarg-strict.yaml`.

### Installing clarg

If clarg is not on your PATH, clemp will print install instructions:

```bash
brew install bn-l/tap/clarg
# or
cargo install --git https://github.com/bn-l/clarg
```

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
├── clarg/                        # Optional clarg configs
│   └── strict.yaml
├── lang-files/                  # Optional language-specific files
│   ├── typescript/
│   │   └── ...                  # Files copied when ts/typescript specified
│   ├── swift/
│   │   └── ...
│   └── ...
└── gitignore-additions
```

### Language-specific files

The `lang-files/` directory allows you to include extra files that are only copied when a specific language is specified. For example, if you run `clemp swift`, any files in `lang-files/swift/` will be copied to your project root.

The `CLAUDE-template.md` uses [MiniJinja](https://github.com/mitsuhiko/minijinja) syntax with access to:
- `languages` - list of canonical language names
- `language_rules` - rendered language rule sections
