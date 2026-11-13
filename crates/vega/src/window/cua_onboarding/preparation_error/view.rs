use nsh_core::prelude::*;
use tokio::sync::mpsc;
use vega_theme::{Layout, Typography};
use vega_ui::nsh::{OrderedCommands, color};

use super::{Request, Snapshot};

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
    let content = signal_with(incoming.get_untracked(), SignalEquals::Never);
    effect(move || {
        let next = incoming.get();
        if !content.get_untracked().same_frame(&next) {
            content.set(next);
        }
    });
    view()
        .w_full()
        .child(dynamic(move || {
            let snapshot = content.get();
            let colors = snapshot.colors;
            let root = signal(None::<[f32; 4]>);
            let row = signal(None::<[f32; 4]>);
            let first = signal(None::<[f32; 4]>);
            let second = signal(None::<[f32; 4]>);
            let acknowledgements = commands.clone();
            let observed = snapshot.clone();
            effect(move || {
                if let (Some(root), Some(row), Some(first), Some(second)) =
                    (root.get(), row.get(), first.get(), second.get())
                {
                    let controls = [first, second].map(|control| {
                        [
                            root[0] + row[0] + control[0],
                            root[1] + row[1] + control[1],
                            control[2],
                            control[3],
                        ]
                    });
                    acknowledgements.send(Request::Layout(observed.clone(), root, controls));
                }
            });
            let mut errors = scroll()
                .w_full()
                .max_h(px(snapshot.max_error_height))
                .text_color(color(colors.danger))
                .child(text(snapshot.error.clone()));
            if let Some(fault) = &snapshot.fault {
                errors = errors.child(text(fault.clone()));
            }
            let mut controls = view()
                .w_full()
                .flex_row()
                .justify_end()
                .on_layout(move |bounds: &Bounds<Points>| row.set(Some(rect(bounds))))
                .gap(px(Layout::IMAGE_PREVIEW_GAP));
            for (index, action) in snapshot.actions().into_iter().enumerate() {
                let measured = if index == 0 { first } else { second };
                let observed = snapshot.clone();
                let requests = commands.clone();
                controls = controls.child(
                    view()
                        .role(AccessibilityRole::Button)
                        .label(action.label())
                        .on_layout(move |bounds: &Bounds<Points>| measured.set(Some(rect(bounds))))
                        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
                        .py(px(Layout::TITLEBAR_CONTROL_GAP))
                        .rounded(px(Layout::PANEL_RADIUS))
                        .border(px(1.))
                        .border_color(color(if snapshot.focused && snapshot.selected == action {
                            colors.brand_primary
                        } else {
                            colors.border_subtle
                        }))
                        .bg(color(colors.bg_elevated))
                        .hover(move |style| style.bg(color(colors.bg_hover)))
                        .active(move |style| style.bg(color(colors.bg_active)))
                        .on_press(move |_: &PressEvent| {
                            requests.send(Request::Activate(observed.clone(), action))
                        })
                        .child(text(action.text())),
                );
            }
            let failed = commands.failed;
            view()
                .w_full()
                .flex_col()
                .p(px(Layout::IMAGE_PREVIEW_GAP))
                .gap(px(Layout::IMAGE_PREVIEW_GAP))
                .on_layout(move |bounds: &Bounds<Points>| root.set(Some(rect(bounds))))
                .bg(color(colors.bg_base))
                .text_color(color(colors.text_primary))
                .text_size(px(Typography::METADATA))
                .child(errors)
                .child(controls)
                .child(when(
                    move || failed.get(),
                    move || {
                        text(vega_i18n::t("ui.command_failed")).text_color(color(colors.danger))
                    },
                ))
                .into_element()
        }))
        .into_element()
}
