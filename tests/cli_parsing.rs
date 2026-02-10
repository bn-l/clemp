//! Tests for CLI argument parsing and split_multi_values.

use clap::Parser;
use clemp::{split_multi_values, Cli};

// ── split_multi_values ──────────────────────────────────────────────────

#[test]
fn split_multi_values_comma_only() {
    let input = vec!["a,b,c".into()];
    // commas aren't split by split_multi_values — clap's value_delimiter does that.
    assert_eq!(split_multi_values(input), vec!["a,b,c"]);
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

// ── CLI parsing ─────────────────────────────────────────────────────────

#[test]
fn cli_mcp_space_separated() {
    let cli =
        Cli::try_parse_from(["clemp", "ts", "--mcp", "context7", "sequential-thinking"]).unwrap();
    assert_eq!(cli.mcp, vec!["context7", "sequential-thinking"]);
}

#[test]
fn cli_mcp_comma_separated() {
    let cli =
        Cli::try_parse_from(["clemp", "ts", "--mcp", "context7,sequential-thinking"]).unwrap();
    assert_eq!(cli.mcp, vec!["context7", "sequential-thinking"]);
}

#[test]
fn cli_mcp_mixed_comma_and_space() {
    let cli = Cli::try_parse_from(["clemp", "ts", "--mcp", "a,b", "c"]).unwrap();
    assert_eq!(cli.mcp, vec!["a", "b", "c"]);
}

#[test]
fn cli_hooks_space_separated() {
    let cli = Cli::try_parse_from(["clemp", "ts", "--hooks", "sound", "lint"]).unwrap();
    assert_eq!(cli.hooks, vec!["sound", "lint"]);
}

#[test]
fn cli_hooks_comma_separated() {
    let cli = Cli::try_parse_from(["clemp", "ts", "--hooks", "sound,lint"]).unwrap();
    assert_eq!(cli.hooks, vec!["sound", "lint"]);
}

#[test]
fn cli_no_flags_gives_empty_vecs() {
    let cli = Cli::try_parse_from(["clemp", "ts"]).unwrap();
    assert!(cli.hooks.is_empty());
    assert!(cli.mcp.is_empty());
}

#[test]
fn cli_repeated_flag() {
    let cli = Cli::try_parse_from(["clemp", "ts", "--mcp", "a", "--mcp", "b"]).unwrap();
    assert_eq!(cli.mcp, vec!["a", "b"]);
}

#[test]
fn cli_languages_positional() {
    let cli = Cli::try_parse_from(["clemp", "ts", "python", "rust"]).unwrap();
    assert_eq!(cli.languages, vec!["ts", "python", "rust"]);
}

#[test]
fn cli_no_languages() {
    let cli = Cli::try_parse_from(["clemp"]).unwrap();
    assert!(cli.languages.is_empty());
}

#[test]
fn cli_all_options_combined() {
    let cli = Cli::try_parse_from([
        "clemp",
        "ts",
        "python",
        "--hooks",
        "sound",
        "lint",
        "--mcp",
        "context7",
        "sequential-thinking",
    ])
    .unwrap();
    assert_eq!(cli.languages, vec!["ts", "python"]);
    assert_eq!(cli.hooks, vec!["sound", "lint"]);
    assert_eq!(cli.mcp, vec!["context7", "sequential-thinking"]);
}

#[test]
fn cli_double_dash_separates_positionals() {
    let cli = Cli::try_parse_from([
        "clemp",
        "--mcp",
        "context7",
        "--",
        "ts",
        "python",
    ])
    .unwrap();
    assert_eq!(cli.mcp, vec!["context7"]);
    assert_eq!(cli.languages, vec!["ts", "python"]);
}

#[test]
fn cli_hooks_mixed_comma_and_space() {
    let cli = Cli::try_parse_from(["clemp", "ts", "--hooks", "a,b", "c"]).unwrap();
    assert_eq!(cli.hooks, vec!["a", "b", "c"]);
}
