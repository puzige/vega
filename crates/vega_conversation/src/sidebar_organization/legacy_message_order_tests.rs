use super::{apply, snapshot, sorted_threads};
use crate::types::{
    SidebarGroupColor, SidebarOrganizationAction, SidebarOrganizationSnapshot, SidebarProjectView,
    SidebarView, ThreadStatus,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};
use vega_store::{Connection, Store, messages, projects, sidebar_organization as sql, threads};

const LEGACY_MIGRATIONS: [&str; 27] = [
    include_str!("../../../vega_store/migrations/0001_init.sql"),
    include_str!("../../../vega_store/migrations/0002_plan_review.sql"),
    include_str!("../../../vega_store/migrations/0003_token_usage_pricing.sql"),
    include_str!("../../../vega_store/migrations/0004_sidebar_organization.sql"),
    include_str!("../../../vega_store/migrations/0005_standalone_threads.sql"),
    include_str!("../../../vega_store/migrations/0006_tool_text_offset.sql"),
    include_str!("../../../vega_store/migrations/0007_image_attachments.sql"),
    include_str!("../../../vega_store/migrations/0008_auto_titles.sql"),
    include_str!("../../../vega_store/migrations/0009_context_compaction.sql"),
    include_str!("../../../vega_store/migrations/0010_context_compaction_status.sql"),
    include_str!("../../../vega_store/migrations/0011_model_context_policies.sql"),
    include_str!("../../../vega_store/migrations/0012_mcp_servers.sql"),
    include_str!("../../../vega_store/migrations/0013_skills.sql"),
    include_str!("../../../vega_store/migrations/0014_execution_duration.sql"),
    include_str!("../../../vega_store/migrations/0015_run_diagnostics.sql"),
    include_str!("../../../vega_store/migrations/0016_codex_task_identity.sql"),
    include_str!("../../../vega_store/migrations/0017_assistant_responses.sql"),
    include_str!("../../../vega_store/migrations/0018_computer_use_delivery.sql"),
    include_str!("../../../vega_store/migrations/0019_pending_messages.sql"),
    include_str!("../../../vega_store/migrations/0020_context_compaction_timeout.sql"),
    include_str!("../../../vega_store/migrations/0021_tool_call_rounds.sql"),
    include_str!("../../../vega_store/migrations/0022_partial_compaction.sql"),
    include_str!("../../../vega_store/migrations/0023_credential_continuations.sql"),
    include_str!("../../../vega_store/migrations/0024_credential_privacy.sql"),
    include_str!("../../../vega_store/migrations/0025_failure_details.sql"),
    include_str!("../../../vega_store/migrations/0026_automation.sql"),
    include_str!("../../../vega_store/migrations/0027_subagents.sql"),
];

struct ScenarioOrder {
    regions: [&'static [&'static str]; 4],
    visible_regions: [&'static [&'static str]; 4],
    promoted_project: &'static [&'static str],
    promoted_visible_project: &'static [&'static str],
    dragged_project: &'static [&'static str],
    dragged_visible_project: &'static [&'static str],
}

const UPDATED_ORDER: ScenarioOrder = ScenarioOrder {
    regions: [
        &["pin-a", "pin-archived", "pin-d", "pin-b", "pin-c"],
        &["p-a", "p-archived", "p-d", "p-b", "p-c"],
        &["q-a", "q-archived", "q-d", "q-b", "q-c"],
        &["r-a", "r-archived", "r-d", "r-b", "r-c"],
    ],
    visible_regions: [
        &["pin-a", "pin-d", "pin-b", "pin-c"],
        &["p-a", "p-d", "p-b", "p-c"],
        &["q-a", "q-d", "q-b", "q-c"],
        &["r-a", "r-d", "r-b", "r-c"],
    ],
    promoted_project: &["p-b", "p-a", "p-archived", "p-d", "p-c"],
    promoted_visible_project: &["p-b", "p-a", "p-d", "p-c"],
    dragged_project: &["p-a", "p-archived", "p-d", "p-c", "p-b"],
    dragged_visible_project: &["p-a", "p-d", "p-c", "p-b"],
};

const CREATED_ORDER: ScenarioOrder = ScenarioOrder {
    regions: [
        &["pin-c", "pin-b", "pin-archived", "pin-a", "pin-d"],
        &["p-c", "p-b", "p-archived", "p-a", "p-d"],
        &["q-c", "q-b", "q-archived", "q-a", "q-d"],
        &["r-c", "r-b", "r-archived", "r-a", "r-d"],
    ],
    visible_regions: [
        &["pin-c", "pin-b", "pin-a", "pin-d"],
        &["p-c", "p-b", "p-a", "p-d"],
        &["q-c", "q-b", "q-a", "q-d"],
        &["r-c", "r-b", "r-a", "r-d"],
    ],
    promoted_project: &["p-b", "p-c", "p-archived", "p-a", "p-d"],
    promoted_visible_project: &["p-b", "p-c", "p-a", "p-d"],
    dragged_project: &["p-c", "p-archived", "p-a", "p-d", "p-b"],
    dragged_visible_project: &["p-c", "p-a", "p-d", "p-b"],
};

const MANUAL_ORDER: ScenarioOrder = ScenarioOrder {
    regions: [
        &["pin-d", "pin-b", "pin-c", "pin-archived", "pin-a"],
        &["p-d", "p-b", "p-c", "p-archived", "p-a"],
        &["q-d", "q-b", "q-c", "q-archived", "q-a"],
        &["r-d", "r-b", "r-c", "r-archived", "r-a"],
    ],
    visible_regions: [
        &["pin-d", "pin-b", "pin-c", "pin-a"],
        &["p-d", "p-b", "p-c", "p-a"],
        &["q-d", "q-b", "q-c", "q-a"],
        &["r-d", "r-b", "r-c", "r-a"],
    ],
    promoted_project: &["p-b", "p-d", "p-c", "p-archived", "p-a"],
    promoted_visible_project: &["p-b", "p-d", "p-c", "p-a"],
    dragged_project: &["p-d", "p-c", "p-archived", "p-a", "p-b"],
    dragged_visible_project: &["p-d", "p-c", "p-a", "p-b"],
};

#[derive(Debug, PartialEq)]
struct TableSnapshot {
    name: &'static str,
    rows: Vec<Vec<String>>,
}

fn table_snapshot(conn: &Connection, name: &'static str, sql: &str) -> TableSnapshot {
    let mut statement = conn.prepare(sql).unwrap();
    let column_count = statement.column_count();
    let rows = statement
        .query_map([], |row| {
            (0..column_count)
                .map(|column| row.get_ref(column).map(|value| format!("{value:?}")))
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    TableSnapshot { name, rows }
}

fn preserved_rows(conn: &Connection) -> Vec<TableSnapshot> {
    [
        ("projects", "SELECT * FROM projects ORDER BY id"),
        ("threads", "SELECT * FROM threads ORDER BY id"),
        ("messages", "SELECT * FROM messages ORDER BY id"),
        (
            "sidebar_groups",
            "SELECT * FROM sidebar_groups ORDER BY position,id",
        ),
        (
            "sidebar_memberships",
            "SELECT * FROM sidebar_memberships ORDER BY group_id,position,thread_id",
        ),
        (
            "sidebar_project_order",
            "SELECT * FROM sidebar_project_order ORDER BY position,project_id",
        ),
    ]
    .into_iter()
    .map(|(name, sql)| table_snapshot(conn, name, sql))
    .collect()
}

fn organization_snapshot(conn: &Connection) -> TableSnapshot {
    table_snapshot(
        conn,
        "sidebar_organization",
        "SELECT * FROM sidebar_organization ORDER BY singleton",
    )
}

fn seed_schema_twenty_seven(conn: &Connection, root: &Path, sort: &str, saved_order: bool) {
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    for (index, migration) in LEGACY_MIGRATIONS.iter().enumerate() {
        let tx = conn.unchecked_transaction().unwrap();
        tx.execute_batch(migration).unwrap();
        tx.pragma_update(None, "user_version", (index + 1) as u32)
            .unwrap();
        tx.commit().unwrap();
    }
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 27);

    for (id, name, created_at, last_opened_at) in [
        ("project-one", "First project", 7, 11),
        ("project-two", "Second project", 8, 10),
    ] {
        let path = root.join(id);
        conn.execute(
            "INSERT INTO projects(id,path,name,git_default_branch,created_at,last_opened_at) \
             VALUES (?1,?2,?3,'main',?4,?5)",
            (id, path.to_str().unwrap(), name, created_at, last_opened_at),
        )
        .unwrap();
    }

    for (prefix, project, pinned) in [
        ("p", Some("project-one"), false),
        ("q", Some("project-two"), false),
        ("pin", Some("project-two"), true),
        ("r", None, false),
    ] {
        for (suffix, created_at, updated_at, message_time) in [
            ("a", 100, 400, 910),
            ("b", 300, 200, 930),
            ("c", 400, 100, 990),
            ("d", 50, 250, 950),
            ("archived", 200, 300, 970),
            ("deleted", 5, 5, 999),
        ] {
            let id = format!("{prefix}-{suffix}");
            let status = if suffix == "archived" {
                "archived"
            } else {
                "active"
            };
            conn.execute(
                "INSERT INTO threads \
                 (id,project_id,title,mode,permission_mode,model,status,pinned,unread,created_at,updated_at,auto_title_state) \
                 VALUES (?1,?2,?3,'ask','confirm','legacy-model',?4,?5,1,?6,?7,'legacy')",
                (
                    &id,
                    project,
                    format!("Legacy {id}"),
                    status,
                    pinned,
                    created_at,
                    updated_at,
                ),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at) \
                 VALUES (?1,?2,1,?3,'text',?4,'done',?5)",
                (
                    format!("history-{id}"),
                    &id,
                    if suffix == "c" { "assistant" } else { "user" },
                    format!("History for {id}\n{{\"manual_thread_order\":[\"not-a-thread\"]}}"),
                    message_time,
                ),
            )
            .unwrap();
            if suffix == "deleted" {
                conn.execute("DELETE FROM messages WHERE thread_id=?1", [&id])
                    .unwrap();
                conn.execute("DELETE FROM threads WHERE id=?1", [&id])
                    .unwrap();
            }
        }
    }
    conn.execute(
        "INSERT INTO sidebar_groups(id,name,color,position) VALUES ('keep-group','Kept group',?1,6)",
        [serde_json::to_string(&SidebarGroupColor::Purple).unwrap()],
    )
    .unwrap();
    conn.execute_batch(
        "UPDATE threads SET parent_thread_id='q-a',workspace_owner_thread_id='q-a' WHERE id='pin-c'; \
         INSERT INTO sidebar_memberships(thread_id,group_id,position) VALUES ('p-a','keep-group',4),('r-archived','keep-group',9); \
         INSERT INTO sidebar_project_order(project_id,position) VALUES ('project-two',3),('project-one',8);",
    )
    .unwrap();

    let mut preferences = json!({
        "view": "Groups",
        "project_view": "Timeline",
        "sort": sort,
        "extension": {"keep": [1, "two"], "enabled": true}
    });
    if saved_order {
        preferences["manual_thread_order"] = json!([
            "p-deleted",
            "p-c",
            "pin-c",
            "pin-deleted",
            "q-c",
            "r-c",
            "p-archived",
            "pin-archived",
            "r-deleted",
            "q-archived",
            "r-archived",
            "p-a",
            "p-a",
            "pin-a",
            "q-deleted",
            "q-a",
            "r-a"
        ]);
    }
    conn.execute(
        "UPDATE sidebar_organization SET revision=37,preferences=?1,collapsed=?2 WHERE singleton=1",
        (
            preferences.to_string(),
            json!([{"Project": "project-two"}, {"Group": "keep-group"}, "Pinned"]).to_string(),
        ),
    )
    .unwrap();
}

fn manual_order(conn: &Connection) -> Vec<String> {
    let rows = sql::read(conn).unwrap();
    let preferences: Value = serde_json::from_str(&rows.preferences).unwrap();
    serde_json::from_value(
        preferences
            .get("manual_thread_order")
            .expect("upgraded preferences must contain the complete sidebar order")
            .clone(),
    )
    .unwrap()
}

fn assert_snapshot_orders(
    state: &SidebarOrganizationSnapshot,
    expected: [&[&str]; 4],
    visible_expected: [&[&str]; 4],
) {
    for ((project, pinned), (expected_ids, visible_ids)) in [
        (None, true),
        (Some("project-one"), false),
        (Some("project-two"), false),
        (None, false),
    ]
    .into_iter()
    .zip(expected.into_iter().zip(visible_expected))
    {
        let region = state
            .threads
            .iter()
            .filter(|thread| {
                thread.pinned == pinned && (pinned || thread.project_binding() == project)
            })
            .cloned()
            .collect::<Vec<_>>();
        let ordered = sorted_threads(&region, &state.preferences);
        let actual = ordered
            .iter()
            .map(|thread| thread.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected_ids, "project={project:?}, pinned={pinned}");
        let visible = sorted_threads(
            &region
                .into_iter()
                .filter(|thread| thread.status == ThreadStatus::Active)
                .collect::<Vec<_>>(),
            &state.preferences,
        );
        assert_eq!(
            visible
                .iter()
                .map(|thread| thread.id.as_str())
                .collect::<Vec<_>>(),
            visible_ids,
            "visible project={project:?}, pinned={pinned}"
        );
    }
}

fn assert_region_orders(
    store: &Store,
    expected: [&[&str]; 4],
    visible_expected: [&[&str]; 4],
) -> SidebarOrganizationSnapshot {
    let state = snapshot(store).unwrap();
    assert_snapshot_orders(&state, expected, visible_expected);
    let order = manual_order(store.conn());
    for ((project, pinned), expected_ids) in [
        (None, true),
        (Some("project-one"), false),
        (Some("project-two"), false),
        (None, false),
    ]
    .into_iter()
    .zip(expected)
    {
        let members: BTreeSet<_> = state
            .threads
            .iter()
            .filter(|thread| {
                thread.pinned == pinned && (pinned || thread.project_binding() == project)
            })
            .map(|thread| thread.id.as_str())
            .collect();
        let actual: Vec<_> = order
            .iter()
            .map(String::as_str)
            .filter(|id| members.contains(id))
            .collect();
        assert_eq!(
            actual, expected_ids,
            "saved project={project:?}, pinned={pinned}"
        );
    }
    let unique: BTreeSet<_> = order.iter().map(String::as_str).collect();
    let registered: BTreeSet<_> = state
        .threads
        .iter()
        .map(|thread| thread.id.as_str())
        .collect();
    assert_eq!(
        unique.len(),
        order.len(),
        "duplicate sidebar ranks survived"
    );
    assert_eq!(
        unique, registered,
        "sidebar order must contain every valid ID"
    );
    state
}

fn assert_json_compatibility(before: &str, after: &str) {
    let mut before: Value = serde_json::from_str(before).unwrap();
    let mut after: Value = serde_json::from_str(after).unwrap();
    assert!(matches!(
        after["sort"].as_str(),
        Some("Updated" | "Created" | "Manual")
    ));
    for preferences in [&mut before, &mut after] {
        let object = preferences.as_object_mut().unwrap();
        object.remove("sort");
        object.remove("manual_thread_order");
    }
    assert_eq!(after, before);
}

fn open_upgraded(path: &Path) -> Store {
    let store = Store::open(path).unwrap();
    store.migrate().unwrap();
    store
}

fn exercise_upgrade(sort: &str, saved_order: bool, expected: &ScenarioOrder) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.sqlite");
    let conn = Connection::open(&path).unwrap();
    seed_schema_twenty_seven(&conn, directory.path(), sort, saved_order);
    let original = preserved_rows(&conn);
    let old_organization = sql::read(&conn).unwrap();
    drop(conn);

    let store = open_upgraded(&path);
    let state = assert_region_orders(&store, expected.regions, expected.visible_regions);
    assert_eq!(state.groups.len(), 1);
    assert_eq!(state.groups[0].color, SidebarGroupColor::Purple);
    assert_eq!(state.preferences.view, SidebarView::Groups);
    assert_eq!(state.preferences.project_view, SidebarProjectView::Timeline);
    let child = state
        .threads
        .iter()
        .find(|thread| thread.id == "pin-c")
        .unwrap();
    let parent = state
        .threads
        .iter()
        .find(|thread| thread.id == "q-a")
        .unwrap();
    assert_eq!(child.parent_thread_id.as_deref(), Some("q-a"));
    assert_eq!(child.project_binding(), parent.project_binding());
    assert_eq!(child.project_binding(), Some("project-two"));
    assert_eq!(preserved_rows(store.conn()), original);
    let migrated = sql::read(store.conn()).unwrap();
    assert_json_compatibility(&old_organization.preferences, &migrated.preferences);
    assert_eq!(migrated.collapsed, old_organization.collapsed);
    let organization = organization_snapshot(store.conn());
    drop(store);

    let store = open_upgraded(&path);
    assert_eq!(organization_snapshot(store.conn()), organization);
    assert_eq!(preserved_rows(store.conn()), original);
    assert_eq!(
        assert_region_orders(&store, expected.regions, expected.visible_regions),
        state
    );

    let visited = threads::visit_thread(store.conn(), "p-c", 9_000)
        .unwrap()
        .unwrap();
    assert!(!visited.unread);
    assert_eq!(visited.updated_at, 9_000);
    assert_eq!(
        projects::find(store.conn(), "project-one")
            .unwrap()
            .unwrap()
            .last_opened_at,
        9_000
    );
    assert_region_orders(&store, expected.regions, expected.visible_regions);
    assert_eq!(
        threads::rename(store.conn(), "p-a", "Renamed after upgrade", 9_100).unwrap(),
        1
    );
    assert_eq!(
        threads::set_model(store.conn(), "p-d", "replacement-model", 9_200).unwrap(),
        1
    );
    assert_eq!(
        threads::set_permission_mode(store.conn(), "p-b", "readonly", 9_300).unwrap(),
        1
    );
    assert_eq!(
        threads::find(store.conn(), "p-a").unwrap().unwrap().title,
        "Renamed after upgrade"
    );
    assert_eq!(
        threads::find(store.conn(), "p-d").unwrap().unwrap().model,
        "replacement-model"
    );
    assert_eq!(
        threads::find(store.conn(), "p-b")
            .unwrap()
            .unwrap()
            .permission_mode,
        "readonly"
    );
    assert_region_orders(&store, expected.regions, expected.visible_regions);
    for id in ["p-archived", "q-archived", "pin-archived", "r-archived"] {
        assert_eq!(threads::set_status(store.conn(), id, "active").unwrap(), 1);
        assert_eq!(
            threads::find(store.conn(), id).unwrap().unwrap().status,
            "active"
        );
    }
    assert_region_orders(&store, expected.regions, expected.regions);
    for id in ["p-archived", "q-archived", "pin-archived", "r-archived"] {
        assert_eq!(
            threads::set_status(store.conn(), id, "archived").unwrap(),
            1
        );
    }
    assert_eq!(organization_snapshot(store.conn()), organization);
    assert_region_orders(&store, expected.regions, expected.visible_regions);
    let edited = preserved_rows(store.conn());
    drop(store);

    let store = open_upgraded(&path);
    assert_eq!(organization_snapshot(store.conn()), organization);
    assert_eq!(preserved_rows(store.conn()), edited);
    assert_region_orders(&store, expected.regions, expected.visible_regions);
    let message = messages::MessageRow {
        id: "post-upgrade-message".into(),
        thread_id: "p-b".into(),
        seq: 2,
        role: "user".into(),
        kind: "text".into(),
        content: "A genuinely new message after the upgrade".into(),
        status: "done".into(),
        created_at: 5,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    };
    messages::insert(store.conn(), &message).unwrap();
    assert_eq!(
        messages::find(store.conn(), &message.id).unwrap(),
        Some(message.clone())
    );
    let [pinned, _, other_project, recent] = expected.regions;
    let [visible_pinned, _, visible_other_project, visible_recent] = expected.visible_regions;
    let promoted = [pinned, expected.promoted_project, other_project, recent];
    let promoted_visible = [
        visible_pinned,
        expected.promoted_visible_project,
        visible_other_project,
        visible_recent,
    ];
    assert_region_orders(&store, promoted, promoted_visible);
    let after_message = sql::read(store.conn()).unwrap();
    assert_json_compatibility(&migrated.preferences, &after_message.preferences);
    assert_eq!(after_message.collapsed, old_organization.collapsed);
    let mut without_new_message = preserved_rows(store.conn());
    *without_new_message
        .iter_mut()
        .find(|table| table.name == "messages")
        .unwrap() = table_snapshot(
        store.conn(),
        "messages",
        "SELECT * FROM messages WHERE id <> 'post-upgrade-message' ORDER BY id",
    );
    assert_eq!(without_new_message, edited);
    let committed_organization = organization_snapshot(store.conn());
    let committed_contents = preserved_rows(store.conn());
    drop(store);

    let store = open_upgraded(&path);
    let before_drag = assert_region_orders(&store, promoted, promoted_visible);
    assert_eq!(organization_snapshot(store.conn()), committed_organization);
    assert_eq!(preserved_rows(store.conn()), committed_contents);
    let outcome = apply(
        &store,
        before_drag.revision,
        SidebarOrganizationAction::ReorderThread {
            thread_id: "p-b".into(),
            before_id: None,
        },
    )
    .unwrap();
    assert!(outcome.created_thread.is_none());
    let dragged = [pinned, expected.dragged_project, other_project, recent];
    let dragged_visible = [
        visible_pinned,
        expected.dragged_visible_project,
        visible_other_project,
        visible_recent,
    ];
    assert_snapshot_orders(&outcome.snapshot, dragged, dragged_visible);
    let after_drag = assert_region_orders(&store, dragged, dragged_visible);
    assert_eq!(outcome.snapshot, after_drag);
    assert_eq!(after_drag.revision, before_drag.revision + 1);
    assert_eq!(after_drag.projects, before_drag.projects);
    assert_eq!(after_drag.threads, before_drag.threads);
    assert_eq!(after_drag.groups, before_drag.groups);
    assert_eq!(after_drag.memberships, before_drag.memberships);
    assert_eq!(after_drag.project_order, before_drag.project_order);
    assert_eq!(after_drag.collapsed, before_drag.collapsed);
    assert_eq!(after_drag.preferences.view, SidebarView::Groups);
    assert_eq!(
        after_drag.preferences.project_view,
        SidebarProjectView::Timeline
    );
    let dragged_contents = preserved_rows(store.conn());
    for table in committed_contents
        .iter()
        .filter(|table| matches!(table.name, "projects" | "threads" | "messages"))
    {
        assert_eq!(
            dragged_contents
                .iter()
                .find(|after| after.name == table.name)
                .unwrap(),
            table
        );
    }
    let dragged_organization = organization_snapshot(store.conn());
    drop(store);

    let store = open_upgraded(&path);
    assert_eq!(
        assert_region_orders(&store, dragged, dragged_visible),
        after_drag
    );
    assert_eq!(organization_snapshot(store.conn()), dragged_organization);
    assert_eq!(preserved_rows(store.conn()), dragged_contents);
    let foreign_key_errors: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(foreign_key_errors, 0);
}

#[test]
fn schema_twenty_seven_updated_order_is_preserved_once_and_then_follows_new_messages() {
    exercise_upgrade("Updated", true, &UPDATED_ORDER);
}

#[test]
fn schema_twenty_seven_created_order_is_preserved_once_and_then_follows_new_messages() {
    exercise_upgrade("Created", true, &CREATED_ORDER);
}

#[test]
fn schema_twenty_seven_manual_ranks_keep_fallback_and_archived_neighbors_after_upgrade() {
    exercise_upgrade("Manual", true, &MANUAL_ORDER);
}

#[test]
fn schema_twenty_seven_preferences_without_saved_ranks_upgrade_without_losing_json_fields() {
    exercise_upgrade("Created", false, &CREATED_ORDER);
}
