use super::*;
use vega_conversation::thread_permissions::ThreadPermissionService;

enum Outcome {
    Confirmation(ThreadPermissionChange),
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
        let Some((operation, source)) = stream.update(cx, |stream, cx| {
            stream.begin_thread_permission_change(mode, cx)
        }) else {
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
        let worker = cx.background_executor().spawn(async move {
            let service = ThreadPermissionService::new(path);
            let change = service.prepare(&worker_source, mode)?;
            if mode == PermissionMode::FullAccess {
                Ok(Outcome::Confirmation(change))
            } else if change.is_draft() {
                if draft.as_ref() != Some(&worker_source) {
                    return Err(ThreadPermissionError::Conflict);
                }
                Ok(Outcome::Draft(Thread {
                    permission_mode: mode,
                    full_access_grant: None,
                    default_permission: None,
                    ..worker_source
                }))
            } else {
                service.save(&change, false).map(Outcome::Saved)
            }
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !this.trusted_actions.release(lease)
                    || !stream.read(cx).owns_thread_permission_operation(operation)
                {
                    return;
                }
                match result {
                    Ok(Outcome::Confirmation(change)) => {
                        if this.owns_stream_request(&stream, &source.id, cx)
                            && !cx.global::<SettingsOpen>().0
                        {
                            stream.update(cx, |stream, cx| {
                                stream.show_thread_permission_confirmation(operation, change, cx)
                            });
                        } else {
                            stream.update(cx, |stream, cx| {
                                stream.cancel_thread_permission_change(cx)
                            });
                        }
                    }
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

    pub(super) fn confirm_thread_permission(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ThreadPermissionConfirmed,
        cx: &mut Context<Self>,
    ) {
        let source = request.change.source().clone();
        if !self.owns_stream_request(&stream, &source.id, cx)
            || cx.global::<SettingsOpen>().0
            || self.trusted_actions.is_busy()
            || (request.change.is_draft()
                && self.draft_for_route(&source.id).as_ref() != Some(&source))
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
        let Some(lease) = self.trusted_actions.acquire(
            TrustedActionKind::PermissionSelection,
            0,
            request.operation,
        ) else {
            return;
        };
        if !stream.update(cx, |stream, cx| {
            stream.begin_thread_permission_save(request, cx)
        }) {
            self.trusted_actions.release(lease);
            return;
        }
        let request = request.clone();
        let operation = request.operation;
        let worker = cx
            .background_executor()
            .spawn(async move { ThreadPermissionService::new(path).save(&request.change, true) });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !this.trusted_actions.release(lease) {
                    return;
                }
                match result {
                    Ok(saved) => this
                        .finish_permission_projection(&stream, operation, &source, saved, true, cx),
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
        let worker = cx.background_executor().spawn(async move {
            let service = ThreadPermissionService::new(path);
            if let Some(draft) = draft {
                service.prepare(&draft, draft.permission_mode)?;
                Ok(draft)
            } else {
                service.read(&request.source.id)
            }
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(saved) => {
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
        if self.owns_stream_request(stream, &source.id, cx)
            && cx.global::<OpenedThread>().0.as_ref().is_some_and(|route| {
                route.project_id == source.project_id
                    && route.permission_revision == source.permission_revision
                    && route.permission_mode == source.permission_mode
                    && route.full_access_grant == source.full_access_grant
            })
        {
            let current = stream.read(cx).thread_snapshot();
            if !durable && self.draft.as_ref() == Some(source) {
                self.draft = Some(current.clone());
            }
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
