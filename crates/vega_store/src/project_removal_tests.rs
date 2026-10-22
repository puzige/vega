use std::collections::BTreeMap;

use rusqlite::{Connection, params, types::Value};

use crate::{MIGRATIONS, Store, codex_tasks, projects, threads};

const SESSION_STATES: [&str; 5] = [
    "absent",
    "intent",
    "confirmed",
    "uncertain",
    "definitively_failed",
];

struct Fixture {
    store: Store,
    removed: projects::Project,
    retained: projects::Project,
    task_ids: Vec<String>,
}

fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut statement = conn.prepare(sql).expect("prepare observation");
    let columns = statement.column_count();
    statement
        .query_map([], |row| (0..columns).map(|index| row.get(index)).collect())
        .expect("query observation")
        .collect::<Result<_, _>>()
        .expect("read observation")
}

fn database_rows(conn: &Connection) -> BTreeMap<String, Vec<Vec<Value>>> {
    let names = rows(
        conn,
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    );
    names
        .into_iter()
        .map(|name| {
            let Value::Text(name) = &name[0] else {
                panic!("table name is text");
            };
            let columns = conn
                .prepare(&format!("SELECT * FROM \"{name}\""))
                .expect("inspect observation columns")
                .column_count();
            let ordering = (1..=columns)
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            (
                name.clone(),
                rows(
                    conn,
                    &format!("SELECT * FROM \"{name}\" ORDER BY {ordering}"),
                ),
            )
        })
        .collect()
}

fn assert_foreign_keys(conn: &Connection) {
    assert_eq!(
        rows(conn, "PRAGMA foreign_keys"),
        vec![vec![Value::Integer(1)]]
    );
    assert!(rows(conn, "PRAGMA foreign_key_check").is_empty());
}

fn new_thread<'a>(id: &'a str, project_id: &'a str) -> threads::NewThread<'a> {
    threads::NewThread {
        id,
        project_id,
        title: "Preserved task title",
        mode: "plan",
        permission_mode: "readonly",
        model: "fixture-model",
        status: "active",
        pinned: true,
        unread: true,
        created_at: 101,
        updated_at: 202,
    }
}

fn snapshot(project_id: &str) -> codex_tasks::NewCodexTaskSnapshot {
    codex_tasks::NewCodexTaskSnapshot {
        profile_id: "historical-profile".into(),
        profile_display_name: "Historical profile".into(),
        adapter_kind: "codex_acp".into(),
        executable: "/synthetic/adapter-never-executed".into(),
        arguments: vec![
            vec!["flag".into(), "--fixture".into()],
            vec!["option".into(), "--model".into(), "fixture-model".into()],
        ],
        adapter_version: "1.2.3-fixture".into(),
        codex_version: "4.5.6".into(),
        model: Some("fixture-model".into()),
        model_provider: Some("fixture-provider".into()),
        reasoning_effort: Some("high".into()),
        sandbox_mode: "read_only".into(),
        approval_policy: "on_request".into(),
        selected_project_id: Some(project_id.into()),
        worktree_id: Some("historical-worktree".into()),
        canonical_working_directory: "/synthetic/project".into(),
        additional_directories: vec!["/synthetic/extra".into()],
    }
}

fn create_codex(conn: &Connection, id: &str, project_id: &str) {
    assert!(
        codex_tasks::materialize_codex_thread(
            conn,
            Some(project_id),
            new_thread(id, project_id),
            &snapshot(project_id),
            303,
        )
        .expect("materialize Codex task")
    );
}

fn set_session_state(conn: &Connection, id: &str, state: &str) {
    if state == "absent" {
        return;
    }
    let intent = format!("intent-{id}");
    assert!(codex_tasks::begin_session_creation(conn, id, &intent, 404).expect("begin session"));
    match state {
        "intent" => {}
        "confirmed" => assert!(
            codex_tasks::confirm_session_creation(conn, id, &intent, &format!("session-{id}"), 505)
                .expect("confirm session")
        ),
        "uncertain" => assert!(
            codex_tasks::mark_session_uncertain(conn, id, &intent, "outcome_unknown", 506)
                .expect("mark uncertain session")
        ),
        "definitively_failed" => assert!(
            codex_tasks::mark_session_definitively_failed(
                conn,
                id,
                &intent,
                "authentication_required",
                507,
            )
            .expect("mark failed session")
        ),
        _ => panic!("known fixture state"),
    }
}

fn populate_history(conn: &Connection, id: &str, position: usize) {
    let message_id = format!("message-{id}");
    conn.execute(
        "INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at, plan_status, plan_review_note, plan_reviewed_at) VALUES (?1, ?2, 1, 'assistant', 'plan', 'Preserved answer and tool history', 'done', 601, 'approved', 'Preserved review', 602)",
        params![message_id, id],
    )
    .expect("seed message");
    conn.execute(
        "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, output_text, output_full_path, status, approval, exit_code, duration_ms, created_at, finished_at, text_offset) VALUES (?1, ?2, ?3, 1, 'read', '{\"path\":\"fixture.txt\"}', 'Preserved tool output', '/synthetic/tool-output', 'success', 'once', 0, 17, 603, 604, 4)",
        params![format!("tool-{id}"), id, message_id],
    )
    .expect("seed tool");
    conn.execute(
        "INSERT INTO token_usage (thread_id, message_id, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_microcents, created_at, pricing_version, pricing_profile, call_started_at) VALUES (?1, ?2, 'fixture-model', 31, 13, 7, 5, 1234, 605, 'pricing_v1', 'base', 606)",
        params![id, message_id],
    )
    .expect("seed usage");
    conn.execute(
        "INSERT INTO sidebar_memberships (thread_id, group_id, position) VALUES (?1, 'preserved-group', ?2)",
        params![id, position as i64],
    )
    .expect("seed group membership");
    conn.execute(
        "INSERT INTO assistant_run_durations (message_id, execution_duration_ms) VALUES (?1, 123)",
        [&message_id],
    )
    .expect("seed duration");
    conn.execute(
        "INSERT INTO assistant_responses (message_id, response_id, text_start_bytes, text_end_bytes, kind) VALUES (?1, 'response', 0, 9, 'final')",
        [&message_id],
    )
    .expect("seed response");
    conn.execute(
        "INSERT INTO tool_call_rounds (message_id, call_id, response_id) VALUES (?1, ?2, 'response')",
        params![message_id, format!("tool-{id}")],
    )
    .expect("seed round");
    conn.execute(
        "INSERT INTO image_attachments (message_id, ordinal, encoded) VALUES (?1, 0, x'01020304')",
        [&message_id],
    )
    .expect("seed attachment");
}

fn fixture(schema_31: bool) -> Fixture {
    let store = Store::open(":memory:").expect("open owned in-memory store");
    if schema_31 {
        for (index, sql) in MIGRATIONS[..31].iter().enumerate() {
            if index == 4 {
                store
                    .conn()
                    .pragma_update(None, "foreign_keys", false)
                    .expect("legacy thread rebuild");
            }
            let tx = store
                .conn()
                .unchecked_transaction()
                .expect("legacy migration transaction");
            tx.execute_batch(sql).expect("apply legacy migration");
            tx.pragma_update(None, "user_version", index + 1)
                .expect("record legacy version");
            tx.commit().expect("commit legacy migration");
            if index == 4 {
                store
                    .conn()
                    .pragma_update(None, "foreign_keys", true)
                    .expect("restore legacy enforcement");
            }
        }
        assert_eq!(
            rows(store.conn(), "PRAGMA user_version"),
            vec![vec![Value::Integer(31)]]
        );
    } else {
        store.migrate().expect("initialize fresh schema");
    }
    let conn = store.conn();
    let removed = projects::create(conn, "/synthetic/project", "Removed project", Some("main"))
        .expect("create removed project");
    let retained = projects::create(conn, "/synthetic/retained", "Retained project", None)
        .expect("create retained project");
    conn.execute(
        "INSERT INTO sidebar_groups (id, name, color, position) VALUES ('preserved-group', 'Preserved group', 'Blue', 0)",
        [],
    )
    .expect("seed group");
    let mut task_ids = Vec::new();
    for (index, state) in SESSION_STATES.iter().enumerate() {
        let id = format!("codex-{state}");
        create_codex(conn, &id, &removed.id);
        set_session_state(conn, &id, state);
        if index % 2 == 0 {
            threads::set_status(conn, &id, "archived").expect("archive fixture task");
        }
        populate_history(conn, &id, index);
        task_ids.push(id);
    }
    threads::create(conn, new_thread("native-history", &removed.id)).expect("create Native task");
    populate_history(conn, "native-history", 5);
    task_ids.push("native-history".into());
    threads::create(conn, new_thread("retained-task", &retained.id))
        .expect("create unrelated task");
    populate_history(conn, "retained-task", 6);
    create_codex(conn, "retained-codex", &retained.id);
    set_session_state(conn, "retained-codex", "confirmed");
    assert_foreign_keys(conn);
    Fixture {
        store,
        removed,
        retained,
        task_ids,
    }
}

fn assert_removal_preserves_history(fixture: &Fixture) {
    let conn = fixture.store.conn();
    let mut expected = database_rows(conn);
    let project_column: usize = conn
        .query_row(
            "SELECT cid FROM pragma_table_info('threads') WHERE name = 'project_id'",
            [],
            |row| row.get(0),
        )
        .expect("find nullable binding column");
    for row in expected.get_mut("threads").expect("threads observation") {
        if row[project_column] == Value::Text(fixture.removed.id.clone()) {
            row[project_column] = Value::Null;
        }
    }
    expected
        .get_mut("projects")
        .expect("projects observation")
        .retain(|row| row[0] != Value::Text(fixture.removed.id.clone()));
    expected
        .get_mut("sidebar_project_order")
        .expect("order observation")
        .retain(|row| row[0] != Value::Text(fixture.removed.id.clone()));
    assert!(
        projects::remove(conn, &fixture.removed.id)
            .expect("remove project with full Codex history")
    );
    assert_eq!(
        database_rows(conn),
        expected,
        "only the registration, its order entry and current thread bindings may change"
    );
    assert_eq!(
        projects::find(conn, &fixture.retained.id).expect("find retained project"),
        Some(fixture.retained.clone())
    );
    for id in &fixture.task_ids {
        assert_eq!(
            conn.query_row(
                "SELECT project_id FROM threads WHERE id = ?1",
                [id],
                |row| row.get::<_, Option<String>>(0)
            )
            .expect("read nullable binding"),
            None,
        );
    }
    assert_foreign_keys(conn);
    assert!(!projects::remove(conn, &fixture.removed.id).expect("repeat project removal"));
    assert_eq!(database_rows(conn), expected);
}

#[test]
fn fresh_project_removal_preserves_native_and_every_codex_session_state() {
    let fixture = fixture(false);
    assert_removal_preserves_history(&fixture);
}

#[test]
fn populated_schema_31_upgrade_preserves_all_rows_and_removes_historical_project() {
    let fixture = fixture(true);
    let before = database_rows(fixture.store.conn());
    fixture
        .store
        .migrate()
        .expect("upgrade populated schema 31");
    assert_eq!(
        database_rows(fixture.store.conn()),
        before,
        "upgrade must preserve every populated field"
    );
    assert_foreign_keys(fixture.store.conn());
    fixture.store.migrate().expect("repeat upgrade");
    assert_eq!(database_rows(fixture.store.conn()), before);
    assert_removal_preserves_history(&fixture);
}

#[test]
fn rejected_project_delete_rolls_back_bindings_order_and_history() {
    let fixture = fixture(false);
    let conn = fixture.store.conn();
    conn.execute_batch(
        "CREATE TRIGGER reject_project_delete BEFORE DELETE ON projects BEGIN SELECT RAISE(ABORT, 'injected project deletion failure'); END;",
    )
    .expect("inject deletion failure");
    let before = database_rows(conn);
    let error = projects::remove(conn, &fixture.removed.id).expect_err("reject deletion");
    assert!(
        error
            .to_string()
            .contains("injected project deletion failure")
    );
    assert_eq!(database_rows(conn), before);
    assert!(conn.is_autocommit());
    assert_foreign_keys(conn);
    conn.execute_batch("DROP TRIGGER reject_project_delete")
        .expect("release injected failure");
    assert_removal_preserves_history(&fixture);
}

#[test]
fn empty_and_unknown_project_removal_are_idempotent() {
    let store = Store::open(":memory:").expect("open owned store");
    store.migrate().expect("initialize schema");
    let project = projects::create(store.conn(), "/synthetic/empty", "Empty", None)
        .expect("register empty project");
    assert!(projects::remove(store.conn(), &project.id).expect("remove empty project"));
    let before = database_rows(store.conn());
    assert!(!projects::remove(store.conn(), &project.id).expect("repeat empty removal"));
    assert!(!projects::remove(store.conn(), "unknown-project").expect("remove unknown project"));
    assert_eq!(database_rows(store.conn()), before);
    assert_foreign_keys(store.conn());
}

fn assert_constraints(fixture: &Fixture) {
    let conn = fixture.store.conn();
    let before = database_rows(conn);
    let columns = rows(
        conn,
        "SELECT name FROM pragma_table_info('codex_task_snapshots') ORDER BY cid",
    );
    for column in columns {
        let Value::Text(column) = &column[0] else {
            panic!("column name is text");
        };
        assert!(conn.execute(&format!("UPDATE codex_task_snapshots SET {column} = {column} WHERE thread_id = 'codex-absent'"), []).is_err(), "snapshot column {column} must stay immutable");
    }
    assert!(
        conn.execute(
            "UPDATE threads SET backend = 'native' WHERE id = 'codex-absent'",
            []
        )
        .is_err()
    );
    assert!(conn.execute("UPDATE codex_session_creations SET state = 'confirmed', session_id = 'forged' WHERE thread_id = 'codex-absent'", []).is_err());
    assert!(conn.execute("UPDATE codex_session_creations SET state = 'intent', intent_id = 'retry' WHERE thread_id = 'codex-uncertain'", []).is_err());
    assert!(conn.execute("UPDATE codex_session_creations SET state = 'intent', intent_id = 'retry' WHERE thread_id = 'codex-definitively_failed'", []).is_err());
    assert!(conn.execute("UPDATE codex_session_creations SET state = 'confirmed', session_id = 'changed' WHERE thread_id = 'codex-confirmed'", []).is_err());
    assert!(conn.execute("UPDATE codex_session_creations SET state = 'confirmed', session_id = 'session-codex-confirmed', updated_at = 600 WHERE thread_id = 'codex-intent'", []).is_err());
    assert!(conn.execute("UPDATE codex_session_creations SET updated_at = 1, state = 'uncertain', uncertainty_code = 'outcome_unknown' WHERE thread_id = 'codex-intent'", []).is_err());
    assert_eq!(database_rows(conn), before);
    create_codex(conn, "new-session-task", &fixture.retained.id);
    assert_eq!(
        codex_tasks::find_session_creation(conn, "new-session-task")
            .expect("read initial session")
            .expect("initial row")
            .state,
        "absent"
    );
    assert!(
        codex_tasks::begin_session_creation(conn, "new-session-task", "new-intent", 700)
            .expect("new intent")
    );
    assert!(
        !codex_tasks::confirm_session_creation(
            conn,
            "new-session-task",
            "wrong-intent",
            "new-session",
            701
        )
        .expect("reject wrong intent")
    );
    assert!(
        codex_tasks::confirm_session_creation(
            conn,
            "new-session-task",
            "new-intent",
            "new-session",
            702
        )
        .expect("confirm new session")
    );
    assert_eq!(
        codex_tasks::find_prompt_eligible_session(conn, "new-session-task")
            .expect("prompt session"),
        Some("new-session".into())
    );
    assert_foreign_keys(conn);
}

#[test]
fn fresh_and_upgraded_schemas_keep_snapshot_and_session_constraints() {
    for legacy in [false, true] {
        let fixture = fixture(legacy);
        fixture.store.migrate().expect("migrate fixture");
        assert_constraints(&fixture);
    }
}

#[test]
fn removed_history_remains_immutable_and_task_deletion_cascades() {
    let fixture = fixture(true);
    fixture.store.migrate().expect("upgrade schema");
    assert_removal_preserves_history(&fixture);
    let conn = fixture.store.conn();
    assert!(conn.execute("UPDATE codex_task_snapshots SET selected_project_id = NULL WHERE thread_id = 'codex-confirmed'", []).is_err());
    assert_eq!(
        threads::delete_thread(conn, "codex-confirmed").expect("delete historical task"),
        1
    );
    assert!(
        codex_tasks::find_task_snapshot(conn, "codex-confirmed")
            .expect("find removed snapshot")
            .is_none()
    );
    assert!(
        codex_tasks::find_session_creation(conn, "codex-confirmed")
            .expect("find removed session")
            .is_none()
    );
    assert!(
        codex_tasks::find_task_snapshot(conn, "codex-uncertain")
            .expect("find preserved sibling snapshot")
            .is_some()
    );
    assert_foreign_keys(conn);
}

#[test]
fn new_codex_bindings_reject_missing_or_mismatched_projects_atomically() {
    let fixture = fixture(false);
    let conn = fixture.store.conn();
    let before = database_rows(conn);
    assert!(
        !codex_tasks::materialize_codex_thread(
            conn,
            Some(&fixture.retained.id),
            new_thread("mismatched", &fixture.retained.id),
            &snapshot(&fixture.removed.id),
            800
        )
        .expect("reject mismatched snapshot")
    );
    assert!(
        codex_tasks::materialize_codex_thread(
            conn,
            Some("missing-project"),
            new_thread("missing-project-task", "missing-project"),
            &snapshot("missing-project"),
            800
        )
        .is_err()
    );
    assert!(
        !codex_tasks::bind_to_materialized_thread(
            conn,
            "retained-task",
            &snapshot(&fixture.removed.id),
            800
        )
        .expect("reject mismatched bind")
    );
    assert_eq!(database_rows(conn), before);
    assert_foreign_keys(conn);
}
