use super::*;
use crate::Store;
use rusqlite::params;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(streaming: bool) -> Result<Store, Box<dyn std::error::Error>> {
    let store = Store::open(":memory:")?;
    store.migrate()?;
    store.conn().execute_batch(
        "INSERT INTO threads(id,model,created_at,updated_at) VALUES('thread','model',1,1);
         INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES
             ('task','thread',1,'user','RAW_TASK','done',1),
             ('owner','thread',2,'assistant','好','done',2);
         INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes)
             VALUES('call-1','thread','owner',3,'read','{}','RESULT','success',3,3);
         INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('owner','call-1','response-1');
         INSERT INTO historical_message_delivery(message_id,thread_id,model_content)
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

fn append_turn(store: &Store) -> Result<(), rusqlite::Error> {
    store.conn().execute_batch(
        "INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES
             ('next-task','thread',10,'user','NEXT_TASK','done',10),
             ('next-owner','thread',11,'assistant','new','done',11);
         INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes)
             VALUES('next-call','thread','next-owner',12,'read','{}','NEXT_RESULT','success',12,3);
         INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('next-owner','next-call','next-response');
         INSERT INTO historical_message_delivery(message_id,thread_id,model_content) VALUES('next-task','thread','PROJECTED_NEXT');
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
    assert_eq!(current.source_version, 12);
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
        store.conn().execute_batch(
            "UPDATE messages SET content='好追加' WHERE id='owner';
             INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes)
                 VALUES('call-2','thread','owner',4,'read','{}','SECOND','success',4,9);
             INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('owner','call-2','response-2');",
        )?;
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
    store.conn().execute_batch(
        "UPDATE messages SET content='first',status='done' WHERE id='owner';
         INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes)
             VALUES('first-call','thread','owner',3,'read','{}','FIRST','success',3,5);
         INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('owner','first-call','first-response');",
    )?;
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
        store.conn().execute_batch(
            "UPDATE messages SET content='好追加' WHERE id='owner';
             INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes)
                 VALUES('call-2','thread','owner',4,'read','{}','SECOND','success',4,9);
             INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('owner','call-2','response-2');",
        )?;
        store.conn().execute(mutation, [])?;
        assert!(validate(&store, &fence).is_err(), "{mutation}");
    }
    let store = fixture(false)?;
    let (_, fence) = capture(&store, None)?;
    store.conn().execute_batch(
        "INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes)
             VALUES('injected','thread','owner',4,'read','{}','SECOND','success',4,3);",
    )?;
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
    store.conn().execute_batch(
        "INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes)
             VALUES('screenshot-2','thread','owner',4,'ComputerScreenshot','{}','RECEIPT','success',4,3);
         INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('owner','screenshot-2','response-2');
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
fn append_fence_decoding_is_versioned_bounded_and_closed() -> TestResult {
    let store = fixture(false)?;
    let (_, fence) = capture(&store, None)?;
    let value = serde_json::to_value(&fence)?;
    for (field, replacement) in [
        ("schema", serde_json::json!(2)),
        ("through_seq", serde_json::json!(u64::MAX)),
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
