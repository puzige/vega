use super::*;
use vega_runtime::{ConversationHistoryReader, ConversationReadRequest};

pub(super) fn reader(database: PathBuf, thread: String) -> Arc<ConversationHistoryReader> {
    Arc::new(move |request| read(&database, &thread, request).map_err(|_| unavailable()))
}

fn unavailable() -> VegaError {
    VegaError::Tool {
        tool: "read_conversation".into(),
        message: "Conversation details unavailable".into(),
    }
}

fn read(
    database: &std::path::Path,
    thread: &str,
    request: &ConversationReadRequest,
) -> rusqlite::Result<serde_json::Value> {
    if !(1..=8).contains(&request.limit)
        || request.after_sequence > i64::MAX as u64
        || request.content_offset > 1_048_576
        || request
            .source_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 128)
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let store = Store::open_read_only(database)?;
    let query = match request.kind.as_str() {
        "messages" => {
            "SELECT m.id,m.seq,m.role,m.kind,m.status,CASE WHEN length(CAST(m.content AS BLOB))<=1048576 THEN m.content END,(SELECT COUNT(*) FROM image_attachments i WHERE i.message_id=m.id),(SELECT COALESCE(SUM(length(i.encoded)),0) FROM image_attachments i WHERE i.message_id=m.id) FROM messages m JOIN threads t ON t.id=m.thread_id WHERE m.thread_id=?1 AND m.seq>?2 AND (?3 IS NULL OR m.id=?3) ORDER BY m.seq LIMIT ?4"
        }
        "tools" => {
            "SELECT c.id,c.seq,c.message_id,c.tool,c.status,CASE WHEN length(CAST(c.input_json AS BLOB))<=1048576 THEN c.input_json END,CASE WHEN length(CAST(c.output_text AS BLOB))<=1048576 THEN c.output_text END FROM tool_calls c JOIN messages m ON m.id=c.message_id AND m.thread_id=c.thread_id AND m.role='assistant' JOIN threads t ON t.id=c.thread_id WHERE c.thread_id=?1 AND c.seq>?2 AND (?3 IS NULL OR c.id=?3) ORDER BY c.seq LIMIT ?4"
        }
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let mut statement = store.conn().prepare(query)?;
    let mut rows = statement.query(rusqlite::params![
        thread,
        request.after_sequence as i64,
        request.source_id,
        request.limit as i64 + 1,
    ])?;
    let mut records = Vec::new();
    let mut next = request.after_sequence;
    let mut more = false;
    while let Some(row) = rows.next()? {
        if records.len() == request.limit {
            more = true;
            break;
        }
        let id: String = row.get(0)?;
        let sequence: i64 = row.get(1)?;
        next = u64::try_from(sequence).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let a: String = row.get(2)?;
        let b: String = row.get(3)?;
        let status: String = row.get(4)?;
        let content: Option<String> = row.get(5)?;
        let record = if request.kind == "messages" {
            let count: i64 = row.get(6)?;
            let bytes: i64 = row.get(7)?;
            serde_json::json!({"source_id":id,"sequence":sequence,"role":a,"message_kind":b,
                "status":status,"content":content,"attachments":{"count":count,"encoded_bytes":bytes}})
        } else {
            let output: Option<String> = row.get(6)?;
            serde_json::json!({"source_id":id,"sequence":sequence,"message_id":a,"tool":b,
                "status":status,"input":content,"output":output})
        };
        records.push(record);
    }
    Ok(serde_json::json!({"records":records,"next_after_sequence":next,"has_more":more}))
}

#[cfg(test)]
mod tests;
