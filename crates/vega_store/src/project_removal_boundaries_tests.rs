use std::collections::BTreeMap;

use rusqlite::{Connection, params, types::Value};

use crate::{MIGRATIONS, Store, codex_tasks, projects, threads};

type RawRows = Vec<Vec<Value>>;

struct Fixture {
    store: Store,
    project: projects::Project,
    other: projects::Project,
}

fn rows(conn: &Connection, sql: &str) -> RawRows {
    let mut statement = conn.prepare(sql).expect("prepare raw observation");
    let width = statement.column_count();
    statement
        .query_map([], |row| {
            (0..width)
                .map(|column| row.get::<_, Value>(column))
                .collect::<Result<Vec<_>, _>>()
        })
        .expect("query raw observation")
        .collect::<Result<Vec<_>, _>>()
        .expect("read raw values")
}

fn table_values(conn: &Connection) -> BTreeMap<String, RawRows> {
    [
        ("projects", "id"),
        ("threads", "id"),
        ("sidebar_project_order", "project_id"),
        ("codex_task_snapshots", "thread_id"),
        ("codex_session_creations", "thread_id"),
    ]
    .into_iter()
    .map(|(table, key)| {
        (
            table.to_string(),
            rows(conn, &format!("SELECT * FROM {table} ORDER BY {key}")),
        )
    })
    .collect()
}

fn schema_objects(conn: &Connection) -> RawRows {
    rows(
        conn,
        "SELECT 'main', type, name, tbl_name, sql FROM sqlite_master \
         UNION ALL SELECT 'temp', type, name, tbl_name, sql FROM sqlite_temp_master \
         ORDER BY 1, 2, 3",
    )
}

fn assert_constraint(error: rusqlite::Error, extended_code: i32) {
    match error {
        rusqlite::Error::SqliteFailure(code, message) => {
            assert_eq!(code.extended_code, extended_code, "{message:?}");
        }
        other => panic!("expected SQLite constraint failure, got {other:?}"),
    }
}

fn assert_enforced_and_idle(conn: &Connection) {
    assert_eq!(
        rows(conn, "PRAGMA foreign_keys"),
        vec![vec![Value::Integer(1)]]
    );
    assert!(conn.is_autocommit());
    assert!(rows(conn, "PRAGMA foreign_key_check").is_empty());
}

fn schema_31_store() -> Store {
    let store = Store::open(":memory:").expect("open owned in-memory database");
    assert!(MIGRATIONS.len() >= 31);
    for (index, sql) in MIGRATIONS.iter().take(31).enumerate() {
        let target = (index + 1) as u32;
        if target == 5 {
            store
                .conn()
                .pragma_update(None, "foreign_keys", false)
                .expect("legacy thread rebuild exception");
        }
        let result = (|| -> Result<(), rusqlite::Error> {
            let transaction = store.conn().unchecked_transaction()?;
            transaction.execute_batch(sql)?;
            transaction.pragma_update(None, "user_version", target)?;
            transaction.commit()
        })();
        if target == 5 {
            store
                .conn()
                .pragma_update(None, "foreign_keys", true)
                .expect("restore legacy foreign-key enforcement");
        }
        result.expect("apply original schema migration");
    }
    assert_eq!(
        rows(store.conn(), "PRAGMA user_version"),
        vec![vec![Value::Integer(31)]]
    );
    assert_enforced_and_idle(store.conn());
    store
}

fn new_thread<'a>(id: &'a str, project_id: &'a str) -> threads::NewThread<'a> {
    threads::NewThread {
        id,
        project_id,
        title: "Boundary history",
        mode: "execute",
        permission_mode: "readonly",
        model: "fixture-model",
        status: "archived",
        pinned: true,
        unread: false,
        created_at: 101,
        updated_at: 303,
    }
}

fn snapshot(project_id: Option<&str>) -> codex_tasks::NewCodexTaskSnapshot {
    codex_tasks::NewCodexTaskSnapshot {
        profile_id: "boundary-profile".into(),
        profile_display_name: "Boundary profile".into(),
        adapter_kind: "codex_acp".into(),
        executable: "/fixture/adapter-never-executed".into(),
        arguments: vec![vec!["flag".into(), "--fixture".into()]],
        adapter_version: "1.2.3".into(),
        codex_version: "4.5.6".into(),
        model: None,
        model_provider: None,
        reasoning_effort: None,
        sandbox_mode: "read_only".into(),
        approval_policy: "on_request".into(),
        selected_project_id: project_id.map(str::to_string),
        worktree_id: Some("boundary-worktree".into()),
        canonical_working_directory: "/fixture/worktree".into(),
        additional_directories: vec!["/fixture/additional".into()],
    }
}

fn insert_raw_snapshot(
    conn: &Connection,
    thread_id: &str,
    snapshot: &codex_tasks::NewCodexTaskSnapshot,
    created_at: i64,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "INSERT INTO codex_task_snapshots \
         (thread_id, profile_id, profile_display_name, adapter_kind, executable, arguments_json, \
          adapter_version, codex_version, model, model_provider, reasoning_effort, sandbox_mode, \
          approval_policy, selected_project_id, worktree_id, canonical_working_directory, \
          additional_directories_json, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            thread_id,
            snapshot.profile_id,
            snapshot.profile_display_name,
            snapshot.adapter_kind,
            snapshot.executable,
            "[ [ \"flag\", \"--fixture\" ], [ \"option\", \"--model\", \"fixture-model\" ] ]",
            snapshot.adapter_version,
            snapshot.codex_version,
            snapshot.model,
            snapshot.model_provider,
            snapshot.reasoning_effort,
            snapshot.sandbox_mode,
            snapshot.approval_policy,
            snapshot.selected_project_id,
            snapshot.worktree_id,
            snapshot.canonical_working_directory,
            "[ \"/fixture/additional\" ]",
            created_at,
        ],
    )
}

fn prepare_thread(conn: &Connection, id: &str, project_id: Option<&str>, codex: bool) {
    let thread = new_thread(id, project_id.unwrap_or_default());
    if project_id.is_some() {
        threads::create(conn, thread).expect("create project-bound fixture thread");
    } else {
        threads::create_standalone(conn, thread).expect("create standalone fixture thread");
    }
    if codex {
        assert_eq!(
            conn.execute("UPDATE threads SET backend = 'codex' WHERE id = ?1", [id])
                .expect("seed Codex backend without snapshot"),
            1
        );
    }
}

fn fixture(schema_31: bool) -> Fixture {
    let store = if schema_31 {
        schema_31_store()
    } else {
        let store = Store::open(":memory:").expect("open fresh owned store");
        store.migrate().expect("initialize current schema");
        store
    };
    let conn = store.conn();
    let project = projects::create(conn, "/fixture/project", "Historical project", Some("main"))
        .expect("register historical project");
    let other = projects::create(conn, "/fixture/other", "Other project", None)
        .expect("register independent project");
    for (index, state) in [
        "absent",
        "intent",
        "confirmed",
        "uncertain",
        "definitively_failed",
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("history-{state}");
        prepare_thread(conn, &id, Some(&project.id), true);
        let mut snapshot = snapshot(Some(&project.id));
        if index % 2 == 0 {
            snapshot.model = Some("fixture-model".into());
            snapshot.model_provider = Some("fixture-provider".into());
            snapshot.reasoning_effort = Some("high".into());
        }
        if index % 2 == 1 {
            snapshot.worktree_id = None;
        }
        insert_raw_snapshot(conn, &id, &snapshot, 1_000 + index as i64)
            .expect("seed whitespace-preserving raw snapshot");
        if state != "absent" {
            let intent_id = format!("intent-{id}");
            assert!(
                codex_tasks::begin_session_creation(conn, &id, &intent_id, 10_000)
                    .expect("seed session intent")
            );
            let changed = match state {
                "intent" => true,
                "confirmed" => codex_tasks::confirm_session_creation(
                    conn,
                    &id,
                    &intent_id,
                    "fixture-confirmed-session",
                    20_000,
                )
                .expect("seed confirmed session"),
                "uncertain" => codex_tasks::mark_session_uncertain(
                    conn,
                    &id,
                    &intent_id,
                    "outcome_unknown",
                    20_000,
                )
                .expect("seed uncertain session"),
                "definitively_failed" => codex_tasks::mark_session_definitively_failed(
                    conn,
                    &id,
                    &intent_id,
                    "authentication_required",
                    20_000,
                )
                .expect("seed failed session"),
                _ => panic!("known fixture state"),
            };
            assert!(changed);
        }
    }
    prepare_thread(conn, "history-worktree-only", None, true);
    insert_raw_snapshot(conn, "history-worktree-only", &snapshot(None), 1_999)
        .expect("seed NULL project and non-NULL worktree");
    prepare_thread(conn, "native-history", Some(&project.id), false);
    prepare_thread(conn, "retained-native", Some(&other.id), false);
    assert_enforced_and_idle(conn);
    Fixture {
        store,
        project,
        other,
    }
}

fn current_fixture(upgrade: bool) -> Fixture {
    let fixture = fixture(upgrade);
    if upgrade {
        fixture
            .store
            .migrate()
            .expect("upgrade populated schema 31");
    }
    fixture
}

#[test]
fn migration_foreign_key_failure_restores_schema_and_raw_history_before_retry() {
    let fixture = fixture(true);
    let conn = fixture.store.conn();
    conn.execute_batch(
        "CREATE TABLE migration_drop_guard (\
             thread_id TEXT PRIMARY KEY REFERENCES codex_session_creations(thread_id) ON DELETE RESTRICT\
         ); \
         INSERT INTO migration_drop_guard(thread_id) VALUES ('history-confirmed');",
    )
    .expect("inject owned migration retirement guard");
    let original_schema = schema_objects(conn);
    let original_values = table_values(conn);
    let original_guard = rows(
        conn,
        "SELECT * FROM migration_drop_guard ORDER BY thread_id",
    );
    assert_enforced_and_idle(conn);

    let error = fixture
        .store
        .migrate()
        .expect_err("session-table retirement must encounter the injected foreign-key guard");
    match error {
        rusqlite::Error::SqliteFailure(code, message) => {
            assert_eq!(code.code, rusqlite::ErrorCode::ConstraintViolation);
            assert_eq!(message.as_deref(), Some("FOREIGN KEY constraint failed"));
        }
        other => panic!("expected foreign-key retirement failure, got {other:?}"),
    }
    assert_eq!(schema_objects(conn), original_schema);
    assert_eq!(table_values(conn), original_values);
    assert_eq!(
        rows(
            conn,
            "SELECT * FROM migration_drop_guard ORDER BY thread_id"
        ),
        original_guard
    );
    assert_eq!(
        rows(conn, "PRAGMA user_version"),
        vec![vec![Value::Integer(31)]]
    );
    assert_enforced_and_idle(conn);

    conn.execute_batch("DROP TABLE migration_drop_guard")
        .expect("remove owned fault injection");
    fixture.store.migrate().expect("retry real migration");
    assert_eq!(
        rows(conn, "PRAGMA user_version"),
        vec![vec![Value::Integer(32)]]
    );
    assert_eq!(table_values(conn), original_values);
    let migrated_schema = schema_objects(conn);
    fixture
        .store
        .migrate()
        .expect("migration remains idempotent");
    assert_eq!(schema_objects(conn), migrated_schema);
    assert_eq!(table_values(conn), original_values);
    assert_enforced_and_idle(conn);

    assert!(
        projects::remove(conn, &fixture.project.id).expect("remove migrated historical project")
    );
    assert!(
        projects::find(conn, &fixture.project.id)
            .expect("find removed project")
            .is_none()
    );
    assert_eq!(
        rows(
            conn,
            "SELECT * FROM codex_task_snapshots ORDER BY thread_id"
        ),
        original_values["codex_task_snapshots"]
    );
    assert_eq!(
        rows(
            conn,
            "SELECT * FROM codex_session_creations ORDER BY thread_id"
        ),
        original_values["codex_session_creations"]
    );
    assert_eq!(
        rows(
            conn,
            "SELECT COUNT(*) FROM threads WHERE id <> 'retained-native' AND project_id IS NOT NULL",
        ),
        vec![vec![Value::Integer(0)]]
    );
    assert_eq!(
        threads::find(conn, "retained-native")
            .expect("load unrelated thread")
            .expect("unrelated thread retained")
            .project_id,
        fixture.other.id
    );
    assert_enforced_and_idle(conn);
}

#[test]
fn fresh_and_upgraded_snapshot_inserts_require_codex_and_null_safe_live_project_binding() {
    for upgrade in [false, true] {
        let fixture = current_fixture(upgrade);
        let conn = fixture.store.conn();
        for (id, thread_project, snapshot_project, codex) in [
            (
                "native-probe",
                Some(fixture.project.id.as_str()),
                Some(fixture.project.id.as_str()),
                false,
            ),
            (
                "other-project-probe",
                Some(fixture.other.id.as_str()),
                Some(fixture.project.id.as_str()),
                true,
            ),
            (
                "null-thread-probe",
                None,
                Some(fixture.project.id.as_str()),
                true,
            ),
            (
                "null-snapshot-probe",
                Some(fixture.project.id.as_str()),
                None,
                true,
            ),
        ] {
            prepare_thread(conn, id, thread_project, codex);
            let before = table_values(conn);
            let error = insert_raw_snapshot(conn, id, &snapshot(snapshot_project), 30_000)
                .expect_err("invalid new snapshot must be rejected before creating a session");
            assert_constraint(error, rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER);
            assert_eq!(table_values(conn), before, "probe {id}, upgrade={upgrade}");
            assert!(
                codex_tasks::find_task_snapshot(conn, id)
                    .expect("snapshot observation")
                    .is_none()
            );
            assert!(
                codex_tasks::find_session_creation(conn, id)
                    .expect("session observation")
                    .is_none()
            );
        }

        let transaction = conn
            .unchecked_transaction()
            .expect("owned deferred-FK fixture");
        transaction
            .pragma_update(None, "defer_foreign_keys", true)
            .expect("defer only the owned thread fixture reference");
        prepare_thread(
            &transaction,
            "missing-project-probe",
            Some("missing-project"),
            true,
        );
        let before = table_values(&transaction);
        let error = insert_raw_snapshot(
            &transaction,
            "missing-project-probe",
            &snapshot(Some("missing-project")),
            30_000,
        )
        .expect_err(
            "matching but nonexistent project must be rejected by the snapshot INSERT guard",
        );
        assert_constraint(error, rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER);
        assert_eq!(table_values(&transaction), before);
        assert!(
            codex_tasks::find_task_snapshot(&transaction, "missing-project-probe")
                .expect("missing-project snapshot observation")
                .is_none()
        );
        assert!(
            codex_tasks::find_session_creation(&transaction, "missing-project-probe")
                .expect("missing-project session observation")
                .is_none()
        );
        transaction
            .rollback()
            .expect("discard the otherwise matching missing-project fixture");

        for (id, project_id) in [
            (
                "matching-project-control",
                Some(fixture.project.id.as_str()),
            ),
            ("worktree-only-control", None),
        ] {
            prepare_thread(conn, id, project_id, true);
            assert_eq!(
                insert_raw_snapshot(conn, id, &snapshot(project_id), 30_000)
                    .expect("valid snapshot INSERT"),
                1
            );
            let stored = codex_tasks::find_task_snapshot(conn, id)
                .expect("valid snapshot observation")
                .expect("snapshot persisted");
            assert_eq!(stored.selected_project_id.as_deref(), project_id);
            assert_eq!(stored.worktree_id.as_deref(), Some("boundary-worktree"));
            assert_eq!(
                codex_tasks::find_session_creation(conn, id).expect("initial session observation"),
                Some(codex_tasks::CodexSessionCreationRow {
                    state: "absent".into(),
                    intent_id: None,
                    session_id: None,
                    uncertainty_code: None,
                    failure_code: None,
                    updated_at: 30_000,
                })
            );
        }
        assert_enforced_and_idle(conn);
    }
}

#[test]
fn snapshot_check_failure_rolls_back_real_bind_and_materialize_writes() {
    for upgrade in [false, true] {
        let fixture = current_fixture(upgrade);
        let conn = fixture.store.conn();
        let valid = snapshot(Some(&fixture.project.id));
        let mut invalid = valid.clone();
        invalid.profile_id = "invalid profile".into();
        prepare_thread(conn, "bind-rollback", Some(&fixture.project.id), false);
        let original = table_values(conn);
        let error =
            codex_tasks::bind_to_materialized_thread(conn, "bind-rollback", &invalid, 30_000)
                .expect_err("snapshot CHECK must fail after the real backend update");
        assert_constraint(error, rusqlite::ffi::SQLITE_CONSTRAINT_CHECK);
        assert_eq!(table_values(conn), original);
        assert_eq!(
            threads::find(conn, "bind-rollback")
                .expect("rolled-back thread")
                .expect("original thread retained")
                .backend,
            "native"
        );
        assert!(
            codex_tasks::find_task_snapshot(conn, "bind-rollback")
                .expect("rolled-back snapshot")
                .is_none()
        );
        assert!(
            codex_tasks::find_session_creation(conn, "bind-rollback")
                .expect("rolled-back session")
                .is_none()
        );
        assert_enforced_and_idle(conn);
        assert!(
            codex_tasks::bind_to_materialized_thread(conn, "bind-rollback", &valid, 30_000)
                .expect("valid bind control")
        );

        let original = table_values(conn);
        let error = codex_tasks::materialize_codex_thread(
            conn,
            Some(&fixture.project.id),
            new_thread("materialize-rollback", &fixture.project.id),
            &invalid,
            30_000,
        )
        .expect_err("snapshot CHECK must fail after the real thread INSERT");
        assert_constraint(error, rusqlite::ffi::SQLITE_CONSTRAINT_CHECK);
        assert_eq!(table_values(conn), original);
        assert!(
            threads::find(conn, "materialize-rollback")
                .expect("rolled-back materialization")
                .is_none()
        );
        assert!(
            codex_tasks::find_task_snapshot(conn, "materialize-rollback")
                .expect("materialization snapshot")
                .is_none()
        );
        assert!(
            codex_tasks::find_session_creation(conn, "materialize-rollback")
                .expect("materialization session")
                .is_none()
        );
        assert_enforced_and_idle(conn);
        assert!(
            codex_tasks::materialize_codex_thread(
                conn,
                Some(&fixture.project.id),
                new_thread("materialize-rollback", &fixture.project.id),
                &valid,
                30_000
            )
            .expect("valid materialization control")
        );
        assert_enforced_and_idle(conn);
    }
}

#[test]
fn fresh_and_upgraded_terminal_sessions_reject_otherwise_valid_restart_intents() {
    for upgrade in [false, true] {
        let fixture = current_fixture(upgrade);
        let conn = fixture.store.conn();
        for state in ["confirmed", "uncertain", "definitively_failed"] {
            let id = format!("history-{state}");
            let original = table_values(conn);
            let error = conn.execute(
                "UPDATE codex_session_creations SET state = 'intent', intent_id = ?1, \
                 session_id = NULL, uncertainty_code = NULL, failure_code = NULL, updated_at = 40_000 \
                 WHERE thread_id = ?2",
                params![format!("restart-{state}"), id],
            ).expect_err("terminal-to-intent must fail independently of row CHECK constraints");
            assert_constraint(error, rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER);
            assert_eq!(
                table_values(conn),
                original,
                "state {state}, upgrade={upgrade}"
            );
        }
        assert_enforced_and_idle(conn);
    }
}

#[test]
fn fresh_and_upgraded_schemas_preserve_intent_uniqueness_insert_checks_and_profile_index() {
    for upgrade in [false, true] {
        let fixture = current_fixture(upgrade);
        let conn = fixture.store.conn();
        let original = table_values(conn);
        let error = codex_tasks::begin_session_creation(
            conn,
            "history-absent",
            "intent-history-intent",
            40_000,
        )
        .expect_err("duplicate intent must violate uniqueness");
        assert_constraint(error, rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE);
        assert_eq!(table_values(conn), original);
        assert!(
            codex_tasks::begin_session_creation(
                conn,
                "history-absent",
                "unique-intent-control",
                40_000
            )
            .expect("distinct intent control")
        );

        prepare_thread(
            conn,
            "snapshot-check-probe",
            Some(&fixture.project.id),
            true,
        );
        let mut invalid = snapshot(Some(&fixture.project.id));
        invalid.sandbox_mode = "unconfined".into();
        let original = table_values(conn);
        let error = insert_raw_snapshot(conn, "snapshot-check-probe", &invalid, 30_000)
            .expect_err("snapshot INSERT must preserve sandbox CHECK");
        assert_constraint(error, rusqlite::ffi::SQLITE_CONSTRAINT_CHECK);
        assert_eq!(table_values(conn), original);
        insert_raw_snapshot(
            conn,
            "snapshot-check-probe",
            &snapshot(Some(&fixture.project.id)),
            30_000,
        )
        .expect("valid snapshot INSERT control");
        conn.execute(
            "DELETE FROM codex_session_creations WHERE thread_id = 'snapshot-check-probe'",
            [],
        )
        .expect("prepare owned initial-session INSERT probe");
        let original = table_values(conn);
        let error = conn.execute(
            "INSERT INTO codex_session_creations \
             (thread_id, state, intent_id, session_id, uncertainty_code, failure_code, updated_at) \
             VALUES ('snapshot-check-probe', 'confirmed', 'insert-check-intent', NULL, NULL, NULL, 40_000)",
            [],
        ).expect_err("confirmed session INSERT requires a non-NULL session id");
        assert_constraint(error, rusqlite::ffi::SQLITE_CONSTRAINT_CHECK);
        assert_eq!(table_values(conn), original);
        conn.execute(
            "INSERT INTO codex_session_creations \
             (thread_id, state, intent_id, session_id, uncertainty_code, failure_code, updated_at) \
             VALUES ('snapshot-check-probe', 'confirmed', 'insert-check-intent', 'insert-check-session', NULL, NULL, 40_000)",
            [],
        ).expect("CHECK-valid session INSERT control");

        assert_eq!(
            rows(
                conn,
                "SELECT \"unique\", origin, partial FROM pragma_index_list('codex_task_snapshots') WHERE name = 'idx_codex_task_profile'"
            ),
            vec![vec![
                Value::Integer(0),
                Value::Text("c".into()),
                Value::Integer(0)
            ]]
        );
        assert_eq!(
            rows(
                conn,
                "SELECT name FROM pragma_index_info('idx_codex_task_profile') ORDER BY seqno"
            ),
            vec![
                vec![Value::Text("profile_id".into())],
                vec![Value::Text("thread_id".into())]
            ]
        );
        assert_enforced_and_idle(conn);
    }
}
