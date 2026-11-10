use super::*;
use crate::Store;
use crate::context_compaction::source_version::{PreparedSourceVersion, SourceFence};
use crate::context_precompute::{self, accounting, consume, ready};
use crate::context_projection::{
    ContinuationMode, ProjectionFragment, ProjectionPlan, ProjectionStrategy, SourceCursor,
    SourceRange,
};
use crate::delegated_stages::{
    self as stages, PreparedAbandonedRecovery, PreparedStageCheckpoint, PreparedTerminal,
};
use serde_json::json;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    store: Store,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("vega-prepared-source-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        store.conn().execute_batch("INSERT INTO threads(id,model,created_at,updated_at) VALUES('source','model',1,1),('other','model',1,1); INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('user','source',1,'user','Question','done',1),('assistant','source',2,'assistant','Answer','done',2),('other-user','other',1,'user','Other','done',1);")?;
        Ok(Self { store, root })
    }

    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn external(&self) -> rusqlite::Result<Store> {
        Store::open_existing(self.root.join("fixture.db"))
    }

    fn checkpoint(&self) -> Result<NewContextCheckpoint, Box<dyn std::error::Error>> {
        let source = load_source(self.conn(), "source")?;
        Ok(NewContextCheckpoint {
            thread_id: "source".into(),
            model: "model".into(),
            source_version: source.source_version,
            covered_through_seq: 2,
            covered_assistant_id: None,
            covered_assistant_text_bytes: 0,
            covered_assistant_tool_seq: 0,
            source_fingerprint: source.fingerprint,
            summary: "Historical summary".into(),
            estimator_version: "development-estimator".into(),
            expected_previous_id: None,
            projection_plan: None,
            created_at: 10,
        })
    }

    fn tool(&self, tool: &str, status: &str) -> rusqlite::Result<()> {
        self.conn().execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,finished_at,text_offset_bytes) VALUES('call','source','assistant',3,?1,'{}','Tool output',?2,3,4,6)", params![tool,status])?;
        Ok(())
    }

    fn candidate(&self) -> Result<ready::ReadyCompaction, Box<dyn std::error::Error>> {
        ready::begin_generation(self.conn(), "generation", "source", "model", None)?;
        accounting::record_stage(
            self.conn(),
            "generation",
            "source",
            "model",
            1,
            Some(&accounting::ReportedUsage {
                input_tokens: 11,
                output_tokens: 7,
                cache_read_tokens: None,
                cache_write_tokens: None,
                cost_microcents: 13,
                created_at: 5,
                pricing_version: Some("v1".into()),
                pricing_profile: Some("profile".into()),
                call_started_at: Some(4),
            }),
        )?;
        accounting::finish(self.conn(), "generation", "source", "model", 1, true)?;
        let mut checkpoint = self.checkpoint()?;
        let cursor = SourceCursor {
            message_id: "assistant".into(),
            message_seq: 2,
            assistant: None,
        };
        checkpoint.projection_plan = Some(ProjectionPlan {
            history_dependencies: None,
            schema: 1,
            strategy: ProjectionStrategy::Precompute,
            continuation: ContinuationMode::NoContinuation,
            source_owner_id: None,
            task_message_id: None,
            covered: vec![SourceRange {
                after: None,
                through: Some(cursor.clone()),
            }],
            omitted: vec![],
            fragments: vec![
                ProjectionFragment::Summary,
                ProjectionFragment::Source {
                    range: SourceRange {
                        after: Some(cursor),
                        through: None,
                    },
                },
            ],
        });
        let tx = self.conn().unchecked_transaction()?;
        let (_, source) = context_precompute::capture_in_transaction(&tx, "source", None)?;
        let summary_dependencies =
            ready::summary_dependencies_in_transaction(&tx, "source", "model")?;
        tx.commit()?;
        Ok(ready::ReadyCompaction {
            schema: 1,
            generation: "generation".into(),
            checkpoint,
            source,
            summary_dependencies,
            pre_tokens: 100,
            preferences_digest: "a".repeat(64),
            stage_count: 1,
            usage_complete: true,
        })
    }

    fn publish(&self, candidate: &ready::ReadyCompaction) -> Result<(), ContextCheckpointError> {
        let prepared = ready::PreparedPublication::prepare(self.conn(), candidate, 11)?;
        ready::publish_prepared_with_guard(self.conn(), candidate, 11, &prepared, || true)
    }

    fn consumption(&self) -> Result<consume::PreparedConsumption, Box<dyn std::error::Error>> {
        let tx = self.conn().unchecked_transaction()?;
        let prepared = consume::prepare_in_transaction(&tx, "source", None, 12)?
            .ok_or("published candidate unavailable")?;
        tx.commit()?;
        Ok(prepared)
    }

    fn counts(&self) -> rusqlite::Result<(i64, i64, i64, i64, i64)> {
        self.conn().query_row("SELECT (SELECT COUNT(*) FROM context_checkpoints),(SELECT COUNT(*) FROM context_precompute_ready),(SELECT COALESCE(SUM(retired),0) FROM context_precompute_accounting),(SELECT COUNT(*) FROM token_usage),(SELECT COUNT(*) FROM context_precompute_stages)", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))
    }

    fn durable_state(&self) -> rusqlite::Result<Vec<String>> {
        let mut values = Vec::new();
        for table in [
            "context_checkpoints",
            "context_precompute_ready",
            "context_precompute_accounting",
            "context_precompute_slots",
            "context_precompute_stages",
            "token_usage",
        ] {
            let mut query = self
                .conn()
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
            let columns = query.column_count();
            let rows = query
                .query_map([], |row| {
                    (0..columns)
                        .map(|column| row.get::<_, rusqlite::types::Value>(column))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            values.push(format!("{table}:{rows:?}"));
        }
        Ok(values)
    }

    fn register(&self) -> rusqlite::Result<()> {
        stages::register(self.conn(), "source", "assistant", None, &json!([]))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn compact_fence_rejects_external_same_sequence_edits_without_rejecting_other_threads() -> TestResult
{
    let fixture = Fixture::new()?;
    let fence = SourceFence::capture(fixture.conn(), "source")?;
    let source = PreparedSourceVersion::capture(fixture.conn(), "source")?;
    let external = fixture.external()?;
    external.conn().execute(
        "UPDATE messages SET content='Unrelated' WHERE id='other-user'",
        [],
    )?;
    let tx = fixture.conn().unchecked_transaction()?;
    assert!(fence.validate(&tx)?);
    assert!(source.validate(&tx, "source")?);
    assert!(!source.validate(&tx, "other")?);
    tx.commit()?;
    external.conn().execute(
        "UPDATE messages SET content='Edited at the same sequence' WHERE id='assistant'",
        [],
    )?;
    assert_eq!(
        load_source(fixture.conn(), "source")?.source_version,
        source.source().source_version
    );
    let tx = fixture.conn().unchecked_transaction()?;
    assert!(!fence.validate(&tx)?);
    assert!(!source.validate(&tx, "source")?);
    Ok(())
}

#[test]
fn compact_fence_tracks_transitive_source_and_authority_mutations() -> TestResult {
    for mutation in [
        "UPDATE tool_calls SET output_text='Changed' WHERE id='call'",
        "INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('assistant','response',0,6,'final')",
        "INSERT INTO context_micro_compactions(call_id,thread_id,source_digest,created_at) VALUES('call','source','digest',5)",
        "UPDATE threads SET permission_mode='readonly' WHERE id='source'",
        "INSERT INTO credential_continuations(id,thread_id,user_message_id,source_version,source_fingerprint,raw_user_fingerprint,state,reason,created_at) VALUES('user','source','user',3,'fingerprint','raw','ready','credentials',5)",
    ] {
        let fixture = Fixture::new()?;
        fixture.tool("read", "success")?;
        let fence = SourceFence::capture(fixture.conn(), "source")?;
        fixture.external()?.conn().execute_batch(mutation)?;
        let tx = fixture.conn().unchecked_transaction()?;
        assert!(
            !fence.validate(&tx)?,
            "mutation retained a stale fence: {mutation}"
        );
    }
    Ok(())
}

#[test]
fn replace_invalidates_both_displaced_and_incoming_owners_with_recursive_triggers_disabled()
-> TestResult {
    for mutation in [
        "INSERT OR REPLACE INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('assistant','other',2,'assistant','Replacement','done',5)",
        "INSERT OR REPLACE INTO credential_continuations(id,thread_id,user_message_id,source_version,source_fingerprint,raw_user_fingerprint,state,reason,created_at) VALUES('user','other','other-user',1,'fingerprint','raw','ready','credentials',5)",
    ] {
        let fixture = Fixture::new()?;
        fixture.conn().execute("INSERT INTO credential_continuations(id,thread_id,user_message_id,source_version,source_fingerprint,raw_user_fingerprint,state,reason,created_at) VALUES('user','source','user',2,'fingerprint','raw','ready','credentials',4)", [])?;
        let original = SourceFence::capture(fixture.conn(), "source")?;
        let incoming = SourceFence::capture(fixture.conn(), "other")?;
        let external = fixture.external()?;
        external
            .conn()
            .pragma_update(None, "recursive_triggers", false)?;
        external.conn().execute_batch(mutation)?;
        let tx = fixture.conn().unchecked_transaction()?;
        assert!(!original.validate(&tx)?);
        assert!(!incoming.validate(&tx)?);
    }
    Ok(())
}

#[test]
fn missing_or_replaced_schema_trigger_fails_closed_even_when_schema_cookie_is_restored()
-> TestResult {
    for (trigger, replacement) in [
        ("preparation_messages_update", ""),
        (
            "preparation_messages_update",
            "CREATE TRIGGER preparation_messages_update BEFORE UPDATE ON messages BEGIN SELECT 1; END;",
        ),
        ("source_validation_credential_continuations_update", ""),
        (
            "source_validation_credential_continuations_update",
            "CREATE TRIGGER source_validation_credential_continuations_update BEFORE UPDATE ON credential_continuations BEGIN SELECT 1; END;",
        ),
    ] {
        let fixture = Fixture::new()?;
        let fence = SourceFence::capture(fixture.conn(), "source")?;
        let schema: i64 = fixture
            .conn()
            .pragma_query_value(None, "schema_version", |row| row.get(0))?;
        let external = fixture.external()?;
        external
            .conn()
            .execute_batch(&format!("DROP TRIGGER {trigger};{replacement}"))?;
        external
            .conn()
            .pragma_update(None, "schema_version", schema)?;
        assert!(SourceFence::capture(fixture.conn(), "source").is_err());
        let tx = fixture.conn().unchecked_transaction()?;
        assert!(!fence.validate(&tx)?);
    }
    Ok(())
}

#[test]
fn prepared_checkpoint_installs_and_retires_generation_atomically() -> TestResult {
    let fixture = Fixture::new()?;
    let candidate = fixture.candidate()?;
    fixture.publish(&candidate)?;
    let prepared = PreparedContextCheckpoint::prepare(fixture.conn(), fixture.checkpoint()?)?;
    let (installed, retired) = prepared.install_and_retire(fixture.conn(), || false)?;
    assert!(matches!(installed, ContextCheckpointInstall::Applied(_)));
    assert_eq!(retired.as_deref(), Some("generation"));
    assert_eq!(fixture.counts()?, (1, 0, 1, 1, 1));
    Ok(())
}

#[test]
fn stale_prepared_install_preserves_usage_accounting_and_ready_generation() -> TestResult {
    let fixture = Fixture::new()?;
    let candidate = fixture.candidate()?;
    fixture.publish(&candidate)?;
    let prepared = PreparedContextCheckpoint::prepare(fixture.conn(), fixture.checkpoint()?)?;
    let before = fixture.durable_state()?;
    fixture
        .external()?
        .conn()
        .execute("UPDATE messages SET content='Changed' WHERE id='user'", [])?;
    let (installed, retired) = prepared.install_and_retire(fixture.conn(), || false)?;
    assert!(matches!(installed, ContextCheckpointInstall::Stale));
    assert_eq!(retired, None);
    assert_eq!(fixture.durable_state()?, before);
    Ok(())
}

#[test]
fn cancellation_after_checkpoint_and_retirement_rolls_back_both() -> TestResult {
    let fixture = Fixture::new()?;
    let candidate = fixture.candidate()?;
    fixture.publish(&candidate)?;
    let prepared = PreparedContextCheckpoint::prepare(fixture.conn(), fixture.checkpoint()?)?;
    let before = fixture.durable_state()?;
    let observed_retirement = std::cell::Cell::new(false);
    let result = prepared.install_and_retire(fixture.conn(), || {
        let retired: bool = fixture
            .conn()
            .query_row(
                "SELECT retired FROM context_precompute_accounting WHERE generation='generation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        observed_retirement.set(observed_retirement.get() || retired);
        retired
    });
    assert!(observed_retirement.get());
    assert!(matches!(result, Err(ContextCheckpointError::Cancelled)));
    assert_eq!(fixture.durable_state()?, before);
    Ok(())
}

#[test]
fn prepare_rejects_nonterminal_projection_and_invalid_utf8_partial_boundary() -> TestResult {
    let fixture = Fixture::new()?;
    let candidate = fixture.candidate()?;
    fixture.conn().execute(
        "UPDATE messages SET status='streaming' WHERE id='assistant'",
        [],
    )?;
    let mut checkpoint = candidate.checkpoint;
    checkpoint.source_fingerprint = load_source(fixture.conn(), "source")?.fingerprint;
    assert!(matches!(
        PreparedContextCheckpoint::prepare(fixture.conn(), checkpoint),
        Err(ContextCheckpointError::IncompleteCoverage)
    ));
    fixture.conn().execute(
        "UPDATE messages SET status='done',content='éAnswer' WHERE id='assistant'",
        [],
    )?;
    fixture.tool("read", "success")?;
    let mut checkpoint = fixture.checkpoint()?;
    checkpoint.covered_through_seq = 1;
    checkpoint.covered_assistant_id = Some("assistant".into());
    checkpoint.covered_assistant_text_bytes = 1;
    checkpoint.covered_assistant_tool_seq = 3;
    assert!(matches!(
        PreparedContextCheckpoint::prepare(fixture.conn(), checkpoint),
        Err(ContextCheckpointError::IncompleteCoverage)
    ));
    assert_eq!(fixture.counts()?.0, 0);
    Ok(())
}

#[test]
fn prepared_publication_and_consumption_commit_with_priced_usage() -> TestResult {
    let fixture = Fixture::new()?;
    let candidate = fixture.candidate()?;
    fixture.publish(&candidate)?;
    let prepared = fixture.consumption()?;
    assert!(prepared.usage_priced());
    let checkpoint = prepared.checkpoint("model", None, "development-estimator", 12)?;
    let install = prepared.prepare_install(fixture.conn(), checkpoint.clone())?;
    let projection = prepared.commit_projection()?;
    drop(prepared);
    assert!(matches!(
        consume::install_prepared_with_guard(
            fixture.conn(),
            &projection,
            &checkpoint,
            12,
            &install,
            || false
        )?,
        ContextCheckpointInstall::Applied(_)
    ));
    assert_eq!(fixture.counts()?, (1, 0, 1, 1, 1));
    Ok(())
}

#[test]
fn prepared_publication_rejects_source_or_accounting_changes_and_revoked_permission() -> TestResult
{
    for mutation in [
        "UPDATE messages SET content='Changed' WHERE id='assistant'",
        "UPDATE token_usage SET input_tokens=input_tokens+1 WHERE thread_id='source'",
        "UPDATE context_precompute_accounting SET usage_complete=0 WHERE generation='generation'",
    ] {
        let fixture = Fixture::new()?;
        let candidate = fixture.candidate()?;
        let prepared = ready::PreparedPublication::prepare(fixture.conn(), &candidate, 11)?;
        fixture.external()?.conn().execute_batch(mutation)?;
        let before = fixture.durable_state()?;
        assert!(matches!(
            ready::publish_prepared_with_guard(fixture.conn(), &candidate, 11, &prepared, || true),
            Err(ContextCheckpointError::Stale)
        ));
        assert_eq!(fixture.durable_state()?, before);
        assert_eq!(fixture.counts()?.1, 0);
    }
    let fixture = Fixture::new()?;
    let candidate = fixture.candidate()?;
    let prepared = ready::PreparedPublication::prepare(fixture.conn(), &candidate, 11)?;
    let checks = std::cell::Cell::new(0);
    assert!(matches!(
        ready::publish_prepared_with_guard(fixture.conn(), &candidate, 11, &prepared, || {
            checks.set(checks.get() + 1);
            checks.get() == 1
        }),
        Err(ContextCheckpointError::Cancelled)
    ));
    assert_eq!(checks.get(), 2);
    assert_eq!(fixture.counts()?, (0, 0, 0, 1, 1));
    Ok(())
}

#[test]
fn prepared_consumption_rejects_source_or_usage_changes_without_half_commit() -> TestResult {
    for mutation in [
        "UPDATE messages SET content='Changed' WHERE id='assistant'",
        "UPDATE token_usage SET cost_microcents=cost_microcents+1 WHERE thread_id='source'",
    ] {
        let fixture = Fixture::new()?;
        let candidate = fixture.candidate()?;
        fixture.publish(&candidate)?;
        let prepared = fixture.consumption()?;
        let checkpoint = prepared.checkpoint("model", None, "development-estimator", 12)?;
        let install = prepared.prepare_install(fixture.conn(), checkpoint.clone())?;
        let projection = prepared.commit_projection()?;
        drop(prepared);
        fixture.external()?.conn().execute_batch(mutation)?;
        let before = fixture.durable_state()?;
        assert!(matches!(
            consume::install_prepared_with_guard(
                fixture.conn(),
                &projection,
                &checkpoint,
                12,
                &install,
                || false
            )?,
            ContextCheckpointInstall::Stale
        ));
        assert_eq!(fixture.durable_state()?, before);
        assert_eq!(fixture.counts()?, (0, 1, 0, 1, 1));
    }
    Ok(())
}

#[test]
fn prepared_micro_installs_expected_fingerprint_and_rejects_later_tool_edit() -> TestResult {
    for stale in [false, true] {
        let fixture = Fixture::new()?;
        fixture.tool("read", "success")?;
        let source = load_source(fixture.conn(), "source")?;
        let prepared = crate::context_micro::PreparedMicro::prepare(
            fixture.conn(),
            &source,
            "model",
            None,
            &["call".into()],
            10,
        )?;
        if stale {
            fixture.external()?.conn().execute(
                "UPDATE tool_calls SET output_text='Changed' WHERE id='call'",
                [],
            )?;
            assert!(matches!(
                prepared.install(fixture.conn(), || false),
                Err(ContextCheckpointError::Stale)
            ));
        } else {
            let expected = prepared.install(fixture.conn(), || false)?;
            assert_eq!(load_source(fixture.conn(), "source")?.fingerprint, expected);
        }
        let count: i64 = fixture.conn().query_row(
            "SELECT COUNT(*) FROM context_micro_compactions",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, i64::from(!stale));
    }
    Ok(())
}

#[test]
fn delegated_checkpoint_preserves_unknown_effects_and_rejects_wrong_or_changed_source() -> TestResult
{
    let fixture = Fixture::new()?;
    fixture.tool("bash", "running")?;
    fixture.register()?;
    let prepared = PreparedStageCheckpoint::prepare(fixture.conn(), "source", "assistant")?;
    let tx = fixture.conn().unchecked_transaction()?;
    let checkpoint = prepared.append(
        &tx,
        "source",
        "assistant",
        json!({"ready_for_review":false}),
        10,
    )?;
    assert_eq!(
        checkpoint["unknown_effects"],
        json!([{"call_id":"call","tool":"bash","status":"running"}])
    );
    tx.commit()?;
    let prepared = PreparedStageCheckpoint::prepare(fixture.conn(), "source", "assistant")?;
    let tx = fixture.conn().unchecked_transaction()?;
    assert!(
        prepared
            .append(&tx, "other", "assistant", json!({}), 11)
            .is_err()
    );
    tx.rollback()?;
    let prepared = PreparedStageCheckpoint::prepare(fixture.conn(), "source", "assistant")?;
    fixture
        .external()?
        .conn()
        .execute("UPDATE tool_calls SET status='success' WHERE id='call'", [])?;
    let tx = fixture.conn().unchecked_transaction()?;
    assert!(
        prepared
            .append(&tx, "source", "assistant", json!({}), 11)
            .is_err()
    );
    tx.rollback()?;
    let count: i64 = fixture.conn().query_row(
        "SELECT COUNT(*) FROM delegated_stage_checkpoints",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn prepared_terminal_recovery_matches_normalized_source_and_keeps_unknown_effects() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.tool("bash", "running")?;
    fixture.conn().execute(
        "UPDATE messages SET status='streaming' WHERE id='assistant'",
        [],
    )?;
    fixture.register()?;
    let prepared = PreparedTerminal::prepare(fixture.conn(), "source", "assistant", true, 10)?;
    let expected = prepared.source().clone();
    let tx = fixture.conn().unchecked_transaction()?;
    prepared.validate(&tx, "source", "assistant")?;
    prepared.recover(&tx, "source", None)?;
    let checkpoint = prepared.final_checkpoint(&tx, "source", "assistant", true)?;
    assert_eq!(checkpoint["origin"], "interrupted");
    assert_eq!(checkpoint["unknown_effects"][0]["status"], "running");
    assert_eq!(load_source_version_in_transaction(&tx, "source")?, expected);
    tx.commit()?;
    let source = load_source(fixture.conn(), "source")?;
    assert_eq!(source.messages[1].status, "interrupted");
    assert_eq!(source.tool_calls[0].status, "cancelled");
    Ok(())
}

#[test]
fn prepared_terminal_and_abandoned_recovery_reject_post_prepare_mutations() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.tool("bash", "running")?;
    fixture.conn().execute(
        "UPDATE messages SET status='streaming' WHERE id='assistant'",
        [],
    )?;
    fixture.register()?;
    let terminal = PreparedTerminal::prepare(fixture.conn(), "source", "assistant", true, 10)?;
    let abandoned = PreparedAbandonedRecovery::prepare(fixture.conn(), "source", 10)?;
    fixture.external()?.conn().execute(
        "UPDATE messages SET content='Changed' WHERE id='assistant'",
        [],
    )?;
    let tx = fixture.conn().unchecked_transaction()?;
    assert!(terminal.validate(&tx, "source", "assistant").is_err());
    assert!(abandoned.install(&tx, "source", None).is_err());
    tx.rollback()?;
    let source = load_source(fixture.conn(), "source")?;
    assert_eq!(source.messages[1].status, "streaming");
    assert_eq!(source.tool_calls[0].status, "running");
    assert!(stages::latest(fixture.conn(), "source", "assistant")?.is_none());
    Ok(())
}

#[test]
fn prepared_delegated_recovery_requires_matching_native_owner() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.conn().execute("UPDATE threads SET parent_thread_id='other',workspace_owner_thread_id='other' WHERE id='source'", [])?;
    fixture.conn().execute(
        "UPDATE messages SET status='streaming' WHERE id='assistant'",
        [],
    )?;
    let prepared = PreparedAbandonedRecovery::prepare(fixture.conn(), "source", 10)?;
    let wrong = crate::delegated_ownership::DelegatedOwnership::acquire(
        &fixture.root.join("fixture.db"),
        "other",
    )?;
    let owner = crate::delegated_ownership::DelegatedOwnership::acquire(
        &fixture.root.join("fixture.db"),
        "source",
    )?;
    for candidate in [None, Some(wrong.as_ref())] {
        let tx = fixture.conn().unchecked_transaction()?;
        assert!(prepared.install(&tx, "source", candidate).is_err());
        tx.rollback()?;
    }
    let tx = fixture.conn().unchecked_transaction()?;
    prepared.install(&tx, "source", Some(owner.as_ref()))?;
    tx.commit()?;
    assert_eq!(
        load_source(fixture.conn(), "source")?.messages[1].status,
        "interrupted"
    );
    Ok(())
}
