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
            before.epoch,
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
            before.epoch,
            &["op".into()],
            "stale",
            5
        )
        .is_err()
    );
    resume_writes(
        store.conn(),
        "/workspace",
        closed.epoch,
        &["op".into()],
        "accepted-loss",
        6,
    )?;
    resume_writes(
        store.conn(),
        "/workspace",
        closed.epoch,
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
        initial.epoch,
        &["op".into()],
        "accept-one",
        3,
    )?;
    let mut third = record_for("prepared");
    third.operation_id = "new-operation";
    record(store.conn(), &third, 4)?;
    third.state = "outcome_unknown";
    record_with_facts(store.conn(), &third, &settled_facts(true), 5)?;
    resume_writes(
        store.conn(),
        "/workspace",
        initial.epoch,
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
