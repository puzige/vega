use super::*;
use serde_json::Value;

#[cfg(test)]
#[path = "shell_operations_tests.rs"]
mod tests;

pub(super) struct ShellOperationPersistence {
    pub database_path: PathBuf,
    pub thread_id: String,
    pub run_id: String,
}

impl vega_runtime::RuntimeShellOperationHook for ShellOperationPersistence {
    fn resource(
        &self,
        operation: String,
        resource: vega_tools::BashSessionResource,
    ) -> BoxFuture<'static, Result<(), VegaError>> {
        let path = self.database_path.clone();
        let thread = self.thread_id.clone();
        let run = self.run_id.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                let store = Store::open_existing(path).map_err(VegaError::from)?;
                let authorized: bool = store.conn().query_row("SELECT EXISTS(SELECT 1 FROM shell_operations WHERE operation_id=?1 AND thread_id=?2 AND run_id=?3 AND generation=?4)", [operation.as_str(), thread.as_str(), run.as_str(), vega_runtime::shell_generation()], |row| row.get(0)).map_err(VegaError::from)?;
                if !authorized { return Err(shell_state_error()); }
                let identity = serde_json::to_string(&resource).map_err(|_| shell_state_error())?;
                match vega_store::shell_operations::register_resource(store.conn(), &operation, &identity) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        if vega_store::shell_operations::resource(store.conn(), &operation).map_err(VegaError::from)?.as_deref() == Some(&identity) { Ok(()) } else { Err(VegaError::from(error)) }
                    }
                }
            }).await.map_err(|_| shell_state_error())?
        }.boxed()
    }

    fn record(
        &self,
        record: vega_runtime::RuntimeShellOperationRecord,
    ) -> BoxFuture<'static, Result<(), VegaError>> {
        let path = self.database_path.clone();
        let thread = self.thread_id.clone();
        let run = self.run_id.clone();
        async move {
            if record.thread_id != thread || record.run_id != run {
                return Err(shell_state_error());
            }
            tokio::task::spawn_blocking(move || {
                let store = Store::open(path).map_err(VegaError::from)?;
                let facts = vega_store::shell_operations::ShellFacts {
                    command_result: record.command_result.clone(),
                    exit_signal: record.exit_signal,
                    cleanup_state: record.cleanup_state.clone(),
                    output_incomplete: record.output_incomplete,
                    resources_closed: record.resources_closed,
                    process_resolved: record.process_resolved,
                    cleanup_reasons: record.cleanup_reasons.clone(),
                };
                let persisted = vega_store::shell_operations::ShellOperationRecord {
                    operation_id: &record.operation_id,
                    thread_id: &record.thread_id,
                    run_id: &record.run_id,
                    call_id: &record.call_id,
                    generation: &record.generation,
                    workspace: &record.workspace,
                    state: &record.state,
                    exit_code: record.exit_code,
                    error: record.error.as_deref(),
                    duration_ms: record.duration_ms,
                };
                match vega_store::shell_operations::record_with_facts(
                    store.conn(),
                    &persisted,
                    &facts,
                    now_ms(),
                ) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        if vega_store::shell_operations::verify_record(
                            store.conn(),
                            &persisted,
                            &facts,
                        )
                        .map_err(VegaError::from)?
                        {
                            Ok(())
                        } else {
                            Err(VegaError::from(error))
                        }
                    }
                }
            })
            .await
            .map_err(|_| shell_state_error())?
        }
        .boxed()
    }
}

pub(super) fn validate_projection(
    tool: &str,
    input_json: &str,
    output: &str,
    status: RuntimeToolStatus,
    approval: &ApprovalAudit,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
) -> Option<bool> {
    let control = matches!(tool, "bash_wait" | "bash_cancel");
    let session = tool == "bash"
        && serde_json::from_str::<Value>(input_json)
            .is_ok_and(|input| input.get("wait_ms").is_some_and(|wait| !wait.is_null()));
    if !control && !session {
        return None;
    }
    let input_valid = if session {
        vega_tools::bash_permission_signature(input_json).is_ok()
    } else {
        valid_control_input(tool, input_json)
    };
    let valid = (|| {
        if control && input_json == "{}" && status == RuntimeToolStatus::Rejected {
            return output == "Tool error: invalid shell session input"
                && approval.decision == Approval::Deny
                && approval.source == ApprovalSource::Validation
                && exit_code.is_none()
                && duration_ms.is_none();
        }
        if !input_valid {
            return false;
        }
        if status == RuntimeToolStatus::Rejected {
            return exit_code.is_none()
                && duration_ms.is_none()
                && approval.decision == Approval::Deny
                && matches!(
                    approval.source,
                    ApprovalSource::Validation
                        | ApprovalSource::RunMode
                        | ApprovalSource::User
                        | ApprovalSource::Timeout
                        | ApprovalSource::Recovery
                        | ApprovalSource::Cancelled
                        | ApprovalSource::Danger
                        | ApprovalSource::Unavailable
                        | ApprovalSource::ReadOnly
                )
                && output.starts_with("Tool error:");
        }
        if approval.decision == Approval::Deny
            || (control && approval.source != ApprovalSource::ReadonlyTool)
            || (session
                && !matches!(
                    approval.source,
                    ApprovalSource::User
                        | ApprovalSource::Rule
                        | ApprovalSource::Auto
                        | ApprovalSource::FullAccess
                        | ApprovalSource::ReadonlyTool
                ))
        {
            return false;
        }
        if output.starts_with("Tool error:") {
            return status == RuntimeToolStatus::Failed
                && exit_code.is_none()
                && duration_ms.is_none();
        }
        valid_receipt(output, status, exit_code, duration_ms)
    })();
    Some(valid)
}

pub(crate) fn valid_receipt(
    output: &str,
    status: RuntimeToolStatus,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(output) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    let keys = [
        "handle",
        "operation_id",
        "state",
        "exit_code",
        "output",
        "next_cursor",
        "retained_from",
        "output_gap",
        "truncated",
        "duration_ms",
        "error",
        "cleanup_safe",
        "command_result",
        "exit_signal",
        "output_incomplete",
        "resources_closed",
        "process_resolved",
        "cleanup_reasons",
    ];
    if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
        return false;
    }
    if !["handle", "operation_id"].iter().all(|key| {
        value[*key]
            .as_str()
            .is_some_and(|id| id.len() == 26 && ulid::Ulid::from_string(id).is_ok())
    }) {
        return false;
    }
    let Some(text) = value["output"].as_str() else {
        return false;
    };
    let Some(next) = value["next_cursor"].as_u64() else {
        return false;
    };
    let Some(retained) = value["retained_from"].as_u64() else {
        return false;
    };
    if text.len() > 16 * 1024
        || retained > next
        || u64::try_from(text.len()).map_or(true, |bytes| bytes > next - retained)
        || value["output_gap"].as_bool().is_none()
        || value["truncated"].as_bool().is_none()
        || value["duration_ms"].as_u64() != duration_ms
        || duration_ms.is_none()
        || !(value["error"].is_null()
            || value["error"]
                .as_str()
                .is_some_and(|error| error.len() <= 256 && !error.is_empty()))
    {
        return false;
    }
    let json_exit = if value["exit_code"].is_null() {
        None
    } else {
        let Some(code) = value["exit_code"]
            .as_i64()
            .and_then(|code| i32::try_from(code).ok())
        else {
            return false;
        };
        Some(code)
    };
    if json_exit != exit_code {
        return false;
    }
    let Some(safe) = value["cleanup_safe"].as_bool() else {
        return false;
    };
    if value["output_incomplete"].as_bool().is_none()
        || value["resources_closed"].as_bool().is_none()
        || value["process_resolved"].as_bool().is_none()
        || !value["cleanup_reasons"].as_array().is_some_and(|reasons| {
            reasons.len() <= 256
                && reasons
                    .iter()
                    .all(|reason| reason.as_str().is_some_and(|reason| reason.len() <= 128))
        })
        || !(value["command_result"].is_null()
            || value["command_result"].as_str().is_some_and(|result| {
                matches!(
                    result,
                    "exited" | "cancelled" | "timed_out" | "outcome_unknown" | "output_failed"
                )
            }))
        || !(value["exit_signal"].is_null()
            || value["exit_signal"]
                .as_i64()
                .is_some_and(|signal| (1..128).contains(&signal)))
    {
        return false;
    }
    if safe
        && (value["output_incomplete"] != false
            || value["resources_closed"] != true
            || value["process_resolved"] != true)
    {
        return false;
    }
    match value["state"].as_str() {
        Some("running") => status == RuntimeToolStatus::Success && exit_code.is_none() && !safe,
        Some("finishing" | "stopping") => status == RuntimeToolStatus::Success && !safe,
        Some("succeeded") => {
            status == RuntimeToolStatus::Success
                && exit_code == Some(0)
                && safe
                && value["error"].is_null()
        }
        Some("failed") => {
            status == RuntimeToolStatus::Failed
                && safe
                && (exit_code.is_some_and(|code| code != 0) || !value["error"].is_null())
        }
        Some("timed_out") => {
            status == RuntimeToolStatus::Failed && safe && !value["error"].is_null()
        }
        Some("cancelled") => status == RuntimeToolStatus::Cancelled && safe,
        Some("outcome_unknown") => {
            status == RuntimeToolStatus::Failed && !safe && !value["error"].is_null()
        }
        _ => false,
    }
}

pub(crate) fn valid_control_input(tool: &str, input: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(input) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    object.keys().all(|key| {
        matches!(key.as_str(), "handle" | "cursor" | "wait_ms")
            && (tool != "bash_cancel" || key == "handle" || value[key].is_null())
    }) && value["handle"]
        .as_str()
        .is_some_and(|id| id.len() == 26 && ulid::Ulid::from_string(id).is_ok())
        && (value["cursor"].is_null() || value["cursor"].as_u64().is_some())
        && (value["wait_ms"].is_null()
            || value["wait_ms"]
                .as_u64()
                .is_some_and(|wait| wait <= 300_000))
}

fn shell_state_error() -> VegaError {
    VegaError::Tool {
        tool: "bash".into(),
        message: "shell operation authority unavailable".into(),
    }
}
