use super::{Action, Request, Snapshot};
use ns::{
    Accessible as _, Element as _, Interactive as _, IntoElement as _, ParentElement as _,
    Styled as _,
};
use nsh_core::prelude as ns;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

pub(super) fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = crate::nsh::OrderedCommands::new(sender);
    let command_owner = nsh_core::element::Owner::current();
    ns::dynamic(move || {
        let observed = state.get();
        let presentation_owner = nsh_core::element::Owner::current();
        let colors = observed.colors;
        let target = ns::element_ref();
        let revision = ns::signal(0u64);
        let displayed = observed.clone();
        let display_commands = commands.clone();
        ns::effect(move || {
            if !command_owner.is_live() || !presentation_owner.is_live() {
                return;
            }
            let observation = revision.get();
            let displayed = displayed.clone();
            let commands = display_commands.clone();
            command_owner.run(|| {
                ns::spawn(async move {
                    if !command_owner.is_live() || !presentation_owner.is_live() {
                        return;
                    }
                    let Some(measurement) = target.measure().await else {
                        return;
                    };
                    if command_owner.is_live()
                        && presentation_owner.is_live()
                        && revision.get_untracked() == observation
                        && measurable(measurement)
                    {
                        commands.send(Request::Displayed(Box::new(displayed)));
                    }
                });
            });
        });
        let width = observed.width;
        let id = observed.id.clone();
        let measure_commands = commands.clone();
        let mut root = ns::view()
            .element_ref(&target)
            .w(ns::px(width))
            .min_w(ns::px(0.))
            .flex_row()
            .flex_wrap()
            .items_center()
            .flex_shrink_0()
            .gap(ns::px(8.))
            .on_layout(move |bounds: &ns::Bounds<ns::Points>| {
                if !command_owner.is_live() || !presentation_owner.is_live() {
                    return;
                }
                revision.update(|value| *value = value.wrapping_add(1));
                measure_commands.send(Request::Layout {
                    id: id.clone(),
                    width,
                    height: bounds.size.height.0,
                });
            });
        if let Some(message) = &observed.message {
            let mut text = ns::view()
                .min_w(ns::px(0.))
                .flex_1()
                .flex_col()
                .child(label(message.clone(), &observed));
            if let Some(detail) = &observed.detail {
                text = text.child(label(detail.clone(), &observed));
            }
            root = root.child(text);
        }
        for (action, name) in &observed.buttons {
            root = root.child(button(
                observed.clone(),
                *action,
                name.clone(),
                PressContext {
                    commands: commands.clone(),
                    command_owner,
                    presentation_owner,
                    target,
                    revision,
                },
            ));
        }
        root.text_color(crate::nsh::color(colors.text_secondary))
            .into_element()
    })
    .into_element()
}

fn label(text: String, snapshot: &Snapshot) -> ns::AnyElement {
    let label = ns::text(text.clone())
        .min_w(ns::px(0.))
        .font_family(snapshot.family.clone())
        .text_size(ns::px(Typography::BODY))
        .line_height(ns::px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
        .text_color(crate::nsh::color(snapshot.colors.text_secondary));
    label.into_element()
}

struct PressContext {
    commands: crate::nsh::OrderedCommands<Request>,
    command_owner: nsh_core::element::Owner,
    presentation_owner: nsh_core::element::Owner,
    target: ns::ElementRef,
    revision: ns::Signal<u64>,
}

fn button(
    observed: Snapshot,
    action: Action,
    name: String,
    context: PressContext,
) -> ns::AnyElement {
    let PressContext {
        commands,
        command_owner,
        presentation_owner,
        target,
        revision,
    } = context;
    let colors = observed.colors;
    let selected = observed.selected == Some(action);
    let enabled = observed.enabled;
    let hover = ns::signal(false);
    let down = ns::signal(false);
    let text = label(name.clone(), &observed);
    let button = ns::view()
        .min_w(ns::px(0.))
        .max_w(ns::px(observed.width))
        .flex_shrink_0()
        .px_2()
        .py_1()
        .rounded(ns::px(Layout::SETTINGS_CONTROL_RADIUS))
        .role(nsh_core::AccessibilityRole::Button)
        .label(name);
    let press = move || {
        if !enabled || !command_owner.is_live() || !presentation_owner.is_live() {
            return;
        }
        let commands = commands.clone();
        let observed = observed.clone();
        command_owner.run(|| {
            ns::spawn(async move {
                if !command_owner.is_live() || !presentation_owner.is_live() {
                    return;
                }
                let observation = revision.get_untracked();
                let Some(measurement) = target.measure().await else {
                    return;
                };
                if command_owner.is_live()
                    && presentation_owner.is_live()
                    && observation == revision.get_untracked()
                    && measurable(measurement)
                {
                    commands.send(Request::Displayed(Box::new(observed.clone())));
                    commands.send(Request::Activate {
                        observed: Box::new(observed),
                        action,
                    });
                }
            });
        });
    };
    button
        .border(ns::px(1.))
        .border_color(crate::nsh::color(if selected {
            colors.accent
        } else {
            colors.bg_base.opacity(0.)
        }))
        .bg(move || {
            crate::nsh::color(if enabled && down.get() {
                colors.bg_active
            } else if enabled && hover.get() {
                colors.bg_hover
            } else {
                colors.bg_base.opacity(0.)
            })
        })
        .on_mouse_enter(move |_: &nsh_core::MouseEvent| {
            if command_owner.is_live() && presentation_owner.is_live() {
                hover.set(true);
            }
        })
        .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
            if command_owner.is_live() && presentation_owner.is_live() {
                hover.set(false);
                down.set(false);
            }
        })
        .on_press_down(move || {
            if command_owner.is_live() && presentation_owner.is_live() {
                down.set(true);
            }
        })
        .on_press_up(move || {
            if command_owner.is_live() && presentation_owner.is_live() {
                down.set(false);
            }
        })
        .on_press(press)
        .child(text)
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
