use std::fmt;
use std::sync::{Arc, Mutex};

use futures::{FutureExt, future::BoxFuture};
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    RuntimeUserQuestionHook, RuntimeUserQuestionRequest, RuntimeUserQuestionResponse,
};

use crate::types::UserQuestionAnswers;

type Response = RuntimeUserQuestionResponse;

struct QuestionResponseState {
    request: RuntimeUserQuestionRequest,
    sender: Mutex<Option<oneshot::Sender<Response>>>,
    cancel: CancellationToken,
}

impl QuestionResponseState {
    fn resolve(&self, response: Response) -> bool {
        let Ok(mut sender) = self.sender.lock() else {
            return false;
        };
        let Some(sender) = sender.take() else {
            return false;
        };
        let cancelled = self.cancel.is_cancelled();
        sender
            .send(if cancelled {
                Response::Cancelled
            } else {
                response
            })
            .is_ok()
            && !cancelled
    }

    fn resolved(&self) -> bool {
        self.sender.lock().map_or(true, |sender| sender.is_none())
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
    version: u64,
}

#[derive(Clone, Default)]
pub struct UserQuestionQueue {
    state: Arc<Mutex<QueueState>>,
}

impl UserQuestionQueue {
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
            request,
            sender: Mutex::new(Some(sender)),
            cancel: cancel.clone(),
        });
        let notified = if let Ok(mut state) = self.state.lock() {
            if let Some(active) = state.active.replace(response.clone()) {
                active.resolve(Response::Cancelled);
            }
            state.pending = Some(response.clone());
            state.version = state.version.wrapping_add(1);
            state
                .listener
                .as_ref()
                .is_some_and(|(_, listener)| listener.send(state.version).is_ok())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::UserQuestionInput;

    fn request() -> RuntimeUserQuestionRequest {
        RuntimeUserQuestionRequest { call_id: "call".into(), input: UserQuestionInput::from_json(r#"{"questions":[{"id":"choice","header":"Choice","question":"Which one?","options":[{"label":"A","description":"First"},{"label":"B","description":"Second"}]}]}"#).unwrap() }
    }

    fn answer() -> UserQuestionAnswers {
        UserQuestionAnswers::from_json(
            r#"{"answers":{"choice":{"answers":["A"]}}}"#,
            &request().input,
        )
        .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn question_wait_has_no_permission_timeout_and_invalid_answer_keeps_latch() {
        let queue = UserQuestionQueue::new();
        let mut listener = queue.subscribe();
        let wait = queue.request(request(), CancellationToken::new());
        listener.changed().await;
        let pending = queue.take_pending().unwrap();
        tokio::pin!(wait);
        assert!(futures::poll!(&mut wait).is_pending());
        tokio::time::advance(std::time::Duration::from_secs(3600)).await;
        assert!(futures::poll!(&mut wait).is_pending());
        assert!(pending.respond(UserQuestionAnswers::default()).is_err());
        assert!(queue.has_pending());
        assert!(pending.respond(answer()).unwrap());
        assert!(!pending.respond(answer()).unwrap());
        assert_eq!(wait.await, Response::Answered(answer()));
    }

    #[tokio::test]
    async fn question_submit_cancel_and_listener_replacement_are_first_wins() {
        for answer_first in [true, false] {
            let queue = UserQuestionQueue::new();
            let _listener = queue.subscribe();
            let cancel = CancellationToken::new();
            let wait = queue.request(request(), cancel.clone());
            let pending = queue.take_pending().unwrap();
            if answer_first {
                assert!(pending.respond(answer()).unwrap());
            }
            cancel.cancel();
            if !answer_first {
                assert!(!pending.respond(answer()).unwrap());
            }
            assert_eq!(
                wait.await,
                if answer_first {
                    Response::Answered(answer())
                } else {
                    Response::Cancelled
                }
            );
            assert!(!queue.has_pending());
        }
        let queue = UserQuestionQueue::new();
        let first_listener = queue.subscribe();
        let first = queue.request(request(), CancellationToken::new());
        let first_pending = queue.take_pending().unwrap();
        let _second_listener = queue.subscribe();
        assert_eq!(first.await, Response::Cancelled);
        assert!(!first_pending.respond(answer()).unwrap());
        drop(first_listener);
        let second = queue.request(request(), CancellationToken::new());
        assert!(queue.take_pending().unwrap().respond(answer()).unwrap());
        assert_eq!(second.await, Response::Answered(answer()));
    }

    #[tokio::test]
    async fn missing_listener_and_destroyed_owner_release_question_wait() {
        let queue = UserQuestionQueue::new();
        assert_eq!(
            queue.request(request(), CancellationToken::new()).await,
            Response::Unavailable
        );
        let listener = queue.subscribe();
        let wait = queue.request(request(), CancellationToken::new());
        let pending = queue.take_pending().unwrap();
        drop(pending);
        assert_eq!(wait.await, Response::Cancelled);
        let wait = queue.request(request(), CancellationToken::new());
        drop(listener);
        assert_eq!(wait.await, Response::Cancelled);
        assert!(!queue.has_pending());
    }
}
