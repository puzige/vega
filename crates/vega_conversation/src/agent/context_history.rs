use super::*;
use vega_runtime::{ConversationHistoryReader, ConversationReadRequest};

pub(super) fn reader(database: PathBuf, thread: String) -> Arc<ConversationHistoryReader> {
    Arc::new(move |request| read(&database, &thread, request).ok_or_else(unavailable))
}

pub(super) fn history_safety(
    database: PathBuf,
    thread: String,
) -> vega_runtime::HistorySafetyCheck {
    vega_runtime::HistorySafetyCheck::new(move |active, summaries, credentials, exceptions| {
        validate_saved_pages(
            &database,
            &thread,
            active,
            summaries,
            credentials,
            exceptions,
        )
        .unwrap_or_else(|| vega_runtime::HistorySafetyReport {
            invalidate_all: true,
            invalidate_summary: true,
            ..Default::default()
        })
    })
}

fn validate_saved_pages(
    database: &std::path::Path,
    thread: &str,
    active: &[String],
    summaries: &[String],
    credentials: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> Option<vega_runtime::HistorySafetyReport> {
    let store = Store::open_read_only(database).ok()?;
    let transaction = store.conn().unchecked_transaction().ok()?;
    let mut report = vega_runtime::HistorySafetyReport::default();
    let mut pages = std::collections::BTreeMap::<String, (String, String)>::new();
    let mut pending = active.to_vec();
    for summary in summaries {
        let plan = transaction.query_row(
            "SELECT projection_plan FROM context_checkpoints WHERE thread_id=?1 AND id=?2 LIMIT 1",
            (thread, summary), |row| row.get::<_, Option<String>>(0),
        ).ok().flatten().and_then(|json| serde_json::from_str::<vega_store::context_projection::ProjectionPlan>(&json).ok());
        if let Some(dependencies) = plan.and_then(|plan| plan.history_dependencies) {
            pending.extend(dependencies.iter().cloned());
            report
                .summary_dependencies
                .insert(summary.clone(), dependencies);
        } else {
            let mut statement = transaction.prepare("SELECT id FROM tool_calls WHERE thread_id=?1 AND tool='read_conversation' AND status='success' LIMIT 257").ok()?;
            let dependencies = statement
                .query_map([thread], |row| row.get::<_, String>(0))
                .ok()?
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if dependencies.len() > 256 {
                report.invalid_summaries.insert(summary.clone());
            } else {
                pending.extend(dependencies.iter().cloned());
                report
                    .summary_dependencies
                    .insert(summary.clone(), dependencies);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut nested_dependencies = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut output_bytes = 0usize;
    let mut index = 0;
    while index < pending.len() {
        let id = pending[index].clone();
        index += 1;
        if !seen.insert(id.clone()) {
            continue;
        }
        if seen.len() > 256 || output_bytes >= 8 * 1024 * 1024 {
            report.invalid_results.insert(id);
            continue;
        }
        let row = transaction.query_row(
            "SELECT input_json,CASE WHEN length(CAST(output_text AS BLOB))<=65536 THEN output_text END,status FROM tool_calls WHERE thread_id=?1 AND id=?2 AND tool='read_conversation'",
            (thread, &id), |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?)),
        ).ok();
        let Some((input, output, status)) = row else {
            report.invalid_results.insert(id);
            continue;
        };
        if status != "success" {
            continue;
        }
        let Some(output) = output else {
            report.invalid_results.insert(id);
            continue;
        };
        output_bytes = output_bytes.saturating_add(output.len());
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&output)
            && let Some(records) = value["vega_history_observation"]["page"]["records"].as_array()
        {
            for record in records {
                if record["output"]["text"].is_string()
                    && let Some(nested) = record["source_id"].as_str()
                    && transaction
                        .query_row(
                            "SELECT tool FROM tool_calls WHERE thread_id=?1 AND id=?2",
                            (thread, nested),
                            |row| row.get::<_, String>(0),
                        )
                        .ok()
                        .as_deref()
                        == Some("read_conversation")
                {
                    pending.push(nested.to_owned());
                    nested_dependencies
                        .entry(id.clone())
                        .or_default()
                        .push(nested.to_owned());
                }
            }
        }
        pages.insert(id, (input, output));
    }
    let owned = credentials.to_vec();
    let config = vega_runtime::RuntimeToolConfig::default()
        .with_credential_reader(Arc::new(move || Ok(owned.clone())))
        .with_credential_exceptions(exceptions.cloned());
    let mut source_cache = std::collections::BTreeMap::new();
    let mut source_bytes = 0usize;
    for (id, (input, output)) in pages {
        if source_bytes >= 16 * 1024 * 1024 {
            report.invalid_results.insert(id);
            continue;
        }
        let valid = (|| {
            let request = ConversationReadRequest::parse(&input)?;
            let original = serde_json::from_str::<serde_json::Value>(&output).ok()?;
            let records = original["vega_history_observation"]["page"]["records"].as_array()?;
            if records.iter().any(|record| {
                record["source_id"].as_str().is_none_or(|id| {
                    id.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER)
                })
            }) {
                return Some(false);
            }
            let key = (
                request.kind.clone(),
                request.source_id.clone(),
                request.after_sequence,
                request.limit,
            );
            if !source_cache.contains_key(&key) {
                let current = read_in_connection(&transaction, thread, &request)?;
                let bytes = serde_json::to_vec(&current).ok()?.len();
                source_cache.insert(key.clone(), (current, bytes));
            }
            let (current, bytes) = source_cache.get(&key)?;
            source_bytes = source_bytes.saturating_add(*bytes);
            if source_bytes > 16 * 1024 * 1024 {
                return Some(false);
            }
            let current = current.clone();
            let projected =
                vega_runtime::project_conversation_history_page(current, &request, &config).ok()?;
            let projected = serde_json::from_str::<serde_json::Value>(&projected).ok()?;
            let current_records =
                projected["vega_history_observation"]["page"]["records"].as_array()?;
            Some(records.iter().all(|record| {
                current_records
                    .iter()
                    .find(|current| current["source_id"] == record["source_id"])
                    .is_some_and(|current| {
                        ["content", "input", "output"].iter().all(|field| {
                            record[*field].is_null()
                                || record[*field]["unavailable"] == true
                                || record[*field] == current[*field]
                        })
                    })
            }))
        })()
        .unwrap_or(false);
        if !valid {
            report.invalid_results.insert(id);
            report.invalidate_summary = true;
        }
    }
    for _ in 0..nested_dependencies.len() {
        let mut changed = false;
        for (id, dependencies) in &nested_dependencies {
            if dependencies
                .iter()
                .any(|dependency| report.invalid_results.contains(dependency))
            {
                changed |= report.invalid_results.insert(id.clone());
            }
        }
        if !changed {
            break;
        }
    }
    for (summary, dependencies) in &report.summary_dependencies {
        if dependencies
            .iter()
            .any(|id| report.invalid_results.contains(id))
        {
            report.invalid_summaries.insert(summary.clone());
        }
    }
    Some(report)
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
    read_in_connection(store.conn(), thread, request)
}

fn read_in_connection(
    conn: &vega_store::Connection,
    thread: &str,
    request: &ConversationReadRequest,
) -> Option<serde_json::Value> {
    let query = match request.kind.as_str() {
        "messages" => {
            "SELECT m.id,m.seq,m.role,m.kind,m.status,CASE WHEN length(CAST(m.content AS BLOB))<=1048576 THEN m.content END,(SELECT COUNT(*) FROM image_attachments i WHERE i.message_id=m.id),(SELECT COALESCE(SUM(length(i.encoded)),0) FROM image_attachments i WHERE i.message_id=m.id) FROM messages m JOIN threads t ON t.id=m.thread_id WHERE length(CAST(m.id AS BLOB)) BETWEEN 1 AND 128 AND length(CAST(m.role AS BLOB))<=128 AND length(CAST(m.kind AS BLOB))<=128 AND length(CAST(m.status AS BLOB))<=128 AND m.thread_id=?1 AND m.seq>?2 AND (?3 IS NULL OR m.id=?3) AND ?5='messages' ORDER BY m.seq LIMIT ?4"
        }
        "tools" | "skill_resources" => {
            "SELECT c.id,c.seq,c.message_id,c.tool,c.status,CASE WHEN length(CAST(c.input_json AS BLOB))<=1048576 THEN c.input_json END,CASE WHEN length(CAST(c.output_text AS BLOB))<=1048576 THEN c.output_text END FROM tool_calls c JOIN messages m ON m.id=c.message_id AND m.thread_id=c.thread_id AND m.role='assistant' JOIN threads t ON t.id=c.thread_id WHERE length(CAST(c.id AS BLOB)) BETWEEN 1 AND 128 AND length(CAST(c.message_id AS BLOB)) BETWEEN 1 AND 128 AND length(CAST(c.tool AS BLOB))<=256 AND length(CAST(c.status AS BLOB))<=128 AND c.thread_id=?1 AND c.seq>?2 AND (?3 IS NULL OR c.id=?3) AND (?5!='skill_resources' OR (c.tool='read_skill_resource' AND c.status='success')) ORDER BY c.seq LIMIT ?4"
        }
        "delegated_results" => {
            "SELECT c.id,c.seq,c.message_id,c.tool,e.state,\
             CASE WHEN length(CAST(p.payload AS BLOB))<=1048576 THEN p.payload END,\
             CASE WHEN length(CAST(e.result AS BLOB))<=1048576 THEN e.result END,\
             e.child_thread_id,e.child_message_id,COALESCE(r.release_state,'unknown'),e.truncated,p.id \
             FROM tool_calls c \
             JOIN messages m ON m.id=c.message_id AND m.thread_id=c.thread_id AND m.role='assistant' \
             JOIN threads t ON t.id=c.thread_id \
             JOIN delegated_executions e ON e.parent_thread_id=c.thread_id AND e.parent_message_id=c.message_id AND e.request_call_id=c.id \
             JOIN messages child ON child.id=e.child_message_id AND child.thread_id=e.child_thread_id AND child.role='assistant' \
             JOIN threads child_thread ON child_thread.id=e.child_thread_id \
             LEFT JOIN delegated_execution_recovery r ON r.child_thread_id=e.child_thread_id AND r.child_message_id=e.child_message_id \
             LEFT JOIN delegated_stage_checkpoints p ON p.id=COALESCE(r.handoff_checkpoint_id,\
                 (SELECT id FROM delegated_stage_checkpoints WHERE child_thread_id=e.child_thread_id AND child_message_id=e.child_message_id ORDER BY rowid DESC LIMIT 1)) \
                 AND p.child_thread_id=e.child_thread_id AND p.child_message_id=e.child_message_id \
                 AND length(CAST(p.id AS BLOB)) BETWEEN 1 AND 128 \
             WHERE c.thread_id=?1 AND c.seq>?2 AND (?3 IS NULL OR c.id=?3) AND ?5='delegated_results' \
             AND c.status='success' AND c.tool IN ('spawn_agent','followup_agent','resume_agent') \
             AND e.state IN ('completed','failed','interrupted') \
             AND length(CAST(c.id AS BLOB)) BETWEEN 1 AND 128 \
             AND length(CAST(c.message_id AS BLOB)) BETWEEN 1 AND 128 \
             AND length(CAST(e.child_thread_id AS BLOB)) BETWEEN 1 AND 128 \
             AND length(CAST(e.child_message_id AS BLOB)) BETWEEN 1 AND 128 \
             AND COALESCE(r.release_state,'unknown') IN ('held','released','recovered_abandoned','quarantined','unknown') \
             ORDER BY c.seq LIMIT ?4"
        }
        _ => return None,
    };
    let mut statement = conn.prepare(query).ok()?;
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
        } else if request.kind == "delegated_results" {
            let output: Option<String> = row.get(6).ok()?;
            let agent: String = row.get(7).ok()?;
            let message: String = row.get(8).ok()?;
            let release: String = row.get(9).ok()?;
            let truncated: bool = row.get(10).ok()?;
            let checkpoint_id: Option<String> = row.get(11).ok()?;
            let checkpoint = content.as_deref().and_then(|payload| {
                checkpoint_projection(payload, checkpoint_id.as_deref()?, &agent, &message)
            });
            serde_json::json!({"source_id":id,"sequence":sequence,"message_id":a,"tool":b,
                "status":status,"agent_id":agent,"child_message_id":message,"release_state":release,
                "worker_liveness":"unknown","result_truncated":truncated,"output":output,"content":checkpoint})
        } else {
            let output: Option<String> = row.get(6).ok()?;
            serde_json::json!({"source_id":id,"sequence":sequence,"message_id":a,"tool":b,
                "status":status,"input":content,"output":output})
        };
        records.push(record);
    }
    Some(serde_json::json!({"records":records,"next_after_sequence":next,"has_more":more}))
}

fn checkpoint_projection(
    payload: &str,
    id: &str,
    agent: &str,
    message: &str,
) -> Option<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    if value["id"] != id || value["agent_id"] != agent || value["message_id"] != message {
        return None;
    }
    for field in [
        "id",
        "agent_id",
        "message_id",
        "origin",
        "source_fingerprint",
    ] {
        let text = value[field].as_str()?;
        if text.is_empty() || text.len() > 128 {
            return None;
        }
    }
    value["source_version"].as_u64()?;
    if !value["previous_checkpoint_id"].is_null()
        && !value["previous_checkpoint_id"]
            .as_str()
            .is_some_and(|text| !text.is_empty() && text.len() <= 128)
    {
        return None;
    }
    let manifest = value["manifest"].as_object()?;
    let artifacts = manifest.get("artifacts")?.as_array()?;
    let artifacts = artifacts
        .iter()
        .map(|artifact| Some(serde_json::json!({"path":artifact.get("path")?.as_str()?})))
        .collect::<Option<Vec<_>>>()?;
    let effects = value["unknown_effects"]
        .as_array()?
        .iter()
        .map(|effect| {
            for (field, limit) in [("call_id", 128), ("tool", 256), ("status", 128)] {
                let text = effect[field].as_str()?;
                if text.is_empty() || text.len() > limit {
                    return None;
                }
            }
            Some(serde_json::json!({"call_id":effect["call_id"],"tool":effect["tool"],"status":effect["status"]}))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut safe_manifest = serde_json::Map::new();
    for field in ["stage", "summary", "next_step"] {
        let text = manifest.get(field)?;
        if !text.is_null() && !text.is_string() {
            return None;
        }
        safe_manifest.insert(field.into(), text.clone());
    }
    for field in ["evidence_call_ids", "unresolved"] {
        let items = manifest.get(field)?.as_array()?;
        for item in items {
            let text = item.as_str()?;
            if field == "evidence_call_ids" && (text.is_empty() || text.len() > 128) {
                return None;
            }
        }
        safe_manifest.insert(field.into(), serde_json::json!(items));
    }
    safe_manifest.insert(
        "ready_for_review".into(),
        serde_json::json!(manifest.get("ready_for_review")?.as_bool()?),
    );
    safe_manifest.insert("artifacts".into(), serde_json::json!(artifacts));
    let mut safe = serde_json::Map::new();
    for field in [
        "id",
        "agent_id",
        "message_id",
        "origin",
        "source_version",
        "source_fingerprint",
        "previous_checkpoint_id",
    ] {
        safe.insert(field.into(), value[field].clone());
    }
    safe.insert("manifest".into(), serde_json::Value::Object(safe_manifest));
    safe.insert("unknown_effects".into(), serde_json::json!(effects));
    Some(serde_json::Value::Object(safe))
}
