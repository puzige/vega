use super::*;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct ConversationLoading {
    generation: u64,
    thread_id: Option<String>,
    started: Option<Instant>,
    result: Option<Result<ConversationLoad, ()>>,
}

impl ConversationLoading {
    pub(super) fn clear(&mut self) {
        if self.thread_id.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
        self.started = None;
        self.result = None;
    }

    pub(super) fn invalidate_route(&mut self, thread_id: Option<&str>) {
        if self.thread_id.as_deref() != thread_id {
            self.clear();
        }
    }

    pub(super) fn take_ready(&mut self) -> Option<ConversationLoad> {
        self.result.take().and_then(Result::ok)
    }
}

impl VegaWindow {
    pub(super) fn conversation_ready(&mut self, thread_id: &str, cx: &mut Context<Self>) -> bool {
        if self.conversation_loading.thread_id.as_deref() != Some(thread_id) {
            self.start_conversation_load(thread_id.to_owned(), cx);
        }
        matches!(self.conversation_loading.result, Some(Ok(_)))
    }

    fn start_conversation_load(&mut self, thread_id: String, cx: &mut Context<Self>) {
        self.conversation_loading.clear();
        self.conversation_loading.thread_id = Some(thread_id.clone());
        self.conversation_loading.started = Some(Instant::now());
        let generation = self.conversation_loading.generation;
        let path = self.file_backed_store_path(cx);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_thread = thread_id.clone();
        let spawned = std::thread::Builder::new()
            .name("vega-conversation-load".into())
            .spawn(move || {
                let result = path.ok_or(()).and_then(|path| {
                    vega_conversation::threads::load_conversation(&path, &worker_thread)
                        .map_err(|_| ())
                });
                let _ = sender.send(result);
            });
        if spawned.is_err() {
            self.conversation_loading.result = Some(Err(()));
            cx.notify();
            return;
        }
        cx.spawn(async move |owner, cx| {
            loop {
                let result = match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Disconnected) => Some(Err(())),
                    Err(mpsc::TryRecvError::Empty) => None,
                };
                let finished = result.is_some();
                let current = owner.update(cx, |this, cx| {
                    let current = !cx.global::<SettingsOpen>().0
                        && cx
                            .global::<OpenedThread>()
                            .0
                            .as_ref()
                            .is_some_and(|thread| thread.id == thread_id)
                        && this.conversation_loading.thread_id.as_ref() == Some(&thread_id)
                        && this.conversation_loading.generation == generation;
                    if !current {
                        return false;
                    }
                    if let Some(result) = result {
                        this.conversation_loading.result = Some(result);
                    }
                    cx.notify();
                    true
                });
                if finished || !matches!(current, Ok(true)) {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(25))
                    .await;
            }
        })
        .detach();
    }

    pub(super) fn render_conversation_loading(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut content = div()
            .id("conversation-loading")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .text_size(px(Typography::BODY))
            .text_color(colors.text_secondary)
            .aria_label(vega_i18n::t("ui.loading"));
        if matches!(self.conversation_loading.result, Some(Err(()))) {
            content = content
                .child(vega_i18n::t("app.conversation.load_failed"))
                .child(
                    gpui_kit::component::button::Button::new("retry-conversation")
                        .label(vega_i18n::t("ui.retry"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(thread_id) = this.conversation_loading.thread_id.clone() {
                                this.start_conversation_load(thread_id, cx);
                            }
                        })),
                );
        } else if let Some(epoch) = self.conversation_loading.started
            && epoch.elapsed() >= Duration::from_millis(Motion::CONVERSATION_LOADING_DELAY_MS)
        {
            content = content.child(crate::loading_logo::loading_logo(epoch, colors));
        }
        content.into_any_element()
    }
}
