use super::*;
use serde_json::json;

fn fixture() -> Store {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    store.conn().execute_batch("INSERT INTO threads(id,model,created_at,updated_at) VALUES ('thread','model',1,1),('other','model',1,1);").unwrap();
    store
}
fn create(store: &Store, text: &str) -> Vec<AutomationTask> {
    command(
        store,
        "thread",
        &parse_command(text).unwrap().unwrap(),
        1000,
    )
    .unwrap()
}
fn report(store: &Store, claim: &AutomationRunClaim, value: serde_json::Value) {
    runtime::call_in_transaction(store.conn(), claim, "update_goal", value).unwrap();
}
fn usage(store: &Store, claim: &AutomationRunClaim, id: i64, input: u64, output: u64) {
    record_usage(store.conn(), claim, id, input, output).unwrap();
}
fn run(store: &Store, kind: AutomationKind, now: i64) -> AutomationRunClaim {
    claim(store, "thread", Some(kind), now).unwrap().unwrap()
}

#[test]
fn explicit_creation_and_claims_are_atomic_and_single_flight() {
    let store = fixture();
    assert!(load(&store, "thread").unwrap().is_empty());
    assert!(claim(&store, "thread", None, 1000).unwrap().is_none());
    create(&store, "/goal --budget 1000 ship changes");
    assert!(
        command(
            &store,
            "thread",
            &parse_command("/goal replace").unwrap().unwrap(),
            1000
        )
        .is_err()
    );
    assert_eq!(
        load(&store, "thread").unwrap()[0].instruction,
        "ship changes"
    );
    let current = run(&store, AutomationKind::Goal, 1000);
    assert!(
        claim(&store, "thread", Some(AutomationKind::Goal), 1000)
            .unwrap()
            .is_none()
    );
    assert!(current_claim(&store, &current).unwrap());
    assert!(
        runtime::call_in_transaction(
            store.conn(),
            &AutomationRunClaim {
                thread_id: "other".into(),
                ..current.clone()
            },
            "get_goal",
            json!({})
        )
        .is_err()
    );
}

#[test]
fn progress_and_completion_only_apply_after_success_and_once() {
    let store = fixture();
    create(&store, "/goal deliver");
    let first = run(&store, AutomationKind::Goal, 1000);
    report(
        &store,
        &first,
        json!({"status":"active","progress":"progress","reason":"Updated the implementation"}),
    );
    assert!(
        runtime::call_in_transaction(
            store.conn(),
            &first,
            "update_goal",
            json!({"status":"complete","reason":"Repeated"})
        )
        .is_err()
    );
    usage(&store, &first, 1, 10, 2);
    usage(&store, &first, 1, 10, 2);
    finish(&store, &first, true, true, 50, 2000).unwrap();
    finish(&store, &first, true, true, 50, 2000).unwrap();
    let tasks = load(&store, "thread").unwrap();
    assert_eq!((tasks[0].used_tokens, tasks[0].elapsed_ms), (12, 50));
    assert_eq!(due_kind(&tasks, 2999), None);
    let next = run(&store, AutomationKind::Goal, 3000);
    report(
        &store,
        &next,
        json!({"status":"complete","reason":"Verified all requested outputs"}),
    );
    assert_eq!(
        load(&store, "thread").unwrap()[0].status,
        AutomationStatus::Active
    );
    finish(&store, &next, true, true, 10, 4000).unwrap();
    assert_eq!(
        load(&store, "thread").unwrap()[0].status,
        AutomationStatus::Complete
    );
    assert_eq!(due_kind(&load(&store, "thread").unwrap(), 99999), None);
}

#[test]
fn failure_never_applies_completion_and_resume_resets_streaks() {
    let store = fixture();
    create(&store, "/goal deliver");
    let first = run(&store, AutomationKind::Goal, 1000);
    report(
        &store,
        &first,
        json!({"status":"complete","reason":"Verified artifacts"}),
    );
    finish(&store, &first, false, true, 1, 2000).unwrap();
    assert_eq!(
        load(&store, "thread").unwrap()[0].status,
        AutomationStatus::Paused
    );
    command(
        &store,
        "thread",
        &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Resume),
        3000,
    )
    .unwrap();
    for (index, blocker) in ["a", "a", "b", "b", "b"].iter().enumerate() {
        let now = 3000 + index as i64 * 2000;
        let current = run(&store, AutomationKind::Goal, now);
        report(
            &store,
            &current,
            json!({"status":"blocked","reason":"Dependency checked unavailable","blocker":blocker}),
        );
        finish(&store, &current, true, true, 1, now + 100).unwrap();
        assert_eq!(
            load(&store, "thread").unwrap()[0].status,
            if index == 4 {
                AutomationStatus::Blocked
            } else {
                AutomationStatus::Active
            }
        );
    }
    let tasks = command(
        &store,
        "thread",
        &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Resume),
        20000,
    )
    .unwrap();
    assert_eq!(tasks[0].blocker_count, 0);
    assert_eq!(tasks[0].no_progress, 0);
}

#[test]
fn three_missing_reports_block_and_progress_clears_streak() {
    let store = fixture();
    create(&store, "/goal deliver");
    for index in 0..2 {
        let now = 1000 + index * 2000;
        let current = run(&store, AutomationKind::Goal, now);
        finish(&store, &current, true, true, 1, now + 100).unwrap();
    }
    let current = run(&store, AutomationKind::Goal, 5000);
    report(
        &store,
        &current,
        json!({"status":"active","progress":"waiting","reason":"Build job 42 is still running, verified now"}),
    );
    finish(&store, &current, true, true, 1, 5100).unwrap();
    assert_eq!(load(&store, "thread").unwrap()[0].no_progress, 0);
    for index in 0..3 {
        let now = 7000 + index * 2000;
        let current = run(&store, AutomationKind::Goal, now);
        finish(&store, &current, true, true, 1, now + 100).unwrap();
    }
    assert_eq!(
        load(&store, "thread").unwrap()[0].status,
        AutomationStatus::Blocked
    );
}

#[test]
fn budget_keeps_inflight_usage_after_pause_and_rejects_bypass() {
    let store = fixture();
    create(&store, "/goal --budget 20 deliver");
    let current = run(&store, AutomationKind::Goal, 1000);
    command(
        &store,
        "thread",
        &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Pause),
        1200,
    )
    .unwrap();
    command(
        &store,
        "thread",
        &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Resume),
        1300,
    )
    .unwrap();
    assert!(!current_claim(&store, &current).unwrap());
    assert!(
        runtime::call_in_transaction(
            store.conn(),
            &current,
            "update_goal",
            json!({"status":"complete","reason":"stale"})
        )
        .is_err()
    );
    usage(&store, &current, 1, 18, 5);
    finish(&store, &current, false, true, 25, 1400).unwrap();
    let task = &load(&store, "thread").unwrap()[0];
    assert_eq!(task.used_tokens, 23);
    assert_eq!(task.status, AutomationStatus::BudgetLimited);
    assert!(!task.can_control());
    assert!(
        command(
            &store,
            "thread",
            &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Resume),
            1500
        )
        .is_err()
    );
}

#[test]
fn clear_recreate_discards_stale_reports_and_metering() {
    let store = fixture();
    create(&store, "/goal old");
    let current = run(&store, AutomationKind::Goal, 1000);
    command(
        &store,
        "thread",
        &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Clear),
        1100,
    )
    .unwrap();
    let new = create(&store, "/goal new");
    usage(&store, &current, 1, 50, 10);
    finish(&store, &current, true, true, 10, 1200).unwrap();
    assert_eq!(load(&store, "thread").unwrap(), new);
}

#[test]
fn loops_use_finish_time_expire_and_never_catch_up() {
    let store = fixture();
    create(&store, "/loop 5m poll");
    create(&store, "/goal deliver");
    let current = run(&store, AutomationKind::Loop, 1000);
    assert!(
        runtime::call_in_transaction(
            store.conn(),
            &current,
            "schedule_wakeup",
            json!({"delay_seconds":60})
        )
        .is_err()
    );
    finish(&store, &current, true, true, 100, 600000).unwrap();
    let tasks = load(&store, "thread").unwrap();
    assert_eq!(
        tasks
            .iter()
            .find(|task| task.kind == AutomationKind::Loop)
            .unwrap()
            .next_due_ms,
        900000
    );
    assert_eq!(due_kind(&tasks, 600001), None);
    assert_eq!(due_kind(&tasks, 601000), Some(AutomationKind::Goal));
    let loop_task = refresh(&store, "thread", LOOP_LIFETIME_MS + 1000)
        .unwrap()
        .into_iter()
        .find(|task| task.kind == AutomationKind::Loop)
        .unwrap();
    assert_eq!(loop_task.status, AutomationStatus::Expired);
    let tasks = command(
        &store,
        "thread",
        &AutomationCommand::Control(AutomationKind::Loop, AutomationControl::Resume),
        LOOP_LIFETIME_MS + 2000,
    )
    .unwrap();
    assert_eq!(
        tasks
            .iter()
            .find(|task| task.kind == AutomationKind::Loop)
            .unwrap()
            .expires_at_ms,
        Some(2 * LOOP_LIFETIME_MS + 2000)
    );
}

#[test]
fn dynamic_wakeup_is_scoped_and_stop_preserves_goal() {
    let store = fixture();
    create(&store, "/loop poll");
    create(&store, "/goal deliver");
    let current = run(&store, AutomationKind::Loop, 1000);
    for value in [
        json!({"delay_seconds":59}),
        json!({"delay_seconds":3601}),
        json!({"delay_seconds":60.5}),
        json!({"delay_seconds":-1}),
    ] {
        assert!(
            runtime::call_in_transaction(store.conn(), &current, "schedule_wakeup", value).is_err()
        );
    }
    runtime::call_in_transaction(
        store.conn(),
        &current,
        "schedule_wakeup",
        json!({"delay_seconds":60,"noop":true}),
    )
    .unwrap();
    finish(&store, &current, true, true, 100, 2000).unwrap();
    assert_eq!(
        load(&store, "thread")
            .unwrap()
            .iter()
            .find(|task| task.kind == AutomationKind::Loop)
            .unwrap()
            .next_due_ms,
        62000
    );
    let user = claim(&store, "thread", None, 3000).unwrap().unwrap();
    assert!(
        runtime::call_in_transaction(store.conn(), &user, "schedule_wakeup", json!({"stop":true}))
            .is_err()
    );
    release_claim(&store, &user).unwrap();
    let next = run(&store, AutomationKind::Loop, 62000);
    runtime::call_in_transaction(store.conn(), &next, "schedule_wakeup", json!({"stop":true}))
        .unwrap();
    finish(&store, &next, true, true, 100, 63000).unwrap();
    assert_eq!(
        load(&store, "thread")
            .unwrap()
            .iter()
            .find(|task| task.kind == AutomationKind::Loop)
            .unwrap()
            .status,
        AutomationStatus::Stopped
    );
    assert!(
        load(&store, "thread")
            .unwrap()
            .iter()
            .any(|task| task.kind == AutomationKind::Goal)
    );
}

#[test]
fn restart_reconciles_claim_once_and_open_does_not_pause() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("automation.db");
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    store
        .conn()
        .execute_batch(
            "INSERT INTO threads(id,model,created_at,updated_at) VALUES ('thread','model',1,1);",
        )
        .unwrap();
    create(&store, "/goal --budget 100 deliver");
    let current = run(&store, AutomationKind::Goal, 1000);
    usage(&store, &current, 1, 7, 3);
    let second = Store::open(&path).unwrap();
    second.migrate().unwrap();
    assert_eq!(
        load(&second, "thread").unwrap()[0].status,
        AutomationStatus::Active
    );
    recover_startup(&second).unwrap();
    recover_startup(&second).unwrap();
    let task = &load(&second, "thread").unwrap()[0];
    assert_eq!(task.status, AutomationStatus::Paused);
    assert_eq!(task.used_tokens, 10);
    assert!(!task.usage_complete);
    assert!(db::active_run(second.conn(), "thread").unwrap().is_none());
    assert!(!current_claim(&second, &current).unwrap());
}

#[test]
fn archive_both_entrypoints_revokes_and_delete_cascades() {
    for typed_update in [false, true] {
        let store = fixture();
        create(&store, "/goal deliver");
        let current = run(&store, AutomationKind::Goal, 1000);
        if typed_update {
            crate::threads::update_thread(
                &store,
                "thread",
                &ThreadUpdate {
                    status: Some(ThreadStatus::Archived),
                    ..ThreadUpdate::default()
                },
            )
            .unwrap();
        } else {
            crate::threads::set_thread_status(&store, "thread", ThreadStatus::Archived).unwrap();
        }
        assert_eq!(
            load(&store, "thread").unwrap()[0].status,
            AutomationStatus::Paused
        );
        assert!(!current_claim(&store, &current).unwrap());
        crate::threads::delete_thread(&store, "thread").unwrap();
        assert!(load(&store, "thread").unwrap().is_empty());
        assert!(
            db::run(store.conn(), "thread", &current.id)
                .unwrap()
                .is_none()
        );
        finish(&store, &current, true, true, 1, 2000).unwrap();
    }
}

#[test]
fn pending_automatic_claim_yields_to_user_but_started_claim_does_not() {
    let store = fixture();
    create(&store, "/goal deliver");
    let pending = run(&store, AutomationKind::Goal, 1000);
    let user = claim(&store, "thread", None, 1000).unwrap().unwrap();
    assert!(activate_claim(&store, &pending).unwrap().is_none());
    assert!(current_claim(&store, &user).unwrap());
    finish(&store, &pending, false, true, 0, 1000).unwrap();
    assert_eq!(
        load(&store, "thread").unwrap()[0].status,
        AutomationStatus::Active
    );
    release_claim(&store, &user).unwrap();
    let started = run(&store, AutomationKind::Goal, 1000);
    assert!(activate_claim(&store, &started).unwrap().is_some());
    assert!(activate_claim(&store, &started).unwrap().is_none());
    assert!(claim(&store, "thread", None, 1000).is_err());
}

#[test]
fn activation_removes_revoked_secondary_task_from_context_tools_and_metering() {
    use vega_runtime::agent::automation::RuntimeAutomationHook;

    let store = fixture();
    create(&store, "/goal --budget 100 old objective");
    create(&store, "/loop 1m check");
    let pending = run(&store, AutomationKind::Loop, 1000);
    assert_eq!(pending.tasks.len(), 2);
    command(
        &store,
        "thread",
        &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Pause),
        1000,
    )
    .unwrap();
    let activated = activate_claim(&store, &pending).unwrap().unwrap();
    assert_eq!(activated.tasks.len(), 1);
    assert_eq!(activated.tasks[0].kind, AutomationKind::Loop);
    let binding = AutomationBinding::new(std::path::PathBuf::new(), activated.clone());
    assert!(binding.context()["goal"].is_null());
    assert!(runtime::call_in_transaction(store.conn(), &pending, "get_goal", json!({})).is_err());
    usage(&store, &activated, 1, 7, 3);
    finish(&store, &pending, true, true, 5, 2000).unwrap();
    let goal = load(&store, "thread")
        .unwrap()
        .into_iter()
        .find(|task| task.kind == AutomationKind::Goal)
        .unwrap();
    assert_eq!(goal.status, AutomationStatus::Paused);
    assert_eq!(goal.used_tokens, 0);
    assert_eq!(goal.elapsed_ms, 0);
}

#[test]
fn unsettled_started_run_keeps_usage_unknown_but_preflight_failure_can_resume() {
    for started in [false, true] {
        let store = fixture();
        create(&store, "/goal --budget 100 verify");
        let pending = run(&store, AutomationKind::Goal, 1000);
        if started {
            activate_claim(&store, &pending).unwrap().unwrap();
            usage(&store, &pending, 1, 7, 3);
        }
        finish_unsettled_claim(&store, &pending, 2000).unwrap();
        finish_unsettled_claim(&store, &pending, 2000).unwrap();
        let task = &load(&store, "thread").unwrap()[0];
        assert_eq!(task.status, AutomationStatus::Paused);
        assert_eq!(task.used_tokens, if started { 10 } else { 0 });
        assert_eq!(task.usage_complete, !started);
        assert_eq!(
            command(
                &store,
                "thread",
                &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Resume),
                2000,
            )
            .is_ok(),
            !started
        );
    }
}

#[test]
fn current_meter_includes_only_current_ledger_and_old_failure_does_not_pause_new_identity() {
    let store = fixture();
    let snapshot = create(&store, "/goal --budget 100 old");
    let current = run(&store, AutomationKind::Goal, 1000);
    usage(&store, &current, 1, 7, 3);
    let result =
        runtime::call_in_transaction(store.conn(), &current, "get_goal", json!({})).unwrap();
    assert_eq!(result["goal"]["used_tokens"], 10);
    assert_eq!(result["remaining_tokens"], 90);
    assert_eq!(load(&store, "thread").unwrap()[0].used_tokens, 0);
    finish(&store, &current, false, true, 1, 1100).unwrap();
    assert_eq!(load(&store, "thread").unwrap()[0].used_tokens, 10);
    command(
        &store,
        "thread",
        &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Clear),
        1200,
    )
    .unwrap();
    let new = create(&store, "/goal new");
    pause_matching(&store, "thread", &snapshot, "old failure").unwrap();
    assert_eq!(load(&store, "thread").unwrap(), new);
}

#[test]
fn queued_input_and_pending_plan_prevent_automatic_claim() {
    let store = fixture();
    create(&store, "/loop 1m poll");
    crate::pending_messages::enqueue(
        &store,
        &PendingMessage {
            id: "queued".into(),
            thread_id: "thread".into(),
            generation: "prior".into(),
            behavior: PendingMessageBehavior::Queue,
            content: "user first".into(),
            images: Vec::new(),
        },
    )
    .unwrap();
    assert!(
        claim(&store, "thread", Some(AutomationKind::Loop), 1000)
            .unwrap()
            .is_none()
    );
    store
        .conn()
        .execute("DELETE FROM pending_messages", [])
        .unwrap();
    store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at,plan_status) VALUES ('plan','thread',1,'assistant','text','review','done',1,'pending')",[]).unwrap();
    assert!(
        claim(&store, "thread", Some(AutomationKind::Loop), 1000)
            .unwrap()
            .is_none()
    );
    assert!(claim(&store, "thread", None, 1000).unwrap().is_some());
}

#[test]
fn invalid_draft_command_rolls_back_thread_materialization() {
    let store = fixture();
    let draft = crate::threads::draft_thread(None, "model", "confirm");
    let invalid = AutomationCommand::Create {
        kind: AutomationKind::Goal,
        instruction: String::new(),
        token_budget: None,
        interval_seconds: None,
    };
    assert!(command_for_draft(&store, &draft, &invalid, 1000).is_err());
    assert!(
        vega_store::threads::find(store.conn(), &draft.id)
            .unwrap()
            .is_none()
    );
    let valid = parse_command("/goal deliver").unwrap().unwrap();
    assert_eq!(
        command_for_draft(&store, &draft, &valid, 1000)
            .unwrap()
            .len(),
        1
    );
    assert!(
        vega_store::threads::find(store.conn(), &draft.id)
            .unwrap()
            .is_some()
    );
}
