use super::*;
use crate::Store;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    store: Store,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("vega-privacy-prepared-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        store.conn().execute_batch("INSERT INTO threads(id,model,created_at,updated_at) VALUES('source','model',1,1),('other','model',1,1); INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('user','source',1,'user','secret','done',1),('assistant','source',2,'assistant','secret answer','done',2),('other-user','other',1,'user','other','done',1);")?;
        Ok(Self { store, root })
    }

    fn prepare(&self) -> rusqlite::Result<PreparedCleanup> {
        let expected = source_fingerprint(self.store.conn(), "source")?;
        prepare_clean(self.store.conn(), "source", &expected, redact)
    }

    fn content(&self) -> rusqlite::Result<String> {
        self.store
            .conn()
            .query_row("SELECT content FROM messages WHERE id='user'", [], |row| {
                row.get(0)
            })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn redact(text: &str, offsets: &[usize]) -> (String, Vec<usize>) {
    let mapped = offsets
        .iter()
        .map(|offset| {
            let removed = text
                .match_indices("secret")
                .map(|(start, _)| offset.saturating_sub(start).min(6))
                .sum::<usize>();
            offset - removed
        })
        .collect();
    (text.replace("secret", ""), mapped)
}

#[test]
fn cleanup_preparation_leaves_original_untouched_and_installs_on_another_connection() -> TestResult
{
    let fixture = Fixture::new()?;
    let expected = source_fingerprint(fixture.store.conn(), "source")?;
    let thread = std::thread::current().id();
    let mut calls = 0usize;
    let prepared = prepare_clean(
        fixture.store.conn(),
        "source",
        &expected,
        |text, offsets| {
            assert_eq!(std::thread::current().id(), thread);
            calls += 1;
            redact(text, offsets)
        },
    )?;
    assert!(calls > 0);
    assert_eq!(fixture.content()?, "secret");
    let external = Store::open_existing(fixture.root.join("fixture.db"))?;
    std::thread::spawn(move || -> rusqlite::Result<()> { prepared.install(external.conn()) })
        .join()
        .map_err(|_| "install panicked")??;
    assert_eq!(fixture.content()?, "");
    assert_eq!(
        fixture.store.conn().query_row(
            "SELECT content FROM messages WHERE id='assistant'",
            [],
            |row| row.get::<_, String>(0)
        )?,
        " answer"
    );
    Ok(())
}

#[test]
fn cleanup_rejects_source_schema_permission_exception_and_database_changes() -> TestResult {
    for mutation in [
        "UPDATE messages SET content='changed' WHERE id='user'",
        "UPDATE messages SET status='streaming' WHERE id='assistant'",
        "INSERT INTO threads(id,model,created_at,updated_at) VALUES('new','model',1,1)",
        "UPDATE permission_contract_state SET authority_epoch='01ARZ3NDEKTSV4RRFFQ69G5FAV' WHERE singleton=1",
        "INSERT INTO credential_privacy_exceptions(thread_id,rule_id,fingerprint) VALUES('source','rule','digest')",
        "INSERT INTO credential_privacy_epoch(singleton,digest) VALUES(1,'new') ON CONFLICT(singleton) DO UPDATE SET digest=excluded.digest",
        "CREATE TABLE unrelated_schema_change(id INTEGER)",
    ] {
        let fixture = Fixture::new()?;
        let prepared = fixture.prepare()?;
        fixture.store.conn().execute_batch(mutation)?;
        let before = fixture.content()?;
        assert!(
            prepared.install(fixture.store.conn()).is_err(),
            "{mutation}"
        );
        assert_eq!(fixture.content()?, before);
    }
    let first = Fixture::new()?;
    let second = Fixture::new()?;
    assert!(first.prepare()?.install(second.store.conn()).is_err());
    assert_eq!(second.content()?, "secret");
    Ok(())
}

#[test]
fn cleanup_remaps_response_tool_timestamp_and_generated_notice_offsets() -> TestResult {
    let fixture = Fixture::new()?;
    let notice = crate::messages::GeneratedNoticeRow {
        message_id: "assistant".into(),
        text_start_bytes: 6,
        text_end_bytes: 6,
        kind: "turn_limit".into(),
        turn_limit: Some(3),
        delivered_images: None,
    };
    let canonical = notice.canonical_text().ok_or("notice missing")?;
    let content = format!("secret{canonical}");
    let mut notice = notice;
    notice.text_end_bytes = content.len();
    fixture.store.conn().execute(
        "UPDATE messages SET content='secret',status='streaming' WHERE id='assistant'",
        [],
    )?;
    crate::messages::record_assistant_response_start(
        fixture.store.conn(),
        "assistant",
        "response",
        6,
        1,
    )?;
    fixture.store.conn().execute(
        "UPDATE messages SET content=?1,status='streaming' WHERE id='assistant'",
        [&content],
    )?;
    assert!(crate::messages::record_generated_notice(
        fixture.store.conn(),
        "assistant",
        &content,
        &notice
    )?);
    fixture.store.conn().execute_batch("INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('assistant','response',6,7,'final'); INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,text_offset_bytes) VALUES('call','source','assistant',1,'read','{}','secret output','success',1,6);")?;
    crate::messages::record_timestamp_boundary(fixture.store.conn(), "assistant", "boundary", 6)?;
    fixture
        .store
        .conn()
        .execute("UPDATE messages SET status='done' WHERE id='assistant'", [])?;
    fixture.prepare()?.install(fixture.store.conn())?;
    assert_eq!(
        fixture.store.conn().query_row(
            "SELECT content FROM messages WHERE id='assistant'",
            [],
            |row| row.get::<_, String>(0)
        )?,
        canonical
    );
    for query in [
        "SELECT text_start_bytes FROM assistant_responses WHERE message_id='assistant'",
        "SELECT text_start_bytes FROM generated_notices WHERE message_id='assistant'",
        "SELECT text_start_bytes FROM message_timestamp_boundaries WHERE message_id='assistant'",
        "SELECT text_start_bytes FROM assistant_response_starts WHERE message_id='assistant'",
        "SELECT text_offset_bytes FROM tool_calls WHERE id='call'",
    ] {
        assert_eq!(
            fixture
                .store
                .conn()
                .query_row(query, [], |row| row.get::<_, i64>(0))?,
            0
        );
    }
    Ok(())
}

#[test]
fn cleanup_preserves_json_collision_and_historical_identity_redaction() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().execute_batch("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at) VALUES('call','source','assistant',1,'read','{\"\":\"kept\",\"secret\":\"secret\"}','secret result','success',1); INSERT INTO historical_tool_receipts(call_id,thread_id,kind,source_thread_id,source_message_id,source_call_id,input_json,output_text,references_json) VALUES('call','source','delegation','secret-thread','secret-message','secret-call','{\"secret\":\"secret\"}','secret result','[\"secret\"]'); INSERT INTO historical_message_delivery(message_id,thread_id,model_content) VALUES('user','source','secret model');")?;
    fixture.prepare()?.install(fixture.store.conn())?;
    let input: String = fixture.store.conn().query_row(
        "SELECT input_json FROM tool_calls WHERE id='call'",
        [],
        |row| row.get(0),
    )?;
    let json: serde_json::Value = serde_json::from_str(&input)?;
    assert_eq!(json.as_object().ok_or("not object")?.len(), 2);
    assert_eq!(json[""], "kept");
    assert_eq!(json["_1"], "");
    let identity: (String,String,String) = fixture.store.conn().query_row("SELECT source_thread_id,source_message_id,source_call_id FROM historical_tool_receipts WHERE call_id='call'", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?;
    assert_eq!(
        identity,
        ("-thread".into(), "-message".into(), "-call".into())
    );
    Ok(())
}

#[test]
fn cleanup_rolls_back_earlier_changes_when_later_install_fails() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().execute_batch("CREATE TRIGGER reject_cleaned_user BEFORE UPDATE OF content ON messages WHEN NEW.id='user' BEGIN SELECT RAISE(ABORT,'deliberate install failure'); END;")?;
    let prepared = fixture.prepare()?;
    assert!(prepared.install(fixture.store.conn()).is_err());
    let prepared = fixture.prepare()?;
    let mut transaction = fixture.store.immediate_transaction()?;
    assert!(prepared.install_in_transaction(&mut transaction).is_err());
    transaction.commit()?;
    assert_eq!(fixture.content()?, "secret");
    assert_eq!(
        fixture.store.conn().query_row(
            "SELECT content FROM messages WHERE id='assistant'",
            [],
            |row| row.get::<_, String>(0)
        )?,
        "secret answer"
    );
    Ok(())
}

#[test]
fn cleanup_retires_related_precompute_and_invalidates_continuations() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().execute_batch("INSERT INTO threads(id,model,created_at,updated_at,parent_thread_id) VALUES('child','model',1,1,'source'); INSERT INTO context_precompute_accounting(generation,thread_id,model) VALUES('source-generation','source','model'),('child-generation','child','model'); INSERT INTO context_precompute_slots(thread_id,generation) VALUES('source','source-generation'),('child','child-generation'); INSERT INTO context_precompute_ready(thread_id,generation,payload) VALUES('source','source-generation','{}'),('child','child-generation','{}'); INSERT INTO credential_continuations(id,thread_id,user_message_id,source_version,source_fingerprint,raw_user_fingerprint,state,reason,created_at) VALUES('assistant','source','user',2,'source','raw','ready','prepared',1);")?;
    fixture.prepare()?.install(fixture.store.conn())?;
    assert_eq!(
        fixture.store.conn().query_row(
            "SELECT COUNT(*) FROM context_precompute_accounting WHERE retired=1",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        2
    );
    assert_eq!(
        fixture.store.conn().query_row(
            "SELECT COUNT(*) FROM context_precompute_slots",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        0
    );
    assert_eq!(
        fixture.store.conn().query_row(
            "SELECT COUNT(*) FROM context_precompute_ready",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        0
    );
    let continuation: (String, String) = fixture.store.conn().query_row(
        "SELECT state,reason FROM credential_continuations WHERE id='assistant'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(continuation, ("unknown".into(), "source_changed".into()));
    Ok(())
}

#[test]
fn cleanup_rejects_invalid_projection_offsets_without_writing() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.conn().execute_batch("INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('assistant','response',0,6,'final');")?;
    let expected = source_fingerprint(fixture.store.conn(), "source")?;
    assert!(
        prepare_clean(fixture.store.conn(), "source", &expected, |text, _| (
            text.to_owned(),
            Vec::new()
        ))
        .is_err()
    );
    assert_eq!(fixture.content()?, "secret");
    Ok(())
}
