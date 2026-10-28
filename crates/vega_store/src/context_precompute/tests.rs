use super::*;
use crate::Store;
use rusqlite::params;

mod persistence;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(streaming: bool) -> Result<Store, Box<dyn std::error::Error>> {
    let store = Store::open(":memory:")?;
    store.migrate()?;
    store.conn().execute(
        "INSERT INTO threads(id,model,created_at,updated_at) VALUES('thread','model',1,1)",
        [],
    )?;
    assert_eq!(insert_message(&store, "task", "user", "RAW_TASK")?, 1);
    assert_eq!(insert_message(&store, "owner", "assistant", "好")?, 2);
    assert_eq!(insert_call(&store, "call-1", "owner", "response-1", 3)?, 1);
    store.conn().execute_batch(
        "INSERT INTO historical_message_delivery(message_id,thread_id,model_content)
             VALUES('task','thread','PROJECTED_TASK');
         INSERT INTO historical_tool_receipts(call_id,thread_id,kind,source_thread_id,source_message_id,source_call_id,input_json,output_text,references_json)
             VALUES('call-1','thread','delegation','origin','original-owner','original-call','{}','HISTORICAL_RESULT','[]');
         INSERT INTO image_attachments(message_id,ordinal,encoded) VALUES('task',0,X'010203');",
    )?;
    if streaming {
        store.conn().execute(
            "UPDATE messages SET status='streaming' WHERE id='owner'",
            [],
        )?;
    }
    Ok(store)
}

fn capture(
    store: &Store,
    owner: Option<&str>,
) -> Result<(ContextSource, ContextAppendFence), ContextCheckpointError> {
    let transaction = store.conn().unchecked_transaction()?;
    let result = capture_in_transaction(&transaction, "thread", owner)?;
    transaction.commit()?;
    Ok(result)
}

fn validate(
    store: &Store,
    fence: &ContextAppendFence,
) -> Result<ContextSource, ContextCheckpointError> {
    let transaction = store.conn().unchecked_transaction()?;
    let result = validate_in_transaction(&transaction, fence)?;
    transaction.commit()?;
    Ok(result)
}

fn insert_message(
    store: &Store,
    id: &str,
    role: &str,
    content: &str,
) -> Result<i64, rusqlite::Error> {
    let seq = crate::messages::next_seq(store.conn(), "thread")?;
    store.conn().execute(
        "INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,'thread',?2,?3,?4,'done',?2)",
        params![id, seq, role, content],
    )?;
    Ok(seq)
}

fn insert_call(
    store: &Store,
    id: &str,
    owner: &str,
    response: &str,
    offset: i64,
) -> Result<i64, rusqlite::Error> {
    let seq = crate::tool_calls::next_seq(store.conn(), "thread")?;
    store.conn().execute(
        "INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes)
         VALUES(?1,'thread',?2,?3,'read','{}','RESULT','success',?3,?4)",
        params![id, owner, seq, offset],
    )?;
    store.conn().execute(
        "INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES(?1,?2,?3)",
        params![owner, id, response],
    )?;
    Ok(seq)
}

fn append_turn(store: &Store) -> Result<(), rusqlite::Error> {
    insert_message(store, "next-task", "user", "NEXT_TASK")?;
    insert_message(store, "next-owner", "assistant", "new")?;
    insert_call(store, "next-call", "next-owner", "next-response", 3)?;
    store.conn().execute_batch(
        "INSERT INTO historical_message_delivery(message_id,thread_id,model_content) VALUES('next-task','thread','PROJECTED_NEXT');
         INSERT INTO historical_tool_receipts(call_id,thread_id,kind,source_thread_id,source_message_id,source_call_id,input_json,output_text,references_json)
             VALUES('next-call','thread','delegation','origin','next-origin-owner','next-origin-call','{}','NEXT_HISTORY','[]');
         INSERT INTO image_attachments(message_id,ordinal,encoded) VALUES('next-task',0,X'040506');",
    )
}

#[test]
fn append_fence_accepts_new_turns_and_returns_the_current_full_source() -> TestResult {
    let store = fixture(false)?;
    let (original, fence) = capture(&store, None)?;
    let encoded = fence.encode()?;
    assert!(!encoded.contains("PROJECTED_TASK"));
    assert!(!encoded.contains("RESULT"));
    let recovered = ContextAppendFence::decode(&encoded)?;
    assert_eq!(recovered, fence);
    assert_eq!(validate(&store, &recovered)?, original);
    append_turn(&store)?;
    let current = validate(&store, &recovered)?;
    assert_eq!(current.source_version, 4);
    assert_ne!(current.fingerprint, original.fingerprint);
    assert_eq!(current.messages.len(), 4);
    assert_eq!(current.tool_calls.len(), 2);
    assert_eq!(current.images.len(), 2);
    assert_eq!(
        current,
        crate::context_compaction::load_source(store.conn(), "thread")?
    );
    let checkpoints: i64 =
        store
            .conn()
            .query_row("SELECT COUNT(*) FROM context_checkpoints", [], |row| {
                row.get(0)
            })?;
    assert_eq!(checkpoints, 0);
    Ok(())
}

#[test]
fn append_fence_preserves_streaming_owner_prefix_and_complete_response_groups() -> TestResult {
    for terminal in [false, true] {
        let store = fixture(true)?;
        let (original, fence) = capture(&store, Some("owner"))?;
        store
            .conn()
            .execute("UPDATE messages SET content='好追加' WHERE id='owner'", [])?;
        assert_eq!(insert_call(&store, "call-2", "owner", "response-2", 9)?, 2);
        if terminal {
            store
                .conn()
                .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
            append_turn(&store)?;
        }
        let current = validate(&store, &fence)?;
        assert_eq!(
            current
                .messages
                .iter()
                .find(|row| row.id == "owner")
                .ok_or("owner missing")?
                .content,
            "好追加"
        );
        assert_ne!(current.fingerprint, original.fingerprint);
        store
            .conn()
            .execute("UPDATE messages SET content='坏追加' WHERE id='owner'", [])?;
        assert!(matches!(
            validate(&store, &fence),
            Err(ContextCheckpointError::Stale)
        ));
    }
    Ok(())
}

#[test]
fn append_fence_accepts_an_empty_owner_without_fabricating_a_response_cursor() -> TestResult {
    let store = fixture(true)?;
    store.conn().execute_batch(
        "DELETE FROM historical_tool_receipts;
         DELETE FROM tool_call_rounds;
         DELETE FROM tool_calls;
         UPDATE messages SET content='' WHERE id='owner';",
    )?;
    let (_, fence) = capture(&store, Some("owner"))?;
    assert!(
        fence
            .append_owner
            .as_ref()
            .is_some_and(|owner| owner.response.is_none())
    );
    store.conn().execute(
        "UPDATE messages SET content='first',status='done' WHERE id='owner'",
        [],
    )?;
    assert_eq!(
        insert_call(&store, "first-call", "owner", "first-response", 5)?,
        1
    );
    assert_eq!(validate(&store, &fence)?.tool_calls.len(), 1);
    Ok(())
}

#[test]
fn append_fence_rejects_edits_to_old_messages_calls_images_and_delivery_metadata() -> TestResult {
    for mutation in [
        "UPDATE messages SET content='edited' WHERE id='owner'",
        "UPDATE messages SET content='edited raw task' WHERE id='task'",
        "UPDATE messages SET plan_review_note='edited' WHERE id='task'",
        "UPDATE messages SET status='failed' WHERE id='owner'",
        "DELETE FROM messages WHERE id='task'",
        "UPDATE tool_calls SET output_text='edited' WHERE id='call-1'",
        "UPDATE tool_calls SET input_json='{\"edited\":true}' WHERE id='call-1'",
        "UPDATE tool_calls SET approval='changed' WHERE id='call-1'",
        "UPDATE tool_calls SET output_full_path='/synthetic/changed' WHERE id='call-1'",
        "UPDATE tool_call_rounds SET response_id='changed' WHERE call_id='call-1'",
        "UPDATE image_attachments SET encoded=X'070809' WHERE message_id='task'",
        "UPDATE historical_message_delivery SET model_content='changed' WHERE message_id='task'",
        "UPDATE historical_message_delivery SET previous_assistant_id='owner' WHERE message_id='task'",
        "UPDATE historical_tool_receipts SET references_json='[\"changed\"]' WHERE call_id='call-1'",
        "DELETE FROM historical_tool_receipts WHERE call_id='call-1'",
    ] {
        let store = fixture(false)?;
        let (_, fence) = capture(&store, None)?;
        store.conn().execute(mutation, [])?;
        assert!(
            matches!(validate(&store, &fence), Err(ContextCheckpointError::Stale)),
            "{mutation}"
        );
    }
    let store = fixture(false)?;
    let (source, fence) = capture(&store, None)?;
    let call = source.tool_calls.first().ok_or("call missing")?;
    store.conn().execute(
        "INSERT INTO context_micro_compactions(call_id,thread_id,source_digest,created_at) VALUES(?1,'thread',?2,1)",
        params![call.id, crate::context_micro::call_digest(call)],
    )?;
    assert!(matches!(
        validate(&store, &fence),
        Err(ContextCheckpointError::Stale)
    ));
    Ok(())
}

#[test]
fn append_fence_rejects_invalid_extensions_and_partial_capture_boundaries() -> TestResult {
    for mutation in [
        "UPDATE tool_call_rounds SET response_id='response-1' WHERE call_id='call-2'",
        "UPDATE tool_calls SET text_offset_bytes=0 WHERE id='call-2'",
        "UPDATE tool_calls SET output_text=NULL,status='running' WHERE id='call-2'",
        "UPDATE messages SET status='interrupted' WHERE id='owner'",
        "UPDATE messages SET content='好追加x' WHERE id='owner'",
    ] {
        let store = fixture(true)?;
        let (_, fence) = capture(&store, Some("owner"))?;
        store
            .conn()
            .execute("UPDATE messages SET content='好追加' WHERE id='owner'", [])?;
        assert_eq!(insert_call(&store, "call-2", "owner", "response-2", 9)?, 2);
        store.conn().execute(mutation, [])?;
        assert!(validate(&store, &fence).is_err(), "{mutation}");
    }
    let store = fixture(false)?;
    let (_, fence) = capture(&store, None)?;
    assert_eq!(
        insert_call(&store, "injected", "owner", "injected-response", 3)?,
        2
    );
    assert!(matches!(
        validate(&store, &fence),
        Err(ContextCheckpointError::Stale)
    ));
    for mutation in [
        "UPDATE messages SET content='好partial' WHERE id='owner'",
        "UPDATE tool_calls SET status='running',output_text=NULL WHERE id='call-1'",
        "UPDATE tool_calls SET text_offset_bytes=1 WHERE id='call-1'",
    ] {
        let store = fixture(true)?;
        store.conn().execute(mutation, [])?;
        assert!(capture(&store, Some("owner")).is_err(), "{mutation}");
    }
    Ok(())
}

#[test]
fn append_fence_tracks_screenshot_bytes_but_accepts_new_owner_artifacts() -> TestResult {
    let store = fixture(true)?;
    store.conn().execute_batch(
        "INSERT INTO image_attachments(message_id,ordinal,encoded) VALUES('owner',0,X'010203');
         INSERT INTO computer_screenshot_artifacts(call_id,message_id,ordinal) VALUES('call-1','owner',0);",
    )?;
    let (before, fence) = capture(&store, Some("owner"))?;
    assert_eq!(
        insert_call(&store, "screenshot-2", "owner", "response-2", 3)?,
        2
    );
    store.conn().execute_batch(
        "UPDATE tool_calls SET tool='ComputerScreenshot',output_text='RECEIPT' WHERE id='screenshot-2';
         INSERT INTO image_attachments(message_id,ordinal,encoded) VALUES('owner',1,X'040506');
         INSERT INTO computer_screenshot_artifacts(call_id,message_id,ordinal) VALUES('screenshot-2','owner',1);",
    )?;
    let appended = validate(&store, &fence)?;
    assert_eq!(appended.tool_calls.len(), 2);
    assert_ne!(before.fingerprint, appended.fingerprint);
    store.conn().execute(
        "UPDATE image_attachments SET encoded=X'0809' WHERE message_id='owner' AND ordinal=0",
        [],
    )?;
    assert!(matches!(
        validate(&store, &fence),
        Err(ContextCheckpointError::Stale)
    ));
    let current = crate::context_compaction::load_source(store.conn(), "thread")?;
    assert_eq!(appended.fingerprint, current.fingerprint);
    Ok(())
}

#[test]
fn append_fence_uses_independent_allocators_when_either_sequence_space_leads() -> TestResult {
    for tools_lead in [false, true] {
        let store = fixture(false)?;
        for index in 0..6 {
            if tools_lead {
                insert_call(
                    &store,
                    &format!("old-call-{index}"),
                    "owner",
                    &format!("old-response-{index}"),
                    3,
                )?;
            } else {
                insert_message(&store, &format!("old-message-{index}"), "user", "old task")?;
            }
        }
        let (source, fence) = capture(&store, None)?;
        assert_eq!(fence.through_message_seq, if tools_lead { 2 } else { 8 });
        assert_eq!(fence.through_tool_seq, if tools_lead { 7 } else { 1 });
        let expected_message = crate::messages::next_seq(store.conn(), "thread")?;
        assert_eq!(
            insert_message(&store, "new-task", "user", "new task")?,
            expected_message
        );
        let with_task = validate(&store, &fence)?;
        assert_eq!(with_task.messages.len(), source.messages.len() + 1);
        if tools_lead {
            assert_eq!(with_task.source_version, source.source_version);
        }
        assert_eq!(
            insert_message(&store, "new-owner", "assistant", "new reply")?,
            expected_message + 1
        );
        let expected_call = crate::tool_calls::next_seq(store.conn(), "thread")?;
        assert_eq!(
            insert_call(&store, "new-call", "new-owner", "new-response", 9)?,
            expected_call
        );
        let current = validate(&store, &fence)?;
        assert_eq!(current.messages.len(), source.messages.len() + 2);
        assert_eq!(current.tool_calls.len(), source.tool_calls.len() + 1);
        store.conn().execute(
            "UPDATE messages SET content='changed old task' WHERE id='task'",
            [],
        )?;
        assert!(matches!(
            validate(&store, &fence),
            Err(ContextCheckpointError::Stale)
        ));
    }
    Ok(())
}

#[test]
fn append_fence_decoding_is_versioned_bounded_and_closed() -> TestResult {
    let store = fixture(false)?;
    let (_, fence) = capture(&store, None)?;
    let value = serde_json::to_value(&fence)?;
    for (field, replacement) in [
        ("schema", serde_json::json!(2)),
        ("through_message_seq", serde_json::json!(u64::MAX)),
        ("through_tool_seq", serde_json::json!(u64::MAX)),
        ("fingerprint", serde_json::json!("invalid")),
        ("unknown", serde_json::json!(true)),
        ("thread_id", serde_json::json!("")),
    ] {
        let mut changed = value.clone();
        changed[field] = replacement;
        assert!(ContextAppendFence::decode(&serde_json::to_string(&changed)?).is_err());
    }
    assert!(ContextAppendFence::decode(&" ".repeat(32 * 1024 + 1)).is_err());
    Ok(())
}
