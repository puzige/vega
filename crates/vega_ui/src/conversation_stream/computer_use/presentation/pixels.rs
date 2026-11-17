use super::{Request, Snapshot};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};

pub(super) fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = crate::nsh::OrderedCommands::new(sender);
    let command_owner = nsh_core::element::Owner::current();
    let measured = ns::signal((0.0f32, 0.0f32));
    ns::dynamic(move || {
        let observed = state.get();
        let colors = observed.colors;
        let layout = commands.clone();
        let id = observed.id.clone();
        let mut root = ns::view()
            .w_full()
            .min_w(ns::px(0.))
            .flex_col()
            .flex_shrink_0()
            .py(ns::px(Layout::STATUS_PADDING))
            .gap(ns::px(Layout::STATUS_GAP))
            .on_layout(move |bounds: &ns::Bounds<ns::Points>| {
                let width = bounds.size.width.0;
                let height = bounds.size.height.0;
                let previous = measured.get_untracked();
                if width.is_finite()
                    && width > 0.
                    && height.is_finite()
                    && height > 0.
                    && ((previous.0 - width).abs() > 0.5 || (previous.1 - height).abs() > 0.5)
                {
                    measured.set((width, height));
                    layout.send(Request::Layout {
                        id: id.clone(),
                        width,
                        height,
                    });
                }
            });
        for text in [observed.text.clone(), observed.target.clone()]
            .into_iter()
            .flatten()
        {
            root = root.child(
                ns::text(text)
                    .w_full()
                    .min_w(ns::px(0.))
                    .text_size(ns::px(Typography::BODY))
                    .line_height(ns::px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
                    .text_color(crate::nsh::color(colors.text_secondary)),
            );
        }
        if let (Some(action), Some(label)) = (observed.source.action(), observed.label.clone()) {
            let hover = ns::signal(false);
            let down = ns::signal(false);
            let enabled = observed.enabled;
            let focused = observed.selected;
            let sender = commands.clone();
            let button = ns::view()
                .min_w(ns::px(0.))
                .px_2()
                .py_1()
                .role(nsh_core::AccessibilityRole::Button)
                .label(label.clone())
                .disabled(!enabled)
                .border(ns::px(1.))
                .border_color(crate::nsh::color(if focused {
                    colors.brand_primary
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
                .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
                .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                    hover.set(false);
                    down.set(false);
                })
                .on_press_down(move || down.set(true))
                .on_press_up(move || down.set(false))
                .on_press(move || {
                    if enabled {
                        command_owner.run(|| {
                            sender.send(Request::Activate {
                                observed: Box::new(observed.clone()),
                                action,
                            });
                        });
                    }
                })
                .child(
                    ns::text(label)
                        .min_w(ns::px(0.))
                        .text_size(ns::px(Typography::BODY))
                        .line_height(ns::px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
                        .text_color(crate::nsh::color(colors.text_secondary)),
                );
            root = root.child(
                ns::view()
                    .w_full()
                    .min_w(ns::px(0.))
                    .flex_row()
                    .child(button),
            );
        }
        root.into_element()
    })
    .into_element()
}
