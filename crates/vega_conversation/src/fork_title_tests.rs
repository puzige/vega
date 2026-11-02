use vega_store::Store;

fn fixture(title: &str) -> Store {
    let store = Store::open(":memory:").expect("open");
    store.migrate().expect("migrate");
    store.conn().execute_batch(
        "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project','/fixture','fixture',1,1),('other','/other','other',1,1);
         INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES('source','project','source','model',1,1);
         INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES
         ('question','source',1,'user','question','done',1),
         ('cutoff','source',2,'assistant','finished','done',2);
         INSERT INTO assistant_response_starts(message_id,response_id,ordinal,text_start_bytes,started_at_ms) VALUES('cutoff','final',0,0,2);
         INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('cutoff','final',0,8,'final');",
    ).expect("history");
    store
        .conn()
        .execute("UPDATE threads SET title=?1 WHERE id='source'", [title])
        .expect("title");
    store
}

fn existing(store: &Store, id: &str, project: &str, title: &str, status: &str) {
    store.conn().execute(
        "INSERT INTO threads(id,project_id,title,model,status,created_at,updated_at) VALUES(?1,?2,?3,'model',?4,1,1)",
        [id, project, title, status],
    ).expect("existing title");
}

#[test]
fn first_repeated_and_nested_forks_use_ascii_sequence_and_preserve_source() {
    let store = fixture("四步法多Agent");
    let first = crate::threads::fork_latest_thread(&store, "source").expect("first");
    let second = crate::threads::fork_latest_thread(&store, "source").expect("second");
    let nested = crate::threads::fork_latest_thread(&store, &first.id).expect("nested");
    assert_eq!(first.title, "四步法多Agent (2)");
    assert_eq!(second.title, "四步法多Agent (3)");
    assert_eq!(nested.title, "四步法多Agent (4)");
    assert_eq!(
        crate::threads::get_thread(&store, "source")
            .expect("source")
            .title,
        "四步法多Agent"
    );
    assert_eq!(first.forked_from_message_id.as_deref(), Some("cutoff"));
}

#[test]
fn archived_collisions_count_but_other_projects_do_not() {
    let store = fixture("Topic");
    existing(&store, "archived", "project", "Topic (4)", "archived");
    existing(&store, "other-title", "other", "Topic (99)", "active");
    let fork = crate::threads::fork_latest_thread(&store, "source").expect("fork");
    assert_eq!(fork.title, "Topic (5)");
}

#[test]
fn ordinary_numeric_parentheses_are_preserved_when_forking_again() {
    let store = fixture("Report (2026)");
    let first = crate::threads::fork_latest_thread(&store, "source").expect("first");
    let second = crate::threads::fork_latest_thread(&store, &first.id).expect("nested");
    assert_eq!(first.title, "Report (2026) (2)");
    assert_eq!(second.title, "Report (2026) (3)");
}

#[test]
fn punctuation_is_literal_and_only_canonical_suffixes_affect_sequence() {
    let store = fixture("A%_[测试]");
    existing(&store, "different", "project", "AX_[测试] (50)", "active");
    existing(
        &store,
        "leading-zero",
        "project",
        "A%_[测试] (09)",
        "active",
    );
    existing(
        &store,
        "full-width",
        "project",
        "A%_[测试] （20）",
        "active",
    );
    let fork = crate::threads::fork_latest_thread(&store, "source").expect("fork");
    assert_eq!(fork.title, "A%_[测试] (2)");
}

#[test]
fn latest_fork_excludes_trailing_user_and_interrupted_messages() {
    let store = fixture("Topic");
    store
        .conn()
        .execute_batch(
            "INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES
         ('tail-user','source',3,'user','continue','done',3),
         ('tail-assistant','source',4,'assistant','partial','interrupted',4);",
        )
        .expect("tail");
    let fork = crate::threads::fork_latest_thread(&store, "source").expect("fork");
    assert_eq!(fork.forked_from_message_id.as_deref(), Some("cutoff"));
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id=?1",
            [&fork.id],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(count, 2);
}

#[test]
fn missing_final_and_running_source_reject_without_creating_destination() {
    let store = fixture("Topic");
    store
        .conn()
        .execute("DELETE FROM assistant_responses", [])
        .expect("remove final");
    assert!(crate::threads::fork_latest_thread(&store, "source").is_err());
    store.conn().execute_batch(
        "INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('cutoff','final',0,8,'final');
         INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('active','source',3,'assistant','','streaming',3);",
    ).expect("active");
    let error = crate::threads::fork_latest_thread(&store, "source").expect_err("must reject");
    assert!(
        error.to_string().contains("persisted active work"),
        "{error}"
    );
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 1);
}

#[test]
fn exhausted_numbering_rejects_without_changing_source() {
    let store = fixture("Topic");
    existing(
        &store,
        "last",
        "project",
        "Topic (18446744073709551615)",
        "active",
    );
    let error = crate::threads::fork_latest_thread(&store, "source").expect_err("must reject");
    assert!(error.to_string().contains("sequence exhausted"), "{error}");
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 2);
}
