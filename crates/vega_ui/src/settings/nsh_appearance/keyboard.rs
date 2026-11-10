use super::{Action, Actions, Request, SettingsView, Snapshot};
use gpui_kit::{prelude::*, *};
use nsh_platform_gpui::{ComponentContext, NativeComponent};
use serde_json::Value;
use tokio::sync::mpsc;

pub(super) const CONTROLS: [Action; 6] = [
    Action::Theme("light"),
    Action::Theme("dark"),
    Action::Theme("system"),
    Action::Language("en"),
    Action::Language("zh-CN"),
    Action::Sidebar,
];

impl Action {
    pub(super) fn component(self) -> &'static str {
        match self {
            Self::Theme(value) | Self::Language(value) => value,
            Self::Sidebar => "appearance-sidebar",
        }
    }
}

fn state_key(snapshot: &Snapshot, action: Action) -> Value {
    let value = match action {
        Action::Theme(_) => Value::from(snapshot.theme.as_str()),
        Action::Language(_) => Value::from(snapshot.language.as_str()),
        Action::Sidebar => Value::from(snapshot.sidebar),
    };
    serde_json::json!([snapshot.epoch, value])
}

pub(super) fn target(
    action: Action,
    actions: Actions,
    commands: mpsc::Sender<Request>,
    label: impl Fn() -> &'static str + 'static,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    native(action.component())
        .absolute()
        .inset_0()
        .prop("state", move || state_key(&actions.state.get(), action))
        .prop("label", move || Value::from(label()))
        .prop("selected", move || {
            Value::from(super::selected(&actions.state.get(), action))
        })
        .prop("enabled", move || {
            Value::from(
                !actions.pending.get()
                    && (matches!(action, Action::Sidebar) || actions.state.get().enabled),
            )
        })
        .on_native_event(move |event: &NativeEvent| {
            if &*event.name == "activate"
                && event.data == state_key(&actions.state.get_untracked(), action)
            {
                actions.send(commands.clone(), action);
            }
        })
        .into_element()
}

#[derive(Clone)]
pub(super) struct FocusTarget {
    pub(super) handle: FocusHandle,
    placed_state: std::rc::Rc<std::cell::RefCell<Option<Value>>>,
}

impl FocusTarget {
    pub(super) fn new(cx: &App) -> Self {
        Self {
            handle: cx.focus_handle().tab_stop(true),
            placed_state: Default::default(),
        }
    }

    pub(super) fn available(&self, snapshot: &Snapshot, action: Action) -> bool {
        (matches!(action, Action::Sidebar) || snapshot.enabled)
            && self.placed_state.borrow().as_ref() == Some(&state_key(snapshot, action))
    }
}

pub(super) struct Target {
    pub(super) owner: WeakEntity<SettingsView>,
    pub(super) displayed: Option<Value>,
    pub(super) context: ComponentContext,
    pub(super) target: FocusTarget,
    pub(super) action: Action,
    pub(super) label: String,
    pub(super) selected: bool,
    pub(super) enabled: bool,
}

impl NativeComponent for Target {
    fn placed(&mut self, bounds: Option<Bounds<Pixels>>) {
        let state = self
            .displayed
            .as_ref()
            .filter(|_| self.enabled && bounds.is_some_and(|b| !b.is_empty()));
        *self.target.placed_state.borrow_mut() = state.cloned();
    }

    fn focus_handle(&self) -> Option<FocusHandle> {
        self.enabled.then(|| self.target.handle.clone())
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>, _: &mut Window, _: &mut App) {
        match name {
            "state" => self.displayed = value.cloned(),
            "label" => self.label = value.and_then(Value::as_str).unwrap_or_default().to_owned(),
            "selected" => self.selected = value.and_then(Value::as_bool).unwrap_or(false),
            "enabled" => self.enabled = value.and_then(Value::as_bool).unwrap_or(false),
            _ => {}
        }
    }

    fn render(&mut self, _: &mut Window, _: &mut App) -> Option<AnyElement> {
        let displayed = self.displayed.as_ref()?;
        let switch = matches!(self.action, Action::Sidebar);
        let mut element = div()
            .id(format!(
                "appearance-{}-{displayed}-{}",
                self.action.component(),
                self.enabled
            ))
            .size_full()
            .role(if switch { Role::Switch } else { Role::Button })
            .aria_label(self.label.clone());
        if switch {
            element = element.aria_toggled(if self.selected {
                Toggled::True
            } else {
                Toggled::False
            });
        } else if matches!(self.action, Action::Language(_)) {
            element = element.aria_toggled(self.selected.into());
        } else {
            element = element.aria_selected(self.selected);
        }
        if !self.enabled {
            return Some(
                element
                    .a11y_synthetic_children(|tree| tree.parent_node().set_disabled())
                    .into_any_element(),
            );
        }
        let focus = self.target.handle.clone();
        let key_focus = focus.clone();
        let ax_focus = focus;
        let owner = self.owner.clone();
        let displayed = self.displayed.clone()?;
        let action = self.action;
        let valid = move |window: &Window, cx: &App| {
            let Some(owner) = owner.upgrade() else {
                return false;
            };
            let settings = owner.read(cx);
            let Some(page) = &settings.appearance_presentation else {
                return false;
            };
            let current = settings.appearance_snapshot(cx, page.id.clone());
            window.is_window_active()
                && settings.appearance_visible(cx)
                && (matches!(action, Action::Sidebar) || current.enabled)
                && displayed == state_key(&current, action)
        };
        let mouse_valid = valid.clone();
        let ax_valid = valid.clone();
        let context = self.context.clone();
        let displayed = self.displayed.clone()?;
        let activate = move |window: &Window, cx: &App| {
            if valid(window, cx) {
                context.emit("activate", displayed.clone());
            }
        };
        let key_activate = activate.clone();
        let ax_activate = activate.clone();
        Some(
            element
                .track_focus(&self.target.handle)
                .capture_any_mouse_down(move |_, w, cx| {
                    if !mouse_valid(w, cx) {
                        w.prevent_default();
                        cx.stop_propagation();
                    }
                })
                .on_click(move |_, w, cx| {
                    activate(w, cx);
                    cx.stop_propagation();
                })
                .on_a11y_action(AccessibleAction::Focus, move |_, w, cx| {
                    if ax_valid(w, cx) {
                        w.focus(&ax_focus, cx);
                    }
                })
                .on_a11y_action(AccessibleAction::Click, move |_, w, cx| ax_activate(w, cx))
                .capture_key_down(move |event, w, cx| {
                    use crate::nsh::keyboard::Key;
                    if key_focus.is_focused(w) && matches!(Key::from_event(event), Key::Activate) {
                        key_activate(w, cx);
                        cx.stop_propagation();
                    }
                })
                .into_any_element(),
        )
    }
}
