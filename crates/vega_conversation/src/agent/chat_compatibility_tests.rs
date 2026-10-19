use super::*;
use serde_json::{Value, json};
use vega_runtime::{ChatMessage, ChatRequest, ChatRole, EventStream};

pub(super) struct NetworkFixture {
    endpoint: String,
    replies: Mutex<VecDeque<Vec<Value>>>,
    pub(super) requests: Arc<Mutex<Vec<ChatRequest>>>,
    pub(super) bodies: Arc<Mutex<Vec<Value>>>,
}

impl NetworkFixture {
    pub(super) fn new(endpoint: &str, replies: Vec<Vec<Value>>) -> Self {
        Self {
            endpoint: endpoint.into(),
            replies: Mutex::new(replies.into()),
            requests: Default::default(),
            bodies: Default::default(),
        }
    }
}

impl Provider for NetworkFixture {
    fn chat_stream(
        &self,
        mut request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        request.messages =
            vega_runtime::project_model_messages(&request.messages).expect("provider projection");
        let body = vega_runtime::test_support::encode(&self.endpoint, false, &request);
        self.requests.lock().expect("requests").push(request);
        if let Ok(body) = &body {
            self.bodies.lock().expect("bodies").push(body.clone());
        }
        let chunks = self
            .replies
            .lock()
            .expect("replies")
            .pop_front()
            .expect("unexpected network request");
        Box::pin(async move {
            body?;
            Ok(
                Box::pin(futures::stream::iter(vega_runtime::test_support::decode(
                    &chunks, true,
                ))) as EventStream,
            )
        })
    }
}

pub(super) fn read_chunks(state: Value) -> Vec<Value> {
    vec![
        json!({"choices":[{"delta":state}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"read-owned","function":{"name":"read","arguments":"{\"path\":\"sample.txt\",\"offset\":null,\"limit\":null}"}}]},"finish_reason":"tool_calls"}]}),
    ]
}

pub(super) fn reply_chunks(state: Value, text: &str) -> Vec<Value> {
    let mut state = state.as_object().expect("response state").clone();
    state.insert("content".into(), json!(text));
    vec![json!({"choices":[{"delta":state,"finish_reason":"stop"}]})]
}

fn store_fixture(model: &str) -> (tempfile::TempDir, Store, vega_tools::Tools) {
    let root = tempfile::tempdir().expect("owned persistence fixture");
    let store = Store::open(root.path().join("owned.sqlite")).expect("owned store");
    store.migrate().expect("migrations");
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            id: "owned-thread",
            project_id: "",
            title: "fixture",
            mode: "execute",
            permission_mode: "readonly",
            model,
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .expect("owned thread");
    std::fs::write(
        root.path().join("sample.txt"),
        "verified durable tool result",
    )
    .expect("owned file");
    let tools = vega_tools::Tools::new(root.path()).expect("owned tools");
    (root, store, tools)
}

async fn run(
    store: &Store,
    tools: &vega_tools::Tools,
    provider: &dyn Provider,
    text: &str,
    config: PersistenceActorConfig,
) -> ConversationRun {
    run_thread_task_with_pricing_and_reasoning(
        store,
        provider,
        tools,
        "owned-thread",
        text,
        "system",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        config,
        None,
        None,
        None,
    )
    .await
    .expect("conversation run")
}

#[tokio::test]
async fn parsed_chat_state_replays_warm_but_never_persists_in_sqlite_or_visible_answer() {
    for state in [
        json!({"reasoning_text":"raw persistence sentinel"}),
        json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"e","data":"opaque persistence sentinel","signature":"signature persistence sentinel"}]}),
    ] {
        let (root, store, tools) = store_fixture("custom-reasoner");
        let provider = NetworkFixture::new(
            "https://proxy.example/v1",
            vec![
                read_chunks(state.clone()),
                reply_chunks(state, "done"),
                reply_chunks(json!({}), "continued"),
            ],
        );
        let registry = ReplaySessionRegistry::new();
        let first = run(
            &store,
            &tools,
            &provider,
            "read sample.txt",
            PersistenceActorConfig::default()
                .with_replay_session_registry(registry.clone(), [7; 32]),
        )
        .await;
        assert!(!first.failed);
        assert_eq!(first.content, "done");
        let second = run(
            &store,
            &tools,
            &provider,
            "continue",
            PersistenceActorConfig::default().with_replay_session_registry(registry, [7; 32]),
        )
        .await;
        assert!(!second.failed);
        assert_eq!(second.content, "continued");
        let bodies = provider.bodies.lock().expect("bodies");
        let body = bodies[2].to_string();
        assert!(
            body.contains("raw persistence sentinel")
                || body.contains("opaque persistence sentinel")
        );
        assert!(!format!("{first:?} {second:?}").contains("persistence sentinel"));
        let rows = vega_store::messages::recent(store.conn(), "owned-thread", 100)
            .expect("stored messages");
        assert!(
            rows.iter()
                .all(|row| !row.content.contains("persistence sentinel"))
        );
        let calls: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
            .expect("tool count");
        assert_eq!(calls, 1);
        for file in ["owned.sqlite", "owned.sqlite-wal"] {
            if let Ok(bytes) = std::fs::read(root.path().join(file)) {
                assert!(
                    !bytes
                        .windows(b"persistence sentinel".len())
                        .any(|bytes| bytes == b"persistence sentinel")
                );
            }
        }
    }
}

#[tokio::test]
async fn new_raw_and_opaque_credential_content_is_blocked_before_tool_persistence_or_execution() {
    let credential = "sk-owned-secret-9x6B3n8q2T7v5P1r";
    for state in [
        json!({"reasoning_text":credential}),
        json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"e","data":credential,"signature":"signed"}]}),
    ] {
        let (_root, store, tools) = store_fixture("custom-reasoner");
        let network = Arc::new(NetworkFixture::new(
            "https://proxy.example/v1",
            vec![
                read_chunks(state),
                reply_chunks(json!({}), "must not finish"),
            ],
        ));
        let known: vega_runtime::CredentialReader =
            Arc::new(move || Ok(vec![credential.to_string()]));
        let owner = OwnerCredentialProvider::new(network.clone(), known.clone());
        let outcome = run(
            &store,
            &tools,
            &owner,
            "read sample.txt",
            PersistenceActorConfig::default().with_owner_credential_reader(known),
        )
        .await;
        assert!(outcome.failed);
        let calls: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
            .expect("tool count");
        assert_eq!(calls, 0);
        assert_eq!(network.requests.lock().expect("requests").len(), 1);
        assert!(!outcome.content.contains(credential));
        assert!(!format!("{outcome:?} {:?}", outcome.events).contains(credential));
    }
}

#[tokio::test]
async fn pre_attempt_guard_rejects_parsed_raw_and_signed_details_without_mutating_them() {
    let credential = "sk-owned-secret-3w6C8d2E9f4G7h5J";
    for state in [
        json!({"reasoning":credential}),
        json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"e","data":"opaque data","signature":credential}]}),
    ] {
        let (_root, tools) = {
            let root = tempfile::tempdir().expect("runtime fixture");
            std::fs::write(root.path().join("sample.txt"), "verified").expect("file");
            let tools = vega_tools::Tools::new(root.path()).expect("tools");
            (root, tools)
        };
        let provider = NetworkFixture::new(
            "https://proxy.example/v1",
            vec![read_chunks(state), reply_chunks(json!({}), "done")],
        );
        let request = AgentRequest {
            model: "custom".into(),
            system_prompt: "system".into(),
            history: vec![ChatMessage::new(ChatRole::User, "read")],
            max_tokens: None,
            completed_tool_results: Default::default(),
            tool_config: RuntimeToolConfig::default().with_turn_limit(4),
            pricing_catalog: None,
            reasoning: None,
            replay_frame: None,
            context_budget: None,
            context_source_version: None,
            context_source_fingerprint: None,
            context_operation_id: None,
            context_compaction_hook: None,
        };
        let outcome = vega_runtime::run_agent(&provider, &tools, request, CancellationToken::new())
            .await
            .expect("raw producer");
        assert!(!outcome.failed);
        let request = provider.requests.lock().expect("requests")[1].clone();
        let before =
            vega_runtime::test_support::encode("https://proxy.example/v1", false, &request)
                .expect("raw wire");
        assert!(before.to_string().contains(credential));
        let guard = OwnerCredentialProvider::pre_attempt_guard(Arc::new(move || {
            Ok(vec![credential.into()])
        }));
        assert!(matches!(
            guard(&request),
            Err(VegaError::CredentialExposureBlocked)
        ));
        assert_eq!(
            before,
            vega_runtime::test_support::encode("https://proxy.example/v1", false, &request)
                .expect("unchanged raw wire")
        );
    }
}

#[tokio::test]
async fn raw_detail_retention_accounts_for_payload_in_the_real_registry_limit() {
    let root = tempfile::tempdir().expect("retention fixture");
    std::fs::write(root.path().join("sample.txt"), "verified").expect("file");
    let tools = vega_tools::Tools::new(root.path()).expect("tools");
    let provider = NetworkFixture::new(
        "https://proxy.example/v1",
        vec![
            read_chunks(
                json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"e","data":"r".repeat(48*1024)}]}),
            ),
            reply_chunks(json!({}), "done"),
        ],
    );
    let mut frame = vega_runtime::ReplayFrame::new(2, vega_runtime::ReasoningReplayPolicy::Unknown);
    frame.attach_route_fingerprint([8; 32]);
    let request = AgentRequest {
        model: "custom".into(),
        system_prompt: "system".into(),
        history: vec![ChatMessage::new(ChatRole::User, "read")],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::default().with_turn_limit(4),
        pricing_catalog: None,
        reasoning: None,
        replay_frame: Some(frame),
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    };
    let outcome = vega_runtime::run_agent(&provider, &tools, request, CancellationToken::new())
        .await
        .expect("retention producer");
    assert!(!outcome.failed);
    let raw = outcome
        .messages
        .iter()
        .find(|message| !message.tool_calls.is_empty())
        .expect("raw assistant")
        .clone();
    let mut oversized = vec![outcome.messages[0].clone()];
    oversized.extend(std::iter::repeat_n(raw, 800));
    let registry = ReplaySessionRegistry::new();
    let fence = ReplayCheckpointFence::from_checkpoint(None);
    let lease = registry.begin("owned", 1, "source", fence);
    assert!(!registry.seal(lease, 1, "source".into(), fence, "owner".into(), oversized));
}

#[tokio::test]
async fn real_manual_compaction_uses_the_same_chat_encoder_and_excludes_previous_raw_state() {
    let (_root, store, tools) = store_fixture("openai/gpt-oss-120b");
    let provider = NetworkFixture::new(
        "https://api.groq.com/openai/v1",
        vec![
            read_chunks(
                json!({"reasoning_details":[{"type":"reasoning.encrypted","id":"old","data":"old compaction opaque"}]}),
            ),
            reply_chunks(json!({"reasoning_text":"old compaction raw"}), "first done"),
            reply_chunks(json!({}), "second done"),
        ],
    );
    assert!(
        !run(
            &store,
            &tools,
            &provider,
            "read sample.txt",
            PersistenceActorConfig::default()
        )
        .await
        .failed
    );
    assert!(
        !run(
            &store,
            &tools,
            &provider,
            "what next",
            PersistenceActorConfig::default()
        )
        .await
        .failed
    );
    let summary = NetworkFixture::new(
        "https://api.groq.com/openai/v1",
        vec![reply_chunks(
            json!({"reasoning_text":"auxiliary raw sentinel","reasoning_details":[{"type":"reasoning.encrypted","id":"aux","data":"auxiliary opaque sentinel"}]}),
            "Checkpoint: the file was read successfully; continue from the verified result.",
        )],
    );
    let result = compact_thread_manually(
        &store,
        &summary,
        "owned-thread",
        "openai/gpt-oss-120b",
        "system",
        vega_runtime::tool_definitions(vega_runtime::RuntimeRunMode::Execute),
        vega_runtime::ContextBudget::new(200_000, 20_000, false).expect("budget"),
        CancellationToken::new(),
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "{result:?}");
    let body = summary.bodies.lock().expect("summary body")[0].clone();
    assert_eq!(body["max_completion_tokens"], 20_000);
    assert!(body.get("max_tokens").is_none());
    assert_eq!(body["messages"][0]["role"], "developer");
    let sent = body.to_string();
    assert!(!sent.contains("old compaction opaque"));
    assert!(!sent.contains("old compaction raw"));
    let result = result.expect("successful compaction");
    assert!(
        result
            .messages
            .iter()
            .all(|message| !format!("{message:?}").contains("sentinel"))
    );
    let wire = vega_runtime::test_support::encode(
        "https://api.groq.com/openai/v1",
        false,
        &ChatRequest {
            model: "openai/gpt-oss-120b".into(),
            messages: result.messages,
            tools: vec![],
            max_tokens: None,
            reasoning: None,
        },
    )
    .expect("compacted wire");
    assert!(!wire.to_string().contains("sentinel"));
    assert_eq!(
        store
            .conn()
            .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row
                .get::<_, i64>(0))
            .expect("durable calls"),
        1
    );
}
