//! Pure-data render instruction tree ([`RenderNode`]) and the
//! pulldown-cmark event → node conversion.
//!
//! Nodes carry no layout, color, or font information: the UI layers (S3-T17)
//! map them onto GPUI elements. The conversion is infallible and total —
//! unmodeled constructs degrade to text spans instead of dropping content.

use pulldown_cmark::{Alignment as PulldownAlignment, CodeBlockKind, Event, HeadingLevel, Tag};

/// An inline span inside a paragraph, heading, or table cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    /// A literal text run.
    Text(String),
    /// Inline code span (backtick-delimited).
    Code(String),
    /// `*emphasis*` (italic).
    Emphasis(Vec<Inline>),
    /// `**strong**` (bold).
    Strong(Vec<Inline>),
    /// `~~strikethrough~~`.
    Strikethrough(Vec<Inline>),
    /// A hyperlink; the label spans are the children. Autolinks (`<url>`)
    /// arrive here too, with the URL as the only span.
    Link {
        url: String,
        title: Option<String>,
        spans: Vec<Inline>,
    },
}

/// One top-level render instruction block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderNode {
    /// A paragraph of inline spans.
    Paragraph { spans: Vec<Inline> },
    /// An ATX heading; `level` is 1..=6 (`#` .. `######`).
    Heading { level: u8, spans: Vec<Inline> },
    /// A fenced or indented code block. `language` is the first word of the
    /// fence info string (`None` for indented blocks or bare fences).
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    /// An ordered or unordered list (nested lists appear inside items).
    List(ListBlock),
    /// A block quote containing nested blocks.
    BlockQuote { children: Vec<RenderNode> },
    /// A GFM table with an optional header row and body rows.
    Table(TableBlock),
    /// A thematic break (`---`).
    ThematicBreak,
}

/// A list block: `ordered` selects marker style, `start` is the 1-based first
/// item number (only meaningful when `ordered`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListBlock {
    pub ordered: bool,
    pub start: u64,
    pub items: Vec<ListItem>,
}

/// One list item. `checked` carries a GFM tasklist marker (`- [x]` / `- [ ]`);
/// `None` for regular items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub checked: Option<bool>,
    pub children: Vec<RenderNode>,
}

/// Column alignment per GFM delimiter row (`---` / `:--` / `:-:` / `--:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAlignment {
    None,
    Left,
    Center,
    Right,
}

/// A GFM table. `header` may be empty only when the table is malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableBlock {
    pub alignments: Vec<TableAlignment>,
    pub header: Vec<TableCell>,
    /// Body rows, each with one cell per column.
    pub rows: Vec<Vec<TableCell>>,
}

/// One table cell holding inline spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    pub spans: Vec<Inline>,
}

/// Converts a pulldown-cmark event slice into render nodes.
///
/// The input is one committed/pending markdown block (or a whole document in
/// mdstream's degraded single-block mode), so multiple top-level nodes are
/// possible. Bare inline events (tight list items emit no `Paragraph`) are
/// wrapped into an implicit paragraph.
pub(crate) fn render_nodes_from_events(events: &[Event<'_>]) -> Vec<RenderNode> {
    let mut cursor = 0;
    parse_blocks(events, &mut cursor)
}

fn parse_blocks(events: &[Event<'_>], cursor: &mut usize) -> Vec<RenderNode> {
    let mut nodes = Vec::new();
    let mut stray: Vec<Inline> = Vec::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::Start(tag) => {
                flush_stray(&mut nodes, &mut stray);
                *cursor += 1;
                match tag {
                    Tag::Paragraph => {
                        let spans = parse_inline(events, cursor);
                        nodes.push(RenderNode::Paragraph { spans });
                    }
                    Tag::Heading { level, .. } => {
                        let level = heading_level(*level);
                        let spans = parse_inline(events, cursor);
                        nodes.push(RenderNode::Heading { level, spans });
                    }
                    Tag::CodeBlock(kind) => {
                        let node = parse_code_block(events, cursor, kind);
                        nodes.push(node);
                    }
                    Tag::BlockQuote(_) => {
                        let children = parse_blocks(events, cursor);
                        nodes.push(RenderNode::BlockQuote { children });
                    }
                    Tag::List(start) => {
                        let node = parse_list(events, cursor, *start);
                        nodes.push(node);
                    }
                    Tag::Table(alignments) => {
                        let node = parse_table(events, cursor, alignments);
                        nodes.push(node);
                    }
                    // HTML 块未建模（任务卡节点集之外）：降级为原文段落，保内容不丢
                    Tag::HtmlBlock => {
                        let html = collect_html_block(events, cursor);
                        nodes.push(RenderNode::Paragraph {
                            spans: vec![Inline::Text(html)],
                        });
                    }
                    // 未启用的容器（脚注定义、元数据块等）：平衡跳过
                    _ => skip_balanced(events, cursor),
                }
            }
            // 封闭容器的 End：消费掉并归还给上层
            Event::End(_) => {
                *cursor += 1;
                flush_stray(&mut nodes, &mut stray);
                return nodes;
            }
            Event::Rule => {
                flush_stray(&mut nodes, &mut stray);
                *cursor += 1;
                nodes.push(RenderNode::ThematicBreak);
            }
            Event::Text(text) => {
                push_text(&mut stray, text.to_string());
                *cursor += 1;
            }
            Event::Code(code) => {
                stray.push(Inline::Code(code.to_string()));
                *cursor += 1;
            }
            Event::InlineHtml(html) => {
                push_text(&mut stray, html.to_string());
                *cursor += 1;
            }
            Event::SoftBreak => {
                push_text(&mut stray, " ".to_string());
                *cursor += 1;
            }
            Event::HardBreak => {
                push_text(&mut stray, "\n".to_string());
                *cursor += 1;
            }
            // TaskListMarker 已在条目层消费；其余叶子事件（未启用的数学/脚注引用）跳过
            _ => *cursor += 1,
        }
    }
    flush_stray(&mut nodes, &mut stray);
    nodes
}

fn parse_inline(events: &[Event<'_>], cursor: &mut usize) -> Vec<Inline> {
    let mut spans = Vec::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            // 当前行内容器的 End：消费掉并归还给上层
            Event::End(_) => {
                *cursor += 1;
                return spans;
            }
            Event::Start(Tag::Strong) => {
                *cursor += 1;
                let spans_inner = parse_inline(events, cursor);
                spans.push(Inline::Strong(spans_inner));
            }
            Event::Start(Tag::Emphasis) => {
                *cursor += 1;
                let spans_inner = parse_inline(events, cursor);
                spans.push(Inline::Emphasis(spans_inner));
            }
            Event::Start(Tag::Strikethrough) => {
                *cursor += 1;
                let spans_inner = parse_inline(events, cursor);
                spans.push(Inline::Strikethrough(spans_inner));
            }
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                let url = dest_url.to_string();
                let title = (!title.is_empty()).then(|| title.to_string());
                *cursor += 1;
                let spans_inner = parse_inline(events, cursor);
                spans.push(Inline::Link {
                    url,
                    title,
                    spans: spans_inner,
                });
            }
            // 图片未建模（任务卡行内集之外）：保留 alt 文本，URL 降级丢弃
            Event::Start(Tag::Image { .. }) => {
                *cursor += 1;
                let spans_inner = parse_inline(events, cursor);
                spans.extend(spans_inner);
            }
            Event::Text(text) => {
                push_text(&mut spans, text.to_string());
                *cursor += 1;
            }
            Event::Code(code) => {
                spans.push(Inline::Code(code.to_string()));
                *cursor += 1;
            }
            Event::InlineHtml(html) => {
                push_text(&mut spans, html.to_string());
                *cursor += 1;
            }
            Event::SoftBreak => {
                push_text(&mut spans, " ".to_string());
                *cursor += 1;
            }
            Event::HardBreak => {
                push_text(&mut spans, "\n".to_string());
                *cursor += 1;
            }
            _ => *cursor += 1,
        }
    }
    spans
}

fn parse_code_block(events: &[Event<'_>], cursor: &mut usize, kind: &CodeBlockKind) -> RenderNode {
    let language = match kind {
        CodeBlockKind::Fenced(info) => info.split_whitespace().next().map(str::to_string),
        CodeBlockKind::Indented => None,
    };
    let mut code = String::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::Text(text) => {
                code.push_str(text);
                *cursor += 1;
            }
            Event::End(_) => {
                *cursor += 1;
                break;
            }
            _ => *cursor += 1,
        }
    }
    RenderNode::CodeBlock { language, code }
}

fn parse_list(events: &[Event<'_>], cursor: &mut usize, start: Option<u64>) -> RenderNode {
    let ordered = start.is_some();
    let mut items = Vec::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::Start(Tag::Item) => {
                *cursor += 1;
                // GFM tasklist 标记位于条目内容之首（tight 列表无 Paragraph 包裹）
                let mut checked = None;
                if let Some(Event::TaskListMarker(is_checked)) = events.get(*cursor) {
                    checked = Some(*is_checked);
                    *cursor += 1;
                }
                let children = parse_blocks(events, cursor);
                items.push(ListItem { checked, children });
            }
            // End(List)：消费掉并退出条目循环
            Event::End(_) => {
                *cursor += 1;
                break;
            }
            _ => *cursor += 1,
        }
    }
    RenderNode::List(ListBlock {
        ordered,
        start: start.unwrap_or(1),
        items,
    })
}

fn parse_table(
    events: &[Event<'_>],
    cursor: &mut usize,
    alignments: &[PulldownAlignment],
) -> RenderNode {
    let alignments = alignments
        .iter()
        .map(|alignment| match alignment {
            PulldownAlignment::None => TableAlignment::None,
            PulldownAlignment::Left => TableAlignment::Left,
            PulldownAlignment::Center => TableAlignment::Center,
            PulldownAlignment::Right => TableAlignment::Right,
        })
        .collect();
    let mut header = Vec::new();
    let mut rows = Vec::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::Start(Tag::TableHead) => {
                *cursor += 1;
                header = parse_table_cells(events, cursor);
            }
            Event::Start(Tag::TableRow) => {
                *cursor += 1;
                rows.push(parse_table_cells(events, cursor));
            }
            // End(Table)：消费掉并退出
            Event::End(_) => {
                *cursor += 1;
                break;
            }
            _ => *cursor += 1,
        }
    }
    RenderNode::Table(TableBlock {
        alignments,
        header,
        rows,
    })
}

fn parse_table_cells(events: &[Event<'_>], cursor: &mut usize) -> Vec<TableCell> {
    let mut cells = Vec::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::Start(Tag::TableCell) => {
                *cursor += 1;
                let spans = parse_inline(events, cursor);
                cells.push(TableCell { spans });
            }
            // End(TableHead / TableRow)：消费掉并退出
            Event::End(_) => {
                *cursor += 1;
                break;
            }
            _ => *cursor += 1,
        }
    }
    cells
}

fn collect_html_block(events: &[Event<'_>], cursor: &mut usize) -> String {
    let mut html = String::new();
    while *cursor < events.len() {
        match &events[*cursor] {
            Event::Html(chunk) | Event::InlineHtml(chunk) => {
                html.push_str(chunk);
                *cursor += 1;
            }
            Event::End(_) => {
                *cursor += 1;
                break;
            }
            _ => *cursor += 1,
        }
    }
    html
}

/// Skips a container whose opening `Start` was already consumed: walks until
/// the matching `End` (depth-balanced) and consumes it.
fn skip_balanced(events: &[Event<'_>], cursor: &mut usize) {
    let mut depth = 1;
    while *cursor < events.len() {
        match events[*cursor] {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    *cursor += 1;
                    return;
                }
            }
            _ => {}
        }
        *cursor += 1;
    }
}

fn flush_stray(nodes: &mut Vec<RenderNode>, stray: &mut Vec<Inline>) {
    if !stray.is_empty() {
        nodes.push(RenderNode::Paragraph {
            spans: std::mem::take(stray),
        });
    }
}

/// Pushes a text run, coalescing with the previous span when it is also text
/// (pulldown may split one logical run into several adjacent events).
fn push_text(spans: &mut Vec<Inline>, text: String) {
    if let Some(Inline::Text(previous)) = spans.last_mut() {
        previous.push_str(&text);
    } else {
        spans.push(Inline::Text(text));
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use pulldown_cmark::{Options, Parser};

    use super::*;

    const GFM: Options = Options::ENABLE_TABLES
        .union(Options::ENABLE_STRIKETHROUGH)
        .union(Options::ENABLE_TASKLISTS);

    fn parse(md: &str) -> Vec<RenderNode> {
        let events: Vec<Event<'_>> = Parser::new_ext(md, GFM).collect();
        render_nodes_from_events(&events)
    }

    fn text(s: &str) -> Inline {
        Inline::Text(s.to_string())
    }

    #[test]
    fn heading_levels_map_to_one_through_six() {
        let nodes = parse("# h1\n\n## h2\n\n###### h6\n");
        assert_eq!(
            nodes,
            vec![
                RenderNode::Heading {
                    level: 1,
                    spans: vec![text("h1")]
                },
                RenderNode::Heading {
                    level: 2,
                    spans: vec![text("h2")]
                },
                RenderNode::Heading {
                    level: 6,
                    spans: vec![text("h6")]
                },
            ]
        );
    }

    #[test]
    fn table_alignments_map_from_delimiter_row() {
        let nodes = parse("| a | b | c | d |\n|:--|:-:|--:|---|\n| 1 | 2 | 3 | 4 |\n");
        let Some(RenderNode::Table(table)) = nodes.into_iter().next() else {
            panic!("expected a table node");
        };
        assert_eq!(
            table.alignments,
            vec![
                TableAlignment::Left,
                TableAlignment::Center,
                TableAlignment::Right,
                TableAlignment::None,
            ]
        );
        assert_eq!(table.header.len(), 4);
        assert_eq!(
            table.rows,
            vec![vec![
                TableCell {
                    spans: vec![text("1")]
                },
                TableCell {
                    spans: vec![text("2")]
                },
                TableCell {
                    spans: vec![text("3")]
                },
                TableCell {
                    spans: vec![text("4")]
                },
            ]]
        );
    }

    #[test]
    fn code_block_language_takes_first_info_word() {
        let nodes = parse("```rust ignore\nlet x = 1;\n```\n\n```\nbare\n```\n\n    indented\n");
        assert_eq!(
            nodes,
            vec![
                RenderNode::CodeBlock {
                    language: Some("rust".to_string()),
                    code: "let x = 1;\n".to_string(),
                },
                RenderNode::CodeBlock {
                    language: None,
                    code: "bare\n".to_string(),
                },
                RenderNode::CodeBlock {
                    language: None,
                    code: "indented\n".to_string(),
                },
            ]
        );
    }

    #[test]
    fn tasklist_markers_land_on_list_items() {
        let nodes = parse("- [x] done\n- [ ] todo\n- plain\n");
        let Some(RenderNode::List(list)) = nodes.into_iter().next() else {
            panic!("expected a list node");
        };
        assert!(!list.ordered);
        assert_eq!(list.start, 1);
        assert_eq!(list.items.len(), 3);
        assert_eq!(list.items[0].checked, Some(true));
        assert_eq!(list.items[1].checked, Some(false));
        assert_eq!(list.items[2].checked, None);
        for item in &list.items {
            // tight 列表：裸文本被包成隐式段落
            assert_eq!(item.children.len(), 1);
            assert!(matches!(&item.children[0], RenderNode::Paragraph { .. }));
        }
    }

    #[test]
    fn html_block_degrades_to_raw_text_paragraph() {
        let nodes = parse("<div class=\"x\">\nraw\n</div>\n");
        assert_eq!(
            nodes,
            vec![RenderNode::Paragraph {
                spans: vec![text("<div class=\"x\">\nraw\n</div>\n")]
            }]
        );
    }

    #[test]
    fn breaks_map_to_space_and_newline_text() {
        let nodes = parse("line one\nline two  \nline three\n");
        assert_eq!(
            nodes,
            vec![RenderNode::Paragraph {
                spans: vec![text("line one line two\nline three")]
            }]
        );
    }

    #[test]
    fn adjacent_text_events_coalesce_into_one_span() {
        // "**bo" 在最终语义下是字面文本，且 pulldown 会拆成多个 Text 事件
        let nodes = parse("**bo");
        assert_eq!(
            nodes,
            vec![RenderNode::Paragraph {
                spans: vec![text("**bo")]
            }]
        );
    }

    #[test]
    fn thematic_break_and_blockquote_nest_blocks() {
        let nodes = parse("> quoted **bold**\n> second\n\n---\n");
        assert_eq!(
            nodes,
            vec![
                RenderNode::BlockQuote {
                    children: vec![RenderNode::Paragraph {
                        spans: vec![
                            text("quoted "),
                            Inline::Strong(vec![text("bold")]),
                            text(" second"),
                        ]
                    }]
                },
                RenderNode::ThematicBreak,
            ]
        );
    }
}
