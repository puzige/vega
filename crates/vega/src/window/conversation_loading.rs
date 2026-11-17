use super::*;
use std::time::{Duration, Instant};

mod host;
mod model;
mod view;

use crate::loading_logo as logo;

type ApplyConversation =
    Box<dyn FnOnce(&mut ConversationStream, &mut Context<ConversationStream>) + Send>;

pub(super) struct PreparedConversation {
    pub(super) pending_model: Option<String>,
    pub(super) apply: ApplyConversation,
}

#[derive(Default)]
pub(super) struct ConversationLoading {
    generation: u64,
    thread_id: Option<String>,
    started: Option<Instant>,
    worker_started: bool,
    result: Option<Result<PreparedConversation, ()>>,
    presentation: Option<Entity<host::NativeHost>>,
}

impl ConversationLoading {
    pub(super) fn clear(&mut self) {
        if self.thread_id.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
        self.started = None;
        self.worker_started = false;
        self.result = None;
    }

    pub(super) fn invalidate_route(&mut self, thread_id: Option<&str>) {
        if self.thread_id.as_deref() != thread_id {
            self.clear();
        }
    }

    pub(super) fn opened(&mut self, thread_id: &str, started: Instant) {
        if self.thread_id.as_deref() != Some(thread_id) {
            self.clear();
            self.thread_id = Some(thread_id.to_owned());
        }
        self.started = Some(started);
    }

    pub(super) fn take_ready(&mut self) -> Option<PreparedConversation> {
        self.result.take().and_then(Result::ok)
    }
}

impl VegaWindow {
    pub(super) fn conversation_ready(&mut self, thread_id: &str, cx: &mut Context<Self>) -> bool {
        if self.conversation_loading.thread_id.as_deref() != Some(thread_id)
            || !self.conversation_loading.worker_started
        {
            let started = (self.conversation_loading.thread_id.as_deref() == Some(thread_id))
                .then_some(self.conversation_loading.started)
                .flatten();
            self.start_conversation_load(thread_id.to_owned(), started, cx);
        }
        matches!(self.conversation_loading.result, Some(Ok(_)))
    }

    fn start_conversation_load(
        &mut self,
        thread_id: String,
        started: Option<Instant>,
        cx: &mut Context<Self>,
    ) {
        self.conversation_loading.clear();
        self.conversation_loading.thread_id = Some(thread_id.clone());
        self.conversation_loading.started = Some(started.unwrap_or_else(Instant::now));
        self.conversation_loading.worker_started = true;
        let generation = self.conversation_loading.generation;
        let path = self.file_backed_store_path(cx);
        let reasoning_path = self.reasoning_config_path();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_thread = thread_id.clone();
        let spawned = std::thread::Builder::new()
            .name("vega-conversation-load".into())
            .spawn(move || {
                let result = path.ok_or(()).and_then(|path| {
                    let started = Instant::now();
                    let initial = vega_conversation::threads::load_conversation(
                        &path,
                        &worker_thread,
                        &reasoning_path.ok_or(())?,
                    )
                    .map_err(|_| ())?;
                    tracing::debug!(
                        elapsed_ms = started.elapsed().as_millis(),
                        "conversation history read"
                    );
                    let started = Instant::now();
                    let (pending_model, apply) = ConversationStream::prepare_conversation(initial);
                    tracing::debug!(
                        elapsed_ms = started.elapsed().as_millis(),
                        "conversation body preparation"
                    );
                    Ok(PreparedConversation {
                        pending_model,
                        apply: Box::new(apply),
                    })
                });
                let _ = sender.send(result);
            });
        if spawned.is_err() {
            self.conversation_loading.result = Some(Err(()));
            cx.notify();
            return;
        }
        cx.spawn(async move |owner, cx| {
            loop {
                let result = match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Disconnected) => Some(Err(())),
                    Err(mpsc::TryRecvError::Empty) => None,
                };
                let finished = result.is_some();
                let current = owner.update(cx, |this, cx| {
                    let current = !cx.global::<SettingsOpen>().0
                        && cx
                            .global::<OpenedThread>()
                            .0
                            .as_ref()
                            .is_some_and(|thread| thread.id == thread_id)
                        && this.conversation_loading.thread_id.as_ref() == Some(&thread_id)
                        && this.conversation_loading.generation == generation;
                    if !current {
                        return false;
                    }
                    if let Some(result) = result {
                        this.conversation_loading.result = Some(result);
                    }
                    cx.notify();
                    true
                });
                if finished || !matches!(current, Ok(true)) {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(25))
                    .await;
            }
        })
        .detach();
    }

    fn loading_observation(&self, visit_started: Option<Instant>, cx: &App) -> model::Observation {
        model::Observation {
            thread_id: self.conversation_loading.thread_id.clone(),
            generation: self.conversation_loading.generation,
            started: self.conversation_loading.started,
            worker_started: self.conversation_loading.worker_started,
            result: match &self.conversation_loading.result {
                None => model::ResultPhase::Pending,
                Some(Ok(_)) => model::ResultPhase::Ready,
                Some(Err(())) => model::ResultPhase::Failed,
            },
            visit_started,
            opening: self.sidebar.read(cx).opening_thread(cx),
        }
    }

    fn loading_view_matches(&self, observed: &model::Observation, cx: &App) -> bool {
        if cx.global::<SettingsOpen>().0
            || self.loading_observation(observed.visit_started, cx) != *observed
        {
            return false;
        }
        if let Some((_, started)) = &observed.opening {
            return observed.visit_started == Some(*started);
        }
        let Some(thread_id) = observed.thread_id.as_deref() else {
            return false;
        };
        observed.visit_started.is_none()
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.id == thread_id)
            && !self.is_draft_route(thread_id)
            && self
                .agent_controller
                .active_stream_for_thread(thread_id)
                .is_none()
            && !self
                .agent_controller
                .preparation_stream
                .as_ref()
                .is_some_and(|stream| {
                    self.agent_controller
                        .ordinary_preparing
                        .contains_key(thread_id)
                        && stream.read(cx).thread_snapshot().id == thread_id
                })
            && self.composer_prediction_stream(thread_id).is_none()
            && !self.automation_controller.streams.contains_key(thread_id)
            && !self
                .stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == thread_id)
            && observed.result != model::ResultPhase::Ready
    }

    pub(super) fn render_conversation_loading(
        &mut self,
        visit_started: Option<Instant>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let observed = self.loading_observation(visit_started, cx);
        let host = match &self.conversation_loading.presentation {
            Some(host) => host.clone(),
            None => {
                let owner = cx.weak_entity();
                let observed = observed.clone();
                let host = cx.new(|cx| host::NativeHost::new(owner, observed, cx));
                self.conversation_loading.presentation = Some(host.clone());
                host
            }
        };
        host::Deferred { host, observed }.into_any_element()
    }
}
