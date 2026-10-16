use super::*;
use std::sync::atomic::AtomicBool;

const SCRIPT_COMMAND: &str = "sh scripts/harmless.sh";
const SCRIPT_BYTES: &str = "printf 'S16_DURABLE_PRIVATE_SCRIPT' > s16-result.txt\n";
const SKILL_BODY_MARKER: &str = "S16_DURABLE_PRIVATE_GUIDANCE";

struct RecordingScriptPermission {
    requests: Arc<Mutex<Vec<PermissionRequest>>>,
    decision: PermissionDecision,
}

impl PermissionHook for RecordingScriptPermission {
    fn request(
        &self,
        request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        self.requests.lock().unwrap().push(request);
        let decision = self.decision.clone();
        Box::pin(async move { Ok(decision) })
    }
}
