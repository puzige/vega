use std::fmt;
use std::sync::{Arc, Mutex};

use futures::{FutureExt, future::BoxFuture};
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    RuntimeUserQuestionHook, RuntimeUserQuestionRequest, RuntimeUserQuestionResponse,
};

use crate::types::{UserQuestionAnswers, UserQuestionId, UserQuestionResolution};

type Response = RuntimeUserQuestionResponse;

struct QuestionResponseState {
    activity: Option<super::stage_activity::PendingActivity>,
    request: RuntimeUserQuestionRequest,
    id: UserQuestionId,
    delivery: Mutex<QuestionDelivery>,
    signal: Mutex<Option<watch::Sender<u64>>>,
    cancel: CancellationToken,
}

struct QuestionDelivery {
    sender: Option<oneshot::Sender<Response>>,
    resolution: UserQuestionResolution,
}

impl QuestionResponseState {
    fn resolve(&self, response: Response) -> bool {
        let Ok(mut delivery) = self.delivery.lock() else {
            return false;
        };
        let Some(sender) = delivery.sender.take() else {
            return false;
        };
        if let Some(activity) = &self.activity {
            activity.finish();
        }
        let cancelled = self.cancel.is_cancelled();
        let response = if cancelled {
            Response::Cancelled
        } else {
            response
        };
        let answered = matches!(&response, Response::Answered(_));
        let sent = sender.send(response).is_ok();
        delivery.resolution = if sent && answered {
            UserQuestionResolution::Answered
        } else {
            UserQuestionResolution::Cancelled
        };
        drop(delivery);
        if let Ok(signal) = self.signal.lock()
            && let Some(signal) = signal.as_ref()
        {
            signal.send_modify(|version| *version = version.wrapping_add(1));
        }
        sent && !cancelled
    }

    fn resolution(&self) -> UserQuestionResolution {
        self.delivery
            .lock()
            .map_or(UserQuestionResolution::Cancelled, |delivery| {
                delivery.resolution
            })
    }

    fn resolved(&self) -> bool {
        self.resolution() != UserQuestionResolution::Pending
    }
}

pub struct PendingUserQuestion {
    state: Arc<QuestionResponseState>,
}

impl PendingUserQuestion {
    pub fn request(&self) -> &RuntimeUserQuestionRequest {
        &self.state.request
    }
    pub fn is_resolved(&self) -> bool {
        self.state.resolved()
    }
    pub fn id(&self) -> UserQuestionId {
        self.state.id.clone()
    }
    pub fn resolution(&self) -> UserQuestionResolution {
        self.state.resolution()
    }
    pub fn respond(&self, answers: UserQuestionAnswers) -> Result<bool, &'static str> {
        answers.validate(&self.state.request.input)?;
        Ok(self.state.resolve(Response::Answered(answers)))
    }
}

impl Drop for PendingUserQuestion {
    fn drop(&mut self) {
        self.state.resolve(Response::Cancelled);
    }
}

impl fmt::Debug for PendingUserQuestion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingUserQuestion")
            .field("resolved", &self.is_resolved())
            .finish()
    }
}

#[derive(Default)]
struct QueueState {
    active: Option<Arc<QuestionResponseState>>,
    pending: Option<Arc<QuestionResponseState>>,
    listener: Option<(u64, watch::Sender<u64>)>,
    generation: u64,
}

#[derive(Clone, Default)]
pub struct UserQuestionQueue {
    activity: Option<super::stage_activity::StageActivity>,
    state: Arc<Mutex<QueueState>>,
}

impl UserQuestionQueue {
    pub(super) fn with_activity(activity: super::stage_activity::StageActivity) -> Self {
        Self {
            activity: Some(activity),
            ..Self::default()
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_pending(&self) -> bool {
        self.state.lock().map_or(true, |state| {
            state
                .active
                .as_ref()
                .is_some_and(|active| !active.resolved())
        })
    }

    pub fn cancel_active(&self) {
        if let Ok(state) = self.state.lock()
            && let Some(active) = &state.active
        {
            active.resolve(Response::Cancelled);
        }
    }

    pub fn take_pending(&self) -> Option<PendingUserQuestion> {
        let state = self.state.lock().ok()?.pending.take()?;
        (!state.resolved()).then_some(PendingUserQuestion { state })
    }

    pub fn subscribe(&self) -> UserQuestionListener {
        let (sender, receiver) = watch::channel(0);
        let generation = if let Ok(mut state) = self.state.lock() {
            if let Some(active) = state.active.take() {
                active.resolve(Response::Cancelled);
            }
            state.pending = None;
            state.generation = state.generation.wrapping_add(1);
            let generation = state.generation;
            state.listener = Some((generation, sender));
            generation
        } else {
            0
        };
        UserQuestionListener {
            queue: self.clone(),
            generation,
            receiver,
        }
    }
}

impl fmt::Debug for UserQuestionQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserQuestionQueue")
            .field("pending", &self.has_pending())
            .finish()
    }
}

pub struct UserQuestionListener {
    queue: UserQuestionQueue,
    generation: u64,
    receiver: watch::Receiver<u64>,
}

impl UserQuestionListener {
    pub async fn changed(&mut self) -> bool {
        self.receiver.changed().await.is_ok()
    }
}

impl Drop for UserQuestionListener {
    fn drop(&mut self) {
        if let Ok(mut state) = self.queue.state.lock()
            && state
                .listener
                .as_ref()
                .is_some_and(|(generation, _)| *generation == self.generation)
        {
            state.listener = None;
            state.pending = None;
            if let Some(active) = state.active.take() {
                active.resolve(Response::Cancelled);
            }
        }
    }
}

struct QuestionWaitGuard {
    queue: UserQuestionQueue,
    response: Arc<QuestionResponseState>,
}

impl Drop for QuestionWaitGuard {
    fn drop(&mut self) {
        self.response.resolve(Response::Cancelled);
        if let Ok(mut state) = self.queue.state.lock()
            && state
                .active
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, &self.response))
        {
            state.active = None;
            state.pending = None;
        }
    }
}

impl RuntimeUserQuestionHook for UserQuestionQueue {
    fn request(
        &self,
        request: RuntimeUserQuestionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Response> {
        if request.call_id.is_empty() || request.input.validate().is_err() {
            return async { Response::Unavailable }.boxed();
        }
        let (sender, receiver) = oneshot::channel();
        let response = Arc::new(QuestionResponseState {
            activity: self.activity.as_ref().map(|activity| activity.pending()),
            request,
            id: UserQuestionId(ulid::Ulid::generate().to_string()),
            delivery: Mutex::new(QuestionDelivery {
                sender: Some(sender),
                resolution: UserQuestionResolution::Pending,
            }),
            signal: Mutex::new(None),
            cancel: cancel.clone(),
        });
        let notified = if let Ok(mut state) = self.state.lock() {
            if let Ok(mut signal) = response.signal.lock() {
                *signal = state.listener.as_ref().map(|(_, sender)| sender.clone());
            }
            if let Some(active) = state.active.replace(response.clone()) {
                active.resolve(Response::Cancelled);
            }
            state.pending = Some(response.clone());
            state.listener.as_ref().is_some_and(|(_, listener)| {
                if listener.is_closed() {
                    return false;
                }
                listener.send_modify(|version| *version = version.wrapping_add(1));
                true
            })
        } else {
            false
        };
        if !notified {
            response.resolve(Response::Unavailable);
        }
        let guard = QuestionWaitGuard {
            queue: self.clone(),
            response,
        };
        async move {
            let mut receiver = receiver;
            let answer = tokio::select! {
                biased;
                result = &mut receiver => result.unwrap_or(Response::Unavailable),
                _ = cancel.cancelled() => {
                    guard.response.resolve(Response::Cancelled);
                    receiver.await.unwrap_or(Response::Cancelled)
                }
            };
            drop(guard);
            answer
        }
        .boxed()
    }
}
