use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantResponseRow {
    pub message_id: String,
    pub response_id: String,
    pub text_start_bytes: usize,
    pub text_end_bytes: usize,
    pub kind: String,
}

impl AssistantResponseRow {
    fn valid_for(&self, content: &str) -> bool {
        !self.response_id.is_empty()
            && self.response_id.len() <= 128
            && !self.response_id.chars().any(char::is_control)
            && matches!(self.kind.as_str(), "commentary" | "final")
            && self.text_start_bytes < self.text_end_bytes
            && content.is_char_boundary(self.text_start_bytes)
            && content.is_char_boundary(self.text_end_bytes)
            && (self.kind != "final"
                || (self.text_end_bytes == content.len()
                    && content
                        .get(self.text_start_bytes..self.text_end_bytes)
                        .is_some_and(|text| !text.trim().is_empty())))
    }
}

pub fn record_assistant_response(
    conn: &Connection,
    message_id: &str,
    content: &str,
    response: &AssistantResponseRow,
) -> Result<bool, rusqlite::Error> {
    if response.message_id != message_id || !response.valid_for(content) {
        return Ok(false);
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let owner = find(&tx, message_id)?;
    if !owner.is_some_and(|owner| {
        owner.role == "assistant" && owner.status == "streaming" && owner.content == content
    }) {
        return Ok(false);
    }
    let existing = read_for_message(&tx, message_id)?;
    if let Some(previous) = existing
        .iter()
        .find(|row| row.response_id == response.response_id)
    {
        return Ok(previous == response);
    }
    if existing
        .iter()
        .any(|row| row.kind == "final" || row.text_end_bytes > response.text_start_bytes)
    {
        return Ok(false);
    }
    let (Ok(start), Ok(end)) = (
        i64::try_from(response.text_start_bytes),
        i64::try_from(response.text_end_bytes),
    ) else {
        return Ok(false);
    };
    tx.execute(
        "INSERT INTO assistant_responses (message_id, response_id, text_start_bytes, text_end_bytes, kind) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![message_id, response.response_id, start, end, response.kind],
    )?;
    tx.commit()?;
    Ok(true)
}

fn row_from_query(row: &rusqlite::Row<'_>) -> Result<AssistantResponseRow, rusqlite::Error> {
    let start: i64 = row.get(2)?;
    let end: i64 = row.get(3)?;
    Ok(AssistantResponseRow {
        message_id: row.get(0)?,
        response_id: row.get(1)?,
        text_start_bytes: usize::try_from(start).unwrap_or(usize::MAX),
        text_end_bytes: usize::try_from(end).unwrap_or(usize::MAX),
        kind: row.get(4)?,
    })
}

fn read_for_message(
    conn: &Connection,
    message_id: &str,
) -> Result<Vec<AssistantResponseRow>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT message_id, response_id, text_start_bytes, text_end_bytes, kind FROM assistant_responses WHERE message_id = ?1 ORDER BY text_start_bytes")?;
    stmt.query_map([message_id], row_from_query)?.collect()
}

pub(super) fn for_page(
    conn: &Connection,
    thread_id: &str,
    rows: &[MessageRow],
) -> Result<Vec<AssistantResponseRow>, rusqlite::Error> {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare("SELECT r.message_id, r.response_id, r.text_start_bytes, r.text_end_bytes, r.kind FROM assistant_responses r JOIN messages m ON m.id = r.message_id WHERE m.thread_id = ?1 AND m.seq >= ?2 AND m.seq <= ?3 AND m.status != 'streaming' ORDER BY m.seq, r.text_start_bytes")?;
    let candidates = stmt
        .query_map(params![thread_id, first.seq, last.seq], row_from_query)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut validated = Vec::new();
    for owner in rows {
        let responses: Vec<_> = candidates
            .iter()
            .filter(|response| response.message_id == owner.id)
            .collect();
        let mut end = 0;
        let mut final_seen = false;
        let valid = responses.iter().all(|response| {
            let valid = response.valid_for(&owner.content)
                && response.text_start_bytes >= end
                && !final_seen;
            end = response.text_end_bytes;
            final_seen = response.kind == "final";
            valid
        });
        if valid {
            validated.extend(
                responses
                    .into_iter()
                    .filter(|response| response.kind != "final" || owner.status == "done")
                    .cloned(),
            );
        }
    }
    Ok(validated)
}
