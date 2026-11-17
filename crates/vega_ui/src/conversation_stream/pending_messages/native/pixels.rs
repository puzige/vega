use super::{ButtonStyle, Content, Request, Snapshot, TextKind};
use crate::nsh::OrderedCommands;
use ns::{
    Accessible as _, Element as _, Interactive as _, IntoElement as _, ParentElement as _,
    Styled as _,
};
use nsh_core::prelude as ns;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

pub(super) fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    let command_owner = nsh_core::element::Owner::current();
    ns::dynamic(move || {
        let snapshot = state.get();
        let presentation_owner = nsh_core::element::Owner::current();
        let colors = snapshot.colors;
        let target = ns::element_ref();
        let revision = ns::signal(0u64);
        let observed = snapshot.clone();
        let measure_commands = commands.clone();
        ns::effect(move || {
            let observation = revision.get();
            let observed = observed.clone();
            let commands = measure_commands.clone();
            command_owner.run(|| {
                ns::spawn(async move {
                    let Some(measurement) = target.measure().await else {
                        return;
                    };
                    if presentation_owner.is_live()
                        && revision.get_untracked() == observation
                        && measurable(measurement)
                    {
                        commands.send(Request::Displayed(Box::new(observed)));
                    }
                })
            });
        });
        let mut root = ns::view()
            .element_ref(&target)
            .flex_col()
            .w_full()
            .h_full()
            .on_layout(move |_: &ns::Bounds<ns::Points>| {
                revision.update(|value| *value = value.wrapping_add(1))
            });
        match &snapshot.content {
            Content::Panel(menu) => {
                root = root
                    .rounded(ns::px(if *menu {
                        Layout::MENU_RADIUS
                    } else {
                        Layout::PENDING_MESSAGE_RADIUS
                    }))
                    .bg(crate::nsh::color(colors.bg_elevated))
                    .border(ns::px(1.))
                    .border_color(crate::nsh::color(colors.border_subtle));
            }
            Content::Glyph => {
                root = root
                    .items_center()
                    .justify_center()
                    .child(glyph(&snapshot, colors.text_tertiary));
            }
            Content::Text(text, kind) => {
                let color = match kind {
                    TextKind::Primary => colors.text_primary,
                    TextKind::Error => colors.danger,
                    _ => colors.text_secondary,
                };
                let mut label = ns::text(text.clone())
                    .w_full()
                    .text_size(ns::px(
                        if matches!(kind, TextKind::Reason | TextKind::Badge) {
                            Typography::METADATA
                        } else {
                            Typography::BODY
                        },
                    ))
                    .text_color(crate::nsh::color(color));
                if *kind != TextKind::Error {
                    label = label.truncate();
                }
                if *kind == TextKind::Badge {
                    root = root.bg(crate::nsh::color(colors.bg_elevated));
                }
                root = root.justify_center().child(label);
                if *kind == TextKind::Error {
                    let height_commands = commands.clone();
                    let id = snapshot.id.clone();
                    let measured = ns::signal(0f32);
                    root = ns::view().w_full().child(
                        root.h(nsh_core::geometry::Length::Auto).on_layout(
                            move |bounds: &ns::Bounds<ns::Points>| {
                                let height = bounds.size.height.0;
                                if height.is_finite()
                                    && height > 0.
                                    && (measured.get_untracked() - height).abs() > 0.5
                                {
                                    measured.set(height);
                                    command_owner.run(|| {
                                        height_commands.send(Request::Height(id.clone(), height))
                                    });
                                }
                            },
                        ),
                    );
                }
            }
            Content::Button {
                label,
                reason,
                style,
                enabled,
                selected,
                ..
            } => {
                let enabled = *enabled;
                let selected = *selected;
                let hover = ns::signal(false);
                let pressed = ns::signal(false);
                let observed = snapshot.clone();
                let press_commands = commands.clone();
                let accessible = reason
                    .as_ref()
                    .map_or_else(|| label.clone(), |reason| format!("{label}. {reason}"));
                root = root
                    .role(nsh_core::AccessibilityRole::Button)
                    .label(accessible)
                    .disabled(!enabled)
                    .accessibility_state(nsh_core::AccessibilityState {
                        disabled: !enabled,
                        selected,
                        ..Default::default()
                    })
                    .rounded(ns::px(Layout::SETTINGS_CONTROL_RADIUS))
                    .border(ns::px(1.))
                    .border_color(crate::nsh::color(if snapshot.focused {
                        colors.accent
                    } else {
                        colors.bg_elevated.opacity(0.)
                    }))
                    .bg(move || {
                        crate::nsh::color(if (enabled && pressed.get()) || selected {
                            colors.bg_active
                        } else if enabled && hover.get() {
                            colors.bg_hover
                        } else {
                            colors.bg_elevated.opacity(0.)
                        })
                    })
                    .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
                    .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                        hover.set(false);
                        pressed.set(false);
                    })
                    .on_press_down(move || pressed.set(true))
                    .on_press_up(move || pressed.set(false))
                    .on_press(move || {
                        if !enabled {
                            return;
                        }
                        let commands = press_commands.clone();
                        let observed = observed.clone();
                        command_owner.run(|| {
                            ns::spawn(async move {
                                let observation = revision.get_untracked();
                                let Some(measurement) = target.measure().await else {
                                    return;
                                };
                                if presentation_owner.is_live()
                                    && observation == revision.get_untracked()
                                    && measurable(measurement)
                                {
                                    commands.send(Request::Displayed(Box::new(observed.clone())));
                                    commands.send(Request::Activate(Box::new(observed)));
                                }
                            })
                        });
                    });
                let foreground = if enabled {
                    colors.text_secondary
                } else {
                    colors.text_tertiary
                };
                let mut body = ns::view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(glyph(&snapshot, foreground));
                if *style != ButtonStyle::Icon {
                    let mut copy = ns::view().flex_col().flex_1().min_w(ns::px(0.)).child(
                        ns::text(label.clone())
                            .text_size(ns::px(Typography::BODY))
                            .text_color(crate::nsh::color(foreground))
                            .truncate(),
                    );
                    if *style == ButtonStyle::Menu
                        && let Some(reason) = reason
                    {
                        copy = copy.child(
                            ns::text(reason.clone())
                                .text_size(ns::px(Typography::METADATA))
                                .text_color(crate::nsh::color(colors.text_tertiary)),
                        );
                    }
                    body = body.child(copy);
                    root = root.px_2();
                }
                if *style == ButtonStyle::Menu && reason.is_some() {
                    let height_commands = commands.clone();
                    let id = snapshot.id.clone();
                    let measured = ns::signal(0f32);
                    body = body
                        .py_1()
                        .on_layout(move |bounds: &ns::Bounds<ns::Points>| {
                            let height = bounds.size.height.0;
                            if height.is_finite()
                                && height > 0.
                                && (measured.get_untracked() - height).abs() > 0.5
                            {
                                measured.set(height);
                                command_owner.run(|| {
                                    height_commands.send(Request::Height(id.clone(), height))
                                });
                            }
                        });
                }
                root = root.justify_center().child(body);
            }
        }
        root.into_element()
    })
    .into_element()
}

fn glyph(snapshot: &Snapshot, color: gpui_kit::Rgba) -> ns::AnyElement {
    ns::svg(nsh_core::element::ImageSrc::svg_markup(
        snapshot.glyph.as_deref().unwrap_or_default(),
    ))
    .size(ns::px(Layout::ICON_SIZE))
    .flex_shrink_0()
    .text_color(crate::nsh::color(color))
    .into_element()
}

fn measurable(measurement: nsh_core::Measurement) -> bool {
    [
        measurement.page.x.0,
        measurement.page.y.0,
        measurement.frame.size.width.0,
        measurement.frame.size.height.0,
    ]
    .iter()
    .all(|value| value.is_finite())
        && measurement.frame.size.width.0 > 0.
        && measurement.frame.size.height.0 > 0.
}
