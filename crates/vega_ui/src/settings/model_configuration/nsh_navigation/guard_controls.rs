use super::*;
use crate::settings::nsh_control::{FocusTarget, Semantics};
use serde_json::Value;

pub(super) const BUTTONS: [(Action, &str); 3] = [
    (Action::Save, "settings.navigation.save_continue"),
    (Action::Discard, "settings.navigation.discard"),
    (Action::Stay, "settings.navigation.stay"),
];

pub(super) struct Controls {
    pub(super) authority: Authority,
    pub(super) revision: u64,
    pub(super) keyboard: bool,
    pub(super) focused: Option<Action>,
    _focus_events: Vec<gpui_kit::Subscription>,
}

impl Controls {
    pub(super) fn new(
        surface: &Surface<Snapshot, Request>,
        authority: Authority,
        window: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Self {
        let entity = surface.entity();
        let surface_id = entity.entity_id();
        let mut events = Vec::new();
        for (action, key) in BUTTONS {
            let owner = cx.weak_entity();
            let target = FocusTarget::new(cx);
            target.register(entity.read(cx), key, Semantics::Button, move |stamp, cx| {
                let Some(owner) = owner.upgrade() else {
                    return false;
                };
                let settings = owner.read(cx);
                let Some(page) = settings.navigation_page(Pane::Guard) else {
                    return false;
                };
                if page.surface.entity().entity_id() != surface_id {
                    return false;
                }
                let current = settings.navigation_snapshot(Pane::Guard, page.id.clone(), cx);
                stamp.as_u64() == Some(current.guard_revision)
                    && page
                        .guard
                        .as_ref()
                        .is_some_and(|guard| guard.authority == current.authority)
                    && allowed(&current, action)
            });
            events.push(cx.on_focus(&target.handle, window, move |settings, _, cx| {
                if let Some(controls) = settings
                    .settings_navigation_presentation
                    .as_mut()
                    .and_then(|p| p.guard.as_mut())
                {
                    controls.focused = Some(action);
                    cx.notify();
                }
            }));
            events.push(cx.on_blur(&target.handle, window, move |settings, _, cx| {
                if let Some(controls) = settings
                    .settings_navigation_presentation
                    .as_mut()
                    .and_then(|p| p.guard.as_mut())
                    && controls.focused == Some(action)
                {
                    controls.focused = None;
                    cx.notify();
                }
            }));
        }
        Self {
            authority,
            revision: 0,
            keyboard: window.last_input_was_keyboard(),
            focused: None,
            _focus_events: events,
        }
    }
}

pub(super) fn button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
    key: &'static str,
) -> AnyElement {
    let state = actions.state;
    let hovered = signal(None::<u64>);
    let pressed = signal(None::<u64>);
    let enabled = move || !actions.pending.get() && state.with(|s| allowed(s, action));
    view()
        .relative()
        .accessible(false)
        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
        .py_1()
        .items_center()
        .justify_center()
        .line_height(px((Typography::BODY * Typography::BODY_LINE_HEIGHT).round()))
        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_color(move || {
            state.with(|s| {
                crate::nsh::color(if enabled() && s.guard_focus == Some(action) {
                    s.colors.accent
                } else {
                    s.colors.border_subtle
                })
            })
        })
        .bg(move || {
            state.with(|s| {
                crate::nsh::color(if enabled() && pressed.get() == Some(s.guard_revision) {
                    s.colors.bg_active
                } else if enabled() && hovered.get() == Some(s.guard_revision) {
                    s.colors.bg_hover
                } else {
                    s.colors.bg_base.opacity(0.)
                })
            })
        })
        .text_color(move || {
            state.with(|s| {
                crate::nsh::color(if enabled() {
                    s.colors.text_primary
                } else {
                    s.colors.text_tertiary
                })
            })
        })
        .child(text(move || state.with(|_| vega_i18n::t(key))).accessible(false))
        .child(
            native(key)
                .absolute()
                .inset_0()
                .prop("state", move || Value::from(state.get().guard_revision))
                .prop("label", move || {
                    Value::from(state.with(|_| vega_i18n::t(key)))
                })
                .prop("enabled", move || Value::from(enabled()))
                .prop("report-hover", Value::Bool(true))
                .prop("report-press", Value::Bool(true))
                .on_native_event(move |event: &NativeEvent| {
                    let revision = state.get_untracked().guard_revision;
                    match &*event.name {
                        "activate" if event.data.as_u64() == Some(revision) => {
                            actions.send(commands.clone(), action)
                        }
                        "hover" | "press"
                            if event.data.get(0).and_then(Value::as_u64) == Some(revision) =>
                        {
                            let inside = event.data.get(1).and_then(Value::as_bool) == Some(true);
                            if &*event.name == "hover" {
                                hovered.set(inside.then_some(revision));
                                if !inside {
                                    pressed.set(None);
                                }
                            } else {
                                pressed.set(inside.then_some(revision));
                            }
                        }
                        _ => {}
                    }
                }),
        )
        .into_element()
}
