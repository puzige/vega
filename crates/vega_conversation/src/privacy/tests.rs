use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn permanent_cleanup_refuses_active_admission_and_releases_pause_without_mutation() -> TestResult {
    let store = Store::open(":memory:")?;
    store.migrate()?;
    store.conn().execute(
        "INSERT INTO threads(id,model,created_at,updated_at) VALUES('thread','model',1,1)",
        [],
    )?;
    store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('task','thread',1,'user','unchanged','done',1)",[])?;
    let owner = crate::thread_access::acquire(&store, "thread")?;
    let snapshot = PrivacyReview {
        source_fingerprint: "synthetic".into(),
        findings: Vec::new(),
    };
    assert!(matches!(
        permanently_clean(&store, "thread", &snapshot),
        Err(ConversationError::ContextCleanupPending)
    ));
    assert_eq!(
        vega_store::messages::find(store.conn(), "task")?
            .ok_or("task")?
            .content,
        "unchanged"
    );
    drop(owner);
    let next = crate::thread_access::acquire(&store, "thread")?;
    assert!(next.verifies(store.database_identity(), "thread"));
    Ok(())
}
