use super::*;

pub(in crate::sidebar) fn delete_confirmation(
    key: impl Into<gpui_kit::SharedString>,
    message: String,
    guard: impl Fn(&App) -> bool + 'static,
    handler: impl Fn(usize, Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> AnyElement {
    Host {
        key: key.into(),
        text: Some(message),
        items: vec![
            Item::action(vega_i18n::t("ui.cancel"), None),
            Item::action(vega_i18n::t("ui.delete"), None),
        ],
        presentation: Presentation::DeleteConfirmation,
        height: 100.,
        guard: Rc::new(guard),
        handler: Rc::new(handler),
        menu_scroll: None,
        focus: None,
    }
    .into_any_element()
}

pub(super) fn render(
    content: nsh_core::element::View,
    snapshot: &Snapshot,
    commands: OrderedCommands<Request>,
) -> ns::AnyElement {
    let colors = snapshot.colors;
    let line_height = ns::px(Typography::SIDEBAR * Typography::BODY_LINE_HEIGHT);
    let mut actions = ns::view().w_full().flex_row().justify_end().gap(ns::px(8.));
    for (index, item) in snapshot.items.iter().enumerate() {
        let delete = index == 1;
        let selected = snapshot.selected == Some(index);
        let hover = ns::signal(false);
        let id = snapshot.id.clone();
        let sender = commands.clone();
        let button = ns::view()
            .role(nsh_core::AccessibilityRole::Button)
            .label(item.label.clone())
            .px(ns::px(12.))
            .py(ns::px(4.))
            .rounded(ns::px(6.))
            .border(ns::px(1.))
            .border_color(crate::nsh::color(if selected {
                colors.accent
            } else if delete {
                colors.danger
            } else {
                colors.border_subtle
            }))
            .bg(move || {
                crate::nsh::color(match (delete, hover.get()) {
                    (true, true) => colors.danger.opacity(0.85),
                    (true, false) => colors.danger,
                    (false, true) => colors.bg_hover,
                    (false, false) => colors.bg_elevated,
                })
            })
            .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| hover.set(false))
            .on_press(move |_: &nsh_core::PressEvent| {
                sender.send(Request::Activate {
                    id: id.clone(),
                    index,
                    double: false,
                });
            })
            .child(
                ns::text(item.label.clone())
                    .text_size(ns::px(Typography::SIDEBAR))
                    .line_height(line_height)
                    .text_color(crate::nsh::color(if delete {
                        colors.bg_base
                    } else {
                        colors.text_secondary
                    })),
            );
        actions = actions.child(button);
    }
    let failed = commands.failed;
    content
        .gap(ns::px(12.))
        .p(ns::px(16.))
        .rounded(ns::px(8.))
        .border(ns::px(1.))
        .border_color(crate::nsh::color(colors.border_subtle))
        .bg(crate::nsh::color(colors.bg_elevated))
        .child(
            ns::text(vega_i18n::t("ui.delete_conversation"))
                .w_full()
                .text_size(ns::px(Typography::HEADING_CARD))
                .font_weight(ns::FontWeight(Typography::HEADING_CARD_WEIGHT.0 as u16))
                .line_height(line_height)
                .text_color(crate::nsh::color(colors.text_primary)),
        )
        .child(
            // Keep both decisions visible even when the task name fills several screens.
            ns::scroll().w_full().max_h(ns::px(300.)).min_h_0().child(
                ns::text(snapshot.text.clone().unwrap_or_default())
                    .w_full()
                    .text_size(ns::px(Typography::SIDEBAR))
                    .line_height(line_height)
                    .text_color(crate::nsh::color(colors.text_secondary)),
            ),
        )
        .child(actions)
        .child(ns::when(
            move || failed.get(),
            move || {
                ns::text(vega_i18n::t("ui.reload_view"))
                    .text_color(crate::nsh::color(colors.danger))
            },
        ))
        .into_element()
}
