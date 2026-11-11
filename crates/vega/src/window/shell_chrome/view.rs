use super::{Action, Kind, Request, Snapshot};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};
use vega_ui::nsh::{OrderedCommands, color};

fn glyph(action: Action, ink: gpui_kit::Rgba) -> AnyElement {
    let path = match action {
        Action::Environment => "<path d='M3 6h.01M8 6h13M3 12h.01M8 12h13M3 18h.01M8 18h13'/>",
        Action::Terminal => "<rect x='3' y='3' width='18' height='18' rx='2'/><path d='M3 15h18'/>",
        Action::Panel => "<rect x='3' y='3' width='18' height='18' rx='2'/><path d='M15 3v18'/>",
        Action::Update | Action::PermissionSettings => "",
    };
    let source = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'>{path}</svg>"
    );
    svg(ImageSrc::svg_markup(&source))
        .size(px(Layout::ICON_SIZE))
        .text_color(color(ink))
        .into_element()
}

fn rect(bounds: &Bounds<Points>) -> [f32; 4] {
    [
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
    ]
}

pub(super) fn render(incoming: Signal<Snapshot>, sender: mpsc::Sender<Request>) -> AnyElement {
    let commands = OrderedCommands::new(sender);
    let command_owner = nsh_core::element::Owner::current();
    let content = signal_with(incoming.get_untracked(), SignalEquals::Never);
    effect(move || {
        let next = incoming.get();
        if !content.get_untracked().matches(&next) {
            content.set(next);
        }
    });
    view()
        .size_full()
        .overflow_hidden()
        .child(dynamic(move || {
            let snapshot = content.get();
            let colors = snapshot.colors;
            let root = signal(None::<[f32; 4]>);
            let controls = signal(Vec::<(Action, [f32; 4])>::new());
            let count = snapshot.observation.controls.len();
            let acknowledgements = commands.clone();
            let observed = snapshot.clone();
            effect(move || {
                if let Some(root) = root.get() {
                    let controls = controls.get();
                    if controls.len() == count {
                        acknowledgements.send(Request::Layout(observed.clone(), root, controls));
                    }
                }
            });
            let mut body = view()
                .size_full()
                .overflow_hidden()
                .on_layout(move |bounds: &Bounds<Points>| root.set(Some(rect(bounds))))
                .flex_row()
                .items_center()
                .gap(px(if snapshot.observation.kind == Kind::Slots {
                    Layout::SHELL_SLOT_GAP
                } else {
                    Layout::TITLEBAR_CONTROL_GAP
                }))
                .text_color(color(colors.text_primary));
            match snapshot.observation.kind {
                Kind::Header => {
                    body = body.child(
                        text(snapshot.observation.title.clone())
                            .min_w(px(0.))
                            .flex_1()
                            .truncate()
                            .text_size(px(Typography::HEADING_PAGE))
                            .label(snapshot.observation.title.clone())
                            .font_weight(FontWeight(Typography::HEADING_PAGE_WEIGHT.0 as u16)),
                    );
                }
                Kind::Error => {
                    let sender = commands.clone();
                    let observed = snapshot.clone();
                    return view()
                        .w_full()
                        .px(px(Layout::SETTINGS_CONTENT_GAP))
                        .py(px(Layout::TITLEBAR_CONTROL_GAP))
                        .on_layout(move |bounds: &Bounds<Points>| {
                            sender.send(Request::Height(observed.clone(), bounds.size.height.0))
                        })
                        .text_size(px(Typography::METADATA))
                        .line_height(px(Typography::METADATA * Typography::BODY_LINE_HEIGHT))
                        .text_color(color(colors.text_secondary))
                        .child(text(snapshot.observation.title.clone()))
                        .into_element();
                }
                Kind::PermissionReset => {
                    let sender = commands.clone();
                    let observed = snapshot.clone();
                    body = body
                        .h(nsh_core::geometry::Length::Auto)
                        .px_3()
                        .py_1()
                        .on_layout(move |bounds: &Bounds<Points>| {
                            root.set(Some(rect(bounds)));
                            sender.send(Request::Height(observed.clone(), bounds.size.height.0));
                        })
                        .child(
                            text(snapshot.observation.title.clone())
                                .min_w(px(0.))
                                .flex_1()
                                .text_size(px(Typography::METADATA))
                                .line_height(
                                    px(Typography::METADATA * Typography::BODY_LINE_HEIGHT),
                                )
                                .text_color(color(colors.warning)),
                        );
                }
                Kind::Slots => {}
            }
            for control in &snapshot.observation.controls {
                let action = control.action;
                let focused = snapshot.focused == Some(action);
                let enabled = control.enabled;
                let selected = control.selected;
                let background = if action == Action::Update {
                    colors.brand_primary
                } else if selected {
                    colors.bg_active
                } else {
                    colors.bg_base.opacity(0.)
                };
                let foreground = if action == Action::Update {
                    colors.brand_on_accent
                } else if enabled {
                    colors.text_secondary
                } else {
                    colors.text_tertiary
                };
                let mut button = view()
                    .role(AccessibilityRole::Button)
                    .label(control.label.clone())
                    .disabled(!enabled)
                    .h(px(Layout::TITLEBAR_CONTROL_SIZE))
                    .flex_shrink_0()
                    .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                    .border(px(1.))
                    .border_color(color(if focused {
                        colors.accent
                    } else {
                        colors.bg_base.opacity(0.)
                    }))
                    .bg(color(background))
                    .text_color(color(foreground))
                    .flex_row()
                    .items_center()
                    .justify_center();
                if action == Action::PermissionSettings {
                    button = button
                        .px_2()
                        .text_size(px(Typography::METADATA))
                        .child(text(control.label.clone()));
                } else if action == Action::Update {
                    button = button
                        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
                        .mr(px(Layout::SETTINGS_CONTENT_GAP))
                        .rounded(px(Layout::TITLEBAR_CONTROL_SIZE / 2.))
                        .text_size(px(Typography::BODY))
                        .child(text(control.label.clone()));
                } else {
                    button =
                        button
                            .w(px(Layout::TITLEBAR_CONTROL_SIZE))
                            .child(glyph(action, foreground))
                            .tooltip(view().px_2().py_1().bg(color(colors.bg_elevated)).child(
                                text(if action == Action::Terminal {
                                    format!("{}  ⌘J", control.label)
                                } else {
                                    control.label.clone()
                                }),
                            ));
                }
                if enabled {
                    let observed = snapshot.clone();
                    let sender = commands.clone();
                    button = button
                        .hover(move |style| {
                            style.bg(color(if action == Action::Update {
                                colors.brand_primary_strong
                            } else {
                                colors.bg_hover
                            }))
                        })
                        .active(move |style| {
                            style.bg(color(if action == Action::Update {
                                colors.brand_primary_strong
                            } else {
                                colors.bg_active
                            }))
                        })
                        .on_press(move || {
                            command_owner
                                .run(|| sender.send(Request::Activate(observed.clone(), action)))
                        });
                }
                let record_bounds = move |bounds: &Bounds<Points>| {
                    let mut values = controls.get_untracked();
                    values.retain(|(owned, _)| *owned != action);
                    values.push((action, rect(bounds)));
                    controls.set(values);
                };
                let wrapper = view()
                    .accessible(false)
                    .accessibility_elements_hidden(enabled);
                // Slot buttons fill their wrappers. Only the wrapper's layout
                // reports its offset in the row shared by the native hit targets.
                body = body.child(if snapshot.observation.kind == Kind::Slots {
                    wrapper.on_layout(record_bounds).child(button)
                } else {
                    wrapper.child(button.on_layout(record_bounds))
                });
            }
            body.into_element()
        }))
        .into_element()
}
