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

pub(crate) fn contains_code(nodes: &[RenderNode]) -> bool {
    nodes.iter().any(|node| match node {
        RenderNode::CodeBlock { .. } => true,
        RenderNode::List(list) => list.items.iter().any(|item| contains_code(&item.children)),
        RenderNode::BlockQuote { children } => contains_code(children),
        _ => false,
    })
}

pub(crate) fn restore_code(nodes: &mut [RenderNode], raw: &[RenderNode]) {
    fn collect<'a>(nodes: &'a [RenderNode], codes: &mut Vec<&'a str>) {
        for node in nodes {
            match node {
                RenderNode::CodeBlock { code, .. } => codes.push(code),
                RenderNode::List(list) => {
                    for item in &list.items {
                        collect(&item.children, codes);
                    }
                }
                RenderNode::BlockQuote { children } => collect(children, codes),
                _ => {}
            }
        }
    }
    fn restore<'a>(nodes: &mut [RenderNode], codes: &mut impl Iterator<Item = &'a str>) {
        for node in nodes {
            match node {
                RenderNode::CodeBlock { code, .. } => {
                    if let Some(raw) = codes.next() {
                        *code = raw.to_owned();
                    }
                }
                RenderNode::List(list) => {
                    for item in &mut list.items {
                        restore(&mut item.children, codes);
                    }
                }
                RenderNode::BlockQuote { children } => restore(children, codes),
                _ => {}
            }
        }
    }
    let mut codes = Vec::new();
    collect(raw, &mut codes);
    restore(nodes, &mut codes.into_iter());
}

fn parse_blocks(events: &[Event<'_>], cursor: &mut usize) -> Vec<RenderNode> {
    let mut nodes = Vec::new();
    let mut stray: Vec<Inline> = Vec::new();
    while *cursor < events.len() {
        // Tight list items have no Paragraph tag. Consume their inline events
        // before block dispatch so formatting neither drops content nor flushes
        // the surrounding implicit paragraph.
        if parse_inline_event(events, cursor, &mut stray) {
            continue;
        }

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
        // Consume this container's closing event, but leave nested containers
        // to the shared inline consumer.
        if matches!(&events[*cursor], Event::End(_)) {
            *cursor += 1;
            break;
        }
        if !parse_inline_event(events, cursor, &mut spans) {
            *cursor += 1;
        }
    }
    spans
}

/// Consumes one inline event (including its children), leaving block events
/// and container ends untouched for the caller.
fn parse_inline_event(events: &[Event<'_>], cursor: &mut usize, spans: &mut Vec<Inline>) -> bool {
    match &events[*cursor] {
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
            push_text(spans, text.to_string());
            *cursor += 1;
        }
        Event::Code(code) => {
            spans.push(Inline::Code(code.to_string()));
            *cursor += 1;
        }
        Event::InlineHtml(html) => {
            push_text(spans, html.to_string());
            *cursor += 1;
        }
        Event::SoftBreak => {
            push_text(spans, " ".to_string());
            *cursor += 1;
        }
        Event::HardBreak => {
            push_text(spans, "\n".to_string());
            *cursor += 1;
        }
        _ => return false,
    }
    true
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
