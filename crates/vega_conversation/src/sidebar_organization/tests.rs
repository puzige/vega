//! E2E-REAL: production service calls against owned, migrated SQLite files.
use super::*;
use crate::threads;
use SidebarOrganizationAction as Action;

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
fn members(state: &SidebarOrganizationSnapshot, group_id: &str) -> Vec<String> {
    state
        .memberships
        .iter()
        .filter(|m| m.group_id == group_id)
        .map(|m| m.thread_id.clone())
        .collect()
}
fn rejected_unchanged(store: &Store, action: Action) {
    let before = snapshot(store).unwrap();
    assert!(matches!(
        apply(store, before.revision, action),
        Err(SidebarOrganizationError::Invalid(_))
    ));
    assert_eq!(snapshot(store).unwrap(), before);
}
#[test]
fn sidebar_organization_owned_file_migration_preserves_old_content_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let store = Store::open(&path).unwrap();
    store
        .conn()
        .execute_batch(include_str!("../../../vega_store/migrations/0001_init.sql"))
        .unwrap();
    store.conn().execute_batch("PRAGMA user_version=1; INSERT INTO projects VALUES ('p','owned','Legacy',NULL,1,2); INSERT INTO threads VALUES ('t','p','Task','plan','confirm','mock','active',1,1,3,4); INSERT INTO messages VALUES ('m','t',1,'user','text','owned content','done',5);").unwrap();
    store.migrate().unwrap();
    store.migrate().unwrap();
    let state = snapshot(&store).unwrap();
    assert_eq!(state.revision, 0);
    assert_eq!(state.preferences, SidebarPreferences::default());
    assert!(
        state.groups.is_empty()
            && state.memberships.is_empty()
            && state.project_order.is_empty()
            && state.collapsed.is_empty()
    );
    assert_eq!(state.threads.len(), 1);
    assert_eq!(
        (
            state.threads[0].title.as_str(),
            state.threads[0].created_at,
            state.threads[0].updated_at,
            state.threads[0].unread
        ),
        ("Task", 3, 4, true)
    );
    drop(store);
    let reopened = Store::open(path).unwrap();
    reopened.migrate().unwrap();
    assert_eq!(snapshot(&reopened).unwrap(), state);
    let content: String = reopened
        .conn()
        .query_row("SELECT content FROM messages WHERE id='m'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(content, "owned content");
}
#[test]
fn sidebar_organization_owned_file_groups_cross_project_order_and_restart() {
    let (dir, store) = open();
    let a = threads::create_thread(&store, "p1", "mock", "confirm").unwrap();
    let b = threads::create_thread(&store, "p2", "mock", "confirm").unwrap();
    let c = threads::create_thread(&store, "p1", "mock", "confirm").unwrap();
    let original_threads = snapshot(&store).unwrap().threads;
    let first = new_group(&store, "  Work  ");
    let second = new_group(&store, "Second");
    let third = new_group(&store, "Third");
    assert_eq!(snapshot(&store).unwrap().groups[0].name, "Work");
    for t in [&a, &b, &c] {
        change(
            &store,
            Action::MoveThread {
                thread_id: t.id.clone(),
                group_id: Some(first.clone()),
                before_id: None,
            },
        );
    }
    let s = change(
        &store,
        Action::MoveThread {
            thread_id: c.id.clone(),
            group_id: Some(first.clone()),
            before_id: Some(a.id.clone()),
        },
    );
    assert_eq!(
        members(&s, &first),
        [c.id.clone(), a.id.clone(), b.id.clone()]
    );
    change(
        &store,
        Action::MoveThread {
            thread_id: a.id.clone(),
            group_id: Some(second.clone()),
            before_id: None,
        },
    );
    let s = change(
        &store,
        Action::MoveThread {
            thread_id: b.id.clone(),
            group_id: Some(second.clone()),
            before_id: Some(a.id.clone()),
        },
    );
    assert_eq!(members(&s, &first), std::slice::from_ref(&c.id));
    assert_eq!(members(&s, &second), [b.id.clone(), a.id.clone()]);
    let s = change(
        &store,
        Action::MoveGroup {
            group_id: third.clone(),
            before_id: Some(first.clone()),
        },
    );
    assert_eq!(
        s.groups.iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
        [third.clone(), first.clone(), second.clone()]
    );
    let s = change(
        &store,
        Action::MoveProject {
            project_id: "p1".into(),
            before_id: Some("p3".into()),
        },
    );
    assert_eq!(s.project_order, ["p1", "p3", "p2"]);
    let s = change(
        &store,
        Action::MoveProject {
            project_id: "p3".into(),
            before_id: None,
        },
    );
    assert_eq!(s.project_order, ["p1", "p2", "p3"]);
    change(
        &store,
        Action::RenameGroup {
            group_id: first.clone(),
            name: "🙂".repeat(64),
        },
    );
    for color in [
        SidebarGroupColor::Gray,
        SidebarGroupColor::Red,
        SidebarGroupColor::Orange,
        SidebarGroupColor::Yellow,
        SidebarGroupColor::Green,
        SidebarGroupColor::Blue,
        SidebarGroupColor::Purple,
    ] {
        let s = change(
            &store,
            Action::SetGroupColor {
                group_id: first.clone(),
                color,
            },
        );
        assert_eq!(
            s.groups.iter().find(|g| g.id == first).unwrap().color,
            color
        );
    }
    let collapsed_projects = change(&store, Action::CollapseAll);
    for p in &collapsed_projects.projects {
        assert!(
            collapsed_projects
                .collapsed
                .contains(&SidebarCollapseTarget::Project(p.id.clone()))
        );
    }
    change(
        &store,
        Action::SetPreferences(SidebarPreferences {
            view: SidebarView::Projects,
            project_view: SidebarProjectView::Timeline,
            sort: SidebarTaskSort::Updated,
        }),
    );
    let collapsed_timeline = change(&store, Action::CollapseAll);
    for bucket in [
        SidebarTimelineBucket::Today,
        SidebarTimelineBucket::Yesterday,
        SidebarTimelineBucket::Last7Days,
        SidebarTimelineBucket::Last30Days,
        SidebarTimelineBucket::Earlier,
    ] {
        assert!(
            collapsed_timeline
                .collapsed
                .contains(&SidebarCollapseTarget::Timeline(bucket))
        );
    }
    assert!(
        collapsed_timeline
            .collapsed
            .contains(&SidebarCollapseTarget::Pinned)
    );
    change(
        &store,
        Action::SetPreferences(SidebarPreferences {
            view: SidebarView::Groups,
            project_view: SidebarProjectView::Timeline,
            sort: SidebarTaskSort::Created,
        }),
    );
    change(&store, Action::CollapseAll);
    let state = change(
        &store,
        Action::SetCollapsed {
            target: SidebarCollapseTarget::Group(second.clone()),
            collapsed: false,
        },
    );
    assert!(
        state
            .collapsed
            .contains(&SidebarCollapseTarget::Group(first.clone()))
    );
    assert!(state.collapsed.contains(&SidebarCollapseTarget::Ungrouped));
    assert!(
        !state
            .collapsed
            .contains(&SidebarCollapseTarget::Group(second.clone()))
    );
    assert_eq!(
        state.threads, original_threads,
        "organization must not mutate task timestamps/unread/project ownership"
    );
    drop(store);
    let store = Store::open(dir.path().join("owned.sqlite")).unwrap();
    store.migrate().unwrap();
    assert_eq!(snapshot(&store).unwrap(), state);
    let s = change(
        &store,
        Action::DissolveGroup {
            group_id: second.clone(),
        },
    );
    assert_eq!(s.threads, original_threads);
    assert!(members(&s, &second).is_empty());
    assert!(!s.groups.iter().any(|g| g.id == second));
    let s = change(
        &store,
        Action::MoveThread {
            thread_id: c.id.clone(),
            group_id: None,
            before_id: None,
        },
    );
    assert!(s.memberships.is_empty());
}
#[test]
fn sidebar_organization_owned_file_atomic_creation_archive_restore_and_delete() {
    let (dir, store) = open();
    let group = new_group(&store, "Retained");
    let state = change(
        &store,
        Action::CreateThreadInGroup {
            project_id: "p2".into(),
            group_id: group.clone(),
            model: "mock".into(),
            permission_mode: String::new(),
        },
    );
    assert_eq!(state.threads.len(), 1);
    let id = state.threads[0].id.clone();
    assert_eq!(state.threads[0].project_id, "p2");
    assert_eq!(state.threads[0].permission_mode, PermissionMode::Confirm);
    assert_eq!(members(&state, &group), std::slice::from_ref(&id));
    let revision = state.revision;
    threads::set_thread_status(&store, &id, ThreadStatus::Archived).unwrap();
    let archived = snapshot(&store).unwrap();
    assert_eq!(archived.threads[0].status, ThreadStatus::Archived);
    assert_eq!(members(&archived, &group), std::slice::from_ref(&id));
    drop(store);
    let store = Store::open(dir.path().join("owned.sqlite")).unwrap();
    store.migrate().unwrap();
    assert_eq!(snapshot(&store).unwrap(), archived);
    threads::set_thread_status(&store, &id, ThreadStatus::Active).unwrap();
    threads::open_thread(&store, &id).unwrap();
    assert_eq!(
        snapshot(&store).unwrap().revision,
        revision,
        "navigation and task metadata keep organization revision"
    );
    assert_eq!(
        members(&snapshot(&store).unwrap(), &group),
        std::slice::from_ref(&id)
    );
    store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,created_at) VALUES ('owned-message',?1,1,'user','keep',0)",[&id]).unwrap();
    change(&store, Action::DissolveGroup { group_id: group });
    let content: String = store
        .conn()
        .query_row(
            "SELECT content FROM messages WHERE id='owned-message'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(content, "keep");
    let group = new_group(&store, "Delete");
    change(
        &store,
        Action::MoveThread {
            thread_id: id.clone(),
            group_id: Some(group.clone()),
            before_id: None,
        },
    );
    threads::delete_thread(&store, &id).unwrap();
    let deleted = snapshot(&store).unwrap();
    assert!(deleted.threads.is_empty() && deleted.memberships.is_empty());
    assert_eq!(deleted.groups.len(), 1);
}
#[test]
fn sidebar_organization_owned_file_invalid_actions_and_conflict_have_no_partial_writes() {
    let (_dir, store) = open();
    let first = new_group(&store, "First");
    let second = new_group(&store, "Second");
    let a = threads::create_thread(&store, "p1", "mock", "confirm").unwrap();
    let b = threads::create_thread(&store, "p2", "mock", "confirm").unwrap();
    change(
        &store,
        Action::MoveThread {
            thread_id: a.id.clone(),
            group_id: Some(first.clone()),
            before_id: None,
        },
    );
    change(
        &store,
        Action::MoveThread {
            thread_id: b.id.clone(),
            group_id: Some(second.clone()),
            before_id: None,
        },
    );
    for action in [
        Action::CreateGroup {
            name: " \n ".into(),
            color: SidebarGroupColor::Gray,
        },
        Action::RenameGroup {
            group_id: first.clone(),
            name: "界".repeat(65),
        },
        Action::DissolveGroup {
            group_id: "missing".into(),
        },
        Action::MoveGroup {
            group_id: first.clone(),
            before_id: Some(first.clone()),
        },
        Action::MoveGroup {
            group_id: first.clone(),
            before_id: Some("missing".into()),
        },
        Action::MoveProject {
            project_id: "missing".into(),
            before_id: None,
        },
        Action::MoveProject {
            project_id: "p1".into(),
            before_id: Some("p1".into()),
        },
        Action::MoveThread {
            thread_id: a.id.clone(),
            group_id: Some(first.clone()),
            before_id: Some(b.id.clone()),
        },
        Action::MoveThread {
            thread_id: a.id.clone(),
            group_id: Some(first.clone()),
            before_id: Some(a.id.clone()),
        },
        Action::MoveThread {
            thread_id: a.id.clone(),
            group_id: Some("missing".into()),
            before_id: None,
        },
        Action::MoveThread {
            thread_id: "missing".into(),
            group_id: None,
            before_id: None,
        },
        Action::MoveThread {
            thread_id: a.id.clone(),
            group_id: None,
            before_id: Some(b.id.clone()),
        },
        Action::SetCollapsed {
            target: SidebarCollapseTarget::Project("missing".into()),
            collapsed: true,
        },
        Action::CreateThreadInGroup {
            project_id: "missing".into(),
            group_id: first.clone(),
            model: "mock".into(),
            permission_mode: "confirm".into(),
        },
        Action::CreateThreadInGroup {
            project_id: "p1".into(),
            group_id: first.clone(),
            model: "mock".into(),
            permission_mode: "bad".into(),
        },
    ] {
        rejected_unchanged(&store, action);
    }
    let stale = snapshot(&store).unwrap();
    let committed = change(
        &store,
        Action::RenameGroup {
            group_id: first.clone(),
            name: "Fresh".into(),
        },
    );
    assert!(
        matches!(apply(&store,stale.revision,Action::DissolveGroup{group_id:first}),Err(SidebarOrganizationError::Conflict{expected,actual}) if expected==stale.revision && actual==committed.revision)
    );
    assert_eq!(snapshot(&store).unwrap(), committed);
}
#[test]
fn sidebar_organization_owned_file_storage_failure_rolls_back_new_task_and_metadata() {
    // FAULT-INJECTION: SQLite abort trigger exercises real production transaction rollback.
    let (_dir, store) = open();
    let group = new_group(&store, "Atomic");
    let before = snapshot(&store).unwrap();
    store.conn().execute_batch("CREATE TRIGGER owned_abort BEFORE INSERT ON sidebar_memberships BEGIN SELECT RAISE(ABORT,'owned failure'); END;").unwrap();
    assert!(matches!(
        apply(
            &store,
            before.revision,
            Action::CreateThreadInGroup {
                project_id: "p1".into(),
                group_id: group.clone(),
                model: "mock".into(),
                permission_mode: "confirm".into()
            }
        ),
        Err(SidebarOrganizationError::Store(_))
    ));
    assert_eq!(snapshot(&store).unwrap(), before);
    store
        .conn()
        .execute_batch("DROP TRIGGER owned_abort")
        .unwrap();
    let after = change(
        &store,
        Action::CreateThreadInGroup {
            project_id: "p1".into(),
            group_id: group,
            model: "mock".into(),
            permission_mode: "confirm".into(),
        },
    );
    assert_eq!(after.threads.len(), 1);
    assert_eq!(after.memberships.len(), 1);
}
#[test]
fn sidebar_organization_owned_file_bounds_reject_without_truncation_or_deletion() {
    let (_dir, store) = open();
    store.conn().execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<128) INSERT INTO sidebar_groups SELECT 'g'||x,'Group','\"Gray\"',x FROM n;").unwrap();
    let before = snapshot(&store).unwrap();
    assert_eq!(before.groups.len(), 128);
    assert!(matches!(
        apply(
            &store,
            before.revision,
            Action::CreateGroup {
                name: "Excess".into(),
                color: SidebarGroupColor::Gray
            }
        ),
        Err(SidebarOrganizationError::Limit(_))
    ));
    assert_eq!(snapshot(&store).unwrap(), before);
    store.conn().execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10000) INSERT INTO threads(id,project_id,model,created_at,updated_at) SELECT 't'||x,'p1','mock',0,0 FROM n;").unwrap();
    let at_limit = snapshot(&store).unwrap();
    assert_eq!(at_limit.threads.len(), 10000);
    assert!(matches!(
        apply(
            &store,
            at_limit.revision,
            Action::CreateThreadInGroup {
                project_id: "p1".into(),
                group_id: "g1".into(),
                model: "mock".into(),
                permission_mode: "confirm".into()
            }
        ),
        Err(SidebarOrganizationError::Limit(_))
    ));
    assert_eq!(snapshot(&store).unwrap(), at_limit);
    store.conn().execute("INSERT INTO threads(id,project_id,model,created_at,updated_at) VALUES ('excess','p1','mock',0,0)",[]).unwrap();
    assert!(matches!(
        snapshot(&store),
        Err(SidebarOrganizationError::Limit(_))
    ));
    assert!(matches!(
        apply(
            &store,
            at_limit.revision,
            Action::SetPreferences(SidebarPreferences::default())
        ),
        Err(SidebarOrganizationError::Limit(_))
    ));
    assert_eq!(sql::counts(store.conn()).unwrap(), (10001, 128));
    assert_eq!(
        sql::read(store.conn()).unwrap().revision,
        at_limit.revision as i64
    );
    store.conn().execute_batch("DELETE FROM threads WHERE id='excess'; INSERT INTO sidebar_groups VALUES ('excess-group','Extra','\"Gray\"',129);").unwrap();
    assert!(matches!(
        snapshot(&store),
        Err(SidebarOrganizationError::Limit(_))
    ));
    assert_eq!(sql::counts(store.conn()).unwrap(), (10000, 129));
    assert_eq!(
        sql::read(store.conn()).unwrap().revision,
        at_limit.revision as i64
    );
}

#[test]
fn sidebar_organization_owned_file_returns_exact_created_identity_after_external_creation() {
    let (dir, store) = open();
    let group = new_group(&store, "Exact identity");
    let cached = snapshot(&store).unwrap();
    let external = Store::open(dir.path().join("owned.sqlite")).unwrap();
    let unrelated = threads::create_thread(&external, "p1", "mock", "confirm").unwrap();
    let outcome = apply(
        &store,
        cached.revision,
        Action::CreateThreadInGroup {
            project_id: "p2".into(),
            group_id: group.clone(),
            model: "mock".into(),
            permission_mode: "confirm".into(),
        },
    )
    .unwrap();
    let created = outcome.created_thread.unwrap();
    assert_ne!(created.id, unrelated.id);
    assert_eq!(created.project_id, "p2");
    assert_eq!(outcome.snapshot.threads.len(), 2);
    assert_eq!(
        members(&outcome.snapshot, &group),
        std::slice::from_ref(&created.id)
    );
    assert_eq!(
        outcome.snapshot.threads.iter().find(|t| t.id == created.id),
        Some(&created)
    );
}

#[test]
fn sidebar_organization_owned_file_two_writers_conflict_instead_of_overwriting() {
    let (dir, store) = open();
    let group = new_group(&store, "Original");
    let revision = snapshot(&store).unwrap().revision;
    let path = dir.path().join("owned.sqlite");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: [_; 2] = ["First", "Second"].map(|name| {
        let path = path.clone();
        let barrier = barrier.clone();
        let group = group.clone();
        std::thread::spawn(move || {
            let store = Store::open(path).unwrap();
            barrier.wait();
            apply(
                &store,
                revision,
                Action::RenameGroup {
                    group_id: group,
                    name: name.into(),
                },
            )
        })
    });
    let outcomes = handles.map(|handle| handle.join().unwrap());
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(outcomes.iter().filter(|r|matches!(r,Err(SidebarOrganizationError::Conflict {expected,actual}) if *expected==revision && *actual==revision+1)).count(),1);
    let winner = outcomes.into_iter().find_map(Result::ok).unwrap();
    assert!(winner.created_thread.is_none());
    assert_eq!(snapshot(&store).unwrap(), winner.snapshot);
}

#[test]
fn r14_reveal_is_atomic_revision_checked_and_preserves_other_metadata() {
    let (_dir, store) = open();
    let group = new_group(&store, "keep");
    change(
        &store,
        Action::SetPreferences(SidebarPreferences {
            view: SidebarView::Groups,
            project_view: SidebarProjectView::Timeline,
            sort: SidebarTaskSort::Created,
        }),
    );
    for target in [
        SidebarCollapseTarget::Project("p1".into()),
        SidebarCollapseTarget::Project("p2".into()),
        SidebarCollapseTarget::Group(group),
    ] {
        change(
            &store,
            Action::SetCollapsed {
                target,
                collapsed: true,
            },
        );
    }
    let before = snapshot(&store).unwrap();
    let after = change(
        &store,
        Action::RevealProject {
            project_id: "p1".into(),
        },
    );
    assert_eq!(after.preferences.view, SidebarView::Projects);
    assert_eq!(
        after.preferences.project_view,
        SidebarProjectView::ByProject
    );
    assert_eq!(after.preferences.sort, SidebarTaskSort::Created);
    assert!(
        !after
            .collapsed
            .contains(&SidebarCollapseTarget::Project("p1".into()))
    );
    assert_eq!(after.collapsed.len(), before.collapsed.len() - 1);
    assert_eq!(after.groups, before.groups);
    assert_eq!(after.memberships, before.memberships);
    assert_eq!(after.project_order, before.project_order);
    assert!(matches!(
        apply(
            &store,
            before.revision,
            Action::RevealProject {
                project_id: "p2".into()
            }
        ),
        Err(SidebarOrganizationError::Conflict { .. })
    ));
    assert!(matches!(
        apply(
            &store,
            after.revision,
            Action::RevealProject {
                project_id: "missing".into()
            }
        ),
        Err(SidebarOrganizationError::Invalid(_))
    ));
    assert_eq!(snapshot(&store).unwrap(), after);
}
