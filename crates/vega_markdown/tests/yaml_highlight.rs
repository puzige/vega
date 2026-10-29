use vega_markdown::{HighlightKind, highlight, language_display_name};

fn assert_token(code: &str, token: &str, kind: HighlightKind) {
    let start = code.find(token).expect("sample token must exist");
    let end = start + token.len();
    let spans = highlight(code, "yaml").expect("YAML query must load");
    assert!(
        spans
            .iter()
            .any(|span| span.start_byte <= start && span.end_byte >= end && span.kind == kind),
        "{token:?} should be {kind:?}; got {spans:?}"
    );
}

#[test]
fn yaml_distinguishes_keys_values_and_scalar_kinds() {
    let code = "名称: Vega\n\"quoted key\": \"你好\"\ncount: 42\nratio: 3.14\nenabled: true\nmissing: null\n# 中文注释\nflow: {nested: 'value', items: [1, false]}\nmessage: |\n  多行内容\n";
    for (token, kind) in [
        ("名称", HighlightKind::Property),
        ("\"quoted key\"", HighlightKind::Property),
        ("nested", HighlightKind::Property),
        ("Vega", HighlightKind::String),
        ("\"你好\"", HighlightKind::String),
        ("'value'", HighlightKind::String),
        ("42", HighlightKind::Number),
        ("3.14", HighlightKind::Number),
        ("true", HighlightKind::Constant),
        ("null", HighlightKind::Constant),
        ("false", HighlightKind::Constant),
        ("# 中文注释", HighlightKind::Comment),
        ("多行内容", HighlightKind::String),
    ] {
        assert_token(code, token, kind);
    }
}

#[test]
fn yaml_aliases_share_display_and_highlight_with_valid_utf8_spans() {
    let code = "defaults: &defaults\n  名称: \"中文😀\"\ncopy: *defaults\nlabel: !!str 42\n";
    let expected = highlight(code, "yaml").expect("YAML query must load");
    assert!(!expected.is_empty());
    for language in ["yaml", "yml", "YAML", "YmL"] {
        assert_eq!(language_display_name(Some(language)), "YAML");
        assert_eq!(highlight(code, language), Some(expected.clone()));
    }
    assert_token(code, "!!str", HighlightKind::Type);
    assert_token(code, "defaults", HighlightKind::Property);
    let mut end = 0;
    for span in expected {
        assert!(end <= span.start_byte && span.start_byte < span.end_byte);
        assert!(span.end_byte <= code.len());
        assert!(code.is_char_boundary(span.start_byte));
        assert!(code.is_char_boundary(span.end_byte));
        end = span.end_byte;
    }
    for token in ["&defaults", "*defaults"] {
        let start = code.find(token).expect("anchor token must exist") + 1;
        assert!(
            highlight(code, "yaml")
                .expect("YAML query must load")
                .iter()
                .any(|span| span.kind == HighlightKind::Variable
                    && span.start_byte <= start
                    && span.end_byte >= start + "defaults".len())
        );
    }
}

#[test]
fn yaml_addition_preserves_existing_grammars_and_safe_fallbacks() {
    for (language, code) in [
        ("rust", "fn main() { let n = 42; }"),
        ("javascript", "const n = 42;"),
        ("typescript", "const n: number = 42;"),
        ("tsx", "const el = <div title=\"hello\" />;"),
        ("python", "def greet():\n    return 'hello'\n"),
        ("json", "{\"name\": \"Vega\", \"count\": 42}"),
        ("bash", "if true; then echo hello; fi"),
        ("html", "<div title=\"hello\">Vega</div>"),
        ("css", ".card { color: red; }"),
        ("sql", "SELECT name FROM users WHERE id = 42;"),
    ] {
        assert!(
            !highlight(code, language)
                .expect("existing grammar must load")
                .is_empty()
        );
    }
    assert_eq!(highlight("", "yaml"), Some(Vec::new()));
    assert!(highlight("fn main() {}", "text").is_none());
    assert!(highlight("name: Vega", "unknown").is_none());
    assert_eq!(language_display_name(None), "Text");
    assert_eq!(language_display_name(Some("unknown")), "unknown");
    assert!(highlight("broken: [\"unfinished", "yaml").is_some());
}
