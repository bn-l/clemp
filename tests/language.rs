//! Tests for language resolution, rules building, MCP rules, misc rendering, and template rendering.

mod common;

use clemp::{
    build_language_rules, build_mcp_rules, normalize_language, render_claude_md,
    resolve_all_languages, resolve_language, LanguageResolution,
};
use common::Scaffold;
use std::fs;

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

// ── resolve_language ────────────────────────────────────────────────────

#[test]
fn resolve_with_rules_file() {
    let s = Scaffold::new();
    s.with_template("", &[("typescript.md", "ts rules")]);

    match resolve_language("ts", s.path()) {
        LanguageResolution::HasRulesFile(name) => assert_eq!(name, "typescript"),
        _ => panic!("Expected HasRulesFile"),
    }
}

#[test]
fn resolve_conditional_only() {
    let s = Scaffold::new();
    // No rules file, but svelte has a commands directory
    s.with_commands("svelte", &[("svelte.md", "cmd")]);

    match resolve_language("svelte", s.path()) {
        LanguageResolution::ConditionalOnly(name) => assert_eq!(name, "svelte"),
        _ => panic!("Expected ConditionalOnly"),
    }
}

#[test]
fn resolve_conditional_via_skills_dir() {
    let s = Scaffold::new();
    s.with_skills("svelte", &[("skill.md", "sk")]);

    match resolve_language("svelte", s.path()) {
        LanguageResolution::ConditionalOnly(name) => assert_eq!(name, "svelte"),
        _ => panic!("Expected ConditionalOnly"),
    }
}

#[test]
fn resolve_conditional_via_copied_dir() {
    let s = Scaffold::new();
    s.with_copied("swift", &[("guide.md", "g")]);

    match resolve_language("swift", s.path()) {
        LanguageResolution::ConditionalOnly(name) => assert_eq!(name, "swift"),
        _ => panic!("Expected ConditionalOnly"),
    }
}

#[test]
fn resolve_conditional_via_mcp_dir() {
    let s = Scaffold::new();
    s.with_lang_mcps("svelte", &[("svelte", r#"{"svelte": {}}"#)]);

    match resolve_language("svelte", s.path()) {
        LanguageResolution::ConditionalOnly(name) => assert_eq!(name, "svelte"),
        _ => panic!("Expected ConditionalOnly"),
    }
}

#[test]
fn resolve_no_match() {
    let s = Scaffold::new();

    match resolve_language("brainfuck", s.path()) {
        LanguageResolution::NoMatch => {}
        _ => panic!("Expected NoMatch"),
    }
}

#[test]
fn resolve_unknown_input_falls_back_to_lowercase() {
    let s = Scaffold::new();
    s.with_template("", &[("zig.md", "zig stuff")]);

    match resolve_language("ZIG", s.path()) {
        LanguageResolution::HasRulesFile(name) => assert_eq!(name, "zig"),
        _ => panic!("Expected HasRulesFile for unknown-but-present language"),
    }
}

// ── resolve_all_languages ───────────────────────────────────────────────

#[test]
fn resolve_all_errors_on_unknown() {
    let s = Scaffold::new();

    let result = resolve_all_languages(&["brainfuck".into()], s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Unknown language"));
}

#[test]
fn resolve_all_includes_conditional_only() {
    let s = Scaffold::new();
    s.with_template("", &[("typescript.md", "ts rules")]);
    s.with_commands("svelte", &[("svelte.md", "cmd")]);

    let resolved = resolve_all_languages(&["ts".into(), "svelte".into()], s.path()).unwrap();
    assert_eq!(resolved, vec!["typescript", "svelte"]);
}

// ── build_language_rules ────────────────────────────────────────────────

#[test]
fn build_language_rules_wraps_in_tags() {
    let s = Scaffold::new();
    s.with_template("", &[("rust.md", "  use iterators  \n"), ("go.md", "handle errors")]);

    let claude_md_dir = s.path().join("claude-md");
    let result = build_language_rules(&["rust".into(), "go".into()], &claude_md_dir).unwrap();

    assert!(result.contains("<rust-rules>\nuse iterators\n</rust-rules>"));
    assert!(result.contains("<go-rules>\nhandle errors\n</go-rules>"));
}

#[test]
fn build_language_rules_empty_list() {
    let s = Scaffold::new();
    let claude_md_dir = s.path().join("claude-md");
    fs::create_dir_all(&claude_md_dir).unwrap();

    let result = build_language_rules(&[], &claude_md_dir).unwrap();
    assert_eq!(result, "");
}

#[test]
fn build_language_rules_skips_conditional_only() {
    let s = Scaffold::new();
    s.with_template("", &[("typescript.md", "ts rules")]);

    let claude_md_dir = s.path().join("claude-md");
    // svelte has no rules file, should be skipped
    let result =
        build_language_rules(&["typescript".into(), "svelte".into()], &claude_md_dir).unwrap();

    assert!(result.contains("<typescript-rules>"));
    assert!(!result.contains("svelte"));
}

// ── build_mcp_rules ─────────────────────────────────────────────────────

#[test]
fn build_mcp_rules_wraps_in_tags() {
    let s = Scaffold::new();
    s.with_mcp_rules(&[("textbelt.md", "Use textbelt search")]);

    let claude_md_dir = s.path().join("claude-md");
    let result = build_mcp_rules(&["textbelt".into()], &claude_md_dir).unwrap();

    assert!(result.contains("<textbelt-mcp-rules>"));
    assert!(result.contains("Use textbelt search"));
    assert!(result.contains("</textbelt-mcp-rules>"));
}

#[test]
fn build_mcp_rules_skips_mcps_without_rules() {
    let s = Scaffold::new();
    s.with_mcp_rules(&[("textbelt.md", "rules")]);

    let claude_md_dir = s.path().join("claude-md");
    let result =
        build_mcp_rules(&["textbelt".into(), "context7".into()], &claude_md_dir).unwrap();

    assert!(result.contains("<textbelt-mcp-rules>"));
    assert!(!result.contains("context7"));
}

#[test]
fn build_mcp_rules_empty_mcps() {
    let s = Scaffold::new();
    let claude_md_dir = s.path().join("claude-md");
    fs::create_dir_all(&claude_md_dir).unwrap();

    let result = build_mcp_rules(&[], &claude_md_dir).unwrap();
    assert_eq!(result, "");
}

// ── render_claude_md ────────────────────────────────────────────────────

#[test]
fn render_basic_with_lang_rules() {
    let s = Scaffold::new();
    s.with_template(
        "Languages: {% for l in lang %}{{ l }} {% endfor %}\n{{ lang_rules }}",
        &[("typescript.md", "Use strict mode")],
    );

    let rendered = render_claude_md(&["typescript".into()], &[], s.path()).unwrap();
    assert!(rendered.contains("typescript"));
    assert!(rendered.contains("<typescript-rules>"));
    assert!(rendered.contains("Use strict mode"));
}

#[test]
fn render_lang_dict_conditional() {
    let s = Scaffold::new();
    s.with_template(
        "{% if lang %}HAS_LANG{% endif %}{% if lang.typescript %}HAS_TS{% endif %}{% if lang.rust %}HAS_RUST{% endif %}",
        &[("typescript.md", "ts")],
    );

    let rendered = render_claude_md(&["typescript".into()], &[], s.path()).unwrap();
    assert!(rendered.contains("HAS_LANG"));
    assert!(rendered.contains("HAS_TS"));
    assert!(!rendered.contains("HAS_RUST"));
}

#[test]
fn render_mcp_dict_conditional() {
    let s = Scaffold::new();
    s.with_template(
        "{% if mcp %}HAS_MCP{% endif %}{% if mcp.maps %}HAS_MAPS{% endif %}{% if mcp.docker %}HAS_DOCKER{% endif %}",
        &[],
    );

    let rendered = render_claude_md(&[], &["maps".into()], s.path()).unwrap();
    assert!(rendered.contains("HAS_MCP"));
    assert!(rendered.contains("HAS_MAPS"));
    assert!(!rendered.contains("HAS_DOCKER"));
}

#[test]
fn render_no_lang_no_mcp() {
    let s = Scaffold::new();
    s.with_template(
        "{% if lang %}LANG{% endif %}{% if mcp %}MCP{% endif %}base",
        &[],
    );

    let rendered = render_claude_md(&[], &[], s.path()).unwrap();
    assert!(!rendered.contains("LANG"));
    assert!(!rendered.contains("MCP"));
    assert!(rendered.contains("base"));
}

#[test]
fn render_misc_plain_file() {
    let s = Scaffold::new();
    s.with_template("{{ general }}", &[]);
    s.with_misc_files(&[("general.md", "Be helpful")]);

    let rendered = render_claude_md(&[], &[], s.path()).unwrap();
    assert!(rendered.contains("<general>"));
    assert!(rendered.contains("Be helpful"));
    assert!(rendered.contains("</general>"));
}

#[test]
fn render_misc_jinja_file() {
    let s = Scaffold::new();
    s.with_template("{{ lang_hygiene }}", &[]);
    s.with_misc_files(&[(
        "lang-hygiene.md.jinja",
        "{% if lang.swift %}Swift: check packages{% endif %}",
    )]);

    let rendered = render_claude_md(&["swift".into()], &[], s.path()).unwrap();
    assert!(rendered.contains("<lang-hygiene>"));
    assert!(rendered.contains("Swift: check packages"));
}

#[test]
fn render_misc_jinja_conditional_false() {
    let s = Scaffold::new();
    s.with_template("{{ lang_hygiene }}", &[]);
    s.with_misc_files(&[(
        "lang-hygiene.md.jinja",
        "base{% if lang.swift %}\nSwift stuff{% endif %}",
    )]);

    let rendered = render_claude_md(&[], &[], s.path()).unwrap();
    assert!(rendered.contains("<lang-hygiene>"));
    assert!(rendered.contains("base"));
    assert!(!rendered.contains("Swift stuff"));
}

#[test]
fn render_misc_underscore_filename() {
    let s = Scaffold::new();
    s.with_template("{{ web_search_tutorial }}", &[]);
    s.with_misc_files(&[("web-search-tutorial.md", "Search tips")]);

    let rendered = render_claude_md(&[], &[], s.path()).unwrap();
    assert!(rendered.contains("<web-search-tutorial>"));
    assert!(rendered.contains("Search tips"));
}

#[test]
fn render_mcp_rules_included() {
    let s = Scaffold::new();
    s.with_template("{{ mcp_rules }}", &[]);
    s.with_mcp_rules(&[("textbelt.md", "Use textbelt for search")]);

    let rendered = render_claude_md(&[], &["textbelt".into()], s.path()).unwrap();
    assert!(rendered.contains("<textbelt-mcp-rules>"));
    assert!(rendered.contains("Use textbelt for search"));
}

#[test]
fn render_full_template() {
    let s = Scaffold::new();
    s.with_template(
        "{{ general }}\n{%- if lang %}\n{{ style_general }}\n{{ lang_rules }}\n{%- endif %}\n{{ mcp_rules }}\n{{ CRITICAL }}",
        &[("typescript.md", "strict types")],
    );
    s.with_misc_files(&[
        ("general.md", "Be helpful"),
        ("style-general.md", "Write clean code"),
        ("CRITICAL.md.jinja", "{% if lang %}Check REPOMAP{% endif %}"),
    ]);
    s.with_mcp_rules(&[("textbelt.md", "Use textbelt")]);

    let rendered =
        render_claude_md(&["typescript".into()], &["textbelt".into()], s.path()).unwrap();

    assert!(rendered.contains("<general>"));
    assert!(rendered.contains("Be helpful"));
    assert!(rendered.contains("<style-general>"));
    assert!(rendered.contains("Write clean code"));
    assert!(rendered.contains("<typescript-rules>"));
    assert!(rendered.contains("strict types"));
    assert!(rendered.contains("<textbelt-mcp-rules>"));
    assert!(rendered.contains("<CRITICAL>"));
    assert!(rendered.contains("Check REPOMAP"));
}

// ── resolve_all_languages (edge) ────────────────────────────────────────

#[test]
fn resolve_all_empty_input() {
    let s = Scaffold::new();
    let result = resolve_all_languages(&[], s.path()).unwrap();
    assert!(result.is_empty());
}

// ── build_mcp_rules (multi joined) ──────────────────────────────────────

#[test]
fn build_mcp_rules_multi_joined() {
    let s = Scaffold::new();
    s.with_mcp_rules(&[
        ("textbelt.md", "Use textbelt search"),
        ("maps.md", "Use maps API"),
    ]);

    let claude_md_dir = s.path().join("claude-md");
    let result = build_mcp_rules(&["textbelt".into(), "maps".into()], &claude_md_dir).unwrap();

    assert!(result.contains("<textbelt-mcp-rules>"));
    assert!(result.contains("</textbelt-mcp-rules>"));
    assert!(result.contains("<maps-mcp-rules>"));
    assert!(result.contains("</maps-mcp-rules>"));
    // Sections separated by blank line
    assert!(result.contains("</textbelt-mcp-rules>\n\n<maps-mcp-rules>"));
}

// ── render_claude_md (misc underscore in filename) ──────────────────────

#[test]
fn render_misc_underscore_in_filename() {
    let s = Scaffold::new();
    s.with_template("{{ my_notes }}", &[]);
    s.with_misc_files(&[("my_notes.md", "Personal notes here")]);

    let rendered = render_claude_md(&[], &[], s.path()).unwrap();
    assert!(rendered.contains("<my_notes>"));
    assert!(rendered.contains("Personal notes here"));
    assert!(rendered.contains("</my_notes>"));
}

// ── render_claude_md (error paths) ──────────────────────────────────────

#[test]
fn render_error_no_template() {
    let s = Scaffold::new();
    // No CLAUDE.md.jinja created
    let result = render_claude_md(&[], &[], s.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("CLAUDE.md.jinja"));
}

#[test]
fn render_ok_no_claude_md_dir() {
    let s = Scaffold::new();
    s.with_template("just text", &[]);

    let result = render_claude_md(&[], &[], s.path()).unwrap();
    assert_eq!(result, "just text");
}
