use super::*;

/// Materializes a user echo block (T18 消息块结构): 「你」 label row, one card
/// line per source line (first/last flagged for rounding/border edges), and a
/// trailing spacer row separating it from the next message.
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

/// Renders one visible range across all entries (the `uniform_list` range
/// callback; per-frame: clone-only, P3).
pub(crate) fn build_entry_rows(
    entries: &[StreamEntry],
    range: Range<usize>,
    counters: &StreamCounters,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let row_t0 = Instant::now();
    let colors = theme(cx).colors;
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut offset = 0usize;
    for entry in entries {
        let count = entry.row_count(cx);
        let start = range.start.saturating_sub(offset);
        let end = range.end.saturating_sub(offset).min(count);
        if start < end {
            match entry {
                StreamEntry::User { lines } => {
                    rows.extend(
                        lines[start..end]
                            .iter()
                            .map(|line| render_row(line, &colors)),
                    );
                }
                StreamEntry::Assistant { model, .. } => {
                    rows.extend(model.rows_in(start..end, &colors));
                }
                StreamEntry::Tool { card } => {
                    rows.extend(
                        (start..end).map(|row| ToolCard::render_row(card.clone(), row, cx)),
                    );
                }
                StreamEntry::Artifact { card } => {
                    rows.extend(
                        (start..end)
                            .map(|row| ArtifactCard::render_row(card.clone(), row, window, cx)),
                    );
                }
                StreamEntry::Permission { card } => {
                    rows.extend(
                        (start..end)
                            .map(|row| PermissionCard::render_row(card.clone(), row, window, cx)),
                    );
                }
                StreamEntry::Plan { card } => {
                    rows.extend(
                        (start..end).map(|row| PlanCard::render_row(card.clone(), row, window, cx)),
                    );
                }
                StreamEntry::Summary { card } => {
                    rows.extend(
                        (start..end).map(|row| SummaryCard::render_row(card.clone(), row, cx)),
                    );
                }
            }
        }
        offset += count;
        if offset >= range.end {
            break;
        }
    }
    if let Ok(mut samples) = counters.row_build_ns.lock() {
        samples.push(row_t0.elapsed().as_nanos());
    }
    rows
}

// ─── per-frame row rendering ─────────────────────────────────────────────────

/// Builds the visible rows for one `uniform_list` range by cloning cached
/// [`StreamLine`]s (no materialization here — that is the P3 contract).
/// Single-model form used by the render_frame bench probe.
pub(crate) fn build_rows(
    model: &StreamModel,
    range: Range<usize>,
    counters: &StreamCounters,
    cx: &App,
) -> Vec<AnyElement> {
    let row_t0 = Instant::now();
    let colors = theme(cx).colors;
    let rows = model.rows_in(range, &colors);
    if let Ok(mut samples) = counters.row_build_ns.lock() {
        samples.push(row_t0.elapsed().as_nanos());
    }
    rows
}

/// Renders one [`StreamLine`] into a uniform-height row.
pub(crate) fn render_row(line: &StreamLine, colors: &ThemeColors) -> AnyElement {
    let mut row = div()
        .h(px(ROW_HEIGHT))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .overflow_hidden()
        .flex_shrink_0()
        // 内容列留白（ui-spec §1 左右留白 ≥24px；T18 消息块成型）。
        .px(px(CONTENT_MIN_PADDING))
        // 会话消息正文 14px（ui-spec §3；标题/代码分支按档位覆盖）。
        .text_size(px(Typography::MESSAGE));
    match line.kind {
        LineKind::Spacer => return row.into_any_element(),
        LineKind::UserLabel => {
            return row
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_secondary)
                // 与卡片文字左缘对齐（卡片内缩 px_2）。
                .child(div().px_2().child("你"))
                .into_any_element();
        }
        LineKind::UserLine { first, last } => {
            // user 消息卡片（风格裁决：bg_elevated 圆角卡片 + 「你」标记、
            // 左对齐、1px border_subtle；非右对齐气泡）。
            row = row
                .bg(colors.bg_elevated)
                .border_color(colors.border_subtle);
            row = row.border_l_1().border_r_1();
            if first {
                row = row.border_t_1().rounded_tl_lg().rounded_tr_lg();
            }
            if last {
                row = row.border_b_1().rounded_bl_lg().rounded_br_lg();
            }
        }
        LineKind::Code => {
            row = row
                .bg(colors.code_bg)
                .font_family(MONOFONT.to_string())
                .text_size(px(Typography::CODE));
        }
        LineKind::Quote => {
            row = row.text_color(colors.text_secondary).child(
                // 引用左竖条（token 色，无字形风险）。
                div()
                    .w(px(2.))
                    .h(px(16.))
                    .mr_2()
                    .flex_shrink_0()
                    .bg(colors.border_subtle),
            );
        }
        LineKind::Rule => {
            return row
                .child(div().flex_1().h(px(1.)).bg(colors.border_subtle))
                .into_any_element();
        }
        LineKind::Heading(level) => {
            let (size, weight) = heading_style(level);
            row = row.text_size(px(size)).font_weight(weight);
        }
        LineKind::TableHeader => {
            row = row.bg(colors.bg_hover).font_weight(FontWeight::MEDIUM);
        }
        LineKind::ListItem => {
            let indent = "  ".repeat(line.depth);
            row = row.child(
                div()
                    .flex_shrink_0()
                    .text_color(colors.text_secondary)
                    .child(format!("{}{} ", indent, line.marker)),
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
        }
        LineKind::Paragraph | LineKind::TableRow => {}
    }
    // 代码块/user 卡片的文字需要 bg 内再缩进：包进内层容器（gpui 的 px
    // 后写覆盖前写而非叠加，不能直接在行上再 px_2）。
    if matches!(line.kind, LineKind::Code | LineKind::UserLine { .. }) {
        let mut inner = div().w_full().flex().flex_row().items_center().px_2();
        for span in &line.spans {
            inner = inner.child(render_span(span, colors));
        }
        row = row.child(inner);
    } else {
        for span in &line.spans {
            row = row.child(render_span(span, colors));
        }
    }
    row.into_any_element()
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

pub(crate) fn render_span(span: &StreamSpan, colors: &ThemeColors) -> AnyElement {
    let mut text = div().child(span.text.clone());
    text = match span.style {
        SpanStyle::Plain => text,
        SpanStyle::Strong => text.font_weight(FontWeight::BOLD),
        SpanStyle::Emphasis => text.italic(),
        SpanStyle::Strikethrough => text.line_through(),
        SpanStyle::Code => text
            .font_family(MONOFONT.to_string())
            .text_size(px(Typography::CODE))
            .bg(colors.code_bg)
            .px_1()
            .rounded_sm(),
        SpanStyle::Link => text.underline().text_color(colors.text_secondary),
        // 高亮 token：等宽字体/字号由 Code 行容器继承，只覆写映射表给出的
        // 颜色/字重/斜体。
        SpanStyle::Token(kind) => {
            let token = code_token_style(kind, colors);
            let mut styled = text.text_color(token.color).font_weight(token.weight);
            if token.italic {
                styled = styled.italic();
            }
            styled
        }
    };
    text.into_any_element()
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

// ─── the conversation stream view ────────────────────────────────────────────
