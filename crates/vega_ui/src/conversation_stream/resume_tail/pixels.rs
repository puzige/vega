use super::{Request, Snapshot};
use crate::nsh::{OrderedCommands, color};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use tokio::sync::mpsc;
use vega_theme::Layout;

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
        let previous = content.get_untracked();
        if !previous.matches(&next)
            || previous.hovered != next.hovered
            || previous.pressed != next.pressed
        {
            content.set(next);
        }
    });
    view()
        .size_full()
        .overflow_hidden()
        .accessible(false)
        .accessibility_elements_hidden(true)
        .child(dynamic(move || {
            let snapshot = content.get();
            let colors = snapshot.colors;
            let root = signal(None::<[f32; 4]>);
            let control = signal(None::<[f32; 4]>);
            let layout = commands.clone();
            let observed = snapshot.clone();
            effect(move || {
                if let (Some(root), Some(control)) = (root.get(), control.get()) {
                    command_owner.run(|| {
                        layout.send(Request::Layout(observed.clone(), root, control))
                    });
                }
            });
            let sender = commands.clone();
            let focused = snapshot.focused;
            let source = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='M12 5v14m-7-7 7 7 7-7'/></svg>";
            view()
                .size_full()
                .py(px(Layout::READING_NAVIGATION_PADDING))
                .flex_row()
                .justify_center()
                .on_layout(move |bounds: &Bounds<Points>| root.set(Some(rect(bounds))))
                .child(
                    view()
                        .size(px(Layout::SCROLL_TO_BOTTOM_SIZE))
                        .flex_shrink_0()
                        .rounded(px(Layout::SCROLL_TO_BOTTOM_SIZE / 2.))
                        .border(px(1.))
                        .border_color(color(if focused {
                            colors.accent
                        } else {
                            colors.border_subtle
                        }))
                        .bg(color(if snapshot.pressed {
                            colors.bg_active
                        } else if snapshot.hovered {
                            colors.bg_hover
                        } else {
                            colors.bg_elevated
                        }))
                        .hover(move |style| style.bg(color(colors.bg_hover)))
                        .active(move |style| style.bg(color(colors.bg_active)))
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .on_layout(move |bounds: &Bounds<Points>| {
                            control.set(Some(rect(bounds)))
                        })
                        .on_press(move || {
                            command_owner.run(|| sender.send(Request::Activate(snapshot.clone())))
                        })
                        .child(
                            svg(ImageSrc::svg_markup(source))
                                .size(px(Layout::ICON_SIZE))
                                .text_color(color(colors.text_secondary)),
                        ),
                )
                .into_element()
        }))
        .into_element()
}
