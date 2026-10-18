use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

pub fn exceptions(conn: &Connection, thread: &str) -> rusqlite::Result<Vec<(String, String)>> {
    conn.prepare(
        "SELECT rule_id, fingerprint FROM credential_privacy_exceptions WHERE thread_id = ?1",
    )?
    .query_map([thread], |row| Ok((row.get(0)?, row.get(1)?)))?
    .collect()
}

pub fn allow(
    conn: &Connection,
    thread: &str,
    rule: &str,
    fingerprint: &str,
) -> rusqlite::Result<()> {
    conn.execute("INSERT OR IGNORE INTO credential_privacy_exceptions(thread_id,rule_id,fingerprint) VALUES (?1,?2,?3)", params![thread,rule,fingerprint])?;
    Ok(())
}

pub fn revoke(
    conn: &Connection,
    thread: &str,
    rule: &str,
    fingerprint: &str,
) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM credential_privacy_exceptions WHERE thread_id=?1 AND rule_id=?2 AND fingerprint=?3", params![thread,rule,fingerprint])?;
    Ok(())
}

pub fn set_exception_checked(
    conn: &Connection,
    thread: &str,
    source_fingerprint: &str,
    rule: &str,
    fingerprint: &str,
    allowed: bool,
) -> rusqlite::Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let current = source_fingerprint_in_transaction(&tx, thread)?;
    if current != source_fingerprint {
        return Err(rusqlite::Error::InvalidParameterName(
            "conversation changed; review again".into(),
        ));
    }
    if allowed {
        allow(&tx, thread, rule, fingerprint)?;
    } else {
        revoke(&tx, thread, rule, fingerprint)?;
    }
    tx.commit()
}

pub fn fields(conn: &Connection, thread: &str) -> rusqlite::Result<Vec<(String, String, String)>> {
    let mut query = conn.prepare("SELECT id,'message',content FROM messages WHERE thread_id=?1 UNION ALL SELECT id,'tool.input',input_json FROM tool_calls WHERE thread_id=?1 UNION ALL SELECT id,'tool.output',output_text FROM tool_calls WHERE thread_id=?1 AND output_text IS NOT NULL UNION ALL SELECT CAST(id AS TEXT),'summary',summary FROM context_checkpoints WHERE thread_id=?1 UNION ALL SELECT id,'plan.note',plan_review_note FROM messages WHERE thread_id=?1 AND plan_review_note IS NOT NULL UNION ALL SELECT id,'pending.model',model_content FROM pending_messages WHERE thread_id=?1 AND model_content IS NOT NULL UNION ALL SELECT id,'pending.content',json_extract(payload,'$.content') FROM pending_messages WHERE thread_id=?1 AND payload != '' AND json_valid(payload) AND json_type(payload,'$.content')='text' UNION ALL SELECT child_thread_id,'delegation.name',name FROM delegated_runs WHERE parent_thread_id=?1 OR child_thread_id=?1 UNION ALL SELECT child_thread_id,'delegation.task',task FROM delegated_runs WHERE parent_thread_id=?1 OR child_thread_id=?1 UNION ALL SELECT child_thread_id,'delegation.result',result FROM delegated_runs WHERE parent_thread_id=?1 OR child_thread_id=?1")?;
    query
        .query_map([thread], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect()
}

pub fn source_fingerprint(conn: &Connection, thread: &str) -> rusqlite::Result<String> {
    let tx = conn.unchecked_transaction()?;
    source_fingerprint_in_transaction(&tx, thread)
}

pub fn source_fingerprint_in_transaction(
    conn: &rusqlite::Transaction<'_>,
    thread: &str,
) -> rusqlite::Result<String> {
    let source = crate::context_compaction::load_source_version_in_transaction(conn, thread)?;
    let mut values = fields(conn, thread)?;
    let mut query = conn.prepare("SELECT child_thread_id,json_array(parent_thread_id,parent_message_id,spawn_call_id,child_message_id,state,truncated,usage_complete,created_at) FROM delegated_runs WHERE parent_thread_id=?1 OR child_thread_id=?1")?;
    values.extend(
        query
            .query_map([thread], |row| {
                Ok((row.get(0)?, "delegation.identity".into(), row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    values.sort();
    let mut digest = Sha256::new();
    digest.update(source.fingerprint.as_bytes());
    for (id, field, text) in values {
        for value in [id, field, text] {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn bounded_delegation_text(text: &str, limit: usize) -> (String, bool) {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].into(), end < text.len())
}

fn cleaned_delegation_name(
    conn: &Connection,
    row: &crate::subagents::DelegationRow,
    projected: &str,
) -> rusqlite::Result<String> {
    let name = projected
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    let (mut name, _) = bounded_delegation_text(&name, 256);
    if name.trim().is_empty() {
        name = "[已遮盖]".into();
    }
    let occupied = |name: &str| {
        conn.query_row("SELECT EXISTS(SELECT 1 FROM delegated_runs WHERE parent_thread_id=?1 AND parent_message_id=?2 AND name=?3 AND child_thread_id<>?4)", params![row.parent_thread_id,row.parent_message_id,name,row.child_thread_id], |row| row.get::<_,bool>(0))
    };
    if !occupied(&name)? {
        return Ok(name);
    }
    let identity = format!("{:x}", Sha256::digest(row.child_thread_id.as_bytes()))
        .chars()
        .take(12)
        .collect::<String>();
    let mut attempt = 0u64;
    loop {
        let suffix = format!(" · {identity}.{attempt}");
        let limit = 256usize
            .checked_sub(suffix.len())
            .ok_or(rusqlite::Error::InvalidQuery)?;
        let candidate = format!("{}{suffix}", bounded_delegation_text(&name, limit).0);
        if !occupied(&candidate)? {
            return Ok(candidate);
        }
        attempt = attempt
            .checked_add(1)
            .ok_or(rusqlite::Error::InvalidQuery)?;
    }
}

fn clean_delegations<F>(conn: &Connection, thread: &str, redact: &mut F) -> rusqlite::Result<()>
where
    F: FnMut(&str, &[usize]) -> (String, Vec<usize>),
{
    for row in crate::subagents::privacy_scope(conn, thread)? {
        let name = cleaned_delegation_name(conn, &row, &redact(&row.name, &[]).0)?;
        let (mut task, _) = bounded_delegation_text(&redact(&row.task, &[]).0, 32 * 1024);
        if task.trim().is_empty() {
            task = "[已遮盖]".into();
        }
        let (result, truncated) = bounded_delegation_text(&redact(&row.result, &[]).0, 16 * 1024);
        let truncated = row.truncated || truncated;
        if name == row.name
            && task == row.task
            && result == row.result
            && truncated == row.truncated
        {
            continue;
        }
        if conn.execute("UPDATE delegated_runs SET name=?1,task=?2,result=?3,truncated=?4 WHERE child_thread_id=?5 AND state NOT IN ('running','cancelling')", params![name,task,result,truncated,row.child_thread_id])? != 1 {
            return Err(rusqlite::Error::InvalidParameterName(
                "conversation changed; review again".into(),
            ));
        }
    }
    Ok(())
}

pub fn clean<F>(
    conn: &Connection,
    thread: &str,
    expected: &str,
    mut redact: F,
) -> rusqlite::Result<()>
where
    F: FnMut(&str, &[usize]) -> (String, Vec<usize>),
{
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let source = source_fingerprint_in_transaction(&tx, thread)?;
    let running: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id=?1 AND status='streaming') OR EXISTS(SELECT 1 FROM delegated_runs WHERE (parent_thread_id=?1 OR child_thread_id=?1) AND state IN ('running','cancelling'))",
        [thread],
        |row| row.get(0),
    )?;
    if running || source != expected {
        return Err(rusqlite::Error::InvalidParameterName(
            "conversation changed; review again".into(),
        ));
    }
    let responses = {
        let mut query = tx.prepare("SELECT r.message_id,r.response_id,r.text_start_bytes,r.text_end_bytes,m.content FROM assistant_responses r JOIN messages m ON m.id=r.message_id WHERE m.thread_id=?1")?;
        query
            .query_map([thread], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (message, id, start, end, text) in responses {
        let start = usize::try_from(start).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let end = usize::try_from(end).map_err(|_| rusqlite::Error::InvalidQuery)?;
        if start > end || text.get(start..end).is_none() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (projected, offsets) = redact(&text, &[start, end]);
        let [start, end] = offsets.as_slice() else {
            return Err(rusqlite::Error::InvalidParameterName(
                "invalid response offsets".into(),
            ));
        };
        if start > end || projected.get(*start..*end).is_none() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if start < end {
            let start = i64::try_from(*start).map_err(|_| rusqlite::Error::InvalidQuery)?;
            let end = i64::try_from(*end).map_err(|_| rusqlite::Error::InvalidQuery)?;
            tx.execute("UPDATE assistant_responses SET text_start_bytes=?1,text_end_bytes=?2 WHERE message_id=?3 AND response_id=?4", params![start,end,message,id])?;
        } else {
            tx.execute(
                "DELETE FROM assistant_responses WHERE message_id=?1 AND response_id=?2",
                params![message, id],
            )?;
        }
    }
    crate::context_compaction::redact_legacy_thread_content(&tx, thread, &mut redact)?;
    for (id, field, value) in fields(&tx, thread)? {
        let changed = match field.as_str() {
            "message" | "plan.note" | "pending.model" | "pending.content" => redact(&value, &[]).0,
            "tool.input" => {
                fn visit<F: FnMut(&str, &[usize]) -> (String, Vec<usize>)>(
                    value: &mut serde_json::Value,
                    redact: &mut F,
                ) {
                    match value {
                        serde_json::Value::String(text) => *text = redact(text, &[]).0,
                        serde_json::Value::Array(items) => {
                            for item in items {
                                visit(item, redact);
                            }
                        }
                        serde_json::Value::Object(fields) => {
                            let original = std::mem::take(fields);
                            for (name, mut value) in original {
                                visit(&mut value, redact);
                                let projected = redact(&name, &[]).0;
                                let mut name = projected.clone();
                                let mut suffix = 0usize;
                                while fields.contains_key(&name) {
                                    suffix += 1;
                                    name = format!("{projected}_{suffix}");
                                }
                                fields.insert(name, value);
                            }
                        }
                        _ => {}
                    }
                }
                let mut json: serde_json::Value = serde_json::from_str(&value).map_err(|_| {
                    rusqlite::Error::InvalidParameterName("invalid historical tool input".into())
                })?;
                visit(&mut json, &mut redact);
                serde_json::to_string(&json).map_err(|_| {
                    rusqlite::Error::InvalidParameterName("invalid historical tool input".into())
                })?
            }
            _ => continue,
        };
        if value != changed {
            match field.as_str() {
                "message" => {
                    tx.execute("UPDATE messages SET content=?1 WHERE id=?2 AND thread_id=?3 AND role='user'", params![changed,id,thread])?;
                }
                "plan.note" => {
                    tx.execute(
                        "UPDATE messages SET plan_review_note=?1 WHERE id=?2 AND thread_id=?3",
                        params![changed, id, thread],
                    )?;
                }
                "pending.model" => {
                    tx.execute(
                        "UPDATE pending_messages SET model_content=?1 WHERE id=?2 AND thread_id=?3",
                        params![changed, id, thread],
                    )?;
                }
                "pending.content" => {
                    tx.execute("UPDATE pending_messages SET payload=json_set(payload,'$.content',?1) WHERE id=?2 AND thread_id=?3", params![changed,id,thread])?;
                }
                "tool.input" => {
                    tx.execute(
                        "UPDATE tool_calls SET input_json=?1 WHERE id=?2 AND thread_id=?3",
                        params![changed, id, thread],
                    )?;
                }
                _ => {}
            }
        }
    }
    clean_delegations(&tx, thread, &mut redact)?;
    tx.execute(
        "DELETE FROM context_checkpoints WHERE thread_id=?1",
        [thread],
    )?;
    tx.execute("UPDATE credential_continuations SET state='unknown',reason='source_changed' WHERE thread_id=?1 AND state IN ('ready','claimed')", [thread])?;
    tx.commit()
}

pub fn source_message_id(
    conn: &Connection,
    field: &str,
    id: &str,
) -> rusqlite::Result<Option<String>> {
    match field {
        "message" | "plan.note" => Ok(Some(id.into())),
        "tool.input" | "tool.output" => conn
            .query_row(
                "SELECT message_id FROM tool_calls WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional(),
        _ => Ok(None),
    }
}

pub fn refresh_epoch(conn: &Connection, digest: &str) -> rusqlite::Result<()> {
    fn current(conn: &Connection) -> rusqlite::Result<Option<String>> {
        conn.query_row(
            "SELECT digest FROM credential_privacy_epoch WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
    }
    if current(conn)?.as_deref() == Some(digest) {
        return Ok(());
    }
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let prior = current(&tx)?;
    if prior.as_deref() != Some(digest) {
        tx.execute("DELETE FROM credential_privacy_exceptions", [])?;
        tx.execute("INSERT INTO credential_privacy_epoch(singleton,digest) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET digest=excluded.digest", [digest])?;
    }
    tx.commit()
}

pub fn source_label(conn: &Connection, field: &str, id: &str) -> rusqlite::Result<String> {
    let label = match field {
        "message" => {
            let role: String =
                conn.query_row("SELECT role FROM messages WHERE id=?1", [id], |row| {
                    row.get(0)
                })?;
            if role == "user" {
                "用户消息"
            } else {
                "助手回复"
            }
        }
        "tool.input" => "工具调用",
        "tool.output" => "工具结果",
        "summary" => "对话摘要",
        "plan.note" => "计划反馈",
        "pending.model" => "已发送请求",
        "pending.content" => "待发送消息",
        "delegation.name" => "子任务名称",
        "delegation.task" => "子任务内容",
        "delegation.result" => "子任务结果",
        _ => "对话记录",
    };
    Ok(label.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use std::sync::{Arc, Barrier};

    const DELEGATED_SECRET: &str = "synthetic-delegated-owner-secret";

    fn delegation_fixture() -> Store {
        let store = Store::open(":memory:").unwrap();
        store.migrate().unwrap();
        for id in ["parent", "child", "sibling", "other", "other-child"] {
            store
                .conn()
                .execute(
                    "INSERT INTO threads(id,model,created_at,updated_at) VALUES(?1,'mock',1,1)",
                    [id],
                )
                .unwrap();
            store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'user',?3,'done',1)", params![format!("{id}-user"),id,DELEGATED_SECRET]).unwrap();
        }
        for (child, parent) in [
            ("child", "parent"),
            ("sibling", "parent"),
            ("other-child", "other"),
        ] {
            crate::subagents::insert(
                store.conn(),
                &crate::subagents::DelegationRow {
                    child_thread_id: child.into(),
                    parent_thread_id: parent.into(),
                    parent_message_id: format!("{parent}-run"),
                    spawn_call_id: format!("{child}-spawn"),
                    child_message_id: format!("{child}-run"),
                    name: format!("{child} {DELEGATED_SECRET}"),
                    task: format!("task {DELEGATED_SECRET}"),
                    state: "completed".into(),
                    result: format!("result {DELEGATED_SECRET}"),
                    truncated: false,
                    created_at: 1,
                },
            )
            .unwrap();
        }
        store
    }

    fn delegation_identity(conn: &Connection, child: &str) -> String {
        conn.query_row("SELECT json_array(child_thread_id,parent_thread_id,parent_message_id,spawn_call_id,child_message_id,state,usage_complete,created_at) FROM delegated_runs WHERE child_thread_id=?1", [child], |row| row.get(0)).unwrap()
    }

    fn user_text(conn: &Connection, thread: &str) -> String {
        conn.query_row(
            "SELECT content FROM messages WHERE thread_id=?1",
            [thread],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn m19_delegation_privacy_fields_and_cleanup_preserve_thread_scope_and_identity() {
        for scope in ["parent", "child"] {
            let store = delegation_fixture();
            let conn = store.conn();
            let original = ["child", "sibling", "other-child"]
                .map(|child| crate::subagents::find(conn, child).unwrap().unwrap());
            let identities =
                ["child", "sibling", "other-child"].map(|child| delegation_identity(conn, child));
            let threads = ["child", "sibling", "other-child"]
                .map(|child| crate::threads::find(conn, child).unwrap());
            allow(conn, "parent", "synthetic-rule", "parent-only").unwrap();
            let source = source_fingerprint(conn, scope).unwrap();
            let copied = fields(conn, scope)
                .unwrap()
                .into_iter()
                .filter(|(_, field, _)| field.starts_with("delegation."))
                .collect::<Vec<_>>();
            assert_eq!(copied.len(), if scope == "parent" { 6 } else { 3 });
            for (id, field, text) in copied {
                assert!(text.contains(DELEGATED_SECRET));
                assert!(source_message_id(conn, &field, &id).unwrap().is_none());
                assert_eq!(
                    source_label(conn, &field, &id).unwrap(),
                    match field.as_str() {
                        "delegation.name" => "子任务名称",
                        "delegation.task" => "子任务内容",
                        "delegation.result" => "子任务结果",
                        _ => unreachable!(),
                    }
                );
            }
            for row in &original {
                assert!(
                    crate::subagents::find(conn, &row.child_thread_id)
                        .unwrap()
                        .as_ref()
                        == Some(row)
                );
            }
            clean(conn, scope, &source, |text, _| {
                (text.replace(DELEGATED_SECRET, "[masked]"), vec![])
            })
            .unwrap();
            for (index, child) in ["child", "sibling", "other-child"].into_iter().enumerate() {
                let cleaned = crate::subagents::find(conn, child).unwrap().unwrap();
                assert_eq!(delegation_identity(conn, child), identities[index]);
                assert_eq!(crate::threads::find(conn, child).unwrap(), threads[index]);
                if child == "child" || (scope == "parent" && child == "sibling") {
                    assert!(!cleaned.name.contains(DELEGATED_SECRET));
                    assert!(!cleaned.task.contains(DELEGATED_SECRET));
                    assert!(!cleaned.result.contains(DELEGATED_SECRET));
                    assert!(!cleaned.truncated);
                } else {
                    assert!(cleaned == original[index]);
                }
            }
            for thread in ["parent", "child", "sibling", "other", "other-child"] {
                assert_eq!(
                    user_text(conn, thread).contains(DELEGATED_SECRET),
                    thread != scope
                );
            }
            assert_eq!(exceptions(conn, "parent").unwrap().len(), 1);
            assert!(exceptions(conn, "child").unwrap().is_empty());
        }
    }

    #[test]
    fn m19_delegation_metadata_changes_invalidate_parent_and_child_previews() {
        for scope in ["parent", "child"] {
            for mutation in [
                "UPDATE delegated_runs SET name=name||' changed' WHERE child_thread_id='child'",
                "UPDATE delegated_runs SET task=task||' changed' WHERE child_thread_id='child'",
                "UPDATE delegated_runs SET result=result||' changed' WHERE child_thread_id='child'",
                "UPDATE delegated_runs SET state='failed' WHERE child_thread_id='child'",
                "UPDATE delegated_runs SET truncated=1 WHERE child_thread_id='child'",
            ] {
                let store = delegation_fixture();
                let conn = store.conn();
                let source = source_fingerprint(conn, scope).unwrap();
                conn.execute(mutation, []).unwrap();
                assert_ne!(source_fingerprint(conn, scope).unwrap(), source);
                assert!(
                    set_exception_checked(
                        conn,
                        scope,
                        &source,
                        "synthetic-rule",
                        "synthetic-fingerprint",
                        true
                    )
                    .is_err()
                );
                assert!(exceptions(conn, scope).unwrap().is_empty());
                let mut called = false;
                assert!(
                    clean(conn, scope, &source, |_, _| {
                        called = true;
                        ("[masked]".into(), vec![])
                    })
                    .is_err()
                );
                assert!(!called);
                assert_eq!(user_text(conn, scope), DELEGATED_SECRET);
                assert!(
                    crate::subagents::find(conn, "child")
                        .unwrap()
                        .unwrap()
                        .task
                        .contains(DELEGATED_SECRET)
                );
            }
        }
    }

    #[test]
    fn m19_active_delegation_without_streaming_message_rejects_cleanup() {
        for scope in ["parent", "child"] {
            for state in ["running", "cancelling"] {
                let store = delegation_fixture();
                let conn = store.conn();
                conn.execute(
                    "UPDATE delegated_runs SET state=?1 WHERE child_thread_id='child'",
                    [state],
                )
                .unwrap();
                let source = source_fingerprint(conn, scope).unwrap();
                let mut called = false;
                assert!(
                    clean(conn, scope, &source, |_, _| {
                        called = true;
                        ("[masked]".into(), vec![])
                    })
                    .is_err()
                );
                assert!(!called);
                assert_eq!(source_fingerprint(conn, scope).unwrap(), source);
                assert_eq!(user_text(conn, scope), DELEGATED_SECRET);
            }
        }
    }

    #[test]
    fn m19_delegation_cleanup_failure_rolls_back_transcript_and_all_metadata() {
        let store = delegation_fixture();
        let conn = store.conn();
        let source = source_fingerprint(conn, "parent").unwrap();
        conn.execute_batch("CREATE TRIGGER reject_sibling_cleanup BEFORE UPDATE ON delegated_runs WHEN OLD.child_thread_id='sibling' BEGIN SELECT RAISE(ABORT,'synthetic cleanup failure'); END;").unwrap();
        assert!(
            clean(conn, "parent", &source, |text, _| {
                (text.replace(DELEGATED_SECRET, "[masked]"), vec![])
            })
            .is_err()
        );
        assert_eq!(source_fingerprint(conn, "parent").unwrap(), source);
        for child in ["child", "sibling", "other-child"] {
            let row = crate::subagents::find(conn, child).unwrap().unwrap();
            assert!(row.name.contains(DELEGATED_SECRET));
            assert!(row.task.contains(DELEGATED_SECRET));
            assert!(row.result.contains(DELEGATED_SECRET));
        }
        assert_eq!(user_text(conn, "parent"), DELEGATED_SECRET);
    }

    #[test]
    fn m19_expanded_or_empty_redactions_preserve_bounds_and_unique_names_deterministically() {
        for replacement in [String::new(), "遮".repeat(40_000)] {
            let mut outcomes = Vec::new();
            for _ in 0..2 {
                let store = delegation_fixture();
                let conn = store.conn();
                let source = source_fingerprint(conn, "parent").unwrap();
                clean(conn, "parent", &source, |text, _| {
                    (
                        if text.contains(DELEGATED_SECRET) {
                            replacement.clone()
                        } else {
                            text.into()
                        },
                        vec![],
                    )
                })
                .unwrap();
                let rows = crate::subagents::list(conn, "parent", None).unwrap();
                assert_eq!(rows.len(), 2);
                assert_ne!(rows[0].name, rows[1].name);
                for row in &rows {
                    assert!(!row.name.trim().is_empty());
                    assert!(row.name.len() <= 256);
                    assert!(!row.name.chars().any(char::is_control));
                    assert!(!row.task.trim().is_empty());
                    assert!(row.task.len() <= 32 * 1024);
                    assert!(row.result.len() <= 16 * 1024);
                    assert_eq!(row.truncated, !replacement.is_empty());
                    assert!(!row.name.contains(DELEGATED_SECRET));
                    assert!(!row.task.contains(DELEGATED_SECRET));
                    assert!(!row.result.contains(DELEGATED_SECRET));
                    assert_eq!(row.state, "completed");
                }
                outcomes.push(
                    rows.into_iter()
                        .map(|row| (row.name, row.task, row.result, row.truncated))
                        .collect::<Vec<_>>(),
                );
            }
            assert_eq!(outcomes[0], outcomes[1]);
        }
    }

    #[test]
    fn m19_child_cleanup_resolves_name_collision_without_mutating_sibling() {
        let store = delegation_fixture();
        let conn = store.conn();
        conn.execute(
            "UPDATE delegated_runs SET name='[masked]' WHERE child_thread_id='sibling'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE delegated_runs SET truncated=1 WHERE child_thread_id='child'",
            [],
        )
        .unwrap();
        let sibling = crate::subagents::find(conn, "sibling").unwrap().unwrap();
        let source = source_fingerprint(conn, "child").unwrap();
        clean(conn, "child", &source, |text, _| {
            (
                if text.contains(DELEGATED_SECRET) {
                    "[masked]".into()
                } else {
                    text.into()
                },
                vec![],
            )
        })
        .unwrap();
        let child = crate::subagents::find(conn, "child").unwrap().unwrap();
        assert_ne!(child.name, sibling.name);
        assert!(child.name.len() <= 256);
        assert!(!child.name.contains(DELEGATED_SECRET));
        assert!(child.truncated);
        assert!(crate::subagents::find(conn, "sibling").unwrap().as_ref() == Some(&sibling));
    }

    #[test]
    fn unchanged_epoch_is_readable_while_another_connection_prepares_a_turn() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("epoch.db");
        let first = Store::open(&path).unwrap();
        first.migrate().unwrap();
        let second = Store::open(&path).unwrap();
        refresh_epoch(first.conn(), "synthetic-epoch").unwrap();
        let transaction = first.immediate_transaction().unwrap();
        refresh_epoch(second.conn(), "synthetic-epoch").unwrap();
        transaction.commit().unwrap();
    }

    #[test]
    fn concurrent_epoch_initialization_serializes_without_read_upgrade_failures() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("epoch.db");
        let setup = Store::open(&path).unwrap();
        setup.migrate().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let stores = [Store::open(&path).unwrap(), Store::open(&path).unwrap()];
        let handles = stores
            .into_iter()
            .map(|store| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    refresh_epoch(store.conn(), "synthetic-epoch").unwrap();
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
        let epoch: String = setup
            .conn()
            .query_row(
                "SELECT digest FROM credential_privacy_epoch WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(epoch, "synthetic-epoch");
    }
}
