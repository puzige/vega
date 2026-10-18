use super::*;
use tokio_util::sync::CancellationToken;

pub(crate) struct ComposerPredictionOwner {
    stream: gpui_kit::WeakEntity<ConversationStream>,
    fence: ComposerPredictionFence,
    cancel: CancellationToken,
    request: Option<ComposerPredictionRequest>,
}

impl Drop for ComposerPredictionOwner {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl VegaWindow {
    pub(crate) fn composer_prediction_route_matches(
        &self,
        owner: &ComposerPredictionOwner,
        cx: &App,
    ) -> bool {
        owner.stream.upgrade().is_some_and(|stream| {
            self.owns_stream_request(&stream, &owner.fence.thread_id, cx)
                && cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|thread| {
                        thread.model == owner.fence.model
                            && thread.mode == owner.fence.mode
                            && thread.backend == owner.fence.backend
                    })
        })
    }

    pub(crate) fn cancel_composer_prediction_for_thread(
        &mut self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) {
        if self
            .composer_prediction
            .as_ref()
            .is_some_and(|owner| owner.fence.thread_id == thread_id)
        {
            self.cancel_composer_prediction(cx);
        }
    }

    pub(crate) fn cancel_composer_prediction(&mut self, cx: &mut Context<Self>) {
        if let Some(owner) = self.composer_prediction.take()
            && let Some(stream) = owner.stream.upgrade()
        {
            stream.update(cx, ConversationStream::invalidate_composer_prediction);
        }
    }

    pub(crate) fn prepare_composer_prediction(
        &mut self,
        stream: &Entity<ConversationStream>,
        request: ComposerPredictionRequest,
        completed_message_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = cx.global::<OpenedThread>().0.clone() else {
            return;
        };
        if !self.window_alive.load(Ordering::SeqCst)
            || cx.global::<SettingsOpen>().0
            || !self.owns_stream_request(stream, &thread.id, cx)
            || self.model_catalog_loading
            || self.reasoning_save_pending.is_some()
            || self.trusted_actions.is_busy()
            || self.agent_controller.active.contains_key(&thread.id)
            || request.model() != thread.model
        {
            return;
        }
        self.cancel_composer_prediction(cx);
        let Some((fence, cancel)) = stream.update(cx, |stream, cx| {
            stream.prepare_composer_prediction(completed_message_id, cx)
        }) else {
            return;
        };
        self.composer_prediction = Some(ComposerPredictionOwner {
            stream: stream.downgrade(),
            fence,
            cancel,
            request: Some(request),
        });
    }

    pub(crate) fn start_pending_composer_prediction(
        &mut self,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.composer_prediction.as_ref() else {
            return;
        };
        if owner.fence.thread_id != thread_id || owner.stream.upgrade().as_ref() != Some(stream) {
            return;
        }
        if owner.request.is_none() {
            return;
        }
        let fence = owner.fence.clone();
        if !self.window_alive.load(Ordering::SeqCst)
            || owner.cancel.is_cancelled()
            || cx.global::<SettingsOpen>().0
            || !self.owns_stream_request(stream, thread_id, cx)
            || self.model_catalog_loading
            || self.reasoning_save_pending.is_some()
            || self.trusted_actions.is_busy()
            || self.agent_controller.active.contains_key(thread_id)
            || self
                .agent_controller
                .queue_operations
                .contains_key(thread_id)
            || self
                .agent_controller
                .queue_dispatches
                .contains_key(thread_id)
            || !stream.update(cx, |stream, cx| {
                stream.ready_composer_prediction(&fence, cx)
            })
        {
            self.cancel_composer_prediction(cx);
            return;
        }
        let database = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .ok()
            .and_then(|store| store.database_path())
            .map(std::path::Path::to_path_buf);
        let Some(database) = database else {
            self.cancel_composer_prediction(cx);
            return;
        };
        let Some(owner) = self.composer_prediction.as_mut() else {
            return;
        };
        let Some(request) = owner.request.take() else {
            return;
        };
        let cancel = owner.cancel.clone();
        let pricing = self.pricing_controller.catalog_for_run(&fence.model);
        let worker_fence = fence.clone();
        let worker = cx.background_executor().spawn(async move {
            request.run(
                database,
                worker_fence.thread_id,
                worker_fence.completed_message_id,
                cancel,
                pricing,
            )
        });
        let stream = stream.downgrade();
        cx.spawn(async move |this, cx| {
            let suggestion = worker.await;
            let _ = this.update(cx, |this, cx| {
                let Some(owner) = this.composer_prediction.as_ref() else {
                    return;
                };
                if owner.fence != fence {
                    return;
                }
                if owner.cancel.is_cancelled() {
                    this.cancel_composer_prediction(cx);
                    return;
                }
                let Some(stream) = stream.upgrade() else {
                    this.cancel_composer_prediction(cx);
                    return;
                };
                if cx.global::<SettingsOpen>().0
                    || !this.owns_stream_request(&stream, &fence.thread_id, cx)
                {
                    this.cancel_composer_prediction(cx);
                    return;
                }
                if let Some(suggestion) = suggestion {
                    stream.update(cx, |stream, cx| {
                        stream.apply_composer_prediction(&fence, suggestion, cx);
                    });
                } else {
                    this.cancel_composer_prediction(cx);
                }
            });
        })
        .detach();
    }
}
