use std::path::PathBuf;

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    ChatMessage, ChatRequest, ChatRole, ChatToolCall, ContextBudget, ContextCompactionHook,
    ContextCompactionRequest, ContextCompactionTrigger, ContextRuntimeError, EventStream,
    MICRO_KEEP_RECENT, MICRO_MIN_SAVINGS, MicroCompactionRequest, MicroCompactionResult,
    MicroCompactionTarget, Provider, ReasoningReplayPolicy, ReplayFrame, RuntimeRunMode, VegaError,
    apply_micro_compaction, estimate_wire_context, micro_result_content, tool_definitions,
};
use vega_store::context_compaction::ContextSource;
use vega_store::{Store, messages, threads, tool_calls};

use super::ConversationCompactionHook;

const THREAD: &str = "micro-thread";
const MODEL: &str = "micro-model";
const OWNER: &str = "live-assistant";
const LIVE_CALL: &str = "live-read";
const LIVE_INPUT: &str = r#"{"path":"live.txt","offset":null,"limit":null}"#;
const LIVE_OUTPUT: &str = "Live result";
const SYSTEM: &str = "Inspect the available files.";
const OLD_CALLS: usize = 12;
const RECENT_CALLS: usize = 5;

struct NoProvider;

impl Provider for NoProvider {
    fn chat_stream(
        &self,
        _request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        panic!("micro compaction must not contact a provider")
    }
}

struct Fixture {
    directory: PathBuf,
    database: PathBuf,
}

impl Fixture {
    fn new(result_bytes: usize) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "vega-conversation-micro-{}",
            ulid::Ulid::generate()
        ));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Self {
            database: directory.join("conversation.sqlite"),
            directory,
        };
        let store = Store::open(&fixture.database).unwrap();
        store.migrate().unwrap();
        threads::create_standalone(
            store.conn(),
            threads::NewThread {
                reasoning_preferences: None,
                id: THREAD,
                project_id: "",
                title: "Micro compaction fixture",
                mode: "execute",
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
        .unwrap();
        insert_message(&store, "initial-user", 1, "user", "Original task", "done");

        let mut calls = (0..OLD_CALLS)
            .map(|index| {
                (
                    format!("old-{index:02}"),
                    "read",
                    if index == 1 { "failed" } else { "success" },
                )
            })
            .collect::<Vec<_>>();
        calls.push(("ineligible-write".into(), "write", "success"));
        calls.push(("rejected-read".into(), "read", "rejected"));
        calls.extend(
            (0..RECENT_CALLS).map(|index| (format!("recent-{index:02}"), "read", "success")),
        );
        for (index, (id, tool, status)) in calls.iter().enumerate() {
            let assistant_id = format!("assistant-{index:02}");
            insert_message(
                &store,
                &assistant_id,
                index as i64 + 2,
                "assistant",
                "Earlier investigation",
                "done",
            );
            tool_calls::insert(
                store.conn(),
                tool_calls::NewToolCall {
                    id,
                    thread_id: THREAD,
                    message_id: &assistant_id,
                    seq: index as i64 + 1,
                    tool,
                    input_json: r#"{"path":"fixture.txt","offset":null,"limit":null}"#,
                    description: None,
                    status,
                    created_at: 2,
                },
            )
            .unwrap();
            let output = format!("{id}:{}", "x".repeat(result_bytes));
            assert_eq!(
                tool_calls::update(store.conn(), id, status, None, Some(&output), Some(3)).unwrap(),
                1
            );
        }
        insert_message(
            &store,
            "current-user",
            calls.len() as i64 + 2,
            "user",
            "Continue the current task",
            "done",
        );
        insert_message(
            &store,
            OWNER,
            calls.len() as i64 + 3,
            "assistant",
            "Inspecting the current file",
            "streaming",
        );
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: LIVE_CALL,
                thread_id: THREAD,
                message_id: OWNER,
                seq: calls.len() as i64 + 1,
                tool: "read",
                input_json: LIVE_INPUT,
                description: None,
                status: "success",
                created_at: 4,
            },
        )
        .unwrap();
        assert_eq!(
            tool_calls::update(
                store.conn(),
                LIVE_CALL,
                "success",
                None,
                Some(LIVE_OUTPUT),
                Some(5),
            )
            .unwrap(),
            1
        );
        fixture
    }

    fn source(&self) -> ContextSource {
        let store = Store::open(&self.database).unwrap();
        let transaction = store.conn().unchecked_transaction().unwrap();
        let source =
            vega_store::context_compaction::load_source_in_transaction(&transaction, THREAD)
                .unwrap();
        transaction.commit().unwrap();
        source
    }

    fn markers(&self) -> Vec<(String, String)> {
        let store = Store::open(&self.database).unwrap();
        let mut statement = store
            .conn()
            .prepare(
                "SELECT call_id, source_digest FROM context_micro_compactions WHERE thread_id = ?1 ORDER BY call_id",
            )
            .unwrap();
        statement
            .query_map([THREAD], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn request(&self, budget: ContextBudget, predicted_input: u64) -> MicroCompactionRequest {
        let source = self.source();
        let mut messages = crate::agent::pipeline::history_from_context_source_with_checkpoint(
            &source, None, OWNER,
        )
        .unwrap();
        let replay_frame = Some(ReplayFrame::new(
            messages.len() + 1,
            ReasoningReplayPolicy::StandardChat,
        ));
        messages.push(ChatMessage::assistant_with_tools(
            "Inspecting the current file",
            vec![ChatToolCall {
                id: LIVE_CALL.into(),
                name: "read".into(),
                input_json: LIVE_INPUT.into(),
            }],
        ));
        messages.push(ChatMessage::tool_result(LIVE_CALL, LIVE_OUTPUT));
        let mut request = MicroCompactionRequest {
            context: ContextCompactionRequest {
                trigger: ContextCompactionTrigger::Threshold,
                system_prompt: SYSTEM.into(),
                messages,
                tools: tool_definitions(RuntimeRunMode::Ask),
                budget,
                estimate: vega_runtime::ContextEstimate {
                    input_tokens: predicted_input,
                    message_count: 0,
                    tool_count: 0,
                    image_count: 0,
                },
                target_tokens: budget.target_tokens().unwrap(),
                source_version: source.source_version,
                source_fingerprint: Some(source.fingerprint),
                require_source_fence: false,
                source_owner_id: Some(OWNER.into()),
            },
            replay_frame,
        };
        let mut estimate =
            estimate_wire_context(&request.wire_messages(), &request.context.tools).unwrap();
        estimate.input_tokens = predicted_input;
        request.context.estimate = estimate;
        request
    }

    async fn compact(
        &self,
        request: MicroCompactionRequest,
        model: &str,
    ) -> Result<Option<MicroCompactionResult>, VegaError> {
        let provider = NoProvider;
        let hook = ConversationCompactionHook::new(
            &provider,
            self.database.clone(),
            THREAD,
            model,
            None,
            None,
        );
        hook.micro_compact(request, CancellationToken::new()).await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn insert_message(store: &Store, id: &str, seq: i64, role: &str, content: &str, status: &str) {
    messages::insert(
        store.conn(),
        &messages::MessageRow {
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
    .unwrap();
}

fn expected_targets() -> Vec<MicroCompactionTarget> {
    let mut targets = (0..OLD_CALLS)
        .map(|index| MicroCompactionTarget {
            call_id: format!("old-{index:02}"),
            status: if index == 1 { "failed" } else { "success" }.into(),
        })
        .collect::<Vec<_>>();
    targets.push(MicroCompactionTarget {
        call_id: "recent-00".into(),
        status: "success".into(),
    });
    targets
}

fn wire_savings(request: &MicroCompactionRequest, targets: &[MicroCompactionTarget]) -> u64 {
    let mut wire = request.wire_messages();
    let before = estimate_wire_context(&wire, &request.context.tools)
        .unwrap()
        .input_tokens;
    apply_micro_compaction(&mut wire[1..], targets).unwrap();
    let after = estimate_wire_context(&wire, &request.context.tools)
        .unwrap()
        .input_tokens;
    before.checked_sub(after).unwrap()
}

fn assert_no_install(fixture: &Fixture, source: &ContextSource) {
    assert!(fixture.markers().is_empty());
    assert_eq!(&fixture.source(), source);
}

#[tokio::test]
async fn calibrated_threshold_compacts_and_reloads_real_old_results() {
    assert_eq!(MICRO_KEEP_RECENT, RECENT_CALLS);
    let fixture = Fixture::new(12_000);
    let budget = ContextBudget::new(204_096, 4_096, true).unwrap();
    let threshold = budget.precompute_tokens().unwrap();
    let before = fixture.source();
    let below = fixture.request(budget, threshold - 1);
    let raw = estimate_wire_context(&below.wire_messages(), &below.context.tools)
        .unwrap()
        .input_tokens;
    assert!(raw < threshold);
    let targets = expected_targets();
    assert!(below.context.messages.iter().any(|message| {
        message.role == ChatRole::Tool && message.tool_call_id.as_deref() == Some(LIVE_CALL)
    }));
    let expected_savings = wire_savings(&below, &targets);
    assert!(expected_savings >= MICRO_MIN_SAVINGS);
    assert!(fixture.compact(below, MODEL).await.unwrap().is_none());
    assert_no_install(&fixture, &before);

    let request = fixture.request(budget, threshold);
    assert_eq!(request.context.estimate.input_tokens, threshold);
    let result = fixture
        .compact(request.clone(), MODEL)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.targets, targets);
    assert_eq!(result.estimated_savings, expected_savings);
    assert_eq!(result.source_version, before.source_version);

    let after = fixture.source();
    assert_eq!(result.source_fingerprint, after.fingerprint);
    assert_ne!(before.fingerprint, after.fingerprint);
    assert_eq!(after.source_version, before.source_version);
    assert_eq!(after.messages, before.messages);
    assert_eq!(after.images, before.images);
    let mut expected_calls = before.tool_calls.clone();
    for call in &mut expected_calls {
        call.micro_compacted = targets.iter().any(|target| target.call_id == call.id);
    }
    assert_eq!(after.tool_calls, expected_calls);

    let markers = fixture.markers();
    assert_eq!(markers.len(), targets.len());
    for (id, digest) in markers {
        let call = before.tool_calls.iter().find(|call| call.id == id).unwrap();
        assert!(targets.iter().any(|target| target.call_id == id));
        assert_eq!(digest, vega_store::context_micro::call_digest(call));
    }
    let restored = fixture.request(budget, threshold);
    for original in request
        .context
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::Tool)
    {
        let id = original.tool_call_id.as_deref().unwrap();
        let reloaded = restored
            .context
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(id))
            .unwrap();
        if let Some(target) = targets.iter().find(|target| target.call_id == id) {
            assert_eq!(reloaded.content, micro_result_content(target));
        } else {
            assert_eq!(reloaded.content, original.content);
        }
    }
    let restored_raw = estimate_wire_context(&restored.wire_messages(), &restored.context.tools)
        .unwrap()
        .input_tokens;
    assert_eq!(raw.checked_sub(restored_raw).unwrap(), expected_savings);
    assert!(fixture.compact(restored, MODEL).await.unwrap().is_none());
    assert_eq!(fixture.source(), after);
}

#[tokio::test]
async fn calibrated_below_threshold_does_not_compact_even_when_raw_is_above() {
    let fixture = Fixture::new(12_000);
    let budget = ContextBudget::new(34_096, 4_096, true).unwrap();
    let threshold = budget.precompute_tokens().unwrap();
    let request = fixture.request(budget, threshold - 1);
    let raw = estimate_wire_context(&request.wire_messages(), &request.context.tools)
        .unwrap()
        .input_tokens;
    assert!(request.context.estimate.input_tokens < threshold);
    assert!(raw >= threshold);
    assert!(wire_savings(&request, &expected_targets()) >= MICRO_MIN_SAVINGS);
    let before = fixture.source();
    assert!(fixture.compact(request, MODEL).await.unwrap().is_none());
    assert_no_install(&fixture, &before);
}

#[tokio::test]
async fn calibrated_threshold_still_requires_minimum_raw_savings() {
    let fixture = Fixture::new(1_500);
    let budget = ContextBudget::new(204_096, 4_096, true).unwrap();
    let threshold = budget.precompute_tokens().unwrap();
    let request = fixture.request(budget, threshold);
    let savings = wire_savings(&request, &expected_targets());
    assert!(savings > 0 && savings < MICRO_MIN_SAVINGS);
    let before = fixture.source();
    assert!(fixture.compact(request, MODEL).await.unwrap().is_none());
    assert_no_install(&fixture, &before);
}

#[tokio::test]
async fn changed_same_sequence_source_is_rejected_without_installing() {
    let fixture = Fixture::new(12_000);
    let budget = ContextBudget::new(204_096, 4_096, true).unwrap();
    let threshold = budget.precompute_tokens().unwrap();
    let request = fixture.request(budget, threshold);
    let original = fixture.source();
    let store = Store::open(&fixture.database).unwrap();
    assert_eq!(
        messages::finish(store.conn(), "initial-user", "Changed task", "done").unwrap(),
        1
    );
    drop(store);
    let changed = fixture.source();
    assert_eq!(changed.source_version, original.source_version);
    assert_ne!(changed.fingerprint, original.fingerprint);
    for require_source_fence in [false, true] {
        let mut stale = request.clone();
        stale.context.require_source_fence = require_source_fence;
        let result = fixture.compact(stale, MODEL).await;
        assert!(matches!(
            result,
            Err(VegaError::Context(ContextRuntimeError::SourceChanged))
        ));
        assert_no_install(&fixture, &changed);
    }
}

#[tokio::test]
async fn mismatched_live_tool_result_is_rejected_without_installing() {
    let fixture = Fixture::new(12_000);
    let budget = ContextBudget::new(204_096, 4_096, true).unwrap();
    let threshold = budget.precompute_tokens().unwrap();
    let mut request = fixture.request(budget, threshold);
    let live = request
        .context
        .messages
        .iter_mut()
        .find(|message| message.tool_call_id.as_deref() == Some(LIVE_CALL))
        .unwrap();
    live.content = "Changed live result".into();
    let before = fixture.source();
    let result = fixture.compact(request, MODEL).await;
    assert!(matches!(
        result,
        Err(VegaError::Context(ContextRuntimeError::SourceChanged))
    ));
    assert_no_install(&fixture, &before);
}

#[tokio::test]
async fn changed_model_is_rejected_by_install_cas_without_persisting() {
    let fixture = Fixture::new(12_000);
    let budget = ContextBudget::new(204_096, 4_096, true).unwrap();
    let threshold = budget.precompute_tokens().unwrap();
    let request = fixture.request(budget, threshold);
    assert!(wire_savings(&request, &expected_targets()) >= MICRO_MIN_SAVINGS);
    let before = fixture.source();
    let store = Store::open(&fixture.database).unwrap();
    assert_eq!(
        threads::set_model(store.conn(), THREAD, "another-model", 6).unwrap(),
        1
    );
    drop(store);
    assert_eq!(fixture.source(), before);
    let result = fixture.compact(request, MODEL).await;
    assert!(matches!(
        result,
        Err(VegaError::Context(ContextRuntimeError::SourceChanged))
    ));
    assert_no_install(&fixture, &before);
}
