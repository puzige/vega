use super::super::nsh_control::{FocusTarget, Semantics};
use super::{Action, Actions, Request, SettingsView, Snapshot};
use gpui_kit::{App, WeakEntity};
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

pub(super) fn available(target: &FocusTarget, snapshot: &Snapshot, action: Action) -> bool {
    (matches!(action, Action::Sidebar) || snapshot.enabled)
        && target.placed_for(&state_key(snapshot, action))
}

pub(super) fn register(
    target: &FocusTarget,
    view: &nsh_platform_gpui::AppView,
    action: Action,
    owner: WeakEntity<SettingsView>,
) {
    let semantics = match action {
        Action::Sidebar => Semantics::Switch,
        Action::Language(_) => Semantics::Toggle,
        Action::Theme(_) => Semantics::Selection,
    };
    target.register(
        view,
        action.component(),
        semantics,
        move |displayed, cx: &App| {
            let Some(owner) = owner.upgrade() else {
                return false;
            };
            let settings = owner.read(cx);
            let Some(page) = &settings.appearance_presentation else {
                return false;
            };
            let current = settings.appearance_snapshot(cx, page.id.clone());
            settings.appearance_visible(cx)
                && (matches!(action, Action::Sidebar) || current.enabled)
                && *displayed == state_key(&current, action)
        },
    );
}
