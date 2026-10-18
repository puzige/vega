//! IO-free search panel; workers and route authority remain in the application.
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};
use crate::text_input::TextInput;
use gpui_kit::{prelude::*, *};
use vega_conversation::types::*;
use vega_theme::{Layout, Typography, theme};

actions!(
    vega_palette,
    [
        OpenPalette,
        OpenWorkspacePicker,
        ToggleWorkspaceTerminal,
        PaletteNext,
        PalettePrevious,
        PaletteAccept,
        PaletteDismiss,
        PaletteNextScope
    ]
);
/// Query request emitted after committed input changes.
pub struct PaletteQueryChanged(pub String);
/// Activation of an actual projected result.
pub struct PaletteActivated(pub PaletteTarget);
/// Explicit close without action.
pub struct PaletteClosed;
impl EventEmitter<PaletteQueryChanged> for CommandPalette {}
impl EventEmitter<PaletteActivated> for CommandPalette {}
impl EventEmitter<PaletteClosed> for CommandPalette {}
/// Centered command/search surface; retains its own IME-capable input.
pub struct CommandPalette {
    input: Entity<TextInput>,
    query: String,
    scope: PaletteScope,
    actions: Vec<PaletteAction>,
    results: PaletteSearch,
    selected: usize,
    loading: bool,
    error: Option<PaletteError>,
    scroll: ScrollHandle,
}
impl CommandPalette {
    /// Construct with currently executable actions.
    pub fn new(actions: Vec<PaletteAction>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new(cx, "搜索操作、任务或文件", false));
        cx.observe(&input, |this, input, cx| {
            if input.read(cx).is_composing() {
                return;
            }
            let query: String = input.read(cx).text().chars().take(256).collect();
            if this.query != query {
                this.query = query.clone();
                this.results = PaletteSearch::default();
                this.loading = true;
                this.selected = 0;
                this.scroll.scroll_to_item(0);
                cx.emit(PaletteQueryChanged(query));
                cx.notify();
            }
        })
        .detach();
        Self {
            input,
            query: String::new(),
            scope: PaletteScope::All,
            actions,
            results: PaletteSearch::default(),
            selected: 0,
            loading: true,
            error: None,
            scroll: ScrollHandle::new(),
        }
    }
    /// Apply a bounded worker result; stale authority is rejected by the app first.
    pub fn apply(&mut self, result: Result<PaletteSearch, PaletteError>, cx: &mut Context<Self>) {
        self.loading = false;
        self.error = result.as_ref().err().copied();
        if let Ok(results) = result {
            self.results = results;
        }
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }
    /// Mark a selected result as loading without discarding retryable results.
    pub fn begin_activation(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();
    }

    /// Current bounded text for an initial/retry search.
    pub fn query(&self) -> &str {
        &self.query
    }
    fn entries(&self) -> Vec<(String, String, PaletteTarget)> {
        let mut result = Vec::new();
        if matches!(self.scope, PaletteScope::All | PaletteScope::Actions) {
            let needle = self.query.to_lowercase();
            for action in &self.actions {
                let (label, shortcut, aliases) = action_label(*action);
                if needle.is_empty()
                    || format!("{label} {aliases}")
                        .to_lowercase()
                        .contains(&needle)
                {
                    result.push((
                        label.into(),
                        shortcut.into(),
                        PaletteTarget::Action(*action),
                    ));
                }
            }
        }
        if matches!(self.scope, PaletteScope::All | PaletteScope::Tasks) {
            result.extend(self.results.tasks.iter().take(30).map(|task| {
                (
                    if task.title.is_empty() {
                        "未命名任务".into()
                    } else {
                        task.title.clone()
                    },
                    task.project_name.clone(),
                    PaletteTarget::Task(task.clone()),
                )
            }));
        }
        if matches!(self.scope, PaletteScope::All | PaletteScope::Files) {
            result.extend(self.results.files.iter().take(30).map(|path| {
                (
                    path.clone(),
                    "文件".into(),
                    PaletteTarget::File(path.clone()),
                )
            }));
        }
        result
    }
    fn accept(&mut self, _: &PaletteAccept, _: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() {
            return;
        }
        if let Some((_, _, target)) = self.entries().get(self.selected) {
            cx.emit(PaletteActivated(target.clone()));
        }
    }
    fn dismiss(&mut self, _: &PaletteDismiss, _: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() {
            return;
        }
        cx.emit(PaletteClosed);
    }
    fn next(&mut self, _: &PaletteNext, _: &mut Window, cx: &mut Context<Self>) {
        if !self.input.read(cx).is_composing() {
            self.selected = (self.selected + 1).min(self.entries().len().saturating_sub(1));
            self.scroll.scroll_to_item(self.selected);
            cx.notify();
        } else {
            cx.propagate();
        }
    }
    fn previous(&mut self, _: &PalettePrevious, _: &mut Window, cx: &mut Context<Self>) {
        if !self.input.read(cx).is_composing() {
            self.selected = self.selected.saturating_sub(1);
            self.scroll.scroll_to_item(self.selected);
            cx.notify();
        } else {
            cx.propagate();
        }
    }
    fn scope_next(&mut self, _: &PaletteNextScope, _: &mut Window, cx: &mut Context<Self>) {
        self.scope = match self.scope {
            PaletteScope::All => PaletteScope::Actions,
            PaletteScope::Actions => PaletteScope::Tasks,
            PaletteScope::Tasks => PaletteScope::Files,
            PaletteScope::Files => PaletteScope::All,
        };
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }
}
/// Labels and search aliases for supported real application operations.
pub fn action_label(action: PaletteAction) -> (&'static str, &'static str, &'static str) {
    match action {
        PaletteAction::NewTask => ("新建任务", "⌘N", "new task"),
        PaletteAction::OpenWorkspace => ("打开工作区", "⌘O", "open workspace project"),
        PaletteAction::Settings => ("打开设置", "⌘,", "settings"),
        PaletteAction::ToggleSidebar => ("切换侧栏", "⌘B", "sidebar"),
        PaletteAction::Terminal => ("切换终端", "⌘J", "terminal"),
        PaletteAction::Preview => ("打开预览", "", "preview"),
        PaletteAction::Review => ("打开 Review", "", "review diff"),
    }
}
impl Focusable for CommandPalette {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}
impl Render for CommandPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let entries = self.entries();
        let selected = self.selected;
        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(Layout::COMMAND_PALETTE_TOP_OFFSET))
            .bg(colors.bg_sidebar.opacity(0.85))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(PaletteClosed)),
            )
            .child(
                div()
                    .id("command-palette")
                    .key_context("CommandPalette")
                    .w(px(Layout::COMMAND_PALETTE_WIDTH).min(
                        window.viewport_size().width - px(Layout::COMMAND_PALETTE_SIDE_INSET * 2.0),
                    ))
                    .max_h(
                        px(Layout::COMMAND_PALETTE_MAX_HEIGHT).min(
                            (window.viewport_size().height
                                - px(Layout::COMMAND_PALETTE_VERTICAL_RESERVE))
                            .max(px(Layout::COMMAND_PALETTE_MIN_HEIGHT)),
                        ),
                    )
                    .flex()
                    .flex_col()
                    .rounded(px(Layout::MENU_RADIUS))
                    .bg(colors.bg_elevated)
                    .border_1()
                    .border_color(colors.border_subtle)
                    .shadow_sm()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_action(cx.listener(Self::next))
                    .on_action(cx.listener(Self::previous))
                    .on_action(cx.listener(Self::accept))
                    .on_action(cx.listener(Self::dismiss))
                    .on_action(cx.listener(Self::scope_next))
                    .child(div().p_3().child(self.input.clone()))
                    .child(
                        div().flex().gap_2().px_3().pb_2().children(
                            [
                                (PaletteScope::All, "全部"),
                                (PaletteScope::Actions, "操作"),
                                (PaletteScope::Tasks, "任务"),
                                (PaletteScope::Files, "文件"),
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(index, (scope, label))| {
                                div()
                                    .id(("palette-scope", index))
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .text_size(px(Typography::METADATA))
                                    .bg(if self.scope == scope {
                                        colors.bg_hover
                                    } else {
                                        colors.bg_elevated
                                    })
                                    .cursor_pointer()
                                    .child(label.replace(['\n', '\r'], " "))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.scope = scope;
                                            this.selected = 0;
                                            this.scroll.scroll_to_item(0);
                                            cx.notify();
                                        }),
                                    )
                            }),
                        ),
                    )
                    .child(
                        div()
                            .id("palette-results")
                            .track_scroll(&self.scroll)
                            .overflow_y_scroll()
                            .min_h_0()
                            .children(entries.into_iter().enumerate().map(
                                |(index, (label, detail, target))| {
                                    div()
                                        .id(("palette-result", index))
                                        .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                                        .px_3()
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .bg(if selected == index {
                                            colors.bg_hover
                                        } else {
                                            colors.bg_elevated
                                        })
                                        .text_size(px(Typography::SIDEBAR))
                                        .cursor_pointer()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .child(label.replace(['\n', '\r'], " ")),
                                        )
                                        .child(
                                            div()
                                                .text_color(colors.text_tertiary)
                                                .text_size(px(Typography::METADATA))
                                                .child(detail.replace(['\n', '\r'], " ")),
                                        )
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(move |_, _, _, cx| {
                                                cx.emit(PaletteActivated(target.clone()))
                                            }),
                                        )
                                },
                            ))
                            .tracked_scroll_edge_fade(
                                self.scroll.clone(),
                                colors.bg_elevated,
                                ScrollFade::compact(),
                            ),
                    )
                    .when(self.loading, |view| {
                        view.child(
                            div()
                                .px_4()
                                .py_2()
                                .text_size(px(Typography::METADATA))
                                .child("正在搜索…"),
                        )
                    })
                    .when(
                        self.error.is_some() || self.results.files_unavailable,
                        |view| {
                            view.child(
                                div()
                                    .px_4()
                                    .py_2()
                                    .text_color(colors.text_secondary)
                                    .text_size(px(Typography::METADATA))
                                    .child(
                                        self.error
                                            .map(|error| -> String {
                                                match error {
                                                PaletteError::Binary => {
                                                    "二进制文件不支持文本预览，请在 Finder 中查看"
                                                        .into()
                                                }
                                                PaletteError::TooLarge => {
                                                    "文件超过 128 KiB，暂不支持预览".into()
                                                }
                                                PaletteError::UnsafePath => {
                                                    "文件位于项目之外或已改变，请重新搜索".into()
                                                }
                                                PaletteError::ProjectUnavailable => {
                                                    "项目不可访问，请重新打开工作区".into()
                                                }
                                                PaletteError::Cancelled => {
                                                    "搜索已取消，请重试".into()
                                                }
                                                PaletteError::Unavailable => {
                                                    "无法读取结果，请重试或检查文件是否存在".into()
                                                }
                                            }
                                            })
                                            .unwrap_or_else(|| {
                                                "部分文件结果暂不可用，请重新搜索".into()
                                            }),
                                    ),
                            )
                        },
                    )
                    .when(!self.loading && self.entries().is_empty(), |view| {
                        view.child(div().p_4().child("没有匹配结果"))
                    })
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .border_t_1()
                            .border_color(colors.border_subtle)
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_tertiary)
                            .child("↑↓ 选择   ↵ 打开   Tab 切换范围   Esc 关闭"),
                    ),
            )
    }
}
