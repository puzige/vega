use super::*;
use vega_runtime::RuntimeShellOperationRecord;
use vega_store::shell_operations::{ShellFacts, ShellOperationRecord};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("vega-shell-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        store.conn().execute(
            "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project',?1,'fixture',1,1)",
            [root.to_string_lossy().as_ref()],
        )?;
        store.conn().execute("INSERT INTO threads(id,project_id,model,created_at,updated_at) VALUES('thread','project','model',1,1)", [])?;
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('run','thread',1,'assistant','','streaming',1)", [])?;
        Ok(Self { root, store })
    }

    fn record(
        &self,
        operation: &str,
        concurrent: bool,
    ) -> Result<RuntimeShellOperationRecord, Box<dyn std::error::Error>> {
        self.store.conn().execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,status,created_at) VALUES(?1,'thread','run',1,'bash','{}','running',1)", [operation])?;
        let record = RuntimeShellOperationRecord {
            operation_id: operation.into(),
            thread_id: "thread".into(),
            run_id: "run".into(),
            call_id: operation.into(),
            generation: "old-generation".into(),
            workspace: self.root.to_string_lossy().into_owned(),
            concurrent,
            state: "outcome_unknown".into(),
            exit_code: Some(23),
            error: Some("cleanup_unresolved".into()),
            duration_ms: Some(42),
            command_result: Some("exited".into()),
            exit_signal: Some(15),
            cleanup_state: "unresolved".into(),
            output_incomplete: true,
            resources_closed: false,
            process_resolved: true,
            cleanup_reasons: vec!["reader_unresolved".into()],
        };
        let mut prepared = persisted(&record);
        prepared.state = "prepared";
        prepared.exit_code = None;
        prepared.error = None;
        prepared.duration_ms = None;
        vega_store::shell_operations::record(self.store.conn(), &prepared, 1)?;
        Ok(record)
    }

    fn apply<
        C: crate::database_writer::DatabaseCommand<Output = Result<(), ShellRecoveryError>>,
    >(
        &self,
        command: C,
    ) -> TestResult {
        commit(&self.store, command).map_err(|error| format!("{error:?}").into())
    }

    fn recovery(
        &self,
    ) -> Result<vega_store::shell_operations::ShellWorkspaceRecovery, Box<dyn std::error::Error>>
    {
        Ok(vega_store::shell_operations::recovery(
            self.store.conn(),
            self.root.to_string_lossy().as_ref(),
        )?)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn persisted(record: &RuntimeShellOperationRecord) -> ShellOperationRecord<'_> {
    ShellOperationRecord {
        operation_id: &record.operation_id,
        thread_id: &record.thread_id,
        run_id: &record.run_id,
        call_id: &record.call_id,
        generation: &record.generation,
        workspace: &record.workspace,
        concurrent: record.concurrent,
        state: &record.state,
        exit_code: record.exit_code,
        error: record.error.as_deref(),
        duration_ms: record.duration_ms,
    }
}

fn facts(record: &RuntimeShellOperationRecord) -> ShellFacts {
    ShellFacts {
        command_result: record.command_result.clone(),
        exit_signal: record.exit_signal,
        cleanup_state: record.cleanup_state.clone(),
        output_incomplete: record.output_incomplete,
        resources_closed: record.resources_closed,
        process_resolved: record.process_resolved,
        cleanup_reasons: record.cleanup_reasons.clone(),
    }
}

#[test]
fn concurrent_terminal_replay_preserves_all_facts_and_rejects_changed_content() -> TestResult {
    let fixture = Fixture::new()?;
    let record = fixture.record("concurrent", true)?;
    let before = fixture.recovery()?.epoch;
    fixture.apply(ShellRecord(record.clone()))?;
    fixture.apply(ShellRecord(record.clone()))?;
    assert!(vega_store::shell_operations::verify_record(
        fixture.store.conn(),
        &persisted(&record),
        &facts(&record),
    )?);
    assert!(fixture.recovery()?.items.is_empty());
    assert_eq!(fixture.recovery()?.epoch, before);
    let count: i64 = fixture.store.conn().query_row(
        "SELECT COUNT(*) FROM shell_dispositions WHERE disposition_id='concurrent:terminal'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);
    let mut changed = record.clone();
    changed.concurrent = false;
    assert!(commit(&fixture.store, ShellRecord(changed)).is_err());
    let mut changed = record.clone();
    changed.cleanup_reasons.push("another_reason".into());
    assert!(commit(&fixture.store, ShellRecord(changed)).is_err());
    assert!(vega_store::shell_operations::verify_record(
        fixture.store.conn(),
        &persisted(&record),
        &facts(&record),
    )?);
    Ok(())
}

#[test]
fn concurrent_cleanup_observation_preserves_sandbox_blocker_and_recovery_version() -> TestResult {
    let fixture = Fixture::new()?;
    let sandbox = fixture.record("sandbox", false)?;
    let concurrent = fixture.record("concurrent", true)?;
    fixture.apply(ShellRecord(sandbox.clone()))?;
    fixture.apply(ShellRecord(concurrent))?;
    let before = fixture.recovery()?;
    assert_eq!(before.items.len(), 1);
    assert_eq!(before.items[0].operation_id, sandbox.operation_id);
    let observation = ShellFacts {
        command_result: Some("exited".into()),
        exit_signal: Some(15),
        cleanup_state: "best_effort_completed".into(),
        output_incomplete: false,
        resources_closed: true,
        process_resolved: true,
        cleanup_reasons: Vec::new(),
    };
    for _ in 0..2 {
        fixture.apply(CleanupObservation {
            operation: "concurrent".into(),
            observation: "cleanup".into(),
            facts: observation.clone(),
        })?;
    }
    let after = fixture.recovery()?;
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.items.len(), 1);
    assert_eq!(after.items[0].operation_id, sandbox.operation_id);
    assert_eq!(after.items[0].facts, before.items[0].facts);
    let saved: String = fixture.store.conn().query_row(
        "SELECT facts FROM shell_cleanup_observations WHERE observation_id='cleanup'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(serde_json::from_str::<ShellFacts>(&saved)?, observation);
    Ok(())
}

#[test]
fn generation_recovery_expires_concurrent_control_without_workspace_blocker() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.record("sandbox", false)?;
    fixture.record("concurrent", true)?;
    fixture.apply(RecoverGeneration {
        generation: "new-generation".into(),
    })?;
    let recovery = fixture.recovery()?;
    assert_eq!(recovery.items.len(), 1);
    assert_eq!(recovery.items[0].operation_id, "sandbox");
    let (state, result, error, reasons): (String, String, String, String) = fixture.store.conn().query_row(
        "SELECT state,command_result,error,cleanup_reasons FROM shell_operations WHERE operation_id='concurrent'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    assert_eq!(state, "outcome_unknown");
    assert_eq!(result, "outcome_unknown");
    assert_eq!(error, "session_expired");
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&reasons)?,
        vec!["control_continuity_lost"]
    );
    Ok(())
}
