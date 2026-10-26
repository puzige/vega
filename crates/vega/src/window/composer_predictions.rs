use super::*;
use tokio_util::sync::CancellationToken;

pub(crate) struct ComposerPredictionOwner {
    stream: Entity<ConversationStream>,
    fence: ComposerPredictionFence,
    cancel: CancellationToken,
    request: Option<ComposerPredictionRequest>,
    _subscription: Subscription,
}

impl Drop for ComposerPredictionOwner {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl VegaWindow {
    pub(crate) fn composer_prediction_stream(
        &self,
        thread_id: &str,
    ) -> Option<Entity<ConversationStream>> {
        self.composer_predictions
            .get(thread_id)
            .filter(|owner| !owner.cancel.is_cancelled())
            .map(|owner| owner.stream.clone())
    }

    pub(crate) fn reconcile_composer_prediction_route(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = cx.global::<OpenedThread>().0.as_ref() else {
            return;
        };
        let thread_id = thread.id.clone();
        if self
            .composer_predictions
            .get(&thread_id)
            .is_some_and(|owner| {
                owner.fence.model != thread.model
                    || owner.fence.mode != thread.mode
                    || owner.fence.backend != thread.backend
            })
        {
            self.cancel_composer_prediction_for_thread(&thread_id, cx);
        }
    }

    pub(crate) fn cancel_composer_prediction_for_thread(
        &mut self,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(owner) = self.composer_predictions.remove(thread_id) {
            owner
                .stream
                .update(cx, ConversationStream::invalidate_composer_prediction);
        }
    }

    pub(crate) fn cancel_composer_prediction(&mut self, cx: &mut Context<Self>) {
        for (_, owner) in self.composer_predictions.drain() {
            owner
                .stream
                .update(cx, ConversationStream::invalidate_composer_prediction);
        }
    }

    pub(crate) fn prepare_composer_prediction(
        &mut self,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        request: ComposerPredictionRequest,
        completed_message_id: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.window_alive.load(Ordering::SeqCst)
            || cx.global::<SettingsOpen>().0
            || self.model_catalog_loading
            || self.reasoning_save_pending.is_some()
            || self.trusted_actions.is_busy()
            || self.agent_controller.active.contains_key(thread_id)
            || request.model() != stream.read(cx).displayed_model()
        {
            return;
        }
        self.cancel_composer_prediction_for_thread(thread_id, cx);
        let Some((fence, cancel)) = stream.update(cx, |stream, cx| {
            stream.prepare_composer_prediction(completed_message_id, cx)
        }) else {
            return;
        };
        if fence.thread_id != thread_id {
            stream.update(cx, ConversationStream::invalidate_composer_prediction);
            return;
        }
        let owner_thread = thread_id.to_string();
        let subscription = cx.observe(stream, move |this, stream, cx| {
            if this
                .composer_predictions
                .get(&owner_thread)
                .is_some_and(|owner| {
                    owner.stream == stream
                        && !stream
                            .read(cx)
                            .composer_prediction_matches(&owner.fence, cx)
                })
            {
                this.cancel_composer_prediction_for_thread(&owner_thread, cx);
            }
        });
        self.composer_predictions.insert(
            thread_id.to_string(),
            ComposerPredictionOwner {
                stream: stream.clone(),
                fence,
                cancel,
                request: Some(request),
                _subscription: subscription,
            },
        );
    }

    pub(crate) fn start_pending_composer_prediction(
        &mut self,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.composer_predictions.get(thread_id) else {
            return;
        };
        if owner.stream != *stream || owner.request.is_none() {
            return;
        }
        let fence = owner.fence.clone();
        if !self.window_alive.load(Ordering::SeqCst)
            || owner.cancel.is_cancelled()
            || cx.global::<SettingsOpen>().0
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
            self.cancel_composer_prediction_for_thread(thread_id, cx);
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
            self.cancel_composer_prediction_for_thread(thread_id, cx);
            return;
        };
        let Some(owner) = self.composer_predictions.get_mut(thread_id) else {
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
        cx.spawn(async move |this, cx| {
            let suggestion = worker.await;
            let _ = this.update(cx, |this, cx| {
                let Some(owner) = this.composer_predictions.get(&fence.thread_id) else {
                    return;
                };
                if owner.fence != fence {
                    return;
                }
                let stream = owner.stream.clone();
                if owner.cancel.is_cancelled()
                    || !this.window_alive.load(Ordering::SeqCst)
                    || cx.global::<SettingsOpen>().0
                {
                    this.cancel_composer_prediction_for_thread(&fence.thread_id, cx);
                    return;
                }
                let applied = suggestion.is_some_and(|suggestion| {
                    stream.update(cx, |stream, cx| {
                        stream.apply_composer_prediction(&fence, suggestion, cx)
                    })
                });
                if !applied {
                    this.cancel_composer_prediction_for_thread(&fence.thread_id, cx);
                }
            });
        })
        .detach();
    }
}
