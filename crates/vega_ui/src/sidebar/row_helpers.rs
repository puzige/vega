use super::*;

/// A single hover action button on a session row (compact text label, token
/// colors only). The listener runs on the block entity, so button clicks do
/// not bubble into the row's clickable body (sibling nodes, T10 经验).
pub(crate) fn row_action_button(
    label: &'static str,
    text_color: gpui::Rgba,
    hover_bg: gpui::Rgba,
    listener: impl Fn(&mut ThreadsBlock, &MouseUpEvent, &mut Window, &mut Context<ThreadsBlock>)
    + 'static,
    cx: &mut Context<ThreadsBlock>,
) -> AnyElement {
    div()
        .px_1()
        .rounded_md()
        .text_size(px(Typography::SIDEBAR))
        .text_color(text_color)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .on_mouse_up(MouseButton::Left, cx.listener(listener))
        .child(label)
        .into_any_element()
}

/// The full-window delete confirmation overlay (T13): a token-derived
/// semi-transparent scrim over everything with a centered small card in the
/// ui-spec §4.3 权限卡 style — no shadow, `border_subtle` border, buttons
/// [删除] (danger) + [取消]. Clicking the scrim (any mouse down outside the
/// card) or pressing Esc cancels — Esc routes through the global
/// `CloseSettings` handler, which consumes the overlay first (裁决②). No
/// system modal is used (ui-spec §4.6). Rendered by the window root while
/// [`PendingDeleteConfirm`] is `Some`.
pub fn render_delete_confirm_overlay(
    thread: &Thread,
    sidebar: Entity<Sidebar>,
    colors: ThemeColors,
) -> AnyElement {
    let cancel = |_event: &MouseDownEvent, _window: &mut Window, cx: &mut App| {
        cx.set_global(PendingDeleteConfirm(None));
        cx.refresh_windows();
    };
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(colors.text_primary.opacity(0.4))
        .child(
            div()
                .w(px(320.))
                .flex()
                .flex_col()
                .gap_3()
                .rounded_lg()
                .border_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_elevated)
                .px_4()
                .py_4()
                .on_mouse_down_out(cancel)
                .child(
                    div()
                        .text_size(px(Typography::HEADING_CARD))
                        .font_weight(Typography::HEADING_CARD_WEIGHT)
                        .text_color(colors.text_primary)
                        .child("删除会话"),
                )
                .child(
                    div()
                        .text_size(px(Typography::SIDEBAR))
                        .text_color(colors.text_secondary)
                        .child(format!(
                            "确定删除「{}」？将同时删除该会话的全部消息与工具调用记录，且无法恢复。",
                            thread_title(thread)
                        )),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            // [取消]
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(colors.border_subtle)
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.text_secondary)
                                .cursor_pointer()
                                .hover(move |s| s.bg(colors.bg_hover))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    |_: &MouseUpEvent, _window, cx: &mut App| {
                                        cx.set_global(PendingDeleteConfirm(None));
                                        cx.refresh_windows();
                                    },
                                )
                                .child("取消"),
                        )
                        .child(
                            // [删除]：danger 主操作；确认后由编排器执行删除。
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(colors.danger)
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.bg_base)
                                .cursor_pointer()
                                .hover(move |s| s.bg(colors.danger.opacity(0.85)))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    move |_: &MouseUpEvent, _window, cx: &mut App| {
                                        sidebar.update(cx, Sidebar::confirm_pending_delete);
                                    },
                                )
                                .child("删除"),
                        ),
                ),
        )
        .into_any_element()
}

/// Clears the cached opened thread when it belongs to a project other than
/// `project_id` (used on project selection/removal).
pub(crate) fn clear_opened_thread_of_other_project(project_id: &str, cx: &mut App) {
    let stale = cx
        .global::<OpenedThread>()
        .0
        .as_ref()
        .is_some_and(|thread| thread.project_id != project_id);
    if stale {
        cx.set_global(OpenedThread(None));
    }
}

/// Clears the cached opened thread when its owning project was removed.
pub(crate) fn clear_opened_thread_of_project(project_id: &str, cx: &mut App) {
    let removed = cx
        .global::<OpenedThread>()
        .0
        .as_ref()
        .is_some_and(|thread| thread.project_id == project_id);
    if removed {
        cx.set_global(OpenedThread(None));
    }
}

/// Inline danger bar (ui-spec §4.6: errors are inline, never modals).
pub(crate) fn error_bar(message: String, colors: &ThemeColors) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(colors.bg_elevated)
        .border_1()
        .border_color(colors.danger)
        .text_color(colors.danger)
        .text_size(px(Typography::SIDEBAR))
        .child(message)
        .into_any_element()
}

/// Row/header label for a thread: 「未命名任务」 until T13 adds renaming.
pub(crate) fn thread_title(thread: &Thread) -> String {
    if thread.title.is_empty() {
        "未命名任务".to_string()
    } else {
        thread.title.clone()
    }
}

/// Outcome of a rename submission (pure decision — the Enter/Esc key path
/// itself is manual-acceptance because synthetic keyboard events cannot
/// reach GPUI in this environment; this decision must stay unit-tested).
pub(crate) enum RenameResolution {
    /// 空标题（含纯空白）提交视为取消：退出编辑态，不写库。
    Cancel,
    /// 提交去首尾空白后的新标题。
    Commit(String),
}

/// Classifies a rename submission: blank input cancels, anything else
/// commits trimmed.
pub(crate) fn resolve_rename(raw: &str) -> RenameResolution {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        RenameResolution::Cancel
    } else {
        RenameResolution::Commit(trimmed.to_string())
    }
}

/// Whether a session row currently shows its hover action group (裁决①)：
/// the group appears only while the row is hovered and never while that row
/// is in inline-rename editing (避免编辑态与行操作叠加).
pub(crate) fn row_shows_actions(row_hovered: bool, row_editing: bool) -> bool {
    row_hovered && !row_editing
}

/// 「已归档 (N)」折叠区只在确有归档线程时出现；归档计数即折叠区标题里的 N.
pub(crate) fn archive_section_visible(archived_count: usize) -> bool {
    archived_count > 0
}

/// Relative time for a session row (ui-spec §4.1, "2h" style).
pub(crate) fn relative_time(updated_at_ms: i64) -> String {
    relative_time_from(updated_at_ms, now_ms())
}

/// Pure core of [`relative_time`]: `<1m` → "now", `<1h` → `{n}m`, `<24h` →
/// `{n}h`, `<7d` → `{n}d`, otherwise the UTC date `YYYY-MM-DD` (plain
/// civil-from-days conversion, no external date crate). Future timestamps
/// (clock skew) read "now".
pub(crate) fn relative_time_from(updated_at_ms: i64, now_ms: i64) -> String {
    let elapsed_seconds = (now_ms - updated_at_ms).div_euclid(1000);
    if elapsed_seconds < 60 {
        return "now".to_string();
    }
    if elapsed_seconds < 3600 {
        return format!("{}m", elapsed_seconds / 60);
    }
    if elapsed_seconds < 86_400 {
        return format!("{}h", elapsed_seconds / 3600);
    }
    if elapsed_seconds < 7 * 86_400 {
        return format!("{}d", elapsed_seconds / 86_400);
    }
    let (year, month, day) = civil_from_days(updated_at_ms.div_euclid(86_400_000));
    format!("{year:04}-{month:02}-{day:02}")
}

/// Unix-milliseconds timestamp for the relative-time baseline.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// Days-since-epoch → `(year, month, day)` (Howard Hinnant's
/// civil_from_days; no external date crate needed).
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (era * 400 + yoe + i64::from(month <= 2), month, day)
}
