use super::{PAD, ROW, Request, Snapshot};
use crate::nsh::{OrderedCommands, color};
use nsh_core::prelude::*;
use tokio::sync::mpsc;

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
    let owner = nsh_core::element::Owner::current();
    let content = signal_with(incoming.get_untracked(), SignalEquals::Never);
    effect(move || {
        let next = incoming.get();
        let old = content.get_untracked();
        if !next.matches(&old)
            || next.focused != old.focused
            || next.hovered != old.hovered
            || next.pressed != old.pressed
        {
            content.set(next);
        }
    });
    view()
        .size_full()
        .accessible(false)
        .accessibility_elements_hidden(true)
        .child(dynamic(move || {
            let snapshot = content.get();
            let count = snapshot.items.len();
            let root = signal(None::<[f32; 4]>);
            let rows = signal(vec![None::<[f32; 4]>; count]);
            let layout = commands.clone();
            let observed = snapshot.clone();
            effect(move || {
                if let Some(root) = root.get()
                    && let Some(rows) = rows.get().into_iter().collect::<Option<Vec<_>>>()
                {
                    owner.run(|| layout.send(Request::Layout(observed.clone(), root, rows)));
                }
            });
            let mut menu = view()
                .size_full()
                .flex_col()
                .p(px(PAD))
                .rounded(px(6.))
                .border(px(1.))
                .border_color(color(snapshot.colors.border_subtle))
                .bg(color(snapshot.colors.bg_elevated))
                .overflow_hidden()
                .on_layout(move |bounds: &Bounds<Points>| root.set(Some(rect(bounds))));
            for (index, item) in snapshot.items.iter().enumerate() {
                let send = commands.clone();
                let observed = snapshot.clone();
                let colors = snapshot.colors;
                let enabled = item.enabled;
                menu = menu.child(
                    view()
                        .w_full()
                        .h(px(ROW))
                        .flex_shrink(0.)
                        .px(px(10.))
                        .rounded(px(3.))
                        .flex_row()
                        .items_center()
                        .bg(color(
                            if enabled
                                && (snapshot.focused == Some(index)
                                    || snapshot.pressed == Some(index))
                            {
                                colors.bg_active
                            } else if enabled && snapshot.hovered == Some(index) {
                                colors.bg_hover
                            } else {
                                colors.bg_elevated
                            },
                        ))
                        .hover(move |style| {
                            style.bg(color(if enabled {
                                colors.bg_hover
                            } else {
                                colors.bg_elevated
                            }))
                        })
                        .on_layout(move |bounds: &Bounds<Points>| {
                            let mut next = rows.get_untracked();
                            next[index] = Some(rect(bounds));
                            rows.set(next);
                        })
                        .on_press(move || {
                            if enabled {
                                owner.run(|| send.send(Request::Activate(observed.clone(), index)));
                            }
                        })
                        .child(
                            text(item.label.clone())
                                .text_size(px(13.))
                                .text_color(color(if enabled {
                                    colors.text_primary
                                } else {
                                    colors.text_secondary
                                })),
                        ),
                );
            }
            menu.into_element()
        }))
        .into_element()
}
