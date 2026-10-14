use rusqlite::{Connection, OptionalExtension, Transaction, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialContinuation {
    pub id: String,
    pub thread_id: String,
    pub user_message_id: String,
    pub source_version: u64,
    pub source_fingerprint: String,
    pub state: String,
    pub reason: String,
}

pub fn find(conn: &Connection, id: &str) -> rusqlite::Result<Option<CredentialContinuation>> {
    conn.query_row(
        "SELECT id, thread_id, user_message_id, source_version, source_fingerprint, state, reason FROM credential_continuations WHERE id = ?1",
        [id],
        |row| Ok(CredentialContinuation {
            id: row.get(0)?, thread_id: row.get(1)?, user_message_id: row.get(2)?,
            source_version: row.get(3)?, source_fingerprint: row.get(4)?, state: row.get(5)?, reason: row.get(6)?,
        }),
    ).optional()
}

pub fn suspend(
    tx: &Transaction<'_>,
    thread_id: &str,
    message_id: &str,
    reason: &str,
    now: i64,
) -> rusqlite::Result<()> {
    let source = crate::context_compaction::load_source_in_transaction(tx, thread_id)?;
    let user_message_id: String = tx.query_row(
        "SELECT id FROM messages WHERE thread_id = ?1 AND role = 'user' AND seq < (SELECT seq FROM messages WHERE id = ?2) ORDER BY seq DESC LIMIT 1",
        params![thread_id, message_id], |row| row.get(0),
    )?;
    let unknown = has_unknown_effects(tx, thread_id, &user_message_id)?;
    tx.execute(
        "INSERT INTO credential_continuations (id, thread_id, user_message_id, source_version, source_fingerprint, state, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(id) DO NOTHING",
        params![message_id, thread_id, user_message_id, source.source_version, source.fingerprint, if unknown { "unknown" } else { "ready" }, reason, now],
    )?;
    Ok(())
}

pub fn has_unknown_effects(
    conn: &Connection,
    thread_id: &str,
    user_message_id: &str,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tool_calls WHERE thread_id = ?1 AND message_id IN (SELECT id FROM messages WHERE thread_id = ?1 AND seq > (SELECT seq FROM messages WHERE id = ?2)) AND status IN ('approved', 'running', 'pending_approval', 'cancelled'))",
        params![thread_id, user_message_id], |row| row.get(0),
    )
}

pub fn claim(
    tx: &Transaction<'_>,
    receipt: &CredentialContinuation,
    operation_id: &str,
) -> rusqlite::Result<bool> {
    let current =
        crate::context_compaction::load_source_version_in_transaction(tx, &receipt.thread_id)?;
    if current.source_version != receipt.source_version
        || current.fingerprint != receipt.source_fingerprint
        || has_unknown_effects(tx, &receipt.thread_id, &receipt.user_message_id)?
    {
        return Ok(false);
    }
    Ok(tx.execute(
        "UPDATE credential_continuations SET state = 'claimed', operation_id = ?2 WHERE id = ?1 AND state = 'ready' AND source_version = ?3 AND source_fingerprint = ?4",
        params![receipt.id, operation_id, receipt.source_version, receipt.source_fingerprint],
    )? == 1)
}

pub fn attach(tx: &Transaction<'_>, receipt_id: &str, message_id: &str) -> rusqlite::Result<()> {
    tx.execute("UPDATE credential_continuations SET continuation_message_id = ?2 WHERE id = ?1 AND state = 'claimed'", params![receipt_id, message_id])?;
    Ok(())
}

pub fn complete(conn: &Connection, message_id: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE credential_continuations SET state = 'completed' WHERE continuation_message_id = ?1 AND state = 'claimed'", [message_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn fixture(path: &std::path::Path) -> Store {
        let store = Store::open(path).unwrap();
        store.migrate().unwrap();
        store.conn().execute_batch("INSERT INTO projects (id,path,name,created_at,last_opened_at) VALUES ('p','/synthetic','synthetic',0,0);
            INSERT INTO threads (id,project_id,model,created_at,updated_at) VALUES ('t','p','model',0,0);
            INSERT INTO messages (id,thread_id,seq,role,content,status,created_at) VALUES ('u','t',1,'user','task','done',0),('a','t',2,'assistant','result','failed',0);").unwrap();
        let tx = store.immediate_transaction().unwrap();
        suspend(&tx, "t", "a", "unavailable", 0).unwrap();
        tx.commit().unwrap();
        store
    }

    #[test]
    fn receipt_claim_is_durable_and_single_use_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        drop(fixture(&path));
        let store = Store::open(&path).unwrap();
        let receipt = find(store.conn(), "a").unwrap().unwrap();
        let tx = store.immediate_transaction().unwrap();
        assert!(claim(&tx, &receipt, "operation-1").unwrap());
        tx.commit().unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let tx = store.immediate_transaction().unwrap();
        assert!(!claim(&tx, &receipt, "operation-2").unwrap());
        assert_eq!(find(&tx, "a").unwrap().unwrap().state, "claimed");
    }

    #[test]
    fn source_mutation_rejects_receipt_even_when_sequence_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(&dir.path().join("state.db"));
        let receipt = find(store.conn(), "a").unwrap().unwrap();
        store
            .conn()
            .execute("UPDATE messages SET content = 'edited' WHERE id = 'u'", [])
            .unwrap();
        let tx = store.immediate_transaction().unwrap();
        assert!(!claim(&tx, &receipt, "operation").unwrap());
        assert_eq!(find(&tx, "a").unwrap().unwrap().state, "ready");
    }

    #[test]
    fn unknown_side_effects_are_not_authorized_by_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(&dir.path().join("state.db"));
        store.conn().execute("INSERT INTO tool_calls (id,thread_id,message_id,seq,tool,input_json,status,created_at) VALUES ('tool','t','a',1,'bash','{}','running',0)", []).unwrap();
        let receipt = find(store.conn(), "a").unwrap().unwrap();
        let tx = store.immediate_transaction().unwrap();
        assert!(has_unknown_effects(&tx, "t", "u").unwrap());
        assert!(!claim(&tx, &receipt, "operation").unwrap());
    }

    #[test]
    fn terminal_message_and_receipt_completion_roll_back_together() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(&dir.path().join("state.db"));
        let receipt = find(store.conn(), "a").unwrap().unwrap();
        let tx = store.immediate_transaction().unwrap();
        assert!(claim(&tx, &receipt, "operation").unwrap());
        tx.execute("INSERT INTO messages (id,thread_id,seq,role,content,status,created_at) VALUES ('continued','t',3,'assistant','','streaming',0)", []).unwrap();
        attach(&tx, "a", "continued").unwrap();
        tx.commit().unwrap();
        {
            let tx = store.immediate_transaction().unwrap();
            crate::messages::finish_streaming_in_transaction(
                &tx,
                "continued",
                "done",
                "done",
                None,
            )
            .unwrap();
            assert_eq!(find(&tx, "a").unwrap().unwrap().state, "completed");
        }
        assert_eq!(find(store.conn(), "a").unwrap().unwrap().state, "claimed");
        let status: String = store
            .conn()
            .query_row(
                "SELECT status FROM messages WHERE id = 'continued'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "streaming");
    }
    #[test]
    fn restart_reconstructs_claimed_cursor_without_creating_user_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let store = fixture(&path);
        let receipt = find(store.conn(), "a").unwrap().unwrap();
        let tx = store.immediate_transaction().unwrap();
        assert!(claim(&tx, &receipt, "operation").unwrap());
        tx.execute("INSERT INTO messages (id,thread_id,seq,role,content,status,created_at) VALUES ('continued','t',3,'assistant','','streaming',0)", []).unwrap();
        attach(&tx, "a", "continued").unwrap();
        tx.commit().unwrap();
        drop(store);
        let store = Store::open(path).unwrap();
        crate::recovery::recover_thread(store.conn(), "t", 10).unwrap();
        let recovered = find(store.conn(), "continued").unwrap().unwrap();
        assert_eq!(recovered.user_message_id, "u");
        assert_eq!(recovered.state, "ready");
        assert_eq!(find(store.conn(), "a").unwrap().unwrap().state, "completed");
        let tx = store.immediate_transaction().unwrap();
        assert!(claim(&tx, &recovered, "operation-after-restart").unwrap());
        let users: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE role = 'user'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(users, 1);
    }

    #[test]
    fn old_unknown_effects_do_not_disable_later_user_turns() {
        let dir = tempfile::tempdir().unwrap();
        let store = fixture(&dir.path().join("state.db"));
        store.conn().execute("INSERT INTO tool_calls (id,thread_id,message_id,seq,tool,input_json,status,output_text,created_at) VALUES ('tool','t','a',1,'bash','{}','cancelled',?1,0)", [crate::recovery::RECOVERY_CANCELLED_OUTPUT]).unwrap();
        store.conn().execute("INSERT INTO messages (id,thread_id,seq,role,content,status,created_at) VALUES ('new-user','t',3,'user','reviewed previous operation','done',0)", []).unwrap();
        assert!(has_unknown_effects(store.conn(), "t", "u").unwrap());
        assert!(!has_unknown_effects(store.conn(), "t", "new-user").unwrap());
    }
}
