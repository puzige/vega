use vega_store::Store;

fn fixture(with_call: bool) -> Store {
    let store = Store::open(":memory:").expect("open store");
    store.migrate().expect("migrate");
    store.conn().execute_batch(
        "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project','/fixture','fixture',1,1);
         INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES('source','project','source','model',1,1);
         INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES
         ('question','source',1,'user','question','done',1),
         ('interrupted','source',2,'assistant','working','interrupted',2),
         ('next','source',3,'user','continue','done',3),
         ('cutoff','source',4,'assistant','finished','done',4);
         INSERT INTO assistant_response_starts(message_id,response_id,ordinal,text_start_bytes,started_at_ms) VALUES
         ('interrupted','round',0,0,2),('cutoff','final',0,0,4);
         INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('cutoff','final',0,8,'final');
         INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES
         ('interrupted','proposal-one','round'),('interrupted','proposal-two','round');",
    ).expect("source history");
    if with_call {
        store.conn().execute_batch(
            "INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,approval,exit_code,duration_ms,created_at,finished_at,text_offset_bytes) VALUES
             ('persisted','source','interrupted',1,'bash','{\"command\":\"pwd\",\"timeout\":null}','/fixture','success','{\"decision\":\"once\",\"note\":null,\"source\":\"full_access\",\"danger\":null}',0,1,2,3,7);
             INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES('interrupted','persisted','round');",
        ).expect("persisted call");
    }
    store
}

fn count(store: &Store, sql: &str, thread: &str) -> i64 {
    store
        .conn()
        .query_row(sql, [thread], |row| row.get(0))
        .expect("count")
}

fn assert_rejected_without_destination(store: &Store, expected: &str) {
    let before = count(
        store,
        "SELECT COUNT(*) FROM threads WHERE project_id=?1",
        "project",
    );
    let error = crate::threads::fork_thread(store, "source", "cutoff").expect_err("must reject");
    assert!(error.to_string().contains(expected), "{error}");
    assert_eq!(
        count(
            store,
            "SELECT COUNT(*) FROM threads WHERE project_id=?1",
            "project"
        ),
        before
    );
    assert_eq!(
        count(
            store,
            "SELECT COUNT(*) FROM messages WHERE thread_id<>?1",
            "source"
        ),
        0
    );
}

#[test]
fn mixed_proposals_fork_without_fabricating_calls_and_can_fork_again() {
    let store = fixture(true);
    let fork = crate::threads::fork_thread(&store, "source", "cutoff").expect("fork mixed round");
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM tool_calls WHERE thread_id=?1",
            &fork.id
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM tool_call_rounds r JOIN messages m ON m.id=r.message_id WHERE m.thread_id=?1",
            &fork.id
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM tool_call_rounds r JOIN messages m ON m.id=r.message_id WHERE m.thread_id=?1",
            "source"
        ),
        3
    );
    let (output, status): (String, String) = store
        .conn()
        .query_row(
            "SELECT output_text,status FROM tool_calls WHERE thread_id=?1",
            [&fork.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("cloned call");
    assert_eq!((output.as_str(), status.as_str()), ("/fixture", "success"));
    let cutoff: String = store
        .conn()
        .query_row(
            "SELECT id FROM messages WHERE thread_id=?1 AND seq=4",
            [&fork.id],
            |row| row.get(0),
        )
        .expect("cloned cutoff");
    let second = crate::threads::fork_thread(&store, &fork.id, &cutoff).expect("repeat fork");
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM tool_calls WHERE thread_id=?1",
            &second.id
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM tool_call_rounds r JOIN messages m ON m.id=r.message_id WHERE m.thread_id=?1",
            &second.id
        ),
        1
    );
}

#[test]
fn proposal_only_round_does_not_claim_any_tool_executed() {
    let store = fixture(false);
    let fork = crate::threads::fork_thread(&store, "source", "cutoff").expect("fork proposals");
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM tool_calls WHERE thread_id=?1",
            &fork.id
        ),
        0
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM tool_call_rounds r JOIN messages m ON m.id=r.message_id WHERE m.thread_id=?1",
            &fork.id
        ),
        0
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM tool_call_rounds r JOIN messages m ON m.id=r.message_id WHERE m.thread_id=?1",
            "source"
        ),
        2
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM assistant_response_starts s JOIN messages m ON m.id=s.message_id WHERE m.thread_id=?1",
            &fork.id
        ),
        2
    );
}

#[test]
fn existing_call_from_another_message_still_rejects_atomically() {
    let store = fixture(true);
    store
        .conn()
        .execute(
            "UPDATE tool_call_rounds SET message_id='cutoff' WHERE call_id='persisted'",
            [],
        )
        .expect("foreign message reference");
    assert_rejected_without_destination(&store, "invalid tool round owner");
}

#[test]
fn existing_call_from_another_thread_still_rejects_atomically() {
    let store = fixture(true);
    store.conn().execute_batch(
        "INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES('foreign','project','foreign','model',1,1);
         INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('foreign-message','foreign',1,'assistant','other','done',1);
         UPDATE tool_calls SET thread_id='foreign',message_id='foreign-message' WHERE id='persisted';",
    ).expect("foreign call");
    let error = crate::threads::fork_thread(&store, "source", "cutoff")
        .expect_err("must reject foreign call");
    assert!(
        error.to_string().contains("invalid tool round owner"),
        "{error}"
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM threads WHERE project_id=?1",
            "project"
        ),
        2
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM messages WHERE thread_id NOT IN ('source','foreign') AND id<>?1",
            "unused"
        ),
        0
    );
}

#[test]
fn nonterminal_call_still_blocks_source_fork() {
    let store = fixture(true);
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET status='running' WHERE id='persisted'",
            [],
        )
        .expect("running call");
    assert_rejected_without_destination(&store, "source has persisted active work");
}
