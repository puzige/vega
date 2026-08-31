use std::fs;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tempfile::tempdir;

use super::*;
use crate::{MockProvider, ScriptStep};

mod loop_tools;
mod permission_flow;
mod usage_limits;

struct FixedHook {
    calls: Arc<AtomicUsize>,
    decision: Option<RuntimeUserDecision>,
}

impl RuntimePermissionHook for FixedHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let decision = self.decision.clone();
        async move {
            match decision {
                Some(decision) => Ok(decision),
                None => futures::future::pending().await,
            }
        }
        .boxed()
    }
}

struct ProbeHook {
    fail: bool,
    token: Arc<Mutex<Option<CancellationToken>>>,
}

impl RuntimePermissionHook for ProbeHook {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        if let Ok(mut stored) = self.token.lock() {
            *stored = Some(cancel);
        }
        let fail = self.fail;
        async move {
            if fail {
                Err(VegaError::Tool {
                    tool: "permission".to_string(),
                    message: "closed".to_string(),
                })
            } else {
                Ok(RuntimeUserDecision::Once)
            }
        }
        .boxed()
    }
}

fn tool_config(
    run_mode: RuntimeRunMode,
    permission_mode: RuntimePermissionMode,
    checkpoint_root: PathBuf,
) -> RuntimeToolConfig {
    RuntimeToolConfig::new(
        run_mode,
        permission_mode,
        "project-1".to_string(),
        "thread-1".to_string(),
        checkpoint_root,
        Vec::new(),
    )
}

fn request(history: Vec<ChatMessage>) -> AgentRequest {
    AgentRequest {
        model: "mock".to_string(),
        system_prompt: "Be precise.".to_string(),
        history,
        max_tokens: None,
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::default(),
        pricing_catalog: None,
    }
}

async fn run_bash_permission_case(
    permission_mode: RuntimePermissionMode,
    exact_rule: bool,
    decision: RuntimeUserDecision,
    command: &str,
) -> (AgentOutcome, usize) {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let checkpoint = data.path().join("checkpoints");
    fs::create_dir(&checkpoint).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "bash-case".into(),
                name: "bash".into(),
                input_json: serde_json::json!({ "cmd": command }).to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let mut req = request(Vec::new());
    let mut config = tool_config(RuntimeRunMode::Execute, permission_mode, checkpoint);
    if exact_rule {
        config.exact_rules.push(RuntimeExactRule {
            tool: RuntimeMutatingTool::Bash,
            pattern: command.to_string(),
        });
    }
    req.tool_config = config;
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedHook {
        calls: calls.clone(),
        decision: Some(decision),
    };
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        req,
        CancellationToken::new(),
        &hook,
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    (outcome, calls.load(Ordering::SeqCst))
}

// ─── S7-T38 pricing pipeline ────────────────────────────────────────

fn priced_catalog() -> PricingCatalog {
    PricingCatalog::from_specs(vec![vega_token::ModelPricingSpec {
        model: "quote-model".to_string(),
        rates: vega_token::RateSpec {
            input_usd_per_million: "1".to_string(),
            output_usd_per_million: "2".to_string(),
            cache_read_usd_per_million: "0.1".to_string(),
            cache_write_usd_per_million: "0".to_string(),
        },
        max_standard_input_tokens: Some(2_000_000),
        schedule: None,
    }])
    .unwrap()
}
