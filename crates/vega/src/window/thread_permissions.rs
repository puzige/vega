use super::*;
use vega_conversation::thread_permissions::ThreadPermissionService;

enum Outcome {
    Saved(Thread),
    Draft(Thread),
}

impl VegaWindow {
    pub(super) fn request_thread_permission(
        &mut self,
        stream: Entity<ConversationStream>,
        mode: PermissionMode,
        cx: &mut Context<Self>,
    ) {
        let source = stream.read(cx).thread_snapshot();
        if !self.owns_stream_request(&stream, &source.id, cx)
            || cx.global::<SettingsOpen>().0
            || self.trusted_actions.is_busy()
        {
            return;
        }
        let Some((operation, source)) =
            stream.update(cx, |stream, cx| stream.begin_thread_permission_change(cx))
        else {
            return;
        };
        let Some(path) = self.context_database(cx) else {
            stream.update(cx, |stream, cx| {
                stream.finish_thread_permission_change(
                    operation,
                    Err(ThreadPermissionError::Unavailable),
                    cx,
                )
            });
            return;
        };
        let Some(lease) =
            self.trusted_actions
                .acquire(TrustedActionKind::PermissionSelection, 0, operation)
        else {
            stream.update(cx, |stream, cx| {
                stream.finish_thread_permission_change(
                    operation,
                    Err(ThreadPermissionError::Unavailable),
                    cx,
                )
            });
            return;
        };
        let draft = self.draft_for_route(&source.id);
        let worker_source = source.clone();
        let worker = cx
            .background_executor()
            .spawn(async move { persist_permission_selection(path, worker_source, draft, mode) });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !this.trusted_actions.release(lease)
                    || !stream.read(cx).owns_thread_permission_operation(operation)
                {
                    return;
                }
                match result {
                    Ok(Outcome::Saved(saved)) => this
                        .finish_permission_projection(&stream, operation, &source, saved, true, cx),
                    Ok(Outcome::Draft(saved)) => this.finish_permission_projection(
                        &stream, operation, &source, saved, false, cx,
                    ),
                    Err(error) => {
                        stream.update(cx, |stream, cx| {
                            stream.finish_thread_permission_change(operation, Err(error), cx)
                        });
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn reload_thread_permission(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ThreadPermissionReloadRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.source.id, cx)
            || self.trusted_actions.is_busy()
        {
            stream.update(cx, |stream, cx| {
                stream.finish_thread_permission_change(
                    request.operation,
                    Err(ThreadPermissionError::Conflict),
                    cx,
                )
            });
            return;
        }
        let Some(path) = self.context_database(cx) else {
            stream.update(cx, |stream, cx| {
                stream.finish_thread_permission_change(
                    request.operation,
                    Err(ThreadPermissionError::Unavailable),
                    cx,
                )
            });
            return;
        };
        let draft = self.draft_for_route(&request.source.id);
        let request = request.clone();
        let operation = request.operation;
        let source = request.source.clone();
        let worker = cx
            .background_executor()
            .spawn(async move { reload_permission_selection(path, request.source, draft) });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Outcome::Saved(saved)) => {
                    this.finish_permission_projection(&stream, operation, &source, saved, true, cx)
                }
                Ok(Outcome::Draft(saved)) => {
                    this.finish_permission_projection(&stream, operation, &source, saved, false, cx)
                }
                Err(error) => {
                    stream.update(cx, |stream, cx| {
                        stream.finish_thread_permission_change(operation, Err(error), cx)
                    });
                }
            });
        })
        .detach();
    }

    fn finish_permission_projection(
        &mut self,
        stream: &Entity<ConversationStream>,
        operation: u64,
        source: &Thread,
        saved: Thread,
        durable: bool,
        cx: &mut Context<Self>,
    ) {
        if durable && self.draft.as_ref() == Some(source) {
            self.draft = None;
        }
        if !stream.update(cx, |stream, cx| {
            stream.finish_thread_permission_change(operation, Ok(saved.clone()), cx)
        }) {
            return;
        }
        let current = stream.read(cx).thread_snapshot();
        if !durable && self.draft.as_ref() == Some(source) {
            self.draft = Some(current.clone());
            if cx.global::<OpenedThread>().0.as_ref() == Some(source)
                && let Some(visible) = self.current_cached_stream_for_thread(&source.id, cx)
                && &visible != stream
                && visible.read(cx).thread_snapshot() == *source
                && !visible.read(cx).permission_change_pending()
                && !visible.read(cx).has_pending_model_selection()
            {
                visible.update(cx, |stream, cx| stream.apply_thread(current.clone(), cx));
                cx.set_global(OpenedThread(Some(current.clone())));
            }
        }
        if self.owns_stream_request(stream, &source.id, cx)
            && cx.global::<OpenedThread>().0.as_ref().is_some_and(|route| {
                route.project_id == source.project_id
                    && route.permission_revision == source.permission_revision
                    && route.permission_mode == source.permission_mode
                    && route.full_access_grant == source.full_access_grant
            })
        {
            cx.set_global(OpenedThread(Some(current)));
        }
        if durable {
            stream.update(cx, |stream, cx| stream.set_draft_route(false, cx));
        }
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.reload_sessions(cx));
        cx.notify();
    }
}

fn persist_permission_selection(
    path: PathBuf,
    source: Thread,
    draft: Option<Thread>,
    mode: PermissionMode,
) -> Result<Outcome, ThreadPermissionError> {
    let service = ThreadPermissionService::new(path);
    let change = service.prepare(&source, mode)?;
    if change.is_draft() && draft.as_ref() != Some(&source) {
        return Err(ThreadPermissionError::Conflict);
    }
    let saved = service.save(&change, mode == PermissionMode::FullAccess)?;
    if change.is_draft() {
        Ok(Outcome::Draft(saved))
    } else {
        Ok(Outcome::Saved(saved))
    }
}

fn reload_permission_selection(
    path: PathBuf,
    source: Thread,
    draft: Option<Thread>,
) -> Result<Outcome, ThreadPermissionError> {
    let service = ThreadPermissionService::new(path);
    match service.read(&source.id) {
        Ok(saved) => Ok(Outcome::Saved(saved)),
        Err(error) => {
            let Some(draft) = draft else {
                return Err(error);
            };
            if draft != source {
                return Err(ThreadPermissionError::Conflict);
            }
            let change = service.prepare(&draft, draft.permission_mode)?;
            if change.is_draft() {
                Ok(Outcome::Draft(draft))
            } else {
                service.read(&source.id).map(Outcome::Saved)
            }
        }
    }
}
