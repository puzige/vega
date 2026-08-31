use super::*;

pub struct PendingPermission {
    pub(crate) request: Option<PermissionRequest>,
    pub(crate) responder: Option<PermissionResponder>,
    pub(crate) armed: bool,
}

impl PendingPermission {
    /// Content-free request safe for card rendering.
    pub fn request(&self) -> Option<&PermissionRequest> {
        self.request.as_ref()
    }

    /// Transfers the safe request and a responder-only card lease. The UI may
    /// use the request call id transiently for card lookup, then must discard
    /// it before storing the lease in an entity.
    pub fn into_parts(mut self) -> Option<(PermissionRequest, PermissionLease)> {
        self.armed = false;
        Some((
            self.request.take()?,
            PermissionLease {
                responder: self.responder.take()?,
                armed: true,
            },
        ))
    }
}

/// Fail-closed ownership guard held for the exact lifetime of one UI card.
pub struct PermissionLease {
    pub(crate) responder: PermissionResponder,
    pub(crate) armed: bool,
}

impl PermissionLease {
    /// Resolves the shared latch and disarms the disappearance guard.
    pub fn respond(&mut self, decision: PermissionDecision) -> bool {
        let won = self.responder.respond(decision);
        if self.responder.is_resolved() {
            self.armed = false;
        }
        won
    }

    /// Whether runtime cancellation, timeout, or an explicit action won.
    pub fn is_resolved(&self) -> bool {
        self.responder.is_resolved()
    }
}

impl Drop for PermissionLease {
    fn drop(&mut self) {
        if self.armed {
            self.responder.respond(PermissionDecision::Timeout);
        }
    }
}

impl fmt::Debug for PermissionLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionLease")
            .field("resolved", &self.is_resolved())
            .finish()
    }
}

impl Drop for PendingPermission {
    fn drop(&mut self) {
        if self.armed
            && let Some(responder) = &self.responder
        {
            responder.respond(PermissionDecision::Timeout);
        }
    }
}

impl fmt::Debug for PendingPermission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingPermission")
            .field("tool", &self.request.as_ref().map(|request| &request.tool))
            .field(
                "danger",
                &self
                    .request
                    .as_ref()
                    .is_some_and(|request| request.danger_rule_id.is_some()),
            )
            .field("display_target", &"[REDACTED]")
            .field("call_id", &"[OPAQUE]")
            .finish()
    }
}

pub(crate) struct PermissionResponseState {
    pub(crate) sender: Mutex<Option<oneshot::Sender<PermissionDecision>>>,
}

impl Drop for PermissionResponseState {
    fn drop(&mut self) {
        if let Ok(sender) = self.sender.get_mut()
            && let Some(sender) = sender.take()
        {
            let _ = sender.send(PermissionDecision::Timeout);
        }
    }
}

/// Thread-safe first-wins permission response latch.
#[derive(Clone)]
pub struct PermissionResponder {
    pub(crate) state: Arc<PermissionResponseState>,
}

impl PermissionResponder {
    /// Resolves the request exactly once. Returns false after another path won.
    pub fn respond(&self, decision: PermissionDecision) -> bool {
        let Ok(mut sender) = self.state.sender.lock() else {
            return false;
        };
        let Some(sender) = sender.take() else {
            return false;
        };
        let _ = sender.send(decision);
        true
    }

    /// Whether any explicit or implicit terminal path already won.
    pub fn is_resolved(&self) -> bool {
        self.state
            .sender
            .lock()
            .map_or(true, |sender| sender.is_none())
    }

    pub(crate) fn same_latch(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl fmt::Debug for PermissionResponder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionResponder")
            .field("resolved", &self.is_resolved())
            .finish()
    }
}

pub(crate) struct PermissionWaitGuard {
    pub(crate) responder: PermissionResponder,
    pub(crate) queue_state: Arc<Mutex<PermissionQueueState>>,
}

impl Drop for PermissionWaitGuard {
    fn drop(&mut self) {
        self.responder.respond(PermissionDecision::Timeout);
        if let Ok(mut state) = self.queue_state.lock()
            && state
                .active
                .as_ref()
                .is_some_and(|active| active.same_latch(&self.responder))
        {
            state.active = None;
            state.queued.retain(|pending| {
                !pending
                    .responder
                    .as_ref()
                    .is_some_and(|queued| queued.same_latch(&self.responder))
            });
        }
    }
}

/// Lost-wake-safe listener owned by the exact lifetime of one UI task.
///
/// Dropping the listener clears only its own generation and rejects
/// an unresolved prompt, so a window/thread disappearance cannot leave the
/// runtime waiting on a stale card.
pub struct PermissionQueueListener {
    pub(crate) state: Arc<Mutex<PermissionQueueState>>,
    pub(crate) generation: u64,
    pub(crate) receiver: watch::Receiver<u64>,
}

impl PermissionQueueListener {
    /// Waits for a newly enqueued request. False means the subscription was
    /// closed or replaced and the UI task must stop.
    pub async fn changed(&mut self) -> bool {
        self.receiver.changed().await.is_ok()
    }
}

impl fmt::Debug for PermissionQueueListener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionQueueListener")
            .finish_non_exhaustive()
    }
}

impl Drop for PermissionQueueListener {
    fn drop(&mut self) {
        let closed = self.state.lock().ok().and_then(|mut state| {
            if state
                .notifier
                .as_ref()
                .is_some_and(|(generation, _)| *generation == self.generation)
            {
                state.notifier = None;
                let responder = state.active.take();
                let queued = state.queued.drain(..).collect::<Vec<_>>();
                Some((responder, queued))
            } else {
                None
            }
        });
        if let Some((responder, queued)) = closed {
            if let Some(responder) = responder {
                responder.respond(PermissionDecision::Timeout);
            }
            drop(queued);
        }
    }
}

#[derive(Default)]
pub(crate) struct PermissionQueueState {
    pub(crate) active: Option<PermissionResponder>,
    pub(crate) queued: VecDeque<PendingPermission>,
    pub(crate) notifier: Option<(u64, watch::Sender<u64>)>,
    pub(crate) next_generation: u64,
    pub(crate) notification_version: u64,
}

/// Concrete conversation-to-GPUI request/response handoff.
///
/// This is not a lifecycle stream: durable lifecycle visibility remains
/// exclusively on [`ConversationEvent`]. The queue carries only the pending
/// human decision and resolves every disappearance as Timeout.
#[derive(Clone, Default)]
pub struct PermissionQueue {
    pub(crate) state: Arc<Mutex<PermissionQueueState>>,
}

impl PermissionQueue {
    /// Creates an empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Content-free controller guard for trusted workspace actions.
    /// Lock poisoning fails closed because absence cannot be proven.
    pub fn has_pending(&self) -> bool {
        self.state.lock().map_or(true, |state| {
            state
                .active
                .as_ref()
                .is_some_and(|responder| !responder.is_resolved())
                || state.queued.iter().any(|pending| {
                    pending
                        .responder
                        .as_ref()
                        .is_some_and(|responder| !responder.is_resolved())
                })
        })
    }

    /// Installs the sole live UI wakeup seam. Registration happens before a
    /// runtime turn starts, eliminating the Proposed-before-enqueue lost wake.
    pub fn subscribe(&self) -> PermissionQueueListener {
        let (notifier, receiver) = watch::channel(0);
        let (generation, replaced_active, replaced_queued) = match self.state.lock() {
            Ok(mut state) => {
                let replacing = state.notifier.is_some();
                let replaced_active = replacing.then(|| state.active.take()).flatten();
                let replaced_queued = if replacing {
                    state.queued.drain(..).collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                state.next_generation = state.next_generation.wrapping_add(1);
                if state.next_generation == 0 {
                    state.next_generation = 1;
                }
                let generation = state.next_generation;
                state.notifier = Some((generation, notifier));
                (generation, replaced_active, replaced_queued)
            }
            Err(_) => (0, None, Vec::new()),
        };
        if let Some(replaced_active) = replaced_active {
            replaced_active.respond(PermissionDecision::Timeout);
        }
        drop(replaced_queued);
        let listener = PermissionQueueListener {
            state: self.state.clone(),
            generation,
            receiver,
        };
        if generation == 0 {
            self.timeout_active();
        }
        listener
    }

    /// Removes the newest pending card request for the UI. At most one exists.
    pub fn take_pending(&self) -> Option<PendingPermission> {
        let Ok(mut state) = self.state.lock() else {
            return None;
        };
        while let Some(pending) = state.queued.pop_front() {
            if pending
                .responder
                .as_ref()
                .is_some_and(|responder| !responder.is_resolved())
            {
                return Some(pending);
            }
        }
        None
    }

    /// Fails the current prompt closed, used when the owning view disappears.
    pub fn timeout_active(&self) -> bool {
        let responder = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.active.clone());
        responder.is_some_and(|responder| responder.respond(PermissionDecision::Timeout))
    }
}

impl fmt::Debug for PermissionQueue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pending = self
            .state
            .lock()
            .map_or(0, |state| usize::from(state.active.is_some()));
        formatter
            .debug_struct("PermissionQueue")
            .field("pending_count", &pending)
            .finish()
    }
}

impl PermissionHook for PermissionQueue {
    fn request(
        &self,
        request: PermissionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        if !valid_permission_request(&request) {
            return async { Ok(PermissionDecision::Timeout) }.boxed();
        }
        let (sender, receiver) = oneshot::channel();
        let responder = PermissionResponder {
            state: Arc::new(PermissionResponseState {
                sender: Mutex::new(Some(sender)),
            }),
        };
        let pending = PendingPermission {
            request: Some(request),
            responder: Some(responder.clone()),
            armed: true,
        };
        let old = match self.state.lock() {
            Ok(mut state) => {
                let old_active = state.active.replace(responder.clone());
                let old_queued = state.queued.drain(..).collect::<Vec<_>>();
                state.queued.push_back(pending);
                state.notification_version = state.notification_version.wrapping_add(1);
                let version = state.notification_version;
                let notifier = state
                    .notifier
                    .as_ref()
                    .map(|(_, notifier)| notifier.clone());
                Some((old_active, old_queued, notifier, version))
            }
            Err(_) => None,
        };
        let Some((old_active, old_queued, notifier, version)) = old else {
            responder.respond(PermissionDecision::Timeout);
            return async { Ok(PermissionDecision::Timeout) }.boxed();
        };
        if let Some(old_active) = old_active {
            old_active.respond(PermissionDecision::Timeout);
        }
        drop(old_queued);
        let notified = notifier.is_some_and(|notifier| notifier.send(version).is_ok());
        if !notified {
            responder.respond(PermissionDecision::Timeout);
        }

        let guard = PermissionWaitGuard {
            responder,
            queue_state: self.state.clone(),
        };
        async move {
            let decision = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    guard.responder.respond(PermissionDecision::Timeout);
                    PermissionDecision::Timeout
                }
                result = receiver => result.unwrap_or(PermissionDecision::Timeout),
            };
            Ok(decision)
        }
        .boxed()
    }
}

pub(crate) fn valid_permission_request(request: &PermissionRequest) -> bool {
    if request.call_id.is_empty() || request.display_target.is_empty() {
        return false;
    }
    let danger_valid = match (&request.danger_rule_id, &request.danger_reason) {
        (None, None) => true,
        (Some(rule), Some(reason)) => !rule.is_empty() && !reason.is_empty(),
        _ => false,
    };
    if !danger_valid {
        return false;
    }
    match request.tool.as_str() {
        "bash" => true,
        "write" | "edit" if request.danger_rule_id.is_none() => {
            let path = std::path::Path::new(&request.display_target);
            !path.is_absolute()
                && path
                    .components()
                    .all(|component| matches!(component, std::path::Component::Normal(_)))
        }
        _ => false,
    }
}

pub struct RejectPermissionHook;

impl PermissionHook for RejectPermissionHook {
    fn request(
        &self,
        _request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        async { Ok(PermissionDecision::Timeout) }.boxed()
    }
}

pub(crate) struct RuntimePermissionAdapter<'a> {
    pub(crate) shared: &'a dyn PermissionHook,
}

impl RuntimePermissionHook for RuntimePermissionAdapter<'_> {
    fn request(
        &self,
        prompt: vega_runtime::RuntimePermissionPrompt,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.shared
            .request(permission_request_from_runtime(&prompt), cancel)
            .map(|decision| decision.map(permission_decision_to_runtime))
            .boxed()
    }
}
