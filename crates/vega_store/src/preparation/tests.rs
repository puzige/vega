use super::*;
use crate::{Store, context_compaction, delegated_stages, messages};
use rusqlite::params;
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture() -> rusqlite::Result<Store> {
    let store = Store::open(":memory:")?;
    store.migrate()?;
    for id in ["child", "other", "parent"] {
        store.conn().execute(
            "INSERT INTO threads(id,model,created_at,updated_at) VALUES(?1,'model',1,1)",
            [id],
        )?;
    }
    store.conn().execute_batch(
        r#"INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES
        ('user','child',1,'user','task','done',1),
        ('old','child',2,'assistant','secret final','done',2),
        ('other-user','other',1,'user','other task','done',1);
        INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at) VALUES('call','child','old',1,'read','{}','secret output','success',2);
        INSERT INTO image_attachments(message_id,ordinal,encoded) VALUES('user',0,x'0102');
        INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('old','response',0,12,'final');
        INSERT INTO delegated_stage_checkpoints(id,child_thread_id,child_message_id,payload,authority,created_at) VALUES('checkpoint','child','old','{"summary":"secret"}','authority',1);
        INSERT INTO delegated_execution_recovery(child_thread_id,child_message_id,release_state,authority,reconciliations) VALUES('child','next','held','authority','[]');
        INSERT INTO delegated_execution_messages(message_id,child_thread_id,execution_message_id) VALUES('next','child','next');"#,
    )?;
    Ok(store)
}

fn assistant(conn: &Connection, thread: &str) -> rusqlite::Result<messages::MessageRow> {
    Ok(messages::MessageRow {
        id: "next".into(),
        thread_id: thread.into(),
        seq: messages::next_seq(conn, thread)?,
        role: "assistant".into(),
        kind: "text".into(),
        content: String::new(),
        status: "streaming".into(),
        created_at: 3,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    })
}

fn prepare(conn: &Connection) -> rusqlite::Result<PreparedDelegatedInstall<'_>> {
    let snapshot = DelegatedSnapshot::capture(conn, "child", "next")?;
    let legacy = context_compaction::prepare_legacy_thread_redaction(
        snapshot.connection(),
        "child",
        |text, offsets| (text.replace("secret", "hidden"), offsets.to_vec()),
    )?;
    let payloads =
        delegated_stages::prepare_payload_redaction(snapshot.connection(), "child", |text| {
            text.replace("secret", "hidden")
        })?;
    let assistant = assistant(snapshot.connection(), "child")?;
    snapshot.prepare_install(legacy, payloads, assistant)
}

#[test]
fn prepared_source_equals_committed_canonical_source() -> TestResult {
    let store = fixture()?;
    let prepared = prepare(store.conn())?;
    let projection = prepared.connection().unchecked_transaction()?;
    let expected = context_compaction::load_source_in_transaction(&projection, "child")?;
    projection.commit()?;
    assert_eq!(
        expected
            .messages
            .iter()
            .find(|row| row.id == "old")
            .map(|row| row.content.as_str()),
        Some("hidden final")
    );
    assert_eq!(expected.images.len(), 1);
    let mut tx = store.immediate_transaction()?;
    assert!(prepared.install(&mut tx)?);
    messages::insert(&tx, prepared.assistant())?;
    tx.commit()?;
    let tx = store.conn().unchecked_transaction()?;
    let actual = context_compaction::load_source_in_transaction(&tx, "child")?;
    assert!(actual == expected);
    assert_eq!(
        messages::completed_response_text(&tx, "old")?.as_deref(),
        Some("hidden final")
    );
    tx.commit()?;
    Ok(())
}

#[test]
fn same_sequence_dependency_edits_reject_without_partial_install() -> TestResult {
    for sql in [
        "UPDATE messages SET content='changed' WHERE id='old'",
        "UPDATE tool_calls SET output_text='changed' WHERE id='call'",
        "UPDATE tool_calls SET approval='changed' WHERE id='call'",
        "UPDATE image_attachments SET encoded=x'0304' WHERE message_id='user'",
        "UPDATE assistant_responses SET text_start_bytes=1 WHERE message_id='old'",
        "UPDATE delegated_stage_checkpoints SET payload='{}' WHERE id='checkpoint'",
        "UPDATE delegated_execution_recovery SET reconciliations='[1]' WHERE child_message_id='next'",
        "DELETE FROM tool_calls WHERE id='call'",
        "INSERT INTO pending_messages(id,thread_id,ordinal,behavior,generation,payload,model_content,state,created_at) VALUES('user','child',1,'queue','generation','{}','projected task','delivered',1)",
        "INSERT INTO historical_message_delivery(message_id,thread_id,model_content) VALUES('user','child','historical task')",
        "INSERT INTO historical_tool_receipts(call_id,thread_id,kind,source_thread_id,source_message_id,source_call_id,input_json,output_text,references_json) VALUES('call','child','delegation','parent','parent-message','parent-call','{}','receipt','[]')",
        "INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('old','call','round')",
        "INSERT INTO context_micro_compactions(call_id,thread_id,source_digest,created_at) VALUES('call','child','digest',1)",
        "INSERT INTO computer_screenshot_artifacts(call_id,message_id,ordinal) VALUES('call','user',0)",
        "INSERT INTO assistant_response_starts(message_id,response_id,ordinal,text_start_bytes,started_at_ms) VALUES('old','response',0,0,1)",
        "INSERT INTO message_timestamp_boundaries(message_id,operation_key,text_start_bytes) VALUES('old','operation',0)",
        "INSERT INTO context_checkpoints(thread_id,model,source_version,covered_through_seq,source_fingerprint,summary,estimator_version,created_at) VALUES('child','model',1,1,'fingerprint','summary','estimator',1)",
        "UPDATE threads SET model='changed' WHERE id='child'",
        "INSERT INTO delegated_executions(child_message_id,child_thread_id,parent_thread_id,parent_message_id,request_call_id,ordinal,kind,instruction_message_id,name,task,state,created_at) VALUES('next','child','parent','parent-message','parent-call',1,'spawn','user','agent','task','running',1)",
        "UPDATE messages SET thread_id='other',seq=2 WHERE id='old'",
        "UPDATE delegated_execution_messages SET execution_message_id='old' WHERE message_id='next'",
    ] {
        let store = fixture()?;
        let prepared = prepare(store.conn())?;
        if sql.contains("execution_message_id='old'") {
            store.conn().pragma_update(None, "foreign_keys", false)?;
        }
        store.conn().execute_batch(sql)?;
        let mut tx = store.immediate_transaction()?;
        assert!(prepared.install(&mut tx).is_err(), "{sql}");
        tx.rollback()?;
        assert!(messages::find(store.conn(), "next")?.is_none());
        let payload: String = store.conn().query_row(
            "SELECT payload FROM delegated_stage_checkpoints WHERE id='checkpoint'",
            [],
            |row| row.get(0),
        )?;
        assert!(!payload.contains("hidden"));
    }
    Ok(())
}

#[test]
fn change_then_restore_and_replace_invalidate_old_and_new_owners() -> TestResult {
    let store = fixture()?;
    let prepared = prepare(store.conn())?;
    let other = revision(store.conn(), "other")?;
    store.conn().execute_batch("UPDATE messages SET content='temporary' WHERE id='old'; UPDATE messages SET content='secret final' WHERE id='old';")?;
    let tx = store.immediate_transaction()?;
    assert!(!prepared.validate(&tx)?);
    tx.rollback()?;
    let prepared = prepare(store.conn())?;
    store.conn().execute_batch("INSERT OR REPLACE INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('old','other',2,'assistant','secret final','done',2)")?;
    assert!(revision(store.conn(), "other")? > other);
    let tx = store.immediate_transaction()?;
    assert!(!prepared.validate(&tx)?);
    tx.rollback()?;
    Ok(())
}

#[test]
fn parent_appends_do_not_invalidate_child_projection() -> TestResult {
    let store = fixture()?;
    let prepared = prepare(store.conn())?;
    store.conn().execute_batch("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('parent-event','parent',1,'assistant','event','streaming',4)")?;
    let mut tx = store.immediate_transaction()?;
    assert!(prepared.validate(&tx)?);
    prepared.install(&mut tx)?;
    messages::insert(&tx, prepared.assistant())?;
    tx.commit()?;
    Ok(())
}

#[test]
fn installation_sql_failure_rolls_back_payload_and_message_updates() -> TestResult {
    let store = fixture()?;
    store.conn().execute_batch("CREATE TRIGGER reject_prepared_tool BEFORE UPDATE ON tool_calls BEGIN SELECT RAISE(ABORT,'injected fixture failure'); END;")?;
    let prepared = prepare(store.conn())?;
    let before = revision(store.conn(), "child")?;
    let mut tx = store.immediate_transaction()?;
    assert!(prepared.install(&mut tx).is_err());
    tx.rollback()?;
    assert_eq!(revision(store.conn(), "child")?, before);
    assert_eq!(
        messages::find(store.conn(), "old")?.map(|row| row.content),
        Some("secret final".into())
    );
    let payload: String = store.conn().query_row(
        "SELECT payload FROM delegated_stage_checkpoints WHERE id='checkpoint'",
        [],
        |row| row.get(0),
    )?;
    assert!(payload.contains("secret"));
    assert!(messages::find(store.conn(), "next")?.is_none());
    Ok(())
}

#[test]
fn revision_overflow_cannot_be_ignored_and_tombstones_survive_recreation() -> TestResult {
    let store = fixture()?;
    let before = revision(store.conn(), "child")?;
    store.conn().pragma_update(None, "foreign_keys", false)?;
    store
        .conn()
        .execute("DELETE FROM threads WHERE id='child'", [])?;
    assert!(revision(store.conn(), "child")? > before);
    let deleted = revision(store.conn(), "child")?;
    store.conn().execute(
        "INSERT INTO threads(id,model,created_at,updated_at) VALUES('child','model',1,1)",
        [],
    )?;
    assert!(revision(store.conn(), "child")? > deleted);
    store.conn().execute(
        "UPDATE preparation_revisions SET revision=?1 WHERE thread_id='child'",
        [i64::MAX],
    )?;
    for prefix in ["UPDATE", "UPDATE OR IGNORE", "UPDATE OR REPLACE"] {
        assert!(
            store
                .conn()
                .execute(
                    &format!("{prefix} messages SET content='changed' WHERE id='old'"),
                    []
                )
                .is_err()
        );
        assert_eq!(
            messages::find(store.conn(), "old")?.map(|row| row.content),
            Some("secret final".into())
        );
    }
    assert!(
        store
            .conn()
            .execute(
                "DELETE FROM preparation_revisions WHERE thread_id='child'",
                []
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn missing_revision_cannot_silently_reset_an_existing_scope() -> TestResult {
    let store = fixture()?;
    store.conn().execute_batch("DROP TRIGGER preparation_revision_delete; DELETE FROM preparation_revisions WHERE thread_id='child';")?;
    for prefix in ["UPDATE", "UPDATE OR IGNORE", "UPDATE OR REPLACE"] {
        assert!(
            store
                .conn()
                .execute(
                    &format!("{prefix} messages SET content='changed' WHERE id='old'"),
                    []
                )
                .is_err()
        );
        assert_eq!(
            messages::find(store.conn(), "old")?.map(|row| row.content),
            Some("secret final".into())
        );
    }
    assert!(revision(store.conn(), "child").is_err());
    Ok(())
}

#[test]
fn migration_initializes_existing_threads_and_missing_schema_fails_closed() -> TestResult {
    let store = Store::open(":memory:")?;
    for (index, sql) in crate::MIGRATIONS.iter().take(59).enumerate() {
        if index == 4 {
            store.conn().pragma_update(None, "foreign_keys", false)?;
        }
        store.conn().execute_batch(sql)?;
        if index == 4 {
            store.conn().pragma_update(None, "foreign_keys", true)?;
        }
        if index == 42 {
            let tx = store.conn().unchecked_transaction()?;
            crate::permission_contracts::initialize(&tx)?;
            tx.commit()?;
        }
    }
    store.conn().execute_batch("INSERT INTO threads(id,model,created_at,updated_at) VALUES('existing','model',1,1); PRAGMA user_version=59;")?;
    store.migrate()?;
    assert_eq!(revision(store.conn(), "existing")?, 0);
    assert!(DelegatedSnapshot::capture(store.conn(), "existing", "next").is_ok());
    store
        .conn()
        .execute_batch("DROP TRIGGER preparation_messages_update")?;
    assert!(DelegatedSnapshot::capture(store.conn(), "existing", "next").is_err());
    Ok(())
}

#[test]
fn projection_is_private_preserves_rowids_and_rejects_other_database() -> TestResult {
    let store = fixture()?;
    store
        .conn()
        .execute_batch("UPDATE delegated_stage_checkpoints SET rowid=100 WHERE id='checkpoint'")?;
    let prepared = prepare(store.conn())?;
    let copied: i64 = prepared.connection().query_row(
        "SELECT rowid FROM delegated_stage_checkpoints WHERE id='checkpoint'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(copied, 100);
    let other_rows: i64 = prepared.connection().query_row(
        "SELECT COUNT(*) FROM messages WHERE thread_id<>'child'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(other_rows, 0);
    assert!(
        prepared
            .connection()
            .prepare("SELECT * FROM config")
            .is_err()
    );
    let other_store = fixture()?;
    let tx = other_store.immediate_transaction()?;
    assert!(!prepared.validate(&tx)?);
    tx.rollback()?;
    Ok(())
}

#[test]
fn lock_free_preparation_allows_another_connection_to_commit() -> TestResult {
    let root = std::env::temp_dir().join(format!("vega-preparation-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root)?;
    let path = root.join("fixture.db");
    let store = Store::open(&path)?;
    store.migrate()?;
    store.conn().execute_batch("INSERT INTO threads(id,model,created_at,updated_at) VALUES('child','model',1,1),('parent','model',1,1)")?;
    let snapshot = DelegatedSnapshot::capture(store.conn(), "child", "next")?;
    let other = Store::open(&path)?;
    let tx = other.immediate_transaction()?;
    tx.execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('event','parent',1,'assistant','event','done',1)", [])?;
    tx.commit()?;
    assert_eq!(messages::next_seq(snapshot.connection(), "child")?, 1);
    drop(snapshot);
    drop(other);
    drop(store);
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn controlled_projection_writer_comparison() -> TestResult {
    let store = fixture()?;
    let tx = store.immediate_transaction()?;
    for index in 3..1003 {
        tx.execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,'child',?2,'assistant',?3,'done',?2)", params![format!("history-{index}"), index, "history payload ".repeat(128)])?;
    }
    tx.commit()?;
    let mut old_holds = Vec::<Duration>::new();
    let mut new_holds = Vec::<Duration>::new();
    let mut old_totals = Vec::<Duration>::new();
    let mut new_totals = Vec::<Duration>::new();
    for _ in 0..12 {
        let start = Instant::now();
        let legacy = context_compaction::prepare_legacy_thread_redaction(
            store.conn(),
            "child",
            |text, offsets| (text.to_owned(), offsets.to_vec()),
        )?;
        let payloads =
            delegated_stages::prepare_payload_redaction(store.conn(), "child", str::to_owned)?;
        let mut tx = store.immediate_transaction()?;
        let held = Instant::now();
        assert!(legacy.validate(&tx)?);
        payloads.install(&mut tx)?;
        legacy.install(&mut tx)?;
        messages::insert(&tx, &assistant(&tx, "child")?)?;
        let source = context_compaction::load_source_in_transaction(&tx, "child")?;
        assert_eq!(source.messages.len(), 1003);
        tx.rollback()?;
        old_holds.push(held.elapsed());
        old_totals.push(start.elapsed());
        let start = Instant::now();
        let snapshot = DelegatedSnapshot::capture(store.conn(), "child", "next")?;
        let legacy = context_compaction::prepare_legacy_thread_redaction(
            snapshot.connection(),
            "child",
            |text, offsets| (text.to_owned(), offsets.to_vec()),
        )?;
        let payloads = delegated_stages::prepare_payload_redaction(
            snapshot.connection(),
            "child",
            str::to_owned,
        )?;
        let row = assistant(snapshot.connection(), "child")?;
        let prepared = snapshot.prepare_install(legacy, payloads, row)?;
        let projection = prepared.connection().unchecked_transaction()?;
        let source = context_compaction::load_source_in_transaction(&projection, "child")?;
        assert_eq!(source.messages.len(), 1003);
        projection.commit()?;
        let mut tx = store.immediate_transaction()?;
        let held = Instant::now();
        prepared.install(&mut tx)?;
        messages::insert(&tx, prepared.assistant())?;
        tx.rollback()?;
        new_holds.push(held.elapsed());
        new_totals.push(start.elapsed());
    }
    for values in [
        &mut old_holds,
        &mut new_holds,
        &mut old_totals,
        &mut new_totals,
    ] {
        values.sort();
    }
    let report = format!(
        "Controlled in-memory store projection comparison, 12 samples, 1002 existing messages. p95 nearest-rank: old writer {:?}, new writer {:?}; old total {:?}, new total {:?}. Writer reduction {:.2}%. Excludes conversation authority, history conversion, terminal approval validation, filesystem final checks and competing-writer wait; not a field latency estimate.\n",
        old_holds[11],
        new_holds[11],
        old_totals[11],
        new_totals[11],
        100.0 * (1.0 - new_holds[11].as_secs_f64() / old_holds[11].as_secs_f64())
    );
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(summary)?
            .write_all(report.as_bytes())?;
    }
    Ok(())
}
