use super::*;
use std::sync::Mutex;
use vega_runtime::model_compat::{
    ModelApi, ModelIdentity, ModelRoute, ResolvedModel, lookup_model_definition, resolve_model,
};
use vega_store::context_compaction::{
    ContextCheckpoint, ContextSource, ModelContextPolicy, latest_checkpoint, load_source,
    save_model_policy,
};
use vega_store::context_projection::{ContinuationMode, ProjectionFragment};

const THREAD: &str = "synthetic-manual-thread";
const MODEL: &str = "synthetic-manual-model";
const PROVIDER: &str = "synthetic-manual-provider";
const SYSTEM: &str = "Assist with the synthetic task.";
const SUMMARY: &str =
    "Synthetic checkpoint: inspected sources; work remains complete, not resumed.";

struct TestDirectory(PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct HistoryFixture {
    store: Store,
    directory: TestDirectory,
}

impl HistoryFixture {
    fn new() -> Self {
        let directory = TestDirectory(
            std::env::temp_dir().join(format!("vega-manual-history-{}", ulid::Ulid::generate())),
        );
        std::fs::create_dir(&directory.0).expect("create isolated synthetic directory");
        let store =
            Store::open(directory.0.join("synthetic.sqlite")).expect("open synthetic store");
        store.migrate().expect("install current migrations");
        vega_store::threads::create_standalone(
            store.conn(),
            vega_store::threads::NewThread {
                reasoning_preferences: None,
                id: THREAD,
                project_id: "",
                title: "Synthetic compaction history",
                mode: "ask",
                permission_mode: "confirm",
                permission_revision: 0,
                full_access_grant: None,
                model: MODEL,
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .expect("create synthetic thread");
        save_model_policy(
            store.conn(),
            &ModelContextPolicy {
                provider: PROVIDER.into(),
                model: MODEL.into(),
                input_limit: Some(300_000),
                output_reserve: Some(128_000),
                automatic_compaction: false,
                updated_at: 1,
            },
        )
        .expect("save valid capacity");
        Self { store, directory }
    }

    fn message(&self, id: &str, role: &str, content: &str, status: &str) {
        let seq =
            vega_store::messages::next_seq(self.store.conn(), THREAD).expect("message sequence");
        vega_store::messages::insert(
            self.store.conn(),
            &vega_store::messages::MessageRow {
                id: id.into(),
                thread_id: THREAD.into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: status.into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("insert synthetic message");
    }

    fn user(&self) {
        self.message(
            "request",
            "user",
            "Inspect the synthetic sources, then stop.",
            "done",
        );
    }

    fn owner(
        &self,
        id: &str,
        status: &str,
        round_sizes: &[usize],
        same_offset: bool,
    ) -> Vec<String> {
        self.message(id, "assistant", "", "streaming");
        let mut content = String::new();
        let mut call_ids = Vec::new();
        for (round, count) in round_sizes.iter().copied().enumerate() {
            let response_id = format!("{id}-response-{round}");
            if round == 0 || !same_offset {
                let start = content.len();
                content.push_str(
                    &format!("Synthetic source inspection α, response {round}.\n").repeat(80),
                );
                assert_eq!(
                    vega_store::messages::update_streaming_content(self.store.conn(), id, &content)
                        .expect("persist response text"),
                    1
                );
                assert!(
                    vega_store::messages::record_assistant_response(
                        self.store.conn(),
                        id,
                        &content,
                        &vega_store::messages::AssistantResponseRow {
                            message_id: id.into(),
                            response_id: response_id.clone(),
                            text_start_bytes: start,
                            text_end_bytes: content.len(),
                            kind: "commentary".into(),
                        },
                    )
                    .expect("record completed response text")
                );
            }
            for parallel in 0..count {
                let call_id = format!("{id}-round-{round}-call-{parallel}");
                let seq = vega_store::tool_calls::next_seq(self.store.conn(), THREAD)
                    .expect("tool sequence");
                vega_store::tool_calls::insert(
                    self.store.conn(),
                    vega_store::tool_calls::NewToolCall {
                        id: &call_id,
                        thread_id: THREAD,
                        message_id: id,
                        seq,
                        tool: "read",
                        input_json: r#"{"path":"synthetic.txt"}"#,
                        description: None,
                        status: "running",
                        created_at: seq,
                    },
                )
                .expect("insert synthetic call at current UTF-8 boundary");
                vega_store::tool_calls::finish(
                    self.store.conn(),
                    vega_store::tool_calls::FinishToolCall {
                        id: &call_id,
                        status: "success",
                        output_text: &format!("Synthetic result for {call_id}.\n").repeat(80),
                        exit_code: None,
                        duration_ms: None,
                        finished_at: seq + 1,
                    },
                )
                .expect("finish synthetic tool call");
                let transaction = self
                    .store
                    .immediate_transaction()
                    .expect("round transaction");
                transaction
                    .execute(
                        "INSERT INTO tool_call_rounds (message_id, call_id, response_id) VALUES (?1, ?2, ?3)",
                        (id, &call_id, &response_id),
                    )
                    .expect("persist response identity");
                transaction.commit().expect("commit response identity");
                call_ids.push(call_id);
            }
        }
        assert_eq!(
            vega_store::messages::finish(self.store.conn(), id, &content, status)
                .expect("finish assistant owner"),
            1
        );
        call_ids
    }

    fn projection(&self) -> crate::types::ContextProjection {
        read_context_projection_with_provider(&self.store, THREAD, MODEL, Some(PROVIDER), SYSTEM)
            .expect("read public context projection")
    }

    fn source(&self) -> ContextSource {
        load_source(self.store.conn(), THREAD).expect("load persisted synthetic source")
    }

    fn checkpoint(&self) -> ContextCheckpoint {
        latest_checkpoint(self.store.conn(), THREAD, MODEL)
            .expect("read installed checkpoint")
            .expect("manual path installed checkpoint")
    }

    fn history(&self) -> Vec<ChatMessage> {
        crate::agent::pipeline::primary_history_from_context_source_with_checkpoint(
            &self.source(),
            Some(&self.checkpoint()),
            "__manual_test__",
        )
        .expect("project persisted checkpoint")
    }

    async fn compact(&self, provider: &SummaryProvider) -> ContextCompactionResult {
        let before = self
            .projection()
            .estimated_tokens
            .expect("original estimate");
        let result = compact_thread_manually(
            &self.store,
            provider,
            THREAD,
            MODEL,
            SYSTEM,
            vega_runtime::tool_definitions(vega_runtime::RuntimeRunMode::Ask),
            ContextBudget::new(428_000, 128_000, false).expect("valid manual budget"),
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .unwrap_or_else(|failure| panic!("real owned manual compaction failed: {}", failure.error));
        assert!(
            self.projection()
                .estimated_tokens
                .expect("compacted estimate")
                < before
        );
        assert!(result.usage_complete);
        assert!(!result.usages.is_empty());
        result
    }
}

#[derive(Default)]
struct SummaryProvider {
    requests: Mutex<Vec<ChatRequest>>,
}

impl Provider for SummaryProvider {
    fn model_snapshot(&self) -> Option<ResolvedModel> {
        let route = ModelRoute::new(ModelApi::ChatCompletions, "https://synthetic.invalid/v1")
            .expect("synthetic model route");
        let mut definition = lookup_model_definition(
            &route,
            &ModelIdentity {
                provider: PROVIDER.into(),
                model: MODEL.into(),
            },
        )
        .expect("resolve synthetic model definition");
        definition.context_window = Some(428_000);
        definition.max_output_tokens = Some(128_000);
        definition.reasoning = Some(false);
        Some(resolve_model(&definition, None).expect("resolve synthetic model snapshot"))
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert!(!cancel.is_cancelled());
        assert!(request.tools.is_empty());
        assert_complete_pairs(&request.messages);
        self.requests
            .lock()
            .expect("capture summary request")
            .push(request);
        Box::pin(async {
            let events: vega_runtime::EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta(SUMMARY.into())),
                Ok(ProviderEvent::Usage {
                    input: 120,
                    output: 18,
                    cache_read: None,
                    cache_write: None,
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(events)
        })
    }
}

fn assert_complete_pairs(history: &[ChatMessage]) {
    let mut pending = HashSet::new();
    for message in history {
        if message.role == ChatRole::Tool {
            assert!(
                pending.remove(
                    message
                        .tool_call_id
                        .as_deref()
                        .expect("tool result identity")
                )
            );
        } else {
            assert!(
                pending.is_empty(),
                "response ended before all tool results arrived"
            );
            for call in &message.tool_calls {
                assert!(
                    pending.insert(call.id.as_str()),
                    "duplicate tool call identity"
                );
            }
        }
    }
    assert!(pending.is_empty(), "missing terminal tool result");
}

fn call_ids(history: &[ChatMessage]) -> Vec<String> {
    history
        .iter()
        .flat_map(|message| message.tool_calls.iter().map(|call| call.id.clone()))
        .collect()
}

fn assert_idle_plan(checkpoint: &ContextCheckpoint, source: &ContextSource) {
    let plan = checkpoint
        .projection_plan
        .as_ref()
        .expect("durable response plan");
    assert_eq!(plan.continuation, ContinuationMode::NoContinuation);
    assert!(plan.source_owner_id.is_none());
    assert!(
        !plan
            .fragments
            .iter()
            .any(|fragment| matches!(fragment, ProjectionFragment::TaskReference { .. }))
    );
    assert!(plan.omitted.is_empty());
    plan.validate_install(source)
        .expect("complete installed plan");
}

#[tokio::test]
async fn single_user_multi_response_history_is_eligible_and_accounted_manual_installs() {
    let fixture = HistoryFixture::new();
    fixture.user();
    let ids = fixture.owner("assistant", "done", &[1, 1, 1], false);
    let before = fixture.projection();
    assert!(before.compactable);
    assert_eq!(
        fixture
            .source()
            .messages
            .iter()
            .filter(|message| message.role == "user")
            .count(),
        1
    );
    let provider = SummaryProvider::default();
    let events = compact_thread_manually_accounted(
        &fixture.store,
        &provider,
        THREAD,
        MODEL,
        SYSTEM,
        CancellationToken::new(),
        None,
        None,
        7,
    )
    .await
    .expect("accounted manual service returns lifecycle events");
    let statuses = events
        .iter()
        .filter_map(|event| match event {
            crate::types::ConversationEvent::ContextCompactionStatus { record } => {
                Some(record.status)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        [
            crate::types::ContextCompactionStatus::Compacting,
            crate::types::ContextCompactionStatus::Succeeded
        ]
    );
    assert!(events.iter().any(|event| matches!(event,
        crate::types::ConversationEvent::ContextCompactionUsageUpdated { usage, .. }
            if usage.input == 120 && usage.output == 18
    )));
    let requests = provider.requests.lock().expect("summary requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(call_ids(&requests[0].messages), ids[..2]);
    let checkpoint = fixture.checkpoint();
    assert!(checkpoint.summary.contains(SUMMARY));
    assert_idle_plan(&checkpoint, &fixture.source());
    assert!(
        fixture
            .projection()
            .estimated_tokens
            .expect("compacted estimate")
            < before.estimated_tokens.expect("original estimate")
    );
    let history = fixture.history();
    assert_complete_pairs(&history);
    assert_eq!(call_ids(&history), ids[2..]);
    assert!(history.iter().any(ChatMessage::is_historical_summary));
    assert_eq!(
        fixture
            .projection()
            .last_status
            .expect("persisted success")
            .status,
        crate::types::ContextCompactionStatus::Succeeded
    );
}

#[tokio::test]
async fn failed_and_done_owners_compact_without_resuming_either_owner() {
    let fixture = HistoryFixture::new();
    fixture.user();
    let failed = fixture.owner("failed-owner", "failed", &[1, 1], false);
    let done = fixture.owner("done-owner", "done", &[1, 1], false);
    let before = fixture.source();
    assert!(fixture.projection().compactable);
    let provider = SummaryProvider::default();
    let result = fixture.compact(&provider).await;
    assert_idle_plan(&fixture.checkpoint(), &fixture.source());
    assert_eq!(fixture.source(), before);
    let requests = provider.requests.lock().expect("summary requests");
    assert_eq!(requests.len(), 1);
    let mut expected = failed;
    expected.push(done[0].clone());
    assert_eq!(call_ids(&requests[0].messages), expected);
    assert_eq!(call_ids(&result.messages), done[1..]);
    assert_complete_pairs(&result.messages);
    let reloaded = fixture.history();
    assert!(durable_messages_equal(
        &result.messages.iter().collect::<Vec<_>>(),
        &reloaded.iter().collect::<Vec<_>>()
    ));
}

#[tokio::test]
async fn empty_single_group_pending_streaming_and_incomplete_histories_never_summarize() {
    let fixture = HistoryFixture::new();
    let provider = SummaryProvider::default();
    let single_group = HistoryFixture::new();
    single_group.message(
        "orphan-assistant",
        "assistant",
        "One terminal group without a task.",
        "done",
    );
    assert!(!single_group.projection().compactable);
    assert!(
        compact_thread_manually_accounted(
            &single_group.store,
            &provider,
            THREAD,
            MODEL,
            SYSTEM,
            CancellationToken::new(),
            None,
            None,
            0,
        )
        .await
        .is_err()
    );
    assert!(!fixture.projection().compactable);
    fixture.user();
    assert!(!fixture.projection().compactable);
    assert!(
        compact_thread_manually_accounted(
            &fixture.store,
            &provider,
            THREAD,
            MODEL,
            SYSTEM,
            CancellationToken::new(),
            None,
            None,
            1
        )
        .await
        .is_err()
    );
    let ids = fixture.owner("incomplete-owner", "streaming", &[1, 1], false);
    assert!(!fixture.projection().compactable);
    assert!(
        compact_thread_manually_accounted(
            &fixture.store,
            &provider,
            THREAD,
            MODEL,
            SYSTEM,
            CancellationToken::new(),
            None,
            None,
            2
        )
        .await
        .is_err()
    );
    let owner = vega_store::messages::find(fixture.store.conn(), "incomplete-owner")
        .expect("owner row")
        .expect("owner exists");
    vega_store::messages::finish(fixture.store.conn(), &owner.id, &owner.content, "done")
        .expect("finish owner");
    assert_eq!(
        vega_store::tool_calls::update(fixture.store.conn(), &ids[1], "running", None, None, None)
            .expect("mark incomplete call"),
        1
    );
    assert!(
        read_context_projection_with_provider(
            &fixture.store,
            THREAD,
            MODEL,
            Some(PROVIDER),
            SYSTEM
        )
        .is_err()
    );
    assert!(
        compact_thread_manually_accounted(
            &fixture.store,
            &provider,
            THREAD,
            MODEL,
            SYSTEM,
            CancellationToken::new(),
            None,
            None,
            3
        )
        .await
        .is_err()
    );
    assert!(
        provider
            .requests
            .lock()
            .expect("no external requests")
            .is_empty()
    );
    assert!(
        latest_checkpoint(fixture.store.conn(), THREAD, MODEL)
            .expect("checkpoint read")
            .is_none()
    );
}

#[tokio::test]
async fn pending_user_is_retained_verbatim_outside_covered_summary_history() {
    let fixture = HistoryFixture::new();
    fixture.user();
    fixture.owner("completed-owner", "done", &[1, 1], false);
    let pending = "Pending correction: do not resume the completed work.";
    fixture.message("pending-request", "user", pending, "done");
    assert!(fixture.projection().compactable);
    let source = fixture.source();
    let planned = reactive::ResponsePlanner::new(&source, None, None)
        .and_then(|mut planner| planner.start_manual(&source))
        .unwrap_or_else(|failure| panic!("pending-user planning failed: {}", failure.error));
    assert!(
        !planned
            .summary_history
            .iter()
            .any(|message| message.content == pending)
    );
    assert_eq!(planned.plan.continuation, ContinuationMode::NoContinuation);
    let provider = SummaryProvider::default();
    let result = fixture.compact(&provider).await;
    assert_idle_plan(&fixture.checkpoint(), &fixture.source());
    assert_eq!(
        result.messages.last().expect("retained pending user").role,
        ChatRole::User
    );
    assert_eq!(
        result
            .messages
            .last()
            .expect("retained pending user")
            .content,
        pending
    );
    let requests = provider.requests.lock().expect("summary requests");
    assert!(
        !requests
            .iter()
            .flat_map(|request| &request.messages)
            .any(|message| message.role == ChatRole::User && message.content == pending)
    );
    assert!(
        requests
            .iter()
            .any(|request| request.messages.iter().any(|message| {
                message.role == ChatRole::User
                    && message.content.contains("historical task reference")
                    && message.content.contains(pending)
            }))
    );
}

#[tokio::test]
async fn parallel_calls_and_distinct_response_ids_at_one_utf8_offset_stay_paired() {
    let fixture = HistoryFixture::new();
    fixture.user();
    let ids = fixture.owner("parallel-owner", "done", &[2, 1, 2], true);
    let source = fixture.source();
    assert!(
        source
            .tool_calls
            .iter()
            .all(|call| call.text_offset_bytes == source.tool_calls[0].text_offset_bytes)
    );
    assert_eq!(
        source.tool_calls[0].response_id,
        source.tool_calls[1].response_id
    );
    assert_ne!(
        source.tool_calls[1].response_id,
        source.tool_calls[2].response_id
    );
    assert!(fixture.projection().compactable);
    let provider = SummaryProvider::default();
    let result = fixture.compact(&provider).await;
    let requests = provider.requests.lock().expect("summary requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(call_ids(&requests[0].messages), ids[..3]);
    assert_eq!(call_ids(&result.messages), ids[3..]);
    let retained = result
        .messages
        .iter()
        .find(|message| message.tool_calls.len() == 2)
        .expect("complete parallel tail group");
    assert_eq!(
        retained
            .tool_calls
            .iter()
            .map(|call| &call.id)
            .collect::<Vec<_>>(),
        ids[3..].iter().collect::<Vec<_>>()
    );
    assert_complete_pairs(&result.messages);
    assert_idle_plan(&fixture.checkpoint(), &source);
}

#[tokio::test]
async fn reopened_checkpoint_preserves_tail_and_can_plan_and_install_again() {
    let fixture = HistoryFixture::new();
    fixture.user();
    let first_ids = fixture.owner("first-owner", "done", &[1, 1, 1], false);
    let provider = SummaryProvider::default();
    let first = fixture.compact(&provider).await;
    let previous = fixture.checkpoint();
    let reopened = Store::open_existing(fixture.directory.0.join("synthetic.sqlite"))
        .expect("reopen synthetic store");
    let checkpoint = latest_checkpoint(reopened.conn(), THREAD, MODEL)
        .expect("reload checkpoint")
        .expect("durable checkpoint");
    assert_eq!(checkpoint.projection_plan, previous.projection_plan);
    assert!(
        read_context_projection_with_provider(&reopened, THREAD, MODEL, Some(PROVIDER), SYSTEM)
            .expect("read projection after reopening")
            .compactable
    );
    let source = load_source(reopened.conn(), THREAD).expect("reload source");
    let reloaded = crate::agent::pipeline::primary_history_from_context_source_with_checkpoint(
        &source,
        Some(&checkpoint),
        "__manual_test__",
    )
    .expect("reload active history");
    assert!(durable_messages_equal(
        &first.messages.iter().collect::<Vec<_>>(),
        &reloaded.iter().collect::<Vec<_>>()
    ));
    assert_eq!(call_ids(&reloaded), first_ids[2..]);
    let next_ids = fixture.owner("next-owner", "done", &[1, 1], false);
    assert!(fixture.projection().compactable);
    let planned = reactive::ResponsePlanner::new(&fixture.source(), Some(&checkpoint), None)
        .and_then(|mut planner| planner.start_manual(&fixture.source()))
        .unwrap_or_else(|failure| panic!("checkpoint-aware planning failed: {}", failure.error));
    assert_eq!(planned.plan.continuation, ContinuationMode::NoContinuation);
    assert!(
        planned
            .summary_history
            .iter()
            .any(ChatMessage::is_historical_summary)
    );
    assert_eq!(
        call_ids(&planned.summary_history),
        [first_ids[2].clone(), next_ids[0].clone()]
    );
    let second = fixture.compact(&provider).await;
    let installed = fixture.checkpoint();
    assert_ne!(installed.id, previous.id);
    assert_eq!(installed.expected_previous_id, Some(previous.id));
    assert_idle_plan(&installed, &fixture.source());
    let old_plan = previous
        .projection_plan
        .as_ref()
        .expect("previous response plan");
    let new_plan = installed
        .projection_plan
        .as_ref()
        .expect("new response plan");
    assert_eq!(
        new_plan
            .covered
            .first()
            .expect("preserved coverage start")
            .after,
        old_plan
            .covered
            .first()
            .expect("original coverage start")
            .after
    );
    assert!(new_plan.omitted.is_empty());
    assert_eq!(call_ids(&second.messages), next_ids[1..]);
    assert_complete_pairs(&second.messages);
    let final_history = fixture.history();
    assert!(durable_messages_equal(
        &second.messages.iter().collect::<Vec<_>>(),
        &final_history.iter().collect::<Vec<_>>()
    ));
}
