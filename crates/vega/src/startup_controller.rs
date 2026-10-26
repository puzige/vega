use std::{sync::mpsc, time::Duration};

use gpui_kit::{prelude::*, *};
use vega_conversation::types::{PermissionResetError, StartupError};
use vega_i18n::t;
use vega_theme::{Layout, Typography, theme};
use vega_ui::interaction::InteractionExt;

actions!(startup, [NextFocus, PreviousFocus]);

#[cfg(test)]
mod tests;

pub(crate) struct StartupView {
    running: bool,
    error: Option<StartupError>,
    retry_focus: FocusHandle,
    quit_focus: FocusHandle,
    #[cfg(test)]
    fixture_paths: Option<(std::path::PathBuf, std::path::PathBuf)>,
}

impl StartupView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        cx.bind_keys([
            KeyBinding::new("tab", NextFocus, Some("Startup")),
            KeyBinding::new("shift-tab", PreviousFocus, Some("Startup")),
        ]);
        Self {
            running: false,
            error: None,
            retry_focus: cx.focus_handle(),
            quit_focus: cx.focus_handle(),
            #[cfg(test)]
            fixture_paths: None,
        }
    }

    pub(crate) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        self.running = true;
        self.error = None;
        window.focus(&self.quit_focus, cx);
        cx.notify();
        let (sender, receiver) = mpsc::channel();
        #[cfg(test)]
        let fixture_paths = self.fixture_paths.clone();
        if std::thread::Builder::new()
            .name("vega-startup".into())
            .spawn(move || {
                #[cfg(test)]
                if let Some((database, config)) = fixture_paths {
                    let appearance = vega_store::config::read_from(&config).ok();
                    let result = vega_conversation::startup::prepare_workspace(&database, &config);
                    let _ = sender.send((appearance, result));
                    return;
                }
                let appearance = vega_store::config::config_path()
                    .ok()
                    .and_then(|path| vega_store::config::read_from(&path).ok());
                let result = match crate::updater::acknowledge_startup() {
                    Ok(()) => vega_conversation::startup::prepare_default_workspace(),
                    Err(error) => {
                        tracing::error!(%error, "update launch acknowledgment failed");
                        Err(StartupError::WorkerUnavailable)
                    }
                };
                let _ = sender.send((appearance, result));
            })
            .is_err()
        {
            self.fail(StartupError::WorkerUnavailable, window, cx);
            return;
        }
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let (appearance, result) = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        (None, Err(StartupError::WorkerUnavailable))
                    }
                };
                let _ = handle.update(cx, |_, window, cx| {
                    if this.upgrade().is_none() {
                        return;
                    }
                    match result {
                        Ok(workspace) => crate::open_workspace(workspace, window, cx),
                        Err(error) => {
                            crate::apply_appearance(appearance.as_ref(), cx);
                            let _ = this.update(cx, |this, cx| this.fail(error, window, cx));
                        }
                    }
                });
                break;
            }
        })
        .detach();
    }

    fn fail(&mut self, error: StartupError, window: &mut Window, cx: &mut Context<Self>) {
        self.running = false;
        self.error = Some(error);
        window.focus(&self.retry_focus, cx);
        cx.notify();
    }

    fn move_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.running && self.quit_focus.is_focused(window) {
            window.focus(&self.retry_focus, cx);
        } else {
            window.focus(&self.quit_focus, cx);
        }
        cx.notify();
    }

    fn button(&self, retry: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let enabled = !retry || !self.running;
        let (id, label, focus) = if retry {
            ("startup-retry", t("ui.retry"), &self.retry_focus)
        } else {
            ("startup-quit", t("app.startup.quit"), &self.quit_focus)
        };
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
            .py_1()
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .flex()
            .items_center()
            .justify_center()
            .interaction_surface(
                colors.bg_elevated,
                colors.bg_hover,
                colors.bg_active,
                enabled,
            )
            .track_focus(focus)
            .tab_stop(enabled)
            .focus_visible(move |style| style.border_color(colors.accent))
            .when(!enabled, |button| button.text_color(colors.text_tertiary))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            if retry {
                                this.start(window, cx);
                            } else {
                                cx.quit();
                            }
                            cx.stop_propagation();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            if retry {
                                this.start(window, cx);
                            } else {
                                cx.quit();
                            }
                        }),
                    )
            })
            .child(label)
            .into_any_element()
    }
}

impl Render for StartupView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .id("startup")
            .key_context("Startup")
            .size_full()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .p(px(Layout::CONTENT_PADDING))
            .flex()
            .items_center()
            .justify_center()
            .on_action(cx.listener(|this, _: &NextFocus, window, cx| {
                this.move_focus(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &PreviousFocus, window, cx| {
                this.move_focus(window, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .w_full()
                    .max_w(px(Layout::SETTINGS_CONTENT_MAX_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(px(Layout::SETTINGS_CONTENT_GAP))
                    .child(
                        div()
                            .text_size(px(Typography::SETTINGS_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t(if self.running {
                                "app.startup.loading"
                            } else {
                                "app.startup.failed"
                            })),
                    )
                    .when_some(self.error, |content, error| {
                        content.child(div().text_color(colors.danger).child(t(error_key(error))))
                    })
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.button(true, cx))
                            .child(self.button(false, cx)),
                    ),
            )
    }
}

fn error_key(error: StartupError) -> &'static str {
    match error {
        StartupError::PathsUnavailable => "app.startup.paths",
        StartupError::StorageUnavailable => "app.startup.storage",
        StartupError::ConfigurationUnavailable => "app.startup.configuration",
        StartupError::AutomationRecovery => "app.startup.automation",
        StartupError::WorkerUnavailable => "app.startup.worker",
        StartupError::PermissionReset(error) => match error {
            PermissionResetError::Unavailable => "app.startup.reset_unavailable",
            PermissionResetError::Busy => "app.startup.reset_busy",
            PermissionResetError::UnsettledWork => "app.startup.reset_unsettled",
            PermissionResetError::ConfigurationConflict => "app.startup.reset_conflict",
            PermissionResetError::ConfigurationUnavailable => "app.startup.reset_configuration",
            PermissionResetError::OutcomeUnknown => "app.startup.reset_unknown",
        },
    }
}
