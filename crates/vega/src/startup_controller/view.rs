use nsh_core::prelude::*;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};
use vega_ui::nsh::{OrderedCommands, color};

use super::{
    error_key, logo,
    model::{Action, Request, Snapshot, localized},
};

fn control(
    snapshot: &Snapshot,
    commands: OrderedCommands<Request>,
    action: Action,
) -> impl IntoElement {
    let observed = snapshot.rendered_frame();
    let colors = snapshot.colors;
    view()
        .role(AccessibilityRole::Button)
        .label(action.label())
        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
        .py(px(Layout::TITLEBAR_CONTROL_GAP))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border(px(1.))
        .border_color(color(if snapshot.selected == Some(action) {
            colors.accent
        } else {
            colors.border_subtle
        }))
        .bg(color(colors.bg_elevated))
        .hover(move |style| style.bg(color(colors.bg_hover)))
        .active(move |style| style.bg(color(colors.bg_active)))
        .flex_row()
        .items_center()
        .justify_center()
        .on_press(move |_: &PressEvent| {
            commands.send(Request::Activate {
                observed: observed.clone(),
                action,
            });
        })
        .child(text(action.label()))
}

pub(super) fn render(incoming: Signal<Snapshot>, sender: mpsc::Sender<Request>) -> AnyElement {
    let commands = OrderedCommands::new(sender);
    let content = signal_with(incoming.get_untracked(), SignalEquals::Never);
    effect(move || {
        let next = incoming.get();
        let previous = content.get_untracked();
        if !previous.frame.matches(&next.frame) {
            content.set(next);
        }
    });
    view()
        .size_full()
        .overflow_hidden()
        .child(dynamic(move || {
            let snapshot = content.get();
            let colors = snapshot.colors;
            let observed = snapshot.rendered_frame();
            let acknowledgements = commands.clone();
            let mut body = view()
                .on_layout(move |bounds: &Bounds<Points>| {
                    if bounds.size.width.0 > 0. && bounds.size.height.0 > 0. {
                        acknowledgements.send(Request::Rendered(observed.clone()));
                    }
                })
                .size_full()
                .bg(color(colors.bg_base))
                .text_color(color(colors.text_primary))
                .text_size(px(Typography::BODY))
                .line_height(nsh_core::LineHeight::Multiple(Typography::BODY_LINE_HEIGHT))
                .p(px(Layout::CONTENT_PADDING))
                .items_center()
                .justify_center();
            if snapshot.authority.error.is_none() && !snapshot.authority.recovery {
                return body
                    .label(vega_i18n::t("app.startup.loading"))
                    .child(logo::logo(move || {
                        let snapshot = incoming.get();
                        (snapshot.colors, snapshot.phase)
                    }))
                    .into_element();
            }
            let mut column = view()
                .w_full()
                .max_w(px(Layout::SETTINGS_CONTENT_MAX_WIDTH))
                .gap(px(Layout::SETTINGS_CONTENT_GAP));
            if let Some(error) = snapshot.authority.error {
                column = column
                    .child(
                        text(vega_i18n::t("app.startup.failed"))
                            .text_size(px(Typography::SETTINGS_TITLE))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(text(vega_i18n::t(error_key(error))).text_color(color(colors.danger)));
            }
            if snapshot.authority.recovery {
                column = column.child(
                    text(localized(
                        "The startup view could not be displayed. Reload the view to try again.",
                        "无法显示启动界面，请重新加载界面。",
                    ))
                    .text_color(color(colors.danger)),
                );
            }
            let mut actions = view()
                .flex_row()
                .flex_wrap()
                .gap(px(Layout::IMAGE_PREVIEW_GAP));
            for action in snapshot.authority.controls() {
                actions = actions.child(control(&snapshot, commands.clone(), action));
            }
            let failed = commands.failed;
            column = column.child(actions).child(when(
                move || failed.get(),
                move || text(vega_i18n::t("ui.command_failed")).text_color(color(colors.danger)),
            ));
            body = body.child(
                scroll()
                    .w_full()
                    .max_w(px(Layout::SETTINGS_CONTENT_MAX_WIDTH))
                    .max_h(relative(1.))
                    .child(column),
            );
            body.into_element()
        }))
        .into_element()
}
