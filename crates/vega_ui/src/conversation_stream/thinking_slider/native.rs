use super::*;
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::element::{Element as _, ResponderEvent, TouchClaim};
use nsh_core::prelude as ns;
use std::rc::Rc;
use vega_theme::ThemeColors;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) model: ThinkingSliderModel,
    pub(crate) model_name: String,
}

impl Snapshot {
    pub(crate) fn position_label(&self, index: usize) -> String {
        if self.model.shows_off() && index == 0 {
            vega_i18n::t(OFF_LABEL).to_owned()
        } else {
            self.model
                .tiers()
                .get(index.saturating_sub(usize::from(self.model.shows_off())))
                .map(|tier| tier_display_label(tier))
                .unwrap_or_default()
        }
    }
}

pub(crate) fn position_for_x(x: f32, count: usize) -> Option<usize> {
    if !x.is_finite() || count == 0 {
        return None;
    }
    if count == 1 {
        return Some(0);
    }
    let half = THINKING_KNOB_DIAMETER / 2.;
    let travel = THINKING_TRACK_WIDTH - THINKING_KNOB_DIAMETER;
    let fraction = ((x - half) / travel).clamp(0., 1.);
    Some(((fraction * (count - 1) as f32).round() as usize).min(count - 1))
}

pub(crate) struct Actions {
    pub(crate) pointer: Rc<dyn Fn(usize)>,
    pub(crate) accessibility: Rc<dyn Fn(usize)>,
}

pub(crate) fn track(
    snapshot: &Snapshot,
    colors: ThemeColors,
    light: bool,
    enabled: bool,
    focused: bool,
    target: ns::ElementRef,
    actions: Actions,
) -> ns::AnyElement {
    let count = snapshot.model.dot_count();
    let initial = snapshot.model.selected_position();
    let selected = ns::signal(initial);
    let active = ns::signal(false);
    let origin = ns::signal(0f32);
    let update = move |x: f32| {
        if enabled && let Some(position) = position_for_x(x, count) {
            selected.set(Some(position));
        }
    };
    let finish = actions.pointer.clone();
    let click = actions.pointer;
    let accessible = actions.accessibility;
    let title = snapshot.model.label();
    let value = nsh_core::AccessibilityValue {
        min: Some(0.),
        max: count.checked_sub(1).map(|index| index as f32),
        now: snapshot.model.selected_position().map(|index| index as f32),
        text: Some(title.clone().into()),
    };
    let strongest = snapshot.model.strongest_position();
    let mut track = ns::view()
        .element_ref(&target)
        .relative()
        .w(ns::px(THINKING_TRACK_WIDTH))
        .h(ns::px(THINKING_TRACK_HEIGHT))
        .flex_shrink_0()
        .rounded_full()
        .border(ns::px(1.))
        .border_color(crate::nsh::color(if focused {
            colors.accent
        } else {
            colors.border_subtle
        }))
        .overflow_hidden()
        .bg(crate::nsh::color(if light {
            TRACK_UNFILLED
        } else {
            colors.border_subtle
        }))
        .accessible(true)
        .role(nsh_core::AccessibilityRole::Adjustable)
        .label(title)
        .disabled(!enabled)
        .accessibility_value(Some(value))
        .accessibility_state(nsh_core::AccessibilityState {
            disabled: !enabled,
            ..Default::default()
        })
        .accessibility_actions(vec![
            nsh_core::AccessibilityActionDef {
                name: "increment".into(),
                label: None,
            },
            nsh_core::AccessibilityActionDef {
                name: "decrement".into(),
                label: None,
            },
        ])
        .on_accessibility_action(move |name: &str| {
            if !enabled || count == 0 {
                return;
            }
            let current = selected.get_untracked().unwrap_or(0);
            let next = match name {
                "increment" => (current + 1).min(count - 1),
                "decrement" => current.saturating_sub(1),
                _ => return,
            };
            accessible(next);
        })
        .claim_touches(TouchClaim::on_start())
        .keep_touches(true)
        .on_responder_grant(move |event: &ResponderEvent| {
            if enabled {
                active.set(true);
                origin.set(event.location.x.0);
                update(event.location.x.0);
            }
        })
        .on_responder_move(move |event: &ResponderEvent| {
            if active.get_untracked() {
                update(origin.get_untracked() + event.gesture.delta.x.0);
            }
        })
        .on_responder_release(move |event: &ResponderEvent| {
            if active.get_untracked() {
                active.set(false);
                let x = origin.get_untracked() + event.gesture.delta.x.0;
                update(x);
                if let Some(index) = position_for_x(x, count) {
                    finish(index);
                }
            }
        })
        .on_responder_terminate(move |_: &ResponderEvent| {
            active.set(false);
            selected.set(initial);
        })
        .on_press(move |event: &nsh_core::PressEvent| {
            if enabled
                && !active.get_untracked()
                && let Some(index) = position_for_x(event.position.x.0, count)
            {
                click(index);
            }
        });
    let fill = ns::view()
        .absolute()
        .left(ns::px(0.))
        .top(ns::px(0.))
        .h(ns::px(THINKING_TRACK_HEIGHT))
        .w(move || {
            ns::px(
                selected
                    .get()
                    .map_or(0., |index| fill_effective_width(index, count)),
            )
        })
        .rounded_full()
        .pointer_events(nsh_core::PointerEvents::None)
        .bg(crate::nsh::color(TRACK_FILL_FLAT));
    track = track.child(fill).child(ns::dynamic(move || {
        if selected.get().is_some_and(|index| Some(index) == strongest) {
            ns::view()
                .absolute()
                .left(ns::px(0.))
                .top(ns::px(0.))
                .w(ns::px(THINKING_TRACK_WIDTH))
                .h(ns::px(THINKING_TRACK_HEIGHT))
                .rounded_full()
                .bg_linear_to_r()
                .gradient_from(crate::nsh::color(TRACK_GRADIENT[0].1))
                .gradient_via(crate::nsh::color(TRACK_GRADIENT[1].1))
                .gradient_via_position(ns::Percentage(TRACK_GRADIENT[1].0))
                .gradient_to(crate::nsh::color(TRACK_GRADIENT[2].1))
                .pointer_events(nsh_core::PointerEvents::None)
                .into_element()
        } else {
            ns::view().into_element()
        }
    }));
    for index in 0..count {
        track = track.child(
            ns::view()
                .absolute()
                .left(ns::px(
                    dot_center_offset(index, count) - THINKING_DOT_DIAMETER / 2.,
                ))
                .top(ns::px((THINKING_TRACK_HEIGHT - THINKING_DOT_DIAMETER) / 2.))
                .size(ns::px(THINKING_DOT_DIAMETER))
                .rounded_full()
                .pointer_events(nsh_core::PointerEvents::None)
                .bg(move || {
                    crate::nsh::color(
                        if selected.get().is_some_and(|position| index <= position) {
                            TRACK_DOT_FILLED
                        } else {
                            colors.text_tertiary
                        },
                    )
                }),
        );
    }
    track
        .child(ns::dynamic(move || {
            let Some(index) = selected.get() else {
                return ns::view().into_element();
            };
            ns::view()
                .absolute()
                .left(ns::px(
                    dot_center_offset(index, count) - THINKING_KNOB_DIAMETER / 2.,
                ))
                .top(ns::px(0.))
                .size(ns::px(THINKING_KNOB_DIAMETER))
                .rounded_full()
                .bg(crate::nsh::color(colors.bg_elevated))
                .pointer_events(nsh_core::PointerEvents::None)
                .into_element()
        }))
        .into_element()
}
