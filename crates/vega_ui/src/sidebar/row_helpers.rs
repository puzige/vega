use super::*;

pub fn render_delete_confirm_overlay(
    thread: &Thread,
    sidebar: Entity<Sidebar>,
    _colors: ThemeColors,
) -> AnyElement {
    let observed = thread.clone();
    let thread_id = thread.id.clone();
    let mut delete =
        nsh_leaf::Item::action(vega_i18n::t("ui.delete"), Some(crate::icons::Icon::Trash));
    delete.danger = true;
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .child(nsh_leaf::chrome::dimmer())
        .child(
            div()
                .w(px(320.))
                .on_mouse_down_out(|_, _, cx| {
                    cx.set_global(PendingDeleteConfirm(None));
                    cx.refresh_windows();
                })
                .child(nsh_leaf::host(
                    format!("sidebar-delete-confirm-{thread_id}"),
                    Some(format!(
                        "{}\n{}",
                        vega_i18n::t("ui.delete_conversation"),
                        vega_i18n::t_args(
                            "ui.delete_title_all_messages_and_tool_call_records",
                            &[("title", &thread_title(thread))],
                        )
                    )),
                    vec![
                        nsh_leaf::Item::action(vega_i18n::t("ui.cancel"), None),
                        delete,
                    ],
                    true,
                    Typography::SIDEBAR_LINE_HEIGHT * 6.,
                    move |cx| {
                        cx.try_global::<PendingDeleteConfirm>()
                            .and_then(|value| value.0.as_ref())
                            == Some(&observed)
                    },
                    move |index, _, _, cx| {
                        if index == 0 {
                            cx.set_global(PendingDeleteConfirm(None));
                            cx.refresh_windows();
                        } else if cx
                            .global::<PendingDeleteConfirm>()
                            .0
                            .as_ref()
                            .is_some_and(|thread| thread.id == thread_id)
                        {
                            sidebar.update(cx, Sidebar::confirm_pending_delete);
                        }
                    },
                )),
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
pub(crate) fn error_bar(
    message: crate::localized_copy::LocalizedCopy,
    colors: &ThemeColors,
) -> AnyElement {
    let _ = colors;
    let text = message.render();
    let mut item = nsh_leaf::Item::action(text.clone(), None);
    item.danger = true;
    item.enabled = false;
    item.wrap = true;
    nsh_leaf::host(
        format!("sidebar-error-{text}"),
        None,
        vec![item],
        false,
        Typography::SIDEBAR_LINE_HEIGHT * 3.,
        |_| true,
        |_, _, _, _| {},
    )
}

pub(crate) fn thread_title(thread: &Thread) -> String {
    if thread.title.is_empty() {
        vega_i18n::t("ui.untitled_task").to_string()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionRowTrailing {
    Actions,
    Running,
    Unread,
    Timestamp,
    Empty,
}

pub(crate) fn session_row_trailing(
    actions_visible: bool,
    running: bool,
    unread: bool,
    show_timestamp_at_rest: bool,
) -> SessionRowTrailing {
    if actions_visible {
        SessionRowTrailing::Actions
    } else if running {
        SessionRowTrailing::Running
    } else if unread {
        SessionRowTrailing::Unread
    } else if show_timestamp_at_rest {
        SessionRowTrailing::Timestamp
    } else {
        SessionRowTrailing::Empty
    }
}

/// Relative time for a session row (ui-spec §4.1, "2h" style).
pub(crate) fn relative_time(
    updated_at_ms: i64,
    next_change: Option<&std::cell::Cell<i64>>,
) -> String {
    let now = now_ms();
    if let Some(deadline) = next_change
        && let Some((unit, _)) = relative_time_unit(updated_at_ms, now)
    {
        let elapsed = now.saturating_sub(updated_at_ms).max(0);
        let next = updated_at_ms.saturating_add((elapsed / unit + 1).saturating_mul(unit));
        deadline.set(deadline.get().min(next));
    }
    relative_time_from(updated_at_ms, now)
}

fn relative_time_unit(updated: i64, now: i64) -> Option<(i64, &'static str)> {
    match now.saturating_sub(updated) {
        ..3_600_000 => Some((60_000, "m")),
        3_600_000..86_400_000 => Some((3_600_000, "h")),
        86_400_000..604_800_000 => Some((86_400_000, "d")),
        _ => None,
    }
}

pub(crate) fn relative_time_from(updated_at_ms: i64, now_ms: i64) -> String {
    if let Some((unit, suffix)) = relative_time_unit(updated_at_ms, now_ms) {
        let count = now_ms.saturating_sub(updated_at_ms) / unit;
        return if count < 1 {
            "now".to_owned()
        } else {
            format!("{count}{suffix}")
        };
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
