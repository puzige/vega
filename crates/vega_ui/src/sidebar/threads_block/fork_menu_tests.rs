use super::ThreadsBlock;
use vega_store::Store;

#[test]
fn unavailable_storage_is_not_reported_as_missing_reply() {
    assert_eq!(
        ThreadsBlock::fork_cutoff_unavailable(None, "source"),
        Some("ui.fork_failed")
    );
    let unmigrated = Store::open(":memory:").expect("open");
    assert_eq!(
        ThreadsBlock::fork_cutoff_unavailable(Some(&unmigrated), "source"),
        Some("ui.fork_failed")
    );
}

#[test]
fn successfully_read_absence_and_trusted_final_have_distinct_menu_states() {
    let store = Store::open(":memory:").expect("open");
    store.migrate().expect("migrate");
    store.conn().execute_batch(
        "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project','/fixture','fixture',1,1);
         INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES('source','project','source','model',1,1);",
    ).expect("source");
    assert_eq!(
        ThreadsBlock::fork_cutoff_unavailable(Some(&store), "source"),
        Some("ui.fork_no_completed_reply")
    );
    store.conn().execute_batch(
        "INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('cutoff','source',1,'assistant','finished','done',1);
         INSERT INTO assistant_response_starts(message_id,response_id,ordinal,text_start_bytes,started_at_ms) VALUES('cutoff','final',0,0,1);
         INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('cutoff','final',0,8,'final');",
    ).expect("final");
    assert_eq!(
        ThreadsBlock::fork_cutoff_unavailable(Some(&store), "source"),
        None
    );
}
