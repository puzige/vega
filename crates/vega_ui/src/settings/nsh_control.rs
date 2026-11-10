use gpui_kit::{prelude::*, *};
use nsh_platform_gpui::{ComponentContext, NativeComponent};
use serde_json::Value;

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

    pub(super) fn placed_for(&self, stamp: &Value) -> bool {
        self.placed_state.borrow().as_ref() == Some(stamp)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Semantics {
    Selection,
    Toggle,
    Switch,
    Button,
    Disclosure,
}

impl FocusTarget {
    pub(super) fn register(
        &self,
        view: &nsh_platform_gpui::AppView,
        name: &str,
        semantics: Semantics,
        valid: impl Fn(&Value, &App) -> bool + 'static,
    ) {
        let valid: Validator = std::rc::Rc::new(valid);
        let target = self.clone();
        let registered_name: SharedString = name.to_owned().into();
        view.register_component(
            name,
            std::rc::Rc::new(move |context| {
                Box::new(Target {
                    name: registered_name.clone(),
                    semantics,
                    valid: valid.clone(),
                    context,
                    target: target.clone(),
                    focus_request: None,
                    sent_focus: None,
                    displayed: None,
                    label: String::new(),
                    selected: false,
                    enabled: false,
                })
            }),
        );
    }
}

type Validator = std::rc::Rc<dyn Fn(&Value, &App) -> bool>;

struct Target {
    name: SharedString,
    semantics: Semantics,
    valid: Validator,
    focus_request: Option<u64>,
    sent_focus: Option<u64>,
    displayed: Option<Value>,
    context: ComponentContext,
    target: FocusTarget,
    label: String,
    selected: bool,
    enabled: bool,
}

impl NativeComponent for Target {
    fn placed(&mut self, bounds: Option<Bounds<Pixels>>) {
        let state = self
            .displayed
            .as_ref()
            .filter(|_| self.enabled && bounds.is_some_and(|b| !b.is_empty()));
        let changed = self.target.placed_state.borrow().as_ref() != state;
        if changed {
            *self.target.placed_state.borrow_mut() = state.cloned();
        }
        if let (Some(request), Some(state)) = (self.focus_request, state)
            && (changed || self.sent_focus != Some(request))
        {
            self.sent_focus = Some(request);
            self.context
                .emit("focusReady", serde_json::json!([request, state]));
        }
    }

    fn focus_handle(&self) -> Option<FocusHandle> {
        self.enabled.then(|| self.target.handle.clone())
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>, _: &mut Window, _: &mut App) {
        match name {
            "focus-request" => self.focus_request = value.and_then(Value::as_u64),
            "state" => self.displayed = value.cloned(),
            "label" => self.label = value.and_then(Value::as_str).unwrap_or_default().to_owned(),
            "selected" => self.selected = value.and_then(Value::as_bool).unwrap_or(false),
            "enabled" => self.enabled = value.and_then(Value::as_bool).unwrap_or(false),
            _ => {}
        }
    }

    fn render(&mut self, _: &mut Window, _: &mut App) -> Option<AnyElement> {
        let displayed = self.displayed.as_ref()?;
        let switch = matches!(self.semantics, Semantics::Switch);
        let mut element = div()
            .id(format!(
                "settings-{}-{displayed}-{}",
                self.name, self.enabled
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
        } else if matches!(self.semantics, Semantics::Toggle | Semantics::Disclosure) {
            element = element.aria_toggled(self.selected.into());
            if matches!(self.semantics, Semantics::Disclosure) {
                element = element.aria_expanded(self.selected);
            }
        } else if matches!(self.semantics, Semantics::Selection) {
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
        let validator = self.valid.clone();
        let displayed = self.displayed.clone()?;
        let valid =
            move |window: &Window, cx: &App| window.is_window_active() && validator(&displayed, cx);
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
                .on_click(move |event, w, cx| {
                    if !matches!(event, ClickEvent::Keyboard(_)) {
                        activate(w, cx);
                    }
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
