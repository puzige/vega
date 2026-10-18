use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::HashMap;

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

/// One highlighted byte range inside a code block.
///
/// Offsets are UTF-8 byte offsets into the `code` string passed to
/// [`highlight`]; spans are sorted and non-overlapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    /// Inclusive start byte offset.
    pub start_byte: usize,
    /// Exclusive end byte offset.
    pub end_byte: usize,
    /// Semantic token kind (theme-facing).
    pub kind: HighlightKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightKind {
    Keyword,
    String,
    Comment,
    Function,
    Type,
    Number,
    Operator,
    Punctuation,
    Variable,
    Property,
    Constant,
    Escape,
    Attribute,
}

const CAPTURE_KINDS: &[(&str, HighlightKind)] = &[
    ("keyword", HighlightKind::Keyword),
    ("string", HighlightKind::String),
    ("string.special", HighlightKind::String),
    ("comment", HighlightKind::Comment),
    ("comment.documentation", HighlightKind::Comment),
    ("function", HighlightKind::Function),
    ("function.method", HighlightKind::Function),
    ("function.macro", HighlightKind::Function),
    ("function.builtin", HighlightKind::Function),
    ("type", HighlightKind::Type),
    ("type.builtin", HighlightKind::Type),
    ("constructor", HighlightKind::Type),
    ("number", HighlightKind::Number),
    ("operator", HighlightKind::Operator),
    ("punctuation", HighlightKind::Punctuation),
    ("punctuation.bracket", HighlightKind::Punctuation),
    ("punctuation.delimiter", HighlightKind::Punctuation),
    ("punctuation.special", HighlightKind::Punctuation),
    ("variable", HighlightKind::Variable),
    ("variable.builtin", HighlightKind::Variable),
    ("variable.parameter", HighlightKind::Variable),
    ("constant", HighlightKind::Constant),
    ("constant.builtin", HighlightKind::Constant),
    ("property", HighlightKind::Property),
    ("escape", HighlightKind::Escape),
    ("attribute", HighlightKind::Attribute),
];

/// Supported grammars（四语言方案，架构师 2026-08-29 裁决：rust / typescript /
/// javascript / python；typescript crate 附带 tsx 语言，同 crate 零额外依赖。
/// markdown grammar 因硬依赖 tree-sitter ^0.19 被砍掉，未来走 fork/vendor 另议）.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Grammar {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
}

impl Grammar {
    /// Fence info word → grammar，含常见别名（大小写不敏感，调用方先 lowercase）.
    fn for_language_tag(tag: &str) -> Option<Self> {
        match tag {
            "rust" | "rs" => Some(Self::Rust),
            "typescript" | "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "javascript" | "js" => Some(Self::JavaScript),
            "python" | "py" => Some(Self::Python),
            _ => None,
        }
    }

    /// Grammar entry point as the current core's `Language`.
    fn language(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }

    /// Upstream highlight query. TypeScript layers the JavaScript base query
    /// first and the TS-specific patterns after: later patterns win, so
    /// TS-specific captures (e.g. uppercase identifiers as types) take
    /// precedence over the generic JavaScript ones.
    fn highlights_query(self) -> String {
        match self {
            Self::Rust => tree_sitter_rust::HIGHLIGHTS_QUERY.to_string(),
            Self::TypeScript | Self::Tsx => format!(
                "{}\n{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY
            ),
            Self::JavaScript => tree_sitter_javascript::HIGHLIGHT_QUERY.to_string(),
            Self::Python => tree_sitter_python::HIGHLIGHTS_QUERY.to_string(),
        }
    }
}

/// Per-language cached state: parser, compiled query, and a capture-index →
/// kind lookup table.
struct GrammarContext {
    parser: Parser,
    query: Query,
    kinds: HashMap<u32, HighlightKind>,
}

impl GrammarContext {
    fn build(grammar: Grammar) -> Option<Self> {
        let language = grammar.language();
        let mut parser = Parser::new();
        // ABI 不兼容（版本组合被破坏）时降级为 None，不 panic
        parser.set_language(&language).ok()?;
        let query = Query::new(&language, &grammar.highlights_query()).ok()?;
        let mut kinds = HashMap::new();
        for (name, kind) in CAPTURE_KINDS {
            if let Some(index) = query.capture_index_for_name(name) {
                kinds.insert(index, *kind);
            }
        }
        Some(Self {
            parser,
            query,
            kinds,
        })
    }

    /// Parses one whole code block and flattens query captures into the
    /// sorted, non-overlapping span list (module doc 契约).
    fn highlight(&mut self, code: &str) -> Option<Vec<HighlightSpan>> {
        if code.is_empty() {
            return Some(Vec::new());
        }
        let tree = self.parser.parse(code, None)?;
        let root = tree.root_node();
        // 原始 capture 带 pattern 序号；先收集再统一着色，避免流式互踩
        let mut raw: Vec<(usize, usize, usize, HighlightKind)> = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.query, root, code.as_bytes());
        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                let Some(kind) = self.kinds.get(&capture.index) else {
                    continue; // 映射表之外的 capture 不产出 span
                };
                let range = capture.node.byte_range();
                raw.push((match_.pattern_index, range.start, range.end, *kind));
            }
        }
        Some(paint_runs(code, raw))
    }
}

/// Flattens raw captures with tree-sitter's highlight precedence: the later
/// pattern in the query wins (query files list generic fallbacks first and
/// specific overrides later), so paint cells latest-pattern-first and only
/// into cells no later pattern claimed. A nested more-specific capture (e.g.
/// `@escape` inside `@string`) therefore keeps its own span.
fn paint_runs(
    code: &str,
    mut raw: Vec<(usize, usize, usize, HighlightKind)>,
) -> Vec<HighlightSpan> {
    raw.sort_by_key(|&(pattern, start, end, _)| (Reverse(pattern), start, Reverse(end)));
    let mut cells: Vec<Option<HighlightKind>> = vec![None; code.len()];
    for (_, start, end, kind) in raw {
        // capture 边界理论上按 token 对齐；钳制到 char 边界防 panic
        let start = ceil_char_boundary(code, start);
        let end = floor_char_boundary(code, end);
        if start >= end {
            continue;
        }
        for cell in &mut cells[start..end] {
            if cell.is_none() {
                *cell = Some(kind);
            }
        }
    }
    // 相邻同类 cell 合并成 run
    let mut spans: Vec<HighlightSpan> = Vec::new();
    for (offset, cell) in cells.into_iter().enumerate() {
        let Some(kind) = cell else { continue };
        match spans.last_mut() {
            Some(last) if last.kind == kind && last.end_byte == offset => {
                last.end_byte = offset + 1;
            }
            _ => spans.push(HighlightSpan {
                start_byte: offset,
                end_byte: offset + 1,
                kind,
            }),
        }
    }
    spans
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    let len = text.len();
    index = index.min(len);
    while index < len && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

thread_local! {
    /// Per-thread lazy cache: one `Parser`/`Query` per grammar, built on
    /// first use. `None` marks a grammar whose context failed to build
    /// (ABI/query error) so per-block retries are not attempted.
    static CONTEXTS: RefCell<HashMap<Grammar, Option<GrammarContext>>> =
        RefCell::new(HashMap::new());
}

pub fn highlight(code: &str, language: &str) -> Option<Vec<HighlightSpan>> {
    let grammar = Grammar::for_language_tag(&language.to_ascii_lowercase())?;
    CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        let context = contexts
            .entry(grammar)
            .or_insert_with(|| GrammarContext::build(grammar))
            .as_mut()?;
        context.highlight(code)
    })
}

#[cfg(test)]
fn cached_grammar_count() -> usize {
    CONTEXTS.with(|contexts| contexts.borrow().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserts the module contract: sorted, non-overlapping, in-bounds,
    /// char-aligned spans.
    fn assert_span_invariants(code: &str, spans: &[HighlightSpan]) {
        let mut previous_end = 0;
        for span in spans {
            assert!(span.start_byte >= previous_end, "overlapping: {spans:?}");
            assert!(span.end_byte <= code.len(), "out of bounds: {spans:?}");
            assert!(code.is_char_boundary(span.start_byte));
            assert!(code.is_char_boundary(span.end_byte));
            assert!(span.start_byte < span.end_byte);
            previous_end = span.end_byte;
        }
    }

    /// Collects the kinds of spans whose sliced content equals `text`.
    fn kinds_for(code: &str, spans: &[HighlightSpan], text: &str) -> Vec<HighlightKind> {
        spans
            .iter()
            .filter(|span| &code[span.start_byte..span.end_byte] == text)
            .map(|span| span.kind)
            .collect()
    }

    #[test]
    fn rust_sample_highlights_keyword_comment_string_and_nested_escape() {
        let code = "fn main() {\n    // greet\n    let s = \"a\\nb\";\n}\n";
        let spans = highlight(code, "rust").expect("rust is supported");
        assert_span_invariants(code, &spans);
        // 关键字 / 注释 / 函数名
        assert_eq!(kinds_for(code, &spans, "fn"), vec![HighlightKind::Keyword]);
        assert_eq!(
            kinds_for(code, &spans, "main"),
            vec![HighlightKind::Function]
        );
        assert_eq!(kinds_for(code, &spans, "let"), vec![HighlightKind::Keyword]);
        assert_eq!(
            kinds_for(code, &spans, "// greet"),
            vec![HighlightKind::Comment]
        );
        // 嵌套结构：字符串字面量含转义 \n —— rust 查询中 @escape 模式（L154）
        // 晚于 @string（L147），later pattern wins，转义段独立成 Escape span
        // （paint_runs 契约：嵌套更特异的 capture 保留自己的 span）
        let literal = "\"a\\nb\"";
        assert_eq!(kinds_for(code, &spans, "\\n"), vec![HighlightKind::Escape]);
        // 转义段之外的字面量部分（含引号）仍归 String
        assert_eq!(kinds_for(code, &spans, "\"a"), vec![HighlightKind::String]);
        assert_eq!(kinds_for(code, &spans, "b\""), vec![HighlightKind::String]);
        // 整段字面量被转义 span 切开，故无任何 span 覆盖完整 literal
        assert!(
            kinds_for(code, &spans, literal).is_empty(),
            "escape must split the literal: {spans:?}"
        );
    }

    #[test]
    fn python_sample_highlights_keyword_comment_function_and_string() {
        let code = "def greet(name):\n    # say hi\n    return \"hi\"\n";
        let spans = highlight(code, "python").expect("python is supported");
        assert_span_invariants(code, &spans);
        assert_eq!(kinds_for(code, &spans, "def"), vec![HighlightKind::Keyword]);
        assert_eq!(
            kinds_for(code, &spans, "greet"),
            vec![HighlightKind::Function]
        );
        assert_eq!(
            kinds_for(code, &spans, "# say hi"),
            vec![HighlightKind::Comment]
        );
        assert_eq!(
            kinds_for(code, &spans, "\"hi\""),
            vec![HighlightKind::String]
        );
    }

    #[test]
    fn typescript_sample_highlights_keyword_type_and_string() {
        let code = "interface User {\n  id: number;\n}\nconst tag = \"v1\";\n";
        let spans = highlight(code, "typescript").expect("typescript is supported");
        assert_span_invariants(code, &spans);
        assert_eq!(
            kinds_for(code, &spans, "interface"),
            vec![HighlightKind::Keyword]
        );
        // 大写开头的标识符经 TS 查询 pattern 判为类型
        assert_eq!(kinds_for(code, &spans, "User"), vec![HighlightKind::Type]);
        assert_eq!(kinds_for(code, &spans, "number"), vec![HighlightKind::Type]);
        assert_eq!(
            kinds_for(code, &spans, "const"),
            vec![HighlightKind::Keyword]
        );
        assert_eq!(
            kinds_for(code, &spans, "\"v1\""),
            vec![HighlightKind::String]
        );
    }

    #[test]
    fn javascript_sample_highlights_keyword_comment_and_function() {
        let code = "function add(a, b) {\n  // sum\n  return a + b;\n}\n";
        let spans = highlight(code, "javascript").expect("javascript is supported");
        assert_span_invariants(code, &spans);
        assert_eq!(
            kinds_for(code, &spans, "function"),
            vec![HighlightKind::Keyword]
        );
        assert_eq!(
            kinds_for(code, &spans, "add"),
            vec![HighlightKind::Function]
        );
        assert_eq!(
            kinds_for(code, &spans, "// sum"),
            vec![HighlightKind::Comment]
        );
        assert_eq!(
            kinds_for(code, &spans, "return"),
            vec![HighlightKind::Keyword]
        );
    }

    #[test]
    fn unknown_and_empty_language_tags_return_none() {
        assert_eq!(highlight("let x = 1;", "cobol"), None);
        assert_eq!(highlight("let x = 1;", ""), None);
        assert_eq!(highlight("let x = 1;", "not-a-language"), None);
    }

    #[test]
    fn empty_code_with_supported_language_returns_empty_span_list() {
        assert_eq!(highlight("", "rust"), Some(Vec::new()));
        assert_eq!(highlight("", "python"), Some(Vec::new()));
        // 未知语言规则优先于空串规则
        assert_eq!(highlight("", "cobol"), None);
    }

    #[test]
    fn language_tags_match_case_insensitively_with_aliases() {
        let code = "let x = 1;";
        assert!(highlight(code, "Rust").is_some());
        assert!(highlight(code, "RS").is_some());
        assert!(highlight(code, "ts").is_some());
        assert!(highlight(code, "tsx").is_some());
        assert!(highlight(code, "py").is_some());
        // 架构师裁决：markdown grammar 砍掉，其 fence 标签一律降级纯文本
        assert_eq!(highlight(code, "markdown"), None);
        assert_eq!(highlight(code, "MD"), None);
    }

    #[test]
    fn parsers_are_cached_across_blocks_per_language() {
        let code = "fn a() {}\n";
        assert!(highlight(code, "rust").is_some());
        assert!(highlight(code, "rust").is_some()); // 同语言复用缓存 Parser
        assert!(highlight("def b():\n    pass\n", "python").is_some());
        assert_eq!(cached_grammar_count(), 2);
    }
}
