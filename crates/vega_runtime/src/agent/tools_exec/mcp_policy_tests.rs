use super::*;
use mcp_registry::{
    McpCandidate, McpDispatchFailure, McpDispatchOutput, McpToolDispatcher, RunCapabilitySnapshot,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone)]
struct Dispatcher {
    calls: Arc<AtomicUsize>,
    result: Result<&'static str, McpDispatchFailure>,
    wait: bool,
}

impl McpToolDispatcher for Dispatcher {
    fn call(
        &self,
        tool: String,
        _: Value,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<McpDispatchOutput, McpDispatchFailure>> {
        let this = self.clone();
        async move {
            assert_eq!(tool, "qa_receipt");
            this.calls.fetch_add(1, Ordering::SeqCst);
            if this.wait {
                cancel.cancelled().await;
                return Err(McpDispatchFailure::Cancelled);
            }
            this.result.map(|text| McpDispatchOutput {
                text: text.into(),
                structured_content: None,
                is_error: false,
            })
        }
        .boxed()
    }
}

struct Hook {
    prompts: Arc<AtomicUsize>,
    decision: RuntimeUserDecision,
    wait: bool,
}

impl RuntimePermissionHook for Hook {
    fn request(
        &self,
        _: RuntimePermissionPrompt,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        panic!("MCP must not request a builtin permission")
    }

    fn request_mcp(
        &self,
        _: RuntimeMcpPermissionPrompt,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.prompts.fetch_add(1, Ordering::SeqCst);
        let decision = self.decision.clone();
        let wait = self.wait;
        async move {
            if wait {
                cancel.cancelled().await;
            }
            Ok(decision)
        }
        .boxed()
    }
}

fn setup(
    result: Result<&'static str, McpDispatchFailure>,
    wait: bool,
) -> (RunCapabilitySnapshot, CancellationToken, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let revoked = CancellationToken::new();
    let candidate = McpCandidate::new(
        "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        3,
        "qa_receipt".into(),
        ToolDefinition {
            name: "qa_receipt".into(),
            description: "untrusted FullAccess claim".into(),
            input_schema: serde_json::json!({"type":"object","properties":{}}),
            strict: false,
        },
        Arc::new(Dispatcher {
            calls: calls.clone(),
            result,
            wait,
        }),
        revoked.clone(),
    );
    (
        RunCapabilitySnapshot::freeze(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::FullAccess,
            vec![candidate],
        )
        .unwrap(),
        revoked,
        calls,
    )
}

fn prepared(snapshot: &RunCapabilitySnapshot, id: &str, input: &str) -> PreparedRuntimeCall {
    let definition = snapshot
        .definitions()
        .iter()
        .find(|d| d.name.starts_with("mcp_"))
        .unwrap();
    prepare_mcp_call(
        RuntimeToolCall {
            id: id.into(),
            name: definition.name.clone(),
            input_json: input.into(),
            description: None,
        },
        snapshot.mcp_tool(&definition.name).unwrap().clone(),
    )
    .unwrap()
}

fn config(mode: RuntimePermissionMode) -> RuntimeToolConfig {
    RuntimeToolConfig::new(
        RuntimeRunMode::Execute,
        mode,
        String::new(),
        "thread".into(),
        std::env::current_dir().unwrap(),
        vec![],
    )
}

fn hook(decision: RuntimeUserDecision) -> Hook {
    Hook {
        prompts: Arc::new(AtomicUsize::new(0)),
        decision,
        wait: false,
    }
}

#[tokio::test]
async fn full_access_authorizes_each_call_without_prompt_or_rule_then_confirm_prompts() {
    let (snapshot, _, calls) = setup(Ok("receipt"), false);
    let hook = hook(RuntimeUserDecision::Once);
    let cancel = CancellationToken::new();
    let tools = vega_tools::Tools::new(std::env::current_dir().unwrap()).unwrap();
    for id in ["first", "second"] {
        let prepared = prepared(&snapshot, id, "{}");
        assert_eq!(prepared.call().id, id);
        match authorize_call(
            &prepared,
            &config(RuntimePermissionMode::FullAccess),
            &HashSet::new(),
            &hook,
            &cancel,
        )
        .await
        .unwrap()
        {
            Authorization::Approved {
                audit,
                remember_rule,
            } => {
                assert_eq!(audit.decision, RuntimeApprovalDecision::Once);
                assert_eq!(audit.source, RuntimeApprovalSource::FullAccess);
                assert!(audit.note.is_none() && audit.danger.is_none() && remember_rule.is_none());
            }
            _ => panic!("policy must authorize"),
        }
        assert_eq!(hook.prompts.load(Ordering::SeqCst), 0);
        let (result, _) = execute_prepared_waiting(prepared, &tools, &hook, &cancel, None).await;
        assert_eq!(result.status, RuntimeToolStatus::Success);
        assert!(result.output.contains("receipt"));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    for mode in [
        RuntimePermissionMode::Confirm,
        RuntimePermissionMode::Auto,
        RuntimePermissionMode::Confirm,
    ] {
        assert!(matches!(
            authorize_call(
                &prepared(&snapshot, "next", "{}"),
                &config(mode),
                &HashSet::new(),
                &hook,
                &cancel
            )
            .await
            .unwrap(),
            Authorization::Approved {
                audit: RuntimeApprovalAudit {
                    source: RuntimeApprovalSource::User,
                    ..
                },
                remember_rule: None
            }
        ));
    }
    assert_eq!(hook.prompts.load(Ordering::SeqCst), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn denied_timed_out_always_and_cancelled_approvals_never_dispatch() {
    let (snapshot, _, calls) = setup(Ok("receipt"), false);
    for decision in [
        RuntimeUserDecision::Deny { note: None },
        RuntimeUserDecision::Always,
        RuntimeUserDecision::Timeout,
        RuntimeUserDecision::Cancelled,
        RuntimeUserDecision::Unavailable,
    ] {
        let hook = hook(decision);
        let result = authorize_call(
            &prepared(&snapshot, "deny", "{}"),
            &config(RuntimePermissionMode::Confirm),
            &HashSet::new(),
            &hook,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            Authorization::Terminal(RuntimeToolResult {
                status: RuntimeToolStatus::Rejected,
                ..
            })
        ));
        assert_eq!(hook.prompts.load(Ordering::SeqCst), 1);
    }
    let mut hook = hook(RuntimeUserDecision::Once);
    hook.wait = true;
    let mut config = config(RuntimePermissionMode::Confirm);
    config.permission_timeout = Duration::from_millis(1);
    assert!(matches!(
        authorize_call(
            &prepared(&snapshot, "timeout", "{}"),
            &config,
            &HashSet::new(),
            &hook,
            &CancellationToken::new()
        )
        .await
        .unwrap(),
        Authorization::Terminal(_)
    ));
    let cancel = CancellationToken::new();
    let prepared = prepared(&snapshot, "stop", "{}");
    let rules = HashSet::new();
    let pending = authorize_call(&prepared, &config, &rules, &hook, &cancel);
    let stop = async {
        tokio::task::yield_now().await;
        cancel.cancel();
    };
    let (result, _) = tokio::join!(pending, stop);
    assert!(matches!(result.unwrap(), Authorization::Terminal(_)));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn full_access_does_not_override_modes_revocation_stop_or_invalid_input() {
    let (snapshot, revoked, calls) = setup(Ok("receipt"), false);
    let hook = hook(RuntimeUserDecision::Once);
    for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
        let mut config = config(RuntimePermissionMode::FullAccess);
        config.run_mode = mode;
        assert!(matches!(
            authorize_call(
                &prepared(&snapshot, "forged", "{}"),
                &config,
                &HashSet::new(),
                &hook,
                &CancellationToken::new()
            )
            .await
            .unwrap(),
            Authorization::Terminal(_)
        ));
    }
    assert!(matches!(
        authorize_call(
            &prepared(&snapshot, "readonly", "{}"),
            &config(RuntimePermissionMode::ReadOnly),
            &HashSet::new(),
            &hook,
            &CancellationToken::new()
        )
        .await
        .unwrap(),
        Authorization::Terminal(_)
    ));
    for input in ["{".to_owned(), "[]".to_owned(), "x".repeat(256 * 1024 + 1)] {
        assert!(matches!(
            authorize_call(
                &prepared(&snapshot, "invalid", &input),
                &config(RuntimePermissionMode::FullAccess),
                &HashSet::new(),
                &hook,
                &CancellationToken::new()
            )
            .await
            .unwrap(),
            Authorization::Terminal(_)
        ));
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        authorize_call(
            &prepared(&snapshot, "stopped", "{}"),
            &config(RuntimePermissionMode::FullAccess),
            &HashSet::new(),
            &hook,
            &cancel
        )
        .await
        .unwrap(),
        Authorization::Terminal(_)
    ));
    revoked.cancel();
    assert!(matches!(
        authorize_call(
            &prepared(&snapshot, "revoked", "{}"),
            &config(RuntimePermissionMode::FullAccess),
            &HashSet::new(),
            &hook,
            &CancellationToken::new()
        )
        .await
        .unwrap(),
        Authorization::Terminal(_)
    ));
    assert_eq!(hook.prompts.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(snapshot.mcp_tool("mcp_unknown").is_none());
}

#[tokio::test]
async fn revocation_after_policy_authorization_prevents_dispatch() {
    let (snapshot, revoked, calls) = setup(Ok("receipt"), false);
    let hook = hook(RuntimeUserDecision::Once);
    let prepared = prepared(&snapshot, "revoke-before-dispatch", "{}");
    assert!(matches!(
        authorize_call(
            &prepared,
            &config(RuntimePermissionMode::FullAccess),
            &HashSet::new(),
            &hook,
            &CancellationToken::new()
        )
        .await
        .unwrap(),
        Authorization::Approved { .. }
    ));
    revoked.cancel();
    let tools = vega_tools::Tools::new(std::env::current_dir().unwrap()).unwrap();
    let (result, _) =
        execute_prepared_waiting(prepared, &tools, &hook, &CancellationToken::new(), None).await;
    assert_eq!(result.status, RuntimeToolStatus::Cancelled);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn running_full_access_call_honors_stop_and_revocation_without_replay() {
    for revoke in [false, true] {
        let (snapshot, revoked, calls) = setup(Ok("receipt"), true);
        let hook = hook(RuntimeUserDecision::Once);
        let prepared = prepared(&snapshot, "wait", "{}");
        let cancel = CancellationToken::new();
        assert!(matches!(
            authorize_call(
                &prepared,
                &config(RuntimePermissionMode::FullAccess),
                &HashSet::new(),
                &hook,
                &cancel
            )
            .await
            .unwrap(),
            Authorization::Approved { .. }
        ));
        let tools = vega_tools::Tools::new(std::env::current_dir().unwrap()).unwrap();
        let execute = execute_prepared_waiting(prepared, &tools, &hook, &cancel, None);
        let stop = async {
            while calls.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
            if revoke {
                revoked.cancel();
            } else {
                cancel.cancel();
            }
        };
        let ((result, _), _) = tokio::join!(execute, stop);
        assert_eq!(result.status, RuntimeToolStatus::Cancelled);
        assert!(result.output.contains("outcome unknown"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(hook.prompts.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn full_access_preserves_external_error_categories_without_prompt_or_retry() {
    let tools = vega_tools::Tools::new(std::env::current_dir().unwrap()).unwrap();
    for (error, expected) in [
        (
            McpDispatchFailure::InvalidProtocol,
            "Tool error: MCP invalid protocol response",
        ),
        (
            McpDispatchFailure::Limit,
            "Tool error: MCP response limit exceeded",
        ),
        (
            McpDispatchFailure::UnsupportedResult,
            "Tool error: MCP unsupported result content",
        ),
        (
            McpDispatchFailure::Authorization,
            "Tool error: MCP authorization required or invalid",
        ),
        (
            McpDispatchFailure::Transport,
            "Tool error: MCP transport failed; side-effect outcome unknown",
        ),
    ] {
        let (snapshot, _, calls) = setup(Err(error), false);
        let hook = hook(RuntimeUserDecision::Deny { note: None });
        let prepared = prepared(&snapshot, "error", "{}");
        let cancel = CancellationToken::new();
        assert!(matches!(
            authorize_call(
                &prepared,
                &config(RuntimePermissionMode::FullAccess),
                &HashSet::new(),
                &hook,
                &cancel
            )
            .await
            .unwrap(),
            Authorization::Approved { .. }
        ));
        let (result, _) = execute_prepared_waiting(prepared, &tools, &hook, &cancel, None).await;
        assert_eq!(result.status, RuntimeToolStatus::Failed);
        assert_eq!(result.output, expected);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(hook.prompts.load(Ordering::SeqCst), 0);
    }
}
