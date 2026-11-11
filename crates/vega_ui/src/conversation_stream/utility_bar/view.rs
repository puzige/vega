use super::host::{Kind, Request, Snapshot};
use crate::icons::{Icon, nsh_source};
use crate::nsh::OrderedCommands;
use ns::{
    Accessible as _, Element as _, Interactive as _, IntoElement as _, ParentElement as _,
    Styled as _,
};
use nsh_core::prelude as ns;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

pub(super) fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    let owner = nsh_core::element::Owner::current();
    let initial = state.get_untracked();
    if initial.kind == Kind::Background && !initial.failure {
        return ns::view()
            .size_full()
            .accessible(false)
            .accessibility_elements_hidden(true)
            .pointer_events(nsh_core::PointerEvents::None)
            .rounded_t(ns::px(Layout::COMPOSER_UTILITY_BAR_RADIUS))
            .bg(move || crate::nsh::color(state.get().colors.bg_sidebar))
            .into_element();
    }
    let search = ns::element_ref();
    let query = ns::signal(String::new());
    let composing = ns::signal(false);
    let entered = ns::signal(false);
    let viewport = ns::element_ref();
    let offset = ns::signal(ns::Point {
        x: ns::px(0.),
        y: ns::px(0.),
    });
    let revision = ns::signal(0u64);
    ns::effect(move || {
        let snapshot = state.get();
        if let Ok(draft) = snapshot.draft.lock()
            && !draft.composing
            && !composing.get()
        {
            query.set(draft.text.clone());
        }
    });
    ns::watch_scroll(
        move || viewport.node(),
        move |event| {
            state.get_untracked().scroll.fetch_add(1, Ordering::AcqRel);
            offset.set(event.offset);
            revision.update(|revision| *revision = revision.wrapping_add(1));
        },
    );
    let popup = matches!(initial.kind, Kind::Projects | Kind::Backends | Kind::Files);
    let mut content = ns::view()
        .size_full()
        .text_size(ns::px(Typography::SIDEBAR));
    if popup {
        content = content
            .rounded(ns::px(Layout::MENU_RADIUS))
            .border(ns::px(1.))
            .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
            .bg(move || crate::nsh::color(state.get().colors.bg_elevated))
            .py(ns::px(4.));
    }
    if initial.kind == Kind::Projects && !initial.failure {
        let change = commands.clone();
        let composition = commands.clone();
        content = content.child(
            nsh_base::input()
                .element_ref(&search)
                .label(move || state.with(|_| vega_i18n::t("ui.search_projects")))
                .placeholder(move || state.with(|_| vega_i18n::t("ui.search_projects").to_owned()))
                .value(move || query.get())
                .h(ns::px(crate::menu_list::MENU_ROW_HEIGHT))
                .flex_shrink_0()
                .px(ns::px(crate::menu_list::MENU_ROW_PADDING_X))
                .text_color(move || crate::nsh::color(state.get().colors.text_primary))
                .on_layout(move |_: &ns::Bounds<ns::Points>| {
                    if !entered.get_untracked() {
                        entered.set(true);
                        search.focus();
                    }
                })
                .on_change(move |text: &str| {
                    let input = state.get_untracked();
                    query.set(text.to_owned());
                    if let Ok(mut draft) = input.draft.lock() {
                        draft.text = text.to_owned();
                        draft.composing = composing.get_untracked();
                        draft.revision = draft.revision.wrapping_add(1);
                    }
                    owner.run(|| change.send(Request::Edit(input.id)));
                })
                .on_composition_change(move |event: &ns::Composition| {
                    let input = state.get_untracked();
                    composing.set(event.composing);
                    if let Ok(mut draft) = input.draft.lock() {
                        draft.composing = event.composing;
                        draft.revision = draft.revision.wrapping_add(1);
                    }
                    owner.run(|| composition.send(Request::Edit(input.id)));
                }),
        );
    }
    let rows = ns::dynamic(move || {
        let snapshot = state.get();
        let colors = snapshot.colors;
        let mut rows = Vec::new();
        for (index, row) in snapshot.rows.iter().enumerate() {
            let target = ns::element_ref();
            let observed = snapshot.clone();
            let measured = commands.clone();
            let presentation = nsh_core::element::Owner::current();
            let lease = Arc::new(());
            let row_lease = Arc::downgrade(&lease);
            nsh_core::element::cleanup(move || drop(lease));
            let measured_lease = row_lease.clone();
            ns::effect(move || {
                let stamp = revision.get();
                let observed = observed.clone();
                let scroll = observed.scroll.load(Ordering::Acquire);
                let measured = measured.clone();
                let lease = measured_lease.clone();
                owner.run(|| {
                    ns::spawn(async move {
                        let (Some(control), Some(container)) =
                            (target.measure().await, viewport.measure().await)
                        else {
                            return;
                        };
                        if !presentation.is_live()
                            || revision.get_untracked() != stamp
                            || observed.scroll.load(Ordering::Acquire) != scroll
                        {
                            return;
                        }
                        let control = page_bounds(control);
                        let container = page_bounds(container);
                        let current = offset.get_untracked();
                        if observed.focused
                            && observed.selected == Some(index)
                            && let Some(destination) = reveal(control, container, current.y.0)
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
                        measured.send(Request::Displayed(
                            Box::new(observed),
                            index,
                            visible(control, container).then_some(control),
                            scroll,
                            lease,
                        ));
                    })
                });
            });
            let hover = ns::signal(false);
            let down = ns::signal(false);
            let enabled = row.enabled && row.action.is_some();
            let selected = snapshot.focused && snapshot.selected == Some(index);
            let row_checked = row.checked == Some(true);
            let open = match snapshot.kind {
                Kind::Project => snapshot.source_project_open(),
                Kind::Backend => snapshot.source_backend_open(),
                _ => false,
            };
            let background = if popup {
                colors.bg_elevated
            } else {
                colors.bg_sidebar
            };
            let label = row.label.clone();
            let error = snapshot.kind == Kind::Files && row.icon == Some(Icon::Warning);
            let press = commands.clone();
            let observed = snapshot.clone();
            let mut control = ns::view()
                .element_ref(&target)
                .accessibility_elements_hidden(true)
                .role(if row.checked.is_some() {
                    nsh_core::AccessibilityRole::Radio
                } else {
                    nsh_core::AccessibilityRole::Button
                })
                .label(label.clone())
                .disabled(!enabled)
                .accessibility_state(nsh_core::AccessibilityState {
                    disabled: !enabled,
                    selected: row.checked.unwrap_or(false),
                    checked: row.checked.map(|checked| {
                        if checked {
                            nsh_core::Checked::True
                        } else {
                            nsh_core::Checked::False
                        }
                    }),
                    expanded: (!popup && enabled).then_some(open),
                    ..Default::default()
                })
                .h(ns::px(if popup {
                    crate::menu_list::MENU_ROW_HEIGHT
                } else {
                    Layout::COMPOSER_UTILITY_CHIP_HEIGHT
                }))
                .w_full()
                .flex_shrink_0()
                .flex_row()
                .items_center()
                .gap(ns::px(Layout::COMPOSER_UTILITY_CHIP_GAP))
                .px(ns::px(if popup {
                    crate::menu_list::MENU_ROW_PADDING_X
                } else {
                    Layout::COMPOSER_UTILITY_CHIP_PADDING_X
                }))
                .rounded(ns::px(if popup {
                    Layout::SETTINGS_CONTROL_RADIUS
                } else {
                    Layout::COMPOSER_UTILITY_CHIP_RADIUS
                }))
                .border(ns::px(1.))
                .border_color(crate::nsh::color(if selected {
                    colors.accent
                } else {
                    background.opacity(0.)
                }))
                .bg(move || {
                    crate::nsh::color(
                        if enabled && (hover.get() || down.get() || open || row_checked) {
                            if popup {
                                colors.bg_hover
                            } else {
                                colors.bg_utility_chip_overlay
                            }
                        } else {
                            background.opacity(0.)
                        },
                    )
                })
                .text_size(ns::px(Typography::SIDEBAR))
                .text_color(crate::nsh::color(if error {
                    colors.danger
                } else if enabled {
                    colors.text_primary
                } else {
                    colors.text_tertiary
                }))
                .on_layout(move |_: &ns::Bounds<ns::Points>| {
                    state.get_untracked().scroll.fetch_add(1, Ordering::AcqRel);
                    revision.update(|revision| *revision = revision.wrapping_add(1));
                })
                .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
                .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                    hover.set(false);
                    down.set(false);
                })
                .on_press_down(move || {
                    if enabled {
                        down.set(true);
                    }
                })
                .on_press_up(move || down.set(false))
                .on_press(move || {
                    if enabled && row_lease.upgrade().is_some() {
                        owner.run(|| {
                            press.send(Request::Activate(
                                Box::new(observed.clone()),
                                index,
                                row_lease.clone(),
                            ))
                        });
                    }
                });
            if let Some(hint) = &row.hint {
                control = control.hint(hint.clone()).tooltip(
                    ns::text(hint.clone())
                        .text_size(ns::px(Typography::SIDEBAR))
                        .text_color(crate::nsh::color(colors.text_primary)),
                );
            }
            if let Some(icon) = row.icon {
                control = control.child(svg(
                    icon,
                    if error {
                        colors.danger
                    } else {
                        colors.text_secondary
                    },
                ));
            }
            control = control.child(ns::text(label).flex_1().min_w(ns::px(0.)).truncate());
            if row.checked == Some(true) && snapshot.kind != Kind::Files {
                control = control.child(svg(Icon::Check, colors.text_primary));
            }
            rows.push(control.into_element());
        }
        ns::view().w_full().children(rows)
    });
    let on_layout = move |_: &ns::Bounds<ns::Points>| {
        state.get_untracked().scroll.fetch_add(1, Ordering::AcqRel);
        revision.update(|revision| *revision = revision.wrapping_add(1));
    };
    if !popup {
        return content
            .element_ref(&viewport)
            .on_layout(on_layout)
            .child(rows)
            .into_element();
    }
    content
        .child(
            ns::scroll()
                .element_ref(&viewport)
                .flex_1()
                .min_h(ns::px(0.))
                .w_full()
                .on_layout(on_layout)
                .child(rows),
        )
        .into_element()
}

fn svg(icon: Icon, color: gpui_kit::Rgba) -> ns::AnyElement {
    ns::svg(nsh_core::element::ImageSrc::svg_markup(
        &nsh_source(icon).unwrap_or_default(),
    ))
    .size(ns::px(Layout::ICON_SIZE))
    .text_color(crate::nsh::color(color))
    .accessible(false)
    .into_element()
}

pub(super) fn page_bounds(measurement: nsh_core::Measurement) -> ns::Bounds<ns::Points> {
    ns::Bounds {
        origin: measurement.page,
        size: measurement.frame.size,
    }
}

pub(super) fn visible(control: ns::Bounds<ns::Points>, viewport: ns::Bounds<ns::Points>) -> bool {
    let values = [
        control.origin.x.0,
        control.origin.y.0,
        control.size.width.0,
        control.size.height.0,
        viewport.origin.x.0,
        viewport.origin.y.0,
        viewport.size.width.0,
        viewport.size.height.0,
    ];
    values.iter().all(|value| value.is_finite())
        && control.size.width.0 > 0.
        && control.size.height.0 > 0.
        && viewport.size.width.0 > 0.
        && viewport.size.height.0 > 0.
        && control.origin.x.0 >= viewport.origin.x.0 - 0.5
        && control.origin.x.0 + control.size.width.0
            <= viewport.origin.x.0 + viewport.size.width.0 + 0.5
        && control.origin.y.0 >= viewport.origin.y.0 - 0.5
        && control.origin.y.0 + control.size.height.0
            <= viewport.origin.y.0 + viewport.size.height.0 + 0.5
}

pub(super) fn reveal(
    control: ns::Bounds<ns::Points>,
    viewport: ns::Bounds<ns::Points>,
    offset: f32,
) -> Option<f32> {
    if visible(control, viewport) {
        return None;
    }
    let top = control.origin.y.0;
    let start = viewport.origin.y.0;
    let height = control.size.height.0;
    let room = viewport.size.height.0;
    if ![top, start, height, room, offset]
        .iter()
        .all(|value| value.is_finite())
        || height <= 0.
        || room <= 0.
    {
        return None;
    }
    let destination = if height >= room || top < start {
        offset + top - start
    } else {
        offset + top + height - start - room
    };
    ((destination - offset).abs() > 0.5).then_some(destination.max(0.))
}
