use super::*;
use crate::{Store, messages, threads};
use std::path::PathBuf;

struct OwnedDatabase(PathBuf);

impl OwnedDatabase {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "vega-runtime-diagnostic-migration-{}",
            ulid::Ulid::generate()
        ));
        std::fs::create_dir(&directory).expect("owned migration directory");
        Self(directory)
    }
}

impl Drop for OwnedDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn diagnostic(state: DiagnosticState) -> NewDiagnosticEvent {
    NewDiagnosticEvent {
        thread_id: "legacy-thread".into(),
        run_id: "legacy-assistant".into(),
        attempt_id: "legacy-attempt".into(),
        parent_attempt_id: None,
        tool_call_id: None,
        phase: DiagnosticPhase::Run,
        state,
        failure_code: (state == DiagnosticState::Failed)
            .then_some(DiagnosticFailureCode::UnknownSafeFailure),
        occurred_at: 1,
        duration_ms: (state != DiagnosticState::Started).then_some(1),
        metrics: DiagnosticMetrics {
            failure_source: (state == DiagnosticState::Failed).then(|| "runtime".into()),
            failure_kind: (state == DiagnosticState::Failed).then(|| "unknown".into()),
            ..Default::default()
        },
    }
}

#[test]
fn schema_upgrade_preserves_unknown_failures_and_rejects_untrusted_runtime_details() {
    let owned = OwnedDatabase::new();
    let database = owned.0.join("legacy.sqlite");
    let store = Store::open(&database).expect("legacy store");
    for (index, sql) in crate::MIGRATIONS.iter().take(27).enumerate() {
        let rebuilds_threads = index == 4;
        if rebuilds_threads {
            store
                .conn()
                .pragma_update(None, "foreign_keys", false)
                .expect("legacy rebuild foreign keys");
        }
        let transaction = store
            .conn()
            .unchecked_transaction()
            .expect("legacy migration");
        transaction.execute_batch(sql).expect("schema-27 migration");
        transaction
            .pragma_update(
                None,
                "user_version",
                i64::try_from(index + 1).expect("legacy version range"),
            )
            .expect("legacy version");
        transaction.commit().expect("legacy migration commit");
        if rebuilds_threads {
            store
                .conn()
                .pragma_update(None, "foreign_keys", true)
                .expect("legacy foreign keys restored");
        }
    }
    let version: i64 = store
        .conn()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("schema-27 version");
    assert_eq!(version, 27);
    threads::create_standalone(
        store.conn(),
        threads::NewThread {
            id: "legacy-thread",
            project_id: "",
            title: "migration survivor",
            mode: "execute",
            permission_mode: "confirm",
            model: "legacy-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .expect("legacy thread");
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "legacy-assistant".into(),
            thread_id: "legacy-thread".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "failed".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .expect("legacy assistant");
    store
        .conn()
        .execute_batch(
            "INSERT INTO run_diagnostic_events
             (thread_id,run_id,attempt_id,phase,state,occurred_at)
             VALUES('legacy-thread','legacy-assistant','legacy-attempt','run','started',1);
             INSERT INTO run_diagnostic_events
             (thread_id,run_id,attempt_id,phase,state,failure_code,occurred_at,duration_ms,
              failure_source,failure_kind)
             VALUES('legacy-thread','legacy-assistant','legacy-attempt','run','failed',
              'unknown_safe_failure',2,1,'runtime','unknown');",
        )
        .expect("legacy unknown failure rows");
    store.migrate().expect("upgrade legacy database");
    let columns = store
        .conn()
        .prepare("PRAGMA table_info(run_diagnostic_events)")
        .expect("diagnostic schema")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("diagnostic columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("diagnostic column names");
    for name in [
        "runtime_stage",
        "runtime_reason",
        "sqlite_code",
        "sqlite_extended_code",
        "io_kind",
    ] {
        assert!(
            columns.iter().any(|column| column == name),
            "missing {name}"
        );
    }
    let upgraded: i64 = store
        .conn()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("upgraded version");
    assert_eq!(
        upgraded,
        i64::try_from(crate::MIGRATIONS.len()).expect("current version range")
    );
    let export =
        export_run(store.conn(), "legacy-thread", "legacy-assistant").expect("legacy export");
    let json: serde_json::Value = serde_json::from_str(&export).expect("legacy JSON");
    let failed = &json["events"][1];
    assert_eq!(failed["failure_code"], "unknown_safe_failure");
    assert_eq!(failed["metrics"]["failure_kind"], "unknown");
    for field in [
        "runtime_stage",
        "runtime_reason",
        "sqlite_code",
        "sqlite_extended_code",
        "io_kind",
    ] {
        assert!(failed["metrics"][field].is_null());
    }
    let mut event = diagnostic(DiagnosticState::Started);
    event.attempt_id = "new-attempt".into();
    insert(store.conn(), &event).expect("new diagnostic start");
    event.state = DiagnosticState::Failed;
    event.failure_code = Some(DiagnosticFailureCode::UnknownSafeFailure);
    event.duration_ms = Some(1);
    let id = insert(store.conn(), &event).expect("new diagnostic failure");
    for invalid in [
        "runtime_stage='/private/untrusted-stage',runtime_reason='database'",
        "runtime_stage='tool_terminal',runtime_reason='/private/untrusted-reason'",
        "runtime_stage='tool_terminal',runtime_reason='database',sqlite_code=5,sqlite_extended_code=1811",
        "runtime_stage='tool_terminal',runtime_reason='database',sqlite_code=-1,sqlite_extended_code=-1",
        "runtime_stage='tool_terminal',runtime_reason='database',sqlite_code=5,sqlite_extended_code=NULL",
        "runtime_stage='tool_terminal',runtime_reason='approval_audit',sqlite_code=5,sqlite_extended_code=5",
        "runtime_stage='tool_terminal',runtime_reason='io',io_kind='/private/untrusted-io'",
    ] {
        let result = store.conn().execute(
            &format!("UPDATE run_diagnostic_events SET {invalid} WHERE id=?1"),
            [id],
        );
        assert!(
            result.is_err(),
            "untrusted diagnostic details must be rejected"
        );
    }
    store
        .conn()
        .execute(
            "UPDATE run_diagnostic_events
             SET runtime_stage='tool_terminal',runtime_reason='database',
                 sqlite_code=5,sqlite_extended_code=517
             WHERE id=?1",
            [id],
        )
        .expect("valid extended SQLite busy metadata");
    let reopened = Store::open(&database).expect("reopen upgraded database");
    reopened.migrate().expect("idempotent upgrade");
    let events = read_by_run(reopened.conn(), "legacy-thread", "legacy-assistant")
        .expect("validated migrated reads");
    let old = serde_json::to_value(&events[1].event.metrics).expect("old metrics");
    assert!(old["runtime_reason"].is_null());
    let current = serde_json::to_value(&events.last().expect("new failure").event.metrics)
        .expect("current metrics");
    assert_eq!(current["runtime_reason"], "database");
    assert_eq!(current["sqlite_code"], 5);
    assert_eq!(current["sqlite_extended_code"], 517);
    reopened
        .conn()
        .pragma_update(None, "ignore_check_constraints", true)
        .expect("owned corruption fixture");
    reopened
        .conn()
        .execute(
            "UPDATE run_diagnostic_events SET runtime_reason='/private/untrusted-reason' WHERE id=?1",
            [id],
        )
        .expect("bypass DDL only in owned corruption fixture");
    assert!(read_by_run(reopened.conn(), "legacy-thread", "legacy-assistant").is_err());
    assert!(export_run(reopened.conn(), "legacy-thread", "legacy-assistant").is_err());
    reopened
        .conn()
        .execute(
            "UPDATE run_diagnostic_events SET runtime_reason='database' WHERE id=?1",
            [id],
        )
        .expect("restore owned metadata");
    reopened
        .conn()
        .pragma_update(None, "ignore_check_constraints", false)
        .expect("restore DDL validation");
    let restored = classified_failure_message_ids(reopened.conn(), "legacy-thread")
        .expect("legacy failure ownership");
    assert!(restored.iter().any(|(owner, metrics)| {
        owner == "legacy-assistant" && metrics.failure_kind.as_deref() == Some("unknown")
    }));
    assert_eq!(
        threads::find(reopened.conn(), "legacy-thread")
            .expect("unrelated thread")
            .expect("preserved thread")
            .title,
        "migration survivor"
    );
    let final_export =
        export_run(reopened.conn(), "legacy-thread", "legacy-assistant").expect("upgraded export");
    assert!(!final_export.contains("/private/"));
}
