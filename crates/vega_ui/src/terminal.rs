mod surface;

use crate::nsh::Surface;
use gpui_kit::{prelude::*, *};
use std::{path::PathBuf, sync::Arc, time::Duration};
use surface::{Command, Projection, Request};
use vega_conversation::{
    terminal::TerminalSession,
    types::{TerminalSnapshot, TerminalStatus, TerminalTarget},
};
use vega_theme::theme;

pub struct TerminalView {
    target: TerminalTarget,
    focus: FocusHandle,
    session: Option<TerminalSession>,
    snapshot: Option<Arc<TerminalSnapshot>>,
    cell_width: Option<f32>,
    size: (u16, u16),
    requested_size: (u16, u16),
    input_error: bool,
    epoch: u64,
    accepted_input: u64,
    surface: Option<Surface<Projection, Request>>,
    surface_error: bool,
    menu_dismissal: u64,
    focus_subscriptions: Option<[Subscription; 2]>,
}

impl TerminalView {
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        Self::for_target(TerminalTarget::Directory(root), cx)
    }

    pub fn for_target(target: TerminalTarget, cx: &mut Context<Self>) -> Self {
        let session = TerminalSession::start_target(target.clone()).ok();
        let text_system = cx.text_system();
        let cell_width = text_system
            .advance(
                text_system.resolve_font(&font(crate::conversation_stream::MONOFONT)),
                px(vega_theme::Typography::CODE),
                'M',
            )
            .ok()
            .map(|advance| f32::from(advance.width))
            .filter(|width| width.is_finite() && *width > 0.);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
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
            cell_width,
            size: (0, 0),
            requested_size: (0, 0),
            input_error: false,
            epoch: 0,
            accepted_input: 0,
            surface: None,
            surface_error: false,
            menu_dismissal: 0,
            focus_subscriptions: None,
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.size != self.requested_size
            && self.requested_size != (0, 0)
            && let Some(session) = &self.session
            && session
                .resize(self.requested_size.0, self.requested_size.1)
                .is_ok()
        {
            self.size = self.requested_size;
        }
        if let Some(snapshot) = self
            .session
            .as_ref()
            .and_then(|s| s.snapshot(self.snapshot.as_ref().map(|s| s.generation)))
        {
            self.snapshot = Some(Arc::new(snapshot));
            self.publish(cx);
        }
        let failed = self
            .surface
            .as_ref()
            .is_some_and(|surface| surface.errors().borrow().is_some());
        if failed != self.surface_error {
            self.surface_error = failed;
            cx.notify();
        }
    }

    fn projection(&self, cx: &App) -> Projection {
        let status = self.snapshot.as_ref().map_or(
            if self.session.is_some() {
                TerminalStatus::Starting
            } else {
                TerminalStatus::Failed
            },
            |snapshot| snapshot.status,
        );
        let status_label = match status {
            TerminalStatus::Starting => vega_i18n::t("ui.starting_terminal").into(),
            TerminalStatus::Running => String::new(),
            TerminalStatus::Exited(code) => {
                vega_i18n::t_args("ui.process_exited_code", &[("code", &code.to_string())])
            }
            TerminalStatus::Failed => vega_i18n::t("ui.terminal_failed_to_start").into(),
        };
        Projection {
            snapshot: self.snapshot.clone(),
            cell_width: self.cell_width,
            epoch: self.epoch,
            menu_dismissal: self.menu_dismissal,
            colors: theme(cx).colors,
            status_label,
            copy_label: vega_i18n::t("ui.copy_current_terminal_screen_c").into(),
            restart_label: vega_i18n::t("ui.restart_terminal").into(),
            error_label: vega_i18n::t("ui.terminal_input_was_not_sent_try_again").into(),
            input_error: self.input_error,
        }
    }

    fn dismiss_terminal_menu(&mut self, cx: &App) {
        self.menu_dismissal = self.menu_dismissal.wrapping_add(1);
        self.publish(cx);
    }

    fn publish(&self, cx: &App) {
        if let Some(surface) = &self.surface {
            surface.publish(self.projection(cx));
        }
    }

    fn request(&mut self, request: Request, cx: &mut Context<Self>) {
        if request.epoch != self.epoch {
            if let Some(reply) = request.reply {
                let _ = reply.send(false);
            }
            return;
        }
        let is_input = matches!(
            &request.command,
            Command::Input(_) | Command::Paste(_) | Command::Submit(_)
        );
        if is_input {
            let accepted = request
                .sequence
                .is_some_and(|sequence| sequence > 0 && sequence <= self.accepted_input);
            if accepted || request.sequence != self.accepted_input.checked_add(1) {
                if let Some(reply) = request.reply {
                    let _ = reply.send(accepted);
                }
                return;
            }
        } else if request.sequence.is_some() {
            if let Some(reply) = request.reply {
                let _ = reply.send(false);
            }
            return;
        }
        let mut accepted = true;
        match request.command {
            Command::Input(bytes) => accepted = self.send(&bytes),
            Command::Paste(ref text) | Command::Submit(ref text) => {
                let submit = matches!(request.command, Command::Submit(_));
                if text.len().saturating_add(usize::from(submit)) > surface::MAX_PASTE {
                    self.input_error = true;
                    accepted = false;
                } else {
                    let mut bytes = if self.snapshot.as_ref().is_some_and(|s| s.bracketed_paste) {
                        format!("\x1b[200~{text}\x1b[201~").into_bytes()
                    } else {
                        text.as_bytes().to_vec()
                    };
                    if submit {
                        bytes.push(b'\r');
                    }
                    accepted = self.send(&bytes);
                }
            }
            Command::Resize(rows, cols) => {
                self.requested_size = (rows.clamp(2, 240), cols.clamp(2, 400));
            }
            Command::ScrollTo(offset) => {
                if let Some(session) = &self.session {
                    let current = session.snapshot(None).map_or(0, |s| s.scrollback);
                    if session.scroll(offset as i32 - current as i32).is_err() {
                        self.input_error = true;
                    }
                }
            }
            Command::Restart => {
                self.session.take();
                self.session = TerminalSession::start_target(self.target.clone()).ok();
                self.snapshot = None;
                self.size = (0, 0);
                self.epoch = self.epoch.wrapping_add(1);
                self.accepted_input = 0;
                self.input_error = false;
            }
        }
        if accepted
            && is_input
            && let Some(sequence) = request.sequence
        {
            self.accepted_input = sequence;
        }
        if let Some(reply) = request.reply {
            let _ = reply.send(accepted);
        }
        self.publish(cx);
        cx.notify();
    }

    fn send(&mut self, bytes: &[u8]) -> bool {
        self.input_error = bytes.len() > 64 * 1024
            || self
                .session
                .as_ref()
                .is_none_or(|session| session.input(bytes).is_err());
        !self.input_error
    }

    fn ensure_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new(self.projection(cx), surface::render, window, cx);
        self.surface = Some(surface);
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, _, cx| this.request(request, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_surface(window, cx);
        if self.focus_subscriptions.is_none() {
            self.focus_subscriptions = Some([
                cx.on_focus(&self.focus, window, |this, window, cx| {
                    if let Some(surface) = &this.surface {
                        surface.focus(window, cx);
                    }
                }),
                cx.on_focus_out(&self.focus, window, |this, _, _, cx| {
                    this.dismiss_terminal_menu(cx)
                }),
            ]);
            if self.focus.is_focused(window)
                && let Some(surface) = &self.surface
            {
                surface.focus(window, cx);
            }
        }
        if let Some(surface) = &mut self.surface {
            surface.sync_environment(cx);
        }
        self.publish(cx);
        div()
            .id("terminal-view")
            .key_context("Terminal")
            .track_focus(&self.focus)
            .on_mouse_down_out(
                cx.listener(|this, _: &MouseDownEvent, _, cx| this.dismiss_terminal_menu(cx)),
            )
            .size_full()
            .children(self.surface.as_ref().map(Surface::entity))
    }
}
