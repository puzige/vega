use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ForkError {
    #[error("source thread does not exist")]
    MissingSource,
    #[error("source must be a top-level Native project thread")]
    UnsupportedSource,
    #[error("cutoff must be a completed assistant with a trusted Final")]
    InvalidCutoff,
    #[error("source has persisted active work")]
    ActiveSource,
    #[error("corrupt fork history: {0}")]
    CorruptHistory(String),
    #[error("fork storage failed")]
    Store(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalReference {
    pub field: String,
    pub source_id: String,
    pub destination_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalToolReceipt {
    pub call_id: String,
    pub kind: String,
    pub source_thread_id: String,
    pub source_message_id: String,
    pub source_call_id: String,
    pub input_json: String,
    pub output_text: Option<String>,
    pub references: Vec<HistoricalReference>,
}

pub fn historical_receipts(
    conn: &Connection,
    thread_id: &str,
) -> rusqlite::Result<Vec<HistoricalToolReceipt>> {
    historical_receipts_in_range(conn, thread_id, i64::MIN, i64::MAX)
}

pub fn historical_receipts_in_range(
    conn: &Connection,
    thread_id: &str,
    first: i64,
    last: i64,
) -> rusqlite::Result<Vec<HistoricalToolReceipt>> {
    conn.prepare("SELECT h.call_id,h.kind,h.source_thread_id,h.source_message_id,h.source_call_id,h.input_json,h.output_text,h.references_json FROM historical_tool_receipts h JOIN tool_calls c ON c.id=h.call_id JOIN messages m ON m.id=c.message_id WHERE h.thread_id=?1 AND c.thread_id=?1 AND m.thread_id=?1 AND m.seq>=?2 AND m.seq<=?3 ORDER BY c.seq,c.id")?
        .query_map(params![thread_id, first, last], |row| {
            let references: String = row.get(7)?;
            Ok(HistoricalToolReceipt {
                call_id: row.get(0)?,
                kind: row.get(1)?,
                source_thread_id: row.get(2)?,
                source_message_id: row.get(3)?,
                source_call_id: row.get(4)?,
                input_json: row.get(5)?,
                output_text: row.get(6)?,
                references: serde_json::from_str(&references).map_err(|_| rusqlite::Error::InvalidQuery)?,
            })
        })?.collect()
}

pub fn has_archived_delegation(conn: &Connection, thread: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM historical_tool_receipts WHERE thread_id=?1 AND kind='delegation')", [thread], |row| row.get(0))
}

fn fresh_id() -> String {
    ulid::Ulid::generate().to_string()
}

fn corrupt(reason: &str) -> ForkError {
    ForkError::CorruptHistory(reason.into())
}

fn mapped<'a>(map: &'a BTreeMap<String, String>, id: &str) -> Result<&'a str, ForkError> {
    map.get(id)
        .map(String::as_str)
        .ok_or_else(|| corrupt("unresolved prefix reference"))
}

pub fn clone_prefix(
    tx: &Transaction<'_>,
    source_thread_id: &str,
    through_message_id: &str,
    now: i64,
) -> Result<crate::threads::ThreadRow, ForkError> {
    let source = crate::threads::find(tx, source_thread_id)?.ok_or(ForkError::MissingSource)?;
    if source.backend != "native"
        || source.parent_thread_id.is_some()
        || source.workspace_owner_thread_id.is_some()
        || source.project_id.is_empty()
        || !crate::projects::project_exists(tx, &source.project_id)?
    {
        return Err(ForkError::UnsupportedSource);
    }
    let active: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id=?1 AND status='streaming') OR EXISTS(SELECT 1 FROM tool_calls WHERE thread_id=?1 AND status NOT IN ('success','failed','cancelled','rejected'))",
        [source_thread_id], |row| row.get(0),
    )?;
    if active {
        return Err(ForkError::ActiveSource);
    }
    let cutoff = crate::messages::find(tx, through_message_id)?.ok_or(ForkError::InvalidCutoff)?;
    if cutoff.thread_id != source_thread_id
        || cutoff.role != "assistant"
        || cutoff.status != "done"
        || crate::messages::completed_response_text(tx, through_message_id)?.is_none()
    {
        return Err(ForkError::InvalidCutoff);
    }
    let owners = tx
        .prepare("SELECT id FROM messages WHERE thread_id=?1 AND seq<=?2 ORDER BY seq,id")?
        .query_map(params![source_thread_id, cutoff.seq], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let messages: BTreeMap<_, _> = owners.iter().map(|id| (id.clone(), fresh_id())).collect();
    let calls = tx.prepare("SELECT c.id,c.message_id,c.tool,c.output_text,c.input_json FROM tool_calls c JOIN messages m ON m.id=c.message_id WHERE c.thread_id=?1 AND m.thread_id=?1 AND m.seq<=?2 ORDER BY c.seq,c.id")?
        .query_map(params![source_thread_id,cutoff.seq], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,Option<String>>(3)?,row.get::<_,String>(4)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let call_map: BTreeMap<_, _> = calls
        .iter()
        .map(|call| (call.0.clone(), fresh_id()))
        .collect();
    let orphan: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM tool_calls c JOIN messages m ON m.id=c.message_id WHERE m.thread_id=?1 AND m.seq<=?2 AND (c.thread_id<>?1 OR m.role<>'assistant')) OR EXISTS(SELECT 1 FROM tool_calls c JOIN messages m ON m.id=c.message_id WHERE c.thread_id=?1 AND m.thread_id<>?1)", params![source_thread_id,cutoff.seq], |row| row.get(0))?;
    if orphan {
        return Err(corrupt("invalid tool owner"));
    }
    let invalid_calls: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM tool_calls c JOIN messages m ON m.id=c.message_id WHERE m.thread_id=?1 AND m.seq<=?2 AND (c.seq<1 OR c.duration_ms<0 OR c.output_text IS NULL OR (c.text_offset_bytes IS NOT NULL AND c.text_offset_bytes<0))) OR EXISTS(SELECT 1 FROM tool_calls c LEFT JOIN messages m ON m.id=c.message_id WHERE c.thread_id=?1 AND m.id IS NULL)", params![source_thread_id,cutoff.seq], |row| row.get(0))?;
    if invalid_calls {
        return Err(corrupt("invalid terminal tool metadata"));
    }
    let duplicate_seq: bool = tx.query_row("SELECT EXISTS(SELECT c.seq FROM tool_calls c JOIN messages m ON m.id=c.message_id WHERE m.thread_id=?1 AND m.seq<=?2 GROUP BY c.seq HAVING COUNT(*)>1)", params![source_thread_id,cutoff.seq], |row| row.get(0))?;
    if duplicate_seq {
        return Err(corrupt("duplicate tool sequence"));
    }
    let destination = fresh_id();
    tx.execute("INSERT INTO threads(id,project_id,title,mode,permission_mode,model,backend,status,pinned,unread,created_at,updated_at,forked_from_thread_id,forked_from_message_id) VALUES(?1,?2,?3,?4,'confirm',?5,'native','active',0,0,?6,?6,?7,?8)",params![destination,source.project_id,source.title,source.mode,source.model,now,source_thread_id,through_message_id])?;
    if let Some(preferences) = &source.reasoning_preferences {
        crate::threads::set_reasoning_preferences(tx, &destination, preferences, false)?;
    }
    for id in &owners {
        validate_responses(tx, id)?;
        let message =
            crate::messages::find(tx, id)?.ok_or_else(|| corrupt("missing prefix message"))?;
        if !matches!(message.role.as_str(), "user" | "assistant")
            || !matches!(message.kind.as_str(), "text" | "plan" | "error" | "summary")
            || message.seq < 1
            || !matches!(message.status.as_str(), "done" | "interrupted" | "failed")
            || (message.role == "user" && (message.kind != "text" || message.status != "done"))
        {
            return Err(corrupt("invalid prefix message"));
        }
        let new_id = mapped(&messages, id)?;
        tx.execute("INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at,plan_status,plan_review_note,plan_reviewed_at) SELECT ?1,?2,seq,role,kind,content,status,created_at,plan_status,plan_review_note,plan_reviewed_at FROM messages WHERE id=?3",params![new_id,destination,id])?;
    }
    for (id, owner, _, _, _) in &calls {
        tx.execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,output_full_path,status,approval,exit_code,duration_ms,created_at,finished_at,text_offset_bytes,description) SELECT ?1,?2,?3,seq,tool,input_json,output_text,output_full_path,status,approval,exit_code,duration_ms,created_at,finished_at,text_offset_bytes,description FROM tool_calls WHERE id=?4",params![mapped(&call_map,id)?,destination,mapped(&messages,owner)?,id])?;
    }
    for id in &owners {
        let new_id = mapped(&messages, id)?;
        copy_responses(tx, id, new_id, &call_map)?;
        crate::messages::copy_generated_notices_in_transaction(tx, id, new_id)?;
        for (table, columns) in [
            ("image_attachments", "ordinal,encoded"),
            ("assistant_run_durations", "execution_duration_ms"),
            ("builtin_skill_activations", "name,version,content_sha256"),
        ] {
            tx.execute(&format!("INSERT INTO {table}(message_id,{columns}) SELECT ?1,{columns} FROM {table} WHERE message_id=?2"),params![new_id,id])?;
        }
        tx.execute("INSERT INTO skill_activation_audits(run_id,thread_id,name,source_scope,content_sha256,origin,status,created_at) SELECT ?1,?2,name,source_scope,content_sha256,origin,status,created_at FROM skill_activation_audits WHERE thread_id=?3 AND run_id=?4 ORDER BY id",params![new_id,destination,source_thread_id,id])?;
        copy_delivery(tx, source_thread_id, &destination, id, new_id, &messages)?;
    }
    for (id, owner, tool, _, _) in &calls {
        let new_id = mapped(&call_map, id)?;
        let artifact: Option<(String, i64)> = tx
            .query_row(
                "SELECT message_id,ordinal FROM computer_screenshot_artifacts WHERE call_id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((message, ordinal)) = artifact {
            if tool != "ComputerScreenshot" || message != *owner {
                return Err(corrupt("invalid screenshot capture owner"));
            }
            tx.execute("INSERT INTO computer_screenshot_artifacts(call_id,message_id,ordinal) VALUES(?1,?2,?3)",params![new_id,mapped(&messages,&message)?,ordinal])?;
        }
    }
    let archives: BTreeMap<_, _> =
        historical_receipts_in_range(tx, source_thread_id, i64::MIN, cutoff.seq)?
            .into_iter()
            .map(|receipt| (receipt.call_id.clone(), receipt))
            .collect();
    for (id, owner, tool, output, input) in &calls {
        let new_id = mapped(&call_map, id)?;
        let mut references = vec![
            HistoricalReference {
                field: "parent_thread_id".into(),
                source_id: source_thread_id.into(),
                destination_id: Some(destination.clone()),
            },
            HistoricalReference {
                field: "parent_message_id".into(),
                source_id: owner.clone(),
                destination_id: Some(mapped(&messages, owner)?.into()),
            },
            HistoricalReference {
                field: "request_call_id".into(),
                source_id: id.clone(),
                destination_id: Some(new_id.into()),
            },
        ];
        let mut kind = None;
        if is_delegation(tool) {
            kind = Some("delegation");
            delegation_references(input, output.as_deref(), &mut references)?;
        }
        if tool == "ComputerScreenshot"
            && let Some(output) = output
            && let Some(projected) = screenshot_receipt(tx, id, output, &call_map, &mut references)?
        {
            kind = Some("screenshot");
            tx.execute(
                "UPDATE tool_calls SET output_text=?1 WHERE id=?2",
                params![projected, new_id],
            )?;
        }
        if let Some(kind) = kind {
            let (source_thread, source_message, source_call, original_input, original_output) =
                if let Some(archive) = archives.get(id) {
                    for reference in &archive.references {
                        let mut reference = reference.clone();
                        if let Some(id) = reference.destination_id.as_deref() {
                            reference.destination_id = Some(if id == source_thread_id {
                                destination.clone()
                            } else {
                                messages
                                    .get(id)
                                    .or_else(|| call_map.get(id))
                                    .ok_or_else(|| corrupt("unresolved archived local reference"))?
                                    .clone()
                            });
                        }
                        references.push(reference);
                    }
                    (
                        archive.source_thread_id.as_str(),
                        archive.source_message_id.as_str(),
                        archive.source_call_id.as_str(),
                        archive.input_json.as_str(),
                        archive.output_text.as_deref(),
                    )
                } else {
                    (
                        source_thread_id,
                        owner.as_str(),
                        id.as_str(),
                        input.as_str(),
                        output.as_deref(),
                    )
                };
            let references = serde_json::to_string(&references)
                .map_err(|_| corrupt("invalid historical references"))?;
            tx.execute("INSERT INTO historical_tool_receipts(call_id,thread_id,kind,source_thread_id,source_message_id,source_call_id,input_json,output_text,references_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![new_id,destination,kind,source_thread,source_message,source_call,original_input,original_output,references])?;
        }
    }
    validate_images(tx, &destination)?;
    crate::threads::find(tx, &destination)?.ok_or_else(|| corrupt("destination missing"))
}

fn validate_responses(conn: &Connection, message_id: &str) -> Result<(), ForkError> {
    let owner =
        crate::messages::find(conn, message_id)?.ok_or_else(|| corrupt("message missing"))?;
    let rows = conn.prepare("SELECT response_id,text_start_bytes,text_end_bytes,kind FROM assistant_responses WHERE message_id=?1 ORDER BY text_start_bytes,response_id")?
        .query_map([message_id],|row| Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,i64>(2)?,row.get::<_,String>(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut previous_end = 0;
    let mut final_seen = false;
    for (id, start, end, kind) in rows {
        let start = usize::try_from(start).map_err(|_| corrupt("invalid response range"))?;
        let end = usize::try_from(end).map_err(|_| corrupt("invalid response range"))?;
        if owner.role != "assistant"
            || id.is_empty()
            || id.len() > 128
            || id.chars().any(char::is_control)
            || start < previous_end
            || start >= end
            || owner.content.get(start..end).is_none()
            || final_seen
            || !matches!(kind.as_str(), "commentary" | "final")
            || (kind == "final"
                && (owner.status != "done"
                    || end != owner.content.len()
                    || owner.content[start..end].trim().is_empty()))
        {
            return Err(corrupt("invalid response range"));
        }
        previous_end = end;
        final_seen = kind == "final";
    }
    Ok(())
}

fn copy_responses(
    conn: &Connection,
    source: &str,
    destination: &str,
    calls: &BTreeMap<String, String>,
) -> Result<(), ForkError> {
    let ids=conn.prepare("SELECT response_id FROM assistant_responses WHERE message_id=?1 UNION SELECT response_id FROM tool_call_rounds WHERE message_id=?1")?
        .query_map([source],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err(corrupt("invalid response identity"));
        }
        let new_id = fresh_id();
        conn.execute("INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) SELECT ?1,?2,text_start_bytes,text_end_bytes,kind FROM assistant_responses WHERE message_id=?3 AND response_id=?4",params![destination,new_id,source,id])?;
        let rounds=conn.prepare("SELECT r.call_id FROM tool_call_rounds r LEFT JOIN tool_calls c ON c.id=r.call_id WHERE r.message_id=?1 AND r.response_id=?2 AND c.message_id=r.message_id")?
            .query_map(params![source,id],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tool_call_rounds WHERE message_id=?1 AND response_id=?2",
            params![source, id],
            |row| row.get(0),
        )?;
        if count != rounds.len() as i64 {
            return Err(corrupt("invalid tool round owner"));
        }
        for call in rounds {
            conn.execute(
                "INSERT INTO tool_call_rounds(message_id,call_id,response_id) VALUES(?1,?2,?3)",
                params![destination, mapped(calls, &call)?, new_id],
            )?;
        }
    }
    Ok(())
}

fn copy_delivery(
    conn: &Connection,
    source_thread: &str,
    destination_thread: &str,
    source: &str,
    destination: &str,
    messages: &BTreeMap<String, String>,
) -> Result<(), ForkError> {
    let fact:Option<(Option<String>,Option<String>)>=conn.query_row("SELECT model_content,previous_assistant_id FROM historical_message_delivery WHERE message_id=?1 AND thread_id=?2 UNION ALL SELECT model_content,accepted_after_assistant_id FROM pending_messages WHERE id=?1 AND thread_id=?2 AND state='delivered' AND NOT EXISTS(SELECT 1 FROM historical_message_delivery WHERE message_id=?1)",params![source,source_thread],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
    if let Some((model, previous)) = fact {
        let valid: bool = conn.query_row(
            "SELECT role='user' AND status='done' FROM messages WHERE id=?1",
            [source],
            |row| row.get(0),
        )?;
        if !valid {
            return Err(corrupt("invalid delivered message owner"));
        }
        if let Some(previous) = &previous {
            let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM messages p JOIN messages u ON u.id=?1 WHERE p.id=?2 AND p.thread_id=u.thread_id AND p.role='assistant' AND p.seq<u.seq)",params![source,previous],|row|row.get(0))?;
            if !valid {
                return Err(corrupt("invalid continuation predecessor"));
            }
        }
        let previous = previous
            .as_deref()
            .map(|id| mapped(messages, id))
            .transpose()?;
        conn.execute("INSERT INTO historical_message_delivery(message_id,thread_id,model_content,previous_assistant_id) VALUES(?1,?2,?3,?4)",params![destination,destination_thread,model,previous])?;
    }
    Ok(())
}

fn validate_images(conn: &Connection, thread: &str) -> Result<(), ForkError> {
    let invalid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM image_attachments a JOIN messages m ON m.id=a.message_id LEFT JOIN computer_screenshot_artifacts s ON s.message_id=a.message_id AND s.ordinal=a.ordinal LEFT JOIN tool_calls c ON c.id=s.call_id WHERE m.thread_id=?1 AND NOT COALESCE(((m.role='user' AND s.call_id IS NULL) OR (m.role='assistant' AND c.message_id=m.id AND c.thread_id=m.thread_id AND c.tool='ComputerScreenshot')),0)) ",[thread],|row|row.get(0))?;
    if invalid {
        return Err(corrupt("invalid image owner"));
    }
    let captures = conn.prepare("SELECT s.call_id FROM computer_screenshot_artifacts s JOIN messages m ON m.id=s.message_id WHERE m.thread_id=?1 AND m.role='assistant'")?
        .query_map([thread], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for capture in captures {
        if !valid_screenshot_capture(conn, &capture)? {
            return Err(corrupt("invalid image owner"));
        }
    }
    Ok(())
}

fn valid_screenshot_capture(conn: &Connection, call_id: &str) -> Result<bool, ForkError> {
    let Some(call) = crate::tool_calls::find_state(conn, call_id)? else {
        return Ok(false);
    };
    let attached: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM computer_screenshot_artifacts s JOIN image_attachments a ON a.message_id=s.message_id AND a.ordinal=s.ordinal JOIN messages m ON m.id=s.message_id JOIN tool_calls c ON c.id=s.call_id WHERE c.id=?1 AND c.message_id=m.id AND c.thread_id=m.thread_id AND m.role='assistant' AND length(a.encoded)>0)", [call_id], |row| row.get(0))?;
    if call.tool != "ComputerScreenshot" || !attached {
        return Ok(false);
    }
    if call.status == "success" {
        return Ok(true);
    }
    Ok(call.status == "cancelled" && valid_delivered_screenshot(&call, call_id, call_id))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScreenshotApproval {
    decision: String,
    note: serde_json::Value,
    source: String,
    danger: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScreenshotInputAudit {
    kind: String,
    version: u32,
    action: String,
    argument_bytes: usize,
    argument_digest: String,
}

fn valid_delivered_screenshot(
    call: &crate::tool_calls::ToolCallState,
    call_id: &str,
    capture: &str,
) -> bool {
    if call.tool != "ComputerScreenshot"
        || !matches!(call.status.as_str(), "success" | "cancelled")
        || call.exit_code.is_some()
        || call.duration_ms.is_some()
    {
        return false;
    }
    let Some(approval) = call
        .approval
        .as_deref()
        .and_then(|raw| serde_json::from_str::<ScreenshotApproval>(raw).ok())
    else {
        return false;
    };
    if approval.decision != "once"
        || approval.source != "computer_use"
        || !approval.note.is_null()
        || !approval.danger.is_null()
    {
        return false;
    }
    let Ok(input) = serde_json::from_str::<ScreenshotInputAudit>(&call.input_json) else {
        return false;
    };
    if input.kind != "computer_use"
        || input.version != 1
        || input.action != "screenshot"
        || input.argument_bytes > 256 * 1024
        || input.argument_digest.len() != 64
        || !input
            .argument_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return false;
    }
    let Some(output) = call
        .output_text
        .as_deref()
        .filter(|output| output.len() <= 2048)
    else {
        return false;
    };
    let Ok(mut receipt) = serde_json::from_str::<serde_json::Value>(output) else {
        return false;
    };
    if let Some(projection) = receipt
        .as_object_mut()
        .and_then(|object| object.remove("model_projection"))
    {
        let legacy = serde_json::json!({"code":"observation_budget_exceeded","executed":true,"deferred":true,"retryable":true,"next_step":"use_current_results_then_request_remaining_windows"});
        let mut structured = legacy.clone();
        structured["capability"] = "desktop".into();
        structured["stage"] = "screenshot".into();
        structured["execution_state"] = "failed".into();
        structured["field"] = serde_json::Value::Null;
        structured["diagnostic"] =
            "本批观察已达到容量限制，请先使用已有结果，再分批读取其余窗口。".into();
        if projection != legacy && projection != structured {
            return false;
        }
    }
    receipt
        == serde_json::json!({"status":"delivered","artifact_id":format!("screenshot:{capture}"),"delivery":"conversation_image_attachment","saved_in_history":true,"read_only":true,"unchanged":call_id != capture,"next_step":"continue_remaining_requested_work_or_finish"})
}

fn screenshot_receipt(
    conn: &Connection,
    alias: &str,
    output: &str,
    calls: &BTreeMap<String, String>,
    references: &mut Vec<HistoricalReference>,
) -> Result<Option<String>, ForkError> {
    let (first, rest) = output
        .split_once('\n')
        .map_or((output, ""), |(first, rest)| {
            (first, &output[first.len()..first.len() + 1 + rest.len()])
        });
    let Ok(mut receipt) = serde_json::from_str::<serde_json::Value>(first) else {
        return Ok(None);
    };
    let Some(artifact) = receipt
        .get("artifact_id")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(None);
    };
    let Some(capture) = artifact.strip_prefix("screenshot:") else {
        return Err(corrupt("invalid screenshot artifact identity"));
    };
    let new_capture = mapped(calls, capture)?;
    let valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM computer_screenshot_artifacts s JOIN tool_calls c ON c.id=s.call_id JOIN tool_calls a ON a.id=?1 JOIN messages cm ON cm.id=c.message_id JOIN messages am ON am.id=a.message_id WHERE s.call_id=?2 AND c.thread_id=a.thread_id AND c.tool='ComputerScreenshot' AND (cm.seq<am.seq OR (cm.seq=am.seq AND c.seq<=a.seq)))",params![alias,capture],|row|row.get(0))?;
    if !valid || !valid_screenshot_capture(conn, capture)? {
        return Err(corrupt("unresolved screenshot capture"));
    }
    if crate::tool_calls::find_state(conn, capture)?.is_some_and(|call| call.status == "cancelled")
        && !crate::tool_calls::find_state(conn, alias)?
            .is_some_and(|call| valid_delivered_screenshot(&call, alias, capture))
    {
        return Err(corrupt("invalid screenshot receipt"));
    }
    references.push(HistoricalReference {
        field: "artifact_id".into(),
        source_id: capture.into(),
        destination_id: Some(new_capture.into()),
    });
    receipt["artifact_id"] = format!("screenshot:{new_capture}").into();
    let projected =
        serde_json::to_string(&receipt).map_err(|_| corrupt("invalid screenshot receipt"))?;
    Ok(Some(format!("{projected}{rest}")))
}

fn is_delegation(tool: &str) -> bool {
    matches!(
        tool,
        "spawn_agent"
            | "followup_agent"
            | "wait_agent"
            | "list_agents"
            | "cancel_agent"
            | "checkpoint_stage"
            | "resume_agent"
    )
}

pub fn delegation_references(
    input: &str,
    output: Option<&str>,
    references: &mut Vec<HistoricalReference>,
) -> Result<(), ForkError> {
    fn reference(
        value: &serde_json::Value,
        path: &str,
        fields: &[&str],
        references: &mut Vec<HistoricalReference>,
    ) {
        for field in fields {
            if let Some(id) = value.get(field).and_then(serde_json::Value::as_str) {
                references.push(HistoricalReference {
                    field: format!("{path}.{field}"),
                    source_id: id.into(),
                    destination_id: None,
                });
            }
        }
    }
    fn checkpoint(
        value: &serde_json::Value,
        path: &str,
        references: &mut Vec<HistoricalReference>,
    ) {
        reference(
            value,
            path,
            &[
                "id",
                "checkpoint_id",
                "child_thread_id",
                "child_message_id",
                "execution_id",
            ],
            references,
        );
        let payload = value.get("payload").unwrap_or(value);
        if let Some(artifacts) = payload
            .get("artifacts")
            .and_then(serde_json::Value::as_array)
        {
            for (index, artifact) in artifacts.iter().enumerate() {
                reference(
                    artifact,
                    &format!("{path}.artifacts[{index}]"),
                    &["read_call_id"],
                    references,
                );
            }
        }
        if let Some(ids) = payload
            .get("evidence_call_ids")
            .and_then(serde_json::Value::as_array)
        {
            for (index, id) in ids.iter().enumerate() {
                if let Some(id) = id.as_str() {
                    references.push(HistoricalReference {
                        field: format!("{path}.evidence_call_ids[{index}]"),
                        source_id: id.into(),
                        destination_id: None,
                    });
                }
            }
        }
    }
    let input: serde_json::Value =
        serde_json::from_str(input).map_err(|_| corrupt("invalid delegation input"))?;
    checkpoint(&input, "input", references);
    if let Some(rows) = input
        .get("reconciliations")
        .and_then(serde_json::Value::as_array)
    {
        for (index, row) in rows.iter().enumerate() {
            let path = format!("input.reconciliations[{index}]");
            reference(row, &path, &["call_id"], references);
            checkpoint(row, &path, references);
        }
    }
    reference(
        &input,
        "input",
        &[
            "agent_id",
            "message_id",
            "checkpoint_id",
            "execution_id",
            "read_id",
        ],
        references,
    );
    if let Some(output) = output
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(output)
    {
        if let Some(agent) = value.get("agent") {
            reference(
                agent,
                "agent",
                &[
                    "agent_id",
                    "message_id",
                    "parent_thread_id",
                    "parent_message_id",
                    "execution_id",
                    "checkpoint_id",
                ],
                references,
            );
            if let Some(value) = agent.get("checkpoint") {
                checkpoint(value, "agent.checkpoint", references);
            }
        }
        if let Some(agents) = value.get("agents").and_then(serde_json::Value::as_array) {
            for (index, agent) in agents.iter().enumerate() {
                reference(
                    agent,
                    &format!("agents[{index}]"),
                    &[
                        "agent_id",
                        "message_id",
                        "parent_thread_id",
                        "parent_message_id",
                        "execution_id",
                        "checkpoint_id",
                    ],
                    references,
                );
                if let Some(value) = agent.get("checkpoint") {
                    checkpoint(value, &format!("agents[{index}].checkpoint"), references);
                }
            }
        }
        if let Some(value) = value.get("checkpoint") {
            checkpoint(value, "checkpoint", references);
        }
    }
    Ok(())
}
