use super::*;
use crate::history;
use crate::types::{ConversationError, HistoricalHistoryEntry};
use vega_store::messages::PageCursor;
use vega_store::thread_fork::ForkError;

fn database() -> Store {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    store.conn().execute_batch("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project','/historical-only','project',1,1); INSERT INTO threads(id,project_id,title,model,mode,permission_mode,created_at,updated_at) VALUES('source','project','Original','model','execute','full_access',1,2)").unwrap();
    message(&store, "u1", 1, "user", "visible 用户");
    message(&store, "a1", 2, "assistant", "说明✓最终答案");
    store.conn().execute_batch("INSERT INTO assistant_responses VALUES('a1','comment',0,9,'commentary'); INSERT INTO assistant_responses VALUES('a1','final',9,21,'final'); INSERT INTO assistant_run_durations VALUES('a1',42)").unwrap();
    store
}

fn message(store: &Store, id: &str, seq: i64, role: &str, content: &str) {
    store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,'source',?2,?3,?4,'done',100)", (id,seq,role,content)).unwrap();
}

fn call(store: &Store, id: &str, owner: &str, seq: i64, tool: &str, input: &str, output: &str) {
    store.conn().execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,text_offset_bytes,created_at,finished_at) VALUES(?1,'source',?2,?3,?4,?5,?6,'success',0,100,101)", (id,owner,seq,tool,input,output)).unwrap();
}

fn snapshot(store: &Store) -> Vec<(String, Vec<String>)> {
    let tables = store
        .conn()
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    tables
        .into_iter()
        .map(|table| {
            let columns = store
                .conn()
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .map(Result::unwrap)
                .collect::<Vec<_>>();
            let values = columns
                .iter()
                .map(|column| format!("quote({column})"))
                .collect::<Vec<_>>()
                .join("||'|'||");
            let rows = store
                .conn()
                .prepare(&format!("SELECT {values} FROM {table} ORDER BY 1"))
                .unwrap()
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            (table, rows)
        })
        .collect()
}

fn owner(store: &Store, thread: &str, seq: i64) -> String {
    store
        .conn()
        .query_row(
            "SELECT id FROM messages WHERE thread_id=?1 AND seq=?2",
            (thread, seq),
            |row| row.get(0),
        )
        .unwrap()
}

fn count(store: &Store, sql: &str, thread: &str) -> i64 {
    store
        .conn()
        .query_row(sql, [thread], |row| row.get(0))
        .unwrap()
}

fn no_authority(store: &Store, thread: &str) {
    for table in [
        "pending_messages",
        "token_usage",
        "context_checkpoints",
        "credential_continuations",
        "credential_privacy_exceptions",
        "thread_skill_pins",
    ] {
        assert_eq!(
            count(
                store,
                &format!("SELECT COUNT(*) FROM {table} WHERE thread_id=?1"),
                thread
            ),
            0,
            "{table}"
        );
    }
    for table in [
        "delegated_runs",
        "delegated_executions",
        "delegated_stage_checkpoints",
        "delegated_execution_recovery",
        "delegated_read_receipts",
        "delegated_execution_messages",
    ] {
        assert_eq!(
            count(
                store,
                &format!("SELECT COUNT(*) FROM {table} WHERE child_thread_id=?1"),
                thread
            ),
            0,
            "{table}"
        );
    }
    assert_eq!(
        count(
            store,
            "SELECT COUNT(*) FROM skill_run_snapshots WHERE thread_id=?1",
            thread
        ),
        0
    );
    assert!(
        vega_store::delegated_stages::execution_message(
            store.conn(),
            thread,
            &owner(store, thread, 2)
        )
        .unwrap()
        .is_none()
    );
    assert!(
        vega_store::delegated_stages::read_receipt(
            store.conn(),
            thread,
            &owner(store, thread, 2),
            "original-read"
        )
        .is_err()
    );
    let violations = store
        .conn()
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |_| Ok(()))
        .unwrap()
        .count();
    assert_eq!(violations, 0);
}

fn canonical(store: &Store, thread: &str) -> Vec<vega_runtime::ChatMessage> {
    let tx = store.conn().unchecked_transaction().unwrap();
    let source = context_compaction::load_source_in_transaction(&tx, thread).unwrap();
    crate::agent::history_from_context_source(&source, "", None).unwrap()
}

fn assert_canonical_parity(store: &Store, source: &str, destination: &str) {
    let left = canonical(store, source);
    let right = canonical(store, destination);
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(&right) {
        assert_eq!(left.role, right.role);
        assert_eq!(left.content, right.content);
        assert_eq!(left.images, right.images);
        assert_eq!(left.tool_calls.len(), right.tool_calls.len());
        for (left, right) in left.tool_calls.iter().zip(&right.tool_calls) {
            assert_ne!(left.id, right.id);
            assert_eq!(left.name, right.name);
            assert_eq!(left.input_json, right.input_json);
        }
        assert_eq!(left.tool_call_id.is_some(), right.tool_call_id.is_some());
        let mut normalized = right.clone();
        normalized.tool_call_id = left.tool_call_id.clone();
        for (call, original) in normalized.tool_calls.iter_mut().zip(&left.tool_calls) {
            call.id = original.id.clone();
        }
        assert_eq!(left, &normalized);
    }
}

#[test]
fn completed_unicode_prefix_is_atomic_fresh_and_independent() {
    let store = database();
    store.conn().execute_batch("UPDATE messages SET kind='plan',plan_status='changes_requested',plan_review_note='请修改',plan_reviewed_at=123 WHERE id='a1'; INSERT INTO credential_privacy_exceptions VALUES('source','rule','fingerprint')").unwrap();
    call(
        &store,
        "read",
        "a1",
        1,
        "read",
        r#"{"path":"file"}"#,
        "original bounded output",
    );
    store.conn().execute_batch("INSERT INTO tool_call_rounds VALUES('a1','read','round'); UPDATE tool_calls SET output_full_path='/nonexistent/never-open',approval='once',duration_ms=3,exit_code=0 WHERE id='read'").unwrap();
    message(&store, "suffix", 3, "assistant", "unrelated suffix");
    call(
        &store,
        "suffix-call",
        "suffix",
        2,
        "read",
        "{}",
        "suffix output",
    );
    store
        .conn()
        .execute_batch("UPDATE tool_calls SET text_offset_bytes=999999 WHERE id='suffix-call'")
        .unwrap();
    let before = snapshot(&store);
    let fork = fork_thread(&store, "source", "a1").unwrap();
    let second = fork_thread(&store, "source", "a1").unwrap();
    assert_ne!(fork.id, second.id);
    assert_eq!(fork.forked_from_thread_id.as_deref(), Some("source"));
    assert_eq!(fork.forked_from_message_id.as_deref(), Some("a1"));
    assert_eq!(fork.permission_mode, PermissionMode::Confirm);
    assert_eq!(fork.title, "Original");
    assert!(!fork.pinned && !fork.unread && fork.parent_thread_id.is_none());
    assert_eq!(
        task_workspace_root(&store, &fork.id).unwrap(),
        std::path::PathBuf::from("/historical-only")
    );
    assert!(
        list_threads(&store, "project", None)
            .unwrap()
            .iter()
            .any(|thread| thread.id == fork.id)
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM messages WHERE thread_id=?1",
            &fork.id
        ),
        2
    );
    let copied = store_messages::find(store.conn(), &owner(&store, &fork.id, 2))
        .unwrap()
        .unwrap();
    assert_eq!(copied.content, "说明✓最终答案");
    assert_eq!(copied.plan_review_note.as_deref(), Some("请修改"));
    assert_eq!(copied.plan_reviewed_at, Some(123));
    assert_eq!(copied.created_at, 100);
    let path: String = store
        .conn()
        .query_row(
            "SELECT output_full_path FROM tool_calls WHERE thread_id=?1",
            [&fork.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(path, "/nonexistent/never-open");
    let responses = count(
        &store,
        "SELECT COUNT(*) FROM assistant_responses WHERE message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
        &fork.id,
    );
    assert_eq!(responses, 2);
    let shared = count(
        &store,
        "SELECT COUNT(*) FROM messages a JOIN messages b ON a.id=b.id WHERE a.thread_id=?1 AND b.thread_id='source'",
        &fork.id,
    );
    assert_eq!(shared, 0);
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM tool_calls a JOIN tool_calls b ON a.id=b.id WHERE a.thread_id=?1 AND b.thread_id='source'",
            &fork.id
        ),
        0
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM assistant_responses a JOIN assistant_responses b ON a.response_id=b.response_id WHERE a.message_id IN (SELECT id FROM messages WHERE thread_id=?1) AND b.message_id IN (SELECT id FROM messages WHERE thread_id='source')",
            &fork.id
        ),
        0
    );
    assert_ne!(owner(&store, &fork.id, 2), owner(&store, &second.id, 2));
    let after = snapshot(&store);
    for ((table, rows), (_, new_rows)) in before.iter().zip(&after) {
        for row in rows {
            assert!(new_rows.contains(row), "source mutated in {table}");
        }
    }
    no_authority(&store, &fork.id);
    store
        .conn()
        .execute("DELETE FROM messages WHERE thread_id='source'", [])
        .unwrap();
    store
        .conn()
        .execute("DELETE FROM threads WHERE id='source'", [])
        .unwrap();
    assert_eq!(
        get_thread(&store, &fork.id).unwrap().forked_from_thread_id,
        None
    );
    assert_eq!(
        get_thread(&store, &fork.id)
            .unwrap()
            .forked_from_message_id
            .as_deref(),
        Some("a1")
    );
}

#[test]
fn delivered_model_and_cross_page_continuations_survive_repeated_forks() {
    let store = database();
    message(&store, "u2", 3, "user", "visible steering");
    message(&store, "a2", 4, "assistant", "done");
    store.conn().execute_batch("INSERT INTO assistant_responses VALUES('a2','f2',0,4,'final'); INSERT INTO pending_messages(id,thread_id,ordinal,behavior,generation,payload,model_content,state,created_at,accepted_after_assistant_id) VALUES('u2','source',1,'steer','live-generation','{}','actual model 用户','delivered',3,'a1')").unwrap();
    call(
        &store,
        "read",
        "a1",
        1,
        "read",
        r#"{"path":"file"}"#,
        "bounded",
    );
    let before = snapshot(&store);
    let fork = fork_thread(&store, "source", "a2").unwrap();
    assert_canonical_parity(&store, "source", &fork.id);
    assert!(
        canonical(&store, &fork.id)
            .iter()
            .any(|message| message.content == "actual model 用户")
    );
    let next = fork_thread(&store, &fork.id, &owner(&store, &fork.id, 4)).unwrap();
    assert_canonical_parity(&store, &fork.id, &next.id);
    for thread in [&fork.id, &next.id] {
        no_authority(&store, thread);
        for cursor in [
            PageCursor::Head,
            PageCursor::Before(4),
            PageCursor::Before(3),
        ] {
            let page = history::history_page_before(&store, thread, cursor, 1).unwrap();
            assert!(page.entries.iter().any(|entry|matches!(entry,history::HistoryEntry::RunContinuation{previous_assistant_id,user_message_id,next_assistant_id} if previous_assistant_id==&owner(&store,thread,2) && user_message_id==&owner(&store,thread,3) && next_assistant_id==&owner(&store,thread,4))));
        }
    }
    let after = snapshot(&store);
    for ((table, rows), (_, new_rows)) in before.iter().zip(&after) {
        for row in rows {
            assert!(new_rows.contains(row), "source changed in {table}");
        }
    }
}

fn delegation(store: &Store) -> String {
    let receipt=serde_json::json!({"agent":{"agent_id":"historical-child","message_id":"historical-run","parent_thread_id":"source","parent_message_id":"a1","name":"Worker","state":"running","result":"recorded at cutoff","truncated":false,"checkpoint":{"id":"historical-checkpoint","payload":{"artifacts":[{"path":"artifact.txt","read_call_id":"original-read"}],"evidence_call_ids":["original-evidence"]}},"release_state":"held","ready_for_review":false}}).to_string();
    call(
        store,
        "spawn",
        "a1",
        1,
        "spawn_agent",
        r#"{"name":"Worker","task":"historic task"}"#,
        &receipt,
    );
    receipt
}

#[test]
fn historical_delegation_is_typed_paginated_and_never_a_live_snapshot() {
    let store = database();
    let receipt = delegation(&store);
    let ordinary = history::latest_history_page(&store, "source", 2).unwrap();
    assert!(ordinary.entries.iter().any(|entry| matches!(
        entry,
        history::HistoryEntry::Tool {
            result: Some(crate::types::ToolCardResultProjection::Subagent { agent: Some(_), .. }),
            ..
        }
    )));
    let source = history::latest_historical_history_page(&store, "source", 2).unwrap();
    let fork = fork_thread(&store, "source", "a1").unwrap();
    assert_canonical_parity(&store, "source", &fork.id);
    no_authority(&store, &fork.id);
    assert!(
        vega_store::delegated_stages::checkpoint(store.conn(), "historical-checkpoint").is_err()
    );
    let mut pages = vec![
        history::latest_historical_history_page(&store, &fork.id, 1).unwrap(),
        history::historical_history_page_after(&store, &fork.id, 1, 1).unwrap(),
        history::historical_history_page_containing_message(
            &store,
            &fork.id,
            &owner(&store, &fork.id, 2),
            1,
        )
        .unwrap()
        .unwrap(),
        history::historical_history_page_before(&store, &fork.id, PageCursor::Before(3), 1)
            .unwrap(),
    ];
    pages.push(source);
    for page in pages {
        let (_, result) = page
            .entries
            .iter()
            .find_map(|entry| match entry {
                HistoricalHistoryEntry::Delegation {
                    input_json, result, ..
                } => Some((input_json, result)),
                _ => None,
            })
            .unwrap();
        assert_eq!(result.receipt.as_deref(), Some(receipt.as_str()));
        assert_eq!(
            result.agents[0].historical_agent_id.as_deref(),
            Some("historical-child")
        );
        assert_eq!(result.agents[0].state.as_deref(), Some("running"));
        assert_eq!(
            result.agents[0].result.as_deref(),
            Some("recorded at cutoff")
        );
        assert!(
            result
                .references
                .iter()
                .any(|reference| reference.source_id == "original-read"
                    && reference.destination_id.is_none())
        );
    }
    let before = snapshot(&store);
    assert!(matches!(
        history::latest_history_page(&store, &fork.id, 1),
        Err(ConversationError::UnsupportedHistoricalConsumer)
    ));
    assert!(matches!(
        history::history_page_before(&store, &fork.id, PageCursor::Before(2), 1),
        Err(ConversationError::UnsupportedHistoricalConsumer)
    ));
    assert!(matches!(
        history::history_page_after(&store, &fork.id, 0, 1),
        Err(ConversationError::UnsupportedHistoricalConsumer)
    ));
    assert!(matches!(
        history::history_page_containing_message(&store, &fork.id, "absent", 1),
        Err(ConversationError::UnsupportedHistoricalConsumer)
    ));
    assert!(matches!(
        history::restart_history_page(&store, &fork.id, 1),
        Err(ConversationError::UnsupportedHistoricalConsumer)
    ));
    assert_eq!(snapshot(&store), before);
    assert_eq!(
        history::latest_history_page(&store, "source", 2).unwrap(),
        ordinary
    );
    store.conn().execute_batch("INSERT INTO threads(id,project_id,model,created_at,updated_at,parent_thread_id) VALUES('historical-child','project','model',1,1,'source'); INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('historical-run','historical-child',1,'assistant','later child state','done',1)").unwrap();
    let page = history::latest_historical_history_page(&store, &fork.id, 1).unwrap();
    assert!(page.entries.iter().any(|entry|matches!(entry,HistoricalHistoryEntry::Delegation{result,..} if result.agents[0].result.as_deref()==Some("recorded at cutoff"))));
    let second = fork_thread(&store, &fork.id, &owner(&store, &fork.id, 2)).unwrap();
    assert_canonical_parity(&store, &fork.id, &second.id);
    let page = history::latest_historical_history_page(&store, &second.id, 1).unwrap();
    assert!(page.entries.iter().any(|entry| matches!(entry,HistoricalHistoryEntry::Delegation{result,..} if result.references.iter().any(|reference|reference.source_id=="source" && reference.destination_id.as_deref()==Some(second.id.as_str())))));
}

#[test]
fn wait_list_and_checkpoint_receipts_remain_inert_and_exact() {
    let store = database();
    let receipt = delegation(&store);
    let agent: serde_json::Value =
        serde_json::from_str::<serde_json::Value>(&receipt).unwrap()["agent"].clone();
    let agents =
        serde_json::json!({"agents":[agent],"timed_out":true,"truncated":false}).to_string();
    let checkpoint=serde_json::json!({"checkpoint":{"id":"cp","child_thread_id":"historical-child","child_message_id":"historical-run","payload":{"stage":"build","summary":"history","artifacts":[{"path":"artifact","read_call_id":"original-read"}],"evidence_call_ids":["evidence"]}},"ready_for_review":false}).to_string();
    call(
        &store,
        "wait",
        "a1",
        2,
        "wait_agent",
        r#"{"agent_id":"historical-child","message_id":"historical-run","timeout_ms":1}"#,
        &agents,
    );
    call(&store, "list", "a1", 3, "list_agents", "{}", &agents);
    call(
        &store,
        "checkpoint",
        "a1",
        4,
        "checkpoint_stage",
        r#"{"stage":"build","summary":"history","artifacts":[{"path":"artifact","read_call_id":"original-read"}],"evidence_call_ids":["evidence"],"unresolved":[],"next_step":"review","ready_for_review":false}"#,
        &checkpoint,
    );
    let before = history::latest_historical_history_page(&store, "source", 2).unwrap();
    let fork = fork_thread(&store, "source", "a1").unwrap();
    assert_canonical_parity(&store, "source", &fork.id);
    let after = history::latest_historical_history_page(&store, &fork.id, 2).unwrap();
    let receipts = |page: crate::types::HistoricalHistoryPage| {
        page.entries
            .into_iter()
            .filter_map(|entry| match entry {
                HistoricalHistoryEntry::Delegation {
                    tool,
                    input_json,
                    status,
                    result,
                    ..
                } => Some((tool, input_json, status, result.receipt, result.agents)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(receipts(before), receipts(after));
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM historical_tool_receipts WHERE thread_id=?1",
            &fork.id
        ),
        4
    );
    no_authority(&store, &fork.id);
    assert!(vega_store::delegated_stages::checkpoint(store.conn(), "cp").is_err());
}

#[test]
fn screenshot_aliases_keep_the_original_capture_and_trailing_receipt_text() {
    let store = database();
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let bytes = bytes.into_inner();
    call(
        &store,
        "capture",
        "a1",
        1,
        "ComputerScreenshot",
        r#"{"target_id":"window"}"#,
        "{\"status\":\"delivered\",\"artifact_id\":\"screenshot:capture\",\"unchanged\":false}\noriginal tail",
    );
    call(
        &store,
        "alias",
        "a1",
        2,
        "ComputerScreenshot",
        r#"{"target_id":"window"}"#,
        "{\"status\":\"delivered\",\"artifact_id\":\"screenshot:capture\",\"unchanged\":true}\noriginal tail",
    );
    store
        .conn()
        .execute("INSERT INTO image_attachments VALUES('a1',0,?1)", [&bytes])
        .unwrap();
    store
        .conn()
        .execute_batch("INSERT INTO computer_screenshot_artifacts VALUES('capture','a1',0)")
        .unwrap();
    let fork = fork_thread(&store, "source", "a1").unwrap();
    let capture: String = store
        .conn()
        .query_row(
            "SELECT call_id FROM computer_screenshot_artifacts WHERE message_id=?1",
            [owner(&store, &fork.id, 2)],
            |row| row.get(0),
        )
        .unwrap();
    let outputs = store
        .conn()
        .prepare("SELECT output_text FROM tool_calls WHERE thread_id=?1 ORDER BY seq")
        .unwrap()
        .query_map([&fork.id], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    for output in &outputs {
        let (first, tail) = output.split_once('\n').unwrap();
        assert_eq!(tail, "original tail");
        let value: serde_json::Value = serde_json::from_str(first).unwrap();
        assert_eq!(value["artifact_id"], format!("screenshot:{capture}"));
    }
    assert!(outputs[1].contains("\"unchanged\":true"));
    let image: Vec<u8> = store
        .conn()
        .query_row(
            "SELECT encoded FROM image_attachments WHERE message_id=?1",
            [owner(&store, &fork.id, 2)],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(image, bytes);
    assert!(
        history::latest_history_page(&store, &fork.id, 2)
            .unwrap()
            .entries
            .iter()
            .any(|entry| matches!(entry, history::HistoryEntry::AssistantScreenshot { .. }))
    );
    let second = fork_thread(&store, &fork.id, &owner(&store, &fork.id, 2)).unwrap();
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM historical_tool_receipts WHERE thread_id=?1",
            &second.id
        ),
        2
    );
    store.conn().execute("UPDATE tool_calls SET output_text='{\"artifact_id\":\"screenshot:missing\"}' WHERE id='alias'",[]).unwrap();
    let before = snapshot(&store);
    assert!(matches!(
        fork_thread(&store, "source", "a1"),
        Err(ForkThreadError::Refused(ForkError::CorruptHistory(_)))
    ));
    assert_eq!(snapshot(&store), before);
    message(&store, "future", 3, "assistant", "future");
    call(
        &store,
        "future-capture",
        "future",
        3,
        "ComputerScreenshot",
        "{}",
        "future",
    );
    store
        .conn()
        .execute(
            "INSERT INTO image_attachments VALUES('future',0,?1)",
            [&bytes],
        )
        .unwrap();
    store.conn().execute_batch("INSERT INTO computer_screenshot_artifacts VALUES('future-capture','future',0); UPDATE tool_calls SET output_text='{\"artifact_id\":\"screenshot:future-capture\"}' WHERE id='alias'").unwrap();
    let before = snapshot(&store);
    assert!(matches!(
        fork_thread(&store, "source", "a1"),
        Err(ForkThreadError::Refused(ForkError::CorruptHistory(_)))
    ));
    assert_eq!(snapshot(&store), before);
}

#[test]
fn refusals_and_late_storage_failure_leave_no_destination() {
    let store = database();
    assert!(matches!(
        fork_thread(&store, "absent", "a1"),
        Err(ForkThreadError::Refused(ForkError::MissingSource))
    ));
    assert!(matches!(
        fork_thread(&store, "source", "u1"),
        Err(ForkThreadError::Refused(ForkError::InvalidCutoff))
    ));
    for change in [
        "UPDATE threads SET project_id=NULL WHERE id='source'",
        "UPDATE threads SET backend='codex' WHERE id='source'",
        "UPDATE threads SET parent_thread_id='source' WHERE id='source'",
        "UPDATE threads SET workspace_owner_thread_id='source' WHERE id='source'",
    ] {
        let tx = store.conn().unchecked_transaction().unwrap();
        tx.execute_batch(change).unwrap();
        assert!(matches!(
            vega_store::thread_fork::clone_prefix(&tx, "source", "a1", 5),
            Err(ForkError::UnsupportedSource)
        ));
    }
    for (sql, active) in [
        (
            "UPDATE messages SET status='interrupted' WHERE id='a1'",
            false,
        ),
        ("DELETE FROM assistant_responses WHERE kind='final'", false),
        ("UPDATE messages SET status='streaming' WHERE id='a1'", true),
    ] {
        let tx = store.conn().unchecked_transaction().unwrap();
        tx.execute_batch(sql).unwrap();
        let error = vega_store::thread_fork::clone_prefix(&tx, "source", "a1", 5).unwrap_err();
        assert!(if active {
            matches!(error, ForkError::ActiveSource)
        } else {
            matches!(error, ForkError::InvalidCutoff)
        });
    }
    call(&store, "read", "a1", 1, "read", "{}", "result");
    for sql in [
        "UPDATE tool_calls SET text_offset_bytes=1 WHERE id='read'",
        "UPDATE tool_calls SET output_text=NULL WHERE id='read'",
        "UPDATE tool_calls SET duration_ms=-1 WHERE id='read'",
    ] {
        store.conn().execute_batch(sql).unwrap();
        let before = snapshot(&store);
        assert!(fork_thread(&store, "source", "a1").is_err());
        assert_eq!(snapshot(&store), before);
        store.conn().execute_batch("UPDATE tool_calls SET text_offset_bytes=0,output_text='result',duration_ms=NULL WHERE id='read'").unwrap();
    }
    store
        .conn()
        .execute_batch("UPDATE tool_calls SET status='running' WHERE id='read'")
        .unwrap();
    assert!(matches!(
        fork_thread(&store, "source", "a1"),
        Err(ForkThreadError::Refused(ForkError::ActiveSource))
    ));
    store.conn().execute_batch("UPDATE tool_calls SET status='success' WHERE id='read'; CREATE TRIGGER fail_fork_duration BEFORE INSERT ON assistant_run_durations WHEN NEW.message_id<>'a1' BEGIN SELECT RAISE(ABORT,'late insert failure'); END").unwrap();
    let before = snapshot(&store);
    assert!(matches!(
        fork_thread(&store, "source", "a1"),
        Err(ForkThreadError::Refused(ForkError::Store(_)))
    ));
    assert_eq!(snapshot(&store), before);
}

#[test]
fn hidden_historical_fields_are_scanned_redacted_and_fingerprinted() {
    let store = database();
    delegation(&store);
    store.conn().execute_batch("INSERT INTO pending_messages(id,thread_id,ordinal,behavior,generation,payload,model_content,state,created_at) VALUES('u1','source',1,'queue','live','{}','hidden SECRET','delivered',1)").unwrap();
    let fork = fork_thread(&store, "source", "a1").unwrap();
    store.conn().execute("UPDATE historical_tool_receipts SET output_text=output_text||' SECRET' WHERE thread_id=?1",[&fork.id]).unwrap();
    let fields = vega_store::credential_privacy::fields(store.conn(), &fork.id).unwrap();
    assert!(
        fields
            .iter()
            .any(|(_, field, text)| field == "historical.model" && text == "hidden SECRET")
    );
    assert!(
        fields
            .iter()
            .any(|(_, field, text)| field == "historical.output" && text.contains("SECRET"))
    );
    for (id, field, _) in fields
        .iter()
        .filter(|(_, field, _)| field.starts_with("historical."))
    {
        assert!(
            vega_store::credential_privacy::source_message_id(store.conn(), field, id)
                .unwrap()
                .is_some()
        );
    }
    let privacy =
        vega_store::credential_privacy::source_fingerprint(store.conn(), &fork.id).unwrap();
    let context = context_compaction::load_source_version(store.conn(), &fork.id)
        .unwrap()
        .fingerprint;
    store.conn().execute("UPDATE historical_tool_receipts SET input_json='{\"task\":\"SECRET historical\"}' WHERE thread_id=?1",[&fork.id]).unwrap();
    assert_ne!(
        privacy,
        vega_store::credential_privacy::source_fingerprint(store.conn(), &fork.id).unwrap()
    );
    assert_ne!(
        context,
        context_compaction::load_source_version(store.conn(), &fork.id)
            .unwrap()
            .fingerprint
    );
    let source_before =
        vega_store::credential_privacy::source_fingerprint(store.conn(), "source").unwrap();
    let expected =
        vega_store::credential_privacy::source_fingerprint(store.conn(), &fork.id).unwrap();
    vega_store::credential_privacy::clean(store.conn(), &fork.id, &expected, |text, offsets| {
        (text.replace("SECRET", "MASKED"), offsets.to_vec())
    })
    .unwrap();
    assert!(
        vega_store::credential_privacy::fields(store.conn(), &fork.id)
            .unwrap()
            .iter()
            .all(|(_, field, text)| !field.starts_with("historical.") || !text.contains("SECRET"))
    );
    assert_eq!(
        source_before,
        vega_store::credential_privacy::source_fingerprint(store.conn(), "source").unwrap()
    );
    assert!(
        canonical(&store, &fork.id)
            .iter()
            .any(|message| message.content == "hidden MASKED")
    );
    no_authority(&store, &fork.id);
}
