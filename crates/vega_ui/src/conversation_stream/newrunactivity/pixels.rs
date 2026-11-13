use super::{LayoutAck, Snapshot, Target};
use crate::icons::{Icon, nsh_source};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

pub(super) fn render(
    state: ns::Signal<Snapshot>,
    sender: mpsc::Sender<LayoutAck>,
) -> ns::AnyElement {
    let commands = crate::nsh::OrderedCommands::new(sender);
    let command_owner = nsh_core::element::Owner::current();
    let content = ns::signal_with(state.get_untracked(), ns::SignalEquals::Never);
    ns::effect(move || {
        let next = state.get();
        let previous = content.get_untracked();
        if !previous.same_projection(&next)
            || previous.hovered != next.hovered
            || previous.pressed != next.pressed
        {
            content.set(next);
        }
    });
    let state = content;
    ns::dynamic(move || {
        let snapshot = state.get();
        let acknowledgements = commands.clone();
        let observed = snapshot.clone();
        if state.get().divider {
            return ns::view()
                .w_full()
                .h(ns::px(Layout::CONVERSATION_DIVIDER_HEIGHT))
                .bg(move || crate::nsh::color(state.get().colors.conversation_divider))
                .into_element();
        }
        let stage = matches!(state.get().authority.target, Target::Stage(_));
        ns::view()
            .on_layout(move |bounds: &ns::Bounds<ns::Points>| {
                let ack = LayoutAck {
                    snapshot: observed.clone(),
                    root: [
                        bounds.origin.x.0,
                        bounds.origin.y.0,
                        bounds.size.width.0,
                        bounds.size.height.0,
                    ],
                };
                command_owner.run(|| acknowledgements.send(ack));
            })
            .w_full()
            .min_w(ns::px(0.))
            .h(ns::px(if stage {
                Layout::CONVERSATION_TOOL_HEIGHT
            } else {
                Layout::CONVERSATION_CLOCK_HEIGHT
            }))
            .flex_row()
            .items_center()
            .gap(ns::px(Layout::CONVERSATION_INLINE_GAP))
            .text_size(ns::px(Typography::CONVERSATION))
            .line_height(ns::px(
                Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT,
            ))
            .label(move || state.get().label)
            .bg(move || {
                let snapshot = state.get();
                crate::nsh::color(if snapshot.pressed {
                    snapshot.colors.bg_active
                } else if snapshot.focused || snapshot.hovered {
                    snapshot.colors.bg_hover
                } else {
                    snapshot.colors.bg_base
                })
            })
            .children(state.get().authority.icon.map(|icon| {
                glyph(
                    icon,
                    state.get().colors.conversation_stage,
                    Layout::ICON_SIZE,
                )
            }))
            .child(if stage {
                ns::view()
                    .w(move || ns::px(state.get().title_width))
                    .min_w(ns::px(0.))
                    .flex_shrink(1.)
                    .h(ns::px(Layout::CONVERSATION_TOOL_HEIGHT))
                    .into_element()
            } else {
                ns::text(move || state.get().authority.title)
                    .min_w(ns::px(0.))
                    .flex_shrink(1.)
                    .truncate()
                    .text_color(move || crate::nsh::color(state.get().colors.conversation_clock))
                    .into_element()
            })
            .children((!stage || state.get().authority.can_expand).then(|| {
                ns::view()
                    .size(ns::px(Layout::CONVERSATION_CHEVRON_SIZE))
                    .flex_shrink_0()
                    .opacity(move || {
                        let snapshot = state.get();
                        if snapshot.authority.can_expand
                            && (snapshot.authority.expanded || snapshot.focused || snapshot.hovered)
                        {
                            1_f32
                        } else {
                            0_f32
                        }
                    })
                    .child(ns::dynamic(move || {
                        let snapshot = state.get();
                        glyph(
                            if snapshot.authority.expanded {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            },
                            snapshot.colors.conversation_tool,
                            Layout::CONVERSATION_CHEVRON_SIZE,
                        )
                    }))
            }))
            .into_element()
    })
    .into_element()
}

fn glyph(icon: Icon, tint: gpui_kit::Rgba, size: f32) -> ns::AnyElement {
    ns::svg(nsh_core::element::ImageSrc::svg_markup(
        &nsh_source(icon).unwrap_or_default(),
    ))
    .size(ns::px(size))
    .flex_shrink_0()
    .text_color(crate::nsh::color(tint))
    .tint_color(crate::nsh::color(tint))
    .into_element()
}
