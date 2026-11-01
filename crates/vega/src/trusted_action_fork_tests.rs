use super::*;

#[test]
fn fork_lease_is_shared_and_duplicate_requests_cannot_replace_it() {
    let first_window = TrustedActionCoordinator::default();
    let other_window = first_window.clone();
    let lease = first_window
        .acquire(TrustedActionKind::ConversationFork, 7, 0)
        .unwrap();
    assert!(
        other_window
            .acquire(TrustedActionKind::ConversationFork, 7, 0)
            .is_none()
    );
    assert!(
        other_window
            .acquire(TrustedActionKind::AgentPreflight, 7, 1)
            .is_none()
    );
    assert!(!other_window.cancel_agent_preparation());
    assert!(other_window.is_kind_active(TrustedActionKind::ConversationFork));
    let wrong_owner = TrustedActionToken {
        owner_epoch: 8,
        ..lease
    };
    assert!(!other_window.release(wrong_owner));
    assert!(first_window.is_busy());
    assert!(other_window.release(lease));
    assert!(!first_window.is_busy());
    let retry = first_window
        .acquire(TrustedActionKind::ConversationFork, 8, 0)
        .unwrap();
    assert_ne!(retry.generation, lease.generation);
    assert!(!other_window.release(lease));
    assert!(first_window.is_kind_active(TrustedActionKind::ConversationFork));
    assert!(first_window.release(retry));
}

fn source() -> vega_store::Store {
    let store = vega_store::Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    store.conn().execute_batch(
        "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project','/fixture','Fixture',1,1);
         INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES('source','project','Source','model',1,1);
         INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES
           ('question','source',1,'user','Question','done',1),
           ('answer','source',2,'assistant','Answer','done',2);
         INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('answer','final',0,6,'final');"
    ).unwrap();
    store
}

#[test]
fn failed_copy_releases_ownership_and_retry_creates_one_durable_fork() {
    let store = source();
    let actions = TrustedActionCoordinator::default();
    let lease = actions
        .acquire(TrustedActionKind::ConversationFork, 1, 0)
        .unwrap();
    let admission = vega_store::thread_admission::try_acquire(store.database_identity(), "source")
        .unwrap()
        .unwrap();
    assert!(
        vega_store::thread_admission::try_acquire(store.database_identity(), "source")
            .unwrap()
            .is_none()
    );
    assert!(vega_conversation::threads::fork_thread(&store, "source", "question").is_err());
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    drop(admission);
    assert!(actions.release(lease));
    let retry = actions
        .acquire(TrustedActionKind::ConversationFork, 1, 0)
        .unwrap();
    let admission = vega_store::thread_admission::try_acquire(store.database_identity(), "source")
        .unwrap()
        .unwrap();
    let fork = vega_conversation::threads::fork_thread(&store, "source", "answer").unwrap();
    assert_ne!(fork.id, "source");
    assert_eq!(fork.forked_from_thread_id.as_deref(), Some("source"));
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
    assert!(
        actions
            .acquire(TrustedActionKind::ConversationFork, 1, 0)
            .is_none()
    );
    drop(admission);
    assert!(actions.release(retry));
    assert!(!actions.is_busy());
}

#[test]
fn running_source_refuses_copy_without_partial_thread_and_accepts_after_completion() {
    let store = source();
    store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('running','source',3,'assistant','','streaming',3)", []).unwrap();
    let result = vega_conversation::threads::fork_thread(&store, "source", "answer");
    assert!(matches!(
        result,
        Err(vega_conversation::threads::ForkThreadError::Refused(
            vega_store::thread_fork::ForkError::ActiveSource
        ))
    ));
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    store
        .conn()
        .execute(
            "UPDATE messages SET status='interrupted' WHERE id='running'",
            [],
        )
        .unwrap();
    let fork = vega_conversation::threads::fork_thread(&store, "source", "answer").unwrap();
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id=?1",
            [&fork.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let source_status: String = store
        .conn()
        .query_row(
            "SELECT status FROM messages WHERE id='running'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(source_status, "interrupted");
}
