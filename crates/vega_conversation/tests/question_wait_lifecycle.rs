use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::{FutureExt, future::BoxFuture, stream};
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{
    ConversationRun, PERMISSION_TIMEOUT, PermissionHook, PersistenceActorConfig,
    RejectPermissionHook, UserQuestionQueue, run_thread_task_with_images_reasoning_and_mcp,
};
use vega_conversation::types::{
    ConversationEvent, PermissionDecision, PermissionRequest, Thread, ToolCallStatus,
    UserQuestionAnswers,
};
use vega_runtime::{
    ChatRequest, EventStream, Provider, ProviderEvent, RuntimeUserQuestionHook,
    RuntimeUserQuestionRequest, RuntimeUserQuestionResponse, StopReason,
    USER_QUESTION_BATCH_REJECTED, USER_QUESTION_CANCELLED, USER_QUESTION_TOOL,
    USER_QUESTION_UNAVAILABLE, UserQuestionInput, VegaError,
};
use vega_store::Store;

fn input() -> UserQuestionInput {
    UserQuestionInput::from_json(
        r#"{"questions":[{"id":"color","header":"Color","question":"Which color?","options":[{"label":"Blue","description":"Cool"},{"label":"Red","description":"Warm"}]}]}"#,
    )
    .unwrap()
}

fn request(id: &str) -> RuntimeUserQuestionRequest {
    RuntimeUserQuestionRequest {
        call_id: id.into(),
        input: input(),
    }
}

fn answers(skip: bool) -> UserQuestionAnswers {
    UserQuestionAnswers::from_json(
        if skip {
            r#"{"answers":{"color":{"answers":[]}}}"#
        } else {
            r#"{"answers":{"color":{"answers":["Blue"]}}}"#
        },
        &input(),
    )
    .unwrap()
}

#[derive(Clone)]
struct TimeSource(Arc<Mutex<Instant>>);

impl TimeSource {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }

    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }

    fn advance(&self, seconds: u64) {
        *self.0.lock().unwrap() += Duration::from_secs(seconds);
    }

    fn config(&self, queue: UserQuestionQueue) -> PersistenceActorConfig {
        let source = self.clone();
        PersistenceActorConfig::default()
            .with_user_questions(Arc::new(queue))
            .with_owner_credential_reader(Arc::new(|| Ok(Vec::new())))
            .with_work_time_source(Arc::new(move || source.now()))
    }
}

#[derive(Default)]
struct RecordingPermissionHook(AtomicUsize);

impl PermissionHook for RecordingPermissionHook {
    fn request(
        &self,
        request: PermissionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        RejectPermissionHook.request(request, cancel)
    }
}

struct Fixture {
    store: Store,
    thread: Thread,
    tools: vega_tools::Tools,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-question-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("vega.db")).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            root.to_str().unwrap(),
            "Question fixture",
            None,
        )
        .unwrap();
        let thread =
            vega_conversation::threads::create_thread(&store, &project.id, "test-model", "confirm")
                .unwrap();
        let tools = vega_tools::Tools::new(&root).unwrap();
        Self {
            store,
            thread,
            tools,
            root,
        }
    }

    fn stored_duration(&self, message: &str) -> u64 {
        let value = self
            .store
            .conn()
            .query_row(
                "SELECT execution_duration_ms FROM assistant_run_durations WHERE message_id = ?1",
                [message],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        u64::try_from(value).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct ScriptedProvider {
    scripts: Mutex<VecDeque<Vec<ProviderEvent>>>,
    requests: Mutex<Vec<ChatRequest>>,
    time: TimeSource,
    fail_last: bool,
}

impl ScriptedProvider {
    fn new(time: TimeSource, scripts: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            scripts: Mutex::new(scripts.into()),
            requests: Mutex::new(Vec::new()),
            time,
            fail_last: false,
        }
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.requests.lock().unwrap().push(request);
        self.time.advance(2);
        let events = self.scripts.lock().unwrap().pop_front();
        let fail = self.fail_last && events.is_none();
        async move {
            if fail {
                return Err(VegaError::Io(std::io::Error::other(
                    "synthetic provider failure",
                )));
            }
            let events = events.expect("unexpected provider request after the script ended");
            Ok(Box::pin(stream::iter(events.into_iter().map(Ok))) as EventStream)
        }
        .boxed()
    }
}

fn question(id: &str) -> Vec<ProviderEvent> {
    vec![
        ProviderEvent::ToolUse {
            id: id.into(),
            name: USER_QUESTION_TOOL.into(),
            input_json: input().to_json().unwrap(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ]
}

fn done() -> Vec<ProviderEvent> {
    vec![
        ProviderEvent::TextDelta("Continued after the answer".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ]
}

fn terminal_duration(events: &[ConversationEvent]) -> u64 {
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            ConversationEvent::MessageFinished {
                execution_duration_ms,
                ..
            }
            | ConversationEvent::Interrupted {
                execution_duration_ms,
                ..
            }
            | ConversationEvent::Error {
                execution_duration_ms,
                ..
            } => *execution_duration_ms,
            _ => None,
        })
        .expect("terminal event must carry active work duration")
}

#[derive(Clone, Copy)]
enum Resolution {
    Answer,
    Skip,
    Cancel,
    RemoveListener,
    ProviderFailure,
    SinkFailure,
}

async fn run_waiting_scenario(resolution: Resolution, question_count: usize) {
    let fixture = Fixture::new();
    let time = TimeSource::new();
    let queue = UserQuestionQueue::new();
    let mut listener = Some(queue.subscribe());
    let cancel = CancellationToken::new();
    let mut scripts: Vec<_> = (0..question_count)
        .map(|index| question(&format!("question-{index}")))
        .collect();
    if !matches!(resolution, Resolution::ProviderFailure) {
        scripts.push(done());
    }
    let mut provider = ScriptedProvider::new(time.clone(), scripts);
    provider.fail_last = matches!(resolution, Resolution::ProviderFailure);
    let observed = Arc::new(Mutex::new(Vec::new()));
    let sink_events = observed.clone();
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &fixture.store,
        &provider,
        &fixture.tools,
        &fixture.thread.id,
        "Ask before continuing",
        "Follow user answers",
        cancel.clone(),
        &RejectPermissionHook,
        move |event| {
            sink_events.lock().unwrap().push(event.clone());
            if matches!(resolution, Resolution::SinkFailure)
                && matches!(event, ConversationEvent::ToolCallFinished { .. })
            {
                Err(VegaError::Io(std::io::Error::other(
                    "synthetic sink failure",
                )))
            } else {
                Ok(())
            }
        },
        time.config(queue.clone()),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    );
    tokio::pin!(run);
    let mut stale = None;
    for index in 0..question_count {
        let pending = loop {
            tokio::select! {
                result = &mut run => panic!("run completed before question: {result:?}"),
                changed = listener.as_mut().unwrap().changed() => assert!(changed),
            }
            if let Some(pending) = queue.take_pending() {
                break pending;
            }
        };
        assert_eq!(pending.request().call_id, format!("question-{index}"));
        assert!(queue.has_pending());
        let clock = observed
            .lock()
            .unwrap()
            .iter()
            .find_map(|event| match event {
                ConversationEvent::RunStarted { work_clock, .. } => work_clock.clone(),
                _ => None,
            })
            .expect("the UI and persistence must receive the same run clock");
        assert_eq!(clock.elapsed(), Duration::from_secs((index as u64 + 1) * 2));
        time.advance(600);
        tokio::time::advance(PERMISSION_TIMEOUT + Duration::from_secs(1)).await;
        assert!(futures::poll!(&mut run).is_pending());
        assert_eq!(provider.request_count(), index + 1);
        assert_eq!(clock.elapsed(), Duration::from_secs((index as u64 + 1) * 2));
        assert!(pending.respond(UserQuestionAnswers::default()).is_err());
        time.advance(300);
        assert!(queue.has_pending());
        assert!(!pending.is_resolved());
        assert_eq!(clock.elapsed(), Duration::from_secs((index as u64 + 1) * 2));
        if let Some(previous) = stale.take() {
            drop(previous);
            assert!(queue.has_pending());
            assert!(futures::poll!(&mut run).is_pending());
        }
        match resolution {
            Resolution::Cancel => {
                cancel.cancel();
                assert!(!pending.respond(answers(false)).unwrap());
            }
            Resolution::RemoveListener => drop(listener.take()),
            _ => {
                let answer = answers(matches!(resolution, Resolution::Skip));
                assert!(pending.respond(answer.clone()).unwrap());
                assert!(!pending.respond(answer).unwrap());
            }
        }
        stale = Some(pending);
    }
    let result = run.await;
    assert!(!queue.has_pending());
    queue.cancel_active();
    queue.cancel_active();
    let observed = observed.lock().unwrap();
    let message_id = observed
        .iter()
        .find_map(|event| match event {
            ConversationEvent::MessageStarted { message_id, .. } => Some(message_id),
            _ => None,
        })
        .unwrap();
    let expected = match resolution {
        Resolution::Cancel | Resolution::RemoveListener | Resolution::SinkFailure => 2_000,
        _ => (question_count as u64 + 1) * 2_000,
    };
    assert_eq!(fixture.stored_duration(message_id), expected);
    assert_eq!(terminal_duration(&observed), expected);
    let history =
        vega_conversation::history::latest_history_page(&fixture.store, &fixture.thread.id, 100)
            .unwrap();
    assert!(history.entries.iter().any(|entry| matches!(entry, vega_conversation::history::HistoryEntry::AssistantText { message_id: id, execution_duration_ms: Some(duration), .. } if id == message_id && *duration == expected)));
    let row = vega_store::messages::find(fixture.store.conn(), message_id)
        .unwrap()
        .unwrap();
    match resolution {
        Resolution::Answer | Resolution::Skip => {
            let result = result.unwrap();
            assert!(!result.failed && !result.interrupted);
            assert_eq!(result.content, "Continued after the answer");
            assert_eq!(terminal_duration(&result.events), expected);
            for index in 0..question_count {
                let id = format!("question-{index}");
                let results: Vec<_> = result
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        ConversationEvent::ToolCallFinished { call_id, result }
                            if call_id == &id =>
                        {
                            Some(result)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].status, ToolCallStatus::Success);
                let next_request = &provider.requests.lock().unwrap()[index + 1];
                let replies: Vec<_> = next_request
                    .messages
                    .iter()
                    .filter(|message| message.tool_call_id.as_deref() == Some(&id))
                    .collect();
                assert_eq!(replies.len(), 1);
                assert_eq!(
                    UserQuestionAnswers::from_json(&replies[0].content, &input()).unwrap(),
                    answers(matches!(resolution, Resolution::Skip))
                );
            }
        }
        Resolution::Cancel | Resolution::RemoveListener => {
            assert_eq!(provider.request_count(), 1);
            assert_eq!(row.status, "interrupted");
            assert!(
                observed
                    .iter()
                    .any(|event| matches!(event, ConversationEvent::Interrupted { .. }))
            );
            assert!(
                !observed
                    .iter()
                    .any(|event| matches!(event, ConversationEvent::MessageFinished { .. }))
            );
            assert!(observed.iter().any(|event| matches!(event, ConversationEvent::ToolCallFinished { result, .. } if result.status == ToolCallStatus::Cancelled)));
        }
        Resolution::ProviderFailure | Resolution::SinkFailure => {
            assert!(result.is_err() || result.as_ref().is_ok_and(|run| run.failed));
            assert_eq!(row.status, "failed");
        }
    }
}

#[tokio::test(start_paused = true)]
async fn answers_resume_real_runs_and_save_only_work_time() {
    run_waiting_scenario(Resolution::Answer, 1).await;
}

#[tokio::test(start_paused = true)]
async fn explicit_skip_is_one_matching_result_and_excludes_wait() {
    run_waiting_scenario(Resolution::Skip, 1).await;
}

#[tokio::test(start_paused = true)]
async fn stop_wins_over_late_answer_without_fabricating_success() {
    run_waiting_scenario(Resolution::Cancel, 1).await;
}

#[tokio::test(start_paused = true)]
async fn removing_listener_interrupts_and_persists_work_time() {
    run_waiting_scenario(Resolution::RemoveListener, 1).await;
}

#[tokio::test(start_paused = true)]
async fn sequential_questions_ignore_old_cleanup_and_accumulate_wait_once() {
    run_waiting_scenario(Resolution::Answer, 2).await;
}

#[tokio::test(start_paused = true)]
async fn provider_failure_after_answer_saves_work_time() {
    run_waiting_scenario(Resolution::ProviderFailure, 1).await;
}

#[tokio::test(start_paused = true)]
async fn event_sink_failure_uses_active_duration_in_fallback_persistence() {
    run_waiting_scenario(Resolution::SinkFailure, 1).await;
}

#[tokio::test(start_paused = true)]
async fn independent_and_later_runs_have_unshared_waiting_clocks() {
    let waiting_fixture = Fixture::new();
    let independent = Fixture::new();
    let time = TimeSource::new();
    let queue = UserQuestionQueue::new();
    let mut listener = queue.subscribe();
    let provider = ScriptedProvider::new(time.clone(), vec![question("waiting"), done()]);
    let cancel = CancellationToken::new();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let sink = observed.clone();
    let mut waiting = Box::pin(run_thread_task_with_images_reasoning_and_mcp(
        &waiting_fixture.store,
        &provider,
        &waiting_fixture.tools,
        &waiting_fixture.thread.id,
        "Ask first",
        "Wait",
        cancel.clone(),
        &RejectPermissionHook,
        move |event| {
            sink.lock().unwrap().push(event.clone());
            Ok(())
        },
        time.config(queue.clone()),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    ));
    tokio::select! {
        result = &mut waiting => panic!("question never waited: {result:?}"),
        changed = listener.changed() => assert!(changed),
    }
    let pending = queue.take_pending().unwrap();
    time.advance(900);
    let free_provider = ScriptedProvider::new(time.clone(), vec![done(), done()]);
    for _ in 0..2 {
        let run = run_thread_task_with_images_reasoning_and_mcp(
            &independent.store,
            &free_provider,
            &independent.tools,
            &independent.thread.id,
            "Independent work",
            "Finish",
            CancellationToken::new(),
            &RejectPermissionHook,
            |_| Ok(()),
            time.config(UserQuestionQueue::new()),
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
        )
        .await
        .unwrap();
        assert!(!run.failed && !run.interrupted);
        assert_eq!(run.content, "Continued after the answer");
        assert_eq!(terminal_duration(&run.events), 2_000);
        assert_eq!(
            independent.stored_duration(&run.assistant_message_id),
            2_000
        );
        assert!(queue.has_pending());
        assert_eq!(provider.request_count(), 1);
        assert!(futures::poll!(&mut waiting).is_pending());
    }
    let clock = observed
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            ConversationEvent::RunStarted { work_clock, .. } => work_clock.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(clock.elapsed(), Duration::from_secs(2));
    assert!(pending.respond(answers(false)).unwrap());
    let run = waiting.as_mut().await.unwrap();
    assert_eq!(terminal_duration(&run.events), 4_000);
    assert_eq!(
        waiting_fixture.stored_duration(&run.assistant_message_id),
        4_000
    );
    assert!(!queue.has_pending());
    time.advance(120);
    let later_provider =
        ScriptedProvider::new(time.clone(), vec![question("waiting-later"), done()]);
    let later_cancel = CancellationToken::new();
    let later_observed = Arc::new(Mutex::new(Vec::new()));
    let later_sink = later_observed.clone();
    let later = run_thread_task_with_images_reasoning_and_mcp(
        &waiting_fixture.store,
        &later_provider,
        &waiting_fixture.tools,
        &waiting_fixture.thread.id,
        "Ask again on the same task",
        "Wait for a fresh answer",
        later_cancel.clone(),
        &RejectPermissionHook,
        move |event| {
            later_sink.lock().unwrap().push(event.clone());
            Ok(())
        },
        time.config(queue.clone()),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    );
    tokio::pin!(later);
    let current = loop {
        tokio::select! {
            result = &mut later => panic!("later run did not wait: {result:?}"),
            changed = listener.changed() => assert!(changed),
        }
        if let Some(current) = queue.take_pending() {
            break current;
        }
    };
    assert_eq!(current.request().call_id, "waiting-later");
    assert_ne!(current.request().call_id, pending.request().call_id);
    assert!(pending.is_resolved());
    assert!(!current.is_resolved());
    let later_clock = later_observed
        .lock()
        .unwrap()
        .iter()
        .find_map(|event| match event {
            ConversationEvent::RunStarted { work_clock, .. } => work_clock.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(later_clock.elapsed(), Duration::from_secs(2));
    assert!(!pending.respond(answers(true)).unwrap());
    cancel.cancel();
    drop(pending);
    drop(waiting);
    time.advance(900);
    assert!(!later_cancel.is_cancelled());
    assert!(queue.has_pending());
    assert!(!current.is_resolved());
    assert!(futures::poll!(&mut later).is_pending());
    assert_eq!(later_provider.request_count(), 1);
    assert_eq!(later_clock.elapsed(), Duration::from_secs(2));
    assert_eq!(clock.elapsed(), Duration::from_secs(4));
    assert!(current.respond(answers(true)).unwrap());
    assert!(!current.respond(answers(false)).unwrap());
    let later_run = later.await.unwrap();
    assert!(!later_run.failed && !later_run.interrupted);
    assert!(!queue.has_pending());
    assert_ne!(later_run.assistant_message_id, run.assistant_message_id);
    assert_eq!(later_provider.request_count(), 2);
    assert_eq!(terminal_duration(&later_run.events), 4_000);
    assert_eq!(
        waiting_fixture.stored_duration(&later_run.assistant_message_id),
        4_000
    );
    let results: Vec<_> = later_run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result }
                if call_id == "waiting-later" =>
            {
                Some(result)
            }
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, ToolCallStatus::Success);
    assert_eq!(
        UserQuestionAnswers::from_json(&results[0].output, &input()).unwrap(),
        answers(true)
    );
}

#[tokio::test(start_paused = true)]
async fn steering_preserves_one_work_clock_and_wall_origin_across_assistant_messages() {
    let fixture = Fixture::new();
    let time = TimeSource::new();
    time.advance(120);
    let queue = UserQuestionQueue::new();
    let mut listener = queue.subscribe();
    let cancel = CancellationToken::new();
    let source = vega_conversation::pending_messages::QueueSteeringSource::new(
        fixture.root.join("vega.db"),
        fixture.root.clone(),
        fixture.thread.id.clone(),
        "steering-run".into(),
        Arc::new(|| Ok(Vec::new())),
        cancel.clone(),
    );
    let provider = ScriptedProvider::new(
        time.clone(),
        vec![question("before-steer"), question("after-steer"), done()],
    );
    let observed = Arc::new(Mutex::new(Vec::new()));
    let sink = observed.clone();
    let wall_start = Instant::now();
    let run = run_thread_task_with_images_reasoning_and_mcp(
        &fixture.store,
        &provider,
        &fixture.tools,
        &fixture.thread.id,
        "Ask first",
        "Keep working after steering",
        cancel,
        &RejectPermissionHook,
        move |event| {
            sink.lock().unwrap().push(event.clone());
            Ok(())
        },
        time.config(queue.clone())
            .with_pending_input(None, Some(Arc::new(source))),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    );
    tokio::pin!(run);
    for index in 0..2 {
        let pending = loop {
            tokio::select! {
                result = &mut run => panic!("run finished before its question: {result:?}"),
                changed = listener.changed() => assert!(changed),
            }
            if let Some(pending) = queue.take_pending() {
                break pending;
            }
        };
        assert_eq!(
            pending.request().call_id,
            if index == 0 {
                "before-steer"
            } else {
                "after-steer"
            }
        );
        let starts = observed
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                ConversationEvent::RunStarted {
                    message_id,
                    started_at,
                    work_clock,
                } => Some((message_id.clone(), *started_at, work_clock.clone().unwrap())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(starts.len(), index + 1);
        if index == 1 {
            assert_ne!(starts[0].0, starts[1].0);
            assert_eq!(starts[0].1, starts[1].1);
        }
        time.advance(900);
        for (_, started_at, clock) in &starts {
            assert!(*started_at >= wall_start && *started_at <= Instant::now());
            assert_eq!(clock.elapsed(), Duration::from_secs((index as u64 + 1) * 2));
        }
        assert!(futures::poll!(&mut run).is_pending());
        assert_eq!(provider.request_count(), index + 1);
        if index == 0 {
            vega_conversation::pending_messages::enqueue(
                &fixture.store,
                &vega_conversation::types::PendingMessage {
                    id: ulid::Ulid::generate().to_string(),
                    thread_id: fixture.thread.id.clone(),
                    generation: "steering-run".into(),
                    behavior: vega_conversation::types::PendingMessageBehavior::Steer,
                    content: "Ask again before finishing".into(),
                    images: Vec::new(),
                },
            )
            .unwrap();
        }
        assert!(pending.respond(answers(false)).unwrap());
    }
    let result = run.await.unwrap();
    assert!(!result.failed && !result.interrupted);
    assert!(!queue.has_pending());
    assert_eq!(terminal_duration(&result.events), 6_000);
    assert_eq!(fixture.stored_duration(&result.assistant_message_id), 6_000);
    let starts = observed
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::RunStarted { work_clock, .. } => work_clock.clone(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 2);
    assert!(
        starts
            .iter()
            .all(|clock| clock.elapsed() == Duration::from_secs(6))
    );
}

#[tokio::test(start_paused = true)]
async fn replacing_listener_cancels_old_request_without_detaching_new_owner() {
    let queue = UserQuestionQueue::new();
    let old_listener = queue.subscribe();
    let old = queue.request(request("reused-id"), CancellationToken::new());
    let old_pending = queue.take_pending().unwrap();
    let mut current_listener = queue.subscribe();
    assert_eq!(old.await, RuntimeUserQuestionResponse::Cancelled);
    assert!(!old_pending.respond(answers(false)).unwrap());
    let current = queue.request(request("reused-id"), CancellationToken::new());
    assert!(current_listener.changed().await);
    let current_pending = queue.take_pending().unwrap();
    drop(old_listener);
    drop(old_pending);
    assert!(queue.has_pending());
    assert!(current_pending.respond(answers(true)).unwrap());
    assert_eq!(
        current.await,
        RuntimeUserQuestionResponse::Answered(answers(true))
    );
    assert!(!queue.has_pending());
}

#[tokio::test(start_paused = true)]
async fn queue_notifies_resolution_and_cancel_without_a_tool_result_or_repaint() {
    for cancel_request in [false, true] {
        let queue = UserQuestionQueue::new();
        let mut listener = queue.subscribe();
        let cancel = CancellationToken::new();
        let future = queue.request(request("first"), cancel.clone());
        assert!(listener.changed().await);
        let pending = queue.take_pending().unwrap();
        if cancel_request {
            cancel.cancel();
        } else {
            assert!(pending.respond(answers(false)).unwrap());
        }
        let response = future.await;
        assert_eq!(
            response,
            if cancel_request {
                RuntimeUserQuestionResponse::Cancelled
            } else {
                RuntimeUserQuestionResponse::Answered(answers(false))
            }
        );
        assert!(!queue.has_pending());
        assert!(
            tokio::time::timeout(Duration::from_secs(1), listener.changed())
                .await
                .expect("resolved request must wake its UI owner")
        );
        queue.cancel_active();
        assert!(!pending.respond(answers(true)).unwrap());
    }
}

#[tokio::test(start_paused = true)]
async fn answer_winning_before_cancel_is_delivered_once_and_does_not_cancel_next_request() {
    let queue = UserQuestionQueue::new();
    let mut listener = queue.subscribe();
    let cancel = CancellationToken::new();
    let first = queue.request(request("first"), cancel.clone());
    assert!(listener.changed().await);
    let pending = queue.take_pending().unwrap();
    assert!(pending.respond(answers(false)).unwrap());
    cancel.cancel();
    assert_eq!(
        first.await,
        RuntimeUserQuestionResponse::Answered(answers(false))
    );
    assert!(!pending.respond(answers(true)).unwrap());
    let second = queue.request(request("second"), CancellationToken::new());
    let current = queue.take_pending().unwrap();
    drop(pending);
    assert!(queue.has_pending());
    assert!(current.respond(answers(true)).unwrap());
    assert_eq!(
        second.await,
        RuntimeUserQuestionResponse::Answered(answers(true))
    );
}

#[tokio::test(start_paused = true)]
async fn dropped_active_future_notifies_cleanup_and_does_not_leave_a_latch() {
    let queue = UserQuestionQueue::new();
    let mut listener = queue.subscribe();
    let waiting = queue.request(request("dropped"), CancellationToken::new());
    assert!(listener.changed().await);
    let pending = queue.take_pending().unwrap();
    drop(waiting);
    assert!(!queue.has_pending());
    assert!(pending.is_resolved());
    assert!(!pending.respond(answers(false)).unwrap());
    assert!(
        tokio::time::timeout(Duration::from_secs(1), listener.changed())
            .await
            .expect("dropping the hook future must notify UI cleanup")
    );
}

#[tokio::test(start_paused = true)]
async fn dropped_wait_future_releases_only_its_request() {
    let queue = UserQuestionQueue::new();
    let mut listener = queue.subscribe();
    let old = queue.request(request("old"), CancellationToken::new());
    assert!(listener.changed().await);
    let old_pending = queue.take_pending().unwrap();
    let current = queue.request(request("current"), CancellationToken::new());
    assert!(listener.changed().await);
    let current_pending = queue.take_pending().unwrap();
    drop(old);
    drop(old_pending);
    assert!(queue.has_pending());
    assert!(!current_pending.is_resolved());
    assert!(current_pending.respond(answers(true)).unwrap());
    assert_eq!(
        current.await,
        RuntimeUserQuestionResponse::Answered(answers(true))
    );
    assert!(!queue.has_pending());
}

#[tokio::test(start_paused = true)]
async fn invalid_or_unavailable_queue_never_latches_waiting() {
    let queue = UserQuestionQueue::new();
    assert_eq!(
        queue
            .request(request("no-listener"), CancellationToken::new())
            .await,
        RuntimeUserQuestionResponse::Unavailable
    );
    assert!(!queue.has_pending());
    let _listener = queue.subscribe();
    let mut invalid = request("invalid");
    invalid.input.questions.clear();
    assert_eq!(
        queue.request(invalid, CancellationToken::new()).await,
        RuntimeUserQuestionResponse::Unavailable
    );
    assert!(!queue.has_pending());
    let waiting = queue.request(request("valid"), CancellationToken::new());
    let pending = queue.take_pending().unwrap();
    assert!(pending.respond(UserQuestionAnswers::default()).is_err());
    assert!(queue.has_pending());
    assert!(pending.respond(answers(true)).unwrap());
    assert_eq!(
        waiting.await,
        RuntimeUserQuestionResponse::Answered(answers(true))
    );
}

#[tokio::test]
async fn invalid_unavailable_and_mixed_batches_remain_recoverable_without_follow_on_actions() {
    for variant in 0..3 {
        let fixture = Fixture::new();
        let time = TimeSource::new();
        let queue = UserQuestionQueue::new();
        let _listener = (variant != 1).then(|| queue.subscribe());
        let mut first = question("rejected-question");
        if variant == 0 {
            if let ProviderEvent::ToolUse { input_json, .. } = &mut first[0] {
                *input_json = r#"{"questions":[]}"#.into();
            }
        } else if variant == 2 {
            first.insert(
                1,
                ProviderEvent::ToolUse {
                    id: "forbidden-follow-on".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"must-not-exist","content":"not authorized"}"#.into(),
                },
            );
        }
        let provider = ScriptedProvider::new(time.clone(), vec![first, done()]);
        let permissions = RecordingPermissionHook::default();
        let run: ConversationRun = run_thread_task_with_images_reasoning_and_mcp(
            &fixture.store,
            &provider,
            &fixture.tools,
            &fixture.thread.id,
            "Ask first",
            "Respect question isolation",
            CancellationToken::new(),
            &permissions,
            |_| Ok(()),
            time.config(queue.clone()),
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
        )
        .await
        .unwrap();
        assert!(!run.failed && !run.interrupted);
        assert!(!queue.has_pending());
        assert!(!fixture.root.join("must-not-exist").exists());
        assert_eq!(terminal_duration(&run.events), 4_000);
        assert_eq!(fixture.stored_duration(&run.assistant_message_id), 4_000);
        let requests = provider.requests.lock().unwrap();
        let reply = requests[1]
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("rejected-question"))
            .unwrap();
        match variant {
            1 => assert_eq!(reply.content, USER_QUESTION_UNAVAILABLE),
            2 => {
                assert_eq!(reply.content, USER_QUESTION_BATCH_REJECTED);
                for id in ["rejected-question", "forbidden-follow-on"] {
                    let replies: Vec<_> = requests[1]
                        .messages
                        .iter()
                        .filter(|message| message.tool_call_id.as_deref() == Some(id))
                        .collect();
                    assert_eq!(replies.len(), 1);
                    assert_eq!(replies[0].content, USER_QUESTION_BATCH_REJECTED);
                    let results: Vec<_> = run
                        .events
                        .iter()
                        .filter_map(|event| match event {
                            ConversationEvent::ToolCallFinished { call_id, result }
                                if call_id == id =>
                            {
                                Some(result)
                            }
                            _ => None,
                        })
                        .collect();
                    assert_eq!(results.len(), 1);
                    assert_eq!(results[0].status, ToolCallStatus::Rejected);
                    assert_eq!(results[0].output, USER_QUESTION_BATCH_REJECTED);
                }
                assert!(!run.events.iter().any(|event| matches!(
                    event,
                    ConversationEvent::ToolCallApproved { call_id, .. }
                        | ConversationEvent::ToolCallRunning { call_id }
                        if call_id == "forbidden-follow-on"
                )));
                assert_eq!(permissions.0.load(Ordering::Relaxed), 0);
            }
            _ => assert!(reply.content.starts_with("Tool error:")),
        }
        assert_ne!(reply.content, USER_QUESTION_CANCELLED);
    }
}
