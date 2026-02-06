//! Tests for language normalization, resolution, rules building, and template rendering.

mod common;

use clemp::{
    build_language_rules, normalize_language, render_claude_md, resolve_language,
    template_has_conditional, LanguageResolution,
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

// ── template_has_conditional ────────────────────────────────────────────

#[test]
fn template_conditional_double_quotes() {
    let tmpl = r#"{% if "swift" in languages %}swift stuff{% endif %}"#;
    assert!(template_has_conditional(tmpl, "swift"));
    assert!(!template_has_conditional(tmpl, "rust"));
}

#[test]
fn template_conditional_single_quotes() {
    let tmpl = "{% if 'rust' in languages %}rust stuff{% endif %}";
    assert!(template_has_conditional(tmpl, "rust"));
}

#[test]
fn template_conditional_with_dash_syntax() {
    let tmpl = r#"{%- if "go" in languages -%}go stuff{%- endif -%}"#;
    assert!(template_has_conditional(tmpl, "go"));
}

#[test]
fn template_conditional_no_match() {
    assert!(!template_has_conditional("no conditionals here", "rust"));
    assert!(!template_has_conditional("", "rust"));
}

// ── resolve_language ────────────────────────────────────────────────────

#[test]
fn resolve_language_with_rules_file() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(rules_dir.join("typescript-rules.md"), "ts rules").unwrap();

    match resolve_language("ts", &rules_dir, "") {
        LanguageResolution::HasRulesFile(name) => assert_eq!(name, "typescript"),
        _ => panic!("Expected HasRulesFile"),
    }
}

#[test]
fn resolve_language_conditional_only() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();

    let template = r#"{% if "swift" in languages %}swift{% endif %}"#;
    match resolve_language("swift", &rules_dir, template) {
        LanguageResolution::ConditionalOnly(name) => assert_eq!(name, "swift"),
        _ => panic!("Expected ConditionalOnly"),
    }
}

#[test]
fn resolve_language_no_match() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();

    match resolve_language("brainfuck", &rules_dir, "") {
        LanguageResolution::NoMatch => {}
        _ => panic!("Expected NoMatch"),
    }
}

#[test]
fn resolve_language_unknown_input_falls_back_to_lowercase() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(rules_dir.join("zig-rules.md"), "zig stuff").unwrap();

    match resolve_language("ZIG", &rules_dir, "") {
        LanguageResolution::HasRulesFile(name) => assert_eq!(name, "zig"),
        _ => panic!("Expected HasRulesFile for unknown-but-present language"),
    }
}

// ── build_language_rules ────────────────────────────────────────────────

#[test]
fn build_language_rules_wraps_in_tags() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(rules_dir.join("rust-rules.md"), "  use iterators  \n").unwrap();
    fs::write(rules_dir.join("go-rules.md"), "handle errors").unwrap();

    let langs = vec!["rust".into(), "go".into()];
    let result = build_language_rules(&langs, &rules_dir).unwrap();

    assert!(result.contains("<rust-rules>\nuse iterators\n</rust-rules>"));
    assert!(result.contains("<go-rules>\nhandle errors\n</go-rules>"));
}

#[test]
fn build_language_rules_empty_list() {
    let s = Scaffold::new();
    let rules_dir = s.path().join("rules-templates");
    fs::create_dir_all(&rules_dir).unwrap();

    let result = build_language_rules(&[], &rules_dir).unwrap();
    assert_eq!(result, "");
}

// ── render_claude_md ────────────────────────────────────────────────────

#[test]
fn render_claude_md_basic() {
    let s = Scaffold::new();
    s.with_rules_template(
        "Languages: {{ languages | join(', ') }}\n{{ language_rules }}",
        &[("typescript-rules.md", "Use strict mode")],
    );

    let rules_dir = s.path().join("rules-templates");
    let (rendered, resolved) = render_claude_md(&["ts".into()], &rules_dir).unwrap();

    assert_eq!(resolved, vec!["typescript"]);
    assert!(rendered.contains("Languages: typescript"));
    assert!(rendered.contains("<typescript-rules>"));
    assert!(rendered.contains("Use strict mode"));
}

#[test]
fn render_claude_md_multiple_languages() {
    let s = Scaffold::new();
    s.with_rules_template(
        "{{ languages | join(', ') }}\n{{ language_rules }}",
        &[
            ("typescript-rules.md", "ts rules"),
            ("rust-rules.md", "rs rules"),
        ],
    );

    let rules_dir = s.path().join("rules-templates");
    let (rendered, resolved) =
        render_claude_md(&["ts".into(), "rs".into()], &rules_dir).unwrap();

    assert_eq!(resolved, vec!["typescript", "rust"]);
    assert!(rendered.contains("typescript, rust"));
    assert!(rendered.contains("<typescript-rules>"));
    assert!(rendered.contains("<rust-rules>"));
}

#[test]
fn render_claude_md_skips_unknown_language() {
    let s = Scaffold::new();
    s.with_rules_template("{{ languages | join(', ') }}", &[]);

    let rules_dir = s.path().join("rules-templates");
    let (rendered, resolved) = render_claude_md(&["nope".into()], &rules_dir).unwrap();

    assert!(resolved.is_empty());
    assert_eq!(rendered.trim(), "");
}

#[test]
fn render_claude_md_conditional_only_language_in_list() {
    let s = Scaffold::new();
    s.with_rules_template(
        r#"{% if "swift" in languages %}HAS_SWIFT{% endif %} {{ languages | join(', ') }}"#,
        &[],
    );

    let rules_dir = s.path().join("rules-templates");
    let (rendered, resolved) = render_claude_md(&["swift".into()], &rules_dir).unwrap();

    assert_eq!(resolved, vec!["swift"]);
    assert!(rendered.contains("HAS_SWIFT"));
}
