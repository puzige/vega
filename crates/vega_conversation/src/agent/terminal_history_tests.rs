use super::*;

fn fixture(tool: &str) -> Store {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    store.conn().execute_batch(
        "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project','/fixture','fixture',1,1);
         INSERT INTO threads(id,project_id,model,created_at,updated_at) VALUES('source','project','fixture',1,1);
         INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('answer','source',1,'assistant','done','done',1);
         INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('answer','final',0,4,'final');"
    ).unwrap();
    insert_mutation(
        &store, "source", "answer", "original", 1, tool, "source", "original",
    );
    store
}

#[allow(clippy::too_many_arguments)]
fn insert_mutation(
    store: &Store,
    thread: &str,
    message: &str,
    call: &str,
    seq: i64,
    tool: &str,
    checkpoint_thread: &str,
    checkpoint_call: &str,
) {
    let checkpoint_ref =
        vega_tools::CheckpointIds::new("project", checkpoint_thread, checkpoint_call)
            .unwrap()
            .checkpoint_ref();
    let (audit, output) = if tool == "write" {
        (
            vega_tools::WriteEditAudit::Write {
                path: "/fixture/file".into(),
                content_bytes: 4,
                expected_written_bytes: None,
                fingerprint_v1: "a".repeat(64),
            },
            vega_tools::WriteSuccessOutput {
                path: "/fixture/file".into(),
                bytes_written: 4,
                checkpoint_ref,
            }
            .to_json()
            .unwrap(),
        )
    } else {
        (
            vega_tools::WriteEditAudit::Edit {
                path: "/fixture/file".into(),
                old_string_bytes: 3,
                new_string_bytes: 4,
                edit_count: Some(1),
                fingerprint_v1: "b".repeat(64),
            },
            vega_tools::EditSuccessOutput {
                path: "/fixture/file".into(),
                bytes_written: 4,
                replacements: 1,
                checkpoint_ref,
            }
            .to_json()
            .unwrap(),
        )
    };
    store.conn().execute(
        "INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,approval,created_at,finished_at,text_offset_bytes) VALUES(?1,?2,?3,?4,?5,?6,?7,'success','{\"decision\":\"once\",\"note\":null,\"source\":\"full_access\",\"danger\":null}',1,2,0)",
        (&call, &thread, &message, seq, tool, audit.to_json().unwrap(), output),
    ).unwrap();
}

fn fork(store: &Store, source: &str) -> String {
    let answer: String = store
        .conn()
        .query_row(
            "SELECT id FROM messages WHERE thread_id=?1 ORDER BY seq DESC LIMIT 1",
            [source],
            |row| row.get(0),
        )
        .unwrap();
    crate::threads::fork_thread(store, source, &answer)
        .unwrap()
        .id
}

fn cloned_call(store: &Store, thread: &str) -> String {
    store
        .conn()
        .query_row(
            "SELECT id FROM tool_calls WHERE thread_id=?1 ORDER BY seq LIMIT 1",
            [thread],
            |row| row.get(0),
        )
        .unwrap()
}

fn erase_origins(store: &Store) {
    store
        .conn()
        .execute("DELETE FROM fork_mutation_origins", [])
        .unwrap();
}

fn delete_source(store: &Store) {
    store
        .conn()
        .execute("DELETE FROM tool_calls WHERE thread_id='source'", [])
        .unwrap();
    store
        .conn()
        .execute("DELETE FROM messages WHERE thread_id='source'", [])
        .unwrap();
    store
        .conn()
        .execute("DELETE FROM threads WHERE id='source'", [])
        .unwrap();
}

#[test]
fn copied_edit_and_write_recover_with_original_checkpoints() {
    for tool in ["edit", "write"] {
        let store = fixture(tool);
        let original = recover_terminal_tools(store.conn(), "project", "source").unwrap();
        let forked = fork(&store, "source");
        let calls = recover_terminal_tools(store.conn(), "project", &forked).unwrap();
        let call = cloned_call(&store, &forked);
        assert_ne!(call, "original");
        assert_eq!(calls[&call].result.call_id, call);
        assert_eq!(
            calls[&call].result.output,
            original["original"].result.output
        );
        assert_eq!(
            recover_terminal_tools(store.conn(), "project", "source").unwrap()["original"]
                .result
                .output,
            original["original"].result.output
        );
        let again = fork(&store, &forked);
        let origin =
            vega_store::fork_mutations::origin(store.conn(), &again, &cloned_call(&store, &again))
                .unwrap();
        assert_eq!(origin.thread_id, "source");
        assert_eq!(origin.call_id, "original");
        assert!(recover_terminal_tools(store.conn(), "project", &again).is_ok());
    }
}

#[test]
fn legacy_fork_and_repeated_fork_recover_without_rewriting_receipts() {
    let store = fixture("edit");
    let first = fork(&store, "source");
    erase_origins(&store);
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_ok());
    let second = fork(&store, &first);
    erase_origins(&store);
    assert!(recover_terminal_tools(store.conn(), "project", &second).is_ok());
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM fork_mutation_origins", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn saved_origins_survive_source_deletion_and_another_fork() {
    let store = fixture("write");
    let first = fork(&store, "source");
    delete_source(&store);
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_ok());
    let second = fork(&store, &first);
    assert!(recover_terminal_tools(store.conn(), "project", &second).is_ok());
}

#[test]
fn new_mutations_in_fork_require_their_own_identity_after_source_deletion() {
    let store = fixture("edit");
    let first = fork(&store, "source");
    delete_source(&store);
    store.conn().execute(
        "INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('new-answer',?1,2,'assistant','done','done',3)", [&first],
    ).unwrap();
    insert_mutation(
        &store,
        &first,
        "new-answer",
        "new-call",
        2,
        "write",
        &first,
        "new-call",
    );
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_ok());
    store.conn().execute(
        "UPDATE tool_calls SET output_text=(SELECT output_text FROM tool_calls WHERE id=?1) WHERE id='new-call'",
        [&cloned_call(&store, &first)],
    ).unwrap();
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_err());
}

#[test]
fn legacy_mismatched_history_and_missing_source_fail_closed() {
    let store = fixture("edit");
    let first = fork(&store, "source");
    erase_origins(&store);
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET finished_at=3 WHERE thread_id=?1",
            [&first],
        )
        .unwrap();
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_err());
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET finished_at=2 WHERE thread_id=?1",
            [&first],
        )
        .unwrap();
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_ok());
    delete_source(&store);
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_err());
}

#[test]
fn invalid_saved_origin_and_checkpoint_are_rejected() {
    let store = fixture("edit");
    let first = fork(&store, "source");
    let call = cloned_call(&store, &first);
    store
        .conn()
        .execute(
            "UPDATE fork_mutation_origins SET source_call_id='different' WHERE call_id=?1",
            [&call],
        )
        .unwrap();
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_err());
    store.conn().execute("UPDATE fork_mutation_origins SET source_call_id='original',project_id='other' WHERE call_id=?1", [&call]).unwrap();
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_err());
    store
        .conn()
        .execute(
            "UPDATE fork_mutation_origins SET project_id='project' WHERE call_id=?1",
            [&call],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET output_text='{}' WHERE id=?1",
            [&call],
        )
        .unwrap();
    assert!(recover_terminal_tools(store.conn(), "project", &first).is_err());
}

#[test]
fn corrupt_checkpoint_rolls_back_fork_creation_without_changing_source() {
    let store = fixture("edit");
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET output_text='{}' WHERE id='original'",
            [],
        )
        .unwrap();
    assert!(crate::threads::fork_thread(&store, "source", "answer").is_err());
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let output: String = store
        .conn()
        .query_row(
            "SELECT output_text FROM tool_calls WHERE id='original'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(output, "{}");
}

#[test]
fn ordinary_mutation_cannot_claim_a_foreign_checkpoint() {
    let store = fixture("write");
    let output = vega_tools::WriteSuccessOutput {
        path: "/fixture/file".into(),
        bytes_written: 4,
        checkpoint_ref: vega_tools::CheckpointIds::new("project", "foreign", "original")
            .unwrap()
            .checkpoint_ref(),
    }
    .to_json()
    .unwrap();
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET output_text=?1 WHERE id='original'",
            [output],
        )
        .unwrap();
    assert!(recover_terminal_tools(store.conn(), "project", "source").is_err());
}
