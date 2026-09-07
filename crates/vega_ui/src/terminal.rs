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
use vega_theme::{Typography, theme};

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
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.code_bg)
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(label)
                    .child(icon_button(
                        Icon::Document,
                        "复制当前终端屏幕（⌘C）",
                        colors,
                        cx.listener(|this, _, window, cx| {
                            this.copy_screen(&text_input::Copy, window, cx)
                        }),
                    ))
                    .child(icon_button(
                        Icon::Refresh,
                        "重启终端",
                        colors,
                        cx.listener(|this, _, _, cx| this.restart(cx)),
                    )),
            )
            .when(self.input_error, |view| {
                view.child(
                    div()
                        .px_2()
                        .text_color(colors.danger)
                        .child("终端输入未发送，请重试"),
                )
            })
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        paint_terminal(&view, bounds, window, cx);
                    },
                )
                .flex_1()
                .w_full(),
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
                    let mut bg = terminal_color(cell.background, colors.code_bg);
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
    use super::{Duration, Entity, PathBuf, TerminalStatus, TerminalView};
    use gpui_kit::{AppContext, EntityInputHandler, TestAppContext};
    use std::{process::Command, time::Instant};

    fn wait_ui(
        view: &Entity<TerminalView>,
        cx: &mut TestAppContext,
        predicate: impl Fn(&TerminalView) -> bool,
    ) {
        let until = Instant::now() + Duration::from_secs(8);
        loop {
            cx.executor().advance_clock(Duration::from_millis(30));
            cx.run_until_parked();
            if view.read_with(cx, |view, _| predicate(view)) {
                break;
            }
            assert!(Instant::now() < until, "terminal UI timeout");
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    #[gpui_kit::test]
    async fn production_terminal_input_handler_and_keys_reach_real_pty(cx: &mut TestAppContext) {
        const MARKER: &str = "VEGA_R11_TERMINAL_UI_CHILD";
        let Some(root) = std::env::var_os(MARKER) else {
            let root = tempfile::tempdir().unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "terminal::tests::production_terminal_input_handler_and_keys_reach_real_pty",
                    "--nocapture",
                ])
                .env(MARKER, root.path())
                .env("HOME", root.path())
                .env("ZDOTDIR", root.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "owned terminal UI subprocess failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        };
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            crate::init(cx);
        });
        let root = PathBuf::from(root);
        let view = cx.new(|cx| TerminalView::new(root.clone(), cx));
        let entity = view.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, _| entity)
                .unwrap()
        });
        cx.run_until_parked();
        window
            .update(cx, |view, window, cx| window.focus(&view.focus, cx))
            .unwrap();
        wait_ui(&view, cx, |view| {
            view.snapshot
                .as_ref()
                .is_some_and(|s| s.status == TerminalStatus::Running)
        });
        window
            .update(cx, |view, window, cx| {
                view.replace_text_in_range(None, "printf ui-ok > ui-markerX", window, cx)
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "backspace enter");
        let until = Instant::now() + Duration::from_secs(8);
        while !root.join("ui-marker").exists() {
            assert!(
                Instant::now() < until,
                "Backspace/Enter did not reach real shell"
            );
            cx.executor().advance_clock(Duration::from_millis(30));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(
            std::fs::read_to_string(root.join("ui-marker")).unwrap(),
            "ui-ok"
        );
        window
            .update(cx, |view, window, cx| {
                view.replace_text_in_range(None, "sleep 30", window, cx)
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        std::thread::sleep(Duration::from_millis(100));
        cx.simulate_keystrokes(window.into(), "ctrl-c");
        window
            .update(cx, |view, window, cx| {
                view.replace_text_in_range(None, "exit 9", window, cx)
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        wait_ui(&view, cx, |view| {
            view.snapshot
                .as_ref()
                .is_some_and(|s| s.status == TerminalStatus::Exited(9))
        });
        view.update(cx, |view, cx| view.restart(cx));
        wait_ui(&view, cx, |view| {
            view.snapshot
                .as_ref()
                .is_some_and(|s| s.status == TerminalStatus::Running)
        });
        let mut session = view.update(cx, |view, _| view.session.take().unwrap());
        session.close();
        let until = Instant::now() + Duration::from_secs(8);
        while !session
            .snapshot(None)
            .is_some_and(|snapshot| matches!(snapshot.status, TerminalStatus::Exited(_)))
        {
            assert!(Instant::now() < until, "terminal close/reap timeout");
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}
