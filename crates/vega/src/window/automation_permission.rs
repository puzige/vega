use super::*;
use crate::automation_controller::AutomationGlobal;
use vega_conversation::automation::AutomationPermissionService;

pub(super) enum PermissionWork {
    Prepare(Box<AutomationPermissionRequested>),
    Confirm(Box<AutomationPermissionConfirmed>),
    Reload(AutomationPermissionReloadRequested),
}

enum Outcome {
    Confirmation(Box<AutomationPermissionChange>),
    Saved(Vec<AutomationTask>),
    Reloaded(Box<Thread>, Vec<AutomationTask>),
}

impl VegaWindow {
    pub(super) fn automation_permission_work(
        &mut self,
        stream: Entity<ConversationStream>,
        work: PermissionWork,
        cx: &mut Context<Self>,
    ) {
        let (operation, thread) = match &work {
            PermissionWork::Prepare(request) => (request.operation, request.source.id.clone()),
            PermissionWork::Confirm(request) => {
                (request.operation, request.change.task().thread_id.clone())
            }
            PermissionWork::Reload(request) => (request.operation, request.thread_id.clone()),
        };
        crate::automation_controller::ensure_global(cx);
        if !self.owns_stream_request(&stream, &thread, cx)
            || cx.global::<SettingsOpen>().0
            || self.trusted_actions.is_busy()
            || cx
                .global::<AutomationGlobal>()
                .mutations
                .contains_key(&thread)
            || !stream
                .read(cx)
                .owns_automation_permission_operation(operation)
        {
            stream.update(cx, |stream, cx| {
                stream.fail_automation_permission(operation, ThreadPermissionError::Conflict, cx)
            });
            return;
        }
        let Some(database) = self.context_database(cx) else {
            stream.update(cx, |stream, cx| {
                stream.fail_automation_permission(operation, ThreadPermissionError::Unavailable, cx)
            });
            return;
        };
        let Some(lease) =
            self.trusted_actions
                .acquire(TrustedActionKind::PermissionSelection, 0, operation)
        else {
            return;
        };
        if let PermissionWork::Confirm(request) = &work
            && !stream.update(cx, |stream, cx| {
                stream.begin_automation_permission_save(request, cx)
            })
        {
            self.trusted_actions.release(lease);
            return;
        }
        let resume_kind = match &work {
            PermissionWork::Confirm(request) if request.change.resumes_task() => {
                Some(request.change.task().kind)
            }
            _ => None,
        };
        let lane = cx.global::<AutomationGlobal>().mutation_lane.clone();
        let worker = cx.background_executor().spawn(async move {
            let _lane = lane
                .lock()
                .map_err(|_| ThreadPermissionError::Unavailable)?;
            let service = AutomationPermissionService::new(database);
            match work {
                PermissionWork::Prepare(request) => service
                    .prepare(&request.source, &request.task, request.resume)
                    .map(|change| Outcome::Confirmation(Box::new(change))),
                PermissionWork::Confirm(request) => {
                    service.save(&request.change, true).map(Outcome::Saved)
                }
                PermissionWork::Reload(request) => service
                    .read(&request.thread_id)
                    .map(|(thread, tasks)| Outcome::Reloaded(Box::new(thread), tasks)),
            }
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !this.trusted_actions.release(lease)
                    || !stream
                        .read(cx)
                        .owns_automation_permission_operation(operation)
                {
                    return;
                }
                match result {
                    Ok(Outcome::Confirmation(change)) => {
                        if this.owns_stream_request(&stream, &thread, cx)
                            && !cx.global::<SettingsOpen>().0
                        {
                            stream.update(cx, |stream, cx| {
                                stream
                                    .show_automation_permission_confirmation(operation, *change, cx)
                            });
                        } else {
                            stream.update(cx, ConversationStream::cancel_automation_permission);
                        }
                    }
                    Ok(Outcome::Saved(tasks)) => {
                        if let Some(kind) = resume_kind {
                            cx.update_global::<AutomationGlobal, _>(|global, _| {
                                global.revoked.remove(&(thread.clone(), kind));
                            });
                        }
                        this.refresh_automation_editors(
                            &thread,
                            &tasks,
                            &std::collections::HashMap::new(),
                            cx,
                        );
                        stream.update(cx, |stream, cx| {
                            stream.finish_automation_permission_save(operation, Ok(tasks), cx)
                        });
                        this.bind_automation(stream.clone(), thread.clone(), cx);
                    }
                    Ok(Outcome::Reloaded(saved, tasks)) => {
                        let saved = *saved;
                        if this.owns_stream_request(&stream, &thread, cx) {
                            cx.set_global(OpenedThread(Some(saved.clone())));
                        }
                        stream.update(cx, |stream, cx| {
                            stream.finish_automation_permission_reload(
                                operation,
                                Ok((saved, tasks)),
                                cx,
                            )
                        });
                    }
                    Err(error) => stream.update(cx, |stream, cx| {
                        stream.fail_automation_permission(operation, error, cx)
                    }),
                }
            });
        })
        .detach();
    }
}
