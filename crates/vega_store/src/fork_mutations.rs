use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationOrigin {
    pub project_id: String,
    pub thread_id: String,
    pub call_id: String,
}

pub fn origin(conn: &Connection, thread: &str, call: &str) -> rusqlite::Result<MutationOrigin> {
    let mut thread = thread.to_owned();
    let mut call = call.to_owned();
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(thread.clone()) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let owner = crate::threads::find(conn, &thread)?.ok_or(rusqlite::Error::InvalidQuery)?;
        let (tool, message_seq): (String, i64) = conn.query_row(
            "SELECT c.tool,m.seq FROM tool_calls c JOIN messages m ON m.id=c.message_id WHERE c.id=?1 AND c.thread_id=?2 AND m.thread_id=?2 AND m.role='assistant'",
            params![call, thread],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if !matches!(tool.as_str(), "write" | "Write" | "edit" | "Edit") {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let prefix: Option<i64> = conn.query_row(
            "SELECT MAX(prefix_seq) FROM thread_timestamp_fork_prefixes WHERE thread_id=?1",
            [&thread],
            |row| row.get(0),
        )?;
        let saved = conn.query_row(
            "SELECT project_id,source_thread_id,source_call_id FROM fork_mutation_origins WHERE call_id=?1",
            [&call],
            |row| Ok(MutationOrigin { project_id: row.get(0)?, thread_id: row.get(1)?, call_id: row.get(2)? }),
        ).optional()?;
        if let Some(saved) = saved {
            if saved.project_id != owner.project_id
                || saved.thread_id == thread
                || saved.call_id == call
                || !prefix.is_some_and(|prefix| message_seq <= prefix)
            {
                return Err(rusqlite::Error::InvalidQuery);
            }
            return Ok(saved);
        }
        let current = MutationOrigin {
            project_id: owner.project_id.clone(),
            thread_id: thread.clone(),
            call_id: call.clone(),
        };
        if !prefix.is_some_and(|prefix| message_seq <= prefix) {
            return Ok(current);
        }
        let (Some(parent), Some(cutoff)) = (
            owner.forked_from_thread_id.as_deref(),
            owner.forked_from_message_id.as_deref(),
        ) else {
            return Err(rusqlite::Error::InvalidQuery);
        };
        let cutoff = crate::messages::find(conn, cutoff)?.ok_or(rusqlite::Error::InvalidQuery)?;
        if cutoff.thread_id != parent || cutoff.role != "assistant" || cutoff.status != "done" {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if prefix != Some(cutoff.seq) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let source = crate::threads::find(conn, parent)?.ok_or(rusqlite::Error::InvalidQuery)?;
        if source.project_id != owner.project_id {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let ancestor: String = conn.query_row(
            "SELECT s.id FROM tool_calls c JOIN messages m ON m.id=c.message_id JOIN messages n ON n.thread_id=?2 AND n.seq=m.seq JOIN tool_calls s ON s.message_id=n.id AND s.thread_id=?2 AND s.seq=c.seq WHERE c.id=?1 AND c.thread_id=?3 AND m.thread_id=?3 AND n.role=m.role AND n.kind=m.kind AND n.status=m.status AND n.content=m.content AND n.created_at=m.created_at AND n.plan_status IS m.plan_status AND n.plan_review_note IS m.plan_review_note AND n.plan_reviewed_at IS m.plan_reviewed_at AND s.tool=c.tool AND s.input_json=c.input_json AND s.output_text IS c.output_text AND s.status=c.status AND s.approval IS c.approval AND s.exit_code IS c.exit_code AND s.duration_ms IS c.duration_ms AND s.text_offset_bytes IS c.text_offset_bytes AND s.created_at=c.created_at AND s.finished_at IS c.finished_at AND s.output_full_path IS c.output_full_path AND s.description IS c.description",
            params![call, parent, thread],
            |row| row.get(0),
        )?;
        thread = parent.to_owned();
        call = ancestor;
    }
}

pub fn copy_origin(
    conn: &Connection,
    source_thread: &str,
    source_call: &str,
    destination_call: &str,
) -> rusqlite::Result<()> {
    let origin = origin(conn, source_thread, source_call)?;
    conn.execute(
        "INSERT INTO fork_mutation_origins(call_id,project_id,source_thread_id,source_call_id) VALUES(?1,?2,?3,?4)",
        params![destination_call, origin.project_id, origin.thread_id, origin.call_id],
    )?;
    Ok(())
}
