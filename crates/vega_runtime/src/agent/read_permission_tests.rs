use super::{Authorization, PreparedRuntimeCall, authorize_call, execute_prepared_waiting};
use crate::agent::{
    CANCELLED_BEFORE_EXECUTION_OUTPUT, RuntimeExactRule, RuntimePermissionHook, RuntimeToolCall,
    RuntimeToolConfig, RuntimeToolStatus,
};
use crate::error::VegaError;
use crate::{
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeMutatingTool,
    RuntimePermissionMode, RuntimePermissionPrompt, RuntimePermissionTarget, RuntimeRunMode,
    RuntimeUserDecision,
};
use futures::future::BoxFuture;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;
use vega_tools::Tools;

const PROJECT_CONTENT: &str = "owned project read fixture";
const EXTERNAL_CONTENT: &str = "owned external read fixture";

struct ReadFixture(PathBuf);

impl ReadFixture {
    fn new() -> Self {
        let fixture = Self(
            std::env::temp_dir().join(format!("vega-read-permission-{}", ulid::Ulid::generate())),
        );
        std::fs::create_dir(&fixture.0).unwrap();
        std::fs::create_dir(fixture.0.join("project")).unwrap();
        std::fs::write(fixture.0.join("project/inside.txt"), PROJECT_CONTENT).unwrap();
        std::fs::write(fixture.0.join("outside.txt"), EXTERNAL_CONTENT).unwrap();
        fixture
    }

    fn tools(&self) -> Tools {
        Tools::new(self.0.join("project")).unwrap()
    }

    fn config(&self, mode: RuntimePermissionMode) -> RuntimeToolConfig {
        RuntimeToolConfig::new(
            RuntimeRunMode::Execute,
            mode,
            "project".into(),
            "thread".into(),
            self.0.join("checkpoints"),
            Vec::new(),
        )
    }

    fn prepared(&self, tools: &Tools, name: &str, external: bool) -> PreparedRuntimeCall {
        let requested = if external {
            self.0.join("outside.txt")
        } else {
            PathBuf::from("inside.txt")
        };
        let resolved = tools.file_path(requested.to_str().unwrap()).unwrap();
        assert_eq!(resolved.starts_with(tools.root()), !external);
        let path = resolved.to_str().unwrap().to_owned();
        let call = RuntimeToolCall {
            id: ulid::Ulid::generate().to_string(),
            name: name.into(),
            input_json: serde_json::json!({"file_path": path}).to_string(),
        };
        if external {
            PreparedRuntimeCall::ExternalRead {
                target: RuntimePermissionTarget {
                    call_id: call.id.clone(),
                    tool: RuntimeMutatingTool::Read,
                    exact_pattern: path.clone(),
                    display_target: path,
                },
                call,
            }
        } else {
            PreparedRuntimeCall::PinnedRead {
                execution: call.clone(),
                call,
            }
        }
    }
}

impl Drop for ReadFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct UserDecision {
    decision: RuntimeUserDecision,
    prompts: Mutex<Vec<RuntimePermissionPrompt>>,
}

impl UserDecision {
    fn new(decision: RuntimeUserDecision) -> Self {
        Self {
            decision,
            prompts: Mutex::new(Vec::new()),
        }
    }
}

impl RuntimePermissionHook for UserDecision {
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

fn audit(
    decision: RuntimeApprovalDecision,
    source: RuntimeApprovalSource,
    note: Option<&str>,
) -> RuntimeApprovalAudit {
    RuntimeApprovalAudit {
        decision,
        source,
        note: note.map(str::to_owned),
        danger: None,
    }
}

#[tokio::test]
async fn valid_reads_keep_content_and_automatic_authorization() {
    for name in ["read", "Read"] {
        for (external, mode, source, content) in [
            (
                false,
                RuntimePermissionMode::Confirm,
                RuntimeApprovalSource::ReadonlyTool,
                PROJECT_CONTENT,
            ),
            (
                true,
                RuntimePermissionMode::FullAccess,
                RuntimeApprovalSource::FullAccess,
                EXTERNAL_CONTENT,
            ),
        ] {
            let fixture = ReadFixture::new();
            let tools = fixture.tools();
            let prepared = fixture.prepared(&tools, name, external);
            let call_id = prepared.call().id.clone();
            let hook = UserDecision::new(RuntimeUserDecision::Deny { note: None });
            let cancel = CancellationToken::new();
            let authorization = authorize_call(
                &prepared,
                &fixture.config(mode),
                &HashSet::<RuntimeExactRule>::new(),
                &hook,
                &cancel,
            )
            .await
            .unwrap();
            let Authorization::Approved {
                audit: approval,
                remember_rule,
            } = authorization
            else {
                panic!("valid {name} must be approved: external={external}");
            };
            assert_eq!(approval, audit(RuntimeApprovalDecision::Once, source, None));
            assert!(remember_rule.is_none());
            let (result, cancelled) = execute_prepared_waiting(prepared, &tools, &cancel).await;
            assert!(!cancelled);
            assert_eq!(result.call_id, call_id);
            assert_eq!(result.status, RuntimeToolStatus::Success);
            assert_eq!(result.output, format!("1 | {content}"));
            assert_eq!(result.truncated, Some(false));
            assert!(result.approval.is_none());
            assert!(hook.prompts.lock().unwrap().is_empty());
        }
    }
}

#[tokio::test]
async fn external_reads_follow_user_approval_or_denial() {
    for name in ["read", "Read"] {
        for approved in [true, false] {
            let fixture = ReadFixture::new();
            let tools = fixture.tools();
            let prepared = fixture.prepared(&tools, name, true);
            let call_id = prepared.call().id.clone();
            let target = match &prepared {
                PreparedRuntimeCall::ExternalRead { target, .. } => target.clone(),
                _ => unreachable!(),
            };
            let note = "keep this file private";
            let hook = UserDecision::new(if approved {
                RuntimeUserDecision::Once
            } else {
                RuntimeUserDecision::Deny {
                    note: Some(note.into()),
                }
            });
            let cancel = CancellationToken::new();
            let authorization = authorize_call(
                &prepared,
                &fixture.config(RuntimePermissionMode::Confirm),
                &HashSet::new(),
                &hook,
                &cancel,
            )
            .await
            .unwrap();
            match authorization {
                Authorization::Approved {
                    audit: approval,
                    remember_rule,
                } => {
                    assert!(approved, "denied {name} must not become executable");
                    assert_eq!(
                        approval,
                        audit(
                            RuntimeApprovalDecision::Once,
                            RuntimeApprovalSource::User,
                            None
                        )
                    );
                    assert!(remember_rule.is_none());
                    let (result, cancelled) =
                        execute_prepared_waiting(prepared, &tools, &cancel).await;
                    assert!(!cancelled);
                    assert_eq!(result.call_id, call_id);
                    assert_eq!(result.status, RuntimeToolStatus::Success);
                    assert_eq!(result.output, format!("1 | {EXTERNAL_CONTENT}"));
                    assert!(result.approval.is_none());
                }
                Authorization::Terminal(result) => {
                    assert!(!approved, "approved {name} must remain executable");
                    assert_eq!(result.call_id, call_id);
                    assert_eq!(result.status, RuntimeToolStatus::Rejected);
                    assert_eq!(result.output, "Tool error: permission denied");
                    assert!(!result.output.contains(EXTERNAL_CONTENT));
                    assert_eq!(
                        result.approval,
                        Some(audit(
                            RuntimeApprovalDecision::Deny,
                            RuntimeApprovalSource::User,
                            Some(note)
                        ))
                    );
                    assert!(result.remember_rule.is_none());
                }
            }
            assert_eq!(
                *hook.prompts.lock().unwrap(),
                vec![RuntimePermissionPrompt {
                    target,
                    danger: None
                }]
            );
        }
    }
}

#[tokio::test]
async fn cancellation_before_authorization_or_execution_never_returns_content() {
    for name in ["read", "Read"] {
        for external in [false, true] {
            for before_authorization in [true, false] {
                let fixture = ReadFixture::new();
                let tools = fixture.tools();
                let prepared = fixture.prepared(&tools, name, external);
                let call_id = prepared.call().id.clone();
                let hook = UserDecision::new(RuntimeUserDecision::Once);
                let cancel = CancellationToken::new();
                if before_authorization {
                    cancel.cancel();
                }
                let authorization = authorize_call(
                    &prepared,
                    &fixture.config(RuntimePermissionMode::FullAccess),
                    &HashSet::new(),
                    &hook,
                    &cancel,
                )
                .await
                .unwrap();
                let result = if before_authorization {
                    let Authorization::Terminal(result) = authorization else {
                        panic!("cancelled {name} must not be approved");
                    };
                    assert_eq!(result.status, RuntimeToolStatus::Rejected);
                    assert_eq!(result.output, "Tool error: permission denied");
                    assert_eq!(
                        result.approval,
                        Some(audit(
                            RuntimeApprovalDecision::Deny,
                            RuntimeApprovalSource::Timeout,
                            None
                        ))
                    );
                    result
                } else {
                    let Authorization::Approved {
                        audit: approval,
                        remember_rule,
                    } = authorization
                    else {
                        panic!("valid {name} must be approved before cancellation");
                    };
                    assert_eq!(
                        approval,
                        audit(
                            RuntimeApprovalDecision::Once,
                            if external {
                                RuntimeApprovalSource::FullAccess
                            } else {
                                RuntimeApprovalSource::ReadonlyTool
                            },
                            None
                        )
                    );
                    assert!(remember_rule.is_none());
                    cancel.cancel();
                    let (result, cancelled) =
                        execute_prepared_waiting(prepared, &tools, &cancel).await;
                    assert!(cancelled);
                    assert_eq!(result.status, RuntimeToolStatus::Cancelled);
                    assert_eq!(result.output, CANCELLED_BEFORE_EXECUTION_OUTPUT);
                    assert!(result.approval.is_none());
                    result
                };
                assert_eq!(result.call_id, call_id);
                assert!(!result.output.contains(PROJECT_CONTENT));
                assert!(!result.output.contains(EXTERNAL_CONTENT));
                assert!(result.remember_rule.is_none());
                assert!(hook.prompts.lock().unwrap().is_empty());
            }
        }
    }
}
