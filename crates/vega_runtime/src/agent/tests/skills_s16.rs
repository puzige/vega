use super::*;
use std::sync::atomic::AtomicBool;

const SCRIPT_COMMAND: &str = "sh ./s16-sentinel.sh";
const SCRIPT_BYTES: &str = "printf 'PRIVATE S16 SCRIPT BODY' > s16-sentinel-result.txt\n";
const SKILL_BODY_MARKER: &str = "PRIVATE S16 GUIDANCE";

struct ScriptCase {
    run_mode: RuntimeRunMode,
    permission_mode: RuntimePermissionMode,
    command: Option<&'static str>,
    decision: RuntimeUserDecision,
    stop_before_dispatch: bool,
    stop_during_execution: bool,
    hardlinked_script: bool,
}

impl ScriptCase {
    fn new(run_mode: RuntimeRunMode, permission_mode: RuntimePermissionMode) -> Self {
        Self {
            run_mode,
            permission_mode,
            command: Some(SCRIPT_COMMAND),
            decision: RuntimeUserDecision::Once,
            stop_before_dispatch: false,
            stop_during_execution: false,
            hardlinked_script: false,
        }
    }
}

struct RecordingScriptPermission {
    prompts: Arc<Mutex<Vec<RuntimePermissionPrompt>>>,
    decision: RuntimeUserDecision,
}

impl RuntimePermissionHook for RecordingScriptPermission {
    fn request(
        &self,
        prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.prompts.lock().unwrap().push(prompt);
        let decision = self.decision.clone();
        Box::pin(async move { Ok(decision) })
    }
}
