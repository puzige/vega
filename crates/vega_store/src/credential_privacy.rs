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
    let mut query = conn.prepare("SELECT id,'message',content FROM messages WHERE thread_id=?1 UNION ALL SELECT id,'tool.input',input_json FROM tool_calls WHERE thread_id=?1 UNION ALL SELECT id,'tool.output',output_text FROM tool_calls WHERE thread_id=?1 AND output_text IS NOT NULL UNION ALL SELECT CAST(id AS TEXT),'summary',summary FROM context_checkpoints WHERE thread_id=?1 UNION ALL SELECT id,'plan.note',plan_review_note FROM messages WHERE thread_id=?1 AND plan_review_note IS NOT NULL UNION ALL SELECT id,'pending.model',model_content FROM pending_messages WHERE thread_id=?1 AND model_content IS NOT NULL UNION ALL SELECT id,'pending.content',json_extract(payload,'$.content') FROM pending_messages WHERE thread_id=?1 AND payload != '' AND json_valid(payload) AND json_type(payload,'$.content')='text' UNION ALL SELECT child_message_id,'delegation.name',name FROM delegated_executions WHERE parent_thread_id=?1 OR child_thread_id=?1 UNION ALL SELECT child_message_id,'delegation.task',task FROM delegated_executions WHERE parent_thread_id=?1 OR child_thread_id=?1 UNION ALL SELECT child_message_id,'delegation.result',result FROM delegated_executions WHERE parent_thread_id=?1 OR child_thread_id=?1")?;
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
    let mut query = conn.prepare("SELECT child_message_id,json_array(child_thread_id,parent_thread_id,parent_message_id,request_call_id,ordinal,previous_child_message_id,kind,instruction_message_id,state,truncated,usage_complete,created_at) FROM delegated_executions WHERE parent_thread_id=?1 OR child_thread_id=?1")?;
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
        if conn.execute("UPDATE delegated_executions SET name=?1,task=?2,result=?3,truncated=?4 WHERE child_message_id=?5 AND state NOT IN ('running','cancelling')", params![name,task,result,truncated,row.child_message_id])? != 1 {
            return Err(rusqlite::Error::InvalidParameterName(
                "conversation changed; review again".into(),
            ));
        }
        conn.execute("UPDATE delegated_runs SET name=?1,task=?2,result=?3,truncated=?4 WHERE child_message_id=?5", params![name,task,result,truncated,row.child_message_id])?;
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
        "SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id=?1 AND status='streaming') OR EXISTS(SELECT 1 FROM delegated_executions WHERE (parent_thread_id=?1 OR child_thread_id=?1) AND state IN ('running','cancelling'))",
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
