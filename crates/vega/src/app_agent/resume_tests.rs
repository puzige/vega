use super::replay_persistence_config;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use vega_conversation::agent::{PermissionQueue, resume_interrupted_assistant_with_mcp};
use vega_conversation::types::{ConversationEvent, InterruptedAssistant};
use vega_runtime::{ChatRequest, EventStream, Provider, VegaError};
use vega_store::Store;

#[derive(Default)]
struct ObservedProvider(Mutex<Vec<ChatRequest>>);

impl Provider for ObservedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<EventStream, VegaError>> + Send + 'static>> {
        self.0.lock().expect("record request").push(request);
        Box::pin(async { Err(VegaError::Cancelled) })
    }
}

#[test]
fn resume_launch_reuses_controller_admission_without_duplicate_user_or_start() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let store = Store::open(directory.path().join("history.sqlite")).expect("fixture store");
    store.migrate().expect("migrate fixture");
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread",
            project_id: "",
            title: "Resume fixture",
            mode: "ask",
            permission_mode: "readonly",
            model: "fixture-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .expect("create thread");
    for (id, seq, role, status) in [
        ("user", 1, "user", "done"),
        ("stopped", 2, "assistant", "interrupted"),
    ] {
        vega_store::messages::insert(
            store.conn(),
            &vega_store::messages::MessageRow {
                id: id.into(),
                thread_id: "thread".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: format!("original {id}"),
                status: status.into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("insert message");
    }
    let original = vega_store::messages::find(store.conn(), "stopped")
        .expect("read original")
        .expect("original exists");
    let admission = vega_store::thread_admission::try_acquire(store.database_identity(), "thread")
        .expect("controller admission")
        .expect("thread available");
    let provider = ObservedProvider::default();
    let tools = vega_tools::Tools::new(directory.path()).expect("fixture tools");
    let permission = PermissionQueue::new();
    let expected = InterruptedAssistant {
        message_id: "stopped".into(),
        seq: 2,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let mut started = 0;
    let result = runtime
        .block_on(resume_interrupted_assistant_with_mcp(
            &store,
            &provider,
            &tools,
            "thread",
            &expected,
            "fixture system",
            tokio_util::sync::CancellationToken::new(),
            &permission,
            |event| {
                if matches!(event, ConversationEvent::MessageStarted { .. }) {
                    started += 1;
                }
                Ok(())
            },
            replay_persistence_config(None, None, admission.clone()),
            None,
            None,
            Vec::new(),
        ))
        .expect("resume launches while controller holds admission");
    assert!(result.interrupted);
    assert!(!result.failed);
    assert_eq!(result.user_message_id, "user");
    assert_eq!(started, 1);
    assert_eq!(provider.0.lock().expect("requests").len(), 1);
    let user_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'user'",
            [],
            |row| row.get(0),
        )
        .expect("user count");
    assert_eq!(user_count, 1);
    assert_eq!(
        vega_store::messages::find(store.conn(), "stopped").expect("read preserved"),
        Some(original)
    );
    let before_duplicate: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("message count");
    assert_eq!(before_duplicate, 3);
    assert!(
        runtime
            .block_on(resume_interrupted_assistant_with_mcp(
                &store,
                &provider,
                &tools,
                "thread",
                &expected,
                "fixture system",
                tokio_util::sync::CancellationToken::new(),
                &permission,
                |_| Ok(()),
                replay_persistence_config(None, None, admission.clone()),
                None,
                None,
                Vec::new(),
            ))
            .is_err()
    );
    let after_duplicate: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        .expect("duplicate message count");
    assert_eq!(after_duplicate, before_duplicate);
    assert_eq!(provider.0.lock().expect("requests").len(), 1);
    drop(admission);
    assert!(
        vega_store::thread_admission::try_acquire(store.database_identity(), "thread")
            .expect("reacquire after worker exit")
            .is_some()
    );
}
