//! Window-level navigation actions and shared sidebar/collapsed controls.
use crate::interaction::InteractionExt;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonCustomVariant, ButtonVariants},
};
use gpui_kit::{prelude::*, *};
use vega_theme::{Layout, theme};
actions!(
    navigation,
    [
        NavigateBack,
        NavigateForward,
        EditorNavigateBack,
        EditorNavigateForward
    ]
);
/// Root-published navigation availability. Contains no routes or draft text.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct NavigationState {
    /// Whether a backward traversal is available.
    pub back: bool,
    /// Whether a forward traversal is available.
    pub forward: bool,
    /// Short content-free navigation failure or capacity explanation.
    pub error: Option<&'static str>,
}
impl Global for NavigationState {}
/// Install focus-independent shortcuts with more-local text-editor overrides.
pub fn init(cx: &mut App) {
    cx.set_global(NavigationState::default());
    cx.set_global(NavigationControlFocus([
        cx.focus_handle(),
        cx.focus_handle(),
        cx.focus_handle(),
    ]));
    cx.bind_keys([
        KeyBinding::new("cmd-[", NavigateBack, None),
        KeyBinding::new("cmd-]", NavigateForward, None),
        KeyBinding::new("cmd-[", EditorNavigateBack, Some("TextInput")),
        KeyBinding::new("cmd-]", EditorNavigateForward, Some("TextInput")),
    ]);
}
struct NavigationControlFocus([FocusHandle; 3]);
impl Global for NavigationControlFocus {}

/// Render the same actionable controls in either window layout.
pub fn controls(cx: &App, sidebar_visible: bool) -> AnyElement {
    let state = cx
        .try_global::<NavigationState>()
        .cloned()
        .unwrap_or_default();
    let colors = theme(cx).colors;
    div()
        .flex()
        .items_center()
        .gap(px(Layout::TITLEBAR_CONTROL_GAP))
        .child(sidebar_control(cx, sidebar_visible))
        .child(search_control(cx))
        .children(
            [
                (
                    false,
                    state.back,
                    crate::icons::Icon::ArrowLeft,
                    "navigation-back",
                ),
                (
                    true,
                    state.forward,
                    crate::icons::Icon::ArrowRight,
                    "navigation-forward",
                ),
            ]
            .into_iter()
            .map(|(forward, enabled, icon, id)| {
                let focus = cx
                    .try_global::<NavigationControlFocus>()
                    .map(|handles| handles.0[usize::from(forward)].clone())
                    .unwrap_or_else(|| cx.focus_handle())
                    .tab_stop(enabled);
                let name = if forward {
                    "前进 (⌘])"
                } else {
                    "后退 (⌘[)"
                };
                div()
                    .id(id)
                    .track_focus(&focus)
                    .tab_stop(enabled)
                    .tooltip(move |_, cx| crate::icons::tooltip(name, cx))
                    .on_key_down(move |event, window, cx| {
                        if event.keystroke.key == "tab" {
                            cx.stop_propagation();
                            if event.keystroke.modifiers.shift {
                                window.focus_prev(cx);
                            } else {
                                window.focus_next(cx);
                            }
                        }
                        if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            if forward {
                                window.dispatch_action(Box::new(NavigateForward), cx)
                            } else {
                                window.dispatch_action(Box::new(NavigateBack), cx)
                            }
                        }
                    })
                    .debug_selector(move || id.into())
                    .size(px(Layout::TITLEBAR_CONTROL_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .text_color(if enabled {
                        colors.text_secondary
                    } else {
                        colors.text_tertiary
                    })
                    .interactive(colors, enabled, false)
                    .border_2()
                    .border_color(transparent_black())
                    .when(enabled, |button| {
                        button
                            .cursor_pointer()
                            .focus_visible(move |style| style.border_color(colors.accent))
                    })
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        if enabled {
                            window.focus(&focus, cx);
                        }
                    })
                    .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        if enabled {
                            if forward {
                                window.dispatch_action(Box::new(NavigateForward), cx)
                            } else {
                                window.dispatch_action(Box::new(NavigateBack), cx)
                            }
                        }
                    })
                    .child(
                        div()
                            .debug_selector(move || {
                                if forward {
                                    "titlebar-forward-icon".into()
                                } else {
                                    "titlebar-back-icon".into()
                                }
                            })
                            .child(crate::icons::icon(
                                icon,
                                if enabled {
                                    colors.text_secondary
                                } else {
                                    colors.text_tertiary
                                },
                            )),
                    )
            }),
        )
        .into_any_element()
}

/// Shared Search control, adjacent to Sidebar in either window layout.
fn search_control(cx: &App) -> AnyElement {
    let colors = theme(cx).colors;
    div()
        .id("titlebar-search-surface")
        .rounded_md()
        .interactive(colors, true, false)
        .capture_native_button_press()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            Button::new("titlebar-search-button")
                .debug_selector(|| "titlebar-search-button".into())
                .custom(ButtonCustomVariant::new(cx).foreground(colors.text_secondary.into()))
                .small()
                .size(px(Layout::TITLEBAR_CONTROL_SIZE))
                .p_0()
                .accessibility_label("搜索 (⌘K)")
                .tooltip("搜索 (⌘K)")
                .on_click(|_, window, cx| {
                    cx.stop_propagation();
                    window.dispatch_action(Box::new(crate::command_palette::OpenPalette), cx);
                })
                .child(
                    div()
                        .debug_selector(|| "titlebar-search-icon".into())
                        .child(crate::icons::icon(
                            crate::icons::Icon::Search,
                            colors.text_secondary,
                        )),
                ),
        )
        .into_any_element()
}

/// Render only the Sidebar visibility control for callers that need it alone.
///
/// Window titlebars should use [`controls`] so Search and history stay in the
/// shared, stable order.
pub fn sidebar_toggle(cx: &App, sidebar_visible: bool) -> AnyElement {
    sidebar_control(cx, sidebar_visible)
}

/// Shared titlebar control, mounted whether the sidebar is shown or hidden.
fn sidebar_control(cx: &App, sidebar_visible: bool) -> AnyElement {
    let colors = theme(cx).colors;
    let label = if sidebar_visible {
        "隐藏侧栏 (⌘B)"
    } else {
        "显示侧栏 (⌘B)"
    };
    let focus = cx
        .try_global::<NavigationControlFocus>()
        .map(|handles| handles.0[2].clone())
        .unwrap_or_else(|| cx.focus_handle())
        .tab_stop(true);
    div()
        .id("toggle-sidebar")
        .debug_selector(|| "toggle-sidebar".into())
        .track_focus(&focus)
        .tab_stop(true)
        .tooltip(move |_, cx| crate::icons::tooltip(label, cx))
        .size(px(Layout::TITLEBAR_CONTROL_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .interactive(colors, true, false)
        .border_2()
        .border_color(transparent_black())
        .focus_visible(move |style| style.border_color(colors.accent))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            window.focus(&focus, cx);
        })
        .on_mouse_up(MouseButton::Left, |_, window, cx| {
            cx.stop_propagation();
            window.dispatch_action(Box::new(crate::sidebar::ToggleSidebar), cx);
        })
        .on_key_down(|event, window, cx| {
            if event.keystroke.key == "tab" {
                cx.stop_propagation();
                if event.keystroke.modifiers.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
            } else if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                window.dispatch_action(Box::new(crate::sidebar::ToggleSidebar), cx);
            }
        })
        .child(
            div()
                .debug_selector(|| "titlebar-sidebar-icon".into())
                .child(crate::icons::icon(
                    crate::icons::Icon::Sidebar,
                    colors.text_secondary,
                )),
        )
        .into_any_element()
}

/// Weak editor identity and scalar cache budget; this never retains a stream or worker.
#[derive(Clone, PartialEq, Eq)]
pub struct DraftNavigationGuard {
    /// Current editor task identity.
    pub task: String,
    /// Weak composer editor, read at the action boundary (including IME edits).
    pub input: WeakEntity<crate::text_input::TextInput>,
    /// Cached nonempty tasks excluding this live task.
    pub cached_tasks: usize,
    /// Cached UTF-8 bytes excluding this live task.
    pub cached_bytes: usize,
}
impl Global for DraftNavigationGuard {}
/// Reject a visit before database side effects if its departing draft cannot fit.
pub fn allow_task_navigation(target: Option<&str>, cx: &mut App) -> bool {
    let allowed = cx.try_global::<DraftNavigationGuard>().is_none_or(|guard| {
        target == Some(guard.task.as_str())
            || guard.input.upgrade().is_none_or(|input| {
                let text = input.read(cx).text();
                text.is_empty()
                    || (guard.cached_tasks < 100
                        && guard.cached_bytes.saturating_add(text.len()) <= 1024 * 1024)
            })
    });
    if !allowed {
        let mut state = cx
            .try_global::<NavigationState>()
            .cloned()
            .unwrap_or_default();
        state.error = Some("草稿已达容量上限，请先发送或清空当前草稿再导航。");
        cx.set_global(state);
        cx.refresh_windows();
    }
    allowed
}

/// UI mutation fence used by asynchronous route resolvers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskMutationState {
    /// Monotonic revision across mutation dispatch and completion.
    pub epoch: u64,
    /// Mutations whose durable result has not yet been acknowledged.
    pub pending: usize,
}
impl Global for TaskMutationState {}
/// Fence route reads before a task mutation starts.
pub fn begin_task_mutation(cx: &mut App) {
    let mut state = cx
        .try_global::<TaskMutationState>()
        .copied()
        .unwrap_or_default();
    state.epoch = state.epoch.wrapping_add(1);
    state.pending = state.pending.saturating_add(1);
    cx.set_global(state);
}
/// Release one started mutation even when its originating UI entity was dropped.
pub fn finish_task_mutation(cx: &mut App) {
    let mut state = cx
        .try_global::<TaskMutationState>()
        .copied()
        .unwrap_or_default();
    state.epoch = state.epoch.wrapping_add(1);
    state.pending = state.pending.saturating_sub(1);
    cx.set_global(state);
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use gpui_kit::prelude::*;
    use gpui_kit::{Context, KeyDownEvent, KeyUpEvent, Keystroke, Render, Window, div};

    use super::search_control;

    struct SearchControlHarness;

    impl Render for SearchControlHarness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().tab_group().child(search_control(cx))
        }
    }

    #[gpui_kit::test]
    fn shared_search_control_keyboard_activation_dispatches_open_palette(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let activations = Arc::new(AtomicUsize::new(0));
        let observed = activations.clone();
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            crate::init(cx);
            cx.set_global(vega_theme::Theme::light());
            cx.on_action(move |_: &crate::command_palette::OpenPalette, _| {
                observed.fetch_add(1, Ordering::SeqCst);
            });
        });
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(|_| SearchControlHarness))
                .expect("search control window")
        });
        cx.run_until_parked();
        let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.focus_next(cx);
        });
        let keystroke = Keystroke::parse("space").expect("Space keystroke");
        visual.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        visual.simulate_event(KeyUpEvent { keystroke });
        assert_eq!(activations.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod issue296_tests {
    use super::search_control;
    use gpui_kit::{
        Context, IntoElement, Render, TestAppContext, Window, div, prelude::*, px, size,
    };
    use std::{cell::Cell, rc::Rc};

    struct Harness;
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .tab_group()
                .p_4()
                .flex()
                .items_start()
                .child(search_control(cx))
        }
    }

    #[gpui_kit::test]
    fn issue296_native_search_press_keeps_one_pointer_action(cx: &mut TestAppContext) {
        let clicks = Rc::new(Cell::new(0));
        let captured = clicks.clone();
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            crate::init(cx);
            cx.set_global(vega_theme::Theme::light());
            cx.on_action(move |_: &crate::command_palette::OpenPalette, _| {
                captured.set(captured.get() + 1)
            });
        });
        let window = cx.open_window(size(px(400.), px(200.)), |_, _| Harness);
        cx.run_until_parked();
        crate::interaction::tests::assert_native_control_press(
            window,
            cx,
            "titlebar-search-button",
        );
        assert_eq!(clicks.get(), 2);
        let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| window.focus_next(cx));
        for key in ["enter", "space"] {
            let keystroke = gpui_kit::Keystroke::parse(key).expect("key");
            visual.simulate_event(gpui_kit::KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            visual.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        }
        assert_eq!(clicks.get(), 4);
    }

    struct NavigationHarness;
    impl Render for NavigationHarness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .tab_group()
                .p_4()
                .child(super::controls(cx, true))
        }
    }

    #[gpui_kit::test]
    fn issue296_navigation_focus_rings_preserve_icon_bounds(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            crate::init(cx);
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(super::NavigationState {
                back: true,
                forward: true,
                error: None,
            });
        });
        let window = cx.open_window(size(px(400.), px(200.)), |_, _| NavigationHarness);
        cx.run_until_parked();
        let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
        let selectors = [
            "titlebar-sidebar-icon",
            "titlebar-back-icon",
            "titlebar-forward-icon",
        ];
        let original = selectors.map(|selector| visual.debug_bounds(selector).expect("icon"));
        visual.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: gpui_kit::Keystroke::parse("tab").expect("Tab"),
            is_held: false,
            prefer_character_input: false,
        });
        let mut seen = [false; 3];
        for _ in 0..4 {
            visual.update(|window, cx| window.focus_next(cx));
            visual.run_until_parked();
            let focused = visual.update(|window, cx| {
                cx.global::<super::NavigationControlFocus>()
                    .0
                    .each_ref()
                    .map(|focus| focus.is_focused(window))
            });
            for (index, current) in focused.into_iter().enumerate() {
                seen[index] |= current;
            }
            for (selector, bounds) in selectors.into_iter().zip(original) {
                assert_eq!(visual.debug_bounds(selector), Some(bounds));
            }
            if focused.into_iter().any(|current| current) {
                assert!(visual.update(|window, _| {
                    window
                        .painted_quads()
                        .iter()
                        .any(|quad| quad.border_color == vega_theme::LIGHT.accent.into())
                }));
            }
        }
        assert!(seen.into_iter().all(|focused| focused));
        visual.update(|window, cx| {
            cx.set_global(super::NavigationState::default());
            cx.refresh_windows();
            window.blur(cx);
        });
        visual.run_until_parked();
        for _ in 0..4 {
            visual.update(|window, cx| window.focus_next(cx));
            visual.run_until_parked();
            assert!(visual.update(|window, cx| {
                cx.global::<super::NavigationControlFocus>().0[..2]
                    .iter()
                    .all(|focus| !focus.is_focused(window))
            }));
        }
    }
}
