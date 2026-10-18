use super::*;
use crate::threads;
use SidebarOrganizationAction as Action;
use vega_store::{messages, pending_messages, threads as stored_threads, token_usage, tool_calls};

#[path = "message_order_failure_tests.rs"]
mod failures;

fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("owned.sqlite")).unwrap();
    store.migrate().unwrap();
    for (id, path, name, opened) in [
        ("p1", "owned-one", "One", 10),
        ("p2", "owned-two", "Two", 20),
        ("p3", "owned-three", "Three", 30),
    ] {
        store.conn().execute("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES (?1,?2,?3,0,?4)",(id,path,name,opened)).unwrap();
    }
    (dir, store)
}

fn change(store: &Store, action: Action) -> SidebarOrganizationSnapshot {
    apply(store, snapshot(store).unwrap().revision, action)
        .unwrap()
        .snapshot
}

fn new_group(store: &Store, name: &str) -> String {
    let old = snapshot(store).unwrap();
    let next = change(
        store,
        Action::CreateGroup {
            name: name.into(),
            color: SidebarGroupColor::Gray,
        },
    );
    next.groups
        .iter()
        .find(|g| !old.groups.iter().any(|old| old.id == g.id))
        .unwrap()
        .id
        .clone()
}

fn ordered_ids(
    state: &SidebarOrganizationSnapshot,
    project: Option<&str>,
    pinned: bool,
) -> Vec<String> {
    sorted_threads(&state.threads, &state.preferences)
        .into_iter()
        .filter(|thread| thread.pinned == pinned && (pinned || thread.project_binding() == project))
        .map(|thread| thread.id)
        .collect()
}

fn create_task(store: &Store, id: &str, project: Option<&str>, pinned: bool, created_at: i64) {
    let row = stored_threads::NewThread {
        id,
        project_id: project.unwrap_or_default(),
        title: id,
        mode: "ask",
        permission_mode: "confirm",
        model: "mock",
        status: "active",
        pinned,
        unread: true,
        created_at,
        updated_at: created_at,
    };
    if project.is_some() {
        stored_threads::create(store.conn(), row).unwrap();
    } else {
        stored_threads::create_standalone(store.conn(), row).unwrap();
    }
}

fn seed_preferences(store: &Store, sort: SidebarTaskSort, order: &[&str]) {
    let preferences = serde_json::json!({
        "view": "Groups",
        "project_view": "Timeline",
        "sort": sort,
        "manual_thread_order": order,
        "future_settings": {"density": "compact", "keep": [true, 7, "owned"]}
    });
    store
        .conn()
        .execute(
            "UPDATE sidebar_organization SET preferences=?1 WHERE singleton=1",
            [preferences.to_string()],
        )
        .unwrap();
}

fn seed_current_preferences(store: &Store, sort: SidebarTaskSort) {
    let order = snapshot(store).unwrap().preferences.manual_thread_order;
    let ids = order.iter().map(String::as_str).collect::<Vec<_>>();
    seed_preferences(store, sort, &ids);
}

fn fixture() -> (tempfile::TempDir, Store) {
    let (dir, store) = open();
    for (id, project, pinned, created_at) in [
        ("project-a", Some("p1"), false, 300),
        ("project-b", Some("p1"), false, 200),
        ("project-c", Some("p1"), false, 100),
        ("pinned-a", Some("p2"), true, 300),
        ("pinned-b", None, true, 200),
        ("pinned-c", Some("p1"), true, 100),
        ("recent-a", None, false, 300),
        ("recent-b", None, false, 200),
        ("recent-c", None, false, 100),
        ("other-a", Some("p2"), false, 300),
        ("other-b", Some("p2"), false, 200),
    ] {
        create_task(&store, id, project, pinned, created_at);
    }
    let group = new_group(&store, "Retained group");
    for id in ["project-a", "recent-c"] {
        change(
            &store,
            Action::MoveThread {
                thread_id: id.into(),
                group_id: Some(group.clone()),
                before_id: None,
            },
        );
    }
    change(
        &store,
        Action::MoveProject {
            project_id: "p1".into(),
            before_id: Some("p3".into()),
        },
    );
    for target in [
        SidebarCollapseTarget::Project("p1".into()),
        SidebarCollapseTarget::Group(group),
        SidebarCollapseTarget::Pinned,
    ] {
        change(
            &store,
            Action::SetCollapsed {
                target,
                collapsed: true,
            },
        );
    }
    seed_preferences(
        &store,
        SidebarTaskSort::Manual,
        &[
            "pinned-a",
            "pinned-b",
            "pinned-c",
            "project-a",
            "project-b",
            "project-c",
            "recent-a",
            "recent-b",
            "recent-c",
            "other-a",
            "other-b",
        ],
    );
    assert_regions(
        &store,
        &["project-a", "project-b", "project-c"],
        &["pinned-a", "pinned-b", "pinned-c"],
        &["recent-a", "recent-b", "recent-c"],
    );
    (dir, store)
}

fn message(
    store: &Store,
    id: &str,
    thread_id: &str,
    role: &str,
    status: &str,
    created_at: i64,
) -> messages::MessageRow {
    messages::MessageRow {
        id: id.into(),
        thread_id: thread_id.into(),
        seq: messages::next_seq(store.conn(), thread_id).unwrap(),
        role: role.into(),
        kind: "text".into(),
        content: format!("Owned {id}"),
        status: status.into(),
        created_at,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    }
}

fn stored_organization(store: &Store) -> serde_json::Value {
    let rows = sql::read(store.conn()).unwrap();
    serde_json::json!({
        "revision": rows.revision,
        "preferences": rows.preferences,
        "collapsed": rows.collapsed,
        "groups": rows.groups,
        "memberships": rows.memberships,
        "project_order": rows.project_order
    })
}

fn retained_metadata(store: &Store) -> serde_json::Value {
    let mut rows = stored_organization(store);
    rows.as_object_mut().unwrap().remove("revision");
    let mut preferences: serde_json::Value =
        serde_json::from_str(rows["preferences"].as_str().unwrap()).unwrap();
    preferences
        .as_object_mut()
        .unwrap()
        .remove("manual_thread_order");
    rows["preferences"] = preferences;
    rows
}

fn assert_order(store: &Store, project: Option<&str>, pinned: bool, expected: &[&str]) {
    assert_eq!(
        ordered_ids(&snapshot(store).unwrap(), project, pinned),
        expected,
    );
}

fn assert_regions(store: &Store, project: &[&str], pinned: &[&str], recent: &[&str]) {
    assert_order(store, Some("p1"), false, project);
    assert_order(store, None, true, pinned);
    assert_order(store, None, false, recent);
    assert_order(store, Some("p2"), false, &["other-a", "other-b"]);
}

fn assert_other_regions_unchanged(
    before: &SidebarOrganizationSnapshot,
    after: &SidebarOrganizationSnapshot,
    project: Option<&str>,
    pinned: bool,
) {
    for (other_project, other_pinned) in [
        (Some("p1"), false),
        (Some("p2"), false),
        (None, true),
        (None, false),
    ] {
        if (other_project, other_pinned) != (project, pinned) {
            assert_eq!(
                ordered_ids(after, other_project, other_pinned),
                ordered_ids(before, other_project, other_pinned)
            );
        }
    }
    assert_eq!(after.projects, before.projects);
    assert_eq!(after.project_order, before.project_order);
}

fn prepare_plan(store: &Store, thread_id: &str, plan_id: &str) {
    threads::set_thread_mode(store, thread_id, ThreadMode::Plan).unwrap();
    let row = message(store, plan_id, thread_id, "assistant", "streaming", 20);
    messages::insert(store.conn(), &row).unwrap();
    messages::complete_plan(store.conn(), thread_id, plan_id, "Owned plan", 30, Some(10)).unwrap();
}

fn approve_plan(store: &Store, thread_id: &str, plan_id: &str, instruction_id: &str) {
    assert_eq!(
        messages::review_plan(
            store.conn(),
            messages::PlanReview {
                thread_id,
                plan_id,
                status: "approved",
                note: None,
                reviewed_at: 40,
                instruction: Some(messages::PlanInstruction {
                    id: instruction_id,
                    content: "Execute the owned plan",
                    created_at: -100,
                }),
            },
        )
        .unwrap(),
        messages::PlanReviewResult::Applied
    );
}

#[test]
fn visits_and_thread_metadata_edits_preserve_order_with_legacy_preferences() {
    for sort in [
        SidebarTaskSort::Updated,
        SidebarTaskSort::Created,
        SidebarTaskSort::Manual,
    ] {
        let (dir, store) = fixture();
        let history = message(&store, "retained-history", "project-a", "user", "done", 1);
        messages::insert(store.conn(), &history).unwrap();
        seed_preferences(
            &store,
            sort,
            &[
                "pinned-a",
                "pinned-b",
                "pinned-c",
                "project-a",
                "project-b",
                "project-c",
                "recent-a",
                "recent-b",
                "recent-c",
                "other-a",
                "other-b",
            ],
        );
        let before = snapshot(&store).unwrap();
        let original_organization = stored_organization(&store);
        for id in ["project-c", "project-b"] {
            let previous = stored_threads::find(store.conn(), id).unwrap().unwrap();
            let visited = threads::visit_thread(&store, id).unwrap();
            assert!(!visited.unread);
            assert!(visited.updated_at > previous.updated_at);
            let refreshed = snapshot(&store).unwrap();
            assert_eq!(
                refreshed
                    .projects
                    .iter()
                    .find(|project| project.id == "p1")
                    .unwrap()
                    .last_opened_at,
                visited.updated_at
            );
            assert_order(
                &store,
                Some("p1"),
                false,
                &["project-a", "project-b", "project-c"],
            );
        }
        threads::rename_thread(&store, "project-a", "Renamed owned task").unwrap();
        threads::set_thread_model(&store, "project-b", "mock-second").unwrap();
        threads::set_thread_permission_mode(&store, "project-c", PermissionMode::ReadOnly).unwrap();
        threads::set_thread_status(&store, "project-b", ThreadStatus::Archived).unwrap();
        threads::set_thread_status(&store, "project-b", ThreadStatus::Active).unwrap();
        assert_regions(
            &store,
            &["project-a", "project-b", "project-c"],
            &["pinned-a", "pinned-b", "pinned-c"],
            &["recent-a", "recent-b", "recent-c"],
        );
        assert_eq!(stored_organization(&store), original_organization);
        let saved = snapshot(&store).unwrap();
        for previous in &before.threads {
            let current = saved
                .threads
                .iter()
                .find(|thread| thread.id == previous.id)
                .unwrap();
            assert_eq!(current.project_binding(), previous.project_binding());
            assert_eq!(current.pinned, previous.pinned);
            assert_eq!(current.created_at, previous.created_at);
            assert_eq!(current.parent_thread_id, previous.parent_thread_id);
        }
        assert_eq!(
            stored_threads::find(store.conn(), "project-a")
                .unwrap()
                .unwrap()
                .title,
            "Renamed owned task"
        );
        assert_eq!(
            stored_threads::find(store.conn(), "project-b")
                .unwrap()
                .unwrap()
                .model,
            "mock-second"
        );
        assert_eq!(
            stored_threads::find(store.conn(), "project-c")
                .unwrap()
                .unwrap()
                .permission_mode,
            "readonly"
        );
        assert_eq!(
            messages::find(store.conn(), &history.id).unwrap(),
            Some(history.clone())
        );
        drop(store);
        let reopened = Store::open(dir.path().join("owned.sqlite")).unwrap();
        assert_eq!(snapshot(&reopened).unwrap(), saved);
        assert_eq!(
            messages::find(reopened.conn(), &history.id).unwrap(),
            Some(history)
        );
    }
}

#[test]
fn unranked_new_threads_use_creation_order_and_stable_ids_not_visit_time() {
    for sort in [
        SidebarTaskSort::Updated,
        SidebarTaskSort::Created,
        SidebarTaskSort::Manual,
    ] {
        let (_dir, store) = fixture();
        seed_current_preferences(&store, sort);
        create_task(&store, "new-z", Some("p1"), false, 400);
        create_task(&store, "new-a", Some("p1"), false, 400);
        create_task(&store, "newest", Some("p1"), false, 500);
        store
            .conn()
            .execute("UPDATE threads SET updated_at=-1 WHERE id='newest'", [])
            .unwrap();
        let before = stored_organization(&store);
        let expected = [
            "newest",
            "new-a",
            "new-z",
            "project-a",
            "project-b",
            "project-c",
        ];
        assert_order(&store, Some("p1"), false, &expected);
        for id in ["new-z", "project-c", "new-a"] {
            threads::visit_thread(&store, id).unwrap();
            assert_order(&store, Some("p1"), false, &expected);
        }
        assert_eq!(stored_organization(&store), before);
    }
}

#[test]
fn committed_user_and_assistant_messages_promote_only_their_region() {
    for (c, b, project, pinned, after_user, after_assistant) in [
        (
            "project-c",
            "project-b",
            Some("p1"),
            false,
            ["project-c", "project-a", "project-b"],
            ["project-b", "project-c", "project-a"],
        ),
        (
            "pinned-c",
            "pinned-b",
            None,
            true,
            ["pinned-c", "pinned-a", "pinned-b"],
            ["pinned-b", "pinned-c", "pinned-a"],
        ),
        (
            "recent-c",
            "recent-b",
            None,
            false,
            ["recent-c", "recent-a", "recent-b"],
            ["recent-b", "recent-c", "recent-a"],
        ),
    ] {
        for archived in [false, true] {
            let (dir, store) = fixture();
            if archived {
                threads::set_thread_status(&store, c, ThreadStatus::Archived).unwrap();
            }
            let baseline = snapshot(&store).unwrap();
            let metadata = retained_metadata(&store);
            let user = message(&store, "first-user", c, "user", "done", 10);
            messages::insert(store.conn(), &user).unwrap();
            assert_order(&store, project, pinned, &after_user);
            let promoted = snapshot(&store).unwrap();
            assert_eq!(promoted.revision, baseline.revision + 1);
            assert_eq!(promoted.threads, baseline.threads);
            assert_other_regions_unchanged(&baseline, &promoted, project, pinned);
            let assistant = message(&store, "first-assistant", b, "assistant", "streaming", 10);
            messages::insert(store.conn(), &assistant).unwrap();
            assert_order(&store, project, pinned, &after_assistant);
            let saved = snapshot(&store).unwrap();
            assert_eq!(saved.revision, promoted.revision + 1);
            assert_eq!(saved.threads, baseline.threads);
            assert_other_regions_unchanged(&baseline, &saved, project, pinned);
            assert_eq!(retained_metadata(&store), metadata);
            assert_eq!(
                messages::find(store.conn(), &user.id).unwrap(),
                Some(user.clone())
            );
            assert_eq!(
                messages::find(store.conn(), &assistant.id).unwrap(),
                Some(assistant.clone())
            );
            drop(store);
            let reopened = Store::open(dir.path().join("owned.sqlite")).unwrap();
            assert_eq!(snapshot(&reopened).unwrap(), saved);
            assert_eq!(
                messages::find(reopened.conn(), &user.id).unwrap(),
                Some(user)
            );
            assert_eq!(
                messages::find(reopened.conn(), &assistant.id).unwrap(),
                Some(assistant)
            );
        }
    }
}

#[test]
fn plan_approval_instruction_promotes_its_source_in_each_region() {
    for (thread_id, project, pinned, expected) in [
        (
            "project-c",
            Some("p1"),
            false,
            ["project-c", "project-a", "project-b"],
        ),
        ("pinned-c", None, true, ["pinned-c", "pinned-a", "pinned-b"]),
        (
            "recent-c",
            None,
            false,
            ["recent-c", "recent-a", "recent-b"],
        ),
    ] {
        for archived in [false, true] {
            let (dir, store) = fixture();
            prepare_plan(&store, thread_id, "owned-plan");
            if archived {
                threads::set_thread_status(&store, thread_id, ThreadStatus::Archived).unwrap();
            }
            change(
                &store,
                Action::ReorderThread {
                    thread_id: thread_id.into(),
                    before_id: None,
                },
            );
            seed_current_preferences(&store, SidebarTaskSort::Manual);
            let before = snapshot(&store).unwrap();
            let metadata = retained_metadata(&store);
            let mut expected_thread = stored_threads::find(store.conn(), thread_id)
                .unwrap()
                .unwrap();
            expected_thread.mode = "execute".into();
            expected_thread.updated_at = 40;
            approve_plan(&store, thread_id, "owned-plan", "execution-instruction");
            assert_order(&store, project, pinned, &expected);
            let saved = snapshot(&store).unwrap();
            assert_eq!(saved.revision, before.revision + 1);
            assert_other_regions_unchanged(&before, &saved, project, pinned);
            assert_eq!(retained_metadata(&store), metadata);
            let plan = messages::find(store.conn(), "owned-plan").unwrap().unwrap();
            assert_eq!(plan.plan_status.as_deref(), Some("approved"));
            assert_eq!(plan.plan_reviewed_at, Some(40));
            let instruction = messages::find(store.conn(), "execution-instruction")
                .unwrap()
                .unwrap();
            assert_eq!(instruction.thread_id, thread_id);
            assert_eq!(instruction.role, "user");
            assert_eq!(instruction.content, "Execute the owned plan");
            assert_eq!(instruction.seq, plan.seq + 1);
            assert_eq!(instruction.created_at, -100);
            let current = stored_threads::find(store.conn(), thread_id)
                .unwrap()
                .unwrap();
            assert_eq!(current, expected_thread);
            for previous in &before.threads {
                if previous.id != thread_id {
                    assert_eq!(
                        saved.threads.iter().find(|thread| thread.id == previous.id),
                        Some(previous)
                    );
                }
            }
            drop(store);
            let reopened = Store::open(dir.path().join("owned.sqlite")).unwrap();
            assert_eq!(snapshot(&reopened).unwrap(), saved);
            assert_eq!(
                messages::find(reopened.conn(), "execution-instruction").unwrap(),
                Some(instruction)
            );
        }
    }
}

#[test]
fn hidden_child_messages_skip_revision_while_pinned_children_move_only_themselves() {
    for approval in [false, true] {
        let (_dir, store) = fixture();
        create_task(&store, "hidden-child", Some("p1"), false, 50);
        create_task(&store, "pinned-child", Some("p1"), true, 50);
        store.conn().execute("UPDATE threads SET parent_thread_id='project-b' WHERE id IN ('hidden-child','pinned-child')", []).unwrap();
        if approval {
            prepare_plan(&store, "hidden-child", "hidden-plan");
            prepare_plan(&store, "pinned-child", "pinned-plan");
        }
        seed_preferences(
            &store,
            SidebarTaskSort::Manual,
            &[
                "pinned-a",
                "pinned-b",
                "pinned-c",
                "pinned-child",
                "project-a",
                "project-b",
                "project-c",
                "recent-a",
                "recent-b",
                "recent-c",
                "other-a",
                "other-b",
            ],
        );
        let before = snapshot(&store).unwrap();
        let organization = stored_organization(&store);
        let metadata = retained_metadata(&store);
        if approval {
            approve_plan(&store, "hidden-child", "hidden-plan", "hidden-instruction");
        } else {
            let row = message(&store, "hidden-message", "hidden-child", "user", "done", 1);
            messages::insert(store.conn(), &row).unwrap();
        }
        assert_eq!(snapshot(&store).unwrap(), before);
        assert_eq!(stored_organization(&store), organization);
        let hidden_id = if approval {
            "hidden-instruction"
        } else {
            "hidden-message"
        };
        assert!(messages::find(store.conn(), hidden_id).unwrap().is_some());
        if approval {
            approve_plan(&store, "pinned-child", "pinned-plan", "pinned-instruction");
        } else {
            let row = message(
                &store,
                "pinned-message",
                "pinned-child",
                "assistant",
                "streaming",
                1,
            );
            messages::insert(store.conn(), &row).unwrap();
        }
        assert_regions(
            &store,
            &["project-a", "project-b", "project-c"],
            &["pinned-child", "pinned-a", "pinned-b", "pinned-c"],
            &["recent-a", "recent-b", "recent-c"],
        );
        let after = snapshot(&store).unwrap();
        assert_eq!(after.revision, before.revision + 1);
        assert_eq!(after.projects, before.projects);
        assert_eq!(after.project_order, before.project_order);
        assert_eq!(retained_metadata(&store), metadata);
        assert_eq!(
            after.threads.iter().find(|thread| thread.id == "project-b"),
            before
                .threads
                .iter()
                .find(|thread| thread.id == "project-b")
        );
        assert!(
            !after
                .preferences
                .manual_thread_order
                .iter()
                .any(|id| id == "hidden-child")
        );
        let child = stored_threads::find(store.conn(), "pinned-child")
            .unwrap()
            .unwrap();
        assert_eq!(child.parent_thread_id.as_deref(), Some("project-b"));
        assert_eq!(child.project_id, "p1");
        assert!(child.pinned);
    }
}

#[test]
fn first_message_registers_an_unranked_source_and_retains_archived_positions() {
    for approval in [false, true] {
        let (dir, store) = fixture();
        create_task(&store, "fresh-source", Some("p1"), false, 400);
        create_task(&store, "archived-neighbor", Some("p1"), false, 250);
        if approval {
            prepare_plan(&store, "fresh-source", "fresh-plan");
        }
        threads::set_thread_status(&store, "archived-neighbor", ThreadStatus::Archived).unwrap();
        seed_preferences(
            &store,
            SidebarTaskSort::Manual,
            &[
                "pinned-a",
                "pinned-b",
                "pinned-c",
                "project-a",
                "archived-neighbor",
                "project-b",
                "project-c",
                "recent-a",
                "recent-b",
                "recent-c",
                "other-a",
                "other-b",
            ],
        );
        let metadata = retained_metadata(&store);
        let before = snapshot(&store).unwrap();
        assert!(
            !before
                .preferences
                .manual_thread_order
                .iter()
                .any(|id| id == "fresh-source")
        );
        assert_order(
            &store,
            Some("p1"),
            false,
            &[
                "fresh-source",
                "project-a",
                "archived-neighbor",
                "project-b",
                "project-c",
            ],
        );
        if approval {
            approve_plan(&store, "fresh-source", "fresh-plan", "fresh-instruction");
        } else {
            let row = message(&store, "fresh-message", "fresh-source", "user", "done", -1);
            messages::insert(store.conn(), &row).unwrap();
        }
        let registered = snapshot(&store).unwrap();
        assert_eq!(registered.revision, before.revision + 1);
        assert!(
            registered
                .preferences
                .manual_thread_order
                .iter()
                .any(|id| id == "fresh-source")
        );
        assert_eq!(
            registered.preferences.manual_thread_order.len(),
            registered.threads.len()
        );
        assert_eq!(retained_metadata(&store), metadata);
        let next = message(
            &store,
            "later-message",
            "project-b",
            "assistant",
            "streaming",
            -2,
        );
        messages::insert(store.conn(), &next).unwrap();
        let expected = [
            "project-b",
            "fresh-source",
            "project-a",
            "archived-neighbor",
            "project-c",
        ];
        assert_order(&store, Some("p1"), false, &expected);
        threads::set_thread_status(&store, "archived-neighbor", ThreadStatus::Active).unwrap();
        assert_order(&store, Some("p1"), false, &expected);
        drop(store);
        let reopened = Store::open(dir.path().join("owned.sqlite")).unwrap();
        assert_order(&reopened, Some("p1"), false, &expected);
        let saved = snapshot(&reopened).unwrap();
        assert_eq!(
            saved.preferences.manual_thread_order.len(),
            saved.threads.len()
        );
        assert_eq!(saved.groups, before.groups);
        assert_eq!(saved.memberships, before.memberships);
        assert_eq!(saved.project_order, before.project_order);
    }
}

#[test]
fn streaming_updates_responses_tools_and_usage_preserve_dragged_order_after_reopen() {
    let (dir, store) = fixture();
    create_task(&store, "project-hidden", Some("p1"), false, 250);
    create_task(&store, "project-offscreen", Some("p1"), false, 150);
    threads::set_thread_status(&store, "project-hidden", ThreadStatus::Archived).unwrap();
    seed_preferences(
        &store,
        SidebarTaskSort::Manual,
        &[
            "pinned-a",
            "pinned-b",
            "pinned-c",
            "project-a",
            "project-hidden",
            "project-b",
            "project-offscreen",
            "project-c",
            "recent-a",
            "recent-b",
            "recent-c",
            "other-a",
            "other-b",
        ],
    );
    let assistant = message(
        &store,
        "streaming-reply",
        "project-c",
        "assistant",
        "streaming",
        1,
    );
    messages::insert(store.conn(), &assistant).unwrap();
    assert_order(
        &store,
        Some("p1"),
        false,
        &[
            "project-c",
            "project-a",
            "project-hidden",
            "project-b",
            "project-offscreen",
        ],
    );
    change(
        &store,
        Action::ReorderThread {
            thread_id: "project-c".into(),
            before_id: Some("project-offscreen".into()),
        },
    );
    let expected = [
        "project-a",
        "project-hidden",
        "project-b",
        "project-c",
        "project-offscreen",
    ];
    assert_order(&store, Some("p1"), false, &expected);
    let organization = stored_organization(&store);
    let thread_metadata = snapshot(&store).unwrap().threads;
    assert_eq!(
        messages::update_streaming_content(store.conn(), &assistant.id, "Working").unwrap(),
        1
    );
    let commentary = messages::AssistantResponseRow {
        message_id: assistant.id.clone(),
        response_id: "commentary-part".into(),
        text_start_bytes: 0,
        text_end_bytes: 7,
        kind: "commentary".into(),
    };
    for _ in 0..2 {
        assert!(
            messages::record_assistant_response(
                store.conn(),
                &assistant.id,
                "Working",
                &commentary
            )
            .unwrap()
        );
        assert_eq!(stored_organization(&store), organization);
    }
    tool_calls::insert(
        store.conn(),
        tool_calls::NewToolCall {
            id: "owned-tool",
            thread_id: "project-c",
            message_id: &assistant.id,
            seq: 1,
            tool: "read",
            input_json: r#"{"path":"owned.txt"}"#,
            status: "running",
            created_at: 2,
        },
    )
    .unwrap();
    tool_calls::finish(
        store.conn(),
        tool_calls::FinishToolCall {
            id: "owned-tool",
            status: "success",
            output_text: "Owned result",
            exit_code: None,
            duration_ms: Some(5),
            finished_at: 3,
        },
    )
    .unwrap();
    token_usage::insert(
        store.conn(),
        token_usage::NewTokenUsage {
            thread_id: "project-c",
            message_id: Some(&assistant.id),
            model: "mock",
            input_tokens: 11,
            output_tokens: 7,
            cache_read_tokens: 2,
            cache_write_tokens: 1,
            cost_microcents: 0,
            created_at: 4,
            pricing_version: None,
            pricing_profile: None,
            call_started_at: None,
        },
    )
    .unwrap();
    assert_eq!(stored_organization(&store), organization);
    assert_eq!(
        messages::update_streaming_content(store.conn(), &assistant.id, "WorkingDone").unwrap(),
        1
    );
    let final_response = messages::AssistantResponseRow {
        message_id: assistant.id.clone(),
        response_id: "final-part".into(),
        text_start_bytes: 7,
        text_end_bytes: 11,
        kind: "final".into(),
    };
    assert!(
        messages::record_assistant_response(
            store.conn(),
            &assistant.id,
            "WorkingDone",
            &final_response
        )
        .unwrap()
    );
    assert_eq!(
        messages::finish_streaming(store.conn(), &assistant.id, "WorkingDone", "done", Some(20))
            .unwrap(),
        1
    );
    assert_eq!(
        messages::finish_streaming(store.conn(), &assistant.id, "WorkingDone", "done", Some(20))
            .unwrap(),
        0
    );
    assert_eq!(
        messages::update_streaming_content(store.conn(), &assistant.id, "late delta").unwrap(),
        0
    );
    assert_eq!(stored_organization(&store), organization);
    assert_eq!(snapshot(&store).unwrap().threads, thread_metadata);
    drop(store);
    let reopened = Store::open(dir.path().join("owned.sqlite")).unwrap();
    assert_order(&reopened, Some("p1"), false, &expected);
    for _ in 0..2 {
        let page =
            messages::page_before(reopened.conn(), "project-c", messages::PageCursor::Head, 20)
                .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].content, "WorkingDone");
        assert_eq!(page.rows[0].status, "done");
        assert_eq!(
            page.assistant_responses,
            [commentary.clone(), final_response.clone()]
        );
        assert_eq!(page.execution_durations_ms.get(&assistant.id), Some(&20));
        assert_eq!(page.tool_calls.len(), 1);
        assert_eq!(
            tool_calls::find_state(reopened.conn(), "owned-tool")
                .unwrap()
                .unwrap()
                .status,
            "success"
        );
        let usage =
            token_usage::aggregate_by_message(reopened.conn(), "project-c", &assistant.id).unwrap();
        assert_eq!(
            (usage.row_count, usage.input_tokens, usage.output_tokens),
            (1, 11, 7)
        );
        assert_eq!(stored_organization(&reopened), organization);
    }
    threads::set_thread_status(&reopened, "project-hidden", ThreadStatus::Active).unwrap();
    assert_order(&reopened, Some("p1"), false, &expected);
    let next = message(&reopened, "next-message", "project-c", "user", "done", -1);
    messages::insert(reopened.conn(), &next).unwrap();
    assert_order(
        &reopened,
        Some("p1"),
        false,
        &[
            "project-c",
            "project-a",
            "project-hidden",
            "project-b",
            "project-offscreen",
        ],
    );
}

#[test]
fn pending_inputs_and_system_history_do_not_promote_until_a_user_message_is_committed() {
    let (_dir, store) = fixture();
    let before = stored_organization(&store);
    pending_messages::insert(
        store.conn(),
        &pending_messages::PendingMessageRow {
            id: "queued-input".into(),
            thread_id: "project-c".into(),
            ordinal: 0,
            behavior: "queue".into(),
            generation: "owned-generation".into(),
            payload: "Owned pending input".into(),
            created_at: 1,
        },
    )
    .unwrap();
    assert_eq!(
        pending_messages::count(store.conn(), "project-c").unwrap(),
        1
    );
    let system = message(&store, "system-history", "project-c", "system", "done", 1);
    messages::insert(store.conn(), &system).unwrap();
    assert_eq!(stored_organization(&store), before);
    assert_order(
        &store,
        Some("p1"),
        false,
        &["project-a", "project-b", "project-c"],
    );
    let delivered = message(&store, "delivered-user", "project-c", "user", "done", 1);
    let tx = store.immediate_transaction().unwrap();
    messages::insert(&tx, &delivered).unwrap();
    assert!(pending_messages::remove(&tx, "project-c", "queued-input").unwrap());
    tx.commit().unwrap();
    assert_eq!(
        pending_messages::count(store.conn(), "project-c").unwrap(),
        0
    );
    assert_order(
        &store,
        Some("p1"),
        false,
        &["project-c", "project-a", "project-b"],
    );
}

#[test]
fn plan_completion_and_nonapproving_reviews_preserve_the_last_drag() {
    for status in ["changes_requested", "abandoned"] {
        let (_dir, store) = fixture();
        threads::set_thread_mode(&store, "project-c", ThreadMode::Plan).unwrap();
        let plan = message(
            &store,
            "streaming-plan",
            "project-c",
            "assistant",
            "streaming",
            1,
        );
        messages::insert(store.conn(), &plan).unwrap();
        change(
            &store,
            Action::ReorderThread {
                thread_id: "project-c".into(),
                before_id: None,
            },
        );
        let before = stored_organization(&store);
        messages::complete_plan(
            store.conn(),
            "project-c",
            &plan.id,
            "Owned plan",
            40,
            Some(10),
        )
        .unwrap();
        assert_eq!(stored_organization(&store), before);
        assert_order(
            &store,
            Some("p1"),
            false,
            &["project-a", "project-b", "project-c"],
        );
        assert_eq!(
            messages::review_plan(
                store.conn(),
                messages::PlanReview {
                    thread_id: "project-c",
                    plan_id: &plan.id,
                    status,
                    note: Some("Owned review"),
                    reviewed_at: 50,
                    instruction: None,
                }
            )
            .unwrap(),
            messages::PlanReviewResult::Applied
        );
        assert_eq!(stored_organization(&store), before);
        let reviewed = messages::find(store.conn(), &plan.id).unwrap().unwrap();
        assert_eq!(reviewed.plan_status.as_deref(), Some(status));
        assert_eq!(
            messages::recent(store.conn(), "project-c", 20)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            messages::review_plan(
                store.conn(),
                messages::PlanReview {
                    thread_id: "project-c",
                    plan_id: &plan.id,
                    status: "approved",
                    note: None,
                    reviewed_at: 60,
                    instruction: Some(messages::PlanInstruction {
                        id: "stale-instruction",
                        content: "Must not run",
                        created_at: 60
                    }),
                }
            )
            .unwrap(),
            messages::PlanReviewResult::Stale
        );
        assert!(
            messages::find(store.conn(), "stale-instruction")
                .unwrap()
                .is_none()
        );
        assert_eq!(stored_organization(&store), before);
    }
}
