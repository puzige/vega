use super::code_drag::CodeDragSelection;
use super::code_viewport::CodeViewport;
use super::copy_receipt::{CopyReceiptButton, CopySource};
use super::links::link_ranges;
use super::selection::SelectionDocumentBuilder;
use super::*;
use crate::icons::{Icon, icon};
use crate::scroll_edge_fade::{ScrollFade, ScrollableFadeExt};

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
        line.spans = autolinks::text_spans(part);
        lines.push(line);
    }
    lines.push(StreamLine::new(block_id, LineKind::Spacer));
    lines
}

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
    let _ = restored;
    context_control::status_label_for(status, failure).to_owned()
}

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
    suppress_run_notices: bool,
    process_top: f32,
    window: &mut Window,
    cx: &mut App,
    focus: &FocusHandle,
) -> AnyElement {
    let body = render_entry_internal(entry, window, cx, Some(focus), suppress_run_notices);
    if matches!(
        entry,
        StreamEntry::Tool { .. }
            | StreamEntry::ToolGroup { .. }
            | StreamEntry::ContextCompaction { .. }
    ) && process_top > 0.
    {
        return div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .pt(px(process_top))
            .child(body)
            .into_any_element();
    }
    if !matches!(entry, StreamEntry::Assistant { .. }) {
        return body;
    }
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().w_full().min_w_0().flex_shrink_0().child(body))
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
        StreamEntry::Subagents { group } => group.clone().into_any_element(),
        StreamEntry::RunActivity { group } => RunActivityGroup::render(group.clone(), window, cx),
        StreamEntry::RunActivitySegment { group, segment } => {
            RunActivityGroup::render_segment(group.clone(), *segment, window, cx)
        }
        StreamEntry::ContextCompaction {
            record, restored, ..
        } => {
            if matches!(
                record.status,
                vega_conversation::types::ContextCompactionStatus::Succeeded
                    | vega_conversation::types::ContextCompactionStatus::Cancelled
            ) {
                return div().h_0().into_any_element();
            }
            if record.status == vega_conversation::types::ContextCompactionStatus::Failed {
                return div()
                    .w_full()
                    .min_w_0()
                    .child(plain_error_notice(
                        context_compaction_label(record.status, record.failure, *restored),
                        colors.danger,
                    ))
                    .into_any_element();
            }
            let (glyph, color) = context_compaction_visual(record.status, &colors);
            let restored = *restored;
            let label = context_compaction_label(record.status, record.failure, restored);
            div()
                .w_full()
                .flex_shrink_0()
                .min_w_0()
                .min_h(px(Layout::DISCLOSURE_HEADER_HEIGHT))
                .flex()
                .items_start()
                .gap(px(Layout::CONVERSATION_INLINE_GAP))
                .text_size(px(Typography::BODY))
                .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
                .text_color(color)
                .child(
                    div()
                        .w(px(Layout::ICON_SIZE))
                        .h(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .child(icon(glyph, color)),
                )
                .child(div().min_w_0().flex_1().whitespace_normal().child(label))
                .into_any_element()
        }
        StreamEntry::User { lines, copy } => {
            let body = if let Some(focus) = focus {
                selectable_user_message(lines, copy, focus, &colors, window, cx)
            } else {
                user_message_item(lines, &colors)
            };
            message_with_copy(body, copy, copy.id, true, colors)
        }
        StreamEntry::UserImages { images } => attachments::render_user_images(images, cx),
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
                markdown_message_item(
                    model,
                    failure.filter(|_| !suppress_run_notices),
                    &colors,
                    &format!("assistant-message-{}", copy.id),
                    Some(copy.id),
                )
            }
        }
        StreamEntry::Tool { card } => {
            let card = card.clone();
            process_surface(&colors)
                .child(ToolCard::render(card, false, window, cx))
                .into_any_element()
        }
        StreamEntry::ToolGroup { group } => {
            let group = group.clone();
            process_surface(&colors)
                .child(ToolActivityGroup::render(group, window, cx))
                .into_any_element()
        }
        StreamEntry::ArtifactGroup { group } => ArtifactGroup::render(group.clone(), window, cx),
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
            vega_conversation::types::TaskSummaryOutcome::Completed => {
                div().h_0().into_any_element()
            }
            outcome => div()
                .py_1()
                .child(plain_error_notice(
                    if outcome == vega_conversation::types::TaskSummaryOutcome::Failed {
                        RunFailureKind::Persisted.message()
                    } else {
                        "Stopped".into()
                    },
                    if outcome == vega_conversation::types::TaskSummaryOutcome::Failed {
                        colors.danger
                    } else {
                        colors.text_secondary
                    },
                ))
                .into_any_element(),
        },
        StreamEntry::SkillActivation { activation } => {
            use vega_conversation::history::{
                SkillHistoryOrigin, SkillHistorySource, SkillHistoryStatus,
                SkillHistoryVerification,
            };
            let source = match activation.source_scope {
                SkillHistorySource::Project => "Project",
                SkillHistorySource::VegaGlobal => "Vega global",
                SkillHistorySource::Imported => "Imported",
            };
            let origin = match activation.origin {
                SkillHistoryOrigin::Model => "Loaded by model",
                SkillHistoryOrigin::ExplicitUser => "Preloaded by user",
            };
            let status = match activation.status {
                SkillHistoryStatus::Loaded => "Loaded",
                SkillHistoryStatus::Revoked => "Revoked",
            };
            let verification = match activation.verification {
                SkillHistoryVerification::Verified => "Snapshot verified",
                SkillHistoryVerification::Unavailable => "Snapshot unavailable or unverified",
            };
            let digest = activation.content_sha256.get(..12).unwrap_or("invalid");
            div()
                .py_1()
                .text_size(px(Typography::METADATA))
                .text_color(colors.text_secondary)
                .child(format!(
                    "Skill history {} · {} · {} · {} · {} · SHA-256 {}…",
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
    id_prefix: &str,
) -> AnyElement {
    markdown_message_item(model, failure, colors, id_prefix, None)
}

fn markdown_message_item(
    model: &StreamModel,
    failure: Option<RunFailureKind>,
    colors: &ThemeColors,
    id_prefix: &str,
    code_owner: Option<u64>,
) -> AnyElement {
    let message_id = model
        .committed_lines
        .first()
        .or_else(|| model.pending_lines.first())
        .map_or(0, |line| line.block_id);
    let id_prefix = id_prefix.to_owned();
    div()
        .w_full()
        .flex_shrink_0()
        .pt(px(Layout::CONVERSATION_BODY_TOP))
        .flex()
        .flex_col()
        .children(markdown_blocks(
            model,
            colors,
            code_owner,
            None,
            |line, index| {
                let id = format!("{id_prefix}-markdown-{message_id}-line-{index}");
                let marker =
                    independent_list_marker(line).map(|marker| render_line(&marker, colors, &id));
                let content = list_item_content(line);
                (
                    marker,
                    render_line(content.as_ref().unwrap_or(line), colors, &id),
                )
            },
        ))
        .children(
            failure.map(|reason| div().child(plain_error_notice(reason.message(), colors.danger))),
        )
        .into_any_element()
}

pub(super) fn status_notice(message: String, color: Rgba, colors: &ThemeColors) -> AnyElement {
    status_notice_with_icon(message, color, colors, Icon::Warning)
}

pub(super) fn status_notice_with_icon(
    message: String,
    color: Rgba,
    colors: &ThemeColors,
    icon_kind: Icon,
) -> AnyElement {
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
                .mt(px((Typography::BODY * Typography::BODY_LINE_HEIGHT
                    - Layout::ICON_SIZE)
                    / 2.0))
                .size(px(Layout::ICON_SIZE))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(icon_kind, color)),
        )
        .child(div().min_w_0().flex_1().child(message))
        .into_any_element()
}

pub(crate) fn user_message_item(lines: &[StreamLine], colors: &ThemeColors) -> AnyElement {
    let mut body = div()
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
    for (index, line) in lines.iter().enumerate() {
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
            let runs = block_text_runs(&line.spans, user_body_style(colors), colors);
            body = body.child(super::inline_code::interactive_text(
                text,
                runs,
                link_ranges(&line.spans),
                format!("user-message-{}-{index}", line.block_id),
            ));
        }
    }
    div()
        .w_full()
        .flex_shrink_0()
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
            let runs = block_text_runs(&line.spans, user_body_style(colors), colors);
            let selected =
                document.append_styled_runs(&text, runs, link_ranges(&line.spans), "user-body");
            body = body.child(selected);
        }
    }
    let body = div()
        .w_full()
        .flex_shrink_0()
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
        .w_full()
        .flex_shrink_0()
        .pt(px(Layout::CONVERSATION_BODY_TOP))
        .flex()
        .flex_col()
        .child(selectable_markdown_content(
            model, copy, focus, colors, false, window, cx,
        ))
        .children(
            failure.map(|reason| div().child(plain_error_notice(reason.message(), colors.danger))),
        )
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
    let mut previous: Option<(u64, LineKind, Option<Arc<StreamCodeBlock>>)> = None;
    let selection = document.code_drag_selection(window, cx);
    let blocks = markdown_blocks(
        model,
        colors,
        Some(copy.id),
        Some(selection),
        |line, index| {
            if !matches!(line.kind, LineKind::Rule)
                && let Some((previous_block, previous_kind, previous_code)) = &previous
            {
                let same_multiline_block = (*previous_kind == LineKind::Code
                    && line.kind == LineKind::Code
                    && previous_code
                        .as_ref()
                        .zip(line.code_block.as_ref())
                        .is_some_and(|(previous, current)| Arc::ptr_eq(previous, current)))
                    || (*previous_block == line.block_id
                        && (matches!(previous_kind, LineKind::ListItem)
                            && matches!(line.kind, LineKind::ListItem)));
                document.append_literal(if same_multiline_block { "\n" } else { "\n\n" });
            }
            let marker = independent_list_marker(line).map(|marker| {
                render_line_selectable(&marker, colors, &mut document, index, copy.id)
            });
            let content = list_item_content(line);
            let element = render_line_selectable(
                content.as_ref().unwrap_or(line),
                colors,
                &mut document,
                index,
                copy.id,
            );
            if !matches!(line.kind, LineKind::Rule) {
                previous = Some((line.block_id, line.kind, line.code_block.clone()));
            }
            (marker, element)
        },
    );
    selection_context_menu(
        document.wrap(body.children(blocks).into_any_element(), window, cx),
        copy,
        copy_original,
    )
}

fn independent_list_marker(line: &StreamLine) -> Option<StreamLine> {
    if !line.starts_list_item()
        || (line.kind == LineKind::ListItem && line.list_marker_quote_depth().is_none())
    {
        return None;
    }
    let mut marker = line.clone();
    if line.kind != LineKind::ListItem {
        marker.quotes.clear();
    }
    marker.kind = LineKind::ListItem;
    marker.spans.clear();
    marker.table = None;
    marker.code_block = None;
    Some(marker)
}

fn list_item_content(line: &StreamLine) -> Option<StreamLine> {
    line.list_marker_quote_depth()?;
    let mut content = line.clone();
    content.kind = LineKind::Paragraph;
    content.marker.clear();
    content.checked = None;
    Some(content)
}

fn markdown_blocks(
    model: &StreamModel,
    colors: &ThemeColors,
    code_owner: Option<u64>,
    selection: Option<CodeDragSelection>,
    mut render: impl FnMut(&StreamLine, usize) -> (Option<AnyElement>, AnyElement),
) -> Vec<AnyElement> {
    let lines: Vec<_> = model
        .committed_lines
        .iter()
        .chain(&model.pending_lines)
        .collect();
    let mut blocks = Vec::new();
    let mut index = 0;
    let mut code_ordinal = 0;
    while index < lines.len() {
        let line = lines[index];
        let previous = index.checked_sub(1).map(|index| lines[index]);
        let start = index;
        let block_id = line.block_id;
        let in_list = line.in_list;
        let previous_in_list = previous.is_some_and(|line| line.in_list);
        let gap = markdown_block_gap(previous, line, in_list, previous_in_list);
        let (marker, element) = render(line, index);
        let mut content = vec![element];
        index += 1;
        if line.kind == LineKind::Code {
            while index < lines.len() && lines[index].continues_code_block(line) {
                content.push(render(lines[index], index).1);
                index += 1;
            }
        }
        let body = div().w_full().min_w_0().flex_shrink_0().when(
            in_list && line.kind != LineKind::ListItem && marker.is_none(),
            |body| {
                body.pl(px(
                    (line.list_content_inset() - line.quote_list_inset()).max(0.)
                ))
            },
        );
        let body = if line.kind == LineKind::Code {
            let ordinal = code_ordinal;
            code_ordinal += 1;
            let header = code_owner
                .zip(line.code_block.as_ref())
                .map(|(owner, code)| code_header(code, owner, ordinal, colors));
            body.child(
                div()
                    .w_full()
                    .min_w_0()
                    .bg(colors.code_bg)
                    .rounded(px(Layout::TOOL_DETAIL_RADIUS))
                    .px(px(Layout::MARKDOWN_CODE_PADDING))
                    .pt(px(Layout::MARKDOWN_CODE_PADDING))
                    .flex()
                    .flex_col()
                    .when(header.is_some(), |body| {
                        body.gap(px(Layout::COPY_ACTION_GAP))
                    })
                    .children(header)
                    .child(
                        CodeViewport::new(
                            gpui_kit::SharedString::from(format!(
                                "markdown-code-scroll-{}-{block_id}-{start}",
                                code_owner.unwrap_or_default(),
                            )),
                            div()
                                .min_w_full()
                                .flex_shrink_0()
                                .pb(px(Layout::MARKDOWN_CODE_PADDING))
                                .flex()
                                .flex_col()
                                .items_start()
                                .children(content),
                            colors.code_bg,
                        )
                        .selection(selection.clone()),
                    ),
            )
        } else {
            body.children(content)
        };
        let quote_marker_depth = line.list_marker_quote_depth();
        let (body, quote_marker) = if quote_marker_depth.is_some() {
            (body, marker.zip(quote_marker_depth))
        } else if let Some(marker) = marker {
            (
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .child(div().flex_shrink_0().child(marker))
                    .child(div().flex_1().min_w_0().child(body)),
                None,
            )
        } else {
            (body, None)
        };
        blocks.push((
            line.quotes.clone(),
            gap,
            quote_marker,
            body.into_any_element(),
        ));
    }
    quote_blocks(&mut blocks.into_iter().peekable(), 0, None, colors)
}

type MarkdownBlock = (
    Vec<model::QuoteOwner>,
    f32,
    Option<(AnyElement, usize)>,
    AnyElement,
);

fn quote_blocks(
    blocks: &mut std::iter::Peekable<impl Iterator<Item = MarkdownBlock>>,
    depth: usize,
    owner: Option<model::QuoteOwner>,
    colors: &ThemeColors,
) -> Vec<AnyElement> {
    let mut result = Vec::new();
    while let Some((ancestry, gap, _, _)) = blocks.peek() {
        if depth > 0 && ancestry.get(depth - 1).copied() != owner {
            break;
        }
        let gap = *gap;
        if let Some(identity) = ancestry.get(depth).copied() {
            let marker = blocks.peek_mut().and_then(|(_, first_gap, marker, _)| {
                *first_gap = 0.;
                if marker
                    .as_ref()
                    .is_some_and(|(_, marker_depth)| *marker_depth == depth)
                {
                    marker.take().map(|(marker, _)| marker)
                } else {
                    None
                }
            });
            let list_inset =
                (identity.list_inset() - owner.map_or(0., |owner| owner.list_inset())).max(0.);
            let marker_inset = if marker.is_some() { list_inset } else { 0. };
            let children = quote_blocks(blocks, depth + 1, Some(identity), colors);
            result.push(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .mt(px(gap))
                    .pl(px((list_inset - marker_inset).max(0.)))
                    .flex()
                    .children(
                        marker
                            .map(|marker| div().w(px(marker_inset)).flex_shrink_0().child(marker)),
                    )
                    .child(
                        div()
                            .w(px(2.))
                            .mr(px(Layout::MARKDOWN_CODE_PADDING))
                            .flex_shrink_0()
                            .bg(colors.border_subtle),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .children(children),
                    )
                    .into_any_element(),
            );
        } else if let Some((_, _, _, element)) = blocks.next() {
            result.push(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .mt(px(gap))
                    .child(element)
                    .into_any_element(),
            );
        }
    }
    result
}

fn code_header(
    code: &StreamCodeBlock,
    owner: u64,
    ordinal: usize,
    colors: &ThemeColors,
) -> AnyElement {
    let selector = format!("markdown-code-copy-{owner}-{ordinal}");
    let receipt_id = format!(
        "code-copy-receipt-{owner}-{}-{}",
        code.identity.0, code.identity.1
    );
    div()
        .w_full()
        .min_w_0()
        .h(px(Layout::COPY_ACTION_SIZE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(Layout::COPY_ACTION_GAP))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .text_size(px(Typography::METADATA))
                .text_color(colors.text_secondary)
                .children(code.language.clone()),
        )
        .child(CopyReceiptButton::new(
            receipt_id.into(),
            CopySource::Code(code.code.clone()),
            "复制代码",
            *colors,
            |button| {
                button
                    .id(gpui_kit::SharedString::from(selector))
                    .size(px(Layout::COPY_ACTION_SIZE))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .focus_visible(|style| style.bg(colors.bg_active))
            },
        ))
        .into_any_element()
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
    if in_list
        && previous_in_list
        && previous.block_id == current.block_id
        && previous.quotes == current.quotes
    {
        if current.kind == LineKind::ListItem {
            return Layout::MARKDOWN_LIST_ITEM_GAP;
        }
        if !current.quotes.is_empty() {
            return Layout::MARKDOWN_BLOCK_GAP;
        }
        return Layout::MARKDOWN_LIST_PARAGRAPH_GAP;
    }
    if !previous.quotes.is_empty() || !current.quotes.is_empty() {
        return Layout::MARKDOWN_BLOCK_GAP;
    }
    let after = if previous_in_list {
        Layout::MARKDOWN_BLOCK_GAP
    } else {
        match previous.kind {
            LineKind::Heading(_) => Layout::MARKDOWN_HEADING_AFTER,
            LineKind::Code | LineKind::ListItem | LineKind::Table => Layout::MARKDOWN_BLOCK_GAP,
            _ => Layout::CONVERSATION_PARAGRAPH_GAP,
        }
    };
    let before = match current.kind {
        LineKind::Heading(_) => Layout::MARKDOWN_HEADING_BEFORE,
        LineKind::Code | LineKind::ListItem | LineKind::Table => Layout::MARKDOWN_BLOCK_GAP,
        _ => 0.,
    };
    after.max(before)
}

pub(super) fn selection_context_menu(
    body: AnyElement,
    copy: &MessageCopy,
    copy_original: bool,
) -> AnyElement {
    use gpui_kit::component::menu::PopupMenuItem;

    let copy_for_menu = copy.clone();
    super::selection_menu::SelectionMenu::new(copy.id, body, move |menu, _, _| {
        let selected = copy_for_menu.clone();
        let original = copy_for_menu.clone();
        menu.when(copy_original, |menu| {
            menu.item(PopupMenuItem::new("复制原文").on_click(move |_, _, cx| original.copy(cx)))
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
    line_index: usize,
    message_id: u64,
) -> AnyElement {
    let id_prefix = format!("assistant-{message_id}-line-{line_index}");
    if let Some(table) = &line.table {
        return render_table_selectable(line.block_id, &id_prefix, table, colors, document);
    }
    let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
    let item = div()
        .when(
            line.kind != LineKind::Code
                && !(line.kind == LineKind::ListItem && line.spans.is_empty()),
            |item| item.w_full(),
        )
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
                link_ranges(&line.spans),
                &format!("{id_prefix}-user"),
            );
            item.child(div().px_2().py(px(1.0)).child(selected))
                .into_any_element()
        }
        LineKind::Code => {
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(&line.spans, code_run_style(colors.text_primary), colors),
                link_ranges(&line.spans),
                &format!("{id_prefix}-code"),
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
        LineKind::Rule => item
            .py(px(4.0))
            .child(div().w_full().h(px(1.0)).bg(colors.border_subtle))
            .into_any_element(),
        LineKind::Heading(level) => {
            let (size, weight) = heading_style(level);
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(&line.spans, heading_run_style(level, colors), colors),
                link_ranges(&line.spans),
                &format!("{id_prefix}-heading"),
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
            let mut row = div().pl(px(line.list_marker_inset())).flex().flex_row();
            let marker_text = document.append_styled(
                &marker,
                StyledText::new(marker.clone()),
                &format!("{id_prefix}-list-marker"),
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
                    &format!("{id_prefix}-list-checkbox"),
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
                link_ranges(&line.spans),
                &format!("{id_prefix}-list-content"),
            );
            row = row.child(div().flex_1().min_w_0().child(content));
            item.child(row).into_any_element()
        }
        LineKind::Paragraph => {
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(&line.spans, message_run_style(colors.text_primary), colors),
                link_ranges(&line.spans),
                &format!("{id_prefix}-paragraph"),
            );
            item.child(selected).into_any_element()
        }
    }
}

fn render_table_selectable(
    block_id: u64,
    id_prefix: &str,
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
                link_ranges(spans),
                &format!("{id_prefix}-table-{block_id}-{ordinal}-{row_index}-{column}"),
            );
            row = row.child(
                div()
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
            "{id_prefix}-markdown-table-{block_id}-{ordinal}"
        )))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .overflow_x_scroll()
        .child(body)
        .scroll_edge_fade(colors.bg_base, ScrollFade::compact())
        .into_any_element()
}

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

fn markdown_text(
    spans: &[StreamSpan],
    style: TextStyle,
    colors: &ThemeColors,
    id_prefix: String,
) -> AnyElement {
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let runs = block_text_runs(spans, style, colors);
    let links = link_ranges(spans);
    super::inline_code::interactive_text(text, runs, links, id_prefix)
}

pub(super) fn block_text_runs(
    spans: &[StreamSpan],
    default_style: TextStyle,
    colors: &ThemeColors,
) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::with_capacity(spans.len());
    for span in spans {
        let mut style = default_style.clone();
        let linked = span.link.as_deref().is_some_and(super::links::allowed_link);
        if span.style != SpanStyle::Link || linked {
            apply_span_style(&mut style, span.style, colors);
        }
        if linked {
            apply_span_style(&mut style, SpanStyle::Link, colors);
        }
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
            style.underline = None;
            if style.font_weight < FontWeight::MEDIUM {
                style.font_weight = FontWeight::MEDIUM;
            }
            style.color = colors.accent.into();
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

pub(super) fn heading_run_style(level: u8, colors: &ThemeColors) -> TextStyle {
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
pub(crate) fn render_line(line: &StreamLine, colors: &ThemeColors, id_prefix: &str) -> AnyElement {
    if let Some(table) = &line.table {
        return render_table(line.block_id, table, colors, id_prefix);
    }
    let text_id = format!("{id_prefix}-text");
    let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
    let item = div()
        .when(
            line.kind != LineKind::Code
                && !(line.kind == LineKind::ListItem && line.spans.is_empty()),
            |item| item.w_full(),
        )
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
                text_id.clone(),
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
                    format!("{id_prefix}-code"),
                ));
            let code = if text.is_empty() {
                // 空代码行保一行等高占位（非定高行模型）。
                code.h(px(Typography::CODE * Typography::CODE_LINE_HEIGHT))
            } else {
                code
            };
            item.child(code).into_any_element()
        }
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
                    format!("{id_prefix}-heading"),
                ))
                .into_any_element()
        }
        LineKind::Table => item.into_any_element(),
        LineKind::ListItem => {
            let mut row = div().pl(px(line.list_marker_inset())).flex().flex_row();
            row = row.child(
                div()
                    .flex_shrink_0()
                    .w(px(Layout::MARKDOWN_LIST_INDENT))
                    .text_color(colors.text_secondary)
                    .child(format!("{} ", line.marker).into_any_element()),
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
                        .child((if checked { "[x]" } else { "[ ]" }).into_any_element()),
                );
            }
            row = row.child(div().flex_1().min_w_0().child(markdown_text(
                &line.spans,
                message_run_style(colors.text_primary),
                colors,
                format!("{id_prefix}-list"),
            )));
            item.child(row).into_any_element()
        }
        LineKind::Paragraph => item
            .child(markdown_text(
                &line.spans,
                message_run_style(colors.text_primary),
                colors,
                text_id,
            ))
            .into_any_element(),
    }
}

fn render_table(
    block_id: u64,
    table: &StreamTable,
    colors: &ThemeColors,
    id_prefix: &str,
) -> AnyElement {
    let id_prefix = id_prefix.to_owned();
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
                    .flex_basis(px(Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH))
                    .flex_grow(weights[column])
                    .flex_shrink_0()
                    .min_w_0()
                    .px(px(Layout::MARKDOWN_TABLE_PADDING_X))
                    .py(px(Layout::MARKDOWN_TABLE_PADDING_Y))
                    .text_align(alignment)
                    .child(markdown_text(
                        spans,
                        style,
                        colors,
                        format!("{id_prefix}-table-{block_id}-{ordinal}-{row_index}-{column}"),
                    )),
            );
        }
        body = body.child(row);
    }
    div()
        .id(gpui_kit::SharedString::from(format!(
            "{id_prefix}-markdown-table-{block_id}-{ordinal}"
        )))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .overflow_x_scroll()
        .child(body)
        .scroll_edge_fade(colors.bg_base, ScrollFade::compact())
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

pub(super) fn message_with_copy(
    body: AnyElement,
    copy: &MessageCopy,
    id: u64,
    user: bool,
    colors: ThemeColors,
) -> AnyElement {
    if !copy.has_text() {
        return body;
    }
    let group: gpui_kit::SharedString = format!("message-copy-group-{id}").into();
    div()
        .id(("message-with-copy", id))
        .group(group.clone())
        .w_full()
        .flex_shrink_0()
        .relative()
        .flex()
        .flex_col()
        .when(!user, |body| body.pb(px(Layout::MESSAGE_COPY_FOOTER)))
        .child(body)
        .child(
            div()
                .absolute()
                .when(user, |row| {
                    row.right_0().bottom(px(
                        Layout::CONVERSATION_USER_BOTTOM - Layout::MESSAGE_COPY_FOOTER
                    ))
                })
                .when(!user, |row| row.left_0().bottom_0())
                .flex()
                .child(CopyReceiptButton::new(
                    format!("message-copy-receipt-{}", copy.id).into(),
                    CopySource::Message(copy.clone()),
                    if user {
                        "复制消息"
                    } else {
                        "复制最终回复"
                    },
                    colors,
                    |button| {
                        button
                            .id(("message-copy", id))
                            .size(px(Layout::COPY_ACTION_SIZE))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .opacity(0.)
                            .group_hover(group, |style| style.opacity(1.))
                            .focus_visible(|style| style.opacity(1.).bg(colors.bg_active))
                    },
                )),
        )
        .into_any_element()
}

pub(super) fn plain_error_notice(message: String, color: Rgba) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .text_size(px(Typography::BODY))
        .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
        .text_color(color)
        .whitespace_normal()
        .child(message)
        .into_any_element()
}
