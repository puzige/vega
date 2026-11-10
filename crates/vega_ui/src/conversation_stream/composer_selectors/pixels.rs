use super::{Action, Kind, Origin, Request, Snapshot};
use crate::nsh::OrderedCommands;
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::element::Element as _;
use nsh_core::prelude as ns;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

pub(super) fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    let command_owner = nsh_core::element::Owner::current();
    ns::dynamic(move || {
        let snapshot = state.get();
        let presentation_owner = nsh_core::element::Owner::current();
        let viewport = ns::element_ref();
        let track_target = ns::element_ref();
        let revision = ns::signal(0u64);
        let offset = ns::signal(0f32);
        let native_revision = snapshot.native_revision.clone();
        let scroll_revision = native_revision.clone();
        ns::watch_scroll(
            move || viewport.node(),
            move |event| {
                offset.set(event.offset.y.0);
                revision.update(|revision| *revision = revision.wrapping_add(1));
                scroll_revision.fetch_add(1, Ordering::AcqRel);
            },
        );
        let mut content = ns::view().w_full().flex_col();
        if snapshot.kind == Kind::Thinking {
            content = content.items_center().gap(ns::px(8.));
        }
        for line in &snapshot.header {
            content = content.child(
                ns::text(line.clone())
                    .px_2()
                    .py_1()
                    .text_size(ns::px(Typography::METADATA))
                    .text_color(crate::nsh::color(snapshot.colors.text_secondary)),
            );
        }
        let mut controls = Vec::new();
        for (index, row) in snapshot.rows.iter().enumerate() {
            let tier = matches!(row.action, Action::Tier(_));
            let target = if tier {
                track_target
            } else {
                ns::element_ref()
            };
            let measured = snapshot.clone();
            let measure_commands = commands.clone();
            let reveal = snapshot.selected == Some(index)
                && matches!(snapshot.kind, Kind::ModelMenu | Kind::PermissionMenu);
            ns::effect(move || {
                let observation = revision.get();
                let commands = measure_commands.clone();
                let observed = measured.clone();
                let native = observed.native_revision.load(Ordering::Acquire);
                command_owner.run(|| {
                    ns::spawn(async move {
                        let (Some(control), Some(container)) =
                            (target.measure().await, viewport.measure().await)
                        else {
                            return;
                        };
                        if !presentation_owner.is_live()
                            || revision.get_untracked() != observation
                            || observed.native_revision.load(Ordering::Acquire) != native
                        {
                            return;
                        }
                        if reveal
                            && let Some(destination) =
                                reveal_destination(control, container, offset.get_untracked())
                        {
                            viewport.scroll_to(
                                ns::Point {
                                    x: ns::px(0.),
                                    y: ns::px(destination),
                                },
                                false,
                            );
                            return;
                        }
                        if visible(control, container)
                            && let Some((target, origin)) = observed.activation
                            && target == index
                        {
                            commands.send(Request::Activate(
                                Box::new(observed),
                                index,
                                origin,
                                native,
                            ));
                        }
                    })
                });
            });
            if let Some(group) = &row.group {
                controls.push(
                    ns::text(group.clone())
                        .px_2()
                        .py_1()
                        .text_size(ns::px(Typography::METADATA))
                        .text_color(crate::nsh::color(snapshot.colors.text_tertiary))
                        .into_element(),
                );
            }
            let hover = ns::signal(false);
            let pressed = ns::signal(false);
            let colors = snapshot.colors;
            let row_revision = native_revision.clone();
            let selected = snapshot.selected == Some(index);
            let enabled = row.enabled;
            let foreground = if !enabled {
                colors.text_tertiary
            } else if row.warning {
                colors.warning
            } else if snapshot.kind == Kind::Thinking && index == 0 {
                super::super::thinking_slider::native_label_color(&snapshot.source.slider.model)
            } else if row.checked {
                colors.brand_primary
            } else {
                colors.text_primary
            };
            let press_commands = commands.clone();
            let accessibility_commands = commands.clone();
            let accessibility_observed = snapshot.clone();
            let observed = snapshot.clone();
            let label = row.description.as_ref().map_or_else(
                || row.label.clone(),
                |description| format!("{}\n{description}", row.label),
            );
            let mut control = ns::view()
                .element_ref(&target)
                .w_full()
                .flex_shrink_0()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .min_h(ns::px(if snapshot.kind == Kind::PermissionMenu {
                    44.
                } else {
                    Typography::SIDEBAR_LINE_HEIGHT
                }))
                .rounded(ns::px(
                    if matches!(snapshot.kind, Kind::PermissionTrigger | Kind::ModelTrigger) {
                        Layout::COMPOSER_TEXT_TRIGGER_HEIGHT / 2.
                    } else {
                        Layout::MENU_RADIUS
                    },
                ))
                .accessible(true)
                .role(nsh_core::AccessibilityRole::Button)
                .label(label)
                .disabled(!enabled)
                .accessibility_actions(vec![nsh_core::AccessibilityActionDef {
                    name: "activate".into(),
                    label: None,
                }])
                .on_accessibility_action(move |name: &str| {
                    if enabled && name == "activate" {
                        command_owner.run(|| {
                            accessibility_commands.send(Request::Intent(
                                Box::new(accessibility_observed.clone()),
                                index,
                                Origin::Accessibility,
                                accessibility_observed
                                    .native_revision
                                    .load(Ordering::Acquire),
                            ));
                        });
                    }
                })
                .accessibility_state(nsh_core::AccessibilityState {
                    disabled: !enabled,
                    selected: row.checked,
                    ..Default::default()
                })
                .bg(move || {
                    crate::nsh::color(if enabled && pressed.get() {
                        colors.bg_active
                    } else if enabled && hover.get() {
                        colors.bg_hover
                    } else if selected {
                        colors.bg_active
                    } else {
                        colors.bg_elevated.opacity(0.)
                    })
                })
                .border(ns::px(1.))
                .border_color(crate::nsh::color(if selected {
                    colors.accent
                } else {
                    colors.bg_elevated.opacity(0.)
                }))
                .on_layout(move |_: &ns::Bounds<ns::Points>| {
                    revision.update(|revision| *revision = revision.wrapping_add(1));
                    row_revision.fetch_add(1, Ordering::AcqRel);
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
                            let native = observed.native_revision.load(Ordering::Acquire);
                            let (Some(control), Some(container)) =
                                (target.measure().await, viewport.measure().await)
                            else {
                                return;
                            };
                            if presentation_owner.is_live()
                                && revision.get_untracked() == observation
                                && observed.native_revision.load(Ordering::Acquire) == native
                                && visible(control, container)
                            {
                                commands.send(Request::Activate(
                                    Box::new(observed),
                                    index,
                                    Origin::Pointer,
                                    native,
                                ));
                            }
                        })
                    });
                });
            if let Some(glyph) = row.glyph {
                control = control.child(icon(glyph, foreground));
            }
            let mut text = ns::view().flex_1().min_w(ns::px(0.)).flex_col().child(
                ns::text(row.label.clone())
                    .text_size(ns::px(Typography::SIDEBAR))
                    .text_color(crate::nsh::color(foreground)),
            );
            if let Some(description) = &row.description {
                text = text.child(
                    ns::text(description.clone())
                        .text_size(ns::px(Typography::METADATA))
                        .text_color(crate::nsh::color(colors.text_secondary)),
                );
            }
            control = control.child(text);
            if row.checked && matches!(snapshot.kind, Kind::PermissionMenu | Kind::ModelMenu) {
                control = control.child(icon(crate::icons::Icon::Check, colors.brand_primary));
            }
            if snapshot.kind == Kind::ModelTrigger
                || (snapshot.kind == Kind::Thinking && index == 0)
            {
                control = control.child(icon(
                    if snapshot.kind == Kind::Thinking {
                        crate::icons::Icon::ChevronRight
                    } else {
                        crate::icons::Icon::ChevronDown
                    },
                    colors.text_tertiary,
                ));
            }
            if !tier {
                controls.push(control.into_element());
            }
        }
        if snapshot.kind == Kind::Thinking {
            let title = controls.remove(0);
            let default = controls.pop();
            content = content
                .child(title)
                .child(
                    ns::text(snapshot.source.slider.model_name.clone())
                        .text_size(ns::px(Typography::SIDEBAR))
                        .text_color(crate::nsh::color(snapshot.colors.text_secondary)),
                )
                .child({
                    let target = track_target;
                    let drag_commands = commands.clone();
                    let observed = snapshot.clone();
                    let accessibility_commands = commands.clone();
                    let accessibility_observed = snapshot.clone();
                    super::super::thinking_slider::native::track(
                        &snapshot.source.slider,
                        snapshot.colors,
                        snapshot.source.appearance == vega_theme::Appearance::Light,
                        snapshot.rows.first().is_some_and(|row| row.enabled),
                        snapshot.selected.is_some_and(|index| {
                            snapshot
                                .rows
                                .get(index)
                                .is_some_and(|row| matches!(row.action, Action::Tier(_)))
                        }),
                        target,
                        super::super::thinking_slider::native::Actions {
                            pointer: Rc::new(move |position| {
                                let commands = drag_commands.clone();
                                let observed = observed.clone();
                                command_owner.run(|| {
                                    ns::spawn(async move {
                                        let observation = revision.get_untracked();
                                        let native =
                                            observed.native_revision.load(Ordering::Acquire);
                                        let (Some(control), Some(container)) =
                                            (target.measure().await, viewport.measure().await)
                                        else {
                                            return;
                                        };
                                        if presentation_owner.is_live()
                                            && revision.get_untracked() == observation
                                            && observed.native_revision.load(Ordering::Acquire)
                                                == native
                                            && visible(control, container)
                                        {
                                            commands.send(Request::Activate(
                                                Box::new(observed),
                                                position + 1,
                                                Origin::Pointer,
                                                native,
                                            ));
                                        }
                                    })
                                });
                            }),
                            accessibility: Rc::new(move |position| {
                                command_owner.run(|| {
                                    accessibility_commands.send(Request::Intent(
                                        Box::new(accessibility_observed.clone()),
                                        position + 1,
                                        Origin::Accessibility,
                                        accessibility_observed
                                            .native_revision
                                            .load(Ordering::Acquire),
                                    ));
                                });
                            }),
                        },
                    )
                });
            if let Some(default) = default {
                content = content.child(default);
            }
            content = content.child(ns::view().flex_col().children(controls));
        } else {
            content = content.children(controls);
        }
        if matches!(snapshot.kind, Kind::PermissionTrigger | Kind::ModelTrigger) {
            content = content.h_full().justify_center();
        }
        let popup = matches!(
            snapshot.kind,
            Kind::PermissionMenu | Kind::ModelMenu | Kind::Thinking
        );
        if popup {
            content = content
                .p_1()
                .bg(crate::nsh::color(snapshot.colors.bg_elevated))
                .rounded(ns::px(Layout::MENU_RADIUS))
                .border(ns::px(1.))
                .border_color(crate::nsh::color(snapshot.colors.border_subtle));
        }
        let layout_commands = commands.clone();
        let layout_observed = snapshot.clone();
        content = content.on_layout(move |bounds: &ns::Bounds<ns::Points>| {
            revision.update(|revision| *revision = revision.wrapping_add(1));
            native_revision.fetch_add(1, Ordering::AcqRel);
            if popup {
                command_owner.run(|| {
                    layout_commands.send(Request::Height(
                        Box::new(layout_observed.clone()),
                        native_revision.load(Ordering::Acquire),
                        bounds.size.width.0,
                        bounds.size.height.0,
                    ))
                });
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

pub(super) fn visible(control: nsh_core::Measurement, viewport: nsh_core::Measurement) -> bool {
    let x = control.page.x.0;
    let y = control.page.y.0;
    let width = control.frame.size.width.0;
    let height = control.frame.size.height.0;
    let vx = viewport.page.x.0;
    let vy = viewport.page.y.0;
    let vw = viewport.frame.size.width.0;
    let vh = viewport.frame.size.height.0;
    [x, y, width, height, vx, vy, vw, vh]
        .iter()
        .all(|value| value.is_finite())
        && width > 0.
        && height > 0.
        && vw > 0.
        && vh > 0.
        && x >= vx
        && x + width <= vx + vw + 0.5
        && y >= vy
        && (y + height <= vy + vh + 0.5 || (height > vh && (y - vy).abs() < 0.5))
}

fn reveal_destination(
    control: nsh_core::Measurement,
    viewport: nsh_core::Measurement,
    offset: f32,
) -> Option<f32> {
    if visible(control, viewport) || !offset.is_finite() {
        return None;
    }
    let top = control.page.y.0;
    let height = control.frame.size.height.0;
    let start = viewport.page.y.0;
    let extent = viewport.frame.size.height.0;
    if ![top, height, start, extent]
        .iter()
        .all(|value| value.is_finite())
        || height <= 0.
        || extent <= 0.
    {
        return None;
    }
    let delta = if height > extent || top < start {
        top - start
    } else {
        top + height - start - extent
    };
    let target = (offset + delta).max(0.);
    (target.is_finite() && target != offset).then_some(target)
}

fn icon(icon: crate::icons::Icon, color: gpui_kit::Rgba) -> ns::AnyElement {
    ns::svg(nsh_core::element::ImageSrc::svg_markup(
        &crate::icons::nsh_source(icon).unwrap_or_default(),
    ))
    .size(ns::px(Layout::ICON_SIZE))
    .text_color(crate::nsh::color(color))
    .into_element()
}
