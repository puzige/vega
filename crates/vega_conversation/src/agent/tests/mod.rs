use std::fs;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

use tempfile::tempdir;
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason};

use super::*;
use crate::types::{ConversationEvent, PermissionOptionChoice, ToolCallStatus};

mod automatic_titles;
mod credential_redaction;
mod failure_paths;
mod file_edit_parity;
mod history_permissions;
mod images;
mod mcp;
mod model_context;
mod permissions;
mod plan_approval;
mod responses;
mod skills;
mod stream_persistence;
mod tool_lifecycle;

struct FixedPermissionHook {
    calls: Arc<AtomicUsize>,
    decision: PermissionDecision,
}

impl PermissionHook for FixedPermissionHook {
    fn request(
        &self,
        _request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let decision = self.decision.clone();
        async move { Ok(decision) }.boxed()
    }
}

fn permission_request(tool: &str, target: &str) -> PermissionRequest {
    PermissionRequest {
        call_id: "opaque-call".into(),
        tool: tool.into(),
        display_target: target.into(),
        danger_rule_id: None,
        danger_reason: None,
        external: None,
        acp_options: None,
    }
}

fn setup() -> (Store, tempfile::TempDir, String) {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "// TODO: persist me\n").unwrap();
    let store = Store::open(dir.path().join("vega.db")).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        dir.path().to_str().unwrap(),
        "fixture",
        Some("master"),
    )
    .unwrap();
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread-1",
            project_id: &project.id,
            title: "",
            mode: "execute",
            permission_mode: "confirm",
            model: "mock-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    (store, dir, project.id)
}

fn setup_external(permission_mode: &str) -> (Store, tempfile::TempDir, tempfile::TempDir, String) {
    let project_dir = tempdir().unwrap();
    let data_dir = tempdir().unwrap();
    let store = Store::open(data_dir.path().join("vega.db")).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        project_dir.path().to_str().unwrap(),
        "external-fixture",
        Some("master"),
    )
    .unwrap();
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread-1",
            project_id: &project.id,
            title: "",
            mode: "execute",
            permission_mode,
            model: "mock-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    (store, project_dir, data_dir, project.id)
}

fn scripted_provider(call_id: &str, input_json: &str) -> MockProvider {
    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Checking. ".into()),
            ProviderEvent::ThinkingDelta("Need grep".into()),
            ProviderEvent::ToolUse {
                id: call_id.into(),
                name: "grep".into(),
                input_json: input_json.into(),
            },
            ProviderEvent::Usage {
                input: 12,
                output: 3,
                cache_read: 2,
                cache_write: 1,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Found the TODO.".into()),
            ProviderEvent::Usage {
                input: 20,
                output: 5,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ])
}

#[test]
fn issue75_history_accepts_only_dedicated_safe_audit_and_session_approval() {
    use crate::types::{
        Approval, ApprovalAudit, ApprovalSource, ToolCall, ToolCardInputProjection,
        tool_card_input_projection,
    };
    let input=serde_json::json!({"kind":"computer_use","version":1,"action":"click","argument_bytes":24,"argument_digest":"a".repeat(64)}).to_string();
    let audit = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ComputerUse,
        danger: None,
    };
    let encoded = audit.to_json().unwrap();
    assert!(encoded.contains("computer_use"));
    let call = ToolCall {
        id: "call".into(),
        tool: "ComputerAct".into(),
        input_json: input.clone(),
    };
    assert!(matches!(
        tool_card_input_projection(&call),
        ToolCardInputProjection::ComputerUse { .. }
    ));
    assert!(
        super::pipeline::validate_recovered_projection(
            "p",
            "t",
            "call",
            "ComputerAct",
            &input,
            "completed; observation was not retained",
            vega_runtime::RuntimeToolStatus::Success,
            &audit,
            None,
            None
        )
        .is_ok()
    );
    let mut always = audit.clone();
    always.decision = Approval::Always;
    assert!(always.to_json().is_err());
    let mut ordinary = audit;
    ordinary.source = ApprovalSource::User;
    assert!(
        super::pipeline::validate_recovered_projection(
            "p",
            "t",
            "call",
            "ComputerAct",
            &input,
            "completed",
            vega_runtime::RuntimeToolStatus::Success,
            &ordinary,
            None,
            None
        )
        .is_err()
    );
    assert!(
        super::pipeline::validate_recovered_projection(
            "p",
            "t",
            "call",
            "ComputerAct",
            r#"{"text":"secret"}"#,
            "completed",
            vega_runtime::RuntimeToolStatus::Success,
            &ordinary,
            None,
            None
        )
        .is_err()
    );
}
