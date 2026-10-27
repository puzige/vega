use std::path::PathBuf;
use std::sync::Mutex;

use super::*;
use crate::agent::{PersistenceActorConfig, PreparedRun, prepare_run_with_images_and_reasoning};
use vega_credentials::CredentialBinding;
use vega_store::tool_calls;

struct Fixture {
    root: PathBuf,
    store: Store,
    accepted: PreparedRun,
    tools: vega_tools::Tools,
    bindings: Arc<Mutex<Result<Vec<CredentialBinding>, CommandScopeError>>>,
    cancel: CancellationToken,
}

impl Fixture {
    fn new(mode: &str, permission: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-command-scope-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("isolated fixture");
        let root = root.canonicalize().expect("canonical fixture");
        let database = root.join("vega.db");
        let store = Store::open(&database).expect("fixture store");
        store.migrate().expect("fixture schema");
        let thread = ulid::Ulid::generate().to_string();
        store.conn().execute(
            "INSERT INTO threads(id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,'synthetic',?2,?3,1,1)",
            [thread.as_str(), mode, permission],
        ).expect("fixture thread");
        let accepted = prepare_run_with_images_and_reasoning(
            database,
            thread,
            "synthetic request".into(),
            "synthetic system".into(),
            ulid::Ulid::generate().to_string(),
            ulid::Ulid::generate().to_string(),
            PersistenceActorConfig::default(),
            false,
            None,
            None,
            Vec::new(),
            None,
        )
        .expect("real accepted run");
        let tools = vega_tools::Tools::new(&accepted.permission_fence.workspace)
            .expect("prepared workspace");
        Self {
            root,
            store,
            accepted,
            tools,
            bindings: Arc::new(Mutex::new(Ok(Vec::new()))),
            cancel: CancellationToken::new(),
        }
    }

    fn owner(&self) -> CommandScopeRun {
        let values = self.bindings.clone();
        CommandScopeRun::new(
            &self.store,
            self.accepted.permission_fence.clone(),
            self.accepted.assistant_message_id.clone(),
            self.cancel.clone(),
            Arc::new(move || values.lock().expect("fixture binding reader").clone()),
        )
        .expect("live owner")
    }

    fn call(&self, id: &str, command: &str) -> PreparedBash {
        let readonly = self.accepted.permission_fence.run_mode != "execute"
            || self.accepted.permission_fence.permission_mode == "readonly";
        let audit = ApprovalAudit {
            decision: Approval::Once,
            note: None,
            source: if readonly {
                ApprovalSource::SandboxReadOnly
            } else {
                ApprovalSource::User
            },
            danger: None,
        };
        self.call_with(id, command, &audit)
    }

    fn call_with(&self, id: &str, command: &str, audit: &ApprovalAudit) -> PreparedBash {
        let input = serde_json::json!({"command":command,"timeout":17}).to_string();
        let seq: i64 = self
            .store
            .conn()
            .query_row("SELECT COALESCE(MAX(seq),0)+1 FROM tool_calls", [], |row| {
                row.get(0)
            })
            .expect("next call sequence");
        tool_calls::insert_pending(
            self.store.conn(),
            tool_calls::NewToolCall {
                id,
                thread_id: &self.accepted.permission_fence.thread_id,
                message_id: &self.accepted.assistant_message_id,
                seq,
                tool: "bash",
                input_json: &input,
                status: "pending_approval",
                created_at: 1,
            },
        )
        .expect("pending Bash");
        tool_calls::approve(
            self.store.conn(),
            id,
            &audit.to_json().expect("strict audit"),
            None,
            2,
        )
        .expect("ordinary approval");
        self.tools
            .prepare_bash_json(&input)
            .expect("prepared command")
    }

    fn pending(
        &self,
        owner: &CommandScopeRun,
        id: &str,
        prepared: &PreparedBash,
    ) -> PendingCommandScope {
        owner
            .request(
                &self.store,
                id,
                prepared,
                network(),
                Duration::from_secs(30),
            )
            .expect("pending scope")
    }

    fn state(&self, id: &str) -> String {
        self.store
            .conn()
            .query_row(
                "SELECT state FROM command_scope_audits WHERE call_id=?1",
                [id],
                |row| row.get(0),
            )
            .expect("persisted scope state")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn network() -> RequestedCommandScope {
    RequestedCommandScope::from_json(r#"{"network":true,"reason":"Fetch requested content"}"#)
        .expect("network scope")
}

fn binding() -> CredentialBinding {
    CredentialBinding {
        key_ref: "synthetic-reference".into(),
        env: "SERVICE_TOKEN".into(),
        origin: "https://example.com".into(),
        header: "authorization".into(),
        prefix: "Bearer ".into(),
        path: "/api".into(),
    }
}

fn failure<T>(result: Result<T, CommandScopeError>, expected: CommandScopeError) {
    match result {
        Err(actual) => assert_eq!(actual, expected),
        Ok(_) => panic!("operation unexpectedly authorized"),
    }
}

fn copy_state(state: &ScopeState) -> ScopeState {
    ScopeState {
        owner: state.owner.clone(),
        request: state.request.clone(),
        encoded: state.encoded.clone(),
        deadline: state.deadline,
    }
}

#[test]
fn scope_persistence_cas_rejects_stale_decisions_and_duplicate_consumption() {
    let fixture = Fixture::new("execute", "confirm");
    let owner = fixture.owner();
    let prepared = fixture.call("cas", "printf fixture");
    let pending = fixture.pending(&owner, "cas", &prepared);
    let stale = PendingCommandScope {
        state: copy_state(&pending.state),
    };
    let current = pending.request().clone();
    let token = owner
        .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
        .expect("first response")
        .expect("token");
    failure(
        owner.decide(&fixture.store, stale, CommandScopeDecision::Deny),
        CommandScopeError::Conflict,
    );
    assert_eq!(fixture.state("cas"), "approved");
    let stale = CommandScopeToken {
        state: copy_state(&token.state),
    };
    owner
        .consume(&fixture.store, token, &current, &prepared)
        .expect("first consumption");
    failure(
        owner.consume(&fixture.store, stale, &current, &prepared),
        CommandScopeError::Conflict,
    );
    assert_eq!(fixture.state("cas"), "consumed");
    let prepared = fixture.call("expired-token", "printf fixture");
    let pending = fixture.pending(&owner, "expired-token", &prepared);
    let current = pending.request().clone();
    let mut token = owner
        .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
        .expect("approved")
        .expect("token");
    token.state.deadline = Instant::now();
    failure(
        owner.consume(&fixture.store, token, &current, &prepared),
        CommandScopeError::Expired,
    );
    assert_eq!(fixture.state("expired-token"), "timeout");
    let prepared = fixture.call("replacement-owner", "printf fixture");
    let pending = fixture.pending(&owner, "replacement-owner", &prepared);
    let current = pending.request().clone();
    let token = owner
        .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
        .expect("approved")
        .expect("token");
    let replacement = fixture.owner();
    failure(
        replacement.consume(&fixture.store, token, &current, &prepared),
        CommandScopeError::StaleAuthority,
    );
    assert_eq!(fixture.state("replacement-owner"), "failed");
}

#[test]
fn delegated_scope_keeps_parent_authority_and_rejects_parent_or_ownership_loss() {
    for variant in 0..4 {
        let mut fixture = Fixture::new("execute", "confirm");
        let parent = fixture.accepted.permission_fence.clone();
        let parent_run = fixture.accepted.assistant_message_id.clone();
        let child = ulid::Ulid::generate().to_string();
        let child_run = ulid::Ulid::generate().to_string();
        let user = ulid::Ulid::generate().to_string();
        fixture.store.conn().execute("INSERT INTO threads(id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,'synthetic','execute','confirm',1,1)", [&child]).expect("child thread");
        for (id, seq, role, status) in [
            (&user, 1, "user", "done"),
            (&child_run, 2, "assistant", "streaming"),
        ] {
            vega_store::messages::insert(
                fixture.store.conn(),
                &vega_store::messages::MessageRow {
                    id: id.clone(),
                    thread_id: child.clone(),
                    seq,
                    role: role.into(),
                    kind: "text".into(),
                    content: "synthetic".into(),
                    status: status.into(),
                    created_at: 1,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .expect("child message");
        }
        vega_store::subagents::insert(
            fixture.store.conn(),
            &vega_store::subagents::DelegationRow {
                child_thread_id: child.clone(),
                parent_thread_id: parent.thread_id.clone(),
                parent_message_id: parent_run.clone(),
                spawn_call_id: "spawn".into(),
                child_message_id: child_run.clone(),
                name: "synthetic".into(),
                task: "synthetic".into(),
                state: "running".into(),
                result: String::new(),
                truncated: false,
                created_at: 1,
            },
        )
        .expect("delegated ownership row");
        vega_store::delegated_stages::register(
            fixture.store.conn(),
            &child,
            &child_run,
            None,
            &serde_json::json!([]),
        )
        .expect("held ownership");
        let mut fence = parent.clone();
        fence.thread_id = child.clone();
        fence.full_access_grant = None;
        fence.delegation = Some(crate::types::DelegatedPermissionSource {
            parent_message_id: parent_run.clone(),
            child_message_id: child_run.clone(),
            parent_fence: Box::new(parent),
        });
        assert!(fence.is_valid());
        let tx = fixture
            .store
            .immediate_transaction()
            .expect("fence transaction");
        let encoded = serde_json::to_string(&fence).expect("encode child fence");
        permission_contracts::record_run_fence(&tx, &child_run, &child, &encoded)
            .expect("accepted child fence");
        permission_contracts::record_delegated_fence(&tx, &child, &child_run, &encoded)
            .expect("delegated fence");
        tx.commit().expect("persist fences");
        fixture.accepted.permission_fence = fence;
        fixture.accepted.assistant_message_id = child_run.clone();
        let owner = fixture.owner();
        let prepared = fixture.call("delegated", "printf fixture");
        let pending = fixture.pending(&owner, "delegated", &prepared);
        let current = pending.request().clone();
        let token = owner
            .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
            .expect("independent child approval")
            .expect("token");
        match variant {
            0 => {
                owner
                    .consume(&fixture.store, token, &current, &prepared)
                    .expect("exact active parent");
                assert_eq!(fixture.state("delegated"), "consumed");
                continue;
            }
            1 => {
                fixture
                    .store
                    .conn()
                    .execute(
                        "UPDATE messages SET status='interrupted' WHERE id=?1",
                        [&parent_run],
                    )
                    .expect("parent stopped");
            }
            2 => {
                fixture.store.conn().execute("UPDATE delegated_executions SET state='cancelling' WHERE child_message_id=?1", [&child_run]).expect("child cancelling");
            }
            3 => {
                fixture.store.conn().execute("UPDATE delegated_execution_recovery SET release_state='quarantined' WHERE child_message_id=?1", [&child_run]).expect("ownership uncertain");
            }
            _ => unreachable!(),
        }
        failure(
            owner.consume(&fixture.store, token, &current, &prepared),
            CommandScopeError::StaleAuthority,
        );
        assert_eq!(fixture.state("delegated"), "failed");
    }
}

#[test]
fn scope_creation_and_decision_commit_failures_never_publish_live_authority() {
    let fixture = Fixture::new("execute", "confirm");
    let owner = fixture.owner();
    let prepared = fixture.call("insert-failure", "printf fixture");
    fixture.store.conn().execute_batch("CREATE TRIGGER reject_scope_insert BEFORE INSERT ON command_scope_audits BEGIN SELECT RAISE(ABORT,'synthetic insert failure'); END;").expect("insert fault");
    failure(
        owner.request(
            &fixture.store,
            "insert-failure",
            &prepared,
            network(),
            Duration::from_secs(30),
        ),
        CommandScopeError::Unavailable,
    );
    let count: i64 = fixture
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM command_scope_audits", [], |row| {
            row.get(0)
        })
        .expect("no published audit");
    assert_eq!(count, 0);
    fixture.store.conn().execute_batch("DROP TRIGGER reject_scope_insert; CREATE TABLE scope_commit_fault(parent_id TEXT REFERENCES threads(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER reject_scope_commit AFTER UPDATE ON command_scope_audits WHEN NEW.state IN ('approved','consumed') BEGIN INSERT INTO scope_commit_fault VALUES('missing-parent'); END;").expect("commit fault");
    let pending = fixture.pending(&owner, "insert-failure", &prepared);
    failure(
        owner.decide(&fixture.store, pending, CommandScopeDecision::AllowOnce),
        CommandScopeError::Unavailable,
    );
    assert_eq!(fixture.state("insert-failure"), "failed");
    fixture
        .store
        .conn()
        .execute_batch("DROP TRIGGER reject_scope_commit;")
        .expect("remove decision fault");
    let prepared = fixture.call("consume-commit", "printf fixture");
    let pending = fixture.pending(&owner, "consume-commit", &prepared);
    let current = pending.request().clone();
    let token = owner
        .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
        .expect("approved")
        .expect("token");
    fixture.store.conn().execute_batch("CREATE TRIGGER reject_consume_commit AFTER UPDATE ON command_scope_audits WHEN NEW.state='consumed' BEGIN INSERT INTO scope_commit_fault VALUES('missing-parent'); END;").expect("consumption commit fault");
    failure(
        owner.consume(&fixture.store, token, &current, &prepared),
        CommandScopeError::Unavailable,
    );
    assert_eq!(fixture.state("consume-commit"), "failed");
}

#[test]
fn scope_codec_rejects_ordinary_approvals_empty_ranges_and_ambiguous_fields() {
    for raw in [
        r#"{}"#,
        r#"{"reason":"no scope"}"#,
        r#"{"network":false,"reason":"empty"}"#,
        r#"{"network":true}"#,
        r#"{"network":true,"reason":" "}"#,
        r#"{"network":true,"reason":"line\nbreak"}"#,
        r#"{"network":true,"network":false,"reason":"duplicate"}"#,
        r#"{"network":true,"reason":"why","reason":"changed"}"#,
        r#"{"network":null,"reason":"null"}"#,
        r#"{"network":true,"reason":"why","approved":true}"#,
        r#"{"network":true,"reason":"why","full_access":true}"#,
        r#"{"write_roots":["relative"],"reason":"write"}"#,
        r#"{"write_roots":["/tmp/../etc"],"reason":"write"}"#,
        r#"{"write_roots":["/tmp/\"quoted"],"reason":"write"}"#,
    ] {
        assert!(RequestedCommandScope::from_json(raw).is_err(), "{raw}");
    }
    let oversized = serde_json::json!({"network":true,"reason":"x".repeat(513)}).to_string();
    assert!(RequestedCommandScope::from_json(&oversized).is_err());
    for raw in [
        r#""once""#,
        r#""always""#,
        r#"{"option_id":"allow_once"}"#,
        r#"{"decision":"allow_once"}"#,
    ] {
        assert!(
            serde_json::from_str::<CommandScopeDecision>(raw).is_err(),
            "{raw}"
        );
    }
    for (raw, expected) in [
        (r#""allow_once""#, CommandScopeDecision::AllowOnce),
        (r#""deny""#, CommandScopeDecision::Deny),
        (r#""timeout""#, CommandScopeDecision::Timeout),
    ] {
        assert_eq!(
            serde_json::from_str::<CommandScopeDecision>(raw).expect("scope decision"),
            expected
        );
    }
    let scope = network();
    assert_eq!(
        scope,
        serde_json::from_str(&serde_json::to_string(&scope).expect("encode")).expect("round trip")
    );
    assert!(!format!("{scope:?}").contains(scope.reason()));
}

#[test]
fn scope_approval_is_separate_single_use_and_keeps_ordinary_audit_unchanged() {
    let fixture = Fixture::new("execute", "confirm");
    let owner = fixture.owner();
    let prepared = fixture.call("once", "printf fixture");
    let before = tool_calls::find_state(fixture.store.conn(), "once").expect("ordinary state");
    let pending = fixture.pending(&owner, "once", &prepared);
    let current = pending.request().clone();
    assert_eq!(fixture.state("once"), "pending");
    assert_eq!(current.command(), prepared.command());
    assert_eq!(current.timeout_ms(), 17_000);
    assert!(!format!("{current:?}").contains(prepared.command()));
    let token = owner
        .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
        .expect("explicit scope decision")
        .expect("live token");
    assert_eq!(fixture.state("once"), "approved");
    let receipt = owner
        .consume(&fixture.store, token, &current, &prepared)
        .expect("single consumption");
    assert_eq!(receipt.request(), &current);
    assert_eq!(fixture.state("once"), "consumed");
    failure(
        owner.request(
            &fixture.store,
            "once",
            &prepared,
            network(),
            Duration::from_secs(30),
        ),
        CommandScopeError::Conflict,
    );
    let recreated = fixture.owner();
    failure(
        recreated.request(
            &fixture.store,
            "once",
            &prepared,
            network(),
            Duration::from_secs(30),
        ),
        CommandScopeError::Conflict,
    );
    assert_eq!(
        tool_calls::find_state(fixture.store.conn(), "once").expect("ordinary retained"),
        before
    );
    let rules: i64 = fixture
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM permissions", [], |row| row.get(0))
        .expect("rules");
    assert_eq!(rules, 0);
    let saved: String = fixture
        .store
        .conn()
        .query_row(
            "SELECT request FROM command_scope_audits WHERE call_id='once'",
            [],
            |row| row.get(0),
        )
        .expect("audit request");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&saved).expect("audit JSON"),
        serde_json::to_value(&current).expect("exact request")
    );
}

#[test]
fn scope_denial_timeout_cancel_and_lost_owner_never_produce_a_token() {
    let fixture = Fixture::new("execute", "auto");
    let owner = fixture.owner();
    for (call, decision, expected) in [
        ("denied", CommandScopeDecision::Deny, "denied"),
        ("timeout", CommandScopeDecision::Timeout, "timeout"),
    ] {
        let prepared = fixture.call(call, "printf fixture");
        let pending = fixture.pending(&owner, call, &prepared);
        assert!(
            owner
                .decide(&fixture.store, pending, decision)
                .expect("record decline")
                .is_none()
        );
        assert_eq!(fixture.state(call), expected);
        failure(
            owner.request(
                &fixture.store,
                call,
                &prepared,
                network(),
                Duration::from_secs(30),
            ),
            CommandScopeError::Conflict,
        );
    }
    let prepared = fixture.call("expired", "printf fixture");
    let mut pending = fixture.pending(&owner, "expired", &prepared);
    pending.state.deadline = Instant::now();
    failure(
        owner.decide(&fixture.store, pending, CommandScopeDecision::AllowOnce),
        CommandScopeError::Expired,
    );
    assert_eq!(fixture.state("expired"), "timeout");
    let prepared = fixture.call("lost", "printf fixture");
    let old = fixture.owner();
    let pending = fixture.pending(&old, "lost", &prepared);
    drop(old);
    failure(
        owner.decide(&fixture.store, pending, CommandScopeDecision::AllowOnce),
        CommandScopeError::OwnerGone,
    );
    assert_eq!(fixture.state("lost"), "cancelled");
    let prepared = fixture.call("cancelled", "printf fixture");
    let pending = fixture.pending(&owner, "cancelled", &prepared);
    fixture.cancel.cancel();
    failure(
        owner.decide(&fixture.store, pending, CommandScopeDecision::AllowOnce),
        CommandScopeError::Cancelled,
    );
    assert_eq!(fixture.state("cancelled"), "cancelled");
}

#[test]
fn scope_consumption_rejects_every_changed_identity_and_prepared_command() {
    let fixture = Fixture::new("execute", "confirm");
    let owner = fixture.owner();
    for variant in 0..14 {
        let call = format!("identity-{variant}");
        let prepared = fixture.call(&call, "printf exact");
        let pending = fixture.pending(&owner, &call, &prepared);
        let mut current = pending.request().clone();
        let token = owner
            .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
            .expect("approval")
            .expect("token");
        let replacement = match variant {
            0 => {
                current.command.push(' ');
                None
            }
            1 => {
                current.timeout_ms += 1;
                None
            }
            2 => {
                current.call_id.push('x');
                None
            }
            3 => {
                current.run_id.push('x');
                None
            }
            4 => {
                current.request_id.push('x');
                None
            }
            5 => {
                current.project_root.push_str("/changed");
                None
            }
            6 => {
                current.fence.permission_revision += 1;
                None
            }
            7 => {
                current.scope = RequestedCommandScope::from_json(
                    r#"{"network":true,"reason":"different reason"}"#,
                )
                .expect("different scope");
                None
            }
            8 => {
                current.bindings.push(binding());
                None
            }
            9 => {
                current.tool_instance += 1;
                None
            }
            10 => Some(
                fixture
                    .tools
                    .prepare_bash_json(r#"{"command":"printf changed","timeout":17}"#)
                    .expect("different command"),
            ),
            11 => Some(
                fixture
                    .tools
                    .prepare_bash_json(r#"{"command":"printf exact","timeout":18}"#)
                    .expect("different timeout"),
            ),
            12 => Some(
                vega_tools::Tools::new(fixture.tools.root())
                    .expect("different instance")
                    .prepare_bash_json(r#"{"command":"printf exact","timeout":17}"#)
                    .expect("different prepared instance"),
            ),
            13 => {
                current
                    .fence
                    .exact_rules
                    .push(("bash".into(), "printf exact".into()));
                None
            }
            _ => unreachable!(),
        };
        failure(
            owner.consume(
                &fixture.store,
                token,
                &current,
                replacement.as_ref().unwrap_or(&prepared),
            ),
            CommandScopeError::StaleAuthority,
        );
        assert_eq!(fixture.state(&call), "failed");
    }
}

#[test]
fn scope_binding_snapshot_is_rechecked_at_request_decision_and_consumption() {
    let fixture = Fixture::new("execute", "confirm");
    let owner = fixture.owner();
    let prepared = fixture.call("request-error", "printf fixture");
    *fixture.bindings.lock().expect("reader state") = Err(CommandScopeError::Unavailable);
    failure(
        owner.request(
            &fixture.store,
            "request-error",
            &prepared,
            network(),
            Duration::from_secs(30),
        ),
        CommandScopeError::Unavailable,
    );
    *fixture.bindings.lock().expect("reader state") = Ok(vec![binding(), binding()]);
    failure(
        owner.request(
            &fixture.store,
            "request-error",
            &prepared,
            network(),
            Duration::from_secs(30),
        ),
        CommandScopeError::InvalidRequest,
    );
    *fixture.bindings.lock().expect("reader state") = Ok(Vec::new());
    let pending = fixture.pending(&owner, "request-error", &prepared);
    *fixture.bindings.lock().expect("reader state") = Ok(vec![binding()]);
    failure(
        owner.decide(&fixture.store, pending, CommandScopeDecision::AllowOnce),
        CommandScopeError::StaleAuthority,
    );
    assert_eq!(fixture.state("request-error"), "failed");
    let prepared = fixture.call("dispatch-binding", "printf fixture");
    let pending = fixture.pending(&owner, "dispatch-binding", &prepared);
    let current = pending.request().clone();
    assert_eq!(current.bindings(), &[binding()]);
    let token = owner
        .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
        .expect("approve snapshot")
        .expect("token");
    let mut changed = binding();
    changed.path = "/other".into();
    *fixture.bindings.lock().expect("reader state") = Ok(vec![changed]);
    failure(
        owner.consume(&fixture.store, token, &current, &prepared),
        CommandScopeError::StaleAuthority,
    );
    assert_eq!(fixture.state("dispatch-binding"), "failed");
}

#[test]
fn scope_requires_current_live_fence_and_original_approved_bash_call() {
    for variant in 0..8 {
        let fixture = Fixture::new("execute", "confirm");
        let owner = fixture.owner();
        let prepared = fixture.call("durable", "printf fixture");
        let pending = fixture.pending(&owner, "durable", &prepared);
        let current = pending.request().clone();
        let token = owner
            .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
            .expect("approval")
            .expect("token");
        let conn = fixture.store.conn();
        match variant {
            0 => {
                conn.execute("UPDATE run_permission_fences SET permission_fence=json_set(permission_fence,'$.permission_revision',12)", []).expect("changed durable fence");
            }
            1 => {
                conn.execute(
                    "UPDATE permission_contract_state SET authority_epoch=?1",
                    [ulid::Ulid::generate().to_string()],
                )
                .expect("changed epoch");
            }
            2 => {
                conn.execute(
                    "UPDATE messages SET status='interrupted' WHERE id=?1",
                    [&fixture.accepted.assistant_message_id],
                )
                .expect("run ended");
            }
            3 => {
                conn.execute(
                    "UPDATE tool_calls SET input_json=?1 WHERE id='durable'",
                    [r#"{"command":"printf replaced","timeout":17}"#],
                )
                .expect("input changed");
            }
            4 => {
                conn.execute(
                    "UPDATE tool_calls SET approval='once' WHERE id='durable'",
                    [],
                )
                .expect("legacy audit cannot authorize scope");
            }
            5 => {
                conn.execute(
                    "UPDATE tool_calls SET status='cancelled' WHERE id='durable'",
                    [],
                )
                .expect("tool cancelled");
            }
            6 => {
                conn.execute("UPDATE tool_calls SET tool='write' WHERE id='durable'", [])
                    .expect("tool replaced");
            }
            7 => {
                conn.execute(
                    "UPDATE command_scope_audits SET request='{}' WHERE call_id='durable'",
                    [],
                )
                .expect("audit replaced");
            }
            _ => unreachable!(),
        }
        let expected = match variant {
            0..=4 => CommandScopeError::StaleAuthority,
            5..=6 => CommandScopeError::UnapprovedCall,
            _ => CommandScopeError::Conflict,
        };
        failure(
            owner.consume(&fixture.store, token, &current, &prepared),
            expected,
        );
        assert_ne!(fixture.state("durable"), "consumed");
    }
}

#[test]
fn scope_preserves_readonly_danger_and_unsupported_directory_gates() {
    for (mode, permission) in [
        ("ask", "confirm"),
        ("plan", "auto"),
        ("execute", "readonly"),
        ("execute", "confirm"),
        ("execute", "auto"),
    ] {
        let fixture = Fixture::new(mode, permission);
        let owner = fixture.owner();
        let prepared = fixture.call("normal", "printf fixture");
        let directory = RequestedCommandScope::from_json(
            r#"{"write_roots":["/tmp"],"reason":"write outside"}"#,
        )
        .expect("lexical request only");
        failure(
            owner.request(
                &fixture.store,
                "normal",
                &prepared,
                directory,
                Duration::from_secs(30),
            ),
            CommandScopeError::ScopeUnavailable,
        );
        let pending = fixture.pending(&owner, "normal", &prepared);
        let current = pending.request().clone();
        let token = owner
            .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
            .expect("explicit network approval")
            .expect("token");
        owner
            .consume(&fixture.store, token, &current, &prepared)
            .expect("network receipt only");
        let danger = fixture.call("danger", "rm -rf /");
        failure(
            owner.request(
                &fixture.store,
                "danger",
                &danger,
                network(),
                Duration::from_secs(30),
            ),
            CommandScopeError::UnapprovedCall,
        );
        assert!(fixture.tools.prepare_bash_json(r#"{"command":"printf fixture","requested_scope":{"network":true,"reason":"not wired"}}"#).is_err());
    }
}

#[test]
fn scope_storage_failure_and_cancellation_after_approval_never_return_a_dispatch_receipt() {
    let fixture = Fixture::new("execute", "confirm");
    let owner = fixture.owner();
    let prepared = fixture.call("storage", "printf fixture");
    let pending = fixture.pending(&owner, "storage", &prepared);
    let current = pending.request().clone();
    let token = owner
        .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
        .expect("approval")
        .expect("token");
    fixture.store.conn().execute_batch("CREATE TRIGGER reject_scope_consume BEFORE UPDATE ON command_scope_audits WHEN NEW.state='consumed' BEGIN SELECT RAISE(ABORT,'synthetic persistence failure'); END;").expect("synthetic write fault");
    failure(
        owner.consume(&fixture.store, token, &current, &prepared),
        CommandScopeError::Unavailable,
    );
    assert_eq!(fixture.state("storage"), "failed");
    let prepared = fixture.call("cancel-after", "printf fixture");
    let pending = fixture.pending(&owner, "cancel-after", &prepared);
    let current = pending.request().clone();
    let token = owner
        .decide(&fixture.store, pending, CommandScopeDecision::AllowOnce)
        .expect("approval")
        .expect("token");
    fixture.cancel.cancel();
    failure(
        owner.consume(&fixture.store, token, &current, &prepared),
        CommandScopeError::Cancelled,
    );
    assert_eq!(fixture.state("cancel-after"), "cancelled");
}
