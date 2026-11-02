use std::collections::HashMap;

use super::*;

pub(crate) fn recover_terminal_tools(
    conn: &vega_store::Connection,
    project_id: &str,
    thread_id: &str,
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
                Some(
                    vega_store::fork_mutations::origin(conn, thread_id, &call_id).map_err(
                        |_| {
                            ConversationError::CorruptRow(format!(
                                "terminal tool call {call_id} has invalid mutation origin"
                            ))
                        },
                    )?,
                )
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
                },
            };
            Ok((call_id, completed))
        })
        .collect()
}
