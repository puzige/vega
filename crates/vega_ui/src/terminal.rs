//! Interactive terminal viewport. The PTY owns editing, history and command execution.
use crate::scroll_edge_fade::{ScrollFade, ScrollFadeMetrics};
use crate::{conversation_stream::MONOFONT, icons::Icon, text_input};
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
    viewport: Option<(Bounds<Pixels>, Pixels, Pixels)>,
    selection: Option<(usize, usize)>,
    selecting: bool,
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
                            this.clear_selection();
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
            viewport: None,
            selection: None,
            selecting: false,
        }
    }

    fn send(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        self.clear_selection();
        self.input_error = self
            .session
            .as_ref()
            .is_none_or(|session| session.input(bytes).is_err());
        cx.notify();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.clear_selection();
        self.session.take();
        self.session = TerminalSession::start_target(self.target.clone()).ok();
        self.snapshot = None;
        self.size = (0, 0);
        self.input_error = false;
        cx.notify();
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.selecting = false;
    }

    fn selection_range(&self) -> Option<Range<usize>> {
        self.selection.and_then(|(anchor, head)| {
            (anchor != head).then_some(anchor.min(head)..anchor.max(head))
        })
    }

    fn cell_boundary(&self, position: Point<Pixels>) -> Option<usize> {
        let (bounds, cell_width, line_height) = self.viewport?;
        let snapshot = self.snapshot.as_ref()?;
        let cols = usize::from(self.size.1);
        let rows = snapshot.cells.len().min(usize::from(self.size.0));
        if rows == 0 || cols == 0 {
            return None;
        }
        let row = (f32::from(position.y - bounds.top()) / f32::from(line_height))
            .floor()
            .clamp(0., (rows - 1) as f32) as usize;
        let mut col = (f32::from(position.x - bounds.left()) / f32::from(cell_width))
            .round()
            .clamp(0., cols as f32) as usize;
        if col > 0
            && snapshot.cells[row]
                .get(col)
                .is_some_and(|cell| cell.continuation)
        {
            col -= 1;
        }
        Some(row * cols + col)
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.clear_selection();
        if self
            .viewport
            .is_some_and(|(bounds, _, _)| bounds.contains(&event.position))
            && let Some(offset) = self.cell_boundary(event.position)
        {
            self.selection = Some((offset, offset));
            self.selecting = true;
        }
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.selecting = false;
        }
        if self.selecting
            && let Some(offset) = self.cell_boundary(event.position)
            && let Some((anchor, _)) = self.selection
        {
            self.selection = Some((anchor, offset));
            cx.notify();
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting
            && let Some(offset) = self.cell_boundary(event.position)
            && let Some((anchor, _)) = self.selection
        {
            self.selection = Some((anchor, offset));
        }
        self.selecting = false;
        cx.notify();
    }

    fn copy_text(&self, selection: Option<Range<usize>>, cx: &mut Context<Self>) {
        if let Some(snapshot) = &self.snapshot {
            let cols = usize::from(self.size.1);
            let text = snapshot
                .cells
                .iter()
                .enumerate()
                .filter_map(|(index, row)| {
                    let start = index * cols;
                    let end = start + row.len();
                    let range = selection.as_ref().map_or(start..end, |range| {
                        range.start.max(start)..range.end.min(end)
                    });
                    if range.is_empty() {
                        return None;
                    }
                    let text = row
                        .iter()
                        .enumerate()
                        .filter(|(col, cell)| range.contains(&(start + col)) && !cell.continuation)
                        .map(|(_, cell)| {
                            if cell.text.is_empty() {
                                " "
                            } else {
                                &cell.text
                            }
                        })
                        .collect::<String>();
                    Some(if range.end == end {
                        text.trim_end().to_string()
                    } else {
                        text
                    })
                })
                .collect::<Vec<_>>()
                .join("\n");
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        cx.stop_propagation();
    }

    fn copy_screen(&mut self, _: &text_input::Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_text(self.selection_range(), cx);
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
            vega_i18n::t("ui.terminal_failed_to_start").into()
        } else {
            match status {
                TerminalStatus::Starting => vega_i18n::t("ui.starting_terminal").into(),
                TerminalStatus::Running => vega_i18n::t("ui.zsh_login_shell").into(),
                TerminalStatus::Exited(code) => {
                    vega_i18n::t_args("ui.process_exited_code", &[("code", &code.to_string())])
                }
                TerminalStatus::Failed => vega_i18n::t("ui.terminal_error").into(),
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
            .bg(colors.bg_base)
            .font_family(MONOFONT.to_string())
            .text_size(px(Typography::CODE))
            .text_color(colors.text_primary)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy_screen))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.clear_selection();
                cx.notify();
                let delta = event.delta.pixel_delta(px(Typography::CODE * 1.5));
                let rows = (f32::from(delta.y) / (Typography::CODE * 1.5)).round() as i32;
                if let Some(session) = &this.session {
                    let _ = session.scroll(rows);
                }
                cx.stop_propagation();
            }))
            .when(status != TerminalStatus::Running, |view| {
                view.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .h(px(Layout::TERMINAL_TOOLBAR_HEIGHT))
                        .px_3()
                        .border_b_1()
                        .border_color(colors.border_subtle)
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(div().min_w_0().flex_1().truncate().child(label))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .flex_shrink_0()
                                .child(crate::icons::localized_icon_button(
                                    Icon::Document,
                                    "ui.copy_current_terminal_screen_c",
                                    colors,
                                    cx.listener(|this, _, _, cx| this.copy_text(None, cx)),
                                ))
                                .child(crate::icons::localized_icon_button(
                                    Icon::Refresh,
                                    "ui.restart_terminal",
                                    colors,
                                    cx.listener(|this, _, _, cx| this.restart(cx)),
                                )),
                        ),
                )
            })
            .when(self.input_error, |view| {
                view.child(
                    div()
                        .px_2()
                        .text_color(colors.danger)
                        .child(vega_i18n::t("ui.terminal_input_was_not_sent_try_again")),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .px_2()
                    .py_2()
                    .child(
                        div().flex().min_h_0().min_w_0().flex_1().child(
                            canvas(
                                |_, _, _| (),
                                move |bounds, _, window, cx| {
                                    paint_terminal(&view, bounds, window, cx);
                                },
                            )
                            .size_full()
                            .cursor(CursorStyle::IBeam),
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
        if view.size != (rows, cols) {
            view.clear_selection();
        }
        view.viewport = Some((bounds, cell_width, line_height));
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
            let selection = view.selection_range();
            for (row, cells) in snapshot.cells.iter().take(rows as usize).enumerate() {
                let mut text = String::new();
                let mut runs = Vec::new();
                for (col, cell) in cells.iter().take(cols as usize).enumerate() {
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

                    let mut bg = terminal_color(cell.background, colors.bg_base);
                    if cell.inverse {
                        std::mem::swap(&mut fg, &mut bg);
                    }
                    if selection
                        .as_ref()
                        .is_some_and(|range| range.contains(&(row * usize::from(cols) + col)))
                    {
                        fg = colors.brand_on_accent;
                        bg = colors.brand_primary;
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
                let origin = point(bounds.left(), bounds.top() + line_height * row as f32);
                let _ =
                    line.paint_background(origin, line_height, TextAlign::Left, None, window, cx);
                let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
            }
            let mut fade = ScrollFadeMetrics::new(
                bounds,
                point(
                    px(0.),
                    line_height
                        * snapshot.max_scrollback.saturating_sub(snapshot.scrollback) as f32,
                ),
                point(px(0.), line_height * snapshot.scrollback as f32),
            );
            if view.focus.is_focused(window)
                && let Some((row, col)) = snapshot.cursor
                && row < rows
                && col < cols
            {
                fade = fade.protect(
                    Bounds::new(
                        point(bounds.left(), bounds.top() + line_height * row as f32),
                        gpui_kit::size(bounds.size.width, line_height),
                    ),
                    ScrollFade::compact(),
                );
            }
            fade.paint(colors.bg_base, ScrollFade::compact(), window);
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
