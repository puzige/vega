use std::path::PathBuf;

use vega_runtime::{ChatMessage, ChatRole, ReasoningReplayPolicy, ReplayFrame};
use vega_store::{
    Store, config, messages, permission_reset as persistence, thread_admission, tool_calls,
};

use super::{
    pause_admission, prepare_reset, recover_pending, recover_pending_with_pause, reset_quiescent,
};
use crate::agent::{ReplayCheckpointFence, ReplaySessionRegistry};
use crate::permission_contracts::{
    DefaultPermissionService, capture_fence, change_thread_permission,
};
use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, AutomationCommand, AutomationKind,
    AutomationPermissionAuthorization as AutomationGrant, AutomationStatus,
    DefaultPermissionValues, OrdinarySubmission, OrdinaryWorker, PermissionMode,
    PermissionResetError, RawSubmission, RetryBlocker, RetryEligibility, SubmissionFailure, Thread,
};
use crate::{automation, ordinary_submissions, threads};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
    store: Store,
    thread: Thread,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-reset-initial-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("isolated directory");
        let root = root.canonicalize().expect("canonical fixture");
        let config = root.join("config.toml");
        config::AppConfig::default()
            .save_to(&config)
            .expect("isolated configuration");
        let database = root.join("vega.db");
        let store = Store::open(&database).expect("isolated database");
        store.migrate().expect("fixture migrations");
        let thread = threads::materialize_draft(
            &store,
            &threads::draft_thread(None, "synthetic", "confirm"),
        )
        .expect("fixture thread");
        let service = DefaultPermissionService::new(database, config.clone());
        let values =
            DefaultPermissionValues::from(&config::read_from(&config).expect("defaults").defaults);
        let change = service
            .prepare(&values, PermissionMode::FullAccess)
            .expect("default confirmation intent");
        service.save(&change, true).expect("confirmed default");
        let thread =
            change_thread_permission(&store, &thread.id, 0, PermissionMode::FullAccess, true, 1)
                .expect("confirmed thread");
        Self {
            root,
            config,
            store,
            thread,
        }
    }

    fn submission(&self) -> (OrdinarySubmission, OrdinaryWorker) {
        let admission =
            thread_admission::try_acquire(self.store.database_identity(), &self.thread.id)
                .expect("admission registry")
                .expect("ordinary owner");
        OrdinarySubmission::begin(
            self.store.database_identity(),
            self.thread.id.clone(),
            RawSubmission::new(
                "retained input".into(),
                Vec::new(),
                1,
                "synthetic".into(),
                None,
                None,
            ),
            admission,
        )
        .expect("ordinary submission")
    }

    fn failed_submission(&self) -> OrdinarySubmission {
        let (submission, worker) = self.submission();
        let attempt = worker.attempt_id();
        worker
            .complete(Err(SubmissionFailure::PreparationFailed))
            .expect("settled failure");
        submission.seal(attempt).expect("sealed failure");
        assert_eq!(submission.snapshot().retry, RetryEligibility::Uncommitted);
        submission
    }

    fn task(&self, kind: AutomationKind) {
        automation::command(
            &self.store,
            &self.thread.id,
            &AutomationCommand::Create {
                kind,
                instruction: "retained automation instruction".into(),
                token_budget: None,
                interval_seconds: (kind == AutomationKind::Loop).then_some(60),
            },
            2,
        )
        .expect("automation task");
        let task = automation::load(&self.store, &self.thread.id)
            .expect("tasks")
            .into_iter()
            .find(|task| task.kind == kind)
            .expect("requested task");
        let row = vega_store::threads::find(self.store.conn(), &self.thread.id)
            .expect("thread lookup")
            .expect("thread");
        automation::authorize_full_access(
            &self.store,
            &AutomationGrant {
                kind,
                identity: task.identity,
                revision: task.revision,
                fence: capture_fence(&self.store, &row).expect("thread authority"),
            },
            true,
            3,
        )
        .expect("automation confirmation");
    }

    fn begin_pending(&self) -> persistence::ResetRecord {
        let identity = super::configuration_identity(&self.config).expect("config identity");
        let source = super::config_source(
            &identity,
            &config::read_from(&self.config).expect("config source"),
        );
        let state =
            vega_store::permission_contracts::load_state(self.store.conn()).expect("authority");
        let tx = self
            .store
            .immediate_transaction()
            .expect("reset transaction");
        let record =
            persistence::begin(&tx, &state.authority_epoch, &source, 5).expect("pending reset");
        tx.commit().expect("commit pending");
        record
    }

    fn tool(&self, name: &str, rejected: bool) -> tool_calls::ToolCallState {
        messages::insert(
            self.store.conn(),
            &messages::MessageRow {
                id: "assistant".into(),
                thread_id: self.thread.id.clone(),
                seq: 1,
                role: "assistant".into(),
                kind: "text".into(),
                content: "retained history".into(),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("assistant history");
        tool_calls::insert_pending(
            self.store.conn(),
            tool_calls::NewToolCall {
                id: "call",
                thread_id: &self.thread.id,
                message_id: "assistant",
                seq: 1,
                tool: name,
                input_json: if name == "write" {
                    r#"{"path":"synthetic.txt","content":"synthetic"}"#
                } else {
                    r#"{"command":"printf synthetic","timeout":null}"#
                },
                status: "pending_approval",
                created_at: 1,
            },
        )
        .expect("pending call");
        let audit = ApprovalAudit {
            decision: if rejected {
                Approval::Deny
            } else {
                Approval::Once
            },
            note: None,
            source: ApprovalSource::User,
            danger: None,
        }
        .to_json()
        .expect("typed approval");
        if rejected {
            tool_calls::reject(self.store.conn(), "call", &audit, "denied", 2, None)
                .expect("rejected before execution");
        } else {
            tool_calls::approve(self.store.conn(), "call", &audit, None, 2).expect("approved call");
            tool_calls::mark_running(self.store.conn(), "call").expect("running call");
            tool_calls::finish(
                self.store.conn(),
                tool_calls::FinishToolCall {
                    id: "call",
                    status: "success",
                    output_text: "synthetic result",
                    exit_code: (name == "bash").then_some(0),
                    duration_ms: Some(1),
                    finished_at: 3,
                },
            )
            .expect("terminal evidence without native execution");
        }
        tool_calls::find_state(self.store.conn(), "call")
            .expect("tool state")
            .expect("retained call")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn seed_replay(registry: &ReplaySessionRegistry, thread: &str) {
    let lease = registry.begin(
        thread,
        1,
        "source",
        ReplayCheckpointFence::from_checkpoint(None),
    );
    let mut frame = ReplayFrame::new(1, ReasoningReplayPolicy::Unknown);
    frame.bind_environment([7; 32]);
    let mut system = ChatMessage::new(ChatRole::System, "retained system");
    system.attach_replay_frame(frame);
    assert!(registry.seal(
        lease,
        1,
        "source".into(),
        ReplayCheckpointFence::from_checkpoint(None),
        "owner".into(),
        vec![
            system,
            ChatMessage::new(ChatRole::Assistant, "retained answer")
        ]
    ));
}

#[test]
fn quiescent_reset_revokes_authority_and_memory_without_releasing_the_callers_pause() {
    let fixture = Fixture::new();
    fixture.task(AutomationKind::Goal);
    let original = fixture.failed_submission();
    let attempt = original.snapshot().attempts[0].id;
    let original_tasks =
        automation::load(&fixture.store, &fixture.thread.id).expect("original tasks");
    let original_config = config::read_from(&fixture.config).expect("original config");
    let intent = prepare_reset(&fixture.store, &fixture.config).expect("reset intent");
    assert!(
        persistence::load(fixture.store.conn())
            .expect("no journal")
            .is_none()
    );
    assert!(
        !thread_admission::is_database_paused(fixture.store.database_identity())
            .expect("not paused by prepare")
    );
    config::update_from(&fixture.config, |value| value.ui.theme = "light".into())
        .expect("unrelated current preference");
    let replay = ReplaySessionRegistry::new();
    seed_replay(&replay, &fixture.thread.id);
    let candidate = replay.begin(
        &fixture.thread.id,
        1,
        "source",
        ReplayCheckpointFence::from_checkpoint(None),
    );
    assert!(candidate.candidate().is_some());
    drop(candidate);
    seed_replay(&replay, &fixture.thread.id);
    let pause = pause_admission(&fixture.store).expect("held reset barrier");
    let receipt = reset_quiescent(&fixture.store, &fixture.config, &intent, &pause, &replay)
        .expect("quiescent reset");
    assert_ne!(receipt.authority_epoch, intent.authority_epoch);
    let state =
        vega_store::permission_contracts::load_state(fixture.store.conn()).expect("reset state");
    assert!(!state.reset_pending);
    assert_eq!(state.authority_epoch, receipt.authority_epoch);
    let saved = config::read_from(&fixture.config).expect("saved config");
    assert_eq!(saved.defaults.permission_mode, "confirm");
    assert!(saved.defaults.full_access_grant.is_none());
    assert_eq!(
        saved.defaults.permission_revision,
        original_config.defaults.permission_revision + 1
    );
    assert_eq!(saved.ui.theme, "light");
    let thread = threads::get_thread(&fixture.store, &fixture.thread.id).expect("reset thread");
    assert_eq!(thread.permission_mode, PermissionMode::Confirm);
    assert!(thread.full_access_grant.is_none());
    assert_eq!(
        thread.permission_revision,
        fixture.thread.permission_revision + 1
    );
    let tasks = automation::load(&fixture.store, &fixture.thread.id).expect("reset tasks");
    assert_eq!(tasks[0].status, AutomationStatus::Paused);
    assert!(tasks[0].permission_authorization.is_none());
    assert_eq!(tasks[0].revision, original_tasks[0].revision + 1);
    assert_eq!(tasks[0].instruction, original_tasks[0].instruction);
    assert_eq!(original.snapshot().raw.text(), "retained input");
    assert_eq!(
        original.snapshot().retry,
        RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated)
    );
    assert!(original.try_retry(attempt).is_err());
    assert!(
        replay
            .begin(
                &fixture.thread.id,
                1,
                "source",
                ReplayCheckpointFence::from_checkpoint(None)
            )
            .candidate()
            .is_none()
    );
    assert!(
        pause
            .verifies(fixture.store.database_identity())
            .expect("caller retains pause")
    );
    assert!(
        thread_admission::try_acquire(fixture.store.database_identity(), &fixture.thread.id)
            .expect("registry")
            .is_none()
    );
    let journal = persistence::load(fixture.store.conn())
        .expect("completed journal")
        .expect("record");
    assert!(journal.completed);
    assert_eq!(journal.id, receipt.reset_id);
    assert!(reset_quiescent(&fixture.store, &fixture.config, &intent, &pause, &replay).is_err());
    assert_eq!(
        recover_pending_with_pause(&fixture.store, &fixture.config, &pause, &replay),
        Ok(None)
    );
    assert_eq!(
        persistence::load(fixture.store.conn()).expect("same completion"),
        Some(journal)
    );
    drop(pause);
    fixture.failed_submission();
}

#[test]
fn reset_refuses_wrong_or_active_pause_before_mutating_authority() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let replay = ReplaySessionRegistry::new();
    let intent = prepare_reset(&fixture.store, &fixture.config).expect("intent");
    let before = std::fs::read(&fixture.config).expect("config bytes");
    let wrong = pause_admission(&other.store).expect("wrong database pause");
    assert_eq!(
        reset_quiescent(&fixture.store, &fixture.config, &intent, &wrong, &replay),
        Err(PermissionResetError::Busy)
    );
    assert_eq!(
        reset_quiescent(&other.store, &other.config, &intent, &wrong, &replay),
        Err(PermissionResetError::ConfigurationConflict)
    );
    let (submission, worker) = fixture.submission();
    let attempt = worker.attempt_id();
    let pause = pause_admission(&fixture.store).expect("pause existing owner");
    assert_eq!(
        reset_quiescent(&fixture.store, &fixture.config, &intent, &pause, &replay),
        Err(PermissionResetError::Busy)
    );
    assert!(matches!(
        prepare_reset(&fixture.store, &fixture.config),
        Err(PermissionResetError::Busy)
    ));
    assert_eq!(
        std::fs::read(&fixture.config).expect("unchanged config"),
        before
    );
    assert!(
        persistence::load(fixture.store.conn())
            .expect("no journal")
            .is_none()
    );
    worker
        .complete(Err(SubmissionFailure::Cancelled))
        .expect("finish owner");
    submission.seal(attempt).expect("release admission");
    reset_quiescent(&fixture.store, &fixture.config, &intent, &pause, &replay)
        .expect("same intent after real owner release");
}

#[test]
fn reset_intent_rejects_changed_missing_malformed_or_replaced_sources() {
    for fault in [
        "revision",
        "grant",
        "mode",
        "missing",
        "malformed",
        "path",
        "epoch",
    ] {
        let fixture = Fixture::new();
        let intent = prepare_reset(&fixture.store, &fixture.config).expect("intent");
        let mut path = fixture.config.clone();
        match fault {
            "revision" => {
                config::update_from(&path, |value| value.defaults.permission_revision += 1)
                    .expect("competing revision");
            }
            "grant" => {
                config::update_from(&path, |value| {
                    value.defaults.full_access_grant = Some("changed metadata".into())
                })
                .expect("competing grant");
            }
            "mode" => {
                config::update_from(&path, |value| {
                    value.defaults.permission_mode = "auto".into()
                })
                .expect("competing mode");
            }
            "missing" => std::fs::remove_file(&path).expect("remove fixture config"),
            "malformed" => std::fs::write(&path, "not valid toml [").expect("malformed fixture"),
            "path" => {
                path = fixture.root.join("replacement.toml");
                std::fs::copy(&fixture.config, &path).expect("other source");
            }
            "epoch" => {
                fixture
                    .store
                    .conn()
                    .execute(
                        "UPDATE permission_contract_state SET authority_epoch=?1",
                        [ulid::Ulid::generate().to_string()],
                    )
                    .expect("competing epoch");
            }
            _ => unreachable!(),
        }
        let before = std::fs::read(&path).ok();
        let epoch = vega_store::permission_contracts::load_state(fixture.store.conn())
            .expect("source epoch")
            .authority_epoch;
        let pause = pause_admission(&fixture.store).expect("pause");
        let expected = if matches!(fault, "missing" | "malformed") {
            PermissionResetError::ConfigurationUnavailable
        } else {
            PermissionResetError::ConfigurationConflict
        };
        assert_eq!(
            reset_quiescent(
                &fixture.store,
                &path,
                &intent,
                &pause,
                &ReplaySessionRegistry::new()
            ),
            Err(expected),
            "{fault}"
        );
        assert_eq!(std::fs::read(&path).ok(), before, "{fault}");
        assert!(
            persistence::load(fixture.store.conn())
                .expect("no journal")
                .is_none()
        );
        assert_eq!(
            vega_store::permission_contracts::load_state(fixture.store.conn())
                .expect("retained epoch")
                .authority_epoch,
            epoch
        );
        assert_eq!(
            threads::get_thread(&fixture.store, &fixture.thread.id).expect("retained thread"),
            fixture.thread
        );
    }
}

#[test]
fn automation_failure_rolls_back_initial_journal_epoch_and_revocations() {
    for overflow in [false, true] {
        let fixture = Fixture::new();
        fixture.task(AutomationKind::Goal);
        fixture.task(AutomationKind::Loop);
        if overflow {
            fixture.store.conn().execute("UPDATE automation_tasks SET revision=?1,data=json_set(data,'$.revision',?1) WHERE kind='loop'", [i64::MAX]).expect("revision capacity fault");
        } else {
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE automation_tasks SET revision=revision+1 WHERE kind='loop'",
                    [],
                )
                .expect("identity inconsistency");
        }
        let tasks =
            vega_store::automation::all_tasks(fixture.store.conn()).expect("original task blobs");
        let intent = prepare_reset(&fixture.store, &fixture.config).expect("intent");
        let before = std::fs::read(&fixture.config).expect("original config");
        let pause = pause_admission(&fixture.store).expect("pause");
        assert_eq!(
            reset_quiescent(
                &fixture.store,
                &fixture.config,
                &intent,
                &pause,
                &ReplaySessionRegistry::new()
            ),
            Err(PermissionResetError::Unavailable)
        );
        assert!(
            persistence::load(fixture.store.conn())
                .expect("rolled back journal")
                .is_none()
        );
        assert_eq!(
            vega_store::permission_contracts::load_state(fixture.store.conn())
                .expect("rolled back epoch")
                .authority_epoch,
            intent.authority_epoch
        );
        assert_eq!(
            vega_store::automation::all_tasks(fixture.store.conn()).expect("rolled back tasks"),
            tasks
        );
        assert_eq!(
            threads::get_thread(&fixture.store, &fixture.thread.id).expect("rolled back thread"),
            fixture.thread
        );
        assert_eq!(
            std::fs::read(&fixture.config).expect("retained config"),
            before
        );
    }
}

#[test]
fn configuration_save_failure_retains_pending_and_retry_completes_the_same_reset() {
    let fixture = Fixture::new();
    fixture.task(AutomationKind::Goal);
    let original = fixture.failed_submission();
    let original_tasks =
        automation::load(&fixture.store, &fixture.thread.id).expect("original tasks");
    let intent = prepare_reset(&fixture.store, &fixture.config).expect("intent");
    let before = std::fs::read(&fixture.config).expect("original bytes");
    let temporary_path = fixture.config.with_extension("toml.tmp");
    std::fs::create_dir(&temporary_path).expect("isolated publication fault");
    let replay = ReplaySessionRegistry::new();
    let pause = pause_admission(&fixture.store).expect("pause");
    assert_eq!(
        reset_quiescent(&fixture.store, &fixture.config, &intent, &pause, &replay),
        Err(PermissionResetError::OutcomeUnknown)
    );
    let pending = persistence::load(fixture.store.conn())
        .expect("pending journal")
        .expect("retained pending");
    assert!(!pending.completed);
    assert_ne!(pending.authority_epoch, intent.authority_epoch);
    assert_eq!(
        std::fs::read(&fixture.config).expect("source retained"),
        before
    );
    assert_eq!(
        original.snapshot().retry,
        RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated)
    );
    let revoked = automation::load(&fixture.store, &fixture.thread.id).expect("revoked tasks");
    assert_eq!(revoked[0].revision, original_tasks[0].revision + 1);
    assert!(revoked[0].permission_authorization.is_none());
    assert_eq!(
        recover_pending_with_pause(&fixture.store, &fixture.config, &pause, &replay),
        Err(PermissionResetError::OutcomeUnknown)
    );
    assert_eq!(
        persistence::load(fixture.store.conn()).expect("same pending"),
        Some(pending.clone())
    );
    std::fs::remove_dir(&temporary_path).expect("repair isolated publication fault");
    let receipt = recover_pending_with_pause(&fixture.store, &fixture.config, &pause, &replay)
        .expect("retry")
        .expect("completion");
    assert_eq!(receipt.reset_id, pending.id);
    assert_eq!(receipt.authority_epoch, pending.authority_epoch);
    assert_eq!(receipt.default_permission_revision, pending.target_revision);
    assert_eq!(
        automation::load(&fixture.store, &fixture.thread.id).expect("no repeated revision"),
        revoked
    );
    assert!(
        pause
            .verifies(fixture.store.database_identity())
            .expect("pause retained through retry")
    );
    drop(pause);
    assert_eq!(recover_pending(&fixture.store, &fixture.config), Ok(None));
}

#[test]
fn lost_ordinary_worker_blocks_initial_and_pending_reset_even_after_retry_invalidation() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let intent = prepare_reset(&fixture.store, &fixture.config).expect("intent");
    let (submission, worker) = fixture.submission();
    let attempt = worker.attempt_id();
    drop(worker);
    submission.seal(attempt).expect("seal lost worker evidence");
    let before = submission.snapshot();
    let config_before = std::fs::read(&fixture.config).expect("config source");
    let pause = pause_admission(&fixture.store).expect("pause");
    assert!(
        pause
            .owners_released()
            .expect("drained owner is not cleanup")
    );
    let replay = ReplaySessionRegistry::new();
    assert_eq!(
        reset_quiescent(&fixture.store, &fixture.config, &intent, &pause, &replay),
        Err(PermissionResetError::UnsettledWork)
    );
    assert!(
        persistence::load(fixture.store.conn())
            .expect("no journal")
            .is_none()
    );
    ordinary_submissions::invalidate_database_recovery(fixture.store.database_identity(), &pause)
        .expect("invalidate retry without resolving unknown effects");
    assert_eq!(
        reset_quiescent(&fixture.store, &fixture.config, &intent, &pause, &replay),
        Err(PermissionResetError::UnsettledWork)
    );
    drop(pause);
    let pending = fixture.begin_pending();
    assert_eq!(
        recover_pending(&fixture.store, &fixture.config),
        Err(PermissionResetError::UnsettledWork)
    );
    assert_eq!(
        persistence::load(fixture.store.conn()).expect("same pending"),
        Some(pending)
    );
    assert_eq!(
        std::fs::read(&fixture.config).expect("config untouched"),
        config_before
    );
    let after = submission.snapshot();
    assert_eq!(after.attempts[0].commit, before.attempts[0].commit);
    assert_eq!(after.attempts[0].cleanup, before.attempts[0].cleanup);
    assert_eq!(after.attempts[0].primary, before.attempts[0].primary);
    assert!(after.attempts[0].abnormal);
    let other_intent = prepare_reset(&other.store, &other.config).expect("other intent");
    let other_pause = pause_admission(&other.store).expect("other pause");
    reset_quiescent(
        &other.store,
        &other.config,
        &other_intent,
        &other_pause,
        &replay,
    )
    .expect("other database independent");
}

#[test]
fn consumed_scope_is_unsettled_without_cleanup_evidence_even_without_active_tools() {
    for state in [
        "consumed",
        "unknown",
        "pending",
        "approved",
        "denied",
        "timeout",
        "cancelled",
        "failed",
    ] {
        let fixture = Fixture::new();
        fixture.store.conn().execute(
            "INSERT INTO command_scope_audits(request_id,thread_id,run_id,call_id,request,state,created_at,updated_at) VALUES('scope',?1,'run','call','{}',?2,1,1)",
            [fixture.thread.id.as_str(), state],
        ).expect("isolated scope audit");
        let intent = prepare_reset(&fixture.store, &fixture.config).expect("reset intent");
        let pause = pause_admission(&fixture.store).expect("quiescent pause");
        let result = reset_quiescent(
            &fixture.store,
            &fixture.config,
            &intent,
            &pause,
            &ReplaySessionRegistry::new(),
        );
        if matches!(state, "consumed" | "unknown") {
            assert_eq!(result, Err(PermissionResetError::UnsettledWork));
            assert!(
                persistence::load(fixture.store.conn())
                    .expect("journal lookup")
                    .is_none()
            );
        } else {
            result.expect("unconsumed audit permits reset");
        }
        let retained: String = fixture
            .store
            .conn()
            .query_row(
                "SELECT state FROM command_scope_audits WHERE request_id='scope'",
                [],
                |row| row.get(0),
            )
            .expect("scope retained");
        assert_eq!(
            retained,
            if matches!(state, "pending" | "approved") {
                "cancelled"
            } else {
                state
            }
        );
    }
}

#[test]
fn terminal_external_success_is_not_cleanup_but_local_success_and_rejection_remain_usable() {
    for (tool, rejected, blocked) in [
        ("bash", false, true),
        ("external-tool", false, true),
        ("write", false, false),
        ("bash", true, false),
    ] {
        let fixture = Fixture::new();
        let before_tool = fixture.tool(tool, rejected);
        let before_message =
            messages::find(fixture.store.conn(), "assistant").expect("original message");
        let before_config = std::fs::read(&fixture.config).expect("config bytes");
        let intent = prepare_reset(&fixture.store, &fixture.config).expect("intent");
        let pause = pause_admission(&fixture.store).expect("pause");
        let result = reset_quiescent(
            &fixture.store,
            &fixture.config,
            &intent,
            &pause,
            &ReplaySessionRegistry::new(),
        );
        if blocked {
            assert_eq!(result, Err(PermissionResetError::UnsettledWork), "{tool}");
            assert!(
                persistence::load(fixture.store.conn())
                    .expect("no journal")
                    .is_none()
            );
            drop(pause);
            let pending = fixture.begin_pending();
            assert_eq!(
                recover_pending(&fixture.store, &fixture.config),
                Err(PermissionResetError::UnsettledWork)
            );
            assert_eq!(
                persistence::load(fixture.store.conn()).expect("preserved pending"),
                Some(pending)
            );
            assert_eq!(
                std::fs::read(&fixture.config).expect("config unchanged"),
                before_config
            );
        } else {
            result.expect("known local or unexecuted history permits reset");
        }
        assert_eq!(
            tool_calls::find_state(fixture.store.conn(), "call").expect("retained tool"),
            Some(before_tool)
        );
        assert_eq!(
            messages::find(fixture.store.conn(), "assistant").expect("retained message"),
            before_message
        );
    }
}
