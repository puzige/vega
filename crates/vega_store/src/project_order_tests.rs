use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::params;

use crate::{MIGRATIONS, Store, messages, projects, sidebar_organization as sidebar, threads};

struct OwnedTempDir(PathBuf);

impl OwnedTempDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("vega-project-order-{}", ulid::Ulid::generate()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, PartialEq)]
struct NavigationMetadata {
    revision: i64,
    preferences: String,
    collapsed: String,
    groups: Vec<sidebar::GroupRow>,
    memberships: Vec<sidebar::MembershipRow>,
    thread: Option<threads::ThreadRow>,
    message: Option<messages::MessageRow>,
}

fn navigation_metadata(store: &Store) -> NavigationMetadata {
    let rows = sidebar::read(store.conn()).unwrap();
    NavigationMetadata {
        revision: rows.revision,
        preferences: rows.preferences,
        collapsed: rows.collapsed,
        groups: rows.groups,
        memberships: rows.memberships,
        thread: threads::find(store.conn(), "retained-thread").unwrap(),
        message: messages::find(store.conn(), "retained-message").unwrap(),
    }
}

fn order_rows(store: &Store) -> Vec<(String, i64)> {
    store
        .conn()
        .prepare(
            "SELECT project_id,position FROM sidebar_project_order ORDER BY position,project_id",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn assert_order(store: &Store, expected: &[&str]) {
    assert_eq!(sidebar::read(store.conn()).unwrap().project_order, expected);
    assert_eq!(
        sidebar::projects(store.conn())
            .unwrap()
            .iter()
            .map(|project| project.id.as_str())
            .collect::<Vec<_>>(),
        expected
    );
}

fn open_v30(path: &Path) -> Store {
    let store = Store::open(path).unwrap();
    store
        .conn()
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    for migration in &MIGRATIONS[..30] {
        store.conn().execute_batch(migration).unwrap();
    }
    store
        .conn()
        .pragma_update(None, "user_version", 30)
        .unwrap();
    store
        .conn()
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    store
}

fn insert_legacy_project(store: &Store, root: &Path, id: &str, name: &str, opened: i64) {
    store
        .conn()
        .execute(
            "INSERT INTO projects(id,path,name,git_default_branch,created_at,last_opened_at) VALUES (?1,?2,?3,'main',17,?4)",
            params![id, root.join(id).to_str().unwrap(), name, opened],
        )
        .unwrap();
}

fn seed_navigation(store: &Store, project_id: &str) {
    threads::create(
        store.conn(),
        threads::NewThread {
            id: "retained-thread",
            project_id,
            title: "Retained conversation",
            mode: "ask",
            permission_mode: "confirm",
            model: "retained-model",
            status: "active",
            pinned: true,
            unread: true,
            created_at: 19,
            updated_at: 23,
        },
    )
    .unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "retained-message".into(),
            thread_id: "retained-thread".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: "Retained content".into(),
            status: "done".into(),
            created_at: 29,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    store.conn().execute_batch(
        "INSERT INTO sidebar_groups(id,name,color,position) VALUES ('retained-group','Retained group','\"Blue\"',13);
         INSERT INTO sidebar_memberships(thread_id,group_id,position) VALUES ('retained-thread','retained-group',21);
         UPDATE sidebar_organization SET revision=37,preferences='{\"view\":\"Groups\",\"project_view\":\"Timeline\",\"sort\":\"Manual\",\"manual_thread_order\":[\"retained-thread\"],\"retained\":true}',collapsed='[{\"Project\":\"project-a\"},{\"Group\":\"retained-group\"}]' WHERE singleton=1;",
    ).unwrap();
}

fn create_project(store: &Store, root: &Path, name: &str) -> projects::Project {
    projects::create(store.conn(), root.join(name).to_str().unwrap(), name, None).unwrap()
}

fn save_order(store: &Store, ids: &[&str]) {
    let tx = store.immediate_transaction().unwrap();
    let mut rows = sidebar::read(store.conn()).unwrap();
    rows.project_order = ids.iter().map(|id| (*id).to_owned()).collect();
    sidebar::write(store.conn(), &rows).unwrap();
    tx.commit().unwrap();
}

#[test]
fn migration_freezes_legacy_recent_order_without_changing_navigation() {
    let temp = OwnedTempDir::new();
    let path = temp.path().join("store.sqlite");
    let store = open_v30(&path);
    insert_legacy_project(&store, temp.path(), "project-c", "Alpha", 300);
    insert_legacy_project(&store, temp.path(), "project-a", "Middle", 100);
    insert_legacy_project(&store, temp.path(), "project-b", "Zulu", 300);
    seed_navigation(&store, "project-a");
    let expected = ["project-b", "project-c", "project-a"];
    assert!(
        sidebar::read(store.conn())
            .unwrap()
            .project_order
            .is_empty()
    );
    let projects_before = projects::list(store.conn(), projects::ProjectSort::Name).unwrap();
    let navigation_before = navigation_metadata(&store);

    store.migrate().unwrap();
    assert_order(&store, &expected);
    assert_eq!(navigation_metadata(&store), navigation_before);
    assert_eq!(
        projects::list(store.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );
    let frozen_rows = order_rows(&store);
    store.migrate().unwrap();
    assert_eq!(order_rows(&store), frozen_rows);
    assert_order(&store, &expected);
    assert_eq!(navigation_metadata(&store), navigation_before);
    assert_eq!(
        projects::list(store.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );

    drop(store);
    let reopened = Store::open(&path).unwrap();
    reopened.migrate().unwrap();
    assert_order(&reopened, &expected);
    assert_eq!(order_rows(&reopened), frozen_rows);
    assert_eq!(navigation_metadata(&reopened), navigation_before);
    assert_eq!(
        projects::list(reopened.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );
}

#[test]
fn migration_keeps_gapped_tied_manual_prefix_and_freezes_missing_projects() {
    let temp = OwnedTempDir::new();
    let path = temp.path().join("store.sqlite");
    let store = open_v30(&path);
    for (id, name, opened) in [
        ("project-e", "Echo", 900),
        ("project-d", "Alpha", 200),
        ("project-c", "Zulu", 600),
        ("project-b", "Delta", 200),
        ("project-a", "Middle", 500),
    ] {
        insert_legacy_project(&store, temp.path(), id, name, opened);
    }
    seed_navigation(&store, "project-a");
    store.conn().execute_batch(
        "INSERT INTO sidebar_project_order(project_id,position) VALUES ('project-e',40),('project-d',7),('project-b',7);",
    ).unwrap();
    let expected = [
        "project-b",
        "project-d",
        "project-e",
        "project-c",
        "project-a",
    ];
    let navigation_before = navigation_metadata(&store);
    let projects_before = projects::list(store.conn(), projects::ProjectSort::Name).unwrap();

    store.migrate().unwrap();
    assert_order(&store, &expected);
    assert_eq!(navigation_metadata(&store), navigation_before);
    assert_eq!(
        projects::list(store.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );
    let frozen_rows = order_rows(&store);
    for (id, name, opened) in [
        ("project-a", "Aardvark", 5_000),
        ("project-c", "Bravo", 4_000),
        ("project-e", "Charlie", 3_000),
        ("project-d", "Delta", 2_000),
        ("project-b", "Zulu", 1_000),
    ] {
        store
            .conn()
            .execute(
                "UPDATE projects SET name=?1,last_opened_at=?2 WHERE id=?3",
                params![name, opened, id],
            )
            .unwrap();
    }
    assert_order(&store, &expected);
    let changed_projects = projects::list(store.conn(), projects::ProjectSort::Name).unwrap();
    store.migrate().unwrap();
    assert_eq!(order_rows(&store), frozen_rows);
    assert_order(&store, &expected);
    assert_eq!(navigation_metadata(&store), navigation_before);

    drop(store);
    let reopened = Store::open(&path).unwrap();
    reopened.migrate().unwrap();
    assert_order(&reopened, &expected);
    assert_eq!(order_rows(&reopened), frozen_rows);
    assert_eq!(navigation_metadata(&reopened), navigation_before);
    assert_eq!(
        projects::list(reopened.conn(), projects::ProjectSort::Name).unwrap(),
        changed_projects
    );

    reopened.conn().execute_batch(
        "UPDATE sidebar_project_order SET position=CASE project_id WHEN 'project-b' THEN 7 WHEN 'project-d' THEN 7 WHEN 'project-e' THEN 40 WHEN 'project-c' THEN 51 WHEN 'project-a' THEN 63 END;",
    ).unwrap();
    assert_order(&reopened, &expected);
    let gapped_rows = order_rows(&reopened);
    let appended = create_project(&reopened, temp.path(), "Appended");
    assert_order(
        &reopened,
        &[
            "project-b",
            "project-d",
            "project-e",
            "project-c",
            "project-a",
            &appended.id,
        ],
    );
    assert_eq!(&order_rows(&reopened)[..gapped_rows.len()], gapped_rows);
    assert_eq!(navigation_metadata(&reopened), navigation_before);
}

#[test]
fn migration_failure_rolls_back_partial_backfill_and_schema_version_then_retries() {
    let temp = OwnedTempDir::new();
    let store = open_v30(&temp.path().join("store.sqlite"));
    for (id, name, opened) in [
        ("project-c", "Alpha", 200),
        ("project-a", "Middle", 100),
        ("project-b", "Zulu", 300),
    ] {
        insert_legacy_project(&store, temp.path(), id, name, opened);
    }
    seed_navigation(&store, "project-a");
    store.conn().execute_batch(
        "INSERT INTO sidebar_project_order(project_id,position) VALUES ('project-a',9);
         CREATE TEMP TRIGGER reject_later_backfill BEFORE INSERT ON main.sidebar_project_order
         WHEN NEW.project_id != 'project-a' AND EXISTS(SELECT 1 FROM sidebar_project_order WHERE project_id != 'project-a')
         BEGIN SELECT RAISE(ABORT,'migration append rejected'); END;",
    ).unwrap();
    let original_rows = order_rows(&store);
    let navigation_before = navigation_metadata(&store);
    let projects_before = projects::list(store.conn(), projects::ProjectSort::Name).unwrap();

    let error = store.migrate().unwrap_err();
    assert!(error.to_string().contains("migration append rejected"));
    assert_eq!(order_rows(&store), original_rows);
    assert_eq!(navigation_metadata(&store), navigation_before);
    assert_eq!(
        projects::list(store.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );
    assert_eq!(
        store
            .conn()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        30
    );

    store
        .conn()
        .execute_batch("DROP TRIGGER reject_later_backfill")
        .unwrap();
    store.migrate().unwrap();
    assert_order(&store, &["project-a", "project-b", "project-c"]);
    assert!(
        store
            .conn()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap()
            > 30
    );
    assert_eq!(navigation_metadata(&store), navigation_before);
    assert_eq!(
        projects::list(store.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );
}

#[test]
fn creation_appends_to_empty_and_manual_orders_inside_outer_transactions() {
    let temp = OwnedTempDir::new();
    let store = Store::open(temp.path().join("store.sqlite")).unwrap();
    store.migrate().unwrap();
    assert_order(&store, &[]);
    let first = create_project(&store, temp.path(), "Zulu");
    assert_order(&store, &[&first.id]);
    let second = create_project(&store, temp.path(), "Alpha");
    assert_order(&store, &[&first.id, &second.id]);
    let third = create_project(&store, temp.path(), "Middle");
    assert_order(&store, &[&first.id, &second.id, &third.id]);

    save_order(&store, &[&second.id, &first.id, &third.id]);
    let fourth = create_project(&store, temp.path(), "Aardvark");
    assert_order(&store, &[&second.id, &first.id, &third.id, &fourth.id]);
    let tx = store.immediate_transaction().unwrap();
    let fifth = create_project(&store, temp.path(), "Bravo");
    let expected = [&second.id[..], &first.id, &third.id, &fourth.id, &fifth.id];
    assert_order(&store, &expected);
    tx.commit().unwrap();
    assert_order(&store, &expected);
}

#[test]
fn failed_and_rolled_back_creation_leave_no_rows_and_removal_keeps_tail_append() {
    let temp = OwnedTempDir::new();
    let store = Store::open(temp.path().join("store.sqlite")).unwrap();
    store.migrate().unwrap();
    let first = create_project(&store, temp.path(), "First");
    let second = create_project(&store, temp.path(), "Second");
    let third = create_project(&store, temp.path(), "Third");
    save_order(&store, &[&third.id, &first.id, &second.id]);
    let projects_before = projects::list(store.conn(), projects::ProjectSort::Name).unwrap();
    let order_before = order_rows(&store);
    let navigation_before = navigation_metadata(&store);

    assert!(matches!(
        projects::create(store.conn(), &first.path, "Duplicate", None),
        Err(projects::ProjectsError::PathAlreadyRegistered(_))
    ));
    assert_eq!(
        projects::list(store.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );
    assert_eq!(order_rows(&store), order_before);
    assert_eq!(navigation_metadata(&store), navigation_before);

    store.conn().execute_batch(
        "CREATE TEMP TRIGGER reject_tail_append BEFORE INSERT ON sidebar_project_order BEGIN SELECT RAISE(ABORT,'tail append rejected'); END;",
    ).unwrap();
    let rejected_path = temp.path().join("Rejected");
    assert!(matches!(
        projects::create(
            store.conn(),
            rejected_path.to_str().unwrap(),
            "Rejected",
            None
        ),
        Err(projects::ProjectsError::Sql(_))
    ));
    assert_eq!(
        projects::list(store.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );
    assert_eq!(order_rows(&store), order_before);
    assert_eq!(navigation_metadata(&store), navigation_before);
    store
        .conn()
        .execute_batch("DROP TRIGGER reject_tail_append")
        .unwrap();

    let tx = store.immediate_transaction().unwrap();
    let rolled_back = create_project(&store, temp.path(), "RolledBack");
    assert_order(&store, &[&third.id, &first.id, &second.id, &rolled_back.id]);
    tx.rollback().unwrap();
    assert!(
        projects::find(store.conn(), &rolled_back.id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        projects::list(store.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );
    assert_eq!(order_rows(&store), order_before);
    assert_eq!(navigation_metadata(&store), navigation_before);

    assert!(projects::remove(store.conn(), &first.id).unwrap());
    assert_order(&store, &[&third.id, &second.id]);
    assert!(order_rows(&store).iter().all(|(id, _)| id != &first.id));
    let appended = create_project(&store, temp.path(), "Rejected");
    assert_order(&store, &[&third.id, &second.id, &appended.id]);
    let after_rollback = create_project(&store, temp.path(), "RolledBack");
    assert_order(
        &store,
        &[&third.id, &second.id, &appended.id, &after_rollback.id],
    );
}
