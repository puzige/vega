use super::*;
use std::path::PathBuf;
use std::time::Instant;
use vega_store::write_coordination::{WriteCategory, acquire};

struct Database {
    root: PathBuf,
    path: PathBuf,
}

impl Database {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-persistence-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("store.sqlite");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        for index in 0..3 {
            let thread_id = format!("thread-{index}");
            vega_store::threads::create_standalone(
                store.conn(),
                vega_store::threads::NewThread {
                    id: &thread_id,
                    project_id: "",
                    title: "task",
                    mode: "execute",
                    permission_mode: "confirm",
                    model: "model",
                    status: "active",
                    pinned: false,
                    unread: false,
                    created_at: 1,
                    updated_at: 1,
                },
            )
            .unwrap();
            messages::insert(
                store.conn(),
                &messages::MessageRow {
                    id: format!("message-{index}"),
                    thread_id,
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
        }
        Self { root, path }
    }

    async fn actor(&self, index: usize) -> PersistenceActor {
        PersistenceActor::start(PersistenceActorStart {
            database_path: self.path.clone(),
            project_id: String::new(),
            thread_id: format!("thread-{index}"),
            message_id: format!("message-{index}"),
            model: "model".into(),
            is_plan: false,
            next_tool_seq: 1,
            config: PersistenceActorConfig::default(),
        })
        .await
        .unwrap()
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn audit() -> vega_runtime::RuntimeApprovalAudit {
    vega_runtime::RuntimeApprovalAudit {
        decision: vega_runtime::RuntimeApprovalDecision::Once,
        source: vega_runtime::RuntimeApprovalSource::ReadonlyTool,
        note: None,
        danger: None,
    }
}

async fn persist_run(actor: &PersistenceActor, index: usize) {
    let content = format!("saved response {index}");
    let call_id = format!("call-{index}");
    actor.snapshot(content.clone()).await.unwrap();
    actor
        .event(
            RuntimeEvent::ToolCallProposed(vega_runtime::RuntimeToolCall {
                id: call_id.clone(),
                name: "Read".into(),
                input_json: "{\"file_path\":\"input.txt\"}".into(),
            }),
            content.clone(),
            None,
        )
        .await
        .unwrap();
    actor
        .event(
            RuntimeEvent::ToolCallApproved {
                call_id: call_id.clone(),
                audit: audit(),
                remember_rule: None,
            },
            content.clone(),
            None,
        )
        .await
        .unwrap();
    actor
        .event(
            RuntimeEvent::ToolCallRunning {
                call_id: call_id.clone(),
            },
            content.clone(),
            None,
        )
        .await
        .unwrap();
    actor
        .event(
            RuntimeEvent::ToolCallFinished(vega_runtime::RuntimeToolResult {
                call_id: call_id.clone(),
                output: "saved tool result".into(),
                status: RuntimeToolStatus::Success,
                reused: false,
                exit_code: None,
                duration_ms: None,
                truncated: Some(false),
                approval: Some(audit()),
                remember_rule: None,
            }),
            content.clone(),
            None,
        )
        .await
        .unwrap();
    actor
        .event(
            RuntimeEvent::ToolRoundCompleted {
                response_id: format!("response-{index}"),
                call_ids: vec![call_id],
            },
            content.clone(),
            None,
        )
        .await
        .unwrap();
    actor
        .event(
            RuntimeEvent::AssistantResponseCompleted {
                response_id: format!("response-{index}"),
                text_start_bytes: 0,
                text_end_bytes: content.len(),
                kind: vega_runtime::RuntimeAssistantResponseKind::Final,
            },
            content.clone(),
            None,
        )
        .await
        .unwrap();
    actor
        .event(
            RuntimeEvent::UsageUpdated {
                usage: vega_runtime::RuntimeTokenUsage {
                    input: 11,
                    output: 5,
                    cache_read: Some(0),
                    cache_write: Some(0),
                },
                cost_microcents: 0,
                pricing: None,
            },
            content.clone(),
            None,
        )
        .await
        .unwrap();
    actor
        .event(
            RuntimeEvent::Finished(vega_runtime::RuntimeFinishReason::End),
            content,
            Some(20),
        )
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_actors_preserve_text_terminal_audits_responses_and_usage_once() {
    let database = Database::new();
    let first = database.actor(0).await;
    let second = database.actor(1).await;
    let third = database.actor(2).await;
    let writer = Store::open(&database.path).unwrap();
    let permit = acquire(writer.conn(), WriteCategory::Threads).unwrap();
    let running = tokio::spawn(async move {
        tokio::join!(
            persist_run(&first, 0),
            persist_run(&second, 1),
            persist_run(&third, 2)
        );
        first.close().await.unwrap();
        second.close().await.unwrap();
        third.close().await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!running.is_finished());
    drop(permit);
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap();
    for index in 0..3 {
        let thread_id = format!("thread-{index}");
        let message_id = format!("message-{index}");
        let message = messages::find(writer.conn(), &message_id).unwrap().unwrap();
        assert_eq!(message.content, format!("saved response {index}"));
        assert_eq!(message.status, "done");
        let tool = tool_calls::find_state(writer.conn(), &format!("call-{index}"))
            .unwrap()
            .unwrap();
        assert_eq!(tool.status, "success");
        assert_eq!(tool.output_text.as_deref(), Some("saved tool result"));
        let saved_audit = ApprovalAudit::from_json(tool.approval.as_deref().unwrap()).unwrap();
        assert_eq!(saved_audit, approval_audit_from_runtime(&audit()));
        assert_eq!(
            tool_calls::count_by_message(writer.conn(), &thread_id, &message_id).unwrap(),
            1
        );
        let usage =
            token_usage::aggregate_by_message(writer.conn(), &thread_id, &message_id).unwrap();
        assert_eq!(
            (usage.row_count, usage.input_tokens, usage.output_tokens),
            (1, 11, 5)
        );
        let responses: i64 = writer
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM assistant_responses WHERE message_id=?1",
                [&message_id],
                |row| row.get(0),
            )
            .unwrap();
        let rounds: i64 = writer
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM tool_call_rounds WHERE message_id=?1",
                [&message_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((responses, rounds), (1, 1));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actor_waits_beyond_ordinary_timeout_without_replaying_a_usage_event() {
    let database = Database::new();
    let actor = database.actor(0).await;
    let external = vega_store::Connection::open(&database.path).unwrap();
    external.execute_batch("BEGIN IMMEDIATE").unwrap();
    let saving = tokio::spawn(async move {
        actor
            .event(
                RuntimeEvent::UsageUpdated {
                    usage: vega_runtime::RuntimeTokenUsage {
                        input: 7,
                        output: 3,
                        cache_read: Some(0),
                        cache_write: Some(0),
                    },
                    cost_microcents: 0,
                    pricing: None,
                },
                String::new(),
                None,
            )
            .await
            .unwrap();
        actor.close().await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(2_200)).await;
    assert!(!saving.is_finished());
    external.execute_batch("COMMIT").unwrap();
    tokio::time::timeout(Duration::from_secs(4), saving)
        .await
        .unwrap()
        .unwrap();
    let usage = token_usage::aggregate_by_message(&external, "thread-0", "message-0").unwrap();
    assert_eq!(
        (usage.row_count, usage.input_tokens, usage.output_tokens),
        (1, 7, 3)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actor_reports_continuous_lock_and_can_save_after_an_explicit_retry() {
    let database = Database::new();
    let actor = database.actor(0).await;
    let external = vega_store::Connection::open(&database.path).unwrap();
    external.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = Instant::now();
    let error = tokio::time::timeout(Duration::from_secs(8), actor.snapshot("not saved".into()))
        .await
        .unwrap()
        .unwrap_err();
    assert!(started.elapsed() >= Duration::from_secs(4));
    let metrics = error.diagnostic_metrics();
    assert_eq!(metrics.runtime_stage, Some("snapshot"));
    assert_eq!(metrics.sqlite_code, Some(5));
    assert_eq!(metrics.sqlite_extended_code, Some(5));
    assert_eq!(
        messages::find(&external, "message-0")
            .unwrap()
            .unwrap()
            .content,
        ""
    );
    external.execute_batch("ROLLBACK").unwrap();
    actor.snapshot("explicit retry saved".into()).await.unwrap();
    actor.close().await.unwrap();
    assert_eq!(
        messages::find(&external, "message-0")
            .unwrap()
            .unwrap()
            .content,
        "explicit retry saved"
    );
}

#[test]
fn connection_timeout_policies_are_explicit_and_read_only_writes_stay_errors() {
    let database = Database::new();
    let ordinary = Store::open(&database.path).unwrap();
    let background =
        Store::open_with_busy_timeout(&database.path, PERSISTENCE_BUSY_TIMEOUT).unwrap();
    let reader = Store::open_read_only(&database.path).unwrap();
    for (store, expected) in [
        (&ordinary, 2_000_i64),
        (&background, 5_000),
        (&reader, 2_000),
    ] {
        let timeout: i64 = store
            .conn()
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, expected);
    }
    let error = vega_store::threads::rename(reader.conn(), "thread-0", "forbidden", 2).unwrap_err();
    assert_eq!(error.sqlite_error().unwrap().extended_code & 255, 8);
}
