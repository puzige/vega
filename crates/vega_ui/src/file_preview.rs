use std::sync::Arc;

use gpui_kit::{App, Context, EventEmitter, FocusHandle, Focusable, Render, Task, Window};
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::PaletteFilePreview;
use vega_theme::{Theme, ThemeColors, theme};

use crate::nsh::Surface;

pub struct RevealFileRequested {
    pub relative_path: String,
}

pub struct FilePreview {
    preview: Arc<PaletteFilePreview>,
    focus: FocusHandle,
    surface: Option<Surface<Snapshot, Command>>,
    worker: Option<Task<()>>,
}

#[derive(Clone)]
struct Snapshot {
    preview: Arc<PaletteFilePreview>,
    colors: ThemeColors,
    read_only: String,
    reveal: String,
    failed: String,
}

enum Command {
    Reveal(oneshot::Sender<()>),
    OpenLink(String, oneshot::Sender<()>),
}

impl EventEmitter<RevealFileRequested> for FilePreview {}

impl FilePreview {
    pub fn new(preview: PaletteFilePreview, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<Theme>(|_, cx| cx.notify()).detach();
        Self {
            preview: Arc::new(preview),
            focus: cx.focus_handle(),
            surface: None,
            worker: None,
        }
    }

    pub fn title(&self) -> &str {
        &self.preview.relative_path
    }

    fn snapshot(&self, cx: &App) -> Snapshot {
        Snapshot {
            preview: self.preview.clone(),
            colors: theme(cx).colors,
            read_only: vega_i18n::t("ui.read_only").into(),
            reveal: vega_i18n::t("ui.show_in_finder").into(),
            failed: vega_i18n::t("ui.command_failed").into(),
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (surface, mut requests) = Surface::new(self.snapshot(cx), render_page, window, cx);
        if self.focus.contains_focused(window, cx) {
            surface.focus(window, cx);
        }
        self.worker = Some(cx.spawn(async move |this, cx| {
            while let Some(command) = requests.recv().await {
                if this
                    .update(cx, |this, cx| match command {
                        Command::Reveal(reply) => {
                            cx.emit(RevealFileRequested {
                                relative_path: this.preview.relative_path.clone(),
                            });
                            let _ = reply.send(());
                        }
                        Command::OpenLink(target, reply) => {
                            if crate::conversation_stream::links::allowed_link(&target) {
                                crate::link_preview::open_target(&target, cx);
                                let _ = reply.send(());
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        self.surface = Some(surface);
    }
}

impl Focusable for FilePreview {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.surface
            .as_ref()
            .map(|surface| surface.entity().read(cx).focus_handle(cx))
            .unwrap_or_else(|| self.focus.clone())
    }
}

impl Render for FilePreview {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        if self.surface.is_none() {
            self.mount(window, cx);
        }
        let snapshot = self.snapshot(cx);
        self.surface.as_mut().map(|surface| {
            surface.sync_environment(cx);
            surface.publish(snapshot);
            surface.entity()
        })
    }
}

fn render_page(
    state: nsh_core::prelude::Signal<Snapshot>,
    commands: mpsc::Sender<Command>,
) -> nsh_core::prelude::AnyElement {
    use nsh_base::{TextViewLinkPressEvent, button, text_view};
    use nsh_core::prelude::*;
    use vega_theme::Typography;

    let failed = signal(false);
    let reveal_commands = commands.clone();
    let preview = state.get().preview;
    let markdown =
        preview.relative_path.ends_with(".md") || preview.relative_path.ends_with(".markdown");
    let body = if markdown {
        text_view()
            .markdown(preview.content.clone())
            .selectable(true)
            .render_image(|image: &nsh_base::text_view::Image| text(image.alt.clone()))
            .on_link_press(move |event: &TextViewLinkPressEvent| {
                let target = event.url.to_string();
                if target.starts_with('#') {
                    return;
                }
                let commands = commands.clone();
                failed.set(false);
                spawn(async move {
                    let (reply, received) = oneshot::channel();
                    if commands
                        .send(Command::OpenLink(target, reply))
                        .await
                        .is_err()
                        || received.await.is_err()
                    {
                        failed.set(true);
                    }
                });
            })
            .into_element()
    } else {
        let lines = Arc::new(
            preview
                .content
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        );
        virtual_list(lines.len(), move |at| {
            let lines = lines.clone();
            view()
                .flex_row()
                .gap_3()
                .child(
                    text(move || (at.index() + 1).to_string())
                        .w(px(40.0))
                        .text_color(move || {
                            Hsla::from(crate::nsh::color(state.get().colors.text_tertiary))
                        }),
                )
                .child(
                    text(move || lines.get(at.index()).cloned().unwrap_or_default())
                        .selectable(true),
                )
        })
        .estimated_item_size(px(Typography::CODE * Typography::BODY_LINE_HEIGHT))
        .font_family("Menlo")
        .flex_1()
        .into_element()
    };
    view()
        .flex_1()
        .bg(move || Hsla::from(crate::nsh::color(state.get().colors.bg_base)))
        .text_color(move || Hsla::from(crate::nsh::color(state.get().colors.text_primary)))
        .text_size(px(Typography::CODE))
        .line_height(px(Typography::CODE * Typography::BODY_LINE_HEIGHT))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .p_3()
                .border_b(px(1.0))
                .border_color(move || {
                    Hsla::from(crate::nsh::color(state.get().colors.border_subtle))
                })
                .text_size(px(Typography::METADATA))
                .child(text(preview.relative_path.replace(['\n', '\r'], " ")).flex_1())
                .child(text(move || state.get().read_only))
                .child(
                    button()
                        .on_press(move || {
                            let commands = reveal_commands.clone();
                            failed.set(false);
                            spawn(async move {
                                let (reply, received) = oneshot::channel();
                                if commands.send(Command::Reveal(reply)).await.is_err()
                                    || received.await.is_err()
                                {
                                    failed.set(true);
                                }
                            });
                        })
                        .child(text(move || state.get().reveal)),
                ),
        )
        .child(when(
            move || failed.get(),
            move || text(move || state.get().failed),
        ))
        .child(if markdown {
            scroll().flex_1().p_3().child(body).into_element()
        } else {
            view().flex_1().p_3().child(body).into_element()
        })
        .into_element()
}
