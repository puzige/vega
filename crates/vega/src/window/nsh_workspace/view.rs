use super::{Action, Control, Mode, Request, Snapshot};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};
use vega_ui::icons::Icon;
use vega_ui::nsh::{OrderedCommands, color};

fn glyph(kind: Icon, ink: gpui_kit::Rgba) -> AnyElement {
    let path = match kind {
        Icon::Close => "<path d='m18 6-12 12M6 6l12 12'/>",
        Icon::Plus => "<path d='M12 5v14M5 12h14'/>",
        Icon::ChevronDown => "<path d='m6 9 6 6 6-6'/>",
        Icon::ChevronRight => "<path d='m9 6 6 6-6 6'/>",
        Icon::Maximize => "<path d='M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5'/>",
        Icon::DockMove => {
            "<rect x='3' y='3' width='18' height='18' rx='2'/><path d='M15 3v18M6 12h6m-3-3 3 3-3 3'/>"
        }
        Icon::Folder => {
            "<path d='M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z'/>"
        }
        Icon::Terminal => "<path d='m4 7 5 5-5 5m8 0h8'/>",
        Icon::Bot => {
            "<rect x='4' y='7' width='16' height='14' rx='2'/><path d='M12 7V3H9M1 13h3m16 0h3M8 12h.01M16 12h.01M9 17h6'/>"
        }
        Icon::Split => "<rect x='3' y='3' width='18' height='18' rx='2'/><path d='M12 3v18'/>",
        Icon::Camera => {
            "<path d='M14 4h-4L8 7H4a2 2 0 0 0-2 2v11h20V9a2 2 0 0 0-2-2h-4Z'/><circle cx='12' cy='13' r='3'/>"
        }
        Icon::Goal => {
            "<circle cx='12' cy='12' r='9'/><circle cx='12' cy='12' r='5'/><circle cx='12' cy='12' r='1'/>"
        }
        Icon::Refresh => {
            "<path d='M3 11a9 9 0 0 1 15-6l3 3M21 3v5h-5M21 13A9 9 0 0 1 6 19l-3-3M3 21v-5h5'/>"
        }
        _ => {
            "<path d='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z'/><path d='M14 2v6h6M8 13h8M8 17h6'/>"
        }
    };
    let source = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'>{path}</svg>"
    );
    svg(ImageSrc::svg_markup(&source))
        .size(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .text_color(color(ink))
        .into_element()
}

fn label(value: String, ink: gpui_kit::Rgba) -> AnyElement {
    text(value)
        .text_size(px(Typography::SIDEBAR))
        .text_color(color(ink))
        .truncate()
        .into_element()
}

type ScrollViewport = (ElementRef, Signal<(f32, f32)>, bool);

fn button(
    state: Signal<Snapshot>,
    observed: &Snapshot,
    control: Control,
    commands: OrderedCommands<Request>,
    viewport: Option<ScrollViewport>,
) -> AnyElement {
    let target = element_ref();
    let action = control.action.clone();
    let measured_action = action.clone();
    let measured = observed.clone();
    let measure_commands = commands.clone();
    let owner = Owner::current();
    let wake = signal(0u8);
    let busy = signal(false);
    let attempts = signal(0u8);
    let reveal_selected = control.chosen;
    let previous_offset = stored(None::<(f32, f32)>);
    effect(move || {
        let current = state.get();
        wake.get();
        let offset = viewport.map(|(_, offset, _)| offset.get());
        let changed = previous_offset.with(|previous| {
            let changed = *previous != offset;
            *previous = offset;
            changed
        });
        if changed {
            attempts.set(0);
        }
        if !current.same(&measured)
            || current.selected.as_ref() != Some(&measured_action)
            || (!current.focused && !reveal_selected && current.accessible.is_none())
            || busy.get_untracked()
            || attempts.get_untracked() >= 8
        {
            return;
        }
        busy.set(true);
        attempts.update(|count| *count += 1);
        let commands = measure_commands.clone();
        let observed = current.clone();
        let action = measured_action.clone();
        owner.run(move || {
            spawn(async move {
                let mut visible = true;
                if let Some((viewport, offset, horizontal)) = viewport {
                    if let Some(bounds) = target.measure().await
                        && let Some(clip) = viewport.measure().await
                    {
                        let (x, y) = offset.get_untracked();
                        let delta = if horizontal {
                            super::reveal_delta(
                                bounds.page.x.0,
                                bounds.frame.size.width.0,
                                clip.page.x.0,
                                clip.frame.size.width.0,
                            )
                        } else {
                            super::reveal_delta(
                                bounds.page.y.0,
                                bounds.frame.size.height.0,
                                clip.page.y.0,
                                clip.frame.size.height.0,
                            )
                        };
                        if !state.get_untracked().same(&observed) {
                            busy.set(false);
                            return;
                        }
                        if let Some(delta) = delta {
                            visible = delta.abs() < 0.5;
                            if !visible {
                                viewport.scroll_to(
                                    point(
                                        px(if horizontal { (x + delta).max(0.) } else { x }),
                                        px(if horizontal { y } else { (y + delta).max(0.) }),
                                    ),
                                    false,
                                );
                            }
                        } else {
                            visible = false;
                        }
                    } else {
                        visible = false;
                    }
                }
                if visible
                    && state.get_untracked().same(&observed)
                    && state.get_untracked().selected == Some(action.clone())
                    && let Some(bounds) = target.measure().await
                {
                    let rect = [
                        bounds.page.x.0,
                        bounds.page.y.0,
                        bounds.frame.size.width.0,
                        bounds.frame.size.height.0,
                    ];
                    let (reply, response) = tokio::sync::oneshot::channel();
                    commands.send(Request::Measured(
                        Box::new(observed.clone()),
                        action.clone(),
                        rect,
                        reply,
                    ));
                    if let Ok(Some(revision)) = response.await {
                        let (reply, response) = tokio::sync::oneshot::channel();
                        commands.send(Request::Displayed(
                            Box::new(observed),
                            action,
                            rect,
                            revision,
                            reply,
                        ));
                        if response.await == Ok(true) {
                            attempts.set(8);
                        }
                    }
                }
                busy.set(false);
                if attempts.get_untracked() < 8 {
                    wake.update(|wake| *wake = wake.saturating_add(1));
                }
            });
        });
    });
    let colors = observed.colors;
    let focused_action = action.clone();
    let accessibility_action = action.clone();
    let accessibility_observed = observed.clone();
    let accessibility_commands = commands.clone();
    let activation_observed = observed.clone();
    let chosen = control.chosen;
    let name = control.label.clone();
    let mut result = view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .label(name.clone())
        .hint(name.clone())
        .accessibility_actions(vec![nsh_core::AccessibilityActionDef {
            name: "activate".into(),
            label: None,
        }])
        .on_accessibility_action(move |name: &str| {
            if name == "activate" {
                accessibility_commands.send(Request::Accessible(
                    Box::new(accessibility_observed.clone()),
                    accessibility_action.clone(),
                ));
            }
        })
        .accessibility_state(move || AccessibilityState {
            selected: chosen,
            ..Default::default()
        })
        .h(px(Layout::TAB_HEIGHT))
        .min_w_0()
        .px_2()
        .flex_row()
        .items_center()
        .gap_1()
        .flex_shrink_0()
        .rounded(px(Layout::TAB_RADIUS))
        .border(px(1.))
        .border_color(move || {
            color(
                if state.get().focused && state.get().selected.as_ref() == Some(&focused_action) {
                    colors.accent
                } else {
                    colors.bg_base.alpha(0.)
                },
            )
        })
        .bg(color(if chosen {
            colors.bg_active_alpha
        } else {
            colors.bg_base.alpha(0.)
        }))
        .hover(move |style| style.bg(color(colors.bg_hover)))
        .tooltip(
            view()
                .px_2()
                .py_1()
                .bg(color(colors.bg_elevated))
                .child(label(name, colors.text_primary)),
        )
        .on_press(move || {
            commands.send(Request::Act(
                Box::new(activation_observed.clone()),
                action.clone(),
            ))
        });
    if let Some(icon) = control.icon {
        result = result.child(glyph(icon, colors.text_secondary));
    }
    if !control.icon_only {
        result = result
            .max_w(px(150.))
            .child(label(control.label, colors.text_primary));
    }
    result.into_element()
}

pub(super) fn render(incoming: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let state = signal_with(incoming.get_untracked(), SignalEquals::Never);
    let content = signal_with(incoming.get_untracked(), SignalEquals::Never);
    effect(move || {
        let next = incoming.get();
        let old = content.get_untracked();
        state.set(next.clone());
        if !next.same(&old) || next.selected != old.selected || next.focused != old.focused {
            content.set(next);
        }
    });
    let commands = OrderedCommands::new(commands);
    let failed = commands.failed;
    view()
        .size_full()
        .child(dynamic(move || {
            let snapshot = content.get();
            let colors = snapshot.colors;
            let mut root = scroll().size_full().min_w_0().bg(color(
                if snapshot.mode == Mode::Environment || snapshot.mode == Mode::Menu {
                    colors.bg_elevated
                } else {
                    colors.bg_base
                },
            ));
            if snapshot.mode == Mode::Header {
                let viewport = element_ref();
                let offset = signal((0f32, 0f32));
                let scroll_commands = commands.clone();
                let scroll_id = snapshot.id.clone();
                root = root
                    .element_ref(&viewport)
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .horizontal()
                    .on_scroll(move |event: &ScrollEvent| {
                        offset.set((event.offset.x.0, event.offset.y.0));
                        scroll_commands.send(Request::Scrolled(scroll_id.clone()));
                    });
                if let Some(hide) = snapshot.controls.first().cloned() {
                    root = root.child(button(
                        state,
                        &snapshot,
                        hide,
                        commands.clone(),
                        Some((viewport, offset, true)),
                    ));
                }
                for tab in snapshot.tabs.clone() {
                    root = root
                        .child(button(
                            state,
                            &snapshot,
                            tab.open,
                            commands.clone(),
                            Some((viewport, offset, true)),
                        ))
                        .child(button(
                            state,
                            &snapshot,
                            tab.close,
                            commands.clone(),
                            Some((viewport, offset, true)),
                        ));
                }
                for control in snapshot.controls.iter().skip(1).cloned() {
                    root = root.child(button(
                        state,
                        &snapshot,
                        control,
                        commands.clone(),
                        Some((viewport, offset, true)),
                    ));
                }
            } else {
                let viewport = element_ref();
                let offset = signal((0f32, 0f32));
                let scroll_commands = commands.clone();
                let scroll_id = snapshot.id.clone();
                root = root
                    .element_ref(&viewport)
                    .flex_col()
                    .gap_1()
                    .p_2()
                    .on_scroll(move |event: &ScrollEvent| {
                        offset.set((event.offset.x.0, event.offset.y.0));
                        scroll_commands.send(Request::Scrolled(scroll_id.clone()));
                    });
                for line in &snapshot.lines {
                    root = root.child(label(
                        line.text.clone(),
                        if line.error {
                            colors.danger
                        } else {
                            colors.text_secondary
                        },
                    ));
                }
                for control in &snapshot.controls {
                    root = root.child(button(
                        state,
                        &snapshot,
                        control.clone(),
                        commands.clone(),
                        Some((viewport, offset, false)),
                    ));
                }
            }
            root.child(when(
                move || failed.get(),
                move || {
                    label(
                        vega_i18n::t("app.workspace.load_failed").into(),
                        colors.danger,
                    )
                },
            ))
            .into_element()
        }))
        .into_element()
}

pub(super) fn recovery(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let commands = OrderedCommands::new(commands);
    view()
        .size_full()
        .child(dynamic(move || {
            let snapshot = state.get();
            view()
                .flex_col()
                .p_2()
                .gap_1()
                .child(label(
                    vega_i18n::t("app.workspace.load_failed").into(),
                    snapshot.colors.danger,
                ))
                .child(button(
                    state,
                    &snapshot,
                    Control::new(Action::Reload, vega_i18n::t("app.workspace.retry"), None),
                    commands.clone(),
                    None,
                ))
                .into_element()
        }))
        .into_element()
}
