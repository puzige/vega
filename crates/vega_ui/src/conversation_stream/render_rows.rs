use super::code_drag::CodeDragSelection;
use super::code_viewport::CodeViewport;
use super::copy_receipt::{CopyReceiptButton, CopySource};
use super::links::link_ranges;
use super::selection::SelectionDocumentBuilder;
use super::*;
use crate::icons::{Icon, icon, sized_icon};
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

/// Whether a user message has anything to show. Image-only turns still
/// materialize a text entry next to their attachments, and that entry must not
/// render as an empty bubble. Judged on the message text first, so a turn that
/// only carries blank lines keeps its previous (blank line) rendering.
pub(crate) fn user_message_is_visible(copy: &MessageCopy, lines: &[StreamLine]) -> bool {
    copy.has_text()
        || lines.iter().any(|line| {
            matches!(line.kind, LineKind::UserLine { .. })
                && line.spans.iter().any(|span| !span.text.is_empty())
        })
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

#[derive(Clone, Copy)]
pub(crate) struct MessageInteraction<'a> {
    pub(crate) focus: &'a FocusHandle,
    pub(crate) actions_enabled: bool,
}

pub(crate) fn render_entry(entry: &StreamEntry, window: &mut Window, cx: &mut App) -> AnyElement {
    render_entry_internal(entry, window, cx, None, false)
}

pub(crate) fn render_entry_with_selection(
    entry: &StreamEntry,
    window: &mut Window,
    cx: &mut App,
    interaction: MessageInteraction<'_>,
) -> AnyElement {
    render_entry_internal(entry, window, cx, Some(interaction), false)
}

pub(crate) fn render_stream_entry(
    entry: &StreamEntry,
    suppress_run_notices: bool,
    process_top: f32,
    window: &mut Window,
    cx: &mut App,
    interaction: MessageInteraction<'_>,
) -> AnyElement {
    let body = render_entry_internal(entry, window, cx, Some(interaction), suppress_run_notices);
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

pub(crate) fn render_compaction_activity(
    label: impl IntoElement,
    colors: &ThemeColors,
) -> AnyElement {
    let (glyph, color) = context_compaction_visual(
        vega_conversation::types::ContextCompactionStatus::Compacting,
        colors,
    );
    let line_height = Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT;
    let vertical_padding = (Layout::CONVERSATION_TOOL_HEIGHT - line_height) / 2.;
    div()
        .w_full()
        .flex_shrink_0()
        .min_w_0()
        .min_h(px(Layout::CONVERSATION_TOOL_HEIGHT))
        .py(px(vertical_padding))
        .flex()
        .items_start()
        .gap(px(Layout::CONVERSATION_INLINE_GAP))
        .text_size(px(Typography::CONVERSATION))
        .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
        .text_color(color)
        .child(
            div()
                .w(px(Layout::ICON_SIZE))
                .h(px(line_height))
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .child(icon(glyph, color)),
        )
        .child(div().min_w_0().flex_1().whitespace_normal().child(label))
        .into_any_element()
}

fn render_entry_internal(
    entry: &StreamEntry,
    window: &mut Window,
    cx: &mut App,
    interaction: Option<MessageInteraction<'_>>,
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
            render_compaction_activity(
                context_compaction_label(record.status, record.failure, *restored),
                &colors,
            )
        }
        StreamEntry::User { lines, copy } => {
            if !user_message_is_visible(copy, lines) {
                // 仅图片的轮次不应产生空气泡，也不挂复制入口。
                return div().h_0().into_any_element();
            }
            let body = if let Some(interaction) = interaction {
                selectable_user_message(lines, copy, interaction, &colors, window, cx)
            } else {
                user_message_item(lines, &colors)
            };
            message_with_copy(
                body,
                copy,
                copy.id,
                true,
                colors,
                interaction.is_none_or(|interaction| interaction.actions_enabled),
            )
        }
        StreamEntry::UserImages { images } => attachments::render_user_images(images, cx),
        StreamEntry::Screenshot { card } => card.clone().into_any_element(),
        StreamEntry::Assistant {
            model,
            failure,
            copy,
            ..
        } => {
            if let Some(interaction) = interaction {
                selectable_markdown_message(
                    model,
                    failure.filter(|_| !suppress_run_notices),
                    copy,
                    interaction,
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
                        vega_i18n::t("ui.stopped").into()
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
                SkillHistorySource::Project => vega_i18n::t("ui.project"),
                SkillHistorySource::VegaGlobal => vega_i18n::t("ui.vega_global"),
                SkillHistorySource::Imported => vega_i18n::t("ui.imported"),
            };
            let origin = match activation.origin {
                SkillHistoryOrigin::Model => vega_i18n::t("ui.loaded_by_model"),
                SkillHistoryOrigin::ExplicitUser => vega_i18n::t("ui.preloaded_by_user"),
            };
            let status = match activation.status {
                SkillHistoryStatus::Loaded => vega_i18n::t("ui.loaded"),
                SkillHistoryStatus::Revoked => vega_i18n::t("ui.revoked"),
            };
            let verification = match activation.verification {
                SkillHistoryVerification::Verified => vega_i18n::t("ui.snapshot_verified"),
                SkillHistoryVerification::Unavailable => {
                    vega_i18n::t("ui.snapshot_unavailable_or_unverified")
                }
            };
            let digest = activation
                .content_sha256
                .get(..12)
                .unwrap_or_else(|| vega_i18n::t("ui.invalid"));
            div()
                .py_1()
                .text_size(px(Typography::METADATA))
                .text_color(colors.text_secondary)
                .child(vega_i18n::t_args(
                    "ui.skill_history_name_source_origin_status_verification_sha",
                    &[
                        ("name", &activation.name),
                        ("source", source),
                        ("origin", origin),
                        ("status", status),
                        ("verification", verification),
                        ("digest", digest),
                    ],
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
            true,
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

pub(super) fn inline_status_notice(message: String, color: Rgba, icon_kind: Icon) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .min_h(px(Layout::CONVERSATION_TOOL_HEIGHT))
        .flex()
        .items_center()
        .gap(px(Layout::CONVERSATION_INLINE_GAP))
        .text_size(px(Typography::CONVERSATION))
        .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
        .text_color(color)
        .child(
            div()
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
        .block();
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
    interaction: MessageInteraction<'_>,
    colors: &ThemeColors,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let selection_color = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
    let mut document =
        SelectionDocumentBuilder::new(copy.clone(), interaction.focus.clone(), selection_color);
    let mut body = div()
        .max_w(gpui_kit::relative(Layout::CONVERSATION_USER_RATIO))
        .bg(colors.conversation_bubble)
        .rounded(px(Layout::USER_MESSAGE_RADIUS))
        .text_size(px(Typography::CONVERSATION))
        .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
        .px(px(Layout::CONVERSATION_USER_PADDING))
        .py_2()
        .text_color(colors.conversation_user)
        .block();
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
    selection_context_menu(
        document.wrap(body, window, cx),
        copy,
        false,
        interaction.actions_enabled,
    )
}

fn selectable_markdown_message(
    model: &StreamModel,
    failure: Option<RunFailureKind>,
    copy: &MessageCopy,
    interaction: MessageInteraction<'_>,
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
            model,
            copy,
            interaction,
            colors,
            false,
            window,
            cx,
        ))
        .children(
            failure.map(|reason| div().child(plain_error_notice(reason.message(), colors.danger))),
        )
        .into_any_element()
}

pub(crate) fn selectable_markdown_content(
    model: &StreamModel,
    copy: &MessageCopy,
    interaction: MessageInteraction<'_>,
    colors: &ThemeColors,
    copy_original: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let selection_color = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
    let mut document =
        SelectionDocumentBuilder::new(copy.clone(), interaction.focus.clone(), selection_color)
            .with_punctuation_rules();
    let body = div().w_full().min_w_0().flex_shrink_0().flex().flex_col();
    let mut previous: Option<(u64, LineKind, Option<Arc<StreamCodeBlock>>)> = None;
    let selection = document.code_drag_selection(window, cx);
    let mut marker_style = window.text_style();
    marker_style.font_weight = FontWeight::SEMIBOLD;
    let layout = AssistantListLayout::new(model, |marker| {
        f32::from(
            window
                .text_system()
                .shape_line(
                    marker.to_owned().into(),
                    px(Typography::ASSISTANT_BODY),
                    &[marker_style.to_run(marker.len())],
                    None,
                )
                .width(),
        )
        .ceil()
    });
    let blocks = markdown_blocks(
        model,
        colors,
        Some(copy.id),
        Some(selection),
        interaction.actions_enabled,
        Some(&layout),
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
                render_line_selectable(&marker, colors, &mut document, index, copy.id, &layout)
            });
            let content = list_item_content(line);
            let element = render_line_selectable(
                content.as_ref().unwrap_or(line),
                colors,
                &mut document,
                index,
                copy.id,
                &layout,
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
        interaction.actions_enabled,
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
    actions_enabled: bool,
    list_layout: Option<&AssistantListLayout>,
    mut render: impl FnMut(&StreamLine, usize) -> (Option<AnyElement>, AnyElement),
) -> Vec<AnyElement> {
    let assistant = list_layout.is_some();
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
        let gap = if assistant {
            assistant_block_gap(previous, line)
        } else {
            markdown_block_gap(previous, line, in_list, previous_in_list)
        };
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
                let inset = list_layout.map_or_else(
                    || line.list_content_inset() - line.quote_list_inset(),
                    |layout| layout.content_inset(line) - layout.quote_inset(line.quotes.last()),
                );
                body.pl(px(inset.max(0.)))
            },
        );
        let body = if line.kind == LineKind::Code {
            let ordinal = code_ordinal;
            code_ordinal += 1;
            let header = code_owner
                .zip(line.code_block.as_ref())
                .map(|(owner, code)| code_header(code, owner, ordinal, colors, actions_enabled));
            body.child(
                div()
                    .w_full()
                    .min_w_0()
                    .bg(colors.code_bg)
                    .rounded(px(if assistant {
                        Layout::ASSISTANT_CODE_RADIUS
                    } else {
                        Layout::TOOL_DETAIL_RADIUS
                    }))
                    .when(assistant, |body| {
                        body.border_1().border_color(colors.border_subtle)
                    })
                    .px(px(if assistant {
                        Layout::ASSISTANT_CODE_PADDING_X
                    } else {
                        Layout::MARKDOWN_CODE_PADDING
                    }))
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
    quote_blocks(
        &mut blocks.into_iter().peekable(),
        0,
        None,
        colors,
        list_layout,
    )
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
    list_layout: Option<&AssistantListLayout>,
) -> Vec<AnyElement> {
    let assistant = list_layout.is_some();
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
            let list_inset = list_layout
                .map_or_else(
                    || identity.list_inset() - owner.map_or(0., |owner| owner.list_inset()),
                    |layout| {
                        layout.quote_inset(Some(&identity)) - layout.quote_inset(owner.as_ref())
                    },
                )
                .max(0.);
            let marker_inset = if marker.is_some() { list_inset } else { 0. };
            let children = quote_blocks(blocks, depth + 1, Some(identity), colors, list_layout);
            result.push(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .mt(px(gap))
                    .pl(px((list_inset - marker_inset).max(0.)))
                    .when(assistant, |item| {
                        item.py(px(Layout::ASSISTANT_QUOTE_PADDING))
                    })
                    .flex()
                    .children(
                        marker
                            .map(|marker| div().w(px(marker_inset)).flex_shrink_0().child(marker)),
                    )
                    .child(
                        div()
                            .w(px(if assistant {
                                Layout::ASSISTANT_QUOTE_WIDTH
                            } else {
                                2.
                            }))
                            .mr(px(if assistant {
                                Layout::ASSISTANT_QUOTE_GAP
                            } else {
                                Layout::MARKDOWN_CODE_PADDING
                            }))
                            .flex_shrink_0()
                            .when(assistant, |bar| {
                                bar.rounded(px(Layout::ASSISTANT_QUOTE_WIDTH / 2.))
                            })
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
    actions_enabled: bool,
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
        .when(actions_enabled, |header| {
            header.child(CopyReceiptButton::new(
                receipt_id.into(),
                CopySource::Code(code.code.clone()),
                vega_i18n::t("ui.copy_code"),
                *colors,
                |button| {
                    button
                        .id(gpui_kit::SharedString::from(selector))
                        .size(px(Layout::COPY_ACTION_SIZE))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .focus_visible(|style| style.bg(colors.bg_active))
                },
            ))
        })
        .into_any_element()
}

pub(super) struct AssistantListLayout {
    widths: std::collections::HashMap<(u64, usize), f32>,
    quotes: std::collections::HashMap<(u64, usize), f32>,
}

impl AssistantListLayout {
    pub(super) fn new(model: &StreamModel, mut measure: impl FnMut(&str) -> f32) -> Self {
        let mut layout = Self {
            widths: std::collections::HashMap::new(),
            quotes: std::collections::HashMap::new(),
        };
        for line in model.committed_lines.iter().chain(&model.pending_lines) {
            if line.starts_list_item()
                && let Some(owner) = line.lists.iter().find(|owner| owner.depth == line.depth)
            {
                let width = if line.checked.is_some() || line.marker == "•" {
                    Layout::ASSISTANT_LIST_INDENT
                } else {
                    (measure(&line.marker) + Layout::ASSISTANT_LIST_MARKER_GAP)
                        .max(Layout::ASSISTANT_LIST_INDENT)
                };
                let current = layout.widths.entry(owner.identity).or_insert(width);
                *current = current.max(width);
            }
        }
        for line in model.committed_lines.iter().chain(&model.pending_lines) {
            for quote in &line.quotes {
                let inset = quote
                    .list_depth
                    .map_or(0., |depth| layout.inset(line, depth + 1));
                layout.quotes.entry(quote.identity).or_insert(inset);
            }
        }
        layout
    }

    fn inset(&self, line: &StreamLine, levels: usize) -> f32 {
        levels as f32 * Layout::ASSISTANT_LIST_INDENT
            + line
                .lists
                .iter()
                .filter(|owner| owner.depth < levels)
                .map(|owner| {
                    self.widths
                        .get(&owner.identity)
                        .copied()
                        .unwrap_or(Layout::ASSISTANT_LIST_INDENT)
                        - Layout::ASSISTANT_LIST_INDENT
                })
                .sum::<f32>()
    }

    pub(super) fn content_inset(&self, line: &StreamLine) -> f32 {
        if line.in_list {
            let depth = line.lists.last().map_or(line.depth, |owner| owner.depth);
            self.inset(line, depth + 1)
        } else {
            0.
        }
    }

    fn quote_inset(&self, quote: Option<&model::QuoteOwner>) -> f32 {
        quote
            .and_then(|quote| self.quotes.get(&quote.identity))
            .copied()
            .unwrap_or_default()
    }

    fn marker_width(&self, line: &StreamLine) -> f32 {
        self.inset(line, line.depth + 1) - self.inset(line, line.depth)
    }

    fn marker_inset(&self, line: &StreamLine) -> f32 {
        let parent = match line.list_marker_quote_depth() {
            Some(depth) => depth
                .checked_sub(1)
                .and_then(|depth| line.quotes.get(depth)),
            None => line.quotes.last(),
        };
        (self.inset(line, line.depth) - self.quote_inset(parent)).max(0.)
    }
}

pub(crate) fn assistant_block_gap(previous: Option<&StreamLine>, current: &StreamLine) -> f32 {
    let Some(previous) = previous else {
        return 0.;
    };
    if previous.block_id == current.block_id
        && previous.quotes == current.quotes
        && previous.in_list
        && current.in_list
        && current.kind == LineKind::ListItem
    {
        return 0.;
    }
    let after = match previous.kind {
        LineKind::Heading(1) => Layout::ASSISTANT_H1_AFTER,
        LineKind::Heading(2 | 3) => Layout::ASSISTANT_HEADING_AFTER,
        LineKind::Code => Layout::ASSISTANT_CODE_GAP,
        LineKind::Table | LineKind::Rule => Layout::ASSISTANT_BLOCK_GAP,
        _ => Layout::ASSISTANT_HEADING_AFTER,
    };
    let before = match current.kind {
        LineKind::Heading(_) | LineKind::Table | LineKind::Rule => Layout::ASSISTANT_BLOCK_GAP,
        LineKind::Code => Layout::ASSISTANT_CODE_GAP,
        LineKind::Paragraph if matches!(previous.kind, LineKind::Heading(_)) => 0.,
        LineKind::Paragraph if previous.kind == LineKind::Paragraph || current.in_list => {
            Layout::ASSISTANT_BLOCK_GAP
        }
        _ => Layout::ASSISTANT_H1_AFTER,
    };
    after.max(before)
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
    enabled: bool,
) -> AnyElement {
    use gpui_kit::component::menu::PopupMenuItem;

    let copy_for_menu = copy.clone();
    super::selection_menu::SelectionMenu::new(copy.id, body, move |menu, _, _| {
        let selected = copy_for_menu.clone();
        let original = copy_for_menu.clone();
        menu.when(copy_original, |menu| {
            menu.item(
                PopupMenuItem::new(vega_i18n::t("ui.copy_original"))
                    .on_click(move |_, _, cx| original.copy(cx)),
            )
        })
        .item(
            PopupMenuItem::new(vega_i18n::t("ui.copy"))
                .disabled(!copy_for_menu.has_selected_text())
                .on_click(move |_, _, cx| selected.copy_selected_text(cx)),
        )
    })
    .enabled(enabled)
    .into_any_element()
}

fn render_line_selectable(
    line: &StreamLine,
    colors: &ThemeColors,
    document: &mut SelectionDocumentBuilder,
    line_index: usize,
    message_id: u64,
    layout: &AssistantListLayout,
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
        .text_size(px(Typography::ASSISTANT_BODY))
        .line_height(gpui_kit::relative(Typography::ASSISTANT_LINE_HEIGHT))
        .text_color(colors.text_primary);
    match line.kind {
        LineKind::Spacer => item.py(px(6.0)).into_any_element(),
        LineKind::UserLabel => item
            .pt(px(4.0))
            .pb(px(2.0))
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_secondary)
            .child(div().px_2().child(vega_i18n::t("ui.you")))
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
            let style = TextStyle {
                font_size: AbsoluteLength::Pixels(px(Typography::ASSISTANT_CODE)),
                ..code_run_style(colors.text_primary)
            };
            let selected = document.append_code_runs(
                &text,
                block_text_runs(&line.spans, style, colors),
                link_ranges(&line.spans),
                &format!("{id_prefix}-code"),
            );
            let code = div()
                .text_size(px(Typography::ASSISTANT_CODE))
                .line_height(gpui_kit::relative(Typography::CODE_LINE_HEIGHT))
                .whitespace_nowrap()
                .text_color(colors.text_primary)
                .child(selected);
            let code = if text.is_empty() {
                code.h(px(Typography::ASSISTANT_CODE * Typography::CODE_LINE_HEIGHT))
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
            let (size, height) = assistant_heading_metrics(level);
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(
                    &line.spans,
                    assistant_heading_run_style(level, colors),
                    colors,
                ),
                link_ranges(&line.spans),
                &format!("{id_prefix}-heading"),
            );
            item.text_size(px(size))
                .line_height(gpui_kit::relative(height / size))
                .font_weight(Typography::HEADING_PAGE_WEIGHT)
                .child(selected)
                .into_any_element()
        }
        LineKind::Table => item.into_any_element(),
        LineKind::ListItem => {
            let mut marker_text = format!("{}{} ", "  ".repeat(line.depth), line.marker);
            if let Some(checked) = line.checked {
                marker_text.push_str(if checked { "[x] " } else { "[ ] " });
            }
            let marker = document.append_marker(
                &marker_text,
                assistant_list_marker(line, colors),
                &format!("{id_prefix}-list-marker"),
            );
            let mut row = div()
                .pl(px(layout.marker_inset(line)))
                .flex()
                .flex_row()
                .child(
                    div()
                        .w(px(layout.marker_width(line)))
                        .pr(px(Layout::ASSISTANT_LIST_MARKER_GAP))
                        .flex_shrink_0()
                        .flex()
                        .justify_end()
                        .child(marker),
                );
            let content = document.append_styled_runs(
                &text,
                block_text_runs(
                    &line.spans,
                    assistant_run_style(colors.text_primary),
                    colors,
                ),
                link_ranges(&line.spans),
                &format!("{id_prefix}-list-content"),
            );
            row = row.child(div().flex_1().min_w_0().child(content));
            item.child(row).into_any_element()
        }
        LineKind::Paragraph => {
            let selected = document.append_styled_runs(
                &text,
                block_text_runs(
                    &line.spans,
                    assistant_run_style(colors.text_primary),
                    colors,
                ),
                link_ranges(&line.spans),
                &format!("{id_prefix}-paragraph"),
            );
            item.child(selected).into_any_element()
        }
    }
}

pub(super) fn assistant_list_marker_height() -> f32 {
    Typography::ASSISTANT_BODY * Typography::ASSISTANT_LINE_HEIGHT
}

fn assistant_list_marker(line: &StreamLine, colors: &ThemeColors) -> AnyElement {
    let body = div()
        .h(px(assistant_list_marker_height()))
        .flex_shrink_0()
        .flex()
        .items_center()
        .text_color(colors.text_primary)
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap();
    if let Some(checked) = line.checked {
        return body
            .child(
                div()
                    .size(px(Layout::ASSISTANT_LIST_TASK_SIZE))
                    .rounded(px(Layout::ASSISTANT_LIST_TASK_RADIUS))
                    .border_1()
                    .border_color(colors.text_secondary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(checked, |checkbox| {
                        checkbox.bg(colors.text_primary).child(sized_icon(
                            Icon::Check,
                            colors.bg_base,
                            Layout::ASSISTANT_LIST_TASK_SIZE,
                        ))
                    }),
            )
            .into_any_element();
    }
    if line.marker != "•" {
        return body.child(line.marker.clone()).into_any_element();
    }
    let bullet = div().size(px(Layout::ASSISTANT_LIST_BULLET));
    body.child(match line.depth {
        0 => bullet.rounded_full().bg(colors.text_primary),
        1 => bullet
            .rounded_full()
            .border_1()
            .border_color(colors.text_primary),
        _ => bullet.bg(colors.text_primary),
    })
    .into_any_element()
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
        .text_size(px(Typography::ASSISTANT_TABLE))
        .line_height(gpui_kit::relative(Typography::ASSISTANT_LINE_HEIGHT))
        .w_full()
        .min_w(px(Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH * columns as f32))
        .flex_shrink_0()
        .flex()
        .flex_col();
    for (row_index, cells) in table.rows.iter().enumerate() {
        if row_index > 0 {
            document.append_literal("\n");
        }
        let mut row = div().w_full().flex().flex_row().flex_shrink_0().when(
            row_index == 0 || row_index + 1 < table.rows.len(),
            |row| {
                row.border_b_1().border_color(if row_index == 0 {
                    colors.conversation_marker
                } else {
                    colors.border_subtle
                })
            },
        );
        for (column, spans) in cells.iter().enumerate() {
            if column > 0 {
                document.append_literal("\t");
            }
            let mut style = TextStyle {
                font_size: px(Typography::ASSISTANT_TABLE).into(),
                ..assistant_run_style(colors.text_primary)
            };
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
                    .pr(px(assistant_table_right_padding(
                        row_index, column, columns,
                    )))
                    .pt(px(assistant_table_vertical_padding(
                        row_index,
                        table.rows.len(),
                    )
                    .0))
                    .pb(px(assistant_table_vertical_padding(
                        row_index,
                        table.rows.len(),
                    )
                    .1))
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

pub(crate) fn assistant_table_right_padding(row: usize, column: usize, columns: usize) -> f32 {
    if row == 0 && column + 1 == columns {
        Layout::ASSISTANT_TABLE_HEADER_RIGHT
    } else {
        Layout::ASSISTANT_TABLE_PADDING_RIGHT
    }
}

pub(crate) fn assistant_table_vertical_padding(row: usize, rows: usize) -> (f32, f32) {
    if row == 0 {
        (
            Layout::ASSISTANT_TABLE_HEADER_PADDING,
            Layout::ASSISTANT_TABLE_HEADER_PADDING,
        )
    } else {
        (
            Layout::ASSISTANT_TABLE_BODY_PADDING,
            if row + 1 == rows {
                Layout::ASSISTANT_TABLE_LAST_PADDING
            } else {
                Layout::ASSISTANT_TABLE_BODY_PADDING
            },
        )
    }
}

pub(crate) fn assistant_heading_metrics(level: u8) -> (f32, f32) {
    match level {
        1 => (Typography::ASSISTANT_H1, Typography::ASSISTANT_H1_HEIGHT),
        2 => (Typography::ASSISTANT_H2, Typography::ASSISTANT_H2_HEIGHT),
        3 => (Typography::ASSISTANT_H3, Typography::ASSISTANT_H3_HEIGHT),
        4 => (Typography::ASSISTANT_BODY, Typography::ASSISTANT_H4_HEIGHT),
        _ => (
            Typography::ASSISTANT_BODY,
            Typography::ASSISTANT_BODY * Typography::ASSISTANT_LINE_HEIGHT,
        ),
    }
}

fn assistant_run_style(color: Rgba) -> TextStyle {
    TextStyle {
        font_size: px(Typography::ASSISTANT_BODY).into(),
        line_height: gpui_kit::relative(Typography::ASSISTANT_LINE_HEIGHT),
        ..message_run_style(color)
    }
}

fn assistant_heading_run_style(level: u8, colors: &ThemeColors) -> TextStyle {
    let (font_size, line_height) = assistant_heading_metrics(level);
    TextStyle {
        font_size: px(font_size).into(),
        line_height: gpui_kit::relative(line_height / font_size),
        font_weight: Typography::HEADING_PAGE_WEIGHT,
        ..assistant_run_style(colors.text_primary)
    }
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
            .child(div().px_2().child(vega_i18n::t("ui.you")))
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
                    .text_color(if line.marker == "•" {
                        colors.text_primary
                    } else {
                        colors.text_secondary
                    })
                    .font_weight(if line.marker == "•" {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
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
    actions_enabled: bool,
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
        .when(actions_enabled, |body| {
            body.child(
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
                            vega_i18n::t("ui.copy_message")
                        } else {
                            vega_i18n::t("ui.copy_final_reply")
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
        })
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
