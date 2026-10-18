use super::*;
use crate::agent::mcp_registry::{
    McpCandidate, McpDispatchFailure, McpDispatchOutput, McpToolDispatcher, RunCapabilitySnapshot,
};

struct RecordingDispatcher {
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

impl McpToolDispatcher for RecordingDispatcher {
    fn call(
        &self,
        exact_tool_name: String,
        arguments: Value,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<McpDispatchOutput, McpDispatchFailure>> {
        let calls = self.calls.clone();
        async move {
            calls.lock().unwrap().push((exact_tool_name, arguments));
            Ok(McpDispatchOutput {
                text: "owned-tool-ok".to_string(),
                structured_content: None,
                is_error: false,
            })
        }
        .boxed()
    }
}

struct ExternalDecisionHook {
    decision: RuntimeUserDecision,
    prompts: Arc<Mutex<Vec<RuntimeMcpPermissionPrompt>>>,
}

impl RuntimePermissionHook for ExternalDecisionHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        async { Ok(RuntimeUserDecision::Timeout) }.boxed()
    }

    fn request_mcp(
        &self,
        prompt: RuntimeMcpPermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.prompts.lock().unwrap().push(prompt);
        let decision = self.decision.clone();
        async move { Ok(decision) }.boxed()
    }
}

struct NoopDispatcher;

impl McpToolDispatcher for NoopDispatcher {
    fn call(
        &self,
        _exact_tool_name: String,
        _arguments: Value,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<McpDispatchOutput, McpDispatchFailure>> {
        async {
            Ok(McpDispatchOutput {
                text: String::new(),
                structured_content: None,
                is_error: false,
            })
        }
        .boxed()
    }
}

struct RevokeOnApprovalHook {
    revoke: CancellationToken,
}

struct RotateCredentialOnApprovalHook {
    current: Arc<Mutex<String>>,
    replacement: String,
}

impl RuntimePermissionHook for RotateCredentialOnApprovalHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        async { Ok(RuntimeUserDecision::Timeout) }.boxed()
    }

    fn request_mcp(
        &self,
        _prompt: RuntimeMcpPermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        *self.current.lock().unwrap() = self.replacement.clone();
        async { Ok(RuntimeUserDecision::Once) }.boxed()
    }
}

impl RuntimePermissionHook for RevokeOnApprovalHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        async { Ok(RuntimeUserDecision::Timeout) }.boxed()
    }

    fn request_mcp(
        &self,
        _prompt: RuntimeMcpPermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.revoke.cancel();
        async { Ok(RuntimeUserDecision::Once) }.boxed()
    }
}

struct ReturnAfterCancelDispatcher {
    started: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl McpToolDispatcher for ReturnAfterCancelDispatcher {
    fn call(
        &self,
        _exact_tool_name: String,
        _arguments: Value,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<McpDispatchOutput, McpDispatchFailure>> {
        let started = self.started.clone();
        async move {
            if let Some(sender) = started.lock().unwrap().take() {
                let _ = sender.send(());
            }
            cancel.cancelled().await;
            Ok(McpDispatchOutput {
                text: "late-success-must-not-appear".into(),
                structured_content: None,
                is_error: false,
            })
        }
        .boxed()
    }
}

fn candidate(server_id: &str, tool_name: &str) -> McpCandidate {
    McpCandidate::new(
        server_id.to_string(),
        7,
        tool_name.to_string(),
        ToolDefinition {
            name: tool_name.to_string(),
            description: "Untrusted MCP search tool".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"],
                "additionalProperties": false
            }),
            strict: false,
        },
        Arc::new(NoopDispatcher),
        CancellationToken::new(),
    )
}
