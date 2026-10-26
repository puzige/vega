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
