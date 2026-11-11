use std::collections::HashMap;

use super::*;

pub(crate) fn recover_terminal_tools(
    conn: &vega_store::Connection,
    project_id: &str,
    thread_id: &str,
) -> Result<HashMap<String, vega_runtime::CompletedToolCall>, ConversationError> {
    recover_terminal_tools_with_origins(conn, project_id, thread_id, None)
}

pub(crate) fn recover_terminal_tools_with_origins(
    conn: &vega_store::Connection,
    project_id: &str,
    thread_id: &str,
    origins: Option<&HashMap<String, vega_store::fork_mutations::MutationOrigin>>,
) -> Result<HashMap<String, vega_runtime::CompletedToolCall>, ConversationError> {
    tool_calls::terminal_results(conn, thread_id)
        .map_err(|error| runtime_store_error(std::io::Error::other(error.to_string())))?
        .into_iter()
        .map(|(call_id, call)| {
            let approval = ApprovalAudit::from_json(&call.approval).map_err(|_| {
                ConversationError::CorruptRow(format!(
                    "terminal tool call {call_id} has invalid approval"
                ))
            })?;
            let status = match call.status.as_str() {
                "success" => RuntimeToolStatus::Success,
                "failed" => RuntimeToolStatus::Failed,
                "rejected" => RuntimeToolStatus::Rejected,
                "cancelled" => RuntimeToolStatus::Cancelled,
                other => {
                    return Err(ConversationError::CorruptRow(format!(
                        "terminal tool call {call_id} has status {other}"
                    )));
                }
            };
            let origin = if matches!(call.tool.as_str(), "Write" | "write" | "Edit" | "edit") {
                Some(match origins {
                    Some(origins) => origins.get(&call_id).cloned().ok_or_else(|| {
                        ConversationError::CorruptRow(format!(
                            "terminal tool call {call_id} has invalid mutation origin"
                        ))
                    })?,
                    None => vega_store::fork_mutations::origin(conn, thread_id, &call_id).map_err(
                        |_| {
                            ConversationError::CorruptRow(format!(
                                "terminal tool call {call_id} has invalid mutation origin"
                            ))
                        },
                    )?,
                })
            } else {
                None
            };
            let (project, thread, original_call) =
                origin
                    .as_ref()
                    .map_or((project_id, thread_id, call_id.as_str()), |origin| {
                        (
                            origin.project_id.as_str(),
                            origin.thread_id.as_str(),
                            origin.call_id.as_str(),
                        )
                    });
            let canonical_input = validate_recovered_projection(
                project,
                thread,
                original_call,
                &call.tool,
                &call.input_json,
                &call.output,
                status,
                &approval,
                call.exit_code,
                call.duration_ms,
            )?;
            let completed = vega_runtime::CompletedToolCall {
                tool: call.tool,
                input_json: canonical_input,
                result: vega_runtime::RuntimeToolResult {
                    call_id: call_id.clone(),
                    output: call.output,
                    status,
                    reused: true,
                    exit_code: call.exit_code,
                    duration_ms: call.duration_ms,
                    truncated: None,
                    approval: Some(approval_audit_to_runtime(&approval)),
                    remember_rule: None,
                    credential_failure: None,
                },
            };
            Ok((call_id, completed))
        })
        .collect()
}

#[cfg(test)]
mod concurrent_reads_recovery {
    use super::*;

    #[test]
    fn restored_browser_receipts_preserve_success_and_unknown_outcomes() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(ulid::Ulid::generate().to_string());
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("synthetic.db")).unwrap();
        store.migrate().unwrap();
        let conn = store.conn();
        conn.execute_batch("INSERT INTO threads(id,model,created_at,updated_at) VALUES('t','model',1,1); INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('m','t',1,'assistant','','interrupted',1);").unwrap();
        let input = serde_json::json!({"kind":"computer_use","version":1,"action":"click","argument_bytes":2,"argument_digest":"a".repeat(64)}).to_string();
        let approval = r#"{"decision":"once","note":null,"source":"computer_use","danger":null}"#;
        for (id, status, output) in [
            (
                "completed",
                "success",
                "completed; observation_id=fixture; observation images and page data are ephemeral and are not retained in history.",
            ),
            (
                "uncertain",
                "failed",
                "outcome_unknown: synthetic control loss",
            ),
        ] {
            conn.execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,approval,created_at) VALUES(?1,'t','m',1,'BrowserAct',?2,?3,?4,?5,1)",[id,input.as_str(),output,status,approval]).unwrap();
        }
        let receipts = recover_terminal_tools(conn, "", "t").unwrap();
        assert_eq!(receipts.len(), 2);
        assert!(receipts.values().all(|receipt| receipt.result.reused));
        assert_eq!(
            receipts["completed"].result.status,
            RuntimeToolStatus::Success
        );
        assert_eq!(
            receipts["uncertain"].result.status,
            RuntimeToolStatus::Failed
        );
        assert_eq!(
            receipts["uncertain"].result.output,
            "outcome_unknown: synthetic control loss"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
