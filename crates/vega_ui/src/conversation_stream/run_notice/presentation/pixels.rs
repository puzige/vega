use super::{Request, RunNoticeKind, Snapshot};
use crate::icons::{Icon, nsh_source};
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
        let retry = matches!(observed.source.notice.kind, RunNoticeKind::Retrying { .. });
        let color = crate::nsh::color(if retry {
            colors.text_secondary
        } else {
            colors.danger
        });
        let content = ns::view()
            .w_full()
            .min_w(ns::px(0.))
            .flex_row()
            .items_center()
            .gap(ns::px(Layout::CONVERSATION_INLINE_GAP));
        let content = if retry {
            let source = nsh_source(Icon::Wifi).unwrap_or_default();
            content
                .min_h(ns::px(Layout::CONVERSATION_TOOL_HEIGHT))
                .child(
                    ns::svg(nsh_core::element::ImageSrc::svg_markup(&source))
                        .w(ns::px(Layout::ICON_SIZE))
                        .h(ns::px(Layout::ICON_SIZE))
                        .flex_shrink_0()
                        .text_color(color),
                )
        } else {
            content
        };
        let text_size = if retry {
            Typography::CONVERSATION
        } else {
            Typography::BODY
        };
        let line_height = if retry {
            Typography::CONVERSATION_LINE_HEIGHT
        } else {
            Typography::BODY_LINE_HEIGHT
        };
        let content = content.child(
            ns::text(observed.text.clone())
                .min_w(ns::px(0.))
                .flex_1()
                .text_size(ns::px(text_size))
                .line_height(ns::px(text_size * line_height))
                .text_color(color),
        );
        let buttons = observed
            .actions()
            .into_iter()
            .map(|action| {
                let displayed = observed.clone();
                let sender = commands.clone();
                let selected = observed.selected == Some(action);
                let hover = ns::signal(false);
                let down = ns::signal(false);
                ns::view()
                    .px_2()
                    .py_1()
                    .rounded(ns::px(6.))
                    .role(nsh_core::AccessibilityRole::Button)
                    .label(action.label())
                    .border(ns::px(1.))
                    .border_color(crate::nsh::color(if selected {
                        colors.accent
                    } else {
                        colors.bg_base.opacity(0.)
                    }))
                    .bg(move || {
                        crate::nsh::color(if down.get() {
                            colors.bg_active
                        } else if hover.get() {
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
                        command_owner.run(|| {
                            sender.send(Request::Activate {
                                observed: Box::new(displayed.clone()),
                                action,
                            })
                        })
                    })
                    .child(
                        ns::text(action.label())
                            .text_size(ns::px(Typography::BODY))
                            .text_color(crate::nsh::color(colors.text_secondary)),
                    )
            })
            .collect::<Vec<_>>();
        let layout = commands.clone();
        let id = observed.id.clone();
        let root = ns::view()
            .w_full()
            .min_w(ns::px(0.))
            .flex_col()
            .flex_shrink_0()
            .py_1()
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
            })
            .child(content);
        if buttons.is_empty() {
            root.into_element()
        } else {
            root.child(
                ns::view()
                    .w_full()
                    .flex_row()
                    .flex_wrap()
                    .gap(ns::px(8.))
                    .children(buttons),
            )
            .into_element()
        }
    })
    .into_element()
}
