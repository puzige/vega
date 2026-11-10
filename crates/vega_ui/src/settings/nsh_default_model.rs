use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use vega_theme::{Layout, ThemeColors, Typography};

use super::{SettingsView, helpers::all_models};
use crate::nsh::{ProjectionId, Surface};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
    focused: Option<usize>,
    targets: Vec<super::nsh_control::FocusTarget>,
    catalog: Arc<[String]>,
    epoch: u64,
    restore: bool,
    _focus_events: Vec<gpui_kit::Subscription>,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    epoch: u64,
    restore: bool,
    current: String,
    focused: Option<usize>,
    models: Arc<[String]>,
    open: bool,
    visible: bool,
    enabled: bool,
    colors: ThemeColors,
}

#[derive(Clone)]
enum Action {
    Toggle,
    Select(String),
}

struct Decision {
    observed: Snapshot,
    action: Action,
    reply: oneshot::Sender<bool>,
}

enum Request {
    Decide(Box<Decision>),
    Layout(f32),
    FocusReady(u64),
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: &Action) -> bool {
    current.visible
        && current.epoch == observed.epoch
        && current.open == observed.open
        && match action {
            Action::Toggle => current.enabled,
            Action::Select(model) => {
                current.enabled
                    && current.open
                    && current.current == observed.current
                    && current.models == observed.models
                    && current.models.contains(model)
            }
        }
}

impl SettingsView {
    fn default_model_snapshot(&self, cx: &App, id: ProjectionId) -> Snapshot {
        Snapshot {
            id,
            epoch: self
                .default_model_presentation
                .as_ref()
                .map_or(0, |p| p.epoch),
            restore: self
                .default_model_presentation
                .as_ref()
                .is_some_and(|p| p.restore),
            focused: self
                .default_model_presentation
                .as_ref()
                .and_then(|page| page.focused),
            current: self.config.defaults.model.clone(),
            models: self
                .default_model_presentation
                .as_ref()
                .map(|page| page.catalog.clone())
                .unwrap_or_else(|| all_models(&self.config.providers).into()),
            open: self.model_open,
            visible: !self.palette_open
                && self.pending_section.is_none()
                && !self.pending_settings_close
                && self.section == 1
                && cx
                    .try_global::<super::SettingsOpen>()
                    .is_some_and(|open| open.0),
            enabled: !self.provider_management.saving && !self.default_permission.io_pending(),
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(super) fn invalidate_model_controls(&mut self) {
        if let Some(page) = &mut self.default_model_presentation {
            page.epoch = page.epoch.wrapping_add(1);
            page.restore = false;
        }
    }

    fn request_model_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = &mut self.default_model_presentation {
            page.restore = true;
            page.surface.focus(window, cx);
        }
    }

    fn close_model_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(page) = &self.default_model_presentation else {
            return false;
        };
        if !self.model_open
            || self.default_permission.io_pending()
            || !page
                .focused
                .and_then(|slot| page.targets.get(slot))
                .is_some_and(|target| target.handle.is_focused(window))
            || !self.default_model_snapshot(cx, page.id.clone()).visible
        {
            return false;
        }
        self.invalidate_model_controls();
        self.model_open = false;
        self.request_model_focus(window, cx);
        cx.notify();
        true
    }

    fn restore_model_focus(&mut self, epoch: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = &self.default_model_presentation else {
            return;
        };
        let current = self.default_model_snapshot(cx, page.id.clone());
        let Some(trigger) = page.targets.first() else {
            return;
        };
        if current.epoch == epoch
            && current.restore
            && !current.open
            && current.visible
            && current.enabled
            && window.is_window_active()
            && page.surface.focus_handle(cx).is_focused(window)
            && trigger.placed_for(&trigger_stamp(&current))
        {
            window.focus(&trigger.handle, cx);
            if let Some(page) = &mut self.default_model_presentation {
                page.restore = false;
            }
        }
    }

    pub(super) fn sync_default_model(&mut self, cx: &App) {
        if let Some(page) = &mut self.default_model_presentation {
            let catalog = all_models(&self.config.providers);
            if catalog.as_slice() != page.catalog.as_ref() {
                page.catalog = catalog.into();
                page.epoch = page.epoch.wrapping_add(1);
            }
        }
        if let Some(page) = &self.default_model_presentation {
            page.surface
                .publish(self.default_model_snapshot(cx, page.id.clone()));
        }
    }

    pub(super) fn render_default_model(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.default_model_presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) = Surface::new_transparent(
                self.default_model_snapshot(cx, id.clone()),
                render,
                window,
                cx,
            );
            let owner = cx.weak_entity();
            let window_id = window.window_handle().window_id();
            let escape = cx.intercept_keystrokes(move |event, window, cx| {
                if event.keystroke.key == "escape"
                    && event.keystroke.modifiers == gpui_kit::Modifiers::default()
                    && window.window_handle().window_id() == window_id
                    && window.is_window_active()
                    && let Some(owner) = owner.upgrade()
                    && owner.update(cx, |settings, cx| settings.close_model_picker(window, cx))
                {
                    cx.stop_propagation();
                }
            });
            self.default_model_presentation = Some(Presentation {
                focused: None,
                targets: Vec::new(),
                catalog: all_models(&self.config.providers).into(),
                epoch: 0,
                restore: false,
                _focus_events: vec![
                    escape,
                    cx.observe_window_activation(window, |settings, window, cx| {
                        if let Some(page) = &settings.default_model_presentation {
                            settings.restore_model_focus(page.epoch, window, cx);
                        }
                    }),
                ],
                surface,
                id,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted = this.section == 1
                                        && this.default_model_presentation.as_ref().is_some_and(
                                            |page| {
                                                page.id.matches(&command.observed.id)
                                                    && command.observed.models.as_ref()
                                                        == all_models(&this.config.providers)
                                                            .as_slice()
                                                    && accepts(
                                                        &this.default_model_snapshot(
                                                            cx,
                                                            page.id.clone(),
                                                        ),
                                                        &command.observed,
                                                        &command.action,
                                                    )
                                            },
                                        );
                                    if accepted {
                                        this.invalidate_model_controls();
                                        match command.action {
                                            Action::Toggle => this.model_open = !this.model_open,
                                            Action::Select(model) => {
                                                this.select_model(&model, cx);
                                                this.request_model_focus(window, cx);
                                            }
                                        }
                                        cx.notify();
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::FocusReady(epoch) => {
                                    this.restore_model_focus(epoch, window, cx)
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.default_model_presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            if let Some(page) = &this.default_model_presentation {
                                page.surface
                                    .publish(this.default_model_snapshot(cx, page.id.clone()));
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
        let Some(page) = &self.default_model_presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.default_model_snapshot(cx, page.id.clone());
        let Some(page) = &mut self.default_model_presentation else {
            return gpui_kit::div().into_any_element();
        };
        while page.targets.len() <= snapshot.models.len() {
            let slot = page.targets.len();
            let target = super::nsh_control::FocusTarget::new(cx);
            let owner = cx.weak_entity();
            target.register(
                page.surface.entity().read(cx),
                &target_name(slot),
                if slot == 0 {
                    super::nsh_control::Semantics::Disclosure
                } else {
                    super::nsh_control::Semantics::Selection
                },
                move |displayed, cx| {
                    let Some(owner) = owner.upgrade() else {
                        return false;
                    };
                    let settings = owner.read(cx);
                    let Some(page) = &settings.default_model_presentation else {
                        return false;
                    };
                    let current = settings.default_model_snapshot(cx, page.id.clone());
                    current.visible
                        && current.enabled
                        && (slot == 0 || (current.open && slot <= current.models.len()))
                        && *displayed == trigger_stamp(&current)
                },
            );
            let gained = cx.on_focus(&target.handle, window, move |settings, _, cx| {
                if let Some(page) = &mut settings.default_model_presentation {
                    page.focused = Some(slot);
                    cx.notify();
                }
            });
            let lost = cx.on_blur(&target.handle, window, move |settings, _, cx| {
                if let Some(page) = &mut settings.default_model_presentation {
                    if page.focused != Some(slot) {
                        return;
                    }
                    page.focused = None;
                    cx.notify();
                }
            });
            page.targets.push(target);
            page._focus_events.extend([gained, lost]);
        }
        page.surface.sync_environment(cx);
        page.surface.publish(snapshot);
        gpui_kit::div()
            .w_full()
            .flex_shrink_0()
            .h(gpui_kit::px(page.height))
            .child(page.surface.entity())
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct Actions {
    owner: nsh_core::element::Owner,
    state: Signal<Snapshot>,
    pending: Signal<bool>,
    failed: Signal<bool>,
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let observed = self.state.get_untracked();
        if self.pending.get_untracked() || !accepts(&observed, &observed, &action) {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        self.owner.run(|| {
            spawn(async move {
                let (reply, response) = oneshot::channel();
                let accepted = commands
                    .send(Request::Decide(Box::new(Decision {
                        observed,
                        action,
                        reply,
                    })))
                    .await
                    .is_ok()
                    && matches!(response.await, Ok(true));
                self.failed.set(!accepted);
                self.pending.set(false);
            })
        });
    }
}

fn target_name(slot: usize) -> String {
    format!("default-model-{slot}")
}

fn trigger_stamp(snapshot: &Snapshot) -> serde_json::Value {
    serde_json::json!([
        snapshot.epoch,
        snapshot.current,
        snapshot.open,
        snapshot.enabled,
        snapshot.visible
    ])
}

fn control(state: Signal<Snapshot>, slot: usize) -> nsh_core::element::View {
    view()
        .relative()
        .accessible(false)
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .px_3()
        .py_2()
        .border_color(move || {
            crate::nsh::color(if state.get().focused == Some(slot) {
                state.get().colors.accent
            } else if slot == 0 {
                state.get().colors.border_subtle
            } else {
                gpui_kit::transparent_black().into()
            })
        })
        .bg(move || crate::nsh::color(state.get().colors.bg_elevated))
}

fn icon(kind: crate::icons::Icon, state: Signal<Snapshot>) -> AnyElement {
    let source = crate::icons::nsh_source(kind).unwrap_or_default();
    svg(nsh_core::element::ImageSrc::svg_markup(&source))
        .accessible(false)
        .size(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .tint_color(move || crate::nsh::color(state.get().colors.text_secondary).to_hsla())
        .into_element()
}

fn model_label(snapshot: &Snapshot) -> String {
    if snapshot.current.is_empty() {
        vega_i18n::t("settings.model.not_selected").to_string()
    } else {
        snapshot.current.clone()
    }
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        owner: nsh_core::element::Owner::current(),
        state,
        pending: signal(false),
        failed: signal(false),
    };
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let toggle = commands.clone();
    let focus_commands = commands.clone();
    view()
        .w_full()
        .flex_shrink_0()
        .gap_1()
        .text_size(px(Typography::BODY))
        .line_height(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                spawn(async move {
                    if commands.send(Request::Layout(height)).await.is_err() {
                        actions.failed.set(true);
                    }
                });
            }
        })
        .child(
            control(state, 0)
                .self_start()
                .max_w(relative(1.))
                .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                .py_1()
                .flex_row()
                .justify_between()
                .gap_2()
                .child(text(move || state.with(model_label)).accessible(false))
                .child(dynamic(move || {
                    icon(
                        if state.get().open {
                            crate::icons::Icon::ChevronDown
                        } else {
                            crate::icons::Icon::ChevronRight
                        },
                        state,
                    )
                }))
                .child(
                    native(&target_name(0))
                        .absolute()
                        .inset_0()
                        .prop("state", move || state.with(trigger_stamp))
                        .prop("label", move || {
                            serde_json::Value::from(state.with(|s| {
                                format!(
                                    "{}: {}",
                                    vega_i18n::t("settings.defaults.model"),
                                    model_label(s)
                                )
                            }))
                        })
                        .prop("selected", move || {
                            serde_json::Value::from(state.get().open)
                        })
                        .prop("enabled", move || {
                            serde_json::Value::from(
                                state.with(|s| s.visible && s.enabled) && !actions.pending.get(),
                            )
                        })
                        .prop("focus-request", move || {
                            state.with(|s| serde_json::Value::from(s.restore.then_some(s.epoch)))
                        })
                        .on_native_event(move |event: &NativeEvent| {
                            let s = state.get_untracked();
                            if &*event.name == "focusReady"
                                && s.restore
                                && event.data == serde_json::json!([s.epoch, trigger_stamp(&s)])
                            {
                                let commands = focus_commands.clone();
                                spawn(async move {
                                    let _ = commands.send(Request::FocusReady(s.epoch)).await;
                                });
                            }

                            if &*event.name == "activate"
                                && event.data == trigger_stamp(&state.get_untracked())
                            {
                                actions.send(toggle.clone(), Action::Toggle);
                            }
                        }),
                ),
        )
        .child(when(
            move || state.get().open && state.get().enabled,
            move || {
                let commands = commands.clone();
                view()
                    .gap_1()
                    .child(when(
                        move || state.get().models.is_empty(),
                        move || {
                            text(move || {
                                state.with(|_| vega_i18n::t("settings.model.none_available"))
                            })
                            .text_color(move || {
                                crate::nsh::color(state.get().colors.text_secondary)
                            })
                        },
                    ))
                    .child(each(
                        move || {
                            state
                                .get()
                                .models
                                .iter()
                                .cloned()
                                .enumerate()
                                .collect::<Vec<_>>()
                        },
                        |(index, _)| *index,
                        move |item, _| {
                            let commands = commands.clone();
                            let slot = item.get_untracked().0 + 1;
                            control(state, slot)
                                .py_1()
                                .flex_row()
                                .items_center()
                                .bg(move || {
                                    crate::nsh::color(if state.get().current == item.get().1 {
                                        state.get().colors.bg_active
                                    } else {
                                        state.get().colors.bg_elevated
                                    })
                                })
                                .text_color(move || {
                                    crate::nsh::color(if state.get().current == item.get().1 {
                                        state.get().colors.brand_primary
                                    } else {
                                        state.get().colors.text_primary
                                    })
                                })
                                .justify_between()
                                .child(text(move || item.get().1).accessible(false))
                                .child(when(
                                    move || state.get().current == item.get().1,
                                    move || icon(crate::icons::Icon::Check, state),
                                ))
                                .child(
                                    native(&target_name(slot))
                                        .absolute()
                                        .inset_0()
                                        .prop("state", move || state.with(trigger_stamp))
                                        .prop("label", move || {
                                            serde_json::Value::from(item.get().1)
                                        })
                                        .prop("selected", move || {
                                            serde_json::Value::from(
                                                state.get().current == item.get().1,
                                            )
                                        })
                                        .prop("enabled", move || {
                                            serde_json::Value::from(
                                                state.with(|s| s.open && s.enabled && s.visible)
                                                    && !actions.pending.get(),
                                            )
                                        })
                                        .on_native_event(move |ev: &NativeEvent| {
                                            if &*ev.name == "activate"
                                                && ev.data == trigger_stamp(&state.get_untracked())
                                            {
                                                actions.send(
                                                    commands.clone(),
                                                    Action::Select(item.get_untracked().1),
                                                );
                                            }
                                        }),
                                )
                        },
                    ))
            },
        ))
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}
