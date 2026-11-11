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
                scroll_revision.action.fetch_add(1, Ordering::AcqRel);
                offset.set(event.offset.y.0);
                revision.update(|revision| *revision = revision.wrapping_add(1));
            },
        );
        let mut content = ns::view().w_full().flex_col();
        if snapshot.kind == Kind::Thinking {
            content = content.items_center().gap(ns::px(8.));
        }
        if snapshot.kind == Kind::PermissionMenu
            && let [title, more, details @ ..] = snapshot.header.as_slice()
        {
            content = content.child(
                ns::view()
                    .w_full()
                    .flex_row()
                    .items_start()
                    .gap_2()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .child(
                        ns::text(title.clone())
                            .flex_1()
                            .min_w(ns::px(0.))
                            .text_size(ns::px(Typography::METADATA))
                            .line_height(ns::px(
                                Typography::METADATA * Typography::BODY_LINE_HEIGHT,
                            ))
                            .text_color(crate::nsh::color(snapshot.colors.text_secondary)),
                    )
                    .child(
                        ns::text(more.clone())
                            .flex_shrink_0()
                            .text_size(ns::px(Typography::METADATA))
                            .line_height(ns::px(
                                Typography::METADATA * Typography::BODY_LINE_HEIGHT,
                            ))
                            .text_color(crate::nsh::color(snapshot.colors.text_tertiary)),
                    ),
            );
            for detail in details {
                content = content.child(
                    ns::text(detail.clone())
                        .px_2()
                        .pb_1()
                        .text_size(ns::px(Typography::METADATA))
                        .line_height(ns::px(Typography::METADATA * Typography::BODY_LINE_HEIGHT))
                        .text_color(crate::nsh::color(snapshot.colors.text_secondary)),
                );
            }
            content = content.child(
                ns::view()
                    .mx_2()
                    .h(ns::px(1.))
                    .bg(crate::nsh::color(snapshot.colors.border_subtle)),
            );
        } else {
            for line in &snapshot.header {
                content = content.child(
                    ns::text(line.clone())
                        .px_2()
                        .py_1()
                        .text_size(ns::px(Typography::METADATA))
                        .text_color(crate::nsh::color(snapshot.colors.text_secondary)),
                );
            }
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
                let native = observed.native_revision.action.load(Ordering::Acquire);
                command_owner.run(|| {
                    ns::spawn(async move {
                        let (Some(control), Some(container)) =
                            (target.measure().await, viewport.measure().await)
                        else {
                            return;
                        };
                        if !presentation_owner.is_live()
                            || revision.get_untracked() != observation
                            || observed.native_revision.action.load(Ordering::Acquire) != native
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
            let permission_option = snapshot.kind == Kind::PermissionMenu;
            let thinking = snapshot.kind == Kind::Thinking;
            let show_ring =
                selected && (!(permission_option || thinking) || snapshot.keyboard_focus_visible);
            let checked_permission = permission_option && row.checked;
            let enabled = row.enabled;
            let foreground = if !enabled {
                colors.text_tertiary
            } else if row.warning {
                colors.warning
            } else if snapshot.kind == Kind::Thinking && index == 0 {
                super::super::thinking_slider::native_label_color(&snapshot.source.slider.model)
            } else if snapshot.kind == Kind::PermissionMenu {
                colors.text_primary
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
                .min_h(ns::px(match snapshot.kind {
                    Kind::PermissionTrigger | Kind::ModelTrigger => {
                        Layout::COMPOSER_TEXT_TRIGGER_HEIGHT
                    }
                    Kind::PermissionMenu => 44.,
                    _ => Typography::SIDEBAR_LINE_HEIGHT,
                }))
                .rounded(ns::px(
                    if matches!(snapshot.kind, Kind::PermissionTrigger | Kind::ModelTrigger) {
                        Layout::COMPOSER_TEXT_TRIGGER_HEIGHT / 2.
                    } else if permission_option {
                        6.
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
                                    .action
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
                    } else if checked_permission {
                        crate::menu_list::selected_row_bg(colors)
                    } else if selected && !permission_option && !thinking {
                        colors.bg_active
                    } else {
                        colors.bg_elevated.opacity(0.)
                    })
                })
                .border(ns::px(if permission_option { 0. } else { 1. }))
                .border_color(crate::nsh::color(if show_ring {
                    colors.accent
                } else {
                    colors.bg_elevated.opacity(0.)
                }))
                .on_layout(move |_: &ns::Bounds<ns::Points>| {
                    row_revision.action.fetch_add(1, Ordering::AcqRel);
                    revision.update(|revision| *revision = revision.wrapping_add(1));
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
                            let native = observed.native_revision.action.load(Ordering::Acquire);
                            let (Some(control), Some(container)) =
                                (target.measure().await, viewport.measure().await)
                            else {
                                return;
                            };
                            if presentation_owner.is_live()
                                && revision.get_untracked() == observation
                                && observed.native_revision.action.load(Ordering::Acquire) == native
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
            if thinking && index == 0 {
                let group_hover = ns::signal(false);
                let line = control
                    .w(nsh_core::geometry::auto())
                    .min_h(ns::px(20.))
                    .p(ns::px(0.))
                    .bg(crate::nsh::color(colors.bg_elevated.opacity(0.)))
                    .h(ns::px(20.))
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .gap(ns::px(4.))
                    .child(
                        ns::text(row.label.clone())
                            .truncate()
                            .text_size(ns::px(Typography::METADATA))
                            .text_color(crate::nsh::color(foreground)),
                    )
                    .child(icon(crate::icons::Icon::ChevronRight, colors.text_tertiary));
                controls.push(
                    ns::view()
                        .items_center()
                        .px_2()
                        .on_mouse_enter(move |_: &nsh_core::MouseEvent| group_hover.set(true))
                        .on_mouse_leave(move |_: &nsh_core::MouseEvent| group_hover.set(false))
                        .bg(move || {
                            crate::nsh::color(if group_hover.get() {
                                colors.bg_hover
                            } else {
                                colors.bg_elevated.opacity(0.)
                            })
                        })
                        .w(nsh_core::geometry::auto())
                        .max_w(ns::px(super::super::thinking_slider::THINKING_TRACK_WIDTH))
                        .flex_col()
                        .gap(ns::px(8.))
                        .py(ns::px(0.))
                        .rounded(ns::px(6.))
                        .child(line)
                        .child(
                            ns::view()
                                .h(ns::px(20.))
                                .max_w(ns::px(
                                    super::super::thinking_slider::THINKING_TRACK_WIDTH - 18.,
                                ))
                                .justify_center()
                                .child(
                                    ns::text(snapshot.source.slider.model_name.clone())
                                        .truncate()
                                        .text_size(ns::px(Typography::SIDEBAR))
                                        .text_color(crate::nsh::color(colors.text_secondary)),
                                ),
                        )
                        .into_element(),
                );
                continue;
            }
            if permission_option && show_ring {
                control = control
                    .outline(ns::px(1.))
                    .outline_offset(ns::px(-1.))
                    .outline_color(crate::nsh::color(colors.accent));
            }
            if let Some(glyph) = row.glyph {
                control = control.child(icon(
                    glyph,
                    if snapshot.kind == Kind::PermissionMenu && enabled && !row.warning {
                        colors.text_secondary
                    } else {
                        foreground
                    },
                ));
            }
            let mut label = ns::text(row.label.clone())
                .text_size(ns::px(Typography::SIDEBAR))
                .text_color(crate::nsh::color(foreground));
            if matches!(snapshot.kind, Kind::PermissionTrigger | Kind::ModelTrigger) {
                label = label.truncate();
            } else if snapshot.kind == Kind::PermissionMenu {
                label =
                    label.line_height(ns::px(Typography::SIDEBAR * Typography::BODY_LINE_HEIGHT));
            }
            let mut text = ns::view()
                .flex_1()
                .min_w(ns::px(0.))
                .flex_col()
                .child(label);
            if let Some(description) = &row.description {
                let mut desc =
                    ns::text(description.clone()).text_size(ns::px(Typography::METADATA));
                if snapshot.kind == Kind::PermissionMenu {
                    desc = desc
                        .line_height(ns::px(Typography::METADATA * Typography::BODY_LINE_HEIGHT));
                }
                text = text.child(desc.text_color(crate::nsh::color(
                    if snapshot.kind == Kind::PermissionMenu {
                        colors.text_tertiary
                    } else {
                        colors.text_secondary
                    },
                )));
            }
            control = control.child(text);
            if snapshot.kind == Kind::PermissionMenu {
                let mut marker = ns::view()
                    .w(ns::px(Typography::SIDEBAR))
                    .flex_shrink_0()
                    .flex_row()
                    .justify_end();
                if row.checked {
                    marker = marker.child(icon(crate::icons::Icon::Check, colors.brand_primary));
                }
                control = control.child(marker);
            } else if row.checked && snapshot.kind == Kind::ModelMenu {
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
            let track = {
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
                    snapshot.keyboard_focus_visible
                        && snapshot.selected.is_some_and(|index| {
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
                                        observed.native_revision.action.load(Ordering::Acquire);
                                    let (Some(control), Some(container)) =
                                        (target.measure().await, viewport.measure().await)
                                    else {
                                        return;
                                    };
                                    if presentation_owner.is_live()
                                        && revision.get_untracked() == observation
                                        && observed.native_revision.action.load(Ordering::Acquire)
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
                                        .action
                                        .load(Ordering::Acquire),
                                ));
                            });
                        }),
                    },
                )
            };
            content = content.child(title).child(track);
        } else if snapshot.kind == Kind::PermissionMenu {
            content = content.child(ns::view().flex_col().p_1().children(controls));
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
            if snapshot.kind == Kind::Thinking {
                content = content.pt(ns::px(12.)).pb(ns::px(14.));
            } else if snapshot.kind != Kind::PermissionMenu {
                content = content.p_1();
            }
            content = content
                .bg(crate::nsh::color(snapshot.colors.bg_elevated))
                .rounded(ns::px(if snapshot.kind == Kind::Thinking {
                    Layout::COMPOSER_RADIUS
                } else {
                    Layout::MENU_RADIUS
                }))
                .border(ns::px(1.))
                .border_color(crate::nsh::color(snapshot.colors.border_subtle));
        }
        let layout_commands = commands.clone();
        let layout_observed = snapshot.clone();
        content = content.on_layout(move |bounds: &ns::Bounds<ns::Points>| {
            // Effects run synchronously when notified and must observe this layout revision.
            native_revision.action.fetch_add(1, Ordering::AcqRel);
            revision.update(|revision| *revision = revision.wrapping_add(1));
            if popup {
                command_owner.run(|| {
                    layout_commands.send(Request::Height(
                        Box::new(layout_observed.clone()),
                        native_revision
                            .height
                            .fetch_add(1, Ordering::AcqRel)
                            .wrapping_add(1),
                        bounds.size.width.0,
                        bounds.size.height.0,
                    ))
                });
            }
        });
        if !popup {
            return content.element_ref(&viewport).size_full().into_element();
        }
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
