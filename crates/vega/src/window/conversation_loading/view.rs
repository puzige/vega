use nsh_core::prelude::*;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};
use vega_ui::nsh::{OrderedCommands, color};

use super::{
    logo,
    model::{Request, Snapshot},
};

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
        if !content.get_untracked().same_frame(&next) {
            content.set(next);
        }
    });
    view()
        .size_full()
        .overflow_hidden()
        .child(dynamic(move || {
            let snapshot = content.get();
            let colors = snapshot.colors;
            let root = signal(None::<[f32; 4]>);
            let control = signal(None::<[f32; 4]>);
            let acknowledgements = commands.clone();
            let observed = snapshot.clone();
            effect(move || {
                if let (Some(root), Some(control)) = (root.get(), control.get()) {
                    acknowledgements.send(Request::LaidOut(observed.clone(), root, control));
                }
            });
            let mut body = view()
                .size_full()
                .overflow_hidden()
                .on_layout(move |bounds: &Bounds<Points>| root.set(Some(rect(bounds))))
                .bg(color(colors.bg_base))
                .text_size(px(Typography::BODY))
                .text_color(color(colors.text_secondary))
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(Layout::SETTINGS_CONTENT_GAP))
                .label(vega_i18n::t("ui.loading"));
            if snapshot.observed.failed() {
                let retry = commands.clone();
                let observed = snapshot.clone();
                body = body
                    .child(text(vega_i18n::t("app.conversation.load_failed")))
                    .child(
                        view()
                            .role(AccessibilityRole::Button)
                            .label(vega_i18n::t("ui.retry"))
                            .on_layout(move |bounds: &Bounds<Points>| {
                                control.set(Some(rect(bounds)))
                            })
                            .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                            .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
                            .py(px(Layout::TITLEBAR_CONTROL_GAP))
                            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                            .border(px(1.))
                            .border_color(color(if snapshot.focused {
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
                                command_owner.run(|| retry.send(Request::Retry(observed.clone())))
                            })
                            .child(text(vega_i18n::t("ui.retry"))),
                    );
            } else if snapshot.show_logo {
                body = body.child(logo::logo(move || {
                    let snapshot = incoming.get();
                    (snapshot.colors, snapshot.phase)
                }));
            }
            let failed = commands.failed;
            body.child(when(
                move || failed.get(),
                move || text(vega_i18n::t("ui.command_failed")).text_color(color(colors.danger)),
            ))
            .into_element()
        }))
        .into_element()
}
