use super::{Request, Snapshot};
use crate::nsh::OrderedCommands;
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

pub(super) fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    let measured = ns::signal((0.0_f32, 0.0_f32));
    let initial = state.get_untracked();
    let structure = ns::signal_with(initial.clone(), ns::SignalEquals::Never);
    ns::effect(move || {
        let next = state.get();
        let previous = structure.get_untracked();
        if !next.id.matches(&previous.id)
            || next.items != previous.items
            || next.history != previous.history
            || next.pending != previous.pending
            || next.error != previous.error
            || next.recovery != previous.recovery
            || next.focused != previous.focused
            || next.language != previous.language
            || next.colors.bg_base != previous.colors.bg_base
            || next.colors.text_primary != previous.colors.text_primary
        {
            structure.set(next);
        }
    });
    ns::dynamic(move || {
        let snapshot = structure.get();
        let colors = snapshot.colors;
        let layout = commands.clone();
        let id = snapshot.id.clone();
        let root = ns::view()
            .w_full()
            .min_w(ns::px(0.))
            .flex_col()
            .gap(ns::px(8.))
            .on_layout(move |bounds: &ns::Bounds<ns::Points>| {
                let width = bounds.size.width.0;
                let height = bounds.size.height.0;
                let previous = measured.get_untracked();
                if width.is_finite()
                    && width > 0.
                    && height.is_finite()
                    && height >= 0.
                    && ((previous.0 - width).abs() > 0.5 || (previous.1 - height).abs() > 0.5)
                {
                    measured.set((width, height));
                    layout.send(Request::Layout {
                        id: id.clone(),
                        height,
                    });
                }
            });
        if snapshot.recovery {
            let activate = commands.clone();
            let id = snapshot.id.clone();
            let hover = ns::signal(false);
            let owner = nsh_core::element::Owner::current();
            return root
                .child(
                    ns::view()
                        .relative()
                        .h(ns::px(32.))
                        .w_full()
                        .border(ns::px(1.))
                        .border_color(crate::nsh::color(if snapshot.focused {
                            colors.accent
                        } else {
                            colors.bg_elevated
                        }))
                        .rounded(ns::px(6.))
                        .bg(move || {
                            crate::nsh::color(if hover.get() {
                                colors.bg_hover
                            } else {
                                colors.bg_elevated
                            })
                        })
                        .role(nsh_core::AccessibilityRole::Button)
                        .label(vega_i18n::t("ui.reload_view"))
                        .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
                        .on_mouse_leave(move |_: &nsh_core::MouseEvent| hover.set(false))
                        .on_press(move || owner.run(|| activate.send(Request::Reload(id.clone()))))
                        .child(
                            ns::text(vega_i18n::t("ui.reload_view"))
                                .text_size(ns::px(Typography::BODY))
                                .text_color(crate::nsh::color(colors.text_primary)),
                        )
                        .child(
                            ns::native("attachment-recovery-focus")
                                .absolute()
                                .inset(ns::px(0.)),
                        ),
                )
                .into_element();
        }
        let mut grid = ns::view()
            .w_full()
            .min_w(ns::px(0.))
            .flex_row()
            .flex_wrap()
            .gap(ns::px(8.));
        if snapshot.history {
            grid = grid.justify_end();
        }
        for index in 0..snapshot.items.len() {
            let mut item = ns::view()
                .relative()
                .w(ns::px(Layout::ATTACHMENT_THUMBNAIL))
                .h(ns::px(Layout::ATTACHMENT_THUMBNAIL))
                .flex_shrink_0()
                .child(
                    ns::native(&format!("attachment-thumbnail-{index}"))
                        .w_full()
                        .h_full(),
                );
            if !snapshot.history {
                item = item.child(
                    ns::native(&format!("attachment-remove-{index}"))
                        .absolute()
                        .top(ns::px(0.))
                        .right(ns::px(0.))
                        .w(ns::px(24.))
                        .h(ns::px(24.)),
                );
            }
            grid = grid.child(item);
        }
        let root = if snapshot.history {
            root.pt(ns::px(Layout::USER_MESSAGE_PADDING_TOP))
        } else {
            root
        };
        let mut root = root.child(grid);
        if snapshot.pending {
            root = root.child(
                ns::text(vega_i18n::t("ui.adding_images"))
                    .text_size(ns::px(Typography::BODY))
                    .line_height(ns::px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
                    .text_color(crate::nsh::color(colors.text_secondary)),
            );
        }
        if let Some(error) = snapshot.error {
            root = root.child(
                ns::text(vega_i18n::t(error))
                    .text_size(ns::px(Typography::BODY))
                    .line_height(ns::px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
                    .text_color(crate::nsh::color(colors.danger)),
            );
        }
        root.into_element()
    })
    .into_element()
}
