use super::{Bridge, Command, MAX_TEXT, Snapshot};
use nsh_base::{button, input};
use nsh_core::prelude::*;
use tokio::sync::oneshot;
use vega_theme::{Layout, Typography};

#[component]
pub fn PreviewPage() -> impl IntoElement {
    let Some(bridge) = engine_context::<Bridge>() else {
        return view().into_element();
    };
    let mut receiver = bridge.state.clone();
    let state = signal(receiver.borrow_and_update().clone());
    spawn(async move {
        while receiver.changed().await.is_ok() {
            state.set(receiver.borrow_and_update().clone());
        }
    });
    let draft = signal(String::new());
    let reply = signal(String::new());
    let pending = signal(false);
    let failed = signal(false);
    let label = move |get: fn(&Snapshot) -> &String| move || state.with(|state| get(state).clone());
    view()
        .flex_1()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .gap(px(Layout::SETTINGS_CONTENT_GAP))
        .bg(move || Hsla::from(state.get().background))
        .text_color(move || Hsla::from(state.get().foreground))
        .text_size(px(Typography::BODY))
        .line_height(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
        .child(text(label(|s| &s.title)).text_size(px(Typography::SETTINGS_TITLE)))
        .child(text(label(|s| &s.session_only)).text_color(move || Hsla::from(state.get().muted)))
        .child(
            view()
                .gap(px(Layout::STATUS_PADDING))
                .child(text(label(|s| &s.input)))
                .child(
                    input()
                        .label(label(|s| &s.input))
                        .value(draft)
                        .max_length(MAX_TEXT as u32)
                        .disabled(pending)
                        .on_change(move |text: &str| {
                            draft.set(text.chars().take(MAX_TEXT).collect());
                        })
                        .p(px(Layout::SETTINGS_CONTROL_PADDING_X))
                        .border(px(Layout::ARTIFACT_BORDER))
                        .border_color(move || Hsla::from(state.get().border))
                        .bg(move || Hsla::from(state.get().surface))
                        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS)),
                ),
        )
        .child(
            button()
                .disabled(move || pending.get() || draft.with(String::is_empty))
                .label(label(|s| &s.send))
                .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
                .py(px(Layout::STATUS_PADDING))
                .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                .bg(move || Hsla::from(state.get().accent))
                .text_color(move || Hsla::from(state.get().on_accent))
                .on_press(move || {
                    let (sender, receiver) = oneshot::channel();
                    let command = Command::Echo(draft.get(), sender);
                    if bridge.commands.try_send(command).is_err() {
                        failed.set(true);
                        return;
                    }
                    pending.set(true);
                    failed.set(false);
                    reply.set(String::new());
                    spawn(async move {
                        match receiver.await {
                            Ok(text) => reply.set(text),
                            Err(_) => failed.set(true),
                        }
                        pending.set(false);
                    });
                })
                .child(text(label(|s| &s.send))),
        )
        .child(when(
            move || failed.get(),
            move || text(label(|s| &s.failed)),
        ))
        .child(when(
            move || !reply.with(String::is_empty),
            move || {
                view()
                    .gap(px(Layout::STATUS_PADDING))
                    .child(text(label(|s| &s.result)))
                    .child(text(reply).selectable(true))
            },
        ))
        .into_element()
}
