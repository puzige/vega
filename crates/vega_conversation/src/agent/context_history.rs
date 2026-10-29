use super::*;
use vega_runtime::{ConversationHistoryReader, ConversationReadRequest};

pub(super) fn reader(database: PathBuf, thread: String) -> Arc<ConversationHistoryReader> {
    Arc::new(move |request| read(&database, &thread, request).ok_or_else(unavailable))
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
) -> Option<serde_json::Value> {
    if !(1..=8).contains(&request.limit)
        || request.after_sequence > i64::MAX as u64
        || request.content_offset > 1_048_576
        || request
            .source_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 128)
    {
        return None;
    }
    let store = Store::open_read_only(database).ok()?;
    let query = match request.kind.as_str() {
        "messages" => {
            "SELECT m.id,m.seq,m.role,m.kind,m.status,CASE WHEN length(CAST(m.content AS BLOB))<=1048576 THEN m.content END,(SELECT COUNT(*) FROM image_attachments i WHERE i.message_id=m.id),(SELECT COALESCE(SUM(length(i.encoded)),0) FROM image_attachments i WHERE i.message_id=m.id) FROM messages m JOIN threads t ON t.id=m.thread_id WHERE length(CAST(m.id AS BLOB)) BETWEEN 1 AND 128 AND length(CAST(m.role AS BLOB))<=128 AND length(CAST(m.kind AS BLOB))<=128 AND length(CAST(m.status AS BLOB))<=128 AND m.thread_id=?1 AND m.seq>?2 AND (?3 IS NULL OR m.id=?3) AND ?5='messages' ORDER BY m.seq LIMIT ?4"
        }
        "tools" | "skill_resources" => {
            "SELECT c.id,c.seq,c.message_id,c.tool,c.status,CASE WHEN length(CAST(c.input_json AS BLOB))<=1048576 THEN c.input_json END,CASE WHEN length(CAST(c.output_text AS BLOB))<=1048576 THEN c.output_text END FROM tool_calls c JOIN messages m ON m.id=c.message_id AND m.thread_id=c.thread_id AND m.role='assistant' JOIN threads t ON t.id=c.thread_id WHERE length(CAST(c.id AS BLOB)) BETWEEN 1 AND 128 AND length(CAST(c.message_id AS BLOB)) BETWEEN 1 AND 128 AND length(CAST(c.tool AS BLOB))<=256 AND length(CAST(c.status AS BLOB))<=128 AND c.thread_id=?1 AND c.seq>?2 AND (?3 IS NULL OR c.id=?3) AND (?5!='skill_resources' OR (c.tool='read_skill_resource' AND c.status='success')) ORDER BY c.seq LIMIT ?4"
        }
        _ => return None,
    };
    let mut statement = store.conn().prepare(query).ok()?;
    let mut rows = statement
        .query((
            thread,
            request.after_sequence as i64,
            request.source_id.as_deref(),
            request.limit as i64 + 1,
            request.kind.as_str(),
        ))
        .ok()?;
    let mut records = Vec::new();
    let mut next = request.after_sequence;
    let mut more = false;
    while let Some(row) = rows.next().ok()? {
        if records.len() == request.limit {
            more = true;
            break;
        }
        let id: String = row.get(0).ok()?;
        let sequence: i64 = row.get(1).ok()?;
        next = u64::try_from(sequence).ok()?;
        let a: String = row.get(2).ok()?;
        let b: String = row.get(3).ok()?;
        let status: String = row.get(4).ok()?;
        let content: Option<String> = row.get(5).ok()?;
        let record = if request.kind == "messages" {
            let count: i64 = row.get(6).ok()?;
            let bytes: i64 = row.get(7).ok()?;
            serde_json::json!({"source_id":id,"sequence":sequence,"role":a,"message_kind":b,
                "status":status,"content":content,"attachments":{"count":count,"encoded_bytes":bytes}})
        } else {
            let output: Option<String> = row.get(6).ok()?;
            serde_json::json!({"source_id":id,"sequence":sequence,"message_id":a,"tool":b,
                "status":status,"input":content,"output":output})
        };
        records.push(record);
    }
    Some(serde_json::json!({"records":records,"next_after_sequence":next,"has_more":more}))
}

#[cfg(test)]
mod tests;
