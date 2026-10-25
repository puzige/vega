use super::*;
use crate::types::{ContextSettings, PermissionMode};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
pub(super) struct Pause {
    reached: CancellationToken,
    released: CancellationToken,
}

impl Pause {
    pub(super) async fn wait(&self) {
        self.reached.cancel();
        self.released.cancelled().await;
    }
}

struct Fixture {
    root: PathBuf,
    store: Store,
    parent: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-child-{}", ulid::Ulid::generate()));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("store.sqlite")).unwrap();
        store.migrate().unwrap();
        let parent = crate::threads::create_standalone_thread(&store, "model", "confirm")
            .unwrap()
            .id;
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: "parent-run".into(),
                thread_id: parent.clone(),
                seq: 1,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "streaming".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
        Self {
            root,
            store,
            parent,
        }
    }

    fn settings() -> ChildSettings {
        ChildSettings {
            model: "model".into(),
            system_prompt: "system".into(),
            authority: DelegatedAuthority {
                run_mode: RuntimeRunMode::Execute,
                permission_mode: RuntimePermissionMode::Confirm,
                exact_rules: Vec::new(),
            },
            pricing_catalog: None,
            reasoning: None,
            context_budget: None,
            automation_claim: None,
            automation_hook: None,
            owner_credential_reader: Some(Arc::new(|| Ok(Vec::new()))),
            turn_limit: 2,
            steering_source: None,
        }
    }

    fn context(&self, child: &str) -> ContextSettings {
        ContextSettings {
            thread_id: child.into(),
            model: "model".into(),
            context_limit: Some(16_384),
            output_reserve: 1_024,
            automatic_compaction: false,
            updated_at: 1,
        }
    }

    fn assert_mutations_blocked(&self, child: &str) {
        assert!(crate::threads::set_thread_model(&self.store, child, "changed").is_err());
        assert!(
            crate::threads::set_thread_permission_mode(
                &self.store,
                child,
                PermissionMode::ReadOnly,
            )
            .is_err()
        );
        assert!(super::super::save_context_settings(&self.store, &self.context(child)).is_err());
        let thread = crate::threads::get_thread(&self.store, child).unwrap();
        assert_eq!(thread.model, "model");
        assert_eq!(thread.permission_mode, PermissionMode::Confirm);
        assert!(
            super::super::read_context_settings(&self.store, child, "model")
                .unwrap()
                .is_none()
        );
    }

    fn assert_mutations_available(&self, child: &str) {
        super::super::save_context_settings(&self.store, &self.context(child)).unwrap();
        crate::threads::set_thread_permission_mode(&self.store, child, PermissionMode::ReadOnly)
            .unwrap();
        crate::threads::set_thread_model(&self.store, child, "changed").unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Default)]
struct ProviderStub(AtomicUsize);

impl Provider for ProviderStub {
    fn chat_stream(
        &self,
        _: vega_runtime::ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            let events: vega_runtime::EventStream = Box::pin(futures::stream::iter([
                Ok(vega_runtime::ProviderEvent::TextDelta(
                    "child response".into(),
                )),
                Ok(vega_runtime::ProviderEvent::Usage {
                    input: 2,
                    output: 2,
                    cache_read: None,
                    cache_write: None,
                }),
                Ok(vega_runtime::ProviderEvent::Done {
                    stop_reason: vega_runtime::StopReason::End,
                }),
            ]));
            Ok(events)
        })
    }
}

struct Host {
    admission: Option<Arc<Pause>>,
    release: Arc<Pause>,
}

impl SubagentHost for Host {
    fn admit(&self, _: SubagentAdmission) -> BoxFuture<'static, Result<(), VegaError>> {
        let pause = self.admission.clone();
        Box::pin(async move {
            if let Some(pause) = pause {
                pause.wait().await;
            }
            Ok(())
        })
    }

    fn event(&self, _: &str, _: &str, _: &ConversationEvent) -> Result<(), VegaError> {
        Ok(())
    }

    fn updated(&self, _: SubagentSnapshot) -> Result<(), VegaError> {
        Ok(())
    }

    fn finished(&self, _: SubagentSnapshot, _: bool) -> Result<(), VegaError> {
        Ok(())
    }

    fn finish_and_release(
        &self,
        _: SubagentSnapshot,
        _: bool,
    ) -> BoxFuture<'static, Result<(), VegaError>> {
        let pause = self.release.clone();
        Box::pin(async move {
            pause.wait().await;
            Ok(())
        })
    }
}

fn lifecycle<'a>(
    launch: &ChildLaunch<'a>,
    reserved: ReservedExecution,
    supervisor: &Supervisor,
) -> LocalBoxFuture<'a, ()> {
    let (sender, receiver) = oneshot::channel();
    let call = reserved.call_id.clone();
    supervisor
        .registry
        .begin_call(&call, "spawn_agent", &serde_json::json!({}), sender)
        .unwrap();
    let child = child_lifecycle(launch, reserved, supervisor.registry.clone());
    let observer = supervisor.receipt_observer();
    async move {
        let receipt = async move {
            if let Ok(Ok(output)) = receiver.await {
                observer.persisted(&RuntimeEvent::ToolCallFinished(
                    vega_runtime::RuntimeToolResult {
                        call_id: call,
                        output: output.to_string(),
                        status: RuntimeToolStatus::Success,
                        reused: false,
                        exit_code: None,
                        duration_ms: None,
                        truncated: None,
                        approval: None,
                        remember_rule: None,
                    },
                ));
            }
        };
        futures::future::join(child, receipt).await;
    }
    .boxed_local()
}

async fn at_pause(lifecycle: &mut LocalBoxFuture<'_, ()>, pause: &Pause) {
    tokio::select! {
        biased;
        _ = pause.reached.cancelled() => {},
        _ = lifecycle.as_mut() => panic!("child lifecycle ended before its boundary"),
        _ = tokio::time::sleep(Duration::from_secs(10)) => panic!("child lifecycle boundary was not reached"),
    }
}

#[tokio::test]
async fn child_admission_blocks_service_mutations_after_create_until_host_release_is_recorded() {
    let fixture = Fixture::new();
    let supervisor = Supervisor::new();
    let admission = Arc::new(Pause::default());
    let release = Arc::new(Pause::default());
    let host = Arc::new(Host {
        admission: Some(admission.clone()),
        release: release.clone(),
    });
    let provider = ProviderStub::default();
    let tools = vega_tools::Tools::new(&fixture.root).unwrap();
    let launch = ChildLaunch {
        provider: &provider,
        tools: &tools,
        database: fixture.store.database_path().unwrap().into(),
        parent_thread: fixture.parent.clone(),
        parent_message: "parent-run".into(),
        settings: Fixture::settings(),
        mcp_servers: Vec::new(),
        host,
        cancel: CancellationToken::new(),
    };
    let reserved = supervisor
        .reserve(
            &launch,
            "spawn",
            "spawn_agent",
            &serde_json::json!({"name":"child","task":"task"}),
        )
        .unwrap();
    let snapshot = reserved.snapshot.clone();
    let cancel = reserved.cancel.clone();
    let mut lifecycle = lifecycle(&launch, reserved, &supervisor);
    at_pause(&mut lifecycle, &admission).await;
    assert_eq!(provider.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        vega_store::context_compaction::load_source(fixture.store.conn(), &snapshot.agent_id)
            .unwrap()
            .messages
            .len(),
        1
    );
    fixture.assert_mutations_blocked(&snapshot.agent_id);
    cancel.cancel();
    admission.released.cancel();
    at_pause(&mut lifecycle, &release).await;
    fixture.assert_mutations_blocked(&snapshot.agent_id);
    assert_eq!(
        vega_store::delegated_stages::release_state(
            fixture.store.conn(),
            &snapshot.agent_id,
            &snapshot.message_id
        )
        .unwrap(),
        "held"
    );
    release.released.cancel();
    lifecycle.await;
    assert_eq!(
        vega_store::delegated_stages::release_state(
            fixture.store.conn(),
            &snapshot.agent_id,
            &snapshot.message_id
        )
        .unwrap(),
        "released"
    );
    fixture.assert_mutations_available(&snapshot.agent_id);
}

#[tokio::test]
async fn child_admission_survives_runtime_return_terminal_persistence_and_release_bookkeeping() {
    let fixture = Fixture::new();
    let supervisor = Supervisor::new();
    let terminal = Arc::new(Pause::default());
    supervisor.registry.state.lock().unwrap().before_terminal = Some(terminal.clone());
    let release = Arc::new(Pause::default());
    let host = Arc::new(Host {
        admission: None,
        release: release.clone(),
    });
    let provider = ProviderStub::default();
    let tools = vega_tools::Tools::new(&fixture.root).unwrap();
    let launch = ChildLaunch {
        provider: &provider,
        tools: &tools,
        database: fixture.store.database_path().unwrap().into(),
        parent_thread: fixture.parent.clone(),
        parent_message: "parent-run".into(),
        settings: Fixture::settings(),
        mcp_servers: Vec::new(),
        host,
        cancel: CancellationToken::new(),
    };
    let reserved = supervisor
        .reserve(
            &launch,
            "spawn",
            "spawn_agent",
            &serde_json::json!({"name":"child","task":"task"}),
        )
        .unwrap();
    let snapshot = reserved.snapshot.clone();
    let mut lifecycle = lifecycle(&launch, reserved, &supervisor);
    at_pause(&mut lifecycle, &terminal).await;
    assert_eq!(provider.0.load(Ordering::SeqCst), 1);
    let assistant = messages::find(fixture.store.conn(), &snapshot.message_id)
        .unwrap()
        .unwrap();
    assert_eq!(assistant.status, "done");
    assert_eq!(assistant.content, "child response");
    assert_eq!(
        vega_store::subagents::find(fixture.store.conn(), &snapshot.agent_id)
            .unwrap()
            .unwrap()
            .state,
        "running"
    );
    fixture.assert_mutations_blocked(&snapshot.agent_id);
    terminal.released.cancel();
    at_pause(&mut lifecycle, &release).await;
    fixture.assert_mutations_blocked(&snapshot.agent_id);
    assert_eq!(
        vega_store::subagents::find(fixture.store.conn(), &snapshot.agent_id)
            .unwrap()
            .unwrap()
            .state,
        "completed"
    );
    assert_eq!(
        vega_store::delegated_stages::release_state(
            fixture.store.conn(),
            &snapshot.agent_id,
            &snapshot.message_id
        )
        .unwrap(),
        "held"
    );
    release.released.cancel();
    lifecycle.await;
    assert_eq!(
        vega_store::delegated_stages::release_state(
            fixture.store.conn(),
            &snapshot.agent_id,
            &snapshot.message_id
        )
        .unwrap(),
        "released"
    );
    fixture.assert_mutations_available(&snapshot.agent_id);
}
