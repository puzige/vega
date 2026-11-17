use super::{Request, Snapshot};
use crate::icons::{Icon, nsh_source};
use crate::nsh::OrderedCommands;
use nsh_core::prelude::*;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

pub(super) fn header(state: Signal<Snapshot>, sender: mpsc::Sender<Request>) -> AnyElement {
    let commands = OrderedCommands::new(sender);
    let owner = nsh_core::element::Owner::current();
    let phase = signal(0.0f32);
    let hover = signal(false);
    let pressed = signal(false);
    let title = text("")
        .min_w(px(0.))
        .flex_shrink(1.)
        .number_of_lines(1)
        .child(each(
            move || {
                state.with(|snapshot| {
                    snapshot
                        .authority
                        .title
                        .chars()
                        .enumerate()
                        .collect::<Vec<_>>()
                })
            },
            |(index, _)| *index,
            move |part, _| {
                text(move || part.get().1.to_string()).text_color(move || {
                    state.with(|snapshot| {
                        let strength = if snapshot.epoch.is_some() && !reduce_motion() {
                            let center = phase.get().fract() * 1.6 - 0.3;
                            let x = part.get().0 as f32
                                / snapshot.authority.title.chars().count().max(1) as f32;
                            (1. - (x - center).abs() / 0.24).max(0.)
                        } else {
                            0.
                        };
                        let base = snapshot.colors.conversation_tool;
                        let highlight = snapshot.colors.text_primary;
                        crate::nsh::color(gpui_kit::Rgba {
                            r: base.r + (highlight.r - base.r) * strength,
                            g: base.g + (highlight.g - base.g) * strength,
                            b: base.b + (highlight.b - base.b) * strength,
                            a: base.a,
                        })
                    })
                })
            },
        ));
    view()
        .w_full()
        .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
        .min_w(px(0.))
        .flex_row()
        .items_center()
        .gap(px(Layout::CONVERSATION_INLINE_GAP))
        .rounded(px(Layout::DISCLOSURE_RADIUS))
        .text_size(px(Typography::CONVERSATION))
        .line_height(px(
            Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT
        ))
        .bg(move || {
            state.with(|snapshot| {
                crate::nsh::color(
                    if snapshot.authority.available && snapshot.authority.visible && pressed.get() {
                        snapshot.colors.bg_active
                    } else if snapshot.authority.available
                        && snapshot.authority.visible
                        && (hover.get() || snapshot.focused)
                    {
                        snapshot.colors.bg_hover
                    } else {
                        snapshot.colors.bg_base.opacity(0.)
                    },
                )
            })
        })
        .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
        .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
            hover.set(false);
            pressed.set(false);
        })
        .on_press_down(move || pressed.set(true))
        .on_press_up(move || pressed.set(false))
        .on_press(move || {
            let observed = state.get_untracked();
            if observed.authority.available && observed.authority.visible {
                owner.run(|| commands.send(Request::Activate(observed)));
            }
        })
        .child(when(
            move || state.with(|snapshot| snapshot.epoch.is_some()) && !reduce_motion(),
            move || {
                spawn(async move {
                    loop {
                        next_frame().await;
                        if let Some(epoch) = state.with_untracked(|snapshot| snapshot.epoch) {
                            phase.set(
                                epoch.elapsed().as_secs_f32() * 1000.
                                    / vega_theme::Motion::THINKING_SHIMMER_MS as f32,
                            );
                        }
                    }
                });
                view()
            },
        ))
        .child(
            svg(nsh_core::element::ImageSrc::svg_markup(
                &nsh_source(Icon::Bot).unwrap_or_default(),
            ))
            .size(px(Layout::ICON_SIZE))
            .flex_shrink_0()
            .text_color(move || {
                state.with(|snapshot| crate::nsh::color(snapshot.colors.conversation_tool))
            }),
        )
        .child(title)
        .child(dynamic(move || {
            let snapshot = state.get();
            let source = nsh_source(if snapshot.authority.expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            })
            .unwrap_or_default();
            let visible = snapshot.authority.available
                && snapshot.authority.visible
                && (hover.get() || snapshot.focused);
            svg(nsh_core::element::ImageSrc::svg_markup(&source))
                .w(px(Layout::DISCLOSURE_TRAILING_SLOT))
                .h(px(Layout::ICON_SIZE))
                .flex_shrink_0()
                .opacity(if visible { 1. } else { 0. })
                .text_color(crate::nsh::color(snapshot.colors.text_tertiary))
        }))
        .into_element()
}

pub(super) fn omission(state: Signal<Snapshot>, _: mpsc::Sender<Request>) -> AnyElement {
    text(move || state.with(|snapshot| snapshot.omission.clone()))
        .w_full()
        .text_size(px(Typography::METADATA))
        .line_height(px(
            Typography::METADATA * Typography::CONVERSATION_LINE_HEIGHT
        ))
        .text_color(move || state.with(|snapshot| crate::nsh::color(snapshot.colors.text_tertiary)))
        .into_element()
}
