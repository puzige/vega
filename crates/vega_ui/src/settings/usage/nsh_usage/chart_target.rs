use super::{
    Action, ProjectionId, Request, SettingsView, Snapshot, Surface, accepts, day_at, day_count,
};
use gpui_kit::{prelude::*, *};
use nsh_platform_gpui::NativeComponent;
use std::{cell::Cell, rc::Rc};

pub(super) fn state_key(snapshot: &Snapshot) -> serde_json::Value {
    serde_json::json!([
        snapshot.revision,
        snapshot.range,
        snapshot.mode,
        snapshot.selected
    ])
}

pub(super) const NAME: &str = "usage-trend-keyboard";

pub(super) fn register(
    surface: &Surface<Snapshot, Request>,
    owner: WeakEntity<SettingsView>,
    id: ProjectionId,
    focus: FocusHandle,
    cx: &App,
) {
    surface.entity().read(cx).register_component(
        NAME,
        Rc::new(move |_| {
            Box::new(ChartTarget {
                owner: owner.clone(),
                id: id.clone(),
                focus: focus.clone(),
                bounds: Rc::default(),
                rendered_state: None,
            })
        }),
    );
}

struct ChartTarget {
    owner: WeakEntity<SettingsView>,
    id: ProjectionId,
    focus: FocusHandle,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    rendered_state: Option<serde_json::Value>,
}

impl NativeComponent for ChartTarget {
    fn placed(&mut self, bounds: Option<Bounds<Pixels>>) {
        self.bounds.set(bounds);
    }
    fn focus_handle(&self) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
    fn set_prop(
        &mut self,
        name: &str,
        value: Option<&serde_json::Value>,
        _: &mut Window,
        _: &mut App,
    ) {
        if name == "state" {
            self.rendered_state = value.cloned();
        }
    }
    fn render(&mut self, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let owner = self.owner.upgrade()?;
        let observed = owner.read(cx).usage_snapshot(cx, self.id.clone());
        if !observed.visible
            || day_count(&observed) == 0
            || self.rendered_state.as_ref() != Some(&state_key(&observed))
        {
            return None;
        }
        let focus = self.focus.clone();
        let click_focus = focus.clone();
        let keyboard_focus = focus.clone();
        let bounds = self.bounds.clone();
        let click_owner = self.owner.clone();
        let click_observed = observed.clone();
        let action_focus = focus.clone();
        let ax_owner = self.owner.clone();
        let ax_id = self.id.clone();
        let focus_chart = move |window: &mut Window, cx: &mut App| {
            let Some(owner) = ax_owner.upgrade() else {
                return;
            };
            let settings = owner.read(cx);
            let Some(page) = &settings.usage.presentation else {
                return;
            };
            let current = settings.usage_snapshot(cx, page.id.clone());
            if page.id.matches(&ax_id)
                && current.visible
                && day_count(&current) > 0
                && current
                    .dashboard
                    .as_ref()
                    .is_some_and(|data| data.lifetime.calls > 0)
            {
                window.focus(&action_focus, cx);
            }
        };
        let ax_focus = focus_chart.clone();
        Some(
            div()
                .id("usage-daily-trend")
                .size_full()
                .track_focus(&focus)
                .role(Role::Button)
                .aria_label(vega_i18n::t("settings.usage.daily_model_tokens"))
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    let Some(bounds) = bounds.get().filter(|b| b.contains(&event.position)) else {
                        return;
                    };
                    let Some(index) = day_at(
                        (event.position.x - bounds.origin.x).into(),
                        bounds.size.width.into(),
                        day_count(&click_observed),
                    ) else {
                        return;
                    };
                    let accepted = click_owner
                        .update(cx, |this, cx| {
                            this.select_chart_day(&click_observed, index, cx)
                        })
                        .unwrap_or(false);
                    if accepted {
                        window.focus(&click_focus, cx);
                        cx.stop_propagation();
                    }
                })
                .on_a11y_action(AccessibleAction::Focus, move |_, w, cx| ax_focus(w, cx))
                .on_a11y_action(AccessibleAction::Click, move |_, w, cx| focus_chart(w, cx))
                .capture_key_down(move |event, w, cx| {
                    if !keyboard_focus.is_focused(w)
                        || event.keystroke.modifiers != Default::default()
                    {
                        return;
                    }
                    let last = day_count(&observed) - 1;
                    let current = observed.selected.unwrap_or(last).min(last);
                    let index = match event.keystroke.key.as_str() {
                        "left" => current.saturating_sub(1),
                        "right" => (current + 1).min(last),
                        "home" => 0,
                        "end" => last,
                        _ => return,
                    };
                    if owner.update(cx, |this, cx| this.select_chart_day(&observed, index, cx)) {
                        cx.stop_propagation();
                    }
                })
                .into_any_element(),
        )
    }
}

impl SettingsView {
    fn select_chart_day(
        &mut self,
        observed: &Snapshot,
        index: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(page) = &self.usage.presentation else {
            return false;
        };
        if !page.id.matches(&observed.id)
            || !accepts(
                &self.usage_snapshot(cx, page.id.clone()),
                observed,
                Action::Day(index),
            )
        {
            return false;
        }
        self.usage.selected_day = Some(index);
        cx.notify();
        true
    }
}
