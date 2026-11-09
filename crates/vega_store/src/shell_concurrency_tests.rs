use super::*;

fn fixture() -> crate::Store {
    let store = crate::Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    store.conn().execute_batch("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('p','/workspace','project',0,0); INSERT INTO threads(id,project_id,model,created_at,updated_at) VALUES('t','p','model',0,0); INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('r','t',1,'assistant','','streaming',0); INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,status,created_at) VALUES('c','t','r',1,'bash','{}','running',0),('d','t','r',2,'bash','{}','running',0);").unwrap();
    store
}

fn operation(id: &str, concurrent: bool) -> ShellOperationRecord<'_> {
    ShellOperationRecord {
        operation_id: id,
        thread_id: "t",
        run_id: "r",
        call_id: if concurrent { "c" } else { "d" },
        generation: "old",
        workspace: "/workspace",
        concurrent,
        state: "prepared",
        exit_code: None,
        error: None,
        duration_ms: None,
    }
}

#[test]
fn concurrent_unknown_preserves_facts_without_blocking_or_changing_sandbox_recovery() {
    let store = fixture();
    let mut sandbox = operation("sandbox", false);
    record(store.conn(), &sandbox, 1).unwrap();
    sandbox.state = "running";
    record(store.conn(), &sandbox, 2).unwrap();
    sandbox.state = "outcome_unknown";
    record(store.conn(), &sandbox, 3).unwrap();
    let before = recovery(store.conn(), "/workspace").unwrap();
    let mut full = operation("full", true);
    record(store.conn(), &full, 4).unwrap();
    full.state = "running";
    record(store.conn(), &full, 5).unwrap();
    let mut forged = operation("full", false);
    forged.call_id = "c";
    forged.state = "running";
    assert!(record(store.conn(), &forged, 6).is_err());
    full.state = "outcome_unknown";
    full.exit_code = Some(0);
    let facts = ShellFacts {
        command_result: Some("exited".into()),
        cleanup_state: "unresolved".into(),
        resources_closed: true,
        process_resolved: false,
        output_incomplete: true,
        cleanup_reasons: vec!["process_identity_changed".into()],
        ..ShellFacts::default()
    };
    record_with_facts(store.conn(), &full, &facts, 7).unwrap();
    assert!(verify_record(store.conn(), &full, &facts).unwrap());
    record_with_facts(store.conn(), &full, &facts, 8).unwrap();
    append_cleanup_observation(store.conn(), "full", "observation", &facts, 9).unwrap();
    append_cleanup_observation(store.conn(), "full", "observation", &facts, 10).unwrap();
    let mut changed = facts.clone();
    changed.process_resolved = true;
    assert!(append_cleanup_observation(store.conn(), "full", "observation", &changed, 11).is_err());
    changed.cleanup_state = "best_effort_completed".into();
    changed.output_incomplete = false;
    changed.cleanup_reasons.clear();
    append_cleanup_observation(store.conn(), "full", "settled", &changed, 12).unwrap();
    let after = recovery(store.conn(), "/workspace").unwrap();
    assert_eq!(before.epoch, after.epoch);
    assert_eq!(after.items.len(), 1);
    assert_eq!(after.items[0].operation_id, "sandbox");
    let commands = command_presentations(store.conn(), "/workspace").unwrap();
    let full = commands
        .iter()
        .find(|item| item.operation_id == "full")
        .unwrap();
    assert!(!full.cleanup_unresolved);
    assert_eq!(
        (&*full.state, full.exit_code, full.output_incomplete),
        ("outcome_unknown", Some(0), true)
    );
}

#[test]
fn restart_marks_concurrent_execution_unknown_without_creating_workspace_blocker() {
    let store = fixture();
    let mut full = operation("full", true);
    record(store.conn(), &full, 1).unwrap();
    full.state = "running";
    record(store.conn(), &full, 2).unwrap();
    let before = recovery(store.conn(), "/workspace").unwrap().epoch;
    assert_eq!(recover_generation(store.conn(), "new", 3).unwrap(), 1);
    assert_eq!(recover_generation(store.conn(), "new", 4).unwrap(), 0);
    assert!(list_blockers(store.conn()).unwrap().is_empty());
    assert_eq!(recovery(store.conn(), "/workspace").unwrap().epoch, before);
    let actual: (String, String, String) = store.conn().query_row("SELECT state,command_result,cleanup_reasons FROM shell_operations WHERE operation_id='full'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(
        actual,
        (
            "outcome_unknown".into(),
            "outcome_unknown".into(),
            "[\"control_continuity_lost\"]".into()
        )
    );
}
