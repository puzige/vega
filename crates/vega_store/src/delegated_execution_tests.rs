use super::*;
use rusqlite::params;

type LegacyValues = (String, String, String, bool, bool, i64);
type ExecutionIdentity = (
    String,
    String,
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
);

fn legacy_store() -> Store {
    let store = Store::open(":memory:").unwrap();
    store
        .conn()
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    for migration in &MIGRATIONS[..28] {
        store.conn().execute_batch(migration).unwrap();
    }
    store
        .conn()
        .pragma_update(None, "user_version", 28)
        .unwrap();
    store
        .conn()
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    thread(store.conn(), "parent");
    store
}

fn thread(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO threads(id,title,model,created_at,updated_at) VALUES(?1,?1,'fixture',10,10)",
        [id],
    )
    .unwrap();
}

fn old_child(conn: &Connection, id: &str, state: &str, usage: bool, instruction: bool) {
    thread(conn, id);
    if instruction {
        conn.execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'user','Original task','done',11)", params![format!("{id}-input"),id]).unwrap();
    }
    conn.execute("INSERT INTO delegated_runs(child_thread_id,parent_thread_id,parent_message_id,spawn_call_id,child_message_id,name,task,state,result,truncated,usage_complete,created_at) VALUES(?1,'parent','parent-run',?2,?3,?4,'Original task',?5,?6,?7,?8,12)", params![id,format!("{id}-spawn"),format!("{id}-first"),format!("Worker {id}"),state,format!("Original {id}"),id == "failed",usage]).unwrap();
    conn.execute("UPDATE threads SET parent_thread_id='parent',workspace_owner_thread_id='parent' WHERE id=?1", [id]).unwrap();
}

fn insert_followup(conn: &Connection, child: &str, state: &str) {
    conn.execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,3,'user','Follow-up task','done',20)", params![format!("{child}-next-input"),child]).unwrap();
    conn.execute("INSERT INTO delegated_executions(child_thread_id,child_message_id,parent_thread_id,parent_message_id,request_call_id,ordinal,previous_child_message_id,kind,instruction_message_id,name,task,state,result,truncated,usage_complete,created_at) VALUES(?1,?2,'parent','parent-run',?3,2,?4,'followup',?5,?6,'Follow-up task',?7,'Follow-up result',0,0,21)",params![child,format!("{child}-second"),format!("{child}-followup"),format!("{child}-first"),format!("{child}-next-input"),format!("Worker {child}"),state]).unwrap();
}

#[test]
fn upgrade_preserves_every_original_execution_and_missing_instruction() {
    let store = legacy_store();
    for (id, state, known, instruction) in [
        ("completed", "completed", true, true),
        ("failed", "failed", false, true),
        ("interrupted", "interrupted", false, true),
        ("missing", "completed", false, false),
    ] {
        old_child(store.conn(), id, state, known, instruction);
    }
    let legacy: Vec<LegacyValues> = store.conn().prepare(
        "SELECT child_thread_id,state,result,truncated,usage_complete,created_at FROM delegated_runs ORDER BY child_thread_id"
    ).unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).unwrap().map(Result::unwrap).collect();
    store.migrate().unwrap();
    let upgraded: Vec<LegacyValues> = store.conn().prepare(
        "SELECT child_thread_id,state,result,truncated,usage_complete,created_at FROM delegated_executions ORDER BY child_thread_id"
    ).expect("upgrading must install execution history").query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(upgraded, legacy);
    let identities: Vec<ExecutionIdentity> = store.conn().prepare(
        "SELECT child_thread_id,child_message_id,request_call_id,ordinal,kind,previous_child_message_id,instruction_message_id FROM delegated_executions ORDER BY child_thread_id"
    ).unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?))).unwrap().map(Result::unwrap).collect();
    for (child, message, call, ordinal, kind, previous, instruction) in identities {
        assert_eq!(message, format!("{child}-first"));
        assert_eq!(call, format!("{child}-spawn"));
        assert_eq!(ordinal, 1);
        assert_eq!(kind, "spawn");
        assert!(previous.is_none());
        assert_eq!(
            instruction,
            (child != "missing").then(|| format!("{child}-input"))
        );
    }
    store.migrate().unwrap();
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM delegated_executions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 4);
    assert!(subagents::has_unknown_usage(store.conn(), "parent").unwrap());
    let original_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM delegated_runs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(original_count, 4);
}

#[test]
fn execution_constraints_prevent_duplicate_calls_and_two_active_generations() {
    let store = legacy_store();
    old_child(store.conn(), "worker", "completed", true, true);
    store.migrate().unwrap();
    insert_followup(store.conn(), "worker", "running");
    let duplicate = store.conn().execute("INSERT INTO delegated_executions(child_thread_id,child_message_id,parent_thread_id,parent_message_id,request_call_id,ordinal,previous_child_message_id,kind,instruction_message_id,name,task,state,result,truncated,usage_complete,created_at) SELECT child_thread_id,'duplicate',parent_thread_id,parent_message_id,request_call_id,3,child_message_id,kind,instruction_message_id,name,task,'completed',result,truncated,usage_complete,created_at FROM delegated_executions WHERE child_message_id='worker-second'", []);
    assert!(duplicate.is_err());
    let concurrent = store.conn().execute("INSERT INTO delegated_executions(child_thread_id,child_message_id,parent_thread_id,parent_message_id,request_call_id,ordinal,previous_child_message_id,kind,instruction_message_id,name,task,state,result,truncated,usage_complete,created_at) SELECT child_thread_id,'concurrent',parent_thread_id,parent_message_id,'third-call',3,child_message_id,kind,instruction_message_id,name,task,'running',result,truncated,usage_complete,created_at FROM delegated_executions WHERE child_message_id='worker-second'", []);
    assert!(concurrent.is_err());
    assert_eq!(
        subagents::active_count(store.conn(), "parent", "parent-run").unwrap(),
        1
    );
    assert!(!subagents::request_cancel(store.conn(), "worker", "worker-first").unwrap());
    assert!(subagents::request_cancel(store.conn(), "worker", "worker-second").unwrap());
    assert!(
        subagents::finish(
            store.conn(),
            "worker",
            "worker-second",
            "interrupted",
            "Stopped second",
            false
        )
        .unwrap()
    );
    let first = subagents::find(store.conn(), "worker").unwrap().unwrap();
    assert_eq!(first.child_message_id, "worker-second");
    assert_eq!(first.state, "interrupted");
    let original: String = store
        .conn()
        .query_row(
            "SELECT result FROM delegated_runs WHERE child_thread_id='worker'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(original, "Original worker");
}

#[test]
fn recovery_interrupts_only_the_current_execution_and_keeps_unknown_usage() {
    let store = legacy_store();
    old_child(store.conn(), "worker", "completed", true, true);
    store.migrate().unwrap();
    insert_followup(store.conn(), "worker", "running");
    store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('worker-second','worker',4,'assistant','Partial output','streaming',21)", []).unwrap();
    let recovered = recovery::recover_thread(store.conn(), "worker", 30).unwrap();
    assert_eq!(recovered.messages_interrupted, 1);
    assert_eq!(
        subagents::active_count(store.conn(), "parent", "parent-run").unwrap(),
        0
    );
    let records: Vec<(String, String)> = store.conn().prepare("SELECT state,result FROM delegated_executions WHERE child_thread_id='worker' ORDER BY ordinal").unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(records[0], ("completed".into(), "Original worker".into()));
    assert_eq!(records[1].0, "interrupted");
    assert!(!records[1].1.is_empty());
    assert!(subagents::has_unknown_usage(store.conn(), "parent").unwrap());
    assert!(!subagents::run_usage_complete(store.conn(), "parent", "parent-run").unwrap());
    let repeated = recovery::recover_thread(store.conn(), "worker", 31).unwrap();
    assert_eq!(repeated.messages_interrupted, 0);
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM delegated_executions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn privacy_scans_fingerprints_and_cleans_each_execution_with_active_guard() {
    let store = legacy_store();
    old_child(store.conn(), "worker", "completed", true, true);
    store.migrate().unwrap();
    insert_followup(store.conn(), "worker", "running");
    store.conn().execute("UPDATE delegated_executions SET task='sensitive-input',result='sensitive-output' WHERE child_message_id='worker-second'", []).unwrap();
    let before = credential_privacy::source_fingerprint(store.conn(), "parent").unwrap();
    let fields = credential_privacy::fields(store.conn(), "parent").unwrap();
    assert!(
        fields
            .iter()
            .any(|(_, _, value)| value == "sensitive-input")
    );
    assert!(
        fields
            .iter()
            .any(|(_, _, value)| value == "sensitive-output")
    );
    assert!(
        credential_privacy::clean(store.conn(), "parent", &before, |text, offsets| (
            text.replace("sensitive", "redacted"),
            offsets.to_vec()
        ))
        .is_err()
    );
    store.conn().execute("UPDATE delegated_executions SET state='completed' WHERE child_message_id='worker-second'", []).unwrap();
    let after = credential_privacy::source_fingerprint(store.conn(), "parent").unwrap();
    assert_ne!(before, after);
    credential_privacy::clean(store.conn(), "parent", &after, |text, offsets| {
        (text.replace("sensitive", "redacted"), offsets.to_vec())
    })
    .unwrap();
    let texts: (String, String) = store
        .conn()
        .query_row(
            "SELECT task,result FROM delegated_executions WHERE child_message_id='worker-second'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(texts, ("redacted-input".into(), "redacted-output".into()));
    let original: String = store
        .conn()
        .query_row(
            "SELECT result FROM delegated_executions WHERE child_message_id='worker-first'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(original, "Original worker");
    assert!(
        credential_privacy::fields(store.conn(), "parent")
            .unwrap()
            .iter()
            .all(|(_, _, value)| !value.contains("sensitive"))
    );
}

#[test]
fn execution_usage_is_counted_once_and_completeness_is_not_inherited() {
    let store = legacy_store();
    old_child(store.conn(), "worker", "completed", true, true);
    store.migrate().unwrap();
    insert_followup(store.conn(), "worker", "completed");
    for (message, input, output, cost) in
        [("worker-first", 11, 7, 30), ("worker-second", 13, 5, 40)]
    {
        let row = token_usage::insert(
            store.conn(),
            token_usage::NewTokenUsage {
                thread_id: "worker",
                message_id: Some(message),
                model: "fixture",
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_microcents: cost,
                created_at: 20,
                pricing_version: Some(token_usage::PRICED_VERSION),
                pricing_profile: Some("base"),
                call_started_at: Some(20),
            },
        )
        .unwrap();
        subagents::attribute_usage(store.conn(), row, "worker", "parent", "parent-run", true)
            .unwrap();
    }
    let aggregate = token_usage::aggregate_task_by_thread(store.conn(), "parent").unwrap();
    assert_eq!(aggregate.row_count, 2);
    assert_eq!(aggregate.input_tokens, 24);
    assert_eq!(aggregate.output_tokens, 12);
    assert_eq!(aggregate.cost, token_usage::AggregateCost::Priced(70));
    assert!(subagents::has_unknown_usage(store.conn(), "parent").unwrap());
    subagents::mark_usage_complete(store.conn(), "worker", "worker-second", true).unwrap();
    assert!(!subagents::has_unknown_usage(store.conn(), "parent").unwrap());
    assert!(subagents::run_usage_complete(store.conn(), "parent", "parent-run").unwrap());
    subagents::mark_usage_complete(store.conn(), "worker", "worker-first", false).unwrap();
    assert!(subagents::has_unknown_usage(store.conn(), "parent").unwrap());
    assert!(!subagents::run_usage_complete(store.conn(), "parent", "parent-run").unwrap());
    threads::delete_thread(store.conn(), "parent").unwrap();
    let remaining: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM delegated_executions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(remaining, 0);
}
