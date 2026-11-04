use super::*;
use std::sync::{Arc, Barrier};
use tokio_util::sync::CancellationToken;
use vega_store::write_coordination::{WriteCategory, acquire};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn memory_store() -> TestResult<Store> {
    let store = Store::open(":memory:")?;
    store.migrate()?;
    Ok(store)
}

fn seed_organization(store: &Store) -> TestResult {
    let tx = store.immediate_transaction()?;
    tx.execute_batch(
        r#"
        INSERT INTO projects(id,path,name,created_at,last_opened_at)
        VALUES ('project-a','/isolated/a','A',1,2),('project-b','/isolated/b','B',3,4);
        UPDATE sidebar_project_order SET position=17 WHERE project_id='project-a';
        UPDATE sidebar_project_order SET position=3 WHERE project_id='project-b';
        INSERT INTO threads(id,project_id,model,created_at,updated_at)
        VALUES ('thread-a','project-a','test-model',1,2),('thread-b',NULL,'test-model',3,4);
        INSERT INTO sidebar_groups(id,name,color,position)
        VALUES ('group-a','A','"Blue"',8),('group-b','B','"Red"',2);
        INSERT INTO sidebar_memberships(thread_id,group_id,position)
        VALUES ('thread-a','group-a',9),('thread-b','group-a',4);
        UPDATE sidebar_organization
        SET revision=7,preferences=' {"view":"Groups","project_view":"Timeline","sort":"Created","manual_thread_order":["thread-b","thread-a"],"hide_project_threads_from_recents":true,"future_preference":{"keep":true}} ';
        "#,
    )?;
    tx.commit()?;
    Ok(())
}

fn collapse_state(store: &Store) -> TestResult<SidebarCollapseUpdate> {
    let (revision, collapsed) = sql::read_collapse(store.conn())?;
    Ok(SidebarCollapseUpdate {
        revision: u64::try_from(revision)?,
        collapsed: serde_json::from_str(&collapsed)?,
    })
}

fn noncollapse_state(store: &Store) -> TestResult<Vec<String>> {
    let mut statement = store.conn().prepare(
        "SELECT 'preferences:' || quote(preferences) FROM sidebar_organization
         UNION ALL SELECT 'group:' || quote(id) || quote(name) || quote(color) || quote(position) FROM sidebar_groups
         UNION ALL SELECT 'membership:' || quote(thread_id) || quote(group_id) || quote(position) FROM sidebar_memberships
         UNION ALL SELECT 'project:' || quote(project_id) || quote(position) FROM sidebar_project_order
         UNION ALL SELECT 'thread:' || quote(id) || quote(project_id) || quote(status) || quote(updated_at) FROM threads
         ORDER BY 1",
    )?;
    Ok(statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

#[test]
fn uncancelled_save_merges_latest_state_and_preserves_other_metadata() -> TestResult {
    let store = memory_store()?;
    seed_organization(&store)?;
    let before = noncollapse_state(&store)?;
    save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, true)])?;
    let cancellation = CancellationToken::new();
    let update = save_collapsed_with_cancel(
        &store,
        &[(SidebarCollapseTarget::Ungrouped, true)],
        &cancellation,
    )?;
    assert_eq!(update.revision, 9);
    assert_eq!(
        update.collapsed,
        vec![
            SidebarCollapseTarget::Pinned,
            SidebarCollapseTarget::Ungrouped
        ]
    );
    assert_eq!(collapse_state(&store)?, update);
    assert_eq!(noncollapse_state(&store)?, before);
    assert_eq!(
        save_collapsed_with_cancel(&store, &[], &cancellation)?,
        update
    );
    assert!(store.conn().is_autocommit());
    Ok(())
}

#[test]
fn cancelled_saves_reject_changes_and_noops_without_mutating_state() -> TestResult {
    let store = memory_store()?;
    seed_organization(&store)?;
    let before = collapse_state(&store)?;
    let other_metadata = noncollapse_state(&store)?;
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    for changes in [vec![], vec![(SidebarCollapseTarget::Pinned, true)]] {
        assert!(matches!(
            save_collapsed_with_cancel(&store, &changes, &cancellation),
            Err(SidebarOrganizationError::Store(_))
        ));
        assert_eq!(collapse_state(&store)?, before);
        assert_eq!(noncollapse_state(&store)?, other_metadata);
        assert!(store.conn().is_autocommit());
    }
    let recovered = save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, true)])?;
    assert_eq!(recovered.revision, before.revision + 1);
    assert_eq!(recovered.collapsed, vec![SidebarCollapseTarget::Pinned]);
    Ok(())
}

#[test]
fn cancelled_save_rejects_reentrant_writer_permit_and_releases_transaction() -> TestResult {
    let store = memory_store()?;
    let before = collapse_state(&store)?;
    let permit = acquire(store.conn(), WriteCategory::Sidebar)?;
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let result = save_collapsed_with_cancel(
        &store,
        &[(SidebarCollapseTarget::Pinned, true)],
        &cancellation,
    );
    assert!(matches!(result, Err(SidebarOrganizationError::Store(_))));
    assert!(store.conn().is_autocommit());
    assert_eq!(collapse_state(&store)?, before);
    drop(permit);
    let recovered = save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, true)])?;
    assert_eq!(recovered.revision, 1);
    assert_eq!(recovered.collapsed, vec![SidebarCollapseTarget::Pinned]);
    Ok(())
}

#[test]
fn collapse_preserves_preferences_sort_order_groups_and_memberships() -> TestResult {
    let store = memory_store()?;
    seed_organization(&store)?;
    let before = noncollapse_state(&store)?;
    let targets = vec![
        SidebarCollapseTarget::Project("project-a".into()),
        SidebarCollapseTarget::Group("group-b".into()),
        SidebarCollapseTarget::Timeline(SidebarTimelineBucket::Today),
        SidebarCollapseTarget::Timeline(SidebarTimelineBucket::Yesterday),
        SidebarCollapseTarget::Timeline(SidebarTimelineBucket::Last7Days),
        SidebarCollapseTarget::Timeline(SidebarTimelineBucket::Last30Days),
        SidebarCollapseTarget::Timeline(SidebarTimelineBucket::Earlier),
        SidebarCollapseTarget::Pinned,
        SidebarCollapseTarget::Ungrouped,
    ];
    store.conn().execute_batch(
        "CREATE TRIGGER reject_preferences_write BEFORE UPDATE OF preferences ON sidebar_organization
         BEGIN SELECT RAISE(ABORT,'preferences must not be written'); END;",
    )?;
    let changes: Vec<_> = targets
        .iter()
        .cloned()
        .map(|target| (target, true))
        .collect();
    let result = save_collapsed(&store, &changes)?;
    assert_eq!(result.revision, 8);
    assert_eq!(result.collapsed, targets);
    assert_eq!(collapse_state(&store)?, result);
    assert_eq!(noncollapse_state(&store)?, before);
    let expanded = save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, false)])?;
    assert_eq!(expanded.revision, 9);
    assert!(!expanded.collapsed.contains(&SidebarCollapseTarget::Pinned));
    assert_eq!(expanded.collapsed.len(), targets.len() - 1);
    assert_eq!(noncollapse_state(&store)?, before);
    Ok(())
}

#[test]
fn unchanged_and_cancelled_intents_do_not_write_or_increment_revision() -> TestResult {
    let store = memory_store()?;
    let initial = save_collapsed(
        &store,
        &[
            (SidebarCollapseTarget::Pinned, true),
            (SidebarCollapseTarget::Ungrouped, true),
        ],
    )?;
    store.conn().execute_batch(
        "CREATE TRIGGER reject_collapse_write BEFORE UPDATE ON sidebar_organization
         BEGIN SELECT RAISE(ABORT,'no write expected'); END;",
    )?;
    assert_eq!(save_collapsed(&store, &[])?, initial);
    assert_eq!(
        save_collapsed(
            &store,
            &[
                (SidebarCollapseTarget::Pinned, false),
                (SidebarCollapseTarget::Pinned, true),
                (SidebarCollapseTarget::Ungrouped, true),
                (
                    SidebarCollapseTarget::Timeline(SidebarTimelineBucket::Today),
                    false
                ),
            ],
        )?,
        initial
    );
    assert_eq!(collapse_state(&store)?, initial);
    Ok(())
}

#[test]
fn final_intent_wins_and_a_changed_batch_increments_revision_once() -> TestResult {
    let store = memory_store()?;
    let result = save_collapsed(
        &store,
        &[
            (SidebarCollapseTarget::Pinned, true),
            (SidebarCollapseTarget::Ungrouped, false),
            (SidebarCollapseTarget::Pinned, false),
            (SidebarCollapseTarget::Ungrouped, true),
        ],
    )?;
    assert_eq!(result.revision, 1);
    assert_eq!(result.collapsed, vec![SidebarCollapseTarget::Ungrouped]);
    assert_eq!(collapse_state(&store)?, result);
    Ok(())
}

#[test]
fn missing_and_deleted_named_targets_reject_the_entire_batch() -> TestResult {
    for deleted in [false, true] {
        for project in [false, true] {
            for should_collapse in [false, true] {
                let store = memory_store()?;
                seed_organization(&store)?;
                let target = if project {
                    SidebarCollapseTarget::Project(
                        if deleted { "project-a" } else { "missing" }.into(),
                    )
                } else {
                    SidebarCollapseTarget::Group(if deleted { "group-a" } else { "missing" }.into())
                };
                if deleted {
                    if project {
                        assert!(vega_store::projects::remove(store.conn(), "project-a")?);
                    } else {
                        let tx = store.immediate_transaction()?;
                        tx.execute("DELETE FROM sidebar_groups WHERE id='group-a'", [])?;
                        tx.commit()?;
                    }
                }
                let before = collapse_state(&store)?;
                let metadata = noncollapse_state(&store)?;
                let error = save_collapsed(
                    &store,
                    &[
                        (SidebarCollapseTarget::Pinned, true),
                        (target, should_collapse),
                    ],
                )
                .expect_err("a missing named target must reject the whole batch");
                let expected = if project {
                    "domain.organization.validation.project"
                } else {
                    "domain.organization.validation.group"
                };
                assert!(
                    matches!(error, SidebarOrganizationError::InvalidKey(key) if key == expected)
                );
                assert_eq!(collapse_state(&store)?, before);
                assert_eq!(noncollapse_state(&store)?, metadata);
                assert!(store.conn().is_autocommit());
            }
        }
    }
    Ok(())
}

#[test]
fn failed_write_rolls_back_collapse_revision_and_trigger_side_effects() -> TestResult {
    let store = memory_store()?;
    seed_organization(&store)?;
    let before = collapse_state(&store)?;
    let metadata = noncollapse_state(&store)?;
    store.conn().execute_batch(
        r#"CREATE TRIGGER fail_collapse AFTER UPDATE OF collapsed ON sidebar_organization
        BEGIN
            INSERT INTO sidebar_groups(id,name,color,position) VALUES ('partial','Partial','"Gray"',99);
            SELECT RAISE(FAIL,'injected collapse write failure');
        END;"#,
    )?;
    assert!(matches!(
        save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, true)]),
        Err(SidebarOrganizationError::Store(_))
    ));
    assert!(store.conn().is_autocommit());
    assert_eq!(collapse_state(&store)?, before);
    assert_eq!(noncollapse_state(&store)?, metadata);
    store.conn().execute_batch("DROP TRIGGER fail_collapse")?;
    let retry = save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, true)])?;
    assert_eq!(retry.revision, before.revision + 1);
    assert_eq!(retry.collapsed, vec![SidebarCollapseTarget::Pinned]);
    Ok(())
}

#[test]
fn failed_commit_returns_error_and_rolls_back_the_update() -> TestResult {
    let store = memory_store()?;
    let before = collapse_state(&store)?;
    store.conn().execute_batch(
        "CREATE TABLE deferred_collapse_failure (
            project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED
         );
         CREATE TRIGGER fail_collapse_commit AFTER UPDATE OF collapsed ON sidebar_organization
         BEGIN INSERT INTO deferred_collapse_failure VALUES ('missing'); END;",
    )?;
    assert!(matches!(
        save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, true)]),
        Err(SidebarOrganizationError::Store(_))
    ));
    assert!(store.conn().is_autocommit());
    assert_eq!(collapse_state(&store)?, before);
    let rows: i64 = store.conn().query_row(
        "SELECT COUNT(*) FROM deferred_collapse_failure",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(rows, 0);
    Ok(())
}

#[test]
fn collapse_does_not_decode_unrelated_threads_preferences_or_groups() -> TestResult {
    for corruption in [
        "UPDATE threads SET mode='malformed-thread-mode'",
        "UPDATE sidebar_organization SET preferences='malformed-preferences'",
        "UPDATE sidebar_groups SET color='malformed-color'",
    ] {
        let store = memory_store()?;
        seed_organization(&store)?;
        assert!(snapshot(&store).is_ok());
        let tx = store.immediate_transaction()?;
        tx.execute_batch(corruption)?;
        tx.commit()?;
        assert!(snapshot(&store).is_err());
        let before = noncollapse_state(&store)?;
        let result = save_collapsed(
            &store,
            &[
                (SidebarCollapseTarget::Project("project-a".into()), true),
                (SidebarCollapseTarget::Group("group-a".into()), true),
            ],
        )?;
        assert_eq!(result.revision, 8);
        assert_eq!(result.collapsed.len(), 2);
        assert_eq!(collapse_state(&store)?, result);
        assert_eq!(noncollapse_state(&store)?, before);
    }
    Ok(())
}

#[test]
fn malformed_collapse_state_is_rejected_without_overwriting_it() -> TestResult {
    let store = memory_store()?;
    store.conn().execute(
        "UPDATE sidebar_organization SET collapsed='malformed-collapse'",
        [],
    )?;
    assert!(matches!(
        save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, true)]),
        Err(SidebarOrganizationError::Store(_))
    ));
    assert_eq!(
        sql::read_collapse(store.conn())?,
        (0, "malformed-collapse".into())
    );
    assert!(store.conn().is_autocommit());
    Ok(())
}

#[test]
fn revision_exhaustion_rejects_changes_but_allows_noops() -> TestResult {
    let store = memory_store()?;
    store
        .conn()
        .execute("UPDATE sidebar_organization SET revision=?1", [i64::MAX])?;
    let before = collapse_state(&store)?;
    assert_eq!(save_collapsed(&store, &[])?, before);
    assert!(matches!(
        save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, true)]),
        Err(SidebarOrganizationError::LimitKey(
            "domain.organization.validation.revision"
        ))
    ));
    assert_eq!(collapse_state(&store)?, before);
    Ok(())
}

struct TemporaryDatabase(std::path::PathBuf);

impl TemporaryDatabase {
    fn new() -> TestResult<Self> {
        let directory =
            std::env::temp_dir().join(format!("vega-collapse-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&directory)?;
        Ok(Self(directory))
    }

    fn path(&self) -> std::path::PathBuf {
        self.0.join("store.sqlite")
    }
}

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cancelled_old_worker_cannot_overwrite_newer_commit_while_writer_is_held() -> TestResult {
    let database = TemporaryDatabase::new()?;
    let store = Store::open(database.path())?;
    store.migrate()?;
    save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, true)])?;
    let cancellation = CancellationToken::new();
    let old_cancellation = cancellation.clone();
    let path = database.path();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (start_tx, start_rx) = std::sync::mpsc::sync_channel(1);
    let (finished_tx, finished_rx) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let opened = Store::open_existing(path);
        ready_tx
            .send(opened.is_ok())
            .expect("worker readiness must be received");
        start_rx.recv().expect("worker must be released");
        let result = opened.map_err(storage).and_then(|store| {
            save_collapsed_with_cancel(
                &store,
                &[(SidebarCollapseTarget::Pinned, true)],
                &old_cancellation,
            )
        });
        finished_tx
            .send(result)
            .expect("worker result must be received");
    });
    let timeout = std::time::Duration::from_secs(10);
    assert!(ready_rx.recv_timeout(timeout)?, "old database must open");
    let permit = acquire(store.conn(), WriteCategory::Sidebar)?;
    cancellation.cancel();
    let newer = save_collapsed(&store, &[(SidebarCollapseTarget::Pinned, false)])?;
    assert_eq!(newer.revision, 2);
    assert!(newer.collapsed.is_empty());
    start_tx.send(())?;
    let result_while_held = finished_rx.recv_timeout(timeout);
    drop(permit);
    worker.join().expect("cancelled worker must not panic");
    assert!(matches!(
        result_while_held?,
        Err(SidebarOrganizationError::Store(_))
    ));
    assert_eq!(collapse_state(&store)?, newer);
    assert!(store.conn().is_autocommit());
    let reopened = Store::open_existing(database.path())?;
    assert_eq!(collapse_state(&reopened)?, newer);
    Ok(())
}

#[test]
fn concurrent_connections_merge_targets_and_read_latest_persisted_values() -> TestResult {
    let database = TemporaryDatabase::new()?;
    let store = Store::open(database.path())?;
    store.migrate()?;
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [
        SidebarCollapseTarget::Pinned,
        SidebarCollapseTarget::Ungrouped,
    ]
    .into_iter()
    .map(|target| {
        let path = database.path();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let opened = Store::open_existing(path);
            barrier.wait();
            let store = opened.map_err(storage)?;
            save_collapsed(&store, &[(target, true)])
        })
    })
    .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|handle| handle.join()).collect();
    let mut revisions = Vec::new();
    for result in results {
        revisions.push(result.expect("collapse worker must not panic")?.revision);
    }
    revisions.sort_unstable();
    assert_eq!(revisions, vec![1, 2]);
    let merged = collapse_state(&store)?;
    assert_eq!(merged.revision, 2);
    assert_eq!(merged.collapsed.len(), 2);
    assert!(merged.collapsed.contains(&SidebarCollapseTarget::Pinned));
    assert!(merged.collapsed.contains(&SidebarCollapseTarget::Ungrouped));
    let other = Store::open_existing(database.path())?;
    let newer = save_collapsed(
        &other,
        &[
            (SidebarCollapseTarget::Pinned, false),
            (
                SidebarCollapseTarget::Timeline(SidebarTimelineBucket::Today),
                true,
            ),
        ],
    )?;
    assert_eq!(newer.revision, 3);
    let latest = save_collapsed(&store, &[(SidebarCollapseTarget::Ungrouped, false)])?;
    assert_eq!(latest.revision, 4);
    assert_eq!(
        latest.collapsed,
        vec![SidebarCollapseTarget::Timeline(
            SidebarTimelineBucket::Today
        )]
    );
    assert_eq!(collapse_state(&other)?, latest);
    drop(other);
    drop(store);
    let reopened = Store::open_existing(database.path())?;
    assert_eq!(collapse_state(&reopened)?, latest);
    Ok(())
}
