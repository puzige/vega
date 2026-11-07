use super::*;

fn fixture() -> crate::Store {
    let store = crate::Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    store.conn().execute_batch("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('p','/workspace','project',0,0); INSERT INTO threads(id,project_id,model,created_at,updated_at) VALUES('t','p','model',0,0),('other','p','model',0,0); INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('r','t',1,'assistant','','streaming',0); INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,status,created_at) VALUES('c','t','r',1,'bash','{}','running',0);").unwrap();
    store
}

fn record_for(state: &str) -> ShellOperationRecord<'_> {
    ShellOperationRecord {
        operation_id: "op",
        thread_id: "t",
        run_id: "r",
        call_id: "c",
        generation: "generation",
        workspace: "/workspace",
        state,
        exit_code: None,
        error: None,
        duration_ms: None,
    }
}

#[test]
fn records_require_starting_authority_and_keep_terminal_identity() {
    let store = fixture();
    assert!(record(store.conn(), &record_for("running"), 1).is_err());
    let mut invalid = record_for("prepared");
    invalid.thread_id = "other";
    assert!(record(store.conn(), &invalid, 1).is_err());
    record(store.conn(), &record_for("prepared"), 1).unwrap();
    record(store.conn(), &record_for("running"), 2).unwrap();
    let mut done = record_for("succeeded");
    done.exit_code = Some(0);
    done.duration_ms = Some(15);
    record(store.conn(), &done, 3).unwrap();
    record(store.conn(), &done, 4).unwrap();
    assert!(record(store.conn(), &record_for("cancelled"), 5).is_err());
    let actual: (String, Option<i32>, Option<i64>) = store
        .conn()
        .query_row(
            "SELECT state,exit_code,duration_ms FROM shell_operations WHERE operation_id='op'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(actual, ("succeeded".into(), Some(0), Some(15)));
}

#[test]
fn command_presentation_tracks_terminal_identity_without_rewriting_original_receipt()
-> rusqlite::Result<()> {
    let store = fixture();
    store.conn().execute(
        "UPDATE tool_calls SET input_json=?1,output_text='original running receipt' WHERE id='c'",
        [r#"{"command":"printf done","wait_ms":0}"#],
    )?;
    record(store.conn(), &record_for("prepared"), 1)?;
    record(store.conn(), &record_for("running"), 2)?;
    let mut done = record_for("succeeded");
    done.exit_code = Some(0);
    done.duration_ms = Some(23);
    record_with_facts(store.conn(), &done, &settled_facts(false), 3)?;
    let commands = command_presentations(store.conn(), "/workspace")?;
    assert_eq!(commands.len(), 1);
    let command = &commands[0];
    assert_eq!(
        (
            &*command.operation_id,
            &*command.call_id,
            &*command.thread_id
        ),
        ("op", "c", "t")
    );
    assert_eq!(
        (&*command.state, command.exit_code, command.duration_ms),
        ("succeeded", Some(0), Some(23))
    );
    assert_eq!(
        command.input_json,
        r#"{"command":"printf done","wait_ms":0}"#
    );
    assert!(command_presentations(store.conn(), "/unrelated")?.is_empty());
    let receipt: String = store.conn().query_row(
        "SELECT output_text FROM tool_calls WHERE id='c'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(receipt, "original running receipt");
    assert!(recovery(store.conn(), "/workspace")?.items.is_empty());
    Ok(())
}

#[test]
fn recovery_preserves_unknown_outcome_without_replaying_or_rebinding() {
    let store = fixture();
    record(store.conn(), &record_for("prepared"), 1).unwrap();
    record(store.conn(), &record_for("running"), 2).unwrap();
    assert!(!has_unknown_workspace(store.conn(), "/workspace").unwrap());
    assert_eq!(recover_thread(store.conn(), "other", 3).unwrap(), 0);
    assert_eq!(recover_thread(store.conn(), "t", 3).unwrap(), 1);
    assert_eq!(recover_thread(store.conn(), "t", 4).unwrap(), 0);
    assert!(has_unknown_workspace(store.conn(), "/workspace").unwrap());
    assert!(!has_unknown_workspace(store.conn(), "/unrelated").unwrap());
    assert!(record(store.conn(), &record_for("running"), 5).is_err());
    let actual: (String, String, Option<i32>) = store
        .conn()
        .query_row(
            "SELECT state,error,exit_code FROM shell_operations WHERE operation_id='op'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        actual,
        ("outcome_unknown".into(), "session_expired".into(), None)
    );
}

fn settled_facts(incomplete: bool) -> ShellFacts {
    ShellFacts {
        command_result: Some("exited".into()),
        cleanup_state: if incomplete {
            "unresolved"
        } else {
            "best_effort_completed"
        }
        .into(),
        output_incomplete: incomplete,
        resources_closed: true,
        process_resolved: true,
        cleanup_reasons: if incomplete {
            vec!["output_incomplete".into()]
        } else {
            Vec::new()
        },
        ..ShellFacts::default()
    }
}

#[test]
fn committed_terminal_readback_survives_lost_ack_without_erasing_new_blockers()
-> rusqlite::Result<()> {
    let store = fixture();
    record(store.conn(), &record_for("prepared"), 1)?;
    record(store.conn(), &record_for("running"), 2)?;
    let done = record_for("succeeded");
    let facts = settled_facts(false);
    record_with_facts(store.conn(), &done, &facts, 2)?;
    assert!(verify_record(store.conn(), &done, &facts)?);
    assert_eq!(recover_generation(store.conn(), "new-generation", 3)?, 0);
    assert!(!has_unknown_workspace(store.conn(), "/workspace")?);
    let mut second = record_for("prepared");
    second.operation_id = "second";
    record(store.conn(), &second, 4)?;
    second.state = "outcome_unknown";
    record_with_facts(store.conn(), &second, &settled_facts(true), 5)?;
    record_with_facts(store.conn(), &done, &facts, 6)?;
    assert!(verify_record(store.conn(), &done, &facts)?);
    let remaining = recovery(store.conn(), "/workspace")?;
    assert_eq!(remaining.items.len(), 1);
    assert_eq!(remaining.items[0].operation_id, "second");
    let count: i64 = store.conn().query_row(
        "SELECT COUNT(*) FROM shell_dispositions WHERE disposition_id='op:terminal'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);
    let changed = settled_facts(true);
    assert!(!verify_record(store.conn(), &done, &changed)?);
    Ok(())
}

#[test]
fn irreversible_output_loss_can_be_accepted_without_rewriting_history_after_restart()
-> rusqlite::Result<()> {
    let store = fixture();
    record(store.conn(), &record_for("prepared"), 1)?;
    let mut done = record_for("outcome_unknown");
    done.exit_code = Some(0);
    let mut facts = settled_facts(true);
    facts.resources_closed = false;
    record_with_facts(store.conn(), &done, &facts, 2)?;
    let before = recovery(store.conn(), "/workspace")?;
    assert!(
        resume_writes(
            store.conn(),
            "/workspace",
            &before.epoch,
            &["op".into()],
            "too-early",
            3
        )
        .is_err()
    );
    facts.resources_closed = true;
    append_cleanup_observation(store.conn(), "op", "reader-closed", &facts, 4)?;
    let closed = recovery(store.conn(), "/workspace")?;
    assert_eq!(closed.items.len(), 1);
    assert!(closed.items[0].facts.output_incomplete);
    assert!(
        resume_writes(
            store.conn(),
            "/workspace",
            &before.epoch,
            &["op".into()],
            "stale",
            5
        )
        .is_err()
    );
    resume_writes(
        store.conn(),
        "/workspace",
        &closed.epoch,
        &["op".into()],
        "accepted-loss",
        6,
    )?;
    resume_writes(
        store.conn(),
        "/workspace",
        &closed.epoch,
        &["op".into()],
        "accepted-loss",
        7,
    )?;
    assert_eq!(recover_generation(store.conn(), "after-restart", 8)?, 0);
    assert!(recovery(store.conn(), "/workspace")?.items.is_empty());
    let history: (String, Option<i32>, bool) = store.conn().query_row(
        "SELECT state,exit_code,output_incomplete FROM shell_operations WHERE operation_id='op'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    assert_eq!(history, ("outcome_unknown".into(), Some(0), true));
    let decisions: i64 = store.conn().query_row(
        "SELECT COUNT(*) FROM shell_dispositions WHERE kind='user_risk'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(decisions, 1);
    Ok(())
}

#[test]
fn resource_registration_is_stable_and_never_replaces_prior_identity() -> rusqlite::Result<()> {
    let store = fixture();
    assert!(register_resource(store.conn(), "op", "resource").is_err());
    record(store.conn(), &record_for("prepared"), 1)?;
    register_resource(store.conn(), "op", "resource")?;
    register_resource(store.conn(), "op", "resource")?;
    assert_eq!(resource(store.conn(), "op")?.as_deref(), Some("resource"));
    assert!(register_resource(store.conn(), "op", "replacement").is_err());
    record(store.conn(), &record_for("running"), 2)?;
    assert!(register_resource(store.conn(), "op", "replacement").is_err());
    assert_eq!(resource(store.conn(), "op")?.as_deref(), Some("resource"));
    Ok(())
}

#[test]
fn reappearing_alias_exposes_original_blocker_and_rejects_stale_recovery()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("vega-shell-recovery-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    let historical = root.join("historical");
    let target = root.join("target");
    let store = fixture();
    let historical_text = historical.to_string_lossy();
    let target_text = target.to_string_lossy();
    let mut operation = record_for("prepared");
    operation.workspace = &historical_text;
    record(store.conn(), &operation, 1)?;
    operation.state = "outcome_unknown";
    record_with_facts(store.conn(), &operation, &settled_facts(true), 2)?;
    assert!(recovery(store.conn(), &target_text)?.items.is_empty());
    std::fs::create_dir(&target)?;
    std::os::unix::fs::symlink(&target, &historical)?;
    let initial = recovery(store.conn(), &target_text)?;
    assert_eq!(initial.items.len(), 1);
    assert_eq!(initial.items[0].operation_id, "op");
    assert_eq!(command_presentations(store.conn(), &target_text)?.len(), 1);
    append_cleanup_observation(
        store.conn(),
        "op",
        "fresh-observation",
        &settled_facts(true),
        3,
    )?;
    assert!(
        resume_writes(
            store.conn(),
            &target_text,
            &initial.epoch,
            &["op".into()],
            "stale-alias",
            4
        )
        .is_err()
    );
    let current = recovery(store.conn(), &target_text)?;
    assert_ne!(current.epoch, initial.epoch);
    resume_writes(
        store.conn(),
        &target_text,
        &current.epoch,
        &["op".into()],
        "accept-alias",
        5,
    )?;
    assert!(verify_resume_decision(
        store.conn(),
        &target_text,
        &current.epoch,
        &["op".into()],
        "accept-alias"
    )?);
    assert!(recovery(store.conn(), &target_text)?.items.is_empty());
    assert!(recovery(store.conn(), &historical_text)?.items.is_empty());
    let mut newer = record_for("prepared");
    newer.operation_id = "new-alias-operation";
    newer.workspace = &historical_text;
    record(store.conn(), &newer, 6)?;
    newer.state = "outcome_unknown";
    record_with_facts(store.conn(), &newer, &settled_facts(true), 7)?;
    resume_writes(
        store.conn(),
        &target_text,
        &current.epoch,
        &["op".into()],
        "accept-alias",
        8,
    )?;
    let remaining = recovery(store.conn(), &target_text)?;
    assert_eq!(remaining.items.len(), 1);
    assert_eq!(remaining.items[0].operation_id, "new-alias-operation");
    std::fs::remove_file(&historical)?;
    std::fs::remove_dir(&target)?;
    std::fs::remove_dir(root)?;
    Ok(())
}

#[test]
fn alias_leaving_scope_cannot_cancel_a_new_observation_generation()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("vega-shell-version-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    let historical = root.join("historical");
    let target = root.join("target");
    std::fs::create_dir(&target)?;
    std::os::unix::fs::symlink(&target, &historical)?;
    let store = fixture();
    let historical_text = historical.to_string_lossy();
    let target_text = target.to_string_lossy();
    let mut old = record_for("prepared");
    old.operation_id = "completed-history";
    old.workspace = &historical_text;
    record(store.conn(), &old, 1)?;
    old.state = "running";
    record(store.conn(), &old, 1)?;
    old.state = "succeeded";
    old.exit_code = Some(0);
    let mut complete = settled_facts(false);
    complete.cleanup_state = "best_effort_completed".into();
    record_with_facts(store.conn(), &old, &complete, 2)?;
    let mut operation = record_for("prepared");
    operation.workspace = &target_text;
    record(store.conn(), &operation, 3)?;
    operation.state = "outcome_unknown";
    let mut unresolved = settled_facts(false);
    unresolved.cleanup_state = "unresolved".into();
    unresolved.cleanup_reasons = vec!["cleanup_budget_exhausted".into()];
    record_with_facts(store.conn(), &operation, &unresolved, 4)?;
    let initial = recovery(store.conn(), &target_text)?;
    assert_eq!(initial.items.len(), 1);
    let prior_sum: i64 = initial.epoch.scopes.iter().map(|(_, value)| value).sum();
    assert_eq!(prior_sum, 2);
    std::fs::remove_file(&historical)?;
    append_cleanup_observation(
        store.conn(),
        "op",
        "new-output-loss",
        &settled_facts(true),
        5,
    )?;
    let current = recovery(store.conn(), &target_text)?;
    assert_eq!(
        current
            .epoch
            .scopes
            .iter()
            .map(|(_, value)| value)
            .sum::<i64>(),
        prior_sum
    );
    assert_ne!(current.epoch, initial.epoch);
    assert!(current.items[0].facts.output_incomplete);
    assert!(
        resume_writes(
            store.conn(),
            &target_text,
            &initial.epoch,
            &["op".into()],
            "stale-scope",
            6
        )
        .is_err()
    );
    assert!(!verify_resume_decision(
        store.conn(),
        &target_text,
        &initial.epoch,
        &["op".into()],
        "stale-scope"
    )?);
    assert_eq!(recovery(store.conn(), &target_text)?.items.len(), 1);
    resume_writes(
        store.conn(),
        &target_text,
        &current.epoch,
        &["op".into()],
        "fresh-scope",
        7,
    )?;
    assert!(verify_resume_decision(
        store.conn(),
        &target_text,
        &current.epoch,
        &["op".into()],
        "fresh-scope"
    )?);
    assert!(recovery(store.conn(), &target_text)?.items.is_empty());
    std::fs::remove_dir(&target)?;
    std::fs::remove_dir(root)?;
    Ok(())
}

#[test]
fn accepted_decision_replay_does_not_clear_new_or_unselected_blockers() -> rusqlite::Result<()> {
    let store = fixture();
    for id in ["op", "other-operation"] {
        let mut operation = record_for("prepared");
        operation.operation_id = id;
        record(store.conn(), &operation, 1)?;
        operation.state = "outcome_unknown";
        record_with_facts(store.conn(), &operation, &settled_facts(true), 2)?;
    }
    let initial = recovery(store.conn(), "/workspace")?;
    resume_writes(
        store.conn(),
        "/workspace",
        &initial.epoch,
        &["op".into()],
        "accept-one",
        3,
    )?;
    assert!(verify_resume_decision(
        store.conn(),
        "/workspace",
        &initial.epoch,
        &["op".into()],
        "accept-one"
    )?);
    assert!(!verify_resume_decision(
        store.conn(),
        "/workspace",
        &ShellRecoveryVersion {
            epoch: initial.epoch.epoch + 1,
            scopes: initial.epoch.scopes.clone()
        },
        &["op".into()],
        "accept-one"
    )?);
    assert!(!verify_resume_decision(
        store.conn(),
        "/workspace",
        &initial.epoch,
        &["other-operation".into()],
        "accept-one"
    )?);
    assert!(!verify_resume_decision(
        store.conn(),
        "/workspace",
        &initial.epoch,
        &["op".into()],
        "uncommitted"
    )?);
    let mut third = record_for("prepared");
    third.operation_id = "new-operation";
    record(store.conn(), &third, 4)?;
    third.state = "outcome_unknown";
    record_with_facts(store.conn(), &third, &settled_facts(true), 5)?;
    resume_writes(
        store.conn(),
        "/workspace",
        &initial.epoch,
        &["op".into()],
        "accept-one",
        6,
    )?;
    let remaining = recovery(store.conn(), "/workspace")?;
    let mut ids: Vec<_> = remaining
        .items
        .iter()
        .map(|item| item.operation_id.as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["new-operation", "other-operation"]);
    assert!(has_unknown_workspace(store.conn(), "/workspace/nested")?);
    assert!(has_unknown_workspace(store.conn(), "/")?);
    assert!(!has_unknown_workspace(store.conn(), "/independent")?);
    Ok(())
}
