use super::*;

/// Materializes a user echo block (T18 消息块结构): the 「你」 label, one
/// line per source line (blank lines preserved as empty spans), and a
/// trailing spacer. The item model renders these as ONE natural-height card
/// ([`user_message_item`]); the flat form stays for row accounting and tests.
pub(crate) fn user_message_lines(block_id: u64, text: &str) -> Vec<StreamLine> {
    let mut lines = vec![StreamLine::new(block_id, LineKind::UserLabel)];
    let trimmed = text.trim_end_matches('\n');
    let raw: Vec<&str> = trimmed.split('\n').collect();
    let count = raw.len();
    for (index, part) in raw.iter().enumerate() {
        let mut line = StreamLine::new(
            block_id,
            LineKind::UserLine {
                first: index == 0,
                last: index + 1 == count,
            },
        );
        line.spans = coalesce(vec![StreamSpan {
            text: (*part).to_string(),
            style: SpanStyle::Plain,
        }]);
        lines.push(line);
    }
    lines.push(StreamLine::new(block_id, LineKind::Spacer));
    lines
}

/// Renders one visible semantic entry as a single variable-height list item
/// (S8-T44/C4: 一项=一个 user/assistant/tool/permission/plan/artifact/
/// summary item 的自然高度). Per-frame: clone-only element assembly from
/// cached materialization — no markdown re-materialization here (P3).
pub(crate) fn render_entry(
    entry: &StreamEntry,
    counters: &StreamCounters,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let row_t0 = Instant::now();
    let colors = theme(cx).colors;
    let item = match entry {
        StreamEntry::User { lines } => user_message_item(lines, &colors),
        StreamEntry::Assistant { model, .. } => markdown_item(model, &colors),
        StreamEntry::Tool { card } => {
            let card = card.clone();
            let row_count = card.read(cx).row_count();
            card_rows_item(row_count, move |row| {
                ToolCard::render_row(card.clone(), row, cx)
            })
        }
        StreamEntry::Artifact { card } => {
            let card = card.clone();
            let row_count = card.read(cx).row_count();
            card_rows_item(row_count, move |row| {
                ArtifactCard::render_row(card.clone(), row, window, cx)
            })
        }
        StreamEntry::Permission { card } => {
            let card = card.clone();
            let row_count = card.read(cx).row_count();
            card_rows_item(row_count, move |row| {
                PermissionCard::render_row(card.clone(), row, window, cx)
            })
        }
        StreamEntry::Plan { card } => {
            let card = card.clone();
            let row_count = card.read(cx).row_count();
            card_rows_item(row_count, move |row| {
                PlanCard::render_row(card.clone(), row, window, cx)
            })
        }
        StreamEntry::Summary { card } => {
            let card = card.clone();
            let row_count = card.read(cx).row_count();
            card_rows_item(row_count, move |row| {
                SummaryCard::render_row(card.clone(), row, cx)
            })
        }
    };
    if let Ok(mut samples) = counters.row_build_ns.lock() {
        samples.push(row_t0.elapsed().as_nanos());
    }
    item
}

/// One card entry as one natural-height item: the card's compact subrows
/// (24px, C4 rule 1) stacked vertically inside a single list item.
fn card_rows_item(row_count: usize, mut render_row: impl FnMut(usize) -> AnyElement) -> AnyElement {
    let mut rows = Vec::with_capacity(row_count);
    for row in 0..row_count {
        rows.push(render_row(row));
    }
    div()
        .w_full()
        .flex_shrink_0()
        .px(px(CONTENT_MIN_PADDING))
        .pt(px(4.0))
        .pb(px(8.0))
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}

/// One assistant markdown turn as one natural-height item: each materialized
/// block renders at its own natural height (text wraps, C4 禁截断).
pub(crate) fn markdown_item(model: &StreamModel, colors: &ThemeColors) -> AnyElement {
    div()
        .w_full()
        .flex_shrink_0()
        .px(px(CONTENT_MIN_PADDING))
        .pt(px(4.0))
        .pb(px(8.0))
        .flex()
        .flex_col()
        .children(
            model
                .committed_lines
                .iter()
                .chain(model.pending_lines.iter())
                .map(|line| render_line(line, colors)),
        )
        .into_any_element()
}

/// One user echo as one natural-height item: the 「你」 label plus a
/// bg_elevated card whose lines are individual wrapping text rows (风格裁决:
/// bg_elevated 圆角卡片 + 「你」标记、左对齐、1px border_subtle).
pub(crate) fn user_message_item(lines: &[StreamLine], colors: &ThemeColors) -> AnyElement {
    let mut body = div()
        .bg(colors.bg_elevated)
        .border_1()
        .border_color(colors.border_subtle)
        .rounded_lg()
        .px_2()
        .py_1()
        .text_color(colors.text_primary)
        .flex()
        .flex_col();
    for line in lines {
        if !matches!(line.kind, LineKind::UserLine { .. }) {
            continue;
        }
        let text = line
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        if text.is_empty() {
            // 空行保留一行正文行高的自然占位（非定高行模型）。
            body = body.child(div().h(user_line_height()));
        } else {
            body = body.child(block_text(&line.spans, user_body_style(colors), colors));
        }
    }
    div()
        .w_full()
        .flex_shrink_0()
        .px(px(CONTENT_MIN_PADDING))
        .pt(px(8.0))
        .pb(px(8.0))
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_secondary)
                .px_2()
                .child("你"),
        )
        .child(body)
        .into_any_element()
}

/// One blank user-message line's natural box: exactly one body line height.
fn user_line_height() -> Pixels {
    px(Typography::MESSAGE * Typography::MESSAGE_LINE_HEIGHT)
}

// ─── text runs (block-level styled text, S8-T44) ─────────────────────────────

/// Base run style for a message-body text block: 14px at 1.6 line height
/// (ui-spec §3), window default family, caller's color.
pub(crate) fn message_run_style(color: Rgba) -> TextStyle {
    TextStyle {
        font_size: AbsoluteLength::Pixels(px(Typography::MESSAGE)),
        line_height: DefiniteLength::Fraction(Typography::MESSAGE_LINE_HEIGHT),
        color: color.into(),
        ..TextStyle::default()
    }
}

/// Base run style for a monospace block (code lines, table rows): the 12.5px
/// code tier (ui-spec §3).
pub(crate) fn code_run_style(color: Rgba) -> TextStyle {
    TextStyle {
        font_family: MONOFONT.into(),
        font_size: AbsoluteLength::Pixels(px(Typography::CODE)),
        line_height: DefiniteLength::Fraction(Typography::BODY_LINE_HEIGHT),
        color: color.into(),
        ..TextStyle::default()
    }
}

/// User echo body style: card text at the sidebar tier.
fn user_body_style(colors: &ThemeColors) -> TextStyle {
    TextStyle {
        font_size: AbsoluteLength::Pixels(px(Typography::MESSAGE)),
        line_height: DefiniteLength::Fraction(Typography::MESSAGE_LINE_HEIGHT),
        color: colors.text_primary.into(),
        ..TextStyle::default()
    }
}

/// Turns one logical line's spans into a wrapping [`StyledText`] block.
/// Spans are byte-sliced into [`TextRun`]s per the materialization mapping;
/// long text wraps inside the item (C4 禁截断), so an item's natural height
/// covers every line it needs.
pub(crate) fn block_text(
    spans: &[StreamSpan],
    default_style: TextStyle,
    colors: &ThemeColors,
) -> StyledText {
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let styled = StyledText::new(text);
    let mut runs: Vec<TextRun> = Vec::with_capacity(spans.len());
    for span in spans {
        let mut style = default_style.clone();
        apply_span_style(&mut style, span.style, colors);
        runs.push(style.to_run(span.text.len()));
    }
    styled.with_runs(runs)
}

/// Applies one [`SpanStyle`] onto a [`TextStyle`] (run-level mapping; colors
/// stay on ui-spec §2 tokens, no new values).
pub(crate) fn apply_span_style(style: &mut TextStyle, span: SpanStyle, colors: &ThemeColors) {
    match span {
        SpanStyle::Plain => {}
        SpanStyle::Strong => style.font_weight = FontWeight::BOLD,
        SpanStyle::Emphasis => style.font_style = FontStyle::Italic,
        SpanStyle::Strikethrough => style.strikethrough = Some(StrikethroughStyle::default()),
        SpanStyle::Code => {
            style.font_family = MONOFONT.into();
            style.font_size = AbsoluteLength::Pixels(px(Typography::CODE));
            style.background_color = Some(colors.code_bg.into());
        }
        SpanStyle::Link => {
            style.underline = Some(UnderlineStyle::default());
            style.color = colors.text_secondary.into();
        }
        // 高亮 token：等宽字体/字号与代码行基style一致，只覆写映射表给出的
        // 颜色/字重/斜体。
        SpanStyle::Token(kind) => {
            let token = code_token_style(kind, colors);
            style.font_family = MONOFONT.into();
            style.font_size = AbsoluteLength::Pixels(px(Typography::CODE));
            style.color = token.color.into();
            style.font_weight = token.weight;
            if token.italic {
                style.font_style = FontStyle::Italic;
            }
        }
    }
}

/// Heading tier → (font size token, weight token) — 字号全部取自 ui-spec §3
/// 的 Typography 档位，不发明新字号.
pub(crate) fn heading_style(level: u8) -> (f32, FontWeight) {
    match level {
        1..=2 => (Typography::HEADING_PAGE, Typography::HEADING_PAGE_WEIGHT),
        3..=4 => (Typography::HEADING_BLOCK, Typography::HEADING_BLOCK_WEIGHT),
        _ => (Typography::MESSAGE, Typography::HEADING_CARD_WEIGHT),
    }
}

/// Renders one [`StreamLine`] into a natural-height block. 24px no longer
/// applies here: text blocks wrap; vertical rhythm comes from per-kind
/// padding. Fixed heights remain only inside cards' compact subrows.
pub(crate) fn render_line(line: &StreamLine, colors: &ThemeColors) -> AnyElement {
    let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
    let item = div()
        .w_full()
        .flex_shrink_0()
        .text_size(px(Typography::MESSAGE))
        .text_color(colors.text_primary);
    match line.kind {
        LineKind::Spacer => item.py(px(6.0)).into_any_element(),
        LineKind::UserLabel => item
            .pt(px(4.0))
            .pb(px(2.0))
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_secondary)
            .child(div().px_2().child("你"))
            .into_any_element(),
        LineKind::UserLine { .. } => item
            .bg(colors.bg_elevated)
            .border_1()
            .border_color(colors.border_subtle)
            .child(div().px_2().py(px(1.0)).child(block_text(
                &line.spans,
                user_body_style(colors),
                colors,
            )))
            .into_any_element(),
        LineKind::Code => {
            let code = div()
                .w_full()
                .bg(colors.code_bg)
                .px_2()
                .py(px(1.0))
                .text_color(colors.text_primary)
                .child(block_text(
                    &line.spans,
                    code_run_style(colors.text_primary),
                    colors,
                ));
            let code = if text.is_empty() {
                // 空代码行保一行等高占位（非定高行模型）。
                code.h(px(Typography::CODE * Typography::BODY_LINE_HEIGHT))
            } else {
                code
            };
            item.child(code).into_any_element()
        }
        LineKind::Quote => item
            .text_color(colors.text_secondary)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .child(
                        div()
                            .w(px(2.0))
                            .mr_2()
                            .flex_shrink_0()
                            .bg(colors.border_subtle),
                    )
                    .child(block_text(
                        &line.spans,
                        message_run_style(colors.text_secondary),
                        colors,
                    )),
            )
            .into_any_element(),
        LineKind::Rule => item
            .py(px(4.0))
            .child(div().w_full().h(px(1.0)).bg(colors.border_subtle))
            .into_any_element(),
        LineKind::Heading(level) => {
            let (size, weight) = heading_style(level);
            item.text_size(px(size))
                .font_weight(weight)
                .pt(px(8.0))
                .pb(px(2.0))
                .child(block_text(
                    &line.spans,
                    message_run_style(colors.text_primary),
                    colors,
                ))
                .into_any_element()
        }
        LineKind::TableHeader => item
            .bg(colors.bg_hover)
            .text_color(colors.text_primary)
            .child(div().px_2().child(block_text(
                &line.spans,
                code_run_style(colors.text_primary),
                colors,
            )))
            .into_any_element(),
        LineKind::TableRow => item
            .text_color(colors.text_primary)
            .child(div().px_2().child(block_text(
                &line.spans,
                code_run_style(colors.text_primary),
                colors,
            )))
            .into_any_element(),
        LineKind::ListItem => {
            let indent = "  ".repeat(line.depth);
            let mut row = div().flex().flex_row();
            row = row.child(
                div()
                    .flex_shrink_0()
                    .text_color(colors.text_secondary)
                    .child(format!("{indent}{} ", line.marker)),
            );
            if let Some(checked) = line.checked {
                row = row.child(
                    div()
                        .flex_shrink_0()
                        .mr_1()
                        .text_color(if checked {
                            colors.success
                        } else {
                            colors.text_tertiary
                        })
                        .child(if checked { "[x]" } else { "[ ]" }),
                );
            }
            row = row.child(block_text(
                &line.spans,
                message_run_style(colors.text_primary),
                colors,
            ));
            item.child(row).into_any_element()
        }
        LineKind::Paragraph => item
            .child(block_text(
                &line.spans,
                message_run_style(colors.text_primary),
                colors,
            ))
            .into_any_element(),
    }
}

// ─── sample document (演示注入载荷) ──────────────────────────────────────────

/// Builds the built-in demo document (`blocks` blocks: headings / inline
/// styles / tables / lists / code / quotes — 任务卡要求的样本集).
pub(crate) fn sample_document(blocks: usize) -> String {
    let cjk = [
        "这是一段中文文本，验证混排流式渲染。",
        "中文与 English 混排需要保持稳定。",
        "表格里的中文列宽按 2 列计。",
    ];
    let mut doc = String::with_capacity(blocks * 160);
    for index in 0..blocks {
        let zh = cjk[index % cjk.len()];
        match index % 8 {
            0 => doc.push_str(&format!("## 段落 {index}：流式 Markdown\n\n")),
            1 => doc.push_str(&format!(
                "段落 {index} 带 **加粗**、*斜体*、`行内代码`、\
                 [链接](https://example.com/{index}) 和 ~~删除线~~。{zh}\n\n"
            )),
            2 => doc.push_str(&format!(
                "| 列 A {index} | 列 B | 列 C |\n|:--|:-:|--:|\n| 1 | {zh} | 3 |\n| 4 | 5 | 6 |\n\n"
            )),
            3 => doc.push_str(&format!(
                "- 任务甲 {index}\n- [ ] 待办\n- [x] 已完成\n  - 嵌套项 `code`\n\n"
            )),
            4 => doc.push_str(&format!(
                "```rust\nfn demo_{index}() -> u64 {{\n    let v = {index} * 42;\n    v\n}}\n```\n\n"
            )),
            5 => doc.push_str(&format!("> 引用行一 {index}\n> 引用行二 {zh}\n\n")),
            6 => doc.push_str(&format!("1. 有序甲 {index}\n2. 有序乙\n\n")),
            _ => doc.push_str(&format!("普通收尾段落 {index}。{zh}\n\n")),
        }
    }
    doc
}

// (split_deltas moved to vega_markdown::replay — T18 公共回放器基建)
