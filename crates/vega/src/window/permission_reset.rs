use super::*;
use crate::automation_controller::AutomationGlobal;
use vega_conversation::permission_reset::{PermissionResetSession, prepare_reset};
use vega_conversation::types::{
    PermissionResetAction as Action, PermissionResetError as Error,
    PermissionResetProjection as Projection,
};

mod projection;
#[cfg(test)]
mod tests;

type Session = Arc<Mutex<Option<PermissionResetSession>>>;

#[derive(Default)]
pub(super) struct ResetController {
    operation: u64,
    pub(super) projection: Projection,
    intent: Option<PermissionResetIntent>,
    paths: Option<(PathBuf, PathBuf)>,
    session: Option<Session>,
    lease: Option<TrustedActionToken>,
    scheduler_ready: Option<bool>,
}

impl VegaWindow {
    pub(super) fn permission_reset_requested(
        &mut self,
        view: Entity<SettingsView>,
        action: Action,
        cx: &mut Context<Self>,
    ) {
        if self.settings_view.as_ref() != Some(&view) || !cx.global::<SettingsOpen>().0 {
            return;
        }
        match action {
            Action::Prepare
                if matches!(
                    self.permission_reset.projection,
                    Projection::Idle | Projection::Completed
                ) =>
            {
                self.prepare_permission_reset(cx)
            }
            Action::Confirm if self.permission_reset.projection == Projection::Confirming => {
                self.confirm_permission_reset(cx)
            }
            Action::Retry
                if matches!(self.permission_reset.projection, Projection::Blocked { .. }) =>
            {
                if self.permission_reset.session.is_some() {
                    self.run_permission_reset(cx);
                } else {
                    self.prepare_permission_reset(cx);
                }
            }
            Action::Cancel => self.cancel_permission_reset(cx),
            Action::Inspect => self.inspect_permission_reset(cx),
            _ => {}
        }
    }

    fn prepare_permission_reset(&mut self, cx: &mut Context<Self>) {
        if self.trusted_actions.is_busy() || self.default_permission_save_pending(cx) {
            self.publish_permission_reset(
                Projection::Blocked {
                    error: Error::Busy,
                    can_cancel: true,
                    thread_id: None,
                },
                cx,
            );
            return;
        }
        let Some(paths) = self.context_database(cx).zip(self.composer_config_path()) else {
            self.publish_permission_reset(
                Projection::Blocked {
                    error: Error::Unavailable,
                    can_cancel: true,
                    thread_id: None,
                },
                cx,
            );
            return;
        };
        let Some(operation) = self.permission_reset.operation.checked_add(1) else {
            return;
        };
        self.permission_reset.operation = operation;
        self.permission_reset.intent = None;
        self.permission_reset.paths = Some(paths.clone());
        self.publish_permission_reset(Projection::Preparing, cx);
        let worker = cx.background_executor().spawn(async move {
            let store =
                vega_store::Store::open_read_only(&paths.0).map_err(|_| Error::Unavailable)?;
            prepare_reset(&store, &paths.1)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if this.permission_reset.operation != operation
                    || this.permission_reset.projection != Projection::Preparing
                {
                    return;
                }
                if !cx.global::<SettingsOpen>().0 || this.settings_view.is_none() {
                    this.cancel_permission_reset(cx);
                    return;
                }
                match result {
                    Ok(intent) => {
                        this.permission_reset.intent = Some(intent);
                        this.publish_permission_reset(Projection::Confirming, cx);
                    }
                    Err(error) => this.publish_permission_reset(
                        Projection::Blocked {
                            error,
                            can_cancel: true,
                            thread_id: None,
                        },
                        cx,
                    ),
                }
            });
        })
        .detach();
    }

    fn confirm_permission_reset(&mut self, cx: &mut Context<Self>) {
        let Some((database, config)) = self.permission_reset.paths.clone() else {
            return;
        };
        let operation = self.permission_reset.operation;
        let Some(lease) =
            self.trusted_actions
                .acquire(TrustedActionKind::PermissionReset, 0, operation)
        else {
            self.permission_reset.intent = None;
            self.publish_permission_reset(
                Projection::Blocked {
                    error: Error::Busy,
                    can_cancel: true,
                    thread_id: None,
                },
                cx,
            );
            return;
        };
        let Some(intent) = self.permission_reset.intent.take() else {
            self.trusted_actions.release(lease);
            return;
        };
        self.permission_reset.lease = Some(lease);
        self.publish_permission_reset(Projection::Pausing, cx);
        let worker = cx
            .background_executor()
            .spawn(async move { PermissionResetSession::begin(database, config, intent) });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if this.permission_reset.operation != operation {
                    return;
                }
                match result {
                    Ok(session) => {
                        let threads = session.threads().to_vec();
                        this.permission_reset.session = Some(Arc::new(Mutex::new(Some(session))));
                        crate::automation_controller::ensure_global(cx);
                        let ready = cx.update_global::<AutomationGlobal, _>(|global, _| {
                            let ready = global.ready;
                            global.ready = false;
                            for thread in &threads {
                                global.invalidate(thread);
                                if let Some(run) = global.runs.get(thread) {
                                    run.cancel.cancel();
                                }
                            }
                            ready
                        });
                        this.permission_reset.scheduler_ready = Some(ready);
                        let mut cancel = threads;
                        cancel.extend(this.agent_controller.active.keys().cloned());
                        cancel.extend(this.agent_controller.ordinary_preparing.keys().cloned());
                        cancel.extend(this.agent_controller.queue_drain_waits.keys().cloned());
                        cancel.sort();
                        cancel.dedup();
                        for thread in &cancel {
                            this.agent_controller.request_active_cancel(thread);
                            this.cancel_agent_for_thread(thread, cx);
                        }
                        this.context_controller.cancel();
                        this.cancel_composer_prediction(cx);
                        for stream in this.permission_reset_streams() {
                            stream.update(cx, |stream, cx| {
                                stream.timeout_permission(cx);
                                stream.cancel_user_questions(cx);
                                stream.cancel_thread_permission_change(cx);
                            });
                        }
                        this.run_permission_reset(cx);
                    }
                    Err(error) => {
                        this.release_permission_reset(cx);
                        this.publish_permission_reset(
                            Projection::Blocked {
                                error,
                                can_cancel: true,
                                thread_id: None,
                            },
                            cx,
                        );
                    }
                }
            });
        })
        .detach();
    }

    fn run_permission_reset(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.permission_reset.session.clone() else {
            return;
        };
        let Some(replay) = cx
            .try_global::<AppReplaySessions>()
            .map(|sessions| sessions.0.clone())
        else {
            self.publish_permission_reset(
                Projection::Blocked {
                    error: Error::Unavailable,
                    can_cancel: false,
                    thread_id: None,
                },
                cx,
            );
            return;
        };
        let operation = self.permission_reset.operation;
        let mut threads: Vec<_> = self
            .permission_reset_streams()
            .iter()
            .map(|stream| stream.read(cx).thread_snapshot().id)
            .collect();
        if let Some(thread) = &cx.global::<OpenedThread>().0 {
            threads.push(thread.id.clone());
        }
        threads.sort();
        threads.dedup();
        self.publish_permission_reset(Projection::Stopping, cx);
        let worker = cx.background_executor().spawn(async move {
            let mut session = session
                .lock()
                .map_err(|_| (Error::Unavailable, false, None))?;
            let session = session.as_mut().ok_or((Error::Unavailable, false, None))?;
            session
                .complete(&replay, std::time::Duration::from_secs(20))
                .and_then(|_| session.refresh(&threads))
                .map_err(|error| (error, session.can_cancel(), session.blocked_thread()))
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if this.permission_reset.operation != operation {
                    return;
                }
                match result {
                    Ok(refresh) => {
                        this.publish_permission_reset(Projection::Refreshing, cx);
                        if this.apply_permission_reset_refresh(refresh, cx) {
                            this.release_permission_reset(cx);
                            this.publish_permission_reset(Projection::Completed, cx);
                        } else {
                            this.publish_permission_reset(
                                Projection::Blocked {
                                    error: Error::OutcomeUnknown,
                                    can_cancel: false,
                                    thread_id: None,
                                },
                                cx,
                            );
                        }
                    }
                    Err((error, can_cancel, thread_id)) => this.publish_permission_reset(
                        Projection::Blocked {
                            error,
                            can_cancel,
                            thread_id,
                        },
                        cx,
                    ),
                }
            });
        })
        .detach();
    }

    pub(super) fn cancel_unconfirmed_permission_reset(&mut self, cx: &mut Context<Self>) {
        if !cx.global::<SettingsOpen>().0
            && matches!(
                self.permission_reset.projection,
                Projection::Preparing | Projection::Confirming
            )
        {
            self.cancel_permission_reset(cx);
        }
    }

    fn cancel_permission_reset(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.permission_reset.projection,
            Projection::Preparing | Projection::Confirming
        ) || matches!(self.permission_reset.projection, Projection::Blocked { .. })
            && self.permission_reset.session.is_none()
        {
            self.permission_reset.operation = self.permission_reset.operation.saturating_add(1);
            self.permission_reset.intent = None;
            self.publish_permission_reset(Projection::Idle, cx);
            return;
        }
        if !matches!(
            self.permission_reset.projection,
            Projection::Blocked {
                can_cancel: true,
                ..
            }
        ) {
            return;
        }
        let Some(session) = self.permission_reset.session.clone() else {
            return;
        };
        let operation = self.permission_reset.operation;
        self.publish_permission_reset(Projection::Stopping, cx);
        let worker = cx.background_executor().spawn(async move {
            let mut guard = session.lock().map_err(|_| Error::Unavailable)?;
            if !guard
                .as_ref()
                .is_some_and(PermissionResetSession::can_cancel)
            {
                return Err(Error::OutcomeUnknown);
            }
            guard.take();
            Ok(())
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if this.permission_reset.operation != operation {
                    return;
                }
                match result {
                    Ok(()) => {
                        this.release_permission_reset(cx);
                        this.publish_permission_reset(Projection::Idle, cx);
                    }
                    Err(error) => this.publish_permission_reset(
                        Projection::Blocked {
                            error,
                            can_cancel: false,
                            thread_id: None,
                        },
                        cx,
                    ),
                }
            });
        })
        .detach();
    }

    fn release_permission_reset(&mut self, cx: &mut Context<Self>) {
        self.permission_reset.session = None;
        if let Some(lease) = self.permission_reset.lease.take() {
            self.trusted_actions.release(lease);
        }
        if let Some(ready) = self.permission_reset.scheduler_ready.take() {
            cx.update_global::<AutomationGlobal, _>(|global, _| global.ready = ready);
        }
    }

    fn publish_permission_reset(&mut self, projection: Projection, cx: &mut Context<Self>) {
        self.permission_reset.projection = projection.clone();
        if let Some(view) = &self.settings_view {
            view.update(cx, |view, cx| {
                view.apply_permission_reset_projection(projection, cx)
            });
        }
        cx.notify();
    }

    fn inspect_permission_reset(&mut self, cx: &mut Context<Self>) {
        let Projection::Blocked {
            thread_id: Some(thread),
            ..
        } = &self.permission_reset.projection
        else {
            return;
        };
        let thread = match &cx.global::<VegaStore>().0 {
            Ok(store) => vega_conversation::threads::get_thread(store, thread).ok(),
            Err(_) => None,
        };
        if let Some(thread) = thread {
            self.accept_palette_thread(thread, cx);
        }
    }
}
