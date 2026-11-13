use super::{Actions, Request, SettingsView, Snapshot, allowed};
use crate::settings::nsh_control::{FocusTarget, Semantics};
use gpui_kit::{Context, FocusHandle};
use nsh_core::prelude::*;
use serde_json::Value;
use tokio::sync::mpsc;
use vega_conversation::types::UpdateRequest;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Control {
    Automatic,
    Check,
    ReleasePage,
    Later,
    Install,
}

#[derive(Clone, Copy)]
pub(super) struct Pointer {
    control: Control,
    hovered: Signal<Option<Value>>,
    pressed: Signal<Option<Value>>,
}
impl Pointer {
    pub(super) fn new(control: Control) -> Self {
        Self {
            control,
            hovered: signal(None),
            pressed: signal(None),
        }
    }
    pub(super) fn hovered(self, s: &Snapshot) -> bool {
        self.hovered
            .with(|v| v.as_ref() == Some(&self.control.stamp(s)))
    }
    pub(super) fn pressed(self, s: &Snapshot) -> bool {
        self.pressed
            .with(|v| v.as_ref() == Some(&self.control.stamp(s)))
    }
}

impl Control {
    pub(super) const ALL: [Self; 5] = [
        Self::Automatic,
        Self::Check,
        Self::ReleasePage,
        Self::Later,
        Self::Install,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Automatic => "settings.updater.automatic",
            Self::Check => "settings.updater.check",
            Self::ReleasePage => "settings.updater.release_page",
            Self::Later => "settings.updater.later",
            Self::Install => "settings.updater.install",
        }
    }

    fn stamp(self, snapshot: &Snapshot) -> Value {
        if self == Self::ReleasePage {
            Value::from(snapshot.help_epoch)
        } else {
            serde_json::json!([snapshot.help_epoch, snapshot.revision, snapshot.automatic])
        }
    }

    fn request(self, snapshot: &Snapshot) -> UpdateRequest {
        match self {
            Self::Automatic => UpdateRequest::Automatic(!snapshot.automatic),
            Self::Check => UpdateRequest::Check,
            Self::ReleasePage => UpdateRequest::ReleasePage,
            Self::Later => UpdateRequest::Later,
            Self::Install => UpdateRequest::Install,
        }
    }

    pub(super) fn enabled(self, snapshot: &Snapshot) -> bool {
        snapshot.help_visible
            && !snapshot.pending
            && allowed(&snapshot.phase, snapshot.automatic, self.request(snapshot))
    }

    pub(super) fn register(
        self,
        view: &nsh_platform_gpui::AppView,
        cx: &Context<SettingsView>,
    ) -> FocusHandle {
        let owner = cx.weak_entity();
        let target = FocusTarget::new(cx);
        target.register(
            view,
            self.label(),
            if self == Self::Automatic {
                Semantics::Switch
            } else {
                Semantics::Button
            },
            move |displayed, cx| {
                let Some(owner) = owner.upgrade() else {
                    return false;
                };
                let settings = owner.read(cx);
                let current = settings.update_snapshot(cx);
                self.enabled(&current) && *displayed == self.stamp(&current)
            },
        );
        target.handle
    }

    pub(super) fn target(
        self,
        actions: Actions,
        commands: mpsc::Sender<Request>,
        pointer: Pointer,
    ) -> AnyElement {
        let state = actions.state;
        native(self.label())
            .absolute()
            .inset_0()
            .prop("state", move || state.with(|s| self.stamp(s)))
            .prop("report-hover", Value::Bool(true))
            .prop("report-press", Value::Bool(true))
            .prop("enabled", move || {
                Value::from(!actions.waiting.get() && state.with(|s| self.enabled(s)))
            })
            .prop("selected", move || {
                Value::from(self == Self::Automatic && state.get().automatic)
            })
            .prop("label", move || {
                Value::from(state.with(|_| vega_i18n::t(self.label())))
            })
            .on_native_event(move |event: &NativeEvent| {
                let snapshot = state.get_untracked();
                let current = self.stamp(&snapshot);
                if matches!(&*event.name, "hover" | "press") && event.data.get(0) == Some(&current)
                {
                    let active = event.data.get(1).and_then(Value::as_bool) == Some(true);
                    if &*event.name == "hover" {
                        if !active {
                            pointer.pressed.set(None);
                        }
                        pointer.hovered.set(active.then_some(current));
                    } else {
                        pointer.pressed.set(active.then_some(current));
                    }
                } else if &*event.name == "disabledPointerDown"
                    && snapshot.help_visible
                    && snapshot.help_open
                    && event.data == current
                {
                    let commands = commands.clone();
                    actions.owner.run(|| {
                        spawn(async move {
                            let _ = commands
                                .send(Request::Help(snapshot.help_epoch, false))
                                .await;
                        })
                    });
                } else if &*event.name == "activate"
                    && self.enabled(&snapshot)
                    && event.data == current
                {
                    actions.send(commands.clone(), self.request(&snapshot));
                }
            })
            .into_element()
    }
}
