use super::{Activation, Kind, Origin, Request, Snapshot};
use crate::icons::{Icon, nsh_source};
use crate::nsh::OrderedCommands;
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::element::Element as _;
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
        let stop = snapshot.kind != Kind::Menu;
        let viewport = ns::element_ref();
        let revision = ns::signal(0u64);
        let offset = ns::signal(ns::Point {
            x: ns::px(0.),
            y: ns::px(0.),
        });
        ns::watch_scroll(
            move || viewport.node(),
            move |event| {
                offset.set(event.offset);
                revision.update(|revision| *revision = revision.wrapping_add(1));
            },
        );
        let mut rows = Vec::new();
        for (index, row) in snapshot.rows.iter().enumerate() {
            let target = ns::element_ref();
            let selected = snapshot.selected == Some(index);
            let reveal = snapshot.reveal_target() == Some(index);
            let enabled = row.enabled;
            let displayed = snapshot.clone();
            let measure_commands = commands.clone();
            ns::effect(move || {
                let observation = revision.get();
                let observed = displayed.clone();
                let commands = measure_commands.clone();
                command_owner.run(|| {
                    ns::spawn(async move {
                        commands.send(Request::Displayed(Box::new(observed.clone()), index, false));
                        let (Some(control), Some(container)) =
                            (target.measure().await, viewport.measure().await)
                        else {
                            return;
                        };
                        if !presentation_owner.is_live() || revision.get_untracked() != observation
                        {
                            return;
                        }
                        let current = offset.get_untracked();
                        if reveal
                            && let Some(destination) =
                                reveal_destination(control, container, current.y.0)
                        {
                            viewport.scroll_to(
                                ns::Point {
                                    x: current.x,
                                    y: ns::px(destination),
                                },
                                false,
                            );
                            return;
                        }
                        let is_visible = visible(control, container);
                        commands.send(Request::Displayed(
                            Box::new(observed.clone()),
                            index,
                            is_visible,
                        ));
                        if is_visible
                            && let Some(Activation {
                                index: target,
                                origin,
                            }) = observed.activation
                            && target == index
                        {
                            commands.send(Request::Activate(Box::new(observed), index, origin));
                        }
                    })
                });
            });
            let press_commands = commands.clone();
            let observed = snapshot.clone();
            let hover = ns::signal(false);
            let pressed = ns::signal(false);
            let mut control = ns::view()
                .element_ref(&target)
                .role(if row.checked.is_some() {
                    nsh_core::AccessibilityRole::Checkbox
                } else {
                    nsh_core::AccessibilityRole::Button
                })
                .label(row.accessible.clone())
                .disabled(!enabled)
                .accessibility_state(nsh_core::AccessibilityState {
                    checked: row.checked.map(|value| {
                        if value {
                            nsh_core::Checked::True
                        } else {
                            nsh_core::Checked::False
                        }
                    }),
                    selected,
                    disabled: !enabled,
                    ..Default::default()
                })
                .border(ns::px(1.))
                .border_color(crate::nsh::color(if selected {
                    colors.accent
                } else {
                    colors.bg_elevated.opacity(0.)
                }))
                .on_layout(move |_: &ns::Bounds<ns::Points>| {
                    revision.update(|revision| *revision = revision.wrapping_add(1))
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
                            let (Some(control), Some(container)) =
                                (target.measure().await, viewport.measure().await)
                            else {
                                return;
                            };
                            if !presentation_owner.is_live()
                                || revision.get_untracked() != observation
                                || !visible(control, container)
                            {
                                return;
                            }
                            commands.send(Request::Displayed(
                                Box::new(observed.clone()),
                                index,
                                true,
                            ));
                            commands.send(Request::Activate(
                                Box::new(observed),
                                index,
                                Origin::Pointer,
                            ));
                        })
                    });
                });
            if stop {
                let trigger = snapshot.kind == Kind::Trigger;
                let size = if trigger {
                    Layout::TITLEBAR_CONTROL_SIZE
                } else {
                    Layout::COMPOSER_SEND_SIZE
                };
                control = control
                    .w(ns::px(size))
                    .h(ns::px(size))
                    .rounded(ns::px(if trigger {
                        Layout::MENU_RADIUS
                    } else {
                        size / 2.
                    }))
                    .items_center()
                    .justify_center()
                    .bg(move || {
                        crate::nsh::color(if trigger {
                            if pressed.get() {
                                colors.bg_active
                            } else if hover.get() {
                                colors.bg_hover
                            } else {
                                colors.bg_elevated.opacity(0.)
                            }
                        } else if enabled && pressed.get() {
                            colors.accent
                        } else if enabled && hover.get() {
                            colors.brand_primary_strong
                        } else if enabled || snapshot.kind == Kind::Stop {
                            colors.accent
                        } else {
                            colors.bg_hover
                        })
                    });
                let foreground = if trigger {
                    colors.text_secondary
                } else if enabled || snapshot.kind == Kind::Stop {
                    colors.brand_on_accent
                } else {
                    colors.text_tertiary
                };
                control = control.text_color(crate::nsh::color(foreground));
                if snapshot.kind == Kind::ResumePending {
                    let phase = ns::signal(0f32);
                    control = control
                        .child(ns::when(
                            move || {
                                ns::lifecycle() == nsh_core::platform::Lifecycle::Active
                                    && !ns::reduce_motion()
                            },
                            move || {
                                let epoch = std::time::Instant::now();
                                phase.set(0.);
                                ns::spawn(async move {
                                    loop {
                                        ns::next_frame().await;
                                        phase.set(epoch.elapsed().as_secs_f32() * 360.);
                                    }
                                });
                                ns::view()
                            },
                        ))
                        .child(
                            ns::svg(nsh_core::element::ImageSrc::svg_markup(
                                &nsh_source(Icon::Refresh).unwrap_or_default(),
                            ))
                            .size(ns::px(Layout::ICON_SIZE))
                            .rotate(move || {
                                ns::deg(
                                    if ns::lifecycle() == nsh_core::platform::Lifecycle::Active
                                        && !ns::reduce_motion()
                                    {
                                        phase.get()
                                    } else {
                                        0.
                                    },
                                )
                            })
                            .text_color(crate::nsh::color(colors.text_secondary)),
                        );
                } else {
                    control = control.child(icon(
                        match snapshot.kind {
                            Kind::Stop => Icon::Close,
                            Kind::Trigger => Icon::Plus,
                            Kind::Primary if snapshot.source.resume => Icon::Play,
                            _ => Icon::ArrowUp,
                        },
                        foreground,
                    ));
                }
            } else {
                let background = if snapshot.highlight == index {
                    colors.bg_active
                } else {
                    colors.bg_elevated
                };
                control = control
                    .w_full()
                    .min_h(ns::px(Typography::SIDEBAR_LINE_HEIGHT))
                    .px_2()
                    .py_1()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .rounded(ns::px(Layout::MENU_RADIUS))
                    .bg(move || {
                        crate::nsh::color(if enabled && pressed.get() {
                            colors.bg_active
                        } else if enabled && hover.get() {
                            colors.bg_hover
                        } else {
                            background
                        })
                    })
                    .text_size(ns::px(Typography::SIDEBAR))
                    .text_color(crate::nsh::color(if !enabled {
                        colors.text_tertiary
                    } else if row.checked == Some(true) {
                        colors.brand_primary
                    } else {
                        colors.text_primary
                    }));
                let marker = ns::view().w(ns::px(Typography::SIDEBAR)).flex_shrink_0();
                let marker = if row.checked == Some(true) {
                    marker.child(icon(Icon::Check, colors.text_secondary))
                } else if row.description.is_some() {
                    marker.child(icon(Icon::Blocks, colors.text_secondary))
                } else {
                    marker
                };
                let mut content = ns::view()
                    .min_w(ns::px(0.))
                    .flex_1()
                    .flex_col()
                    .child(ns::text(row.label.clone()));
                if let Some(description) = &row.description {
                    content = content.child(
                        ns::text(description.clone())
                            .text_size(ns::px(Typography::METADATA))
                            .text_color(crate::nsh::color(colors.text_secondary)),
                    );
                }
                if let Some(reason) = row.reason {
                    content = content.child(
                        ns::text(vega_i18n::t(reason))
                            .text_size(ns::px(Typography::METADATA))
                            .text_color(crate::nsh::color(colors.text_secondary)),
                    );
                }
                control = control.child(marker).child(content);
                if let Some(source) = &row.source {
                    control = control.child(
                        ns::text(source.clone())
                            .text_size(ns::px(Typography::METADATA))
                            .text_color(crate::nsh::color(colors.text_secondary)),
                    );
                }
                if let Some(token) = row.token {
                    control = control.child(
                        ns::text(token).text_color(crate::nsh::color(colors.text_secondary)),
                    );
                }
            }
            rows.push(control.into_element());
        }
        let mut content = ns::view().w_full().flex_col();
        if let Some(query) = &snapshot.query {
            content = content.child(
                ns::text(query.clone())
                    .px_2()
                    .py_1()
                    .text_size(ns::px(Typography::METADATA))
                    .text_color(crate::nsh::color(colors.text_secondary)),
            );
        }
        content = content.children(rows);
        if let Some(status) = snapshot.status {
            content = content.child(
                ns::text(vega_i18n::t(status))
                    .px_2()
                    .py_1()
                    .text_size(ns::px(Typography::SIDEBAR))
                    .text_color(crate::nsh::color(colors.text_secondary)),
            );
        }
        if !stop {
            content = content
                .p_2()
                .rounded(ns::px(Layout::MENU_RADIUS))
                .bg(crate::nsh::color(colors.bg_elevated))
                .border(ns::px(1.))
                .border_color(crate::nsh::color(colors.border_subtle));
        }
        let layout_commands = commands.clone();
        let id = snapshot.id.clone();
        let measured = ns::signal(0f32);
        content = content.on_layout(move |bounds: &ns::Bounds<ns::Points>| {
            let height = bounds.size.height.0;
            if !stop
                && height.is_finite()
                && height > 0.
                && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                command_owner.run(|| layout_commands.send(Request::Layout(id.clone(), height)));
            }
        });
        ns::scroll()
            .element_ref(&viewport)
            .w_full()
            .h_full()
            .child(content)
            .into_element()
    })
    .into_element()
}

#[cfg(test)]
mod tests;

fn reveal_destination(
    control: nsh_core::Measurement,
    viewport: nsh_core::Measurement,
    offset: f32,
) -> Option<f32> {
    if visible(control, viewport) || !offset.is_finite() || !valid_geometry(control, viewport) {
        return None;
    }
    let top = control.page.y.0;
    let height = control.frame.size.height.0;
    let start = viewport.page.y.0;
    let extent = viewport.frame.size.height.0;
    let delta = if height > extent || top < start {
        top - start
    } else {
        top + height - start - extent
    };
    let destination = (offset + delta).max(0.);
    (destination.is_finite() && destination != offset).then_some(destination)
}

fn visible(control: nsh_core::Measurement, viewport: nsh_core::Measurement) -> bool {
    let top = control.page.y.0;
    let height = control.frame.size.height.0;
    let start = viewport.page.y.0;
    let extent = viewport.frame.size.height.0;
    valid_geometry(control, viewport)
        && top >= start
        && (top + height <= start + extent || (height > extent && top == start))
}

fn valid_geometry(control: nsh_core::Measurement, viewport: nsh_core::Measurement) -> bool {
    let top = control.page.y.0;
    let height = control.frame.size.height.0;
    let start = viewport.page.y.0;
    let extent = viewport.frame.size.height.0;
    [
        control.page.x.0,
        top,
        control.frame.size.width.0,
        height,
        viewport.page.x.0,
        start,
        viewport.frame.size.width.0,
        extent,
    ]
    .iter()
    .all(|value| value.is_finite())
        && control.frame.size.width.0 > 0.
        && viewport.frame.size.width.0 > 0.
        && height > 0.
        && extent > 0.
}

fn icon(icon: Icon, color: gpui_kit::Rgba) -> ns::AnyElement {
    let source = nsh_source(icon).unwrap_or_default();
    ns::svg(nsh_core::element::ImageSrc::svg_markup(&source))
        .w(ns::px(Layout::ICON_SIZE))
        .h(ns::px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .text_color(crate::nsh::color(color))
        .into_element()
}
