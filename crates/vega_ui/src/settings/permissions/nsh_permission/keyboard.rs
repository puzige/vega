use super::*;
use crate::settings::nsh_control::{FocusTarget, Semantics};
use serde_json::Value;

pub(super) struct Keyboard {
    targets: Vec<(Action, FocusTarget)>,
    focused: Option<Action>,
    _subscriptions: Vec<gpui_kit::Subscription>,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Toggle => "permission-picker",
            Self::Select(mode) => mode,
            Self::Cancel => "permission-cancel",
            Self::Reload => "permission-reload",
        }
    }

    pub(super) fn requested(focus: PermissionFocus) -> Self {
        match focus {
            PermissionFocus::Picker => Self::Toggle,
            PermissionFocus::Cancel => Self::Cancel,
            PermissionFocus::Reload => Self::Reload,
        }
    }

    fn selected(self, s: &Snapshot) -> bool {
        match self {
            Self::Toggle => s.open,
            Self::Select(mode) => s.mode == mode,
            _ => false,
        }
    }

    fn label(self, s: &Snapshot) -> String {
        use crate::settings::permission_mode_label;
        match self {
            Self::Toggle => format!(
                "{}: {}",
                vega_i18n::t("settings.defaults.permission_mode"),
                permission_mode_label(&s.mode)
            ),
            Self::Select(mode) if mode == s.mode => vega_i18n::t_args(
                "settings.selection.selected",
                &[("label", permission_mode_label(mode))],
            ),
            Self::Select(mode) => permission_mode_label(mode).to_owned(),
            Self::Cancel => vega_i18n::t("settings.action.cancel").to_owned(),
            Self::Reload => vega_i18n::t("settings.permission.reload").to_owned(),
        }
    }
}

fn stamp(s: &Snapshot) -> Value {
    let phase = match s.phase {
        PermissionPhase::Idle => (0, 0),
        PermissionPhase::Preparing(i) => (1, i),
        PermissionPhase::Saving(i) => (2, i),
        PermissionPhase::Reloading(i) => (3, i),
    };
    serde_json::json!([
        s.epoch,
        s.mode,
        s.permission_revision,
        s.operation,
        phase,
        s.error.map(error_key),
        s.open,
        s.visible,
        s.enabled,
        s.reload_enabled
    ])
}

impl Keyboard {
    pub(super) fn new(
        surface: &Surface<Snapshot, Request>,
        window: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Self {
        let mut targets = Vec::new();
        let mut subscriptions = Vec::new();
        for action in std::iter::once(Action::Toggle)
            .chain(crate::settings::PERMISSION_MODES.map(Action::Select))
            .chain([Action::Cancel, Action::Reload])
        {
            let target = FocusTarget::new(cx);
            let owner = cx.weak_entity();
            target.register(
                surface.entity().read(cx),
                action.name(),
                match action {
                    Action::Toggle => Semantics::Disclosure,
                    Action::Select(_) => Semantics::Toggle,
                    _ => Semantics::Button,
                },
                move |displayed, cx| {
                    let Some(owner) = owner.upgrade() else {
                        return false;
                    };
                    let settings = owner.read(cx);
                    let Some(page) = &settings.default_permission.presentation else {
                        return false;
                    };
                    let current = settings.permission_snapshot(cx, page.id.clone());
                    current.visible && allowed(&current, action) && *displayed == stamp(&current)
                },
            );
            subscriptions.push(cx.on_focus(&target.handle, window, move |settings, _, cx| {
                if let Some(page) = &mut settings.default_permission.presentation {
                    page.keyboard.focused = Some(action);
                    cx.notify();
                }
            }));
            subscriptions.push(cx.on_blur(&target.handle, window, move |settings, _, cx| {
                if let Some(page) = &mut settings.default_permission.presentation
                    && page.keyboard.focused == Some(action)
                {
                    page.keyboard.focused = None;
                    cx.notify();
                }
            }));
            targets.push((action, target));
        }
        subscriptions.push(
            cx.observe_window_activation(window, |settings, window, cx| {
                if window.is_window_active()
                    && let Some((seq, focus)) = settings.default_permission.nsh_focus
                {
                    settings.apply_permission_focus(Action::requested(focus), seq, window, cx);
                }
            }),
        );
        Self {
            targets,
            focused: None,
            _subscriptions: subscriptions,
        }
    }

    fn handle(&self, action: Action, current: &Snapshot) -> Option<&gpui_kit::FocusHandle> {
        let (_, target) = self.targets.iter().find(|(a, _)| *a == action)?;
        (current.visible && allowed(current, action) && target.placed_for(&stamp(current)))
            .then_some(&target.handle)
    }

    pub(super) fn focused(&self) -> Option<Action> {
        self.focused
    }
}

impl SettingsView {
    pub(in crate::settings) fn permission_trigger_handle(
        &self,
        cx: &App,
    ) -> Option<&gpui_kit::FocusHandle> {
        let page = self.default_permission.presentation.as_ref()?;
        page.keyboard.handle(
            Action::Toggle,
            &self.permission_snapshot(cx, page.id.clone()),
        )
    }

    pub(super) fn apply_permission_focus(
        &mut self,
        action: Action,
        seq: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(page) = &self.default_permission.presentation else {
            return false;
        };
        let current = self.permission_snapshot(cx, page.id.clone());
        if page.restored_focus == Some(seq)
            || !current
                .focus
                .is_some_and(|(n, f)| n == seq && Action::requested(f) == action)
            || !window.is_window_active()
            || !page.surface.focus_handle(cx).is_focused(window)
        {
            return false;
        }
        let Some(handle) = page.keyboard.handle(action, &current) else {
            return false;
        };
        window.focus(handle, cx);
        if let Some(page) = &mut self.default_permission.presentation {
            page.restored_focus = Some(seq);
        }
        true
    }
}

pub(super) fn target(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
) -> AnyElement {
    let state = actions.state;
    native(action.name())
        .absolute()
        .inset_0()
        .prop("state", move || stamp(&state.get()))
        .prop("selected", move || {
            Value::from(action.selected(&state.get()))
        })
        .prop("label", move || Value::from(action.label(&state.get())))
        .prop("enabled", move || {
            let s = state.get();
            Value::from(s.visible && allowed(&s, action) && !actions.pending.get())
        })
        .prop("focus-request", move || {
            Value::from(
                state
                    .get()
                    .focus
                    .and_then(|(seq, f)| (Action::requested(f) == action).then_some(seq)),
            )
        })
        .on_native_event(move |event: &NativeEvent| match &*event.name {
            "activate" if event.data == stamp(&state.get_untracked()) => {
                actions.send(commands.clone(), action)
            }
            "focusReady" => {
                let current = state.get_untracked();
                if let Some((seq, f)) = current.focus
                    && Action::requested(f) == action
                    && event.data == serde_json::json!([seq, stamp(&current)])
                {
                    let commands = commands.clone();
                    spawn(async move {
                        let _ = commands.send(Request::FocusReady(action, seq)).await;
                    });
                }
            }
            _ => {}
        })
        .into_element()
}
