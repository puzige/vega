use super::*;
use crate::command_scope::CommandScopeRun;
use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, CommandScopeDecision, RequestedCommandScope,
};
use vega_store::permission_contracts;

fn scope_request(
    fixture: &Fixture,
    snapshot: &SubagentSnapshot,
    cancel: CancellationToken,
) -> (
    CommandScopeRun,
    vega_tools::PreparedBash,
    crate::command_scope::PendingCommandScope,
) {
    let fence: crate::types::PermissionFence = serde_json::from_str(
        &permission_contracts::load_delegated_fence(
            fixture.store.conn(),
            &snapshot.agent_id,
            &snapshot.message_id,
        )
        .unwrap(),
    )
    .unwrap();
    message(
        &fixture.store,
        &snapshot.agent_id,
        &snapshot.message_id,
        "assistant",
        "streaming",
    );
    let tx = fixture.store.immediate_transaction().unwrap();
    permission_contracts::record_run_fence(
        &tx,
        &snapshot.message_id,
        &snapshot.agent_id,
        &serde_json::to_string(&fence).unwrap(),
    )
    .unwrap();
    tx.commit().unwrap();
    let scope = CommandScopeRun::new(
        &fixture.store,
        fence,
        snapshot.message_id.clone(),
        cancel,
        Arc::new(|| Ok(Vec::new())),
    )
    .unwrap();
    let tools = vega_tools::Tools::new(fixture._directory.path()).unwrap();
    let input = r#"{"cmd":"printf test","requested_scope":{"network":true,"reason":"test"}}"#;
    let prepared = tools.prepare_bash_json(input).unwrap();
    let call = id();
    vega_store::tool_calls::insert(
        fixture.store.conn(),
        vega_store::tool_calls::NewToolCall {
            id: &call,
            thread_id: &snapshot.agent_id,
            message_id: &snapshot.message_id,
            seq: 0,
            tool: "bash",
            input_json: input,
            description: None,
            status: "pending_approval",
            created_at: now_ms(),
        },
    )
    .unwrap();
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::User,
        danger: None,
    }
    .to_json()
    .unwrap();
    vega_store::tool_calls::approve(fixture.store.conn(), &call, &approval, None, now_ms())
        .unwrap();
    let requested: RequestedCommandScope =
        serde_json::from_str(r#"{"network":true,"reason":"test"}"#).unwrap();
    let pending = scope
        .request(
            &fixture.store,
            &call,
            &prepared,
            requested,
            Duration::from_secs(30),
        )
        .unwrap();
    (scope, prepared, pending)
}

#[tokio::test]
async fn spawned_and_followed_up_children_can_request_and_consume_scoped_permissions_after_steering()
 {
    let mut fixture = Fixture::new();
    fixture.steer();
    fixture.steer();
    fixture.steer();
    let mut snapshot = fixture.snapshot(None);
    let mut previous = None;
    for _ in 0..3 {
        fixture
            .admit(&snapshot, previous.take(), CancellationToken::new())
            .await
            .unwrap();
        let (scope, prepared, pending) =
            scope_request(&fixture, &snapshot, CancellationToken::new());
        let current = pending.request().clone();
        let token = scope
            .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
            .unwrap()
            .unwrap();
        scope
            .consume(&fixture.store, token, &current, &prepared)
            .unwrap();
        let audit: String = fixture
            .store
            .conn()
            .query_row(
                "SELECT state FROM command_scope_audits WHERE request_id=?1",
                [&current.request_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(audit, "consumed");
        fixture
            .store
            .conn()
            .execute(
                "UPDATE messages SET status='done' WHERE id=?1",
                [&snapshot.message_id],
            )
            .unwrap();
        previous = Some(fixture.complete(&snapshot));
        fixture.steer();
        fixture.steer();
        fixture.steer();
        snapshot = fixture.snapshot(Some(&snapshot.agent_id));
    }
}

#[tokio::test]
async fn scoped_permissions_reject_parent_end_cancellation_and_real_authority_changes() {
    for change in ["parent_end", "cancel", "revision", "epoch", "reset"] {
        for followup in [false, true] {
            let mut fixture = Fixture::new();
            let (snapshot, previous) = if followup {
                let first = fixture.snapshot(None);
                fixture
                    .admit(&first, None, CancellationToken::new())
                    .await
                    .unwrap();
                let boundary = fixture.complete(&first);
                (fixture.snapshot(Some(&first.agent_id)), Some(boundary))
            } else {
                (fixture.snapshot(None), None)
            };
            fixture.steer();
            fixture.steer();
            fixture.steer();
            fixture
                .admit(&snapshot, previous, CancellationToken::new())
                .await
                .unwrap();
            let cancel = CancellationToken::new();
            let (scope, _, pending) = scope_request(&fixture, &snapshot, cancel.clone());
            let request_id = pending.request().request_id.clone();
            match change {
                "parent_end" => {
                    fixture
                        .store
                        .conn()
                        .execute(
                            "UPDATE messages SET status='done' WHERE id=?1",
                            [&fixture.current],
                        )
                        .unwrap();
                }
                "cancel" => cancel.cancel(),
                "revision" => {
                    fixture.store.conn().execute("UPDATE threads SET permission_revision=permission_revision+1 WHERE id=?1", [&fixture.parent]).unwrap();
                }
                "epoch" => {
                    fixture
                        .store
                        .conn()
                        .execute(
                            "UPDATE permission_contract_state SET authority_epoch=?1",
                            [id()],
                        )
                        .unwrap();
                }
                "reset" => {
                    fixture
                        .store
                        .conn()
                        .execute(
                            "UPDATE permission_contract_state SET reset_state='pending'",
                            [],
                        )
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                scope
                    .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
                    .is_err()
            );
            let state: String = fixture
                .store
                .conn()
                .query_row(
                    "SELECT state FROM command_scope_audits WHERE request_id=?1",
                    [&request_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_ne!(state, "approved");
            assert_ne!(state, "consumed");
        }
    }
}
