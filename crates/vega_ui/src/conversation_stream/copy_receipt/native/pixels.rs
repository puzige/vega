use super::{Request, Snapshot, Witness};
use crate::{icons::Icon, nsh::OrderedCommands};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::{element::Element as _, prelude as ns};
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use vega_theme::{Layout, Motion, Typography};

pub(super) fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    let command_owner = nsh_core::element::Owner::current();
    let blend = ns::signal(0f32);
    let receipt = ns::signal(None::<std::time::Instant>);
    ns::dynamic(move || {
        let snapshot = state.get();
        let presentation = nsh_core::element::Owner::current();
        let root = ns::element_ref();
        let target = ns::element_ref();
        let hovered = ns::signal(false);
        let pressed = ns::signal(false);
        let colors = snapshot.colors;
        let enabled = snapshot.enabled;
        let focused = snapshot.focused;
        let bridge_hovered = snapshot.hovered;
        let bridge_pressed = snapshot.pressed;
        let revision = snapshot.revision.clone();
        let observed = snapshot.clone();
        let accessible = snapshot.clone();
        let queued = snapshot.clone();
        let pointer_commands = commands.clone();
        let ax_commands = commands.clone();
        let keyboard_commands = commands.clone();
        let activation = snapshot.activation.is_some();
        ns::effect(move || {
            if activation {
                dispatch(
                    queued.clone(),
                    target,
                    root,
                    keyboard_commands.clone(),
                    command_owner,
                    presentation,
                );
            }
        });
        let mut control = ns::view()
            .element_ref(&target)
            .w_full()
            .h_full()
            .flex_row()
            .items_center()
            .justify_center()
            .relative()
            .rounded(ns::px(Layout::MENU_RADIUS))
            .focusable(false)
            .accessible(true)
            .role(nsh_core::AccessibilityRole::Button)
            .label(snapshot.label)
            .disabled(!enabled)
            .accessibility_state(nsh_core::AccessibilityState {
                disabled: !enabled,
                ..Default::default()
            })
            .bg(move || {
                crate::nsh::color(if enabled && (pressed.get() || bridge_pressed || focused) {
                    colors.bg_active
                } else if enabled && (hovered.get() || bridge_hovered) {
                    colors.bg_hover
                } else {
                    colors.bg_hover.opacity(0.)
                })
            })
            .transition_colors()
            .duration(move || {
                if ns::reduce_motion() {
                    0.
                } else {
                    Motion::INTERACTION_DURATION_MS as f32
                }
            })
            .border(ns::px(1.))
            .border_color(crate::nsh::color(if focused {
                colors.accent
            } else {
                colors.bg_hover.opacity(0.)
            }))
            .on_layout(move |_: &ns::Bounds<ns::Points>| {
                revision.fetch_add(1, Ordering::AcqRel);
            })
            .on_mouse_enter(move |_: &nsh_core::MouseEvent| hovered.set(true))
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                hovered.set(false);
                pressed.set(false);
            })
            .on_press_down(move || pressed.set(true))
            .on_press_up(move || pressed.set(false))
            .on_press(move || {
                if enabled {
                    dispatch(
                        observed.clone(),
                        target,
                        root,
                        pointer_commands.clone(),
                        command_owner,
                        presentation,
                    );
                }
            })
            .accessibility_actions(if enabled {
                vec![nsh_core::AccessibilityActionDef {
                    name: "activate".into(),
                    label: None,
                }]
            } else {
                Vec::new()
            })
            .on_accessibility_action(move |name: &str| {
                if enabled && name == "activate" {
                    dispatch(
                        accessible.clone(),
                        target,
                        root,
                        ax_commands.clone(),
                        command_owner,
                        presentation,
                    );
                }
            })
            .tooltip(
                ns::text(snapshot.label)
                    .text_size(ns::px(Typography::METADATA))
                    .text_color(crate::nsh::color(colors.text_primary))
                    .bg(crate::nsh::color(colors.bg_elevated))
                    .px_2()
                    .py_1()
                    .rounded(ns::px(Layout::MENU_RADIUS))
                    .into_element(),
            );
        if snapshot.receipt.is_some() {
            control = control.hint(vega_i18n::t("ui.copy_requested"));
        }
        if receipt.get_untracked() != snapshot.receipt {
            receipt.set(snapshot.receipt);
            blend.set(if snapshot.receipt.is_some() && ns::reduce_motion() {
                1.
            } else {
                0.
            });
            if let Some(deadline) = snapshot.receipt {
                command_owner.run(|| {
                    ns::spawn(async move {
                        ns::next_frame().await;
                        if receipt.get_untracked() == Some(deadline) {
                            blend.set(1.);
                        }
                    })
                });
            }
        }
        control = control.child(
            ns::view()
                .size(ns::px(Layout::ICON_SIZE))
                .relative()
                .pointer_events(nsh_core::PointerEvents::None)
                .child(
                    ns::view()
                        .absolute()
                        .inset_0()
                        .transition_opacity()
                        .duration(move || {
                            if ns::reduce_motion() {
                                0.
                            } else {
                                Motion::COPY_SWAP_MS as f32
                            }
                        })
                        .opacity(move || 1. - blend.get())
                        .child(icon(Icon::Copy, colors.text_secondary)),
                )
                .child(
                    ns::view()
                        .absolute()
                        .inset_0()
                        .transition_opacity()
                        .duration(move || {
                            if ns::reduce_motion() {
                                0.
                            } else {
                                Motion::COPY_SWAP_MS as f32
                            }
                        })
                        .opacity(move || blend.get())
                        .child(icon(Icon::Check, colors.text_secondary)),
                ),
        );
        ns::view()
            .element_ref(&root)
            .w_full()
            .h_full()
            .child(control)
            .child(ns::dynamic(move || {
                if bridge_hovered {
                    ns::view()
                        .absolute()
                        .top(ns::px(Layout::COPY_ACTION_SIZE))
                        .right(ns::px(0.))
                        .pointer_events(nsh_core::PointerEvents::None)
                        .bg(crate::nsh::color(colors.bg_elevated))
                        .rounded(ns::px(Layout::MENU_RADIUS))
                        .px_2()
                        .py_1()
                        .child(
                            ns::text(snapshot.label)
                                .text_size(ns::px(Typography::METADATA))
                                .text_color(crate::nsh::color(colors.text_primary)),
                        )
                        .into_element()
                } else {
                    ns::view().into_element()
                }
            }))
            .into_element()
    })
    .into_element()
}

fn dispatch(
    observed: Snapshot,
    target: nsh_core::element::ElementRef,
    root: nsh_core::element::ElementRef,
    commands: OrderedCommands<Request>,
    command_owner: nsh_core::element::Owner,
    presentation: nsh_core::element::Owner,
) {
    command_owner.run(|| {
        ns::spawn(async move {
            let revision = observed.revision.load(Ordering::Acquire);
            let (Some(control), Some(container)) = (target.measure().await, root.measure().await)
            else {
                return;
            };
            let control = rectangle(control);
            let root = rectangle(container);
            if presentation.is_live()
                && observed.revision.load(Ordering::Acquire) == revision
                && contained(control, root)
            {
                commands.send(Request::Activate(
                    Box::new(observed),
                    Witness {
                        revision,
                        control,
                        root,
                    },
                ));
            }
        })
    });
}

fn rectangle(measurement: nsh_core::Measurement) -> [f32; 4] {
    [
        measurement.page.x.0,
        measurement.page.y.0,
        measurement.frame.size.width.0,
        measurement.frame.size.height.0,
    ]
}

pub(super) fn contained(control: [f32; 4], root: [f32; 4]) -> bool {
    control
        .iter()
        .chain(root.iter())
        .all(|value| value.is_finite())
        && control[2] > 0.
        && control[3] > 0.
        && root[2] > 0.
        && root[3] > 0.
        && control[0] >= root[0]
        && control[1] >= root[1]
        && control[0] + control[2] <= root[0] + root[2] + 0.5
        && control[1] + control[3] <= root[1] + root[3] + 0.5
}

fn icon(kind: Icon, color: gpui_kit::Rgba) -> ns::AnyElement {
    ns::svg(nsh_core::element::ImageSrc::svg_markup(
        &crate::icons::nsh_source(kind).unwrap_or_default(),
    ))
    .size(ns::px(Layout::ICON_SIZE))
    .text_color(crate::nsh::color(color))
    .into_element()
}
