use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use vega_store::context_compaction::{ContextCheckpoint, ContextSource};

const THREAD: &str = "synthetic-thread";
const MODEL: &str = "synthetic-model";
const LIVE: &str = "live";

struct Fixture {
    directory: PathBuf,
    path: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("vega-owner-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("fixture.db");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        store.conn().execute_batch(
            "INSERT INTO threads (id,title,model,created_at,updated_at) VALUES ('synthetic-thread','Synthetic','synthetic-model',1,1);
             INSERT INTO messages (id,thread_id,seq,role,content,status,created_at) VALUES
             ('user','synthetic-thread',1,'user','previous task','done',1),
             ('empty-before','synthetic-thread',2,'assistant','','interrupted',2),
             ('first','synthetic-thread',3,'assistant','前后尾','done',3),
             ('second','synthetic-thread',4,'assistant','继续完成','interrupted',4),
             ('empty-after','synthetic-thread',5,'assistant','','failed',5),
             ('latest','synthetic-thread',6,'user','continue current task','done',6),
             ('live','synthetic-thread',7,'assistant','','streaming',7);
             INSERT INTO tool_calls (id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,finished_at,text_offset_bytes) VALUES
             ('call-a','synthetic-thread','first',101,'read','{\"path\":\"a\"}','output a','success',1,2,3),
             ('call-b','synthetic-thread','first',102,'read','{\"path\":\"b\"}','output b','success',1,2,3),
             ('call-c','synthetic-thread','first',103,'read','{\"path\":\"c\"}','output c','failed',1,2,6),
             ('call-d','synthetic-thread','second',104,'read','{\"path\":\"d\"}','output d','cancelled',1,2,6);
             INSERT INTO tool_call_rounds (message_id,call_id,response_id) VALUES
             ('first','call-a','round-a'),('first','call-b','round-b'),
             ('first','call-c','round-c'),('second','call-d','round-d');"
        ).unwrap();
        Self {
            directory,
            path,
            store,
        }
    }

    fn source(&self) -> ContextSource {
        vega_store::context_compaction::load_source(self.store.conn(), THREAD).unwrap()
    }

    fn checkpoint(&self) -> Option<ContextCheckpoint> {
        vega_store::context_compaction::latest_checkpoint(self.store.conn(), THREAD, MODEL).unwrap()
    }

    fn partial_checkpoint(&self) {
        let source = self.source();
        let installed = vega_store::context_compaction::install_checkpoint(
            self.store.conn(),
            &NewContextCheckpoint {
                thread_id: THREAD.into(),
                model: MODEL.into(),
                source_version: source.source_version,
                source_fingerprint: source.fingerprint,
                covered_through_seq: 2,
                covered_assistant_id: Some("first".into()),
                covered_assistant_text_bytes: 3,
                covered_assistant_tool_seq: 101,
                summary: "Earlier synthetic work".into(),
                estimator_version: CONTEXT_ESTIMATOR_VERSION.into(),
                expected_previous_id: None,
                created_at: 20,
            },
        )
        .unwrap();
        assert!(matches!(installed, ContextCheckpointInstall::Applied(_)));
        self.store
            .conn()
            .execute_batch("UPDATE messages SET content='new streaming progress' WHERE id='live'")
            .unwrap();
    }

    fn history(&self) -> Vec<ChatMessage> {
        super::super::pipeline::primary_history_from_context_source_with_checkpoint(
            &self.source(),
            self.checkpoint().as_ref(),
            LIVE,
        )
        .unwrap()
    }

    fn request(&self, messages: Vec<ChatMessage>) -> ContextCompactionRequest {
        let source = self.source();
        let budget = ContextBudget::new(100_000, 10_000, true).unwrap();
        ContextCompactionRequest {
            estimate: estimate_chat_context("system", &messages, &[]).unwrap(),
            system_prompt: "system".into(),
            messages,
            tools: Vec::new(),
            budget,
            target_tokens: budget.target_tokens().unwrap(),
            source_version: source.source_version,
            source_fingerprint: Some(source.fingerprint),
            require_source_fence: false,
            source_owner_id: Some(LIVE.into()),
        }
    }

    fn normalize(
        &self,
        messages: &[ChatMessage],
    ) -> Result<Vec<ChatMessage>, ContextCompactionFailure> {
        let source = self.source();
        let checkpoint = self.checkpoint();
        let projected =
            super::super::pipeline::source_after_checkpoint(&source, checkpoint.as_ref()).unwrap();
        normalize_terminal_history_groups(
            &projected,
            checkpoint.map(|checkpoint| checkpoint.covered_through_seq),
            LIVE,
            messages,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn equal(left: &[ChatMessage], right: &[ChatMessage]) -> bool {
    durable_messages_equal(
        &left.iter().collect::<Vec<_>>(),
        &right.iter().collect::<Vec<_>>(),
    )
}

fn warm_history(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut warm = Vec::new();
    for mut message in messages {
        if message.role == ChatRole::Assistant && !message.tool_calls.is_empty() {
            if !message.content.is_empty() {
                warm.push(ChatMessage::new(
                    ChatRole::Assistant,
                    std::mem::take(&mut message.content),
                ));
            }
            message.reasoning_content = Some("run-memory reasoning".into());
        }
        warm.push(message);
    }
    warm
}

#[test]
fn cold_history_preserves_empty_terminal_records_and_multiple_owners() {
    let fixture = Fixture::new();
    let history = fixture.history();
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == ChatRole::Assistant
                && message.content.is_empty()
                && message.tool_calls.is_empty())
            .count(),
        2
    );
    let normalized = fixture.normalize(&history).unwrap();
    assert!(equal(&normalized, &history));
}

#[test]
fn warm_rounds_preserve_distinct_owners_and_same_offset_responses() {
    let fixture = Fixture::new();
    let history = fixture.history();
    assert_eq!(
        history
            .iter()
            .filter(|message| !message.tool_calls.is_empty())
            .count(),
        4
    );
    let warm = warm_history(history.clone());
    let normalized = fixture.normalize(&warm).unwrap();
    assert!(equal(&normalized, &history));
}

#[test]
fn partial_checkpoint_preserves_utf8_suffix_and_resumed_owner_boundaries() {
    let fixture = Fixture::new();
    fixture.partial_checkpoint();
    let history = fixture.history();
    assert!(
        history
            .iter()
            .all(|message| message.tool_call_id.as_deref() != Some("call-a"))
    );
    assert!(
        history
            .iter()
            .any(|message| message.tool_call_id.as_deref() == Some("call-b"))
    );
    let normalized = fixture.normalize(&warm_history(history.clone())).unwrap();
    assert!(equal(&normalized, &history));
}

#[test]
fn missing_or_extra_empty_records_are_not_silently_accepted() {
    let fixture = Fixture::new();
    let mut missing = fixture.history();
    missing.remove(1);
    assert!(fixture.normalize(&missing).is_err());
    let history = fixture.history();
    for index in 0..=history.len() {
        let mut extra = history.clone();
        extra.insert(index, ChatMessage::new(ChatRole::Assistant, ""));
        assert!(fixture.normalize(&extra).is_err(), "insertion {index}");
    }
}

#[test]
fn historical_text_arguments_results_order_and_pairing_changes_are_rejected() {
    let fixture = Fixture::new();
    let history = fixture.history();
    for mutation in 0..8 {
        let mut changed = history.clone();
        let call_index = changed
            .iter()
            .position(|message| !message.tool_calls.is_empty())
            .unwrap();
        match mutation {
            0 => changed[call_index].content = "改".into(),
            1 => changed[call_index].tool_calls[0].input_json = "{}".into(),
            2 => changed[call_index + 1].content = "different result".into(),
            3 => changed.swap(call_index + 1, call_index + 3),
            4 => {
                changed.remove(call_index + 1);
            }
            5 => changed[call_index].tool_calls[0].id = "foreign-call".into(),
            6 => changed.insert(call_index + 2, changed[call_index + 1].clone()),
            7 => {
                let tail = changed
                    .iter_mut()
                    .find(|message| message.content == "尾")
                    .unwrap();
                tail.content = "末".into();
            }
            _ => unreachable!(),
        }
        assert!(fixture.normalize(&changed).is_err(), "mutation {mutation}");
    }
}

#[test]
fn changed_durable_owner_offsets_and_pending_tools_are_rejected() {
    for sql in [
        "UPDATE tool_calls SET text_offset_bytes=0 WHERE id='call-a'",
        "UPDATE tool_calls SET message_id='second' WHERE id='call-a'",
        "UPDATE tool_calls SET status='running',output_text=NULL WHERE id='call-a'",
        "UPDATE messages SET status='unknown' WHERE id='first'",
    ] {
        let fixture = Fixture::new();
        let history = fixture.history();
        fixture.store.conn().execute_batch(sql).unwrap();
        assert!(fixture.normalize(&history).is_err());
    }
}

struct SummaryProvider {
    calls: AtomicUsize,
    mutate_path: Option<PathBuf>,
}

impl SummaryProvider {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            mutate_path: None,
        }
    }
}

impl Provider for SummaryProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(request.tools.is_empty());
        if let Some(path) = &self.mutate_path {
            let store = Store::open(path).unwrap();
            store.conn().execute_batch("UPDATE tool_calls SET output_text='changed while summarizing' WHERE id='call-a'").unwrap();
        }
        Box::pin(async {
            Ok(Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta("Synthetic work checkpoint".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ])) as vega_runtime::EventStream)
        })
    }
}

#[tokio::test]
async fn automatic_compaction_installs_and_reloads_checkpoint_with_resumed_history() {
    for partial in [false, true] {
        for warm in [false, true] {
            let fixture = Fixture::new();
            if partial {
                fixture.partial_checkpoint();
            }
            let source_before = fixture.source();
            let history = fixture.history();
            let history = if warm { warm_history(history) } else { history };
            let provider = SummaryProvider::new();
            let hook = ConversationCompactionHook::new(
                &provider,
                fixture.path.clone(),
                THREAD,
                MODEL,
                None,
                None,
            );
            let result = hook
                .compact(fixture.request(history), CancellationToken::new())
                .await
                .unwrap_or_else(|error| panic!("partial={partial}, warm={warm}: {error:?}"));
            assert!(provider.calls.load(Ordering::SeqCst) > 0);
            assert!(fixture.checkpoint().is_some());
            assert_eq!(fixture.source().fingerprint, source_before.fingerprint);
            assert!(equal(&result.messages, &fixture.history()));
            let reopened = Store::open(&fixture.path).unwrap();
            let checkpoint =
                vega_store::context_compaction::latest_checkpoint(reopened.conn(), THREAD, MODEL)
                    .unwrap()
                    .unwrap();
            assert_eq!(checkpoint.covered_through_seq, 5);
            assert_eq!(
                result.messages.last().unwrap().content,
                "continue current task"
            );
        }
    }
}

#[tokio::test]
async fn changed_history_is_rejected_before_summary_and_checkpoint_install() {
    let fixture = Fixture::new();
    let mut history = fixture.history();
    history
        .iter_mut()
        .find(|message| message.tool_call_id.as_deref() == Some("call-a"))
        .unwrap()
        .content = "changed".into();
    let provider = SummaryProvider::new();
    let hook =
        ConversationCompactionHook::new(&provider, fixture.path.clone(), THREAD, MODEL, None, None);
    let error = hook
        .compact(fixture.request(history), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error.error.as_ref(),
        VegaError::Context(ContextRuntimeError::SourceChanged)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert!(fixture.checkpoint().is_none());
}

#[tokio::test]
async fn model_and_manual_source_fences_still_reject_before_summary() {
    for wrong_model in [false, true] {
        let fixture = Fixture::new();
        let mut request = fixture.request(fixture.history());
        request.require_source_fence = true;
        request.source_fingerprint = Some("stale-fingerprint".into());
        let provider = SummaryProvider::new();
        let model = if wrong_model {
            "different-model"
        } else {
            MODEL
        };
        let hook = ConversationCompactionHook::new(
            &provider,
            fixture.path.clone(),
            THREAD,
            model,
            None,
            None,
        );
        let error = hook
            .compact(request, CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(
            error.error.as_ref(),
            VegaError::Context(ContextRuntimeError::SourceChanged)
        ));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(fixture.checkpoint().is_none());
    }
}

#[tokio::test]
async fn mutation_during_summary_cannot_install_checkpoint() {
    let fixture = Fixture::new();
    let provider = SummaryProvider {
        calls: AtomicUsize::new(0),
        mutate_path: Some(fixture.path.clone()),
    };
    let hook =
        ConversationCompactionHook::new(&provider, fixture.path.clone(), THREAD, MODEL, None, None);
    let error = hook
        .compact(fixture.request(fixture.history()), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error.error.as_ref(),
        VegaError::Context(ContextRuntimeError::SourceChanged)
    ));
    assert!(provider.calls.load(Ordering::SeqCst) > 0);
    assert!(fixture.checkpoint().is_none());
}
