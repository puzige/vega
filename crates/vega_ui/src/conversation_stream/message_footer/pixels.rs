use super::{Action, Request, Snapshot, Witness};
use crate::icons::Icon;
use crate::nsh::OrderedCommands;
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::element::Element as _;
use nsh_core::prelude as ns;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tokio::sync::mpsc;
use vega_theme::{Layout, Motion, Typography};

pub(super) fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    let command_owner = nsh_core::element::Owner::current();
    ns::dynamic(move || {
        let snapshot = state.get();
        let presentation = nsh_core::element::Owner::current();
        let root = ns::element_ref();
        let hovered = ns::signal(false);
        let local_focus = ns::signal(false);
        let shown = snapshot.visible;
        let mut row = ns::view()
            .element_ref(&root)
            .w_full()
            .h(ns::px(Layout::COPY_ACTION_SIZE))
            .flex_row()
            .items_center();
        if snapshot.source.user {
            row = row.justify_end();
        } else {
            row = row.justify_start();
        }
        for action in [Action::Copy, Action::Fork] {
            if !snapshot.enabled(action) && !(action == Action::Fork && snapshot.pending()) {
                continue;
            }
            let pending = action == Action::Fork && snapshot.pending();
            let enabled = snapshot.enabled(action);
            let target = ns::element_ref();
            let hover = ns::signal(false);
            let pressed = ns::signal(false);
            let focused = snapshot.focused == Some(action);
            target.on_focus_in(move || local_focus.set(true));
            target.on_focus_out(move || local_focus.set(false));
            let label = vega_i18n::t(match action {
                Action::Copy if snapshot.source.user => "ui.copy_message",
                Action::Copy => "ui.copy_final_reply",
                Action::Fork if pending => "ui.fork_creating",
                Action::Fork => "ui.branch_in_new_chat",
            });
            let colors = snapshot.colors;
            let press_commands = commands.clone();
            let ax_commands = commands.clone();
            let observed = snapshot.clone();
            let accessible = snapshot.clone();
            let layout_revision = snapshot.revision.clone();
            let activation = snapshot
                .activation
                .is_some_and(|(target, _)| target == action);
            let keyboard_commands = commands.clone();
            let keyboard_observed = snapshot.clone();
            ns::effect(move || {
                if activation {
                    dispatch(
                        keyboard_observed.clone(),
                        action,
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
                .size(ns::px(Layout::COPY_ACTION_SIZE))
                .flex_shrink_0()
                .flex_row()
                .items_center()
                .justify_center()
                .relative()
                .rounded(ns::px(Layout::MENU_RADIUS))
                .focusable(false)
                .accessible(true)
                .role(if pending {
                    nsh_core::AccessibilityRole::Progressbar
                } else {
                    nsh_core::AccessibilityRole::Button
                })
                .label(label)
                .disabled(!enabled)
                .accessibility_state(nsh_core::AccessibilityState {
                    disabled: !enabled,
                    ..Default::default()
                })
                .opacity(move || {
                    if shown || hovered.get() || local_focus.get() || pending {
                        1_f32
                    } else {
                        0_f32
                    }
                })
                .bg(move || {
                    crate::nsh::color(if enabled && pressed.get() {
                        colors.bg_active
                    } else if enabled && (hover.get() || focused) {
                        colors.bg_hover
                    } else {
                        colors.bg_elevated.opacity(0.)
                    })
                })
                .border(ns::px(1.))
                .border_color(crate::nsh::color(if focused {
                    colors.accent
                } else {
                    colors.bg_elevated.opacity(0.)
                }))
                .on_layout(move |_: &ns::Bounds<ns::Points>| {
                    layout_revision.fetch_add(1, Ordering::AcqRel);
                })
                .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
                .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                    hover.set(false);
                    pressed.set(false);
                })
                .on_press_down(move || pressed.set(true))
                .on_press_up(move || pressed.set(false))
                .on_press(move || {
                    if enabled {
                        dispatch(
                            observed.clone(),
                            action,
                            target,
                            root,
                            press_commands.clone(),
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
                            action,
                            target,
                            root,
                            ax_commands.clone(),
                            command_owner,
                            presentation,
                        );
                    }
                })
                .tooltip(
                    ns::text(label)
                        .text_size(ns::px(Typography::METADATA))
                        .text_color(crate::nsh::color(colors.text_primary))
                        .bg(crate::nsh::color(colors.bg_elevated))
                        .px_2()
                        .py_1()
                        .rounded(ns::px(Layout::MENU_RADIUS))
                        .into_element(),
                );
            if action == Action::Copy {
                if snapshot.receipt.is_some() {
                    control = control.hint(vega_i18n::t("ui.copy_requested"));
                }
                let blend = ns::signal(0f32);
                if snapshot.receipt.is_some() {
                    ns::spawn(async move {
                        ns::next_frame().await;
                        blend.set(1.);
                    });
                }
                control = control.child(
                    ns::view()
                        .size(ns::px(Layout::ICON_SIZE))
                        .relative()
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
            } else if pending {
                control = control.child(pending_icon(colors.text_secondary));
            } else {
                control = control.child(icon(Icon::GitBranch, colors.text_secondary));
            }
            row = row.child(control);
        }
        if let Some((timestamp, accessible)) = &snapshot.source.timestamp {
            row = row.child(
                ns::text(timestamp.clone())
                    .accessible(true)
                    .role(nsh_core::AccessibilityRole::Text)
                    .label(accessible.clone())
                    .text_size(ns::px(Typography::METADATA))
                    .text_color(crate::nsh::color(snapshot.colors.text_tertiary))
                    .opacity(move || {
                        if shown || hovered.get() || local_focus.get() {
                            1_f32
                        } else {
                            0_f32
                        }
                    }),
            );
        }
        let revision = snapshot.revision.clone();
        row.on_layout(move |_: &ns::Bounds<ns::Points>| {
            revision.fetch_add(1, Ordering::AcqRel);
        })
        .on_mouse_enter(move |_: &nsh_core::MouseEvent| hovered.set(true))
        .on_mouse_leave(move |_: &nsh_core::MouseEvent| hovered.set(false))
        .into_element()
    })
    .into_element()
}

fn dispatch(
    observed: Snapshot,
    action: Action,
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
                    action,
                    Witness {
                        revision,
                        control,
                        root,
                    },
                ));
            }
        });
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

fn pending_icon(color: gpui_kit::Rgba) -> ns::AnyElement {
    ns::dynamic(move || {
        if ns::lifecycle() != nsh_core::platform::Lifecycle::Active || ns::reduce_motion() {
            return icon(Icon::GitBranch, color);
        }
        let phase = ns::signal(0f32);
        let epoch = Instant::now();
        ns::spawn(async move {
            loop {
                ns::next_frame().await;
                phase.set(epoch.elapsed().as_secs_f32() * 360.);
            }
        });
        use gpui_kit::{AssetSource as _, component::IconNamed as _};
        let source = gpui_kit::assets::Assets::new("")
            .load(&gpui_kit::component::IconName::LoaderCircle.path())
            .ok()
            .flatten()
            .and_then(|data| String::from_utf8(data.into_owned()).ok())
            .unwrap_or_default();
        ns::svg(nsh_core::element::ImageSrc::svg_markup(&source))
            .size(ns::px(Layout::ICON_SIZE))
            .rotate(move || ns::deg(phase.get()))
            .text_color(crate::nsh::color(color))
            .into_element()
    })
    .into_element()
}
