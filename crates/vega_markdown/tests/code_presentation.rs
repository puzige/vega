use vega_markdown::{HighlightKind as Kind, highlight, language_display_name};

fn assert_token(language: &str, code: &str, token: &str, kind: Kind) {
    let spans = highlight(code, language).expect("supported grammar and query");
    let start = code.find(token).expect("fixture token");
    let end = start + token.len();
    assert!(
        spans
            .iter()
            .any(|span| span.kind == kind && span.start_byte <= start && span.end_byte >= end),
        "{language}: {token:?} should be {kind:?}; got {spans:?}"
    );
    let mut previous = 0;
    for span in spans {
        assert!(span.start_byte >= previous);
        assert!(span.start_byte < span.end_byte && span.end_byte <= code.len());
        assert!(code.is_char_boundary(span.start_byte) && code.is_char_boundary(span.end_byte));
        previous = span.end_byte;
    }
}

#[test]
fn rust_example_distinguishes_keywords_functions_types_numbers_operators_and_strings() {
    let code = "fn main() { let values: i32 = 42 + 2; println!(\"平方和\"); }";
    for (token, kind) in [
        ("fn", Kind::Keyword),
        ("let", Kind::Keyword),
        ("main", Kind::Function),
        ("i32", Kind::Type),
        ("42", Kind::Constant),
        ("=", Kind::Operator),
        ("+", Kind::Operator),
        ("println!", Kind::Function),
        ("\"平方和\"", Kind::String),
    ] {
        assert_token("rust", code, token, kind);
    }
}

#[test]
fn additional_languages_highlight_meaningful_syntax() {
    for (lang, code, token, kind) in [
        (
            "json",
            "{\"名称\": \"中文\", \"count\": 42, \"ok\": true}",
            "\"名称\"",
            Kind::Property,
        ),
        ("json", "{\"value\": \"中文\"}", "\"中文\"", Kind::String),
        ("json", "{\"count\": 42}", "42", Kind::Number),
        ("json", "{\"ok\": true}", "true", Kind::Constant),
        (
            "bash",
            "if true; then echo \"中文\"; fi",
            "if",
            Kind::Keyword,
        ),
        ("bash", "echo \"中文\"", "echo", Kind::Function),
        ("bash", "echo \"中文\"", "\"中文\"", Kind::String),
        ("html", "<div class=\"card\">中文</div>", "div", Kind::Type),
        (
            "html",
            "<div class=\"card\">中文</div>",
            "class",
            Kind::Attribute,
        ),
        (
            "html",
            "<div class=\"card\">中文</div>",
            "card",
            Kind::String,
        ),
        ("css", "div { color: red; width: 42px; }", "div", Kind::Type),
        (
            "css",
            "div { color: red; width: 42px; }",
            "color",
            Kind::Property,
        ),
        ("css", "div { width: 42px; }", "42", Kind::Number),
        (
            "sql",
            "SELECT name FROM users WHERE active = true AND score > 42;",
            "SELECT",
            Kind::Keyword,
        ),
        (
            "sql",
            "SELECT name FROM users WHERE score > 42;",
            "42",
            Kind::Number,
        ),
        ("sql", "SELECT '中文';", "'中文'", Kind::String),
    ] {
        assert_token(lang, code, token, kind);
    }
}

#[test]
fn existing_grammars_remain_usable() {
    for (language, code, token, kind) in [
        ("js", "const value = \"中文\";", "const", Kind::Keyword),
        (
            "ts",
            "const value: string = \"中文\";",
            "string",
            Kind::Type,
        ),
        (
            "tsx",
            "const element = <div>中文</div>;",
            "const",
            Kind::Keyword,
        ),
        (
            "py",
            "def greet():\n    return \"中文\"",
            "def",
            Kind::Keyword,
        ),
    ] {
        assert_token(language, code, token, kind);
    }
}

#[test]
fn aliases_fallbacks_and_invalid_syntax_are_safe() {
    for (tag, canonical, code) in [
        ("RS", "rust", "fn main() {}"),
        ("JS", "javascript", "const x = 1;"),
        ("TS", "typescript", "const x: number = 1;"),
        ("PY", "python", "x = 1"),
        ("SH", "bash", "echo hi"),
        ("shell", "bash", "echo hi"),
        ("JSON", "json", "{\"x\": 1}"),
        ("HTML", "html", "<div/>"),
        ("CSS", "css", "a { color: red; }"),
        ("SQL", "sql", "SELECT 1;"),
    ] {
        assert_eq!(highlight(code, tag), highlight(code, canonical));
    }
    for language in [
        "rust", "js", "ts", "tsx", "py", "json", "bash", "html", "css", "sql",
    ] {
        assert_eq!(highlight("", language), Some(vec![]));
        assert!(highlight("{未完成 \"", language).is_some());
    }
    for tag in ["text", "plaintext", "unknown-language", ""] {
        assert!(highlight("plain text", tag).is_none());
    }
    for (tag, expected) in [
        (None, "Text"),
        (Some("text"), "Text"),
        (Some("JS"), "JavaScript"),
        (Some("ts"), "TypeScript"),
        (Some("json"), "JSON"),
        (Some("sh"), "Bash"),
        (Some("html"), "HTML"),
        (Some("css"), "CSS"),
        (Some("sql"), "SQL"),
        (Some("CustomTag"), "CustomTag"),
    ] {
        assert_eq!(language_display_name(tag), expected);
    }
}
