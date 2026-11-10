use super::{Action, Control, Mode, Request, Snapshot};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};
use vega_ui::icons::Icon;
use vega_ui::nsh::{OrderedCommands, color};

fn glyph(kind: Icon, ink: gpui_kit::Rgba) -> AnyElement {
    let source = vega_ui::icons::nsh_source(kind).unwrap_or_default();
    svg(ImageSrc::svg_markup(&source))
        .size(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .text_color(color(ink))
        .tint_color(color(ink).to_hsla())
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
    let grouped = matches!(control.action, Action::Select(_));
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
        .bg(color(if chosen && !grouped {
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
    if control.icon_only {
        result = result.w(px(Layout::TAB_HEIGHT)).px_0().justify_center();
    }
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
            let body = if snapshot.mode == Mode::Header {
                let viewport = element_ref();
                let offset = signal((0f32, 0f32));
                let scroll_commands = commands.clone();
                let scroll_id = snapshot.id.clone();
                let mut root = view()
                    .size_full()
                    .min_w_0()
                    .flex_row()
                    .items_center()
                    .px_1()
                    .gap_1();
                if let Some(hide) = snapshot.controls.first().cloned() {
                    root = root.child(button(state, &snapshot, hide, commands.clone(), None));
                }
                let mut strip = scroll()
                    .min_w_0()
                    .flex_1()
                    .h_full()
                    .horizontal()
                    .element_ref(&viewport)
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .on_scroll(move |event: &ScrollEvent| {
                        offset.set((event.offset.x.0, event.offset.y.0));
                        scroll_commands.send(Request::Scrolled(scroll_id.clone()));
                    });
                for tab in snapshot.tabs.clone() {
                    let group = view()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .rounded(px(Layout::TAB_RADIUS))
                        .bg(color(if tab.open.chosen {
                            colors.bg_active_alpha
                        } else {
                            colors.bg_base.alpha(0.)
                        }))
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
                    strip = strip.child(group);
                }
                root = root.child(strip);
                for control in snapshot.controls.iter().skip(1).cloned() {
                    root = root.child(button(state, &snapshot, control, commands.clone(), None));
                }
                root.into_element()
            } else {
                let mut root = scroll().size_full().min_w_0();
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
                root.into_element()
            };
            view()
                .size_full()
                .min_w_0()
                .bg(color(
                    if snapshot.mode == Mode::Environment || snapshot.mode == Mode::Menu {
                        colors.bg_elevated
                    } else {
                        colors.bg_base
                    },
                ))
                .child(body)
                .child(when(
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
