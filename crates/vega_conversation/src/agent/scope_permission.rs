use super::permission_queue::PermissionQueueState;
use super::*;
use crate::types::{CommandScopeDecision, CommandScopeRequest, PermissionPresentation};

pub struct PendingScopePermission {
    request: CommandScopeRequest,
    lease: CommandScopeLease,
}

impl PendingScopePermission {
    pub fn request(&self) -> &CommandScopeRequest {
        &self.request
    }

    pub fn is_resolved(&self) -> bool {
        self.lease.is_resolved()
    }

    pub fn presentation(&self, tool: Option<(&str, &str)>) -> PermissionPresentation {
        match tool {
            Some(("bash", command)) if command == self.request.command() => {
                PermissionPresentation::Ready
            }
            Some(_) => PermissionPresentation::Mismatch,
            None => PermissionPresentation::WaitingForTool,
        }
    }

    pub fn into_parts(self) -> (CommandScopeRequest, CommandScopeLease) {
        (self.request, self.lease)
    }
}

pub struct CommandScopeLease {
    responder: ScopeResponder,
}

impl CommandScopeLease {
    pub fn respond(&mut self, decision: CommandScopeDecision) -> bool {
        self.responder.respond(decision)
    }

    pub fn is_resolved(&self) -> bool {
        self.responder.is_resolved()
    }
}

impl Drop for CommandScopeLease {
    fn drop(&mut self) {
        self.responder.respond(CommandScopeDecision::Timeout);
    }
}

impl fmt::Debug for CommandScopeLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandScopeLease")
            .field("resolved", &self.is_resolved())
            .finish()
    }
}

#[derive(Clone)]
pub(super) struct ScopeResponder {
    state: Arc<ScopeResponseState>,
}

struct ScopeResponseState {
    sender: Mutex<Option<oneshot::Sender<CommandScopeDecision>>>,
    activity: Option<super::stage_activity::PendingActivity>,
}

impl ScopeResponder {
    pub(super) fn respond(&self, decision: CommandScopeDecision) -> bool {
        let Ok(mut sender) = self.state.sender.lock() else {
            return false;
        };
        let Some(sender) = sender.take() else {
            return false;
        };
        if let Some(activity) = &self.state.activity {
            activity.finish();
        }
        let _ = sender.send(decision);
        true
    }

    pub(super) fn is_resolved(&self) -> bool {
        self.state
            .sender
            .lock()
            .map_or(true, |sender| sender.is_none())
    }

    fn same_latch(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

struct ScopeWaitGuard {
    responder: ScopeResponder,
    queue: Arc<Mutex<PermissionQueueState>>,
}

impl Drop for ScopeWaitGuard {
    fn drop(&mut self) {
        self.responder.respond(CommandScopeDecision::Timeout);
        if let Ok(mut state) = self.queue.lock()
            && state
                .active_scope
                .as_ref()
                .is_some_and(|active| active.same_latch(&self.responder))
        {
            state.active_scope = None;
            state.queued_scope = None;
        }
    }
}

impl PermissionQueue {
    pub fn take_pending_scope(&self) -> Option<PendingScopePermission> {
        self.state
            .lock()
            .ok()?
            .queued_scope
            .take()
            .filter(|pending| !pending.is_resolved())
    }

    pub(super) fn enqueue_scope(
        &self,
        request: CommandScopeRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<CommandScopeDecision, VegaError>> {
        let (sender, receiver) = oneshot::channel();
        let responder = ScopeResponder {
            state: Arc::new(ScopeResponseState {
                sender: Mutex::new(Some(sender)),
                activity: self.activity.as_ref().map(|activity| activity.pending()),
            }),
        };
        let pending = PendingScopePermission {
            request,
            lease: CommandScopeLease {
                responder: responder.clone(),
            },
        };
        let old = self.state.lock().ok().map(|mut state| {
            let old_active = state.active.take();
            let old_queued = state.queued.drain(..).collect::<Vec<_>>();
            let old_scope = state.active_scope.replace(responder.clone());
            let old_pending = state.queued_scope.replace(pending);
            state.notification_version = state.notification_version.wrapping_add(1);
            let notifier = state.notifier.as_ref().map(|(_, sender)| sender.clone());
            (
                old_active,
                old_queued,
                old_scope,
                old_pending,
                notifier,
                state.notification_version,
            )
        });
        if let Some((active, queued, scope, pending, notifier, version)) = old {
            if let Some(active) = active {
                active.respond(PermissionDecision::Unavailable);
            }
            if let Some(scope) = scope {
                scope.respond(CommandScopeDecision::Timeout);
            }
            drop(queued);
            drop(pending);
            if !notifier.is_some_and(|notifier| notifier.send(version).is_ok()) {
                responder.respond(CommandScopeDecision::Timeout);
            }
        } else {
            responder.respond(CommandScopeDecision::Timeout);
        }
        let guard = ScopeWaitGuard {
            responder,
            queue: self.state.clone(),
        };
        async move {
            let decision = tokio::select! {
                biased;
                _ = cancel.cancelled() => CommandScopeDecision::Timeout,
                _ = tokio::time::sleep(PERMISSION_TIMEOUT) => CommandScopeDecision::Timeout,
                result = receiver => result.unwrap_or(CommandScopeDecision::Timeout),
            };
            drop(guard);
            Ok(decision)
        }
        .boxed()
    }
}
