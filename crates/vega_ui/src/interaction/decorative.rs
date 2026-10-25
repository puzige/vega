use std::time::Duration;

use gpui_kit::{
    App, SharedString, Subscription, Window,
    base::motion::{MotionValue, Transition, transition_with_status},
};

struct DecorativeChannel {
    interrupted: bool,
    _activation_subscription: Subscription,
}

pub fn decorative_transition(
    duration: Duration,
    visible: bool,
    window: &Window,
    cx: &App,
) -> Transition {
    Transition::new(
        if visible && window.is_window_active() && !cx.reduce_motion() {
            duration
        } else {
            Duration::ZERO
        },
    )
}

pub fn decorative_value(
    id: SharedString,
    target: f32,
    duration: Duration,
    visible: bool,
    window: &mut Window,
    cx: &mut App,
) -> MotionValue<f32> {
    seeded_decorative_value(id, (target, None), duration, visible, window, cx)
}

pub(super) fn seeded_decorative_value(
    id: SharedString,
    value: (f32, Option<f32>),
    duration: Duration,
    visible: bool,
    window: &mut Window,
    cx: &mut App,
) -> MotionValue<f32> {
    let (target, seed) = value;
    window.with_element_namespace(id, |window| {
        let channel = window.use_keyed_state("decorative-lifecycle", cx, |window, cx| {
            let activation_subscription = cx.observe_window_activation(
                window,
                |state: &mut DecorativeChannel, window, cx| {
                    if !window.is_window_active() {
                        state.interrupted = true;
                    }
                    cx.notify();
                },
            );
            DecorativeChannel {
                interrupted: false,
                _activation_subscription: activation_subscription,
            }
        });
        let interrupted = channel.update(cx, |state, _| std::mem::take(&mut state.interrupted));
        let policy = decorative_transition(
            if interrupted {
                Duration::ZERO
            } else {
                duration
            },
            visible,
            window,
            cx,
        );
        if !interrupted
            && visible
            && window.is_window_active()
            && !cx.reduce_motion()
            && !duration.is_zero()
            && let Some(seed) = seed
        {
            transition_with_status(
                "decorative-sampler",
                seed,
                Transition::new(Duration::ZERO),
                window,
                cx,
            );
        }
        transition_with_status("decorative-sampler", target, policy, window, cx)
    })
}
