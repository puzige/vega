use super::*;
use crate::Store;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-redaction-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("state.db")).unwrap();
        store.migrate().unwrap();
        store.conn().execute_batch(
            "INSERT INTO threads(id,title,model,created_at,updated_at) VALUES('thread','synthetic','synthetic',0,0);
             INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('message','thread',1,'assistant','secret tail','done',1);
             INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes) VALUES('call','thread','message',1,'read','{}','secret result','success',1,7);
             INSERT INTO message_timestamp_boundaries(message_id,operation_key,text_start_bytes) VALUES('message','operation',7);
             INSERT INTO assistant_response_starts(message_id,response_id,ordinal,text_start_bytes,started_at_ms) VALUES('message','response',0,7,1);
             INSERT INTO context_checkpoints(thread_id,model,source_version,covered_through_seq,source_fingerprint,summary,estimator_version,created_at) VALUES('thread','synthetic',1,1,'synthetic','secret summary','synthetic',1);",
        ).unwrap();
        Self { root, store }
    }

    fn plan(&self) -> PreparedLegacyRedaction {
        prepare_legacy_thread_redaction(self.store.conn(), "thread", |text, offsets| {
            let projected = text.replace("secret", "X");
            let mapped = offsets
                .iter()
                .map(|offset| text[..*offset].replace("secret", "X").len())
                .collect();
            (projected, mapped)
        })
        .unwrap()
    }

    fn content(&self) -> String {
        self.store
            .conn()
            .query_row(
                "SELECT content FROM messages WHERE id='message'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn preparation_reads_committed_history_without_writer_and_install_remaps_all_offsets() {
    let fixture = Fixture::new();
    let writer = Store::open_existing(fixture.root.join("state.db")).unwrap();
    let held = writer.immediate_transaction().unwrap();
    let before = crate::write_coordination::recent_observations(fixture.store.conn());
    let plan = fixture.plan();
    assert_eq!(
        crate::write_coordination::recent_observations(fixture.store.conn()),
        before
    );
    assert_eq!(fixture.content(), "secret tail");
    held.rollback().unwrap();
    let mut tx = fixture.store.immediate_transaction().unwrap();
    assert!(plan.install(&mut tx).unwrap());
    tx.commit().unwrap();
    assert_eq!(fixture.content(), "X tail");
    let (output, offset): (String, i64) = fixture
        .store
        .conn()
        .query_row(
            "SELECT output_text,text_offset_bytes FROM tool_calls WHERE id='call'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((output, offset), ("X result".into(), 2));
    for sql in [
        "SELECT text_start_bytes FROM message_timestamp_boundaries",
        "SELECT text_start_bytes FROM assistant_response_starts",
    ] {
        assert_eq!(
            fixture
                .store
                .conn()
                .query_row(sql, [], |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    assert_eq!(
        fixture
            .store
            .conn()
            .query_row("SELECT summary FROM context_checkpoints", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "X summary"
    );
}

#[test]
fn source_summary_and_offset_changes_reject_prepared_redaction_without_partial_updates() {
    for sql in [
        "UPDATE messages SET content='changed' WHERE id='message'",
        "UPDATE context_checkpoints SET summary='changed'",
        "UPDATE message_timestamp_boundaries SET text_start_bytes=6",
        "UPDATE assistant_response_starts SET text_start_bytes=6",
        "UPDATE tool_calls SET text_offset_bytes=6 WHERE id='call'",
    ] {
        let fixture = Fixture::new();
        let plan = fixture.plan();
        fixture.store.conn().execute(sql, []).unwrap();
        let content = fixture.content();
        let mut tx = fixture.store.immediate_transaction().unwrap();
        assert!(!plan.validate(&tx).unwrap());
        assert!(plan.install(&mut tx).is_err());
        tx.commit().unwrap();
        assert_eq!(fixture.content(), content);
        assert_eq!(
            fixture
                .store
                .conn()
                .query_row(
                    "SELECT output_text FROM tool_calls WHERE id='call'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "secret result"
        );
    }
}

#[test]
fn failed_install_rolls_back_earlier_updates_even_if_caller_commits_outer_transaction() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    fixture.store.conn().execute_batch("CREATE TRIGGER reject_redaction BEFORE UPDATE OF content ON messages BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    let mut tx = fixture.store.immediate_transaction().unwrap();
    assert!(plan.install(&mut tx).is_err());
    tx.commit().unwrap();
    assert_eq!(fixture.content(), "secret tail");
    assert_eq!(
        fixture
            .store
            .conn()
            .query_row(
                "SELECT output_text FROM tool_calls WHERE id='call'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "secret result"
    );
    assert_eq!(
        fixture
            .store
            .conn()
            .query_row(
                "SELECT text_start_bytes FROM message_timestamp_boundaries",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        7
    );
}
