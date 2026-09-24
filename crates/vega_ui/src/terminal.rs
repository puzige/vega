//! Interactive terminal viewport. The PTY owns editing, history and command execution.
use crate::{
    conversation_stream::MONOFONT,
    icons::{Icon, icon_button},
    text_input,
};
use gpui_kit::{prelude::*, *};
use std::{ops::Range, path::PathBuf, time::Duration};
use vega_conversation::{
    terminal::TerminalSession,
    types::{TerminalColor, TerminalSnapshot, TerminalStatus, TerminalTarget},
};
use vega_theme::{Layout, Typography, theme};

/// A live terminal session, retained by the workspace across hide/move operations.
pub struct TerminalView {
    target: TerminalTarget,
    focus: FocusHandle,
    session: Option<TerminalSession>,
    snapshot: Option<TerminalSnapshot>,
    size: (u16, u16),
    marked: String,
    input_error: bool,
}

impl TerminalView {
    #[cfg(feature = "test-support")]
    pub fn for_test_target(target: TerminalTarget, cx: &mut Context<Self>) -> Self {
        Self {
            target,
            focus: cx.focus_handle(),
            session: None,
            snapshot: None,
            size: (0, 0),
            marked: String::new(),
            input_error: false,
        }
    }

    /// Starts one persistent login shell in the trusted project root.
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        Self::for_target(TerminalTarget::Directory(root), cx)
    }

    /// Create a terminal whose project authority is resolved off the UI thread.
    pub fn for_target(target: TerminalTarget, cx: &mut Context<Self>) -> Self {
        let session = TerminalSession::start_target(target.clone()).ok();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if let Some(snapshot) = this
                            .session
                            .as_ref()
                            .and_then(|s| s.snapshot(this.snapshot.as_ref().map(|s| s.generation)))
                        {
                            this.snapshot = Some(snapshot);
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            target,
            focus: cx.focus_handle(),
            session,
            snapshot: None,
            size: (0, 0),
            marked: String::new(),
            input_error: false,
        }
    }

    fn send(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        self.input_error = self
            .session
            .as_ref()
            .is_none_or(|session| session.input(bytes).is_err());
        cx.notify();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.session.take();
        self.session = TerminalSession::start_target(self.target.clone()).ok();
        self.snapshot = None;
        self.size = (0, 0);
        self.input_error = false;
        cx.notify();
    }

    fn copy_screen(&mut self, _: &text_input::Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = &self.snapshot {
            let text = snapshot
                .cells
                .iter()
                .map(|row| {
                    row.iter()
                        .filter(|cell| !cell.continuation)
                        .map(|cell| {
                            if cell.text.is_empty() {
                                " "
                            } else {
                                &cell.text
                            }
                        })
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        cx.stop_propagation();
    }

    fn paste(&mut self, _: &text_input::Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            if text.len() > 64 * 1024 - 12 {
                self.input_error = true;
                cx.notify();
                return;
            }
            let text = if self.snapshot.as_ref().is_some_and(|s| s.bracketed_paste) {
                format!("\x1b[200~{text}\x1b[201~")
            } else {
                text
            };
            self.send(text.as_bytes(), cx);
        }
        cx.stop_propagation();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.marked.is_empty() {
            return;
        }
        let key = &event.keystroke.key;
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform {
            return;
        }
        let bytes = if modifiers.control && key.len() == 1 {
            key.bytes()
                .next()
                .map(|byte| vec![byte.to_ascii_uppercase() & 0x1f])
        } else {
            let app = self.snapshot.as_ref().is_some_and(|s| s.application_cursor);
            match key.as_str() {
                "enter" => Some(b"\r".to_vec()),
                "backspace" => Some(vec![127]),
                "delete" => Some(b"\x1b[3~".to_vec()),
                "tab" => Some(if modifiers.shift {
                    b"\x1b[Z".to_vec()
                } else {
                    vec![9]
                }),
                "escape" => Some(vec![27]),
                "up" => Some(if app { b"\x1bOA" } else { b"\x1b[A" }.to_vec()),
                "down" => Some(if app { b"\x1bOB" } else { b"\x1b[B" }.to_vec()),
                "right" => Some(if app { b"\x1bOC" } else { b"\x1b[C" }.to_vec()),
                "left" => Some(if app { b"\x1bOD" } else { b"\x1b[D" }.to_vec()),
                "home" => Some(b"\x1b[H".to_vec()),
                "end" => Some(b"\x1b[F".to_vec()),
                "pageup" => Some(b"\x1b[5~".to_vec()),
                "pagedown" => Some(b"\x1b[6~".to_vec()),
                _ => None,
            }
        };
        if let Some(bytes) = bytes {
            self.send(&bytes, cx);
            cx.stop_propagation();
        }
    }
}
impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let status = self
            .snapshot
            .as_ref()
            .map_or(TerminalStatus::Starting, |s| s.status);
        let label = if self.session.is_none() || status == TerminalStatus::Failed {
            "终端启动失败".into()
        } else {
            match status {
                TerminalStatus::Starting => "正在启动终端…".into(),
                TerminalStatus::Running => "zsh · 登录 shell".into(),
                TerminalStatus::Exited(code) => format!("进程已退出 ({code})"),
                TerminalStatus::Failed => "终端错误".into(),
            }
        };
        let view = cx.entity();
        div()
            .id("terminal-view")
            .debug_selector(|| "terminal-view".into())
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            // R47 §2.3: the terminal body is the session surface (`bg_base`,
            // Codex `--color-background-surface`), not the inline-code inset
            // face. The painted cells' default background below uses the same
            // token so the canvas never re-introduces the gray inset.
            .bg(colors.bg_base)
            .font_family(MONOFONT.to_string())
            .text_size(px(Typography::CODE))
            .text_color(colors.text_primary)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy_screen))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(Typography::CODE * 1.5));
                let rows = (f32::from(delta.y) / (Typography::CODE * 1.5)).round() as i32;
                if let Some(session) = &this.session {
                    let _ = session.scroll(rows);
                }
                cx.stop_propagation();
            }))
            // R47 §2.3: Codex has no terminal status row. The toolbar (status
            // label + copy/restart) is a visibility predicate for abnormal
            // states only (`status != Running`); while Running the row is not
            // rendered at all and copy stays available through the existing
            // ⌘C → `copy_screen` action (restart is covered by `+`/tab `×`).
            .when(status != TerminalStatus::Running, |view| {
                view.child(
                    div()
                        .debug_selector(|| "terminal-toolbar".into())
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .h(px(Layout::TERMINAL_TOOLBAR_HEIGHT))
                        .px_3()
                        .border_b_1()
                        .border_color(colors.border_subtle)
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(
                            div()
                                .debug_selector(|| "terminal-status".into())
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .child(label),
                        )
                        .child(
                            div()
                                .debug_selector(|| "terminal-actions".into())
                                .flex()
                                .items_center()
                                .gap_1()
                                .flex_shrink_0()
                                .child(
                                    icon_button(
                                        Icon::Document,
                                        "复制当前终端屏幕（⌘C）",
                                        colors,
                                        cx.listener(|this, _, window, cx| {
                                            this.copy_screen(&text_input::Copy, window, cx)
                                        }),
                                    )
                                    .debug_selector(|| "terminal-copy".into()),
                                )
                                .child(
                                    icon_button(
                                        Icon::Refresh,
                                        "重启终端",
                                        colors,
                                        cx.listener(|this, _, _, cx| this.restart(cx)),
                                    )
                                    .debug_selector(|| "terminal-restart".into()),
                                ),
                        ),
                )
            })
            .when(self.input_error, |view| {
                view.child(
                    div()
                        .px_2()
                        .text_color(colors.danger)
                        .child("终端输入未发送，请重试"),
                )
            })
            .child(
                div()
                    .debug_selector(|| "terminal-canvas-frame".into())
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    // R47 §2.3: 8px horizontal padding puts the text's left
                    // edge at ≈16px, matching Codex's measured 16.5px.
                    .px_2()
                    .py_2()
                    .child(
                        div()
                            .debug_selector(|| "terminal-canvas".into())
                            .flex()
                            .min_h_0()
                            .min_w_0()
                            .flex_1()
                            .child(
                                canvas(
                                    |_, _, _| (),
                                    move |bounds, _, window, cx| {
                                        paint_terminal(&view, bounds, window, cx);
                                    },
                                )
                                .size_full(),
                            ),
                    ),
            )
    }
}

fn terminal_color(value: TerminalColor, default: Rgba) -> Rgba {
    match value {
        TerminalColor::Default => default,
        TerminalColor::Indexed(i) => vega_theme::terminal_indexed_color(i),
        TerminalColor::Rgb(r, g, b) => {
            rgba((u32::from(r) << 24) | (u32::from(g) << 16) | (u32::from(b) << 8) | 255)
        }
    }
}

fn paint_terminal(
    view: &Entity<TerminalView>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let colors = theme(cx).colors;
    let font = window.text_style().font();
    let size = px(Typography::CODE);
    let line_height = px(Typography::CODE * 1.5);
    let metric = window.text_system().shape_line(
        "M".into(),
        size,
        &[TextRun {
            len: 1,
            font: font.clone(),
            color: colors.text_primary.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    let cell_width = metric.width.max(px(1.));
    let rows = (f32::from(bounds.size.height) / f32::from(line_height))
        .floor()
        .clamp(2., 240.) as u16;
    let cols = (f32::from(bounds.size.width) / f32::from(cell_width))
        .floor()
        .clamp(2., 400.) as u16;
    view.update(cx, |view, cx| {
        if view.size != (rows, cols)
            && let Some(session) = &view.session
            && session.resize(rows, cols).is_ok()
        {
            view.size = (rows, cols);
        }
        window.handle_input(
            &view.focus,
            ElementInputHandler::new(bounds, cx.entity()),
            cx,
        );
        if let Some(snapshot) = &view.snapshot {
            for (row, cells) in snapshot.cells.iter().take(rows as usize).enumerate() {
                let mut text = String::new();
                let mut runs = Vec::new();
                for cell in cells.iter().take(cols as usize) {
                    if cell.continuation {
                        continue;
                    }
                    let content = if cell.text.is_empty() {
                        " "
                    } else {
                        &cell.text
                    };
                    text.push_str(content);
                    let mut fg = terminal_color(cell.foreground, colors.text_primary);
                    // R47 §2.3: the default cell background is the session
                    // surface (`bg_base`), matching the terminal body; explicit
                    // ANSI colors are untouched.
                    let mut bg = terminal_color(cell.background, colors.bg_base);
                    if cell.inverse {
                        std::mem::swap(&mut fg, &mut bg);
                    }
                    let mut font = font.clone();
                    if cell.bold {
                        font.weight = FontWeight::BOLD;
                    }
                    runs.push(TextRun {
                        len: content.len(),
                        font,
                        color: fg.into(),
                        background_color: Some(bg.into()),
                        underline: None,
                        strikethrough: None,
                    });
                }
                let line = window
                    .text_system()
                    .shape_line(text.into(), size, &runs, None);
                let _ = line.paint(
                    point(bounds.left(), bounds.top() + line_height * row as f32),
                    line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            if view.focus.is_focused(window)
                && let Some((row, col)) = snapshot.cursor
                && row < rows
                && col < cols
            {
                window.paint_quad(fill(
                    Bounds::new(
                        point(
                            bounds.left() + cell_width * col as f32,
                            bounds.top() + line_height * (row as f32 + 1.) - px(2.),
                        ),
                        gpui_kit::size(cell_width, px(2.)),
                    ),
                    colors.accent,
                ));
            }
        }
    });
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        *actual = Some(0..0);
        Some(String::new())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.marked.is_empty()).then_some(0..self.marked.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked.clear();
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked.clear();
        self.send(text.as_bytes(), cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = text.chars().take(4096).collect();
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{Entity, PathBuf, TerminalStatus, TerminalView};
    use gpui_kit::{
        AppContext, Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowHandle,
        WindowOptions, point, px, size,
    };
    use vega_conversation::types::{TerminalSnapshot, TerminalTarget};
    use vega_theme::Layout;

    fn assert_pixel_close(actual: gpui_kit::Pixels, expected: f32, label: &str) {
        let actual = f32::from(actual);
        assert!(
            (actual - expected).abs() <= 1.0,
            "{label}: expected {expected}±1px, got {actual}px"
        );
    }

    /// Mounts a `TerminalView` in a fixed unit state (no PTY, snapshot set
    /// directly) so each R47 toolbar-visibility state can be asserted in
    /// isolation without waiting on a real shell.
    fn mount_unit_view(
        status: TerminalStatus,
        cx: &mut TestAppContext,
    ) -> (Entity<TerminalView>, WindowHandle<TerminalView>) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            crate::init(cx);
        });
        let view = cx.new(|cx| TerminalView {
            target: TerminalTarget::Directory(PathBuf::from("/tmp")),
            focus: cx.focus_handle(),
            session: None,
            snapshot: Some(TerminalSnapshot {
                generation: 0,
                status,
                cells: Vec::new(),
                cursor: None,
                application_cursor: false,
                bracketed_paste: false,
                scrollback: 0,
            }),
            size: (0, 0),
            marked: String::new(),
            input_error: false,
        });
        let entity = view.clone();
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(640.), px(400.)),
                    ))),
                    ..Default::default()
                },
                move |_, _| entity,
            )
            .expect("mounted unit terminal window")
        });
        cx.run_until_parked();
        (view, window)
    }

    /// R47 §3 t4: while Running the status toolbar row is not rendered at
    /// all; the terminal surface itself stays mounted.
    #[gpui_kit::test]
    async fn r47_terminal_running_hides_the_status_toolbar(cx: &mut TestAppContext) {
        let (view, window) = mount_unit_view(TerminalStatus::Running, cx);
        view.update(cx, |view, _| {
            view.snapshot.as_mut().unwrap().cells =
                vec![vec![vega_conversation::types::TerminalCell {
                    text: "copied snapshot".into(),
                    foreground: vega_conversation::types::TerminalColor::Default,
                    background: vega_conversation::types::TerminalColor::Default,
                    bold: false,
                    inverse: false,
                    continuation: false,
                }]];
        });
        window
            .update(cx, |view, window, cx| {
                view.copy_screen(&crate::text_input::Copy, window, cx)
            })
            .unwrap();
        assert_eq!(
            cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()))
                .as_deref(),
            Some("copied snapshot")
        );
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("terminal-view").is_some(),
            "terminal surface stays mounted"
        );
        assert!(
            visual.debug_bounds("terminal-toolbar").is_none(),
            "Running terminal must not render the status toolbar"
        );
        let terminal = visual.debug_bounds("terminal-view").unwrap();
        let canvas = visual
            .debug_bounds("terminal-canvas")
            .expect("inset terminal canvas");
        assert_pixel_close(
            canvas.left() - terminal.left(),
            8.0,
            "terminal canvas left inset",
        );
        assert_pixel_close(
            terminal.right() - canvas.right(),
            8.0,
            "terminal canvas right inset",
        );
        assert_pixel_close(
            canvas.top() - terminal.top(),
            8.0,
            "terminal canvas top inset",
        );
        assert_pixel_close(
            terminal.bottom() - canvas.bottom(),
            8.0,
            "terminal canvas bottom inset",
        );

        assert!(
            visual.debug_bounds("terminal-status").is_none()
                && visual.debug_bounds("terminal-copy").is_none()
                && visual.debug_bounds("terminal-restart").is_none(),
            "Running terminal renders neither status label nor copy/restart"
        );
    }

    /// R47 §3 t5: the Exited state renders the status toolbar with both the
    /// copy and the restart action.
    #[gpui_kit::test]
    async fn r47_terminal_exited_renders_the_status_toolbar(cx: &mut TestAppContext) {
        let (_, window) = mount_unit_view(TerminalStatus::Exited(9), cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let toolbar = visual
            .debug_bounds("terminal-toolbar")
            .expect("exited terminal renders the status toolbar");
        let status = visual
            .debug_bounds("terminal-status")
            .expect("bounded terminal status");
        let actions = visual
            .debug_bounds("terminal-actions")
            .expect("terminal trailing actions");
        let copy = visual
            .debug_bounds("terminal-copy")
            .expect("terminal copy action");
        let restart = visual
            .debug_bounds("terminal-restart")
            .expect("terminal restart action");
        assert_pixel_close(
            toolbar.size.height,
            Layout::TERMINAL_TOOLBAR_HEIGHT,
            "terminal toolbar height",
        );
        assert_pixel_close(
            status.left() - toolbar.left(),
            12.0,
            "terminal status leading inset",
        );
        assert!(
            status.right() <= actions.left(),
            "status truncates before the trailing actions"
        );
        assert_pixel_close(
            toolbar.right() - actions.right(),
            12.0,
            "terminal actions trailing inset",
        );
        assert_pixel_close(actions.size.width, 52.0, "compact terminal action group");
        for (bounds, label) in [(copy, "copy hitbox"), (restart, "restart hitbox")] {
            assert_pixel_close(bounds.size.width, 24.0, label);
            assert_pixel_close(bounds.size.height, 24.0, label);
        }
        assert_pixel_close(restart.left() - copy.right(), 4.0, "terminal action gap");
        assert!(
            visual.debug_bounds("terminal-copy").is_some(),
            "exited terminal keeps its copy action"
        );
        assert!(
            visual.debug_bounds("terminal-restart").is_some(),
            "exited terminal keeps its restart action"
        );
    }

    /// R47 §3 t5: the Failed state renders the status toolbar with both the
    /// copy and the restart action.
    #[gpui_kit::test]
    async fn r47_terminal_failed_renders_the_status_toolbar(cx: &mut TestAppContext) {
        let (_, window) = mount_unit_view(TerminalStatus::Failed, cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("terminal-toolbar").is_some(),
            "failed terminal renders the status toolbar"
        );
        assert!(
            visual.debug_bounds("terminal-copy").is_some(),
            "failed terminal keeps its copy action"
        );
        assert!(
            visual.debug_bounds("terminal-restart").is_some(),
            "failed terminal keeps its restart action"
        );
    }
}
