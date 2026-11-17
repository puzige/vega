use super::{
    CloseDiff, DIFF_CHANGE_BACKGROUND_OPACITY, DIFF_ROW_HEIGHT, DiffLayout, DiffRowKind, DiffView,
    GitWorkspaceErrorCode, PreparedLine, PreparedRow, WorkspaceFileId, count_label, error_label,
    exact_current_file, head_label,
};
use crate::conversation_stream::{MONOFONT, code_token_style};
use crate::icons::Icon;
use crate::nsh::{OrderedCommands, ProjectionId, Surface};
use gpui_kit::Subscription;
use gpui_kit::{App, Context, Render, Window, div};
use nsh_core::prelude::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use vega_theme::{Appearance, Layout, ThemeColors, Typography};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    recovery: Option<Surface<Snapshot, Request>>,
    id: ProjectionId,
    focus_revision: u64,
    _focus: Subscription,
}

#[derive(Clone)]
struct Snapshot {
    surface_id: ProjectionId,
    version: ProjectionId,
    generation: Option<u64>,
    rows: Arc<Vec<PreparedRow>>,
    row_revision: u64,
    scroll_revision: u64,
    scroll_target: Option<usize>,
    focus_revision: u64,
    head: String,
    counts: String,
    has_snapshot: bool,
    has_hunks: bool,
    layout: DiffLayout,
    refreshing: bool,
    show_refresh_progress: bool,
    refresh_error: Option<GitWorkspaceErrorCode>,
    colors: ThemeColors,
    appearance: Appearance,
    language: &'static str,
}

#[derive(Clone)]
struct Observed {
    surface_id: ProjectionId,
    version: ProjectionId,
    generation: Option<u64>,
}

impl Snapshot {
    fn observed(&self) -> Observed {
        Observed {
            surface_id: self.surface_id.clone(),
            version: self.version.clone(),
            generation: self.generation,
        }
    }
}

#[derive(Clone, Copy)]
enum Action {
    ToggleFile(WorkspaceFileId),
    RetryProjection(WorkspaceFileId),
    Refresh,
    Layout,
    Previous,
    Next,
    Close,
}

enum Request {
    Action(Observed, Action),
    Reload(ProjectionId),
}

impl DiffView {
    fn native_snapshot(&self, id: ProjectionId, cx: &App) -> Snapshot {
        let (head, counts) = self.snapshot.as_ref().map_or_else(
            || (String::new(), String::new()),
            |snapshot| {
                (
                    head_label(&snapshot.head),
                    vega_i18n::t_args(
                        "ui.diff_file_counts",
                        &[
                            ("count", &snapshot.stats.file_count.to_string()),
                            ("additions", &count_label(snapshot.stats.additions)),
                            ("deletions", &count_label(snapshot.stats.deletions)),
                        ],
                    ),
                )
            },
        );
        Snapshot {
            surface_id: id,
            version: self.presentation_version.clone(),
            generation: self.generation(),
            rows: self.rows.clone(),
            row_revision: self.row_revision,
            scroll_revision: self.scroll_revision,
            scroll_target: self.scroll_target,
            focus_revision: self
                .presentation
                .as_ref()
                .map_or(0, |page| page.focus_revision),
            head,
            counts,
            has_snapshot: self.snapshot.is_some(),
            has_hunks: !self.hunk_indexes.is_empty(),
            layout: self.layout,
            refreshing: self.refreshing,
            show_refresh_progress: self.show_refresh_progress,
            refresh_error: self.refresh_error,
            colors: vega_theme::theme(cx).colors,
            appearance: vega_theme::theme(cx).appearance,
            language: vega_i18n::language(),
        }
    }

    fn apply_native_request(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match request {
            Request::Reload(id) => {
                if self
                    .presentation
                    .as_ref()
                    .is_some_and(|page| page.id.matches(&id))
                {
                    self.presentation = None;
                    cx.notify();
                }
            }
            Request::Action(observed, action) => {
                if self
                    .presentation
                    .as_ref()
                    .is_none_or(|page| !page.id.matches(&observed.surface_id))
                    || !self.presentation_version.matches(&observed.version)
                    || self.generation() != observed.generation
                {
                    return;
                }
                match action {
                    Action::ToggleFile(id) => self.toggle_file(id, cx),
                    Action::RetryProjection(id) => {
                        if self.snapshot.as_ref().is_some_and(|snapshot| {
                            exact_current_file(
                                self.expanded_file,
                                snapshot.files.iter().map(|file| file.id),
                                id,
                            )
                        }) {
                            self.retry_projection(id, cx);
                        }
                    }
                    Action::Refresh => self.retry(cx),
                    Action::Layout => self.toggle_layout(cx),
                    Action::Previous => {
                        self.previous_hunk(cx);
                    }
                    Action::Next => {
                        self.next_hunk(cx);
                    }
                    Action::Close => self.close_action(&CloseDiff, window, cx),
                }
            }
        }
    }

    fn render_native(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{
            InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _,
        };
        if self.rendered_language != vega_i18n::language() {
            self.rendered_language = vega_i18n::language();
            self.rebuild_rows();
        }
        if self.presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new(self.native_snapshot(id.clone(), cx), render, window, cx);
            let focus = cx.on_focus(&self.focus.clone(), window, |this, window, cx| {
                if let Some(page) = this.presentation.as_mut() {
                    page.focus_revision = page.focus_revision.wrapping_add(1);
                    page.recovery
                        .as_ref()
                        .unwrap_or(&page.surface)
                        .focus(window, cx);
                    let id = page.id.clone();
                    let snapshot = this.native_snapshot(id, cx);
                    if let Some(page) = this.presentation.as_ref() {
                        page.recovery
                            .as_ref()
                            .unwrap_or(&page.surface)
                            .publish(snapshot);
                    }
                }
            });
            let mut errors = surface.errors();
            self.presentation = Some(Presentation {
                surface,
                recovery: None,
                id,
                focus_revision: 0,
                _focus: focus,
            });
            cx.spawn(async move |this, cx| {
                while errors.changed().await.is_ok() {
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
            .detach();
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.apply_native_request(request, window, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = self.presentation.as_ref() else {
            return div().into_any_element();
        };
        let snapshot = self.native_snapshot(page.id.clone(), cx);
        if page.surface.errors().borrow().is_some() && page.recovery.is_none() {
            let (recovery, mut requests) =
                Surface::new(snapshot.clone(), render_recovery, window, cx);
            if let Some(page) = self.presentation.as_mut() {
                page.recovery = Some(recovery);
            }
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            if matches!(request, Request::Reload(_)) {
                                this.apply_native_request(request, window, cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let focused = self.focus.is_focused(window);
        let Some(page) = self.presentation.as_mut() else {
            return div().into_any_element();
        };
        let mut snapshot = snapshot;
        if focused {
            page.focus_revision = page.focus_revision.wrapping_add(1);
            snapshot.focus_revision = page.focus_revision;
        }
        let surface = page.recovery.as_mut().unwrap_or(&mut page.surface);
        surface.sync_environment(cx);
        surface.publish(snapshot);
        if focused {
            surface.focus(window, cx);
        }
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .track_focus(&self.focus)
            .key_context("DiffView")
            .on_action(cx.listener(Self::close_action))
            .on_action(cx.listener(Self::previous_action))
            .on_action(cx.listener(Self::next_action))
            .on_action(cx.listener(Self::toggle_layout_action))
            .child(surface.entity())
            .into_any_element()
    }
}

impl Render for DiffView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        self.render_native(window, cx)
    }
}

fn send(state: Signal<Snapshot>, commands: &OrderedCommands<Request>, action: Action) {
    commands.send(Request::Action(state.get_untracked().observed(), action));
}

fn control(state: Signal<Snapshot>, target: ElementRef, outlined: bool) -> View {
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .h(px(DIFF_ROW_HEIGHT))
        .px_2()
        .items_center()
        .justify_center()
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border(px(1.))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                state.with(|s| s.colors.brand_primary)
            } else if outlined {
                state.with(|s| s.colors.border_subtle)
            } else {
                state.with(|s| s.colors.bg_base.opacity(0.))
            })
        })
        .text_size(px(Typography::METADATA))
        .hover(move |s| s.bg(crate::nsh::color(state.with(|s| s.colors.bg_hover))))
}

fn localized(state: Signal<Snapshot>, key: &'static str) -> &'static str {
    let _ = state.with(|snapshot| snapshot.language);
    vega_i18n::t(key)
}

fn button(
    state: Signal<Snapshot>,
    commands: OrderedCommands<Request>,
    key: &'static str,
    action: Action,
) -> View {
    control(state, element_ref(), true)
        .label(move || localized(state, key))
        .child(text(move || localized(state, key)).number_of_lines(1))
        .on_press(move || send(state, &commands, action))
}

fn glyph(icon: Icon, ink: impl Fn() -> gpui_kit::Rgba + Copy + 'static) -> AnyElement {
    svg(nsh_core::element::ImageSrc::svg_markup(
        &crate::icons::nsh_source(icon).unwrap_or_default(),
    ))
    .size(px(Layout::ICON_SIZE))
    .flex_shrink_0()
    .accessible(false)
    .text_color(move || crate::nsh::color(ink()))
    .tint_color(move || crate::nsh::color(ink()).to_hsla())
    .into_element()
}

fn header_button(
    state: Signal<Snapshot>,
    commands: OrderedCommands<Request>,
    icon: Icon,
    key: fn(&Snapshot) -> &'static str,
    action: Action,
) -> View {
    let label = move || state.with(|snapshot| vega_i18n::t(key(snapshot)));
    control(state, element_ref(), false)
        .size(px(DIFF_ROW_HEIGHT))
        .px(px(0.))
        .flex_shrink_0()
        .label(label)
        .tooltip(text(label))
        .child(glyph(icon, move || state.with(|s| s.colors.text_secondary)))
        .on_press(move || send(state, &commands, action))
}

fn row_base() -> View {
    view()
        .h(px(DIFF_ROW_HEIGHT))
        .min_h(px(DIFF_ROW_HEIGHT))
        .max_h(px(DIFF_ROW_HEIGHT))
        .w_full()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .overflow_hidden()
}

#[derive(Clone, Copy)]
enum LineSide {
    Unified,
    Old,
    New,
}

fn code_line(line: Option<&PreparedLine>, colors: &ThemeColors, side: LineSide) -> View {
    let half = !matches!(side, LineSide::Unified);
    let mut base = row_base()
        .font_family(MONOFONT)
        .text_size(px(Typography::CODE));
    if half {
        base = base
            .flex_1()
            .min_w(px(0.))
            .border_r(px(1.))
            .border_color(crate::nsh::color(colors.border_subtle));
    }
    let Some(line) = line else {
        return base;
    };
    let number = match side {
        LineSide::Unified => format!(
            "{:>4} {:>4}",
            line.old_line.map_or(String::new(), |n| n.to_string()),
            line.new_line.map_or(String::new(), |n| n.to_string())
        ),
        LineSide::Old => line.old_line.map_or(String::new(), |n| n.to_string()),
        LineSide::New => line.new_line.map_or(String::new(), |n| n.to_string()),
    };
    let (marker, marker_color, background) = match line.kind {
        DiffRowKind::Addition => ("+", colors.success, Some(colors.success)),
        DiffRowKind::Deletion => ("-", colors.danger, Some(colors.danger)),
        DiffRowKind::Context => (" ", colors.text_tertiary, None),
    };
    if let Some(background) = background {
        base = base.bg(crate::nsh::color(
            background.opacity(DIFF_CHANGE_BACKGROUND_OPACITY),
        ));
    }
    let mut code = text("")
        .number_of_lines(1)
        .text_overflow(nsh_core::EllipsizeMode::Clip);
    for span in &line.spans {
        let mut part = text(span.text.clone());
        if let Some(kind) = span.kind {
            let style = code_token_style(kind, colors);
            part = part
                .text_color(crate::nsh::color(style.color))
                .font_weight(nsh_core::element::FontWeight(style.weight.0 as u16));
            if style.italic {
                part = part.italic();
            }
        }
        code = code.child(part);
    }
    base.child(
        text(number)
            .w(px(DIFF_ROW_HEIGHT * if half { 2. } else { 4. }))
            .flex_shrink_0()
            .pr_2()
            .number_of_lines(1)
            .text_color(crate::nsh::color(colors.text_tertiary)),
    )
    .child(
        text(marker)
            .w(px(DIFF_ROW_HEIGHT))
            .flex_shrink_0()
            .text_color(crate::nsh::color(marker_color)),
    )
    .child(view().flex_1().min_w(px(0.)).overflow_hidden().child(code))
}

fn prepared_row(
    row: &PreparedRow,
    snapshot: &Snapshot,
    state: Signal<Snapshot>,
    commands: OrderedCommands<Request>,
) -> AnyElement {
    let colors = &snapshot.colors;
    let observed = snapshot.observed();
    match row {
        PreparedRow::File {
            id,
            label,
            summary,
            expanded,
        } => {
            let id = *id;
            let focus = element_ref();
            let chevron = if *expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            let ink = colors.text_tertiary;
            control(state, focus, false)
                .rounded(px(0.))
                .h(px(DIFF_ROW_HEIGHT))
                .min_h(px(DIFF_ROW_HEIGHT))
                .max_h(px(DIFF_ROW_HEIGHT))
                .w_full()
                .flex_row()
                .gap_2()
                .px_3()
                .bg(crate::nsh::color(if *expanded {
                    colors.bg_active
                } else {
                    colors.bg_base
                }))
                .label(label.clone())
                .accessibility_state(AccessibilityState {
                    expanded: Some(*expanded),
                    ..Default::default()
                })
                .disabled(move || state.with(|s| s.refresh_error.is_some()))
                .child(glyph(chevron, move || ink))
                .child(
                    text(label.clone())
                        .flex_1()
                        .min_w(px(0.))
                        .number_of_lines(1),
                )
                .child(
                    text(summary.replace(' ', "\u{a0}"))
                        .label(summary.clone())
                        .max_w(px(110.))
                        .flex_shrink_0()
                        .truncate()
                        .font_family(MONOFONT)
                        .text_size(px(Typography::CODE))
                        .text_color(crate::nsh::color(colors.text_secondary)),
                )
                .on_press(move || {
                    commands.send(Request::Action(observed.clone(), Action::ToggleFile(id)))
                })
                .into_element()
        }
        PreparedRow::Section { label } => row_base()
            .px_3()
            .bg(crate::nsh::color(colors.bg_elevated))
            .text_color(crate::nsh::color(colors.text_secondary))
            .font_weight(nsh_core::element::FontWeight(
                Typography::HEADING_CARD_WEIGHT.0 as u16,
            ))
            .child(text(vega_i18n::t(label)))
            .into_element(),
        PreparedRow::Hunk { label } => row_base()
            .px_3()
            .bg(crate::nsh::color(colors.code_bg))
            .font_family(MONOFONT)
            .text_size(px(Typography::CODE))
            .text_color(crate::nsh::color(colors.text_secondary))
            .child(text(label.clone()).number_of_lines(1))
            .into_element(),
        PreparedRow::Unified(line) => {
            code_line(Some(line), colors, LineSide::Unified).into_element()
        }
        PreparedRow::SideBySide(pair) => row_base()
            .child(code_line(pair.left.as_ref(), colors, LineSide::Old))
            .child(code_line(pair.right.as_ref(), colors, LineSide::New))
            .into_element(),
        PreparedRow::ProjectionError { id, code } => {
            let id = *id;
            row_base()
                .px_3()
                .gap_2()
                .text_color(crate::nsh::color(colors.danger))
                .child(
                    text(error_label(*code))
                        .flex_1()
                        .min_w(px(0.))
                        .number_of_lines(1),
                )
                .child(
                    control(state, element_ref(), true)
                        .label(vega_i18n::t("ui.retry"))
                        .child(text(vega_i18n::t("ui.retry")))
                        .on_press(move || {
                            commands.send(Request::Action(
                                observed.clone(),
                                Action::RetryProjection(id),
                            ))
                        }),
                )
                .into_element()
        }
        PreparedRow::Message { key, danger } => row_base()
            .px_3()
            .text_color(crate::nsh::color(if *danger {
                colors.danger
            } else {
                colors.text_tertiary
            }))
            .child(text(vega_i18n::t(key)).number_of_lines(1))
            .into_element(),
    }
}

fn focus_forwarding(state: Signal<Snapshot>, target: ElementRef) -> impl Fn() + Copy {
    let applied = signal(None::<u64>);
    let revision = computed(move || state.with(|s| s.focus_revision));
    let focus = move || {
        let revision = revision.get();
        if revision > 0 && applied.get_untracked() != Some(revision) && target.node().is_some() {
            applied.set(Some(revision));
            target.focus();
        }
    };
    effect(focus);
    focus
}

fn render_recovery(state: Signal<Snapshot>, sender: mpsc::Sender<Request>) -> AnyElement {
    let commands = OrderedCommands::new(sender);
    let target = element_ref();
    let focus = focus_forwarding(state, target);
    view()
        .size_full()
        .p_3()
        .gap_2()
        .on_layout(move |_: &Bounds<Points>| focus())
        .bg(move || crate::nsh::color(state.with(|s| s.colors.bg_base)))
        .text_color(move || crate::nsh::color(state.with(|s| s.colors.danger)))
        .child(text(move || {
            let _ = state.with(|s| s.language);
            vega_i18n::t("ui.git_diff_could_not_be_loaded_safely")
        }))
        .child(
            control(state, target, true)
                .label(move || localized(state, "settings.action.reload"))
                .child(text(move || localized(state, "settings.action.reload")))
                .on_press(move || {
                    commands.send(Request::Reload(
                        state.with_untracked(|s| s.surface_id.clone()),
                    ))
                }),
        )
        .into_element()
}

fn render(state: Signal<Snapshot>, sender: mpsc::Sender<Request>) -> AnyElement {
    let commands = OrderedCommands::new(sender);
    let root = element_ref();
    let list = element_ref();
    let focus = focus_forwarding(state, root);
    for (key, action) in [
        ("escape", Action::Close),
        ("[", Action::Previous),
        ("]", Action::Next),
    ] {
        let commands = commands.clone();
        on_key(key, move |_| send(state, &commands, action));
    }
    let viewport = signal(ScrollEvent::default());
    let applied_scroll = signal(None::<u64>);
    let scroll_revision = computed(move || state.with(|s| s.scroll_revision));
    let scroll = move || {
        let revision = scroll_revision.get();
        if applied_scroll.get_untracked() == Some(revision) || list.node().is_none() {
            return;
        }
        let Some(row) = state.with_untracked(|s| s.scroll_target) else {
            return;
        };
        let viewport = viewport.get_untracked();
        let top = row as f32 * DIFF_ROW_HEIGHT;
        let offset = viewport.offset.y.0;
        let height = viewport.size.height.0;
        if height <= 0. {
            return;
        }
        let target = if top < offset {
            top
        } else if top + DIFF_ROW_HEIGHT > offset + height {
            top + DIFF_ROW_HEIGHT - height
        } else {
            offset
        };
        list.scroll_to(point(viewport.offset.x, px(target.max(0.))), false);
        applied_scroll.set(Some(revision));
    };
    effect(scroll);
    watch_scroll(
        move || list.node(),
        move |event| {
            viewport.set(*event);
            scroll();
        },
    );
    let row_revision = computed(move || state.with(|s| s.row_revision));
    let row_version = computed(move || state.with(|s| (s.row_revision, s.appearance, s.language)));
    let row_commands = commands.clone();
    let native_list = virtual_list(
        move || state.with(|s| s.rows.len()),
        move |at| {
            let commands = row_commands.clone();
            dynamic(move || {
                let _ = row_version.get();
                let index = at.index();
                state.with_untracked(|snapshot| {
                    snapshot
                        .rows
                        .get(index)
                        .map(|row| prepared_row(row, snapshot, state, commands.clone()))
                })
            })
        },
    )
    .keys(move || {
        let revision = row_revision.get();
        let count = state.with_untracked(|s| s.rows.len());
        (0..count).map(move |index| (revision, index))
    })
    .estimated_item_size(px(DIFF_ROW_HEIGHT))
    .element_ref(&list)
    .flex_1()
    .min_h(px(0.))
    .w_full()
    .on_scroll(move |event: &ScrollEvent| viewport.set(*event))
    .on_layout(move |bounds: &Bounds<Points>| {
        viewport.update(|v| v.size = bounds.size);
        scroll();
    });
    let failed_commands = commands.failed;
    let reload_commands = commands.clone();
    let retry_commands = commands.clone();
    let body_empty = computed(move || state.with(|s| s.rows.is_empty()));
    view()
        .element_ref(&root)
        .focusable(true)
        .size_full()
        .min_w(px(0.))
        .min_h(px(0.))
        .on_layout(move |_: &Bounds<Points>| focus())
        .bg(move || crate::nsh::color(state.with(|s| s.colors.bg_base)))
        .text_color(move || crate::nsh::color(state.with(|s| s.colors.text_primary)))
        .text_size(px(Typography::BODY))
        .child(
            view()
                .min_h(px(DIFF_ROW_HEIGHT * 2.))
                .flex_shrink_0()
                .flex_row()
                .flex_wrap()
                .content_center()
                .items_center()
                .gap_1()
                .px_2()
                .border_b(px(1.))
                .border_color(move || crate::nsh::color(state.with(|s| s.colors.border_subtle)))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.))
                        .child(
                            text(move || state.with(|s| s.head.clone()))
                                .number_of_lines(1)
                                .font_weight(nsh_core::element::FontWeight(
                                    Typography::HEADING_CARD_WEIGHT.0 as u16,
                                )),
                        )
                        .child(
                            text(move || state.with(|s| s.counts.clone()))
                                .number_of_lines(1)
                                .text_size(px(Typography::METADATA))
                                .text_color(move || {
                                    crate::nsh::color(state.with(|s| s.colors.text_secondary))
                                }),
                        ),
                )
                .child(
                    header_button(
                        state,
                        commands.clone(),
                        Icon::Refresh,
                        |_| "ui.refresh_workspace_changes",
                        Action::Refresh,
                    )
                    .disabled(move || state.with(|s| s.refreshing)),
                )
                .child(header_button(
                    state,
                    commands.clone(),
                    Icon::Split,
                    |s| match s.layout {
                        DiffLayout::Unified => "ui.unified",
                        DiffLayout::SideBySide => "ui.side_by_side",
                    },
                    Action::Layout,
                ))
                .child(
                    header_button(
                        state,
                        commands.clone(),
                        Icon::ArrowUp,
                        |_| "ui.previous_change",
                        Action::Previous,
                    )
                    .disabled(move || !state.with(|s| s.has_hunks)),
                )
                .child(
                    header_button(
                        state,
                        commands.clone(),
                        Icon::ArrowDown,
                        |_| "ui.next_change",
                        Action::Next,
                    )
                    .disabled(move || !state.with(|s| s.has_hunks)),
                ),
        )
        .child(when(
            move || state.with(|s| s.refresh_error.is_some()),
            move || {
                view()
                    .flex_shrink_0()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .child(
                        text(move || {
                            state.with(|s| s.refresh_error.map(error_label).unwrap_or_default())
                        })
                        .flex_1()
                        .min_w(px(0.))
                        .text_color(move || crate::nsh::color(state.with(|s| s.colors.danger))),
                    )
                    .child(
                        button(state, retry_commands.clone(), "ui.retry", Action::Refresh)
                            .disabled(move || state.with(|s| s.refreshing)),
                    )
            },
        ))
        .child(when(
            move || state.with(|s| s.refreshing && s.show_refresh_progress),
            move || {
                text(move || {
                    state.with(|s| {
                        vega_i18n::t(if s.refresh_error.is_some() {
                            "ui.retrying_workspace_diff"
                        } else {
                            "ui.loading_workspace_diff"
                        })
                    })
                })
                .px_3()
                .py_2()
                .text_color(move || crate::nsh::color(state.with(|s| s.colors.text_secondary)))
            },
        ))
        .child(when(
            move || failed_commands.get(),
            move || {
                view()
                    .flex_row()
                    .gap_2()
                    .p_2()
                    .child(
                        text(move || localized(state, "ui.git_diff_could_not_be_loaded_safely"))
                            .text_color(move || crate::nsh::color(state.with(|s| s.colors.danger))),
                    )
                    .child(
                        control(state, element_ref(), true)
                            .label(move || localized(state, "settings.action.reload"))
                            .child(text(move || localized(state, "settings.action.reload")))
                            .on_press({
                                let commands = reload_commands.clone();
                                move || {
                                    commands.send(Request::Reload(
                                        state.with_untracked(|s| s.surface_id.clone()),
                                    ))
                                }
                            }),
                    )
            },
        ))
        .child(when(
            move || {
                body_empty.get()
                    && state.with(|s| {
                        s.refresh_error.is_none() && !(s.refreshing && s.show_refresh_progress)
                    })
            },
            move || {
                view()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .text_color(move || crate::nsh::color(state.with(|s| s.colors.text_tertiary)))
                    .child(text(move || localized(state, "ui.no_workspace_changes")))
                    .child(when(
                        move || !state.with(|s| s.has_snapshot),
                        move || {
                            text(move || {
                                localized(
                                    state,
                                    "ui.changes_will_appear_here_after_you_modify_files",
                                )
                            })
                            .text_size(px(Typography::METADATA))
                        },
                    ))
            },
        ))
        .child(when(move || !body_empty.get(), move || native_list.clone()))
        .into_element()
}
