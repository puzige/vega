use super::*;

#[cfg(test)]
#[path = "shell_operations_tests.rs"]
mod tests;

pub(super) struct ShellOperationPersistence {
    pub database_path: PathBuf,
    pub thread_id: String,
    pub run_id: String,
}

impl vega_runtime::RuntimeShellOperationHook for ShellOperationPersistence {
    fn record(&self, record: vega_runtime::RuntimeShellOperationRecord) -> BoxFuture<'static, Result<(), VegaError>> {
        let path = self.database_path.clone();
        let thread = self.thread_id.clone();
        let run = self.run_id.clone();
        async move {
            if record.thread_id != thread || record.run_id != run {
                return Err(shell_state_error());
            }
            tokio::task::spawn_blocking(move || {
                let store = Store::open(path).map_err(VegaError::from)?;
                vega_store::shell_operations::record(store.conn(), &vega_store::shell_operations::ShellOperationRecord {
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
                }, now_ms()).map_err(VegaError::from)
            }).await.map_err(|_| shell_state_error())?
        }.boxed()
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
    let session = tool == "bash" && serde_json::from_str::<Value>(input_json).is_ok_and(|input| input.get("wait_ms").is_some_and(|wait| !wait.is_null()));
    if !control && !session { return None; }
    let input_valid = if session {
        vega_tools::bash_permission_signature(input_json).is_ok()
    } else {
        valid_control_input(tool, input_json)
    };
    let valid = (|| {
        if control && input_json == "{}" && status == RuntimeToolStatus::Rejected {
            return output == "Tool error: invalid shell session input" && approval.decision == Approval::Deny && approval.source == ApprovalSource::Validation && exit_code.is_none() && duration_ms.is_none();
        }
        if !input_valid { return false; }
        if status == RuntimeToolStatus::Rejected {
            return exit_code.is_none() && duration_ms.is_none() && approval.decision == Approval::Deny
                && matches!(approval.source, ApprovalSource::Validation | ApprovalSource::RunMode | ApprovalSource::User | ApprovalSource::Timeout | ApprovalSource::Recovery | ApprovalSource::Cancelled | ApprovalSource::Danger | ApprovalSource::Unavailable | ApprovalSource::ReadOnly)
                && output.starts_with("Tool error:");
        }
        if approval.decision == Approval::Deny || (control && approval.source != ApprovalSource::ReadonlyTool)
            || (session && !matches!(approval.source, ApprovalSource::User | ApprovalSource::Rule | ApprovalSource::Auto | ApprovalSource::FullAccess | ApprovalSource::ReadonlyTool)) { return false; }
        if output.starts_with("Tool error:") {
            return status == RuntimeToolStatus::Failed && exit_code.is_none() && duration_ms.is_none();
        }
        let Ok(value) = serde_json::from_str::<Value>(output) else { return false; };
        let Some(object) = value.as_object() else { return false; };
        let keys = ["handle","operation_id","state","exit_code","output","next_cursor","retained_from","output_gap","truncated","duration_ms","error","cleanup_safe"];
        if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) { return false; }
        if !["handle","operation_id"].iter().all(|key| value[*key].as_str().is_some_and(|id| id.len() == 26 && ulid::Ulid::from_string(id).is_ok())) { return false; }
        let Some(text) = value["output"].as_str() else { return false; };
        let Some(next) = value["next_cursor"].as_u64() else { return false; };
        let Some(retained) = value["retained_from"].as_u64() else { return false; };
        if text.len() > 16 * 1024 || retained > next || value["output_gap"].as_bool().is_none() || value["truncated"].as_bool().is_none()
            || value["duration_ms"].as_u64() != duration_ms || duration_ms.is_none()
            || !(value["error"].is_null() || value["error"].as_str().is_some_and(|error| error.len() <= 256 && !error.is_empty())) { return false; }
        let json_exit = if value["exit_code"].is_null() { None } else { let Some(code) = value["exit_code"].as_i64().and_then(|code| i32::try_from(code).ok()) else { return false; }; Some(code) };
        if json_exit != exit_code { return false; }
        let Some(safe) = value["cleanup_safe"].as_bool() else { return false; };
        match value["state"].as_str() {
            Some("running") => status == RuntimeToolStatus::Success && exit_code.is_none() && !safe,
            Some("finishing" | "stopping") => status == RuntimeToolStatus::Success && !safe,
            Some("succeeded") => status == RuntimeToolStatus::Success && exit_code == Some(0) && safe && value["error"].is_null(),
            Some("failed") => status == RuntimeToolStatus::Failed && safe && (exit_code.is_some_and(|code| code != 0) || !value["error"].is_null()),
            Some("timed_out") => status == RuntimeToolStatus::Failed && safe && !value["error"].is_null(),
            Some("cancelled") => status == RuntimeToolStatus::Cancelled && safe,
            Some("outcome_unknown") => status == RuntimeToolStatus::Failed && !safe && !value["error"].is_null(),
            _ => false,
        }
    })();
    Some(valid)
}

fn valid_control_input(tool: &str, input: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(input) else { return false; };
    let Some(object) = value.as_object() else { return false; };
    object.keys().all(|key| matches!(key.as_str(), "handle" | "cursor" | "wait_ms") && (tool != "bash_cancel" || key == "handle" || value[key].is_null()))
        && value["handle"].as_str().is_some_and(|id| id.len() == 26 && ulid::Ulid::from_string(id).is_ok())
        && (value["cursor"].is_null() || value["cursor"].as_u64().is_some())
        && (value["wait_ms"].is_null() || value["wait_ms"].as_u64().is_some_and(|wait| wait <= 300_000))
}

fn shell_state_error() -> VegaError {
    rusqlite::Error::InvalidParameterName("shell operation authority unavailable".into()).into()
}
