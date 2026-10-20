use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{apply, register_and_reveal_project, snapshot};
use crate::threads;
use crate::types::{
    SidebarCollapseTarget, SidebarOrganizationAction, SidebarOrganizationError,
    SidebarOrganizationSnapshot, SidebarPreferences, SidebarProjectView, SidebarTaskSort,
    SidebarView, ThreadUpdate,
};
use vega_store::{Store, messages, projects, sidebar_organization as sql};

struct OwnedTempDir(PathBuf);

impl OwnedTempDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("vega-project-order-{}", ulid::Ulid::generate()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn directory(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir(&path).unwrap();
        path
    }
}

impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    store: Store,
    temp: OwnedTempDir,
}

impl Fixture {
    fn empty() -> Self {
        let temp = OwnedTempDir::new();
        let store = Store::open(temp.0.join("store.sqlite")).unwrap();
        store.migrate().unwrap();
        Self { store, temp }
    }

    fn seeded() -> Self {
        let fixture = Self::empty();
        for (id, name, opened) in [
            ("project-c", "Alpha", 100),
            ("project-a", "Zulu", 200),
            ("project-b", "Middle", 300),
        ] {
            let path = fixture.temp.directory(name);
            fixture.store.conn().execute(
                "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES (?1,?2,?3,11,?4)",
                (id, path.to_str().unwrap(), name, opened),
            ).unwrap();
        }
        for (id, project_id, pinned) in [
            ("thread-a", "project-a", true),
            ("thread-c", "project-c", false),
        ] {
            vega_store::threads::create(
                fixture.store.conn(),
                vega_store::threads::NewThread {
                    id,
                    project_id,
                    title: "Retained conversation",
                    mode: "ask",
                    permission_mode: "confirm",
                    model: "retained-model",
                    status: "active",
                    pinned,
                    unread: true,
                    created_at: 17,
                    updated_at: 23,
                },
            )
            .unwrap();
        }
        messages::insert(
            fixture.store.conn(),
            &message("retained-message", "thread-a", "done"),
        )
        .unwrap();
        let tx = fixture.store.immediate_transaction().unwrap();
        let mut rows = sql::read(fixture.store.conn()).unwrap();
        rows.revision = 19;
        rows.project_order = ["project-b", "project-a", "project-c"]
            .map(str::to_owned)
            .to_vec();
        rows.preferences = serde_json::to_string(&SidebarPreferences {
            manual_thread_order: vec!["thread-a".into(), "thread-c".into()],
            ..SidebarPreferences::default()
        })
        .unwrap();
        rows.collapsed = serde_json::to_string(&[
            SidebarCollapseTarget::Project("project-a".into()),
            SidebarCollapseTarget::Group("retained-group".into()),
        ])
        .unwrap();
        rows.groups = vec![(
            "retained-group".into(),
            "Retained group".into(),
            "\"Blue\"".into(),
        )];
        rows.memberships = vec![("thread-c".into(), "retained-group".into())];
        sql::write(fixture.store.conn(), &rows).unwrap();
        tx.commit().unwrap();
        fixture
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PersistedOrganization {
    revision: i64,
    preferences: String,
    collapsed: String,
    groups: Vec<sql::GroupRow>,
    memberships: Vec<sql::MembershipRow>,
    order: Vec<(String, i64)>,
}

fn persisted(store: &Store) -> PersistedOrganization {
    let rows = sql::read(store.conn()).unwrap();
    let order = store
        .conn()
        .prepare(
            "SELECT project_id,position FROM sidebar_project_order ORDER BY position,project_id",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    PersistedOrganization {
        revision: rows.revision,
        preferences: rows.preferences,
        collapsed: rows.collapsed,
        groups: rows.groups,
        memberships: rows.memberships,
        order,
    }
}

fn assert_order(state: &SidebarOrganizationSnapshot, expected: &[&str]) {
    assert_eq!(
        state
            .projects
            .iter()
            .map(|project| project.id.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(state.project_order, expected);
}

fn read_ordered(store: &Store, expected: &[&str]) -> SidebarOrganizationSnapshot {
    let before = persisted(store);
    let state = snapshot(store).unwrap();
    assert_order(&state, expected);
    assert_eq!(persisted(store), before);
    state
}

fn commit(store: &Store, action: SidebarOrganizationAction) -> SidebarOrganizationSnapshot {
    let revision = snapshot(store).unwrap().revision;
    let outcome = apply(store, revision, action).unwrap();
    assert!(outcome.created_thread.is_none());
    assert_eq!(outcome.snapshot.revision, revision + 1);
    assert_eq!(snapshot(store).unwrap(), outcome.snapshot);
    outcome.snapshot
}

fn assert_navigation_unchanged(
    before: &SidebarOrganizationSnapshot,
    after: &SidebarOrganizationSnapshot,
) {
    assert_eq!(after.preferences, before.preferences);
    assert_eq!(after.collapsed, before.collapsed);
    assert_eq!(after.groups, before.groups);
    assert_eq!(after.memberships, before.memberships);
    assert_eq!(
        after
            .threads
            .iter()
            .map(|thread| (&thread.id, thread))
            .collect::<BTreeMap<_, _>>(),
        before
            .threads
            .iter()
            .map(|thread| (&thread.id, thread))
            .collect::<BTreeMap<_, _>>()
    );
    assert_eq!(
        after
            .projects
            .iter()
            .map(|project| (&project.id, project))
            .collect::<BTreeMap<_, _>>(),
        before
            .projects
            .iter()
            .map(|project| (&project.id, project))
            .collect::<BTreeMap<_, _>>()
    );
}

fn message(id: &str, thread_id: &str, status: &str) -> messages::MessageRow {
    messages::MessageRow {
        id: id.into(),
        thread_id: thread_id.into(),
        seq: 1,
        role: "assistant".into(),
        kind: "text".into(),
        content: "Retained content".into(),
        status: status.into(),
        created_at: 29,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    }
}

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn age_activity(store: &Store, thread_id: &str) {
    store
        .conn()
        .execute("UPDATE threads SET updated_at=1 WHERE id=?1", [thread_id])
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE projects SET last_opened_at=2 WHERE id='project-c'",
            [],
        )
        .unwrap();
}

fn assert_recent_visit(
    state: &SidebarOrganizationSnapshot,
    thread_id: &str,
    started: i64,
    unread: bool,
) {
    let thread = state
        .threads
        .iter()
        .find(|thread| thread.id == thread_id)
        .unwrap();
    let project = state
        .projects
        .iter()
        .find(|project| project.id == "project-c")
        .unwrap();
    assert!((started..=now_ms()).contains(&thread.updated_at));
    assert_eq!(project.last_opened_at, thread.updated_at);
    assert_eq!(thread.unread, unread);
}

#[test]
fn registration_reuses_and_reveals_without_reordering_and_reopens_at_tail() {
    let fixture = Fixture::empty();
    let first_path = fixture.temp.directory("Zulu");
    let second_path = fixture.temp.directory("Alpha");
    let third_path = fixture.temp.directory("Middle");
    let fourth_path = fixture.temp.directory("Aardvark");
    let (first, state) =
        register_and_reveal_project(&fixture.store, &first_path, Some("main")).unwrap();
    assert_order(&state, &[&first]);
    assert_eq!(state, read_ordered(&fixture.store, &[&first]));
    let (second, state) = register_and_reveal_project(&fixture.store, &second_path, None).unwrap();
    assert_order(&state, &[&first, &second]);
    assert_eq!(state, read_ordered(&fixture.store, &[&first, &second]));
    let (third, state) = register_and_reveal_project(&fixture.store, &third_path, None).unwrap();
    assert_order(&state, &[&first, &second, &third]);
    assert_eq!(
        state,
        read_ordered(&fixture.store, &[&first, &second, &third])
    );
    for id in [&first, &second] {
        commit(
            &fixture.store,
            SidebarOrganizationAction::SetCollapsed {
                target: SidebarCollapseTarget::Project(id.clone()),
                collapsed: true,
            },
        );
    }
    let (reused, state) = register_and_reveal_project(&fixture.store, &first_path, None).unwrap();
    assert_eq!(reused, first);
    assert_order(&state, &[&first, &second, &third]);
    assert!(
        !state
            .collapsed
            .contains(&SidebarCollapseTarget::Project(first.clone()))
    );
    assert!(
        state
            .collapsed
            .contains(&SidebarCollapseTarget::Project(second.clone()))
    );
    assert_eq!(
        state,
        read_ordered(&fixture.store, &[&first, &second, &third])
    );
    let (fourth, state) = register_and_reveal_project(&fixture.store, &fourth_path, None).unwrap();
    let expected = [&first[..], &second, &third, &fourth];
    assert_order(&state, &expected);
    assert_eq!(state, read_ordered(&fixture.store, &expected));

    let metadata_before = persisted(&fixture.store);
    let invalid_file = fixture.temp.0.join("not-a-directory");
    fs::write(&invalid_file, b"owned fixture").unwrap();
    assert!(matches!(
        register_and_reveal_project(&fixture.store, &invalid_file, None),
        Err(SidebarOrganizationError::Invalid(_))
    ));
    assert_eq!(snapshot(&fixture.store).unwrap(), state);
    assert_eq!(persisted(&fixture.store), metadata_before);
    let missing = fixture.temp.0.join("missing-directory");
    assert!(matches!(
        register_and_reveal_project(&fixture.store, &missing, None),
        Err(SidebarOrganizationError::Store(_))
    ));
    assert_eq!(snapshot(&fixture.store).unwrap(), state);
    assert_eq!(persisted(&fixture.store), metadata_before);

    let path = fixture.temp.0.join("store.sqlite");
    drop(fixture.store);
    let reopened = Store::open(path).unwrap();
    reopened.migrate().unwrap();
    assert_eq!(read_ordered(&reopened, &expected), state);
    assert_eq!(persisted(&reopened), metadata_before);

    reopened
        .conn()
        .execute(
            "UPDATE sidebar_organization SET revision=?1 WHERE singleton=1",
            [i64::MAX],
        )
        .unwrap();
    let exhausted = read_ordered(&reopened, &expected);
    assert_eq!(exhausted.revision, i64::MAX as u64);
    let metadata_before = persisted(&reopened);
    let projects_before = projects::list(reopened.conn(), projects::ProjectSort::Name).unwrap();
    let rejected_path = fixture.temp.directory("RevisionExhausted");
    assert!(matches!(
        register_and_reveal_project(&reopened, &rejected_path, None),
        Err(SidebarOrganizationError::Limit(_))
    ));
    assert_eq!(read_ordered(&reopened, &expected), exhausted);
    assert_eq!(persisted(&reopened), metadata_before);
    assert_eq!(
        projects::list(reopened.conn(), projects::ProjectSort::Name).unwrap(),
        projects_before
    );
    assert!(
        projects::find_by_path(
            reopened.conn(),
            rejected_path.canonicalize().unwrap().to_str().unwrap()
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn activity_and_projection_actions_keep_project_ids_and_preserve_visit_semantics() {
    let fixture = Fixture::seeded();
    let store = &fixture.store;
    let expected = ["project-b", "project-a", "project-c"];
    let initial = read_ordered(store, &expected);
    let started = now_ms();
    projects::touch_last_opened(store.conn(), "project-c").unwrap();
    let touched = read_ordered(store, &expected);
    let project = touched
        .projects
        .iter()
        .find(|project| project.id == "project-c")
        .unwrap();
    assert!((started..=now_ms()).contains(&project.last_opened_at));
    assert_eq!(touched.revision, initial.revision);

    let thread = threads::create_thread(store, "project-c", "activity-model", "confirm").unwrap();
    assert_eq!(read_ordered(store, &expected).revision, initial.revision);
    threads::update_thread(
        store,
        &thread.id,
        &ThreadUpdate {
            unread: Some(true),
            ..ThreadUpdate::default()
        },
    )
    .unwrap();
    assert!(
        read_ordered(store, &expected)
            .threads
            .iter()
            .find(|entry| entry.id == thread.id)
            .unwrap()
            .unread
    );
    age_activity(store, &thread.id);
    let started = now_ms();
    let opened = threads::open_thread(store, &thread.id).unwrap();
    let after_open = read_ordered(store, &expected);
    assert_recent_visit(&after_open, &thread.id, started, true);
    assert!(after_open.threads.contains(&opened));
    assert_eq!(after_open.revision, initial.revision);

    messages::insert(
        store.conn(),
        &message("activity-message", &thread.id, "streaming"),
    )
    .unwrap();
    let after_insert = read_ordered(store, &expected);
    assert_eq!(after_insert.revision, initial.revision + 1);
    assert_eq!(
        after_insert.preferences.manual_thread_order,
        ["thread-a", &thread.id, "thread-c"]
    );
    assert_eq!(
        messages::update_streaming_content(store.conn(), "activity-message", "Partial answer")
            .unwrap(),
        1
    );
    assert_eq!(read_ordered(store, &expected), after_insert);
    assert_eq!(
        messages::finish_streaming(
            store.conn(),
            "activity-message",
            "Complete answer",
            "done",
            None
        )
        .unwrap(),
        1
    );
    assert_eq!(read_ordered(store, &expected), after_insert);
    let stored_message = messages::find(store.conn(), "activity-message")
        .unwrap()
        .unwrap();
    assert_eq!(stored_message.content, "Complete answer");
    assert_eq!(stored_message.status, "done");
    age_activity(store, &thread.id);
    let started = now_ms();
    let visited = threads::visit_thread(store, &thread.id).unwrap();
    let after_visit = read_ordered(store, &expected);
    assert_recent_visit(&after_visit, &thread.id, started, false);
    assert!(after_visit.threads.contains(&visited));
    assert_eq!(visited.created_at, thread.created_at);
    assert_eq!(after_visit.revision, after_insert.revision);

    let target = SidebarCollapseTarget::Project("project-c".into());
    let collapsed = commit(
        store,
        SidebarOrganizationAction::SetCollapsed {
            target: target.clone(),
            collapsed: true,
        },
    );
    assert_order(&collapsed, &expected);
    assert!(collapsed.collapsed.contains(&target));
    let expanded = commit(
        store,
        SidebarOrganizationAction::SetCollapsed {
            target,
            collapsed: false,
        },
    );
    assert_order(&expanded, &expected);
    assert_eq!(expanded.collapsed, initial.collapsed);
    let preferences = commit(
        store,
        SidebarOrganizationAction::SetPreferences(SidebarPreferences {
            view: SidebarView::Groups,
            project_view: SidebarProjectView::Timeline,
            sort: SidebarTaskSort::Manual,
            manual_thread_order: Vec::new(),
        }),
    );
    assert_order(&preferences, &expected);
    assert_eq!(preferences.preferences.view, SidebarView::Groups);
    assert_eq!(
        preferences.preferences.project_view,
        SidebarProjectView::Timeline
    );
    assert_eq!(
        preferences.preferences.manual_thread_order,
        after_insert.preferences.manual_thread_order
    );
    assert_eq!(preferences.threads, after_visit.threads);
    assert_eq!(preferences.groups, initial.groups);
    assert_eq!(preferences.memberships, initial.memberships);
    assert_eq!(preferences.collapsed, initial.collapsed);
    assert_eq!(read_ordered(store, &expected), preferences);
    assert_eq!(
        messages::find(store.conn(), "activity-message").unwrap(),
        Some(stored_message)
    );
}

#[test]
fn moving_projects_to_front_and_tail_survives_visits_and_file_reopen() {
    let fixture = Fixture::seeded();
    let expected = ["project-b", "project-a", "project-c"];
    let initial = read_ordered(&fixture.store, &expected);
    let retained_message = messages::find(fixture.store.conn(), "retained-message").unwrap();
    let front = commit(
        &fixture.store,
        SidebarOrganizationAction::MoveProject {
            project_id: "project-c".into(),
            before_id: Some("project-b".into()),
        },
    );
    assert_order(&front, &["project-c", "project-b", "project-a"]);
    assert_navigation_unchanged(&initial, &front);
    let metadata_before = persisted(&fixture.store);
    let readonly = Store::open_read_only(fixture.temp.0.join("store.sqlite")).unwrap();
    assert_eq!(
        read_ordered(&readonly, &["project-c", "project-b", "project-a"]),
        front
    );
    assert_eq!(persisted(&readonly), metadata_before);
    assert_eq!(persisted(&fixture.store), metadata_before);
    assert_eq!(snapshot(&fixture.store).unwrap(), front);
    drop(readonly);

    let tail = commit(
        &fixture.store,
        SidebarOrganizationAction::MoveProject {
            project_id: "project-c".into(),
            before_id: None,
        },
    );
    assert_order(&tail, &expected);
    assert_navigation_unchanged(&initial, &tail);
    let started = now_ms();
    let opened = threads::open_thread(&fixture.store, "thread-c").unwrap();
    assert!(opened.unread);
    let after_open = read_ordered(&fixture.store, &expected);
    assert_recent_visit(&after_open, "thread-c", started, true);
    assert_eq!(after_open.revision, tail.revision);
    assert_eq!(after_open.collapsed, initial.collapsed);
    assert_eq!(after_open.preferences, initial.preferences);
    assert_eq!(after_open.groups, initial.groups);
    assert_eq!(after_open.memberships, initial.memberships);
    let metadata_before = persisted(&fixture.store);
    let path = fixture.temp.0.join("store.sqlite");
    drop(fixture.store);
    let reopened = Store::open(path).unwrap();
    reopened.migrate().unwrap();
    assert_eq!(read_ordered(&reopened, &expected), after_open);
    assert_eq!(persisted(&reopened), metadata_before);
    assert_eq!(
        messages::find(reopened.conn(), "retained-message").unwrap(),
        retained_message
    );
}

#[test]
fn rejected_project_moves_leave_full_snapshot_and_metadata_unchanged_then_retry() {
    let fixture = Fixture::seeded();
    let expected = ["project-b", "project-a", "project-c"];
    let initial = read_ordered(&fixture.store, &expected);
    let metadata_before = persisted(&fixture.store);
    let retained_message = messages::find(fixture.store.conn(), "retained-message").unwrap();
    let stale = initial.revision - 1;
    let error = apply(
        &fixture.store,
        stale,
        SidebarOrganizationAction::MoveProject {
            project_id: "project-c".into(),
            before_id: Some("project-b".into()),
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, SidebarOrganizationError::Conflict { expected, actual }
        if expected == stale && actual == initial.revision)
    );
    assert_eq!(read_ordered(&fixture.store, &expected), initial);
    assert_eq!(persisted(&fixture.store), metadata_before);

    for (project_id, before_id) in [
        ("missing-project", Some("project-b")),
        ("project-c", Some("missing-anchor")),
        ("project-c", Some("project-c")),
    ] {
        let error = apply(
            &fixture.store,
            initial.revision,
            SidebarOrganizationAction::MoveProject {
                project_id: project_id.into(),
                before_id: before_id.map(str::to_owned),
            },
        )
        .unwrap_err();
        assert!(matches!(error, SidebarOrganizationError::Invalid(_)));
        assert_eq!(read_ordered(&fixture.store, &expected), initial);
        assert_eq!(persisted(&fixture.store), metadata_before);
        assert_eq!(
            messages::find(fixture.store.conn(), "retained-message").unwrap(),
            retained_message
        );
    }

    let retry = commit(
        &fixture.store,
        SidebarOrganizationAction::MoveProject {
            project_id: "project-c".into(),
            before_id: Some("project-b".into()),
        },
    );
    assert_order(&retry, &["project-c", "project-b", "project-a"]);
    assert_eq!(retry.revision, initial.revision + 1);
    assert_navigation_unchanged(&initial, &retry);
}

#[test]
fn stale_project_move_preserves_other_connection_commits_and_retries_with_new_tail() {
    let fixture = Fixture::seeded();
    let first = &fixture.store;
    let expected = ["project-b", "project-a", "project-c"];
    let initial = read_ordered(first, &expected);
    let second = Store::open(fixture.temp.0.join("store.sqlite")).unwrap();
    let added_path = fixture.temp.directory("NewTail");
    let (added, registered) = register_and_reveal_project(&second, &added_path, None).unwrap();
    assert_order(
        &registered,
        &["project-b", "project-a", "project-c", &added],
    );
    let committed = commit(
        &second,
        SidebarOrganizationAction::MoveProject {
            project_id: "project-c".into(),
            before_id: Some("project-b".into()),
        },
    );
    let after_other_commit = ["project-c", "project-b", "project-a", &added];
    assert_order(&committed, &after_other_commit);
    assert_eq!(committed.revision, initial.revision + 2);
    let metadata_before = persisted(&second);
    let retained_message = messages::find(second.conn(), "retained-message").unwrap();

    let error = apply(
        first,
        initial.revision,
        SidebarOrganizationAction::MoveProject {
            project_id: "project-a".into(),
            before_id: Some("project-b".into()),
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, SidebarOrganizationError::Conflict { expected, actual }
        if expected == initial.revision && actual == committed.revision)
    );
    assert_eq!(read_ordered(first, &after_other_commit), committed);
    assert_eq!(read_ordered(&second, &after_other_commit), committed);
    assert_eq!(persisted(first), metadata_before);
    assert_eq!(persisted(&second), metadata_before);

    let refreshed = snapshot(first).unwrap();
    let retry = apply(
        first,
        refreshed.revision,
        SidebarOrganizationAction::MoveProject {
            project_id: "project-a".into(),
            before_id: Some("project-b".into()),
        },
    )
    .unwrap();
    assert!(retry.created_thread.is_none());
    let expected = ["project-c", "project-a", "project-b", &added];
    assert_order(&retry.snapshot, &expected);
    assert_eq!(retry.snapshot.revision, committed.revision + 1);
    assert_navigation_unchanged(&committed, &retry.snapshot);
    assert_eq!(read_ordered(first, &expected), retry.snapshot);
    assert_eq!(read_ordered(&second, &expected), retry.snapshot);
    assert_eq!(persisted(first), persisted(&second));
    assert_eq!(
        messages::find(first.conn(), "retained-message").unwrap(),
        retained_message
    );
}

#[test]
fn failed_project_order_rebuild_rolls_back_deleted_and_partial_rows_then_retries() {
    let fixture = Fixture::seeded();
    let expected = ["project-b", "project-a", "project-c"];
    let initial = read_ordered(&fixture.store, &expected);
    let metadata_before = persisted(&fixture.store);
    let retained_message = messages::find(fixture.store.conn(), "retained-message").unwrap();
    fixture.store.conn().execute_batch(
        "CREATE TEMP TRIGGER reject_later_order_insert BEFORE INSERT ON main.sidebar_project_order
         WHEN EXISTS(SELECT 1 FROM sidebar_project_order)
         BEGIN SELECT RAISE(ABORT,'order rebuild rejected'); END;",
    ).unwrap();

    let error = apply(
        &fixture.store,
        initial.revision,
        SidebarOrganizationAction::MoveProject {
            project_id: "project-c".into(),
            before_id: Some("project-b".into()),
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, SidebarOrganizationError::Store(ref message) if message.contains("order rebuild rejected"))
    );
    assert_eq!(read_ordered(&fixture.store, &expected), initial);
    assert_eq!(persisted(&fixture.store), metadata_before);
    assert_eq!(
        messages::find(fixture.store.conn(), "retained-message").unwrap(),
        retained_message
    );

    fixture
        .store
        .conn()
        .execute_batch("DROP TRIGGER reject_later_order_insert")
        .unwrap();
    let retry = apply(
        &fixture.store,
        initial.revision,
        SidebarOrganizationAction::MoveProject {
            project_id: "project-c".into(),
            before_id: Some("project-b".into()),
        },
    )
    .unwrap();
    assert!(retry.created_thread.is_none());
    assert_order(&retry.snapshot, &["project-c", "project-b", "project-a"]);
    assert_eq!(retry.snapshot.revision, initial.revision + 1);
    assert_navigation_unchanged(&initial, &retry.snapshot);
    assert_eq!(
        read_ordered(&fixture.store, &["project-c", "project-b", "project-a"]),
        retry.snapshot
    );
    assert_eq!(
        messages::find(fixture.store.conn(), "retained-message").unwrap(),
        retained_message
    );
}
