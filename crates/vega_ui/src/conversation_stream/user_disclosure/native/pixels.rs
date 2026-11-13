use super::{Request, Snapshot, Witness};
use crate::nsh::OrderedCommands;
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::element::Element as _;
use nsh_core::prelude as ns;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

pub(super) fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    let command_owner = nsh_core::element::Owner::current();
    ns::dynamic(move || {
        let snapshot = state.get();
        let presentation = nsh_core::element::Owner::current();
        let root = ns::element_ref();
        let target = ns::element_ref();
        let hovered = ns::signal(false);
        let pressed = ns::signal(false);
        let colors = snapshot.colors;
        let focused = snapshot.focused;
        let expanded = snapshot.source.stamp.expanded;
        let revision = snapshot.revision.clone();
        let layout_revision = snapshot.revision.clone();
        let observed = snapshot.clone();
        let accessible = snapshot.clone();
        let keyboard = snapshot.clone();
        let pointer_commands = commands.clone();
        let ax_commands = commands.clone();
        let keyboard_commands = commands.clone();
        let activation = snapshot.activation.is_some();
        ns::effect(move || {
            if activation {
                dispatch(
                    keyboard.clone(),
                    target,
                    root,
                    keyboard_commands.clone(),
                    command_owner,
                    presentation,
                );
            }
        });
        let control = ns::view()
            .element_ref(&target)
            .h(ns::px(
                Typography::CONVERSATION * Typography::CONVERSATION_USER_LINE_HEIGHT,
            ))
            .flex_row()
            .items_center()
            .flex_shrink_0()
            .gap(ns::px(Layout::CONVERSATION_INLINE_GAP))
            .rounded(ns::px(Layout::COPY_ACTION_GAP))
            .focusable(false)
            .accessible(true)
            .role(nsh_core::AccessibilityRole::Button)
            .label(snapshot.label.clone())
            .accessibility_state(nsh_core::AccessibilityState {
                expanded: Some(expanded),
                ..Default::default()
            })
            .bg(move || {
                crate::nsh::color(if pressed.get() || focused || hovered.get() {
                    colors.bg_active_alpha
                } else {
                    colors.bg_active_alpha.opacity(0.)
                })
            })
            .on_layout(move |_: &ns::Bounds<ns::Points>| {
                layout_revision.fetch_add(1, Ordering::AcqRel);
            })
            .on_mouse_enter(move |_: &nsh_core::MouseEvent| hovered.set(true))
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                hovered.set(false);
                pressed.set(false);
            })
            .on_press_down(move || pressed.set(true))
            .on_press_up(move || pressed.set(false))
            .on_press(move || {
                dispatch(
                    observed.clone(),
                    target,
                    root,
                    pointer_commands.clone(),
                    command_owner,
                    presentation,
                )
            })
            .accessibility_actions(vec![nsh_core::AccessibilityActionDef {
                name: "activate".into(),
                label: None,
            }])
            .on_accessibility_action(move |name: &str| {
                if name == "activate" {
                    dispatch(
                        accessible.clone(),
                        target,
                        root,
                        ax_commands.clone(),
                        command_owner,
                        presentation,
                    );
                }
            })
            .child(
                ns::text(snapshot.label)
                    .text_size(ns::px(Typography::BODY))
                    .text_color(crate::nsh::color(colors.conversation_user)),
            )
            .child(
                ns::svg(nsh_core::element::ImageSrc::svg_markup(
                    &crate::icons::nsh_source(crate::icons::Icon::ChevronDown).unwrap_or_default(),
                ))
                .size(ns::px(Layout::CONVERSATION_CHEVRON_SIZE))
                .flex_shrink_0()
                .text_color(crate::nsh::color(colors.conversation_user))
                .rotate(ns::deg(if expanded { 180. } else { 0. })),
            );
        ns::view()
            .element_ref(&root)
            .w_full()
            .h(ns::px(
                Typography::CONVERSATION * Typography::CONVERSATION_USER_LINE_HEIGHT,
            ))
            .flex_row()
            .items_center()
            .on_layout(move |_: &ns::Bounds<ns::Points>| {
                revision.fetch_add(1, Ordering::AcqRel);
            })
            .child(control)
            .into_element()
    })
    .into_element()
}

fn dispatch(
    observed: Snapshot,
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
        && control[0] + control[2] <= root[0] + root[2]
        && control[1] + control[3] <= root[1] + root[3]
}
