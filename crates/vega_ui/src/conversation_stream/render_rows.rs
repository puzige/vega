use super::selection::SelectionDocumentBuilder;
use super::*;
use crate::icons::{Icon, icon};

/// Materializes a user echo block (T18 消息块结构): the 「你」 label, one
/// line per source line (blank lines preserved as empty spans), and a
/// trailing spacer. The item model renders these as ONE natural-height bubble
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

/// #117: the compaction row's icon and color. Every state shares one glyph
/// (Lucide `text-select`), so the row never changes shape as the operation
/// advances; only failure is recolored and the label carries the transition.
pub(crate) fn context_compaction_visual(
    status: vega_conversation::types::ContextCompactionStatus,
    colors: &ThemeColors,
) -> (Icon, Rgba) {
    use vega_conversation::types::ContextCompactionStatus as Status;
    let color = match status {
        Status::Failed => colors.danger,
        _ => colors.text_secondary,
    };
    (Icon::TextSelect, color)
}

pub(crate) fn context_compaction_label(
    status: vega_conversation::types::ContextCompactionStatus,
    failure: Option<vega_conversation::types::ContextCompactionFailureCode>,
    restored: bool,
) -> String {
    let base = context_control::status_label_for(status, failure);
    if restored {
        format!("已恢复 · {base}")
    } else {
        base.to_owned()
    }
}

/// Renders one visible semantic entry as a single variable-height list item
/// (S8-T44/C4: 一项=一个 user/assistant/tool/permission/plan/artifact/
/// summary item 的自然高度). Per-frame: clone-only element assembly from
/// cached materialization — no markdown re-materialization here (P3).
pub(crate) fn render_entry(entry: &StreamEntry, window: &mut Window, cx: &mut App) -> AnyElement {
    render_entry_internal(entry, window, cx, None, false)
}

pub(crate) fn render_entry_with_selection(
    entry: &StreamEntry,
    window: &mut Window,
    cx: &mut App,
    focus: &FocusHandle,
) -> AnyElement {
    render_entry_internal(entry, window, cx, Some(focus), false)
}

pub(crate) fn render_stream_entry(
    entry: &StreamEntry,
    index: usize,
    suppress_run_notices: bool,
    window: &mut Window,
    cx: &mut App,
    focus: &FocusHandle,
) -> AnyElement {
    let body = render_entry_internal(entry, window, cx, Some(focus), suppress_run_notices);
    if !matches!(entry, StreamEntry::Assistant { .. }) {
        return body;
    }
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .debug_selector(move || format!("assistant-body-{index}"))
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .child(body),
        )
        .into_any_element()
}

pub(crate) fn process_surface(_colors: &ThemeColors) -> gpui_kit::Div {
    div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .px(px(Layout::PROCESS_PADDING_X))
        .flex()
        .flex_col()
        .gap(px(Layout::PROCESS_ITEM_GAP))
}

fn render_entry_internal(
    entry: &StreamEntry,
    window: &mut Window,
    cx: &mut App,
    focus: Option<&FocusHandle>,
    suppress_run_notices: bool,
) -> AnyElement {
    let colors = theme(cx).colors;
    match entry {
        StreamEntry::RunActivity { group } => RunActivityGroup::render(group.clone(), window, cx),
        StreamEntry::RunActivitySegment { group, segment } => {
            RunActivityGroup::render_segment(group.clone(), *segment, window, cx)
        }
        StreamEntry::ContextCompaction {
            record, restored, ..
        } => {
            let (glyph, color) = context_compaction_visual(record.status, &colors);
            let restored = *restored;
            let label = context_compaction_label(record.status, record.failure, restored);
            div()
                .debug_selector(|| "context-compaction-row".into())
                .w_full()
                .flex_shrink_0()
                .pt_1()
                .pb_2()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(Typography::METADATA))
                .text_color(colors.text_secondary)
                .child(
                    div()
                        .debug_selector(|| "context-compaction-icon-slot".into())
                        .flex_shrink_0()
                        .child(icon(glyph, color)),
                )
                .child(
                    div()
                        .debug_selector(move || {
                            if restored {
                                "context-compaction-restored".into()
                            } else {
                                "context-compaction-live".into()
                            }
                        })
                        .min_w_0()
                        .flex_1()
                        .child(label),
                )
                .into_any_element()
        }
        StreamEntry::User { lines, copy } => {
            if let Some(focus) = focus {
                selectable_user_message(lines, copy, focus, &colors, window, cx)
            } else {
                message_with_copy(user_message_item(lines, &colors), copy, true, colors)
            }
        }
        StreamEntry::UserImages { images } => attachments::render_user_images(images),
        StreamEntry::Screenshot { card } => card.clone().into_any_element(),
        StreamEntry::Assistant {
            model,
            failure,
            copy,
            ..
        } => {
            if let Some(focus) = focus {
                selectable_markdown_message(
                    model,
                    failure.filter(|_| !suppress_run_notices),
                    copy,
                    focus,
                    &colors,
                    window,
                    cx,
                )
            } else {
                message_with_copy(
                    markdown_item(model, failure.filter(|_| !suppress_run_notices), &colors),
                    copy,
                    false,
                    colors,
                )
            }
        }
        StreamEntry::Tool { card } => {
            let card = card.clone();
            process_surface(&colors)
                .debug_selector(|| "standalone-tool-process".into())
                .child(ToolCard::render(
                    card,
                    "tool-activity-single-row".to_string(),
                    false,
                    window,
                    cx,
                ))
                .into_any_element()
        }
        StreamEntry::ToolGroup { group } => {
            let group = group.clone();
            process_surface(&colors)
                .debug_selector(|| "standalone-tool-group-process".into())
                .child(ToolActivityGroup::render(group, window, cx))
                .into_any_element()
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
            let rows = (0..row_count)
                .map(|row| PermissionCard::render_row(card.clone(), row, window, cx))
                .collect::<Vec<_>>();
            div()
                .w_full()
                .flex_shrink_0()
                .pt_1()
                .pb_2()
                .child(
                    div()
                        .debug_selector(|| "conversation-permission-card".into())
                        .w_full()
                        .rounded(px(Layout::STATUS_RADIUS))
                        .border_1()
                        .border_color(colors.border_subtle)
                        .p(px(Layout::STATUS_PADDING))
                        .flex()
                        .flex_col()
                        .gap(px(Layout::STATUS_GAP))
                        .children(rows),
                )
                .into_any_element()
        }
        StreamEntry::Plan { card } => {
            let card = card.clone();
            let row_count = card.read(cx).row_count();
            card_rows_item(row_count, move |row| {
                PlanCard::render_row(card.clone(), row, window, cx)
            })
        }
        StreamEntry::Summary { card }
            if suppress_run_notices
                && card.read(cx).summary().outcome
                    != vega_conversation::types::TaskSummaryOutcome::Completed =>
        {
            div().h_0().into_any_element()
        }
        StreamEntry::Summary { card } => match card.read(cx).summary().outcome {
            vega_conversation::types::TaskSummaryOutcome::Completed => div()
                .h_0()
                .debug_selector(|| "completed-task-summary-hidden".to_string())
                .into_any_element(),
            outcome => div()
                .debug_selector(|| "task-outcome-status".to_string())
                .py_1()
                .child(status_notice(
                    if outcome == vega_conversation::types::TaskSummaryOutcome::Failed {
                        "失败".to_string()
                    } else {
                        "已中断".to_string()
                    },
                    if outcome == vega_conversation::types::TaskSummaryOutcome::Failed {
                        colors.danger
                    } else {
                        colors.warning
                    },
                    &colors,
                ))
                .into_any_element(),
        },
        StreamEntry::SkillActivation { activation } => {
            use vega_conversation::history::{
                SkillHistoryOrigin, SkillHistorySource, SkillHistoryStatus,
                SkillHistoryVerification,
            };
            let source = match activation.source_scope {
                SkillHistorySource::Project => "项目",
                SkillHistorySource::VegaGlobal => "Vega 全局",
                SkillHistorySource::Imported => "外部导入",
            };
            let origin = match activation.origin {
                SkillHistoryOrigin::Model => "模型加载",
                SkillHistoryOrigin::ExplicitUser => "用户预载",
            };
            let status = match activation.status {
                SkillHistoryStatus::Loaded => "已加载",
                SkillHistoryStatus::Revoked => "已撤销",
            };
            let verification = match activation.verification {
                SkillHistoryVerification::Verified => "冻结快照已验证",
                SkillHistoryVerification::Unavailable => "冻结快照不可用或未验证",
            };
            let digest = activation.content_sha256.get(..12).unwrap_or("invalid");
            div()
                .debug_selector(|| "history-skill-provenance".to_string())
                .py_1()
                .text_size(px(Typography::METADATA))
                .text_color(colors.text_secondary)
                .child(format!(
                    "历史 Skill {} · {} · {} · {} · {} · SHA-256 {}…",
                    activation.name, source, origin, status, verification, digest
                ))
                .into_any_element()
        }
    }
}

/// One card entry as one natural-height item: the card's compact subrows
/// (24px, C4 rule 1) stacked vertically inside a single list item.
pub(crate) fn card_rows_item(
    row_count: usize,
    mut render_row: impl FnMut(usize) -> AnyElement,
) -> AnyElement {
    let mut rows = Vec::with_capacity(row_count);
    for row in 0..row_count {
        rows.push(render_row(row));
    }
    div()
        .w_full()
        .flex_shrink_0()
        .pt(px(4.0))
        .pb(px(8.0))
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}

/// One assistant markdown turn as one natural-height item: each materialized
/// block renders at its own natural height (text wraps, C4 禁截断).
pub(crate) fn markdown_item(
    model: &StreamModel,
    failure: Option<RunFailureKind>,
    colors: &ThemeColors,
) -> AnyElement {
    div()
        .debug_selector(|| "assistant-message".into())
        .w_full()
        .flex_shrink_0()
        .pt(px(Layout::CONVERSATION_BODY_TOP))
        .flex()
        .flex_col()
        .children(markdown_blocks(model, colors, |line| {
            render_line(line, colors)
        }))
        .children(failure.map(|reason| {
            div()
                .debug_selector(|| "assistant-run-failure".to_string())
                .child(status_notice(
                    reason.message().to_string(),
                    colors.danger,
                    colors,
                ))
        }))
        .into_any_element()
}

#[cfg(test)]
thread_local! {
    // Observe production text layouts without changing measurement or paint.
    pub(super) static USER_TEXT_LAYOUTS: std::cell::RefCell<Vec<gpui_kit::TextLayout>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub(super) fn status_notice(message: String, color: Rgba, colors: &ThemeColors) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_start()
        .gap(px(Layout::STATUS_GAP))
        .p(px(Layout::STATUS_PADDING))
        .rounded(px(Layout::STATUS_RADIUS))
        .border_1()
        .border_color(colors.border_subtle)
        .text_size(px(Typography::BODY))
        .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
        .text_color(color)
        .child(
            div()
                .debug_selector(|| "conversation-status-icon".into())
                .mt(px((Typography::BODY * Typography::BODY_LINE_HEIGHT
                    - Layout::ICON_SIZE)
                    / 2.0))
                .size(px(Layout::ICON_SIZE))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(Icon::Warning, color)),
        )
        .child(
            div()
                .debug_selector(|| "conversation-status-text".into())
                .min_w_0()
                .flex_1()
                .child(message),
        )
        .into_any_element()
}

pub(crate) fn user_message_item(lines: &[StreamLine], colors: &ThemeColors) -> AnyElement {
    #[cfg(test)]
    USER_TEXT_LAYOUTS.with_borrow_mut(Vec::clear);
    let mut body = div()
        .debug_selector(|| "user-message-bubble".into())
        .max_w(gpui_kit::relative(Layout::CONVERSATION_USER_RATIO))
        .bg(colors.conversation_bubble)
        .rounded(px(Layout::USER_MESSAGE_RADIUS))
        .text_size(px(Typography::CONVERSATION))
        .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
        .px(px(Layout::CONVERSATION_USER_PADDING))
        .py_2()
        .text_color(colors.conversation_user)
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
            let text = block_text(&line.spans, user_body_style(colors), colors);
            #[cfg(test)]
            USER_TEXT_LAYOUTS.with_borrow_mut(|layouts| layouts.push(text.layout().clone()));
            body = body.child(text);
        }
    }
    div()
        .w_full()
        .flex_shrink_0()
        .debug_selector(|| "user-message".into())
        .pt(px(Layout::USER_MESSAGE_PADDING_TOP))
        .pb(px(Layout::CONVERSATION_USER_BOTTOM))
        .flex()
        .flex_col()
        .items_end()
        .child(body)
        .into_any_element()
}

/// One blank user-message line's natural box: exactly one body line height.
fn user_line_height() -> Pixels {
    px(Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT)
}

fn selectable_user_message(
    lines: &[StreamLine],
    copy: &MessageCopy,
    focus: &FocusHandle,
    colors: &ThemeColors,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let selection_color = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
    let mut document = SelectionDocumentBuilder::new(copy.clone(), focus.clone(), selection_color);
    let mut body = div()
        .debug_selector(|| "user-message-bubble".into())
        .max_w(gpui_kit::relative(Layout::CONVERSATION_USER_RATIO))
        .bg(colors.conversation_bubble)
        .rounded(px(Layout::USER_MESSAGE_RADIUS))
        .text_size(px(Typography::CONVERSATION))
        .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
        .px(px(Layout::CONVERSATION_USER_PADDING))
        .py_2()
        .text_color(colors.conversation_user)
        .flex()
        .flex_col();
    let mut visible_line = false;
    for line in lines {
        if !matches!(line.kind, LineKind::UserLine { .. }) {
            continue;
        }
        if visible_line {
            document.append_literal("\n");
        }
        visible_line = true;
        let text = line
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        if text.is_empty() {
            body = body.child(div().h(user_line_height()));
        } else {
            let styled = block_text(&line.spans, user_body_style(colors), colors);
            let selected = document.append_styled(&text, styled, "user-body");
            body = body.child(selected);
        }
    }
    let body = div()
        .w_full()
        .flex_shrink_0()
        .debug_selector(|| "user-message".into())
        .pt(px(Layout::USER_MESSAGE_PADDING_TOP))
        .pb(px(Layout::CONVERSATION_USER_BOTTOM))
        .flex()
        .flex_col()
        .items_end()
        .child(body)
        .into_any_element();
    selection_context_menu(document.wrap(body, window, cx), copy, false)
}

fn selectable_markdown_message(
    model: &StreamModel,
    failure: Option<RunFailureKind>,
    copy: &MessageCopy,
    focus: &FocusHandle,
    colors: &ThemeColors,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    div()
        .debug_selector(|| "assistant-message".into())
        .w_full()
        .flex_shrink_0()
        .pt(px(Layout::CONVERSATION_BODY_TOP))
        .flex()
        .flex_col()
        .child(selectable_markdown_content(
            model, copy, focus, colors, false, window, cx,
        ))
        .children(failure.map(|reason| {
            div()
                .debug_selector(|| "assistant-run-failure".to_string())
                .child(status_notice(
                    reason.message().to_string(),
                    colors.danger,
                    colors,
                ))
        }))
        .into_any_element()
}

pub(crate) fn selectable_markdown_content(
    model: &StreamModel,
    copy: &MessageCopy,
    focus: &FocusHandle,
    colors: &ThemeColors,
    copy_original: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let selection_color = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
    let mut document = SelectionDocumentBuilder::new(copy.clone(), focus.clone(), selection_color);
    let body = div().w_full().min_w_0().flex_shrink_0().flex().flex_col();
    let mut previous: Option<(u64, LineKind)> = None;
    let blocks = markdown_blocks(model, colors, |line| {
        if !matches!(line.kind, LineKind::Rule)
            && let Some((previous_block, previous_kind)) = previous
        {
            let same_multiline_block = previous_block == line.block_id
                && ((matches!(previous_kind, LineKind::Code)
                    && matches!(line.kind, LineKind::Code))
                    || (matches!(previous_kind, LineKind::ListItem)
                        && matches!(line.kind, LineKind::ListItem)));
            document.append_literal(if same_multiline_block { "\n" } else { "\n\n" });
        }
        let element = render_line_selectable(line, colors, &mut document);
        if !matches!(line.kind, LineKind::Rule) {
            previous = Some((line.block_id, line.kind));
        }
        element
    });
    selection_context_menu(
        document.wrap(body.children(blocks).into_any_element(), window, cx),
        copy,
        copy_original,
    )
}

fn markdown_blocks(
    model: &StreamModel,
    colors: &ThemeColors,
    mut render: impl FnMut(&StreamLine) -> AnyElement,
) -> Vec<AnyElement> {
    let lines: Vec<_> = model
        .committed_lines
        .iter()
        .chain(&model.pending_lines)
        .collect();
    let list_blocks: std::collections::HashSet<_> = lines
        .iter()
        .filter(|line| line.kind == LineKind::ListItem)
        .map(|line| line.block_id)
        .collect();
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let previous = index.checked_sub(1).map(|index| lines[index]);
        let start = index;
        let block_id = line.block_id;
        let in_list = list_blocks.contains(&block_id);
        let previous_in_list = previous.is_some_and(|line| list_blocks.contains(&line.block_id));
        let gap = markdown_block_gap(previous, line, in_list, previous_in_list);
        let mut content = vec![render(line)];
        index += 1;
        if line.kind == LineKind::Code {
            while index < lines.len()
                && lines[index].block_id == block_id
                && lines[index].kind == LineKind::Code
            {
                content.push(render(lines[index]));
                index += 1;
            }
        }
        let body = div()
            .debug_selector(move || format!("markdown-block-{start}"))
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .mt(px(gap))
            .when(in_list && line.kind != LineKind::ListItem, |body| {
                body.pl(px((line.depth + 1) as f32 * Layout::MARKDOWN_LIST_INDENT))
            });
        let body = if line.kind == LineKind::Code {
            body.child(
                div()
                    .id(gpui_kit::SharedString::from(format!(
                        "markdown-code-{block_id}-{start}"
                    )))
                    .debug_selector(move || format!("markdown-code-{start}"))
                    .w_full()
                    .min_w_0()
                    .overflow_x_scroll()
                    .bg(colors.code_bg)
                    .rounded(px(Layout::TOOL_DETAIL_RADIUS))
                    .p(px(Layout::MARKDOWN_CODE_PADDING))
                    .flex()
                    .child(
                        div()
                            .min_w_full()
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .items_start()
                            .children(content),
                    ),
            )
        } else {
            body.children(content)
        };
        blocks.push(body.into_any_element());
    }
    blocks
}

pub(crate) fn markdown_block_gap(
    previous: Option<&StreamLine>,
    current: &StreamLine,
    in_list: bool,
    previous_in_list: bool,
) -> f32 {
    let Some(previous) = previous else {
        return 0.;
    };
    if in_list && previous.block_id == current.block_id {
        if current.kind == LineKind::ListItem {
            return Layout::MARKDOWN_LIST_ITEM_GAP;
        }
        return Layout::MARKDOWN_LIST_PARAGRAPH_GAP;
    }
    let after = if previous_in_list {
        Layout::MARKDOWN_BLOCK_GAP
    } else {
        match previous.kind {
            LineKind::Heading(_) => Layout::MARKDOWN_HEADING_AFTER,
            LineKind::Code | LineKind::ListItem | LineKind::Table | LineKind::Quote => {
                Layout::MARKDOWN_BLOCK_GAP
            }
            _ => Layout::CONVERSATION_PARAGRAPH_GAP,
        }
    };
    let before = match current.kind {
        LineKind::Heading(_) => Layout::MARKDOWN_HEADING_BEFORE,
        LineKind::Code | LineKind::ListItem | LineKind::Table | LineKind::Quote => {
            Layout::MARKDOWN_BLOCK_GAP
        }
        _ => 0.,
    };
    after.max(before)
}

fn selection_context_menu(body: AnyElement, copy: &MessageCopy, copy_original: bool) -> AnyElement {
    use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};

    let copy_for_menu = copy.clone();
    div()
        .id(("message-selection-context-menu", copy.id))
        .child(body)
        .context_menu(move |menu, _, _| {
            let selected = copy_for_menu.clone();
            let original = copy_for_menu.clone();
            menu.when(copy_original, |menu| {
                menu.item(
                    PopupMenuItem::new("复制原文").on_click(move |_, _, cx| original.copy(cx)),
                )
            })
            .item(
                PopupMenuItem::new("复制")
                    .disabled(!copy_for_menu.has_selected_text())
                    .on_click(move |_, _, cx| selected.copy_selected_text(cx)),
            )
        })
        .into_any_element()
}

fn render_line_selectable(
    line: &StreamLine,
    colors: &ThemeColors,
    document: &mut SelectionDocumentBuilder,
) -> AnyElement {
    if let Some(table) = &line.table {
        return render_table_selectable(line.block_id, table, colors, document);
    }
    let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
    let item = div()
        .when(line.kind != LineKind::Code, |item| item.w_full())
        .flex_shrink_0()
        .text_size(px(Typography::CONVERSATION))
        .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
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
        LineKind::UserLine { .. } => {
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(&line.spans, user_body_style(colors), colors),
                "assistant-user-line",
            );
            item.child(div().px_2().py(px(1.0)).child(selected))
                .into_any_element()
        }
        LineKind::Code => {
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(&line.spans, code_run_style(colors.text_primary), colors),
                "assistant-code",
            );
            let code = div()
                .text_size(px(Typography::CODE))
                .line_height(gpui_kit::relative(Typography::CODE_LINE_HEIGHT))
                .whitespace_nowrap()
                .text_color(colors.text_primary)
                .child(selected);
            let code = if text.is_empty() {
                code.h(px(Typography::CODE * Typography::CODE_LINE_HEIGHT))
            } else {
                code
            };
            item.child(code).into_any_element()
        }
        LineKind::Quote => {
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(&line.spans, message_run_style(colors.text_primary), colors),
                "assistant-quote",
            );
            item.child(
                div()
                    .flex()
                    .flex_row()
                    .child(
                        div()
                            .w(px(2.0))
                            .mr(px(Layout::MARKDOWN_CODE_PADDING))
                            .flex_shrink_0()
                            .bg(colors.border_subtle),
                    )
                    .child(selected),
            )
            .into_any_element()
        }
        LineKind::Rule => item
            .py(px(4.0))
            .child(div().w_full().h(px(1.0)).bg(colors.border_subtle))
            .into_any_element(),
        LineKind::Heading(level) => {
            let (size, weight) = heading_style(level);
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(&line.spans, heading_run_style(level, colors), colors),
                "assistant-heading",
            );
            item.text_size(px(size))
                .font_weight(weight)
                .child(selected)
                .into_any_element()
        }
        LineKind::Table => item.into_any_element(),
        LineKind::ListItem => {
            let marker = format!("{} ", line.marker);
            document.append_literal(&"  ".repeat(line.depth));
            let mut row = div()
                .pl(px(line.depth as f32 * Layout::MARKDOWN_LIST_INDENT))
                .flex()
                .flex_row();
            let marker_text = document.append_styled(
                &marker,
                StyledText::new(marker.clone()),
                "assistant-list-marker",
            );
            row = row.child(
                div()
                    .flex_shrink_0()
                    .w(px(Layout::MARKDOWN_LIST_INDENT))
                    .text_color(colors.text_secondary)
                    .child(marker_text),
            );
            if let Some(checked) = line.checked {
                let checkbox = if checked { "[x]" } else { "[ ]" };
                let checkbox_text = document.append_styled(
                    checkbox,
                    StyledText::new(checkbox),
                    "assistant-list-checkbox",
                );
                row = row.child(
                    div()
                        .flex_shrink_0()
                        .mr_1()
                        .text_color(if checked {
                            colors.success
                        } else {
                            colors.text_tertiary
                        })
                        .child(checkbox_text),
                );
                document.append_literal(" ");
            }
            let content = document.append_styled_runs(
                &text,
                block_text_runs(&line.spans, message_run_style(colors.text_primary), colors),
                "assistant-list-content",
            );
            row = row.child(div().flex_1().min_w_0().child(content));
            item.child(row).into_any_element()
        }
        LineKind::Paragraph => {
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(&line.spans, message_run_style(colors.text_primary), colors),
                "assistant-paragraph",
            );
            item.child(selected).into_any_element()
        }
    }
}

fn render_table_selectable(
    block_id: u64,
    table: &StreamTable,
    colors: &ThemeColors,
    document: &mut SelectionDocumentBuilder,
) -> AnyElement {
    let ordinal = table.ordinal;
    let columns = table.alignments.len();
    let weights = table.column_weights();
    let mut body = div()
        .text_size(px(Typography::CONVERSATION))
        .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
        .w_full()
        .min_w(px(Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH * columns as f32))
        .flex_shrink_0()
        .flex()
        .flex_col();
    for (row_index, cells) in table.rows.iter().enumerate() {
        if row_index > 0 {
            document.append_literal("\n");
        }
        let mut row = div()
            .w_full()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .border_b_1()
            .border_color(colors.border_subtle)
            .when(row_index == 0, |row| row.bg(colors.bg_hover));
        for (column, spans) in cells.iter().enumerate() {
            if column > 0 {
                document.append_literal("\t");
            }
            let mut style = message_run_style(colors.text_primary);
            if row_index == 0 {
                style.font_weight = Typography::HEADING_CARD_WEIGHT;
            }
            let alignment = match table.alignments[column] {
                TableAlignment::Center => gpui_kit::TextAlign::Center,
                TableAlignment::Right => gpui_kit::TextAlign::Right,
                _ => gpui_kit::TextAlign::Left,
            };
            let cell_text: String = spans.iter().map(|span| span.text.as_str()).collect();
            let selected = document.append_styled_runs(
                &cell_text,
                block_text_runs(spans, style, colors),
                &format!("table-{block_id}-{ordinal}-{row_index}-{column}"),
            );
            row = row.child(
                div()
                    .debug_selector(move || {
                        format!("markdown-table-{block_id}-{ordinal}-{row_index}-{column}")
                    })
                    .flex_basis(px(Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH))
                    .flex_grow(weights[column])
                    .flex_shrink_0()
                    .min_w_0()
                    .px(px(Layout::MARKDOWN_TABLE_PADDING_X))
                    .py(px(Layout::MARKDOWN_TABLE_PADDING_Y))
                    .text_align(alignment)
                    .child(selected),
            );
        }
        body = body.child(row);
    }
    div()
        .id(gpui_kit::SharedString::from(format!(
            "markdown-table-{block_id}-{ordinal}"
        )))
        .debug_selector(move || format!("markdown-table-{block_id}-{ordinal}"))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .overflow_x_scroll()
        .child(body)
        .into_any_element()
}

// ─── text runs (block-level styled text, S8-T44) ─────────────────────────────

/// Base run style for a message-body text block: 14px at 1.6 line height
/// (ui-spec §3), window default family, caller's color.
pub(crate) fn message_run_style(color: Rgba) -> TextStyle {
    TextStyle {
        font_size: AbsoluteLength::Pixels(px(Typography::CONVERSATION)),
        line_height: DefiniteLength::Fraction(Typography::CONVERSATION_LINE_HEIGHT),
        color: color.into(),
        ..TextStyle::default()
    }
}

/// Base run style for a monospace block (code lines): the 12.5px
/// code tier (ui-spec §3).
pub(crate) fn code_run_style(color: Rgba) -> TextStyle {
    TextStyle {
        font_family: MONOFONT.into(),
        font_size: AbsoluteLength::Pixels(px(Typography::CODE)),
        line_height: DefiniteLength::Fraction(Typography::CODE_LINE_HEIGHT),
        color: color.into(),
        ..TextStyle::default()
    }
}

/// User echo body style: card text at the sidebar tier.
fn user_body_style(colors: &ThemeColors) -> TextStyle {
    TextStyle {
        font_size: AbsoluteLength::Pixels(px(Typography::CONVERSATION)),
        line_height: DefiniteLength::Fraction(Typography::CONVERSATION_LINE_HEIGHT),
        color: colors.conversation_user.into(),
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
    StyledText::new(text).with_runs(block_text_runs(spans, default_style, colors))
}

fn markdown_text(spans: &[StreamSpan], style: TextStyle, colors: &ThemeColors) -> AnyElement {
    super::inline_code::styled_text(
        spans.iter().map(|span| span.text.as_str()).collect(),
        block_text_runs(spans, style, colors),
    )
}

fn block_text_runs(
    spans: &[StreamSpan],
    default_style: TextStyle,
    colors: &ThemeColors,
) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::with_capacity(spans.len());
    for span in spans {
        let mut style = default_style.clone();
        apply_span_style(&mut style, span.style, colors);
        runs.push(style.to_run(span.text.len()));
    }
    runs
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
            style.background_color = Some(colors.conversation_code.into());
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
        1 => (Typography::MARKDOWN_H1, Typography::HEADING_PAGE_WEIGHT),
        2 => (Typography::MARKDOWN_H2, Typography::HEADING_PAGE_WEIGHT),
        3 => (Typography::MARKDOWN_H3, Typography::HEADING_BLOCK_WEIGHT),
        _ => (Typography::CONVERSATION, Typography::HEADING_CARD_WEIGHT),
    }
}

fn heading_run_style(level: u8, colors: &ThemeColors) -> TextStyle {
    let (size, weight) = heading_style(level);
    TextStyle {
        font_size: AbsoluteLength::Pixels(px(size)),
        font_weight: weight,
        ..message_run_style(colors.text_primary)
    }
}

/// Renders one [`StreamLine`] into a natural-height block. 24px no longer
/// applies here: text blocks wrap; vertical rhythm comes from per-kind
/// padding. Fixed heights remain only inside cards' compact subrows.
pub(crate) fn render_line(line: &StreamLine, colors: &ThemeColors) -> AnyElement {
    if let Some(table) = &line.table {
        return render_table(line.block_id, table, colors);
    }
    let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
    let item = div()
        .when(line.kind != LineKind::Code, |item| item.w_full())
        .flex_shrink_0()
        .text_size(px(Typography::CONVERSATION))
        .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
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
            .child(div().px_2().py(px(1.0)).child(markdown_text(
                &line.spans,
                user_body_style(colors),
                colors,
            )))
            .into_any_element(),
        LineKind::Code => {
            let code = div()
                .text_size(px(Typography::CODE))
                .line_height(gpui_kit::relative(Typography::CODE_LINE_HEIGHT))
                .whitespace_nowrap()
                .text_color(colors.text_primary)
                .child(markdown_text(
                    &line.spans,
                    code_run_style(colors.text_primary),
                    colors,
                ));
            let code = if text.is_empty() {
                // 空代码行保一行等高占位（非定高行模型）。
                code.h(px(Typography::CODE * Typography::CODE_LINE_HEIGHT))
            } else {
                code
            };
            item.child(code).into_any_element()
        }
        LineKind::Quote => item
            .child(
                div()
                    .flex()
                    .flex_row()
                    .child(
                        div()
                            .w(px(2.0))
                            .mr(px(Layout::MARKDOWN_CODE_PADDING))
                            .flex_shrink_0()
                            .bg(colors.border_subtle),
                    )
                    .child(markdown_text(
                        &line.spans,
                        message_run_style(colors.text_primary),
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
                .child(markdown_text(
                    &line.spans,
                    heading_run_style(level, colors),
                    colors,
                ))
                .into_any_element()
        }
        LineKind::Table => item.into_any_element(),
        LineKind::ListItem => {
            let mut row = div()
                .pl(px(line.depth as f32 * Layout::MARKDOWN_LIST_INDENT))
                .flex()
                .flex_row();
            row = row.child(
                div()
                    .flex_shrink_0()
                    .w(px(Layout::MARKDOWN_LIST_INDENT))
                    .text_color(colors.text_secondary)
                    .child(format!("{} ", line.marker)),
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
            row = row.child(div().flex_1().min_w_0().child(markdown_text(
                &line.spans,
                message_run_style(colors.text_primary),
                colors,
            )));
            item.child(row).into_any_element()
        }
        LineKind::Paragraph => item
            .child(markdown_text(
                &line.spans,
                message_run_style(colors.text_primary),
                colors,
            ))
            .into_any_element(),
    }
}

fn render_table(block_id: u64, table: &StreamTable, colors: &ThemeColors) -> AnyElement {
    let ordinal = table.ordinal;
    let columns = table.alignments.len();
    let weights = table.column_weights();
    let mut body = div()
        .text_size(px(Typography::CONVERSATION))
        .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
        .w_full()
        .min_w(px(Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH * columns as f32))
        .flex_shrink_0()
        .flex()
        .flex_col();
    for (row_index, cells) in table.rows.iter().enumerate() {
        let mut row = div()
            .w_full()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .border_b_1()
            .border_color(colors.border_subtle)
            .when(row_index == 0, |row| row.bg(colors.bg_hover));
        for (column, spans) in cells.iter().enumerate() {
            let mut style = message_run_style(colors.text_primary);
            if row_index == 0 {
                style.font_weight = Typography::HEADING_CARD_WEIGHT;
            }
            let alignment = match table.alignments[column] {
                TableAlignment::Center => gpui_kit::TextAlign::Center,
                TableAlignment::Right => gpui_kit::TextAlign::Right,
                _ => gpui_kit::TextAlign::Left,
            };
            row = row.child(
                div()
                    .debug_selector(move || {
                        format!("markdown-table-{block_id}-{ordinal}-{row_index}-{column}")
                    })
                    .flex_basis(px(Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH))
                    .flex_grow(weights[column])
                    .flex_shrink_0()
                    .min_w_0()
                    .px(px(Layout::MARKDOWN_TABLE_PADDING_X))
                    .py(px(Layout::MARKDOWN_TABLE_PADDING_Y))
                    .text_align(alignment)
                    .child(markdown_text(spans, style, colors)),
            );
        }
        body = body.child(row);
    }
    div()
        .id(gpui_kit::SharedString::from(format!(
            "markdown-table-{block_id}-{ordinal}"
        )))
        .debug_selector(move || format!("markdown-table-{block_id}-{ordinal}"))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .overflow_x_scroll()
        .child(body)
        .into_any_element()
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

/// The group includes the message, the gap and the action row, keeping the
/// pointer path continuous. Opacity preserves the exact rest/hover geometry.
///
/// Temporarily disabled by [`MESSAGE_COPY_ACTIONS_ENABLED`] (Issue #78
/// follow-up): the reserved action row read as an ugly, layout-coupled
/// affordance, so while the flag is false the body is returned unchanged and
/// no row is reserved. The plumbing below is retained so the interaction can
/// be re-enabled by flipping that one flag; keeping the branch here (rather
/// than at the call site) also keeps `MessageCopy` and the `Copy` icon
/// reachable for the compiler.
fn message_with_copy(
    body: AnyElement,
    copy: &MessageCopy,
    user: bool,
    colors: ThemeColors,
) -> AnyElement {
    if !MESSAGE_COPY_ACTIONS_ENABLED {
        return body;
    }
    if !copy.has_text() {
        return body;
    }
    let id = copy.id;
    let group: gpui_kit::SharedString = format!("message-copy-group-{id}").into();
    let source = copy.clone();
    div()
        .id(("message-with-copy", id))
        .group(group.clone())
        .w_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .child(body)
        .child(
            div()
                .w_full()
                .flex()
                .when(user, |row| row.justify_end())
                .child(
                    crate::icons::icon_button(
                        crate::icons::Icon::Copy,
                        "复制消息",
                        colors,
                        move |_, _, cx| source.copy(cx),
                    )
                    .id(("message-copy", id))
                    .debug_selector(move || {
                        if user {
                            "message-copy-user"
                        } else {
                            "message-copy-assistant"
                        }
                        .into()
                    })
                    .opacity(0.)
                    .group_hover(group, |style| style.opacity(1.))
                    .focus_visible(|style| style.opacity(1.).bg(colors.bg_active)),
                ),
        )
        .into_any_element()
}
