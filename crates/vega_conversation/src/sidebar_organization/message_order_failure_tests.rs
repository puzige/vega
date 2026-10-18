use super::*;

#[derive(Clone, Copy, Debug)]
enum Entry {
    Autocommit,
    OuterTransaction,
    PlanReview,
}

const ENTRIES: [Entry; 3] = [
    Entry::Autocommit,
    Entry::OuterTransaction,
    Entry::PlanReview,
];

fn prepare_entry(store: &Store, entry: Entry) {
    if matches!(entry, Entry::PlanReview) {
        prepare_plan(store, "project-c", "owned-plan");
        change(
            store,
            Action::ReorderThread {
                thread_id: "project-c".into(),
                before_id: None,
            },
        );
        seed_current_preferences(store, SidebarTaskSort::Manual);
    }
}

fn write_via(store: &Store, entry: Entry, id: &str) -> messages::MessageRow {
    let row = message(store, id, "project-c", "user", "done", 1);
    match entry {
        Entry::Autocommit => messages::insert(store.conn(), &row).unwrap(),
        Entry::OuterTransaction => {
            let tx = store.immediate_transaction().unwrap();
            messages::insert(&tx, &row).unwrap();
            tx.commit().unwrap();
        }
        Entry::PlanReview => approve_plan(store, "project-c", "owned-plan", id),
    }
    assert!(store.conn().is_autocommit());
    messages::find(store.conn(), id).unwrap().unwrap()
}

#[test]
fn outer_transaction_exposes_message_and_promotion_together_only_after_commit() {
    let (dir, store) = fixture();
    let observer = Store::open(dir.path().join("owned.sqlite")).unwrap();
    let before = snapshot(&observer).unwrap();
    let organization = stored_organization(&observer);
    let row = message(
        &store,
        "transaction-message",
        "project-c",
        "user",
        "done",
        1,
    );
    let tx = store.immediate_transaction().unwrap();
    messages::insert(&tx, &row).unwrap();
    assert_eq!(messages::find(&tx, &row.id).unwrap(), Some(row.clone()));
    assert_eq!(sql::read(&tx).unwrap().revision, before.revision as i64 + 1);
    assert!(messages::find(observer.conn(), &row.id).unwrap().is_none());
    assert_eq!(stored_organization(&observer), organization);
    tx.commit().unwrap();
    assert_eq!(messages::find(observer.conn(), &row.id).unwrap(), Some(row));
    assert_order(
        &observer,
        Some("p1"),
        false,
        &["project-c", "project-a", "project-b"],
    );
    assert_eq!(snapshot(&observer).unwrap().revision, before.revision + 1);
}

#[test]
fn outer_transaction_rollback_discards_both_message_and_promotion() {
    let (dir, store) = fixture();
    let observer = Store::open(dir.path().join("owned.sqlite")).unwrap();
    let before = snapshot(&observer).unwrap();
    let organization = stored_organization(&observer);
    let row = message(
        &store,
        "rolled-back-message",
        "project-c",
        "assistant",
        "streaming",
        1,
    );
    let tx = store.immediate_transaction().unwrap();
    messages::insert(&tx, &row).unwrap();
    assert_eq!(sql::read(&tx).unwrap().revision, before.revision as i64 + 1);
    assert!(messages::find(&tx, &row.id).unwrap().is_some());
    assert!(messages::find(observer.conn(), &row.id).unwrap().is_none());
    assert_eq!(stored_organization(&observer), organization);
    tx.rollback().unwrap();
    assert!(messages::find(store.conn(), &row.id).unwrap().is_none());
    assert_eq!(stored_organization(&store), organization);
    assert_eq!(snapshot(&observer).unwrap(), before);
    assert_eq!(stored_organization(&observer), organization);
    drop(observer);
    drop(store);
    let reopened = Store::open(dir.path().join("owned.sqlite")).unwrap();
    assert!(messages::find(reopened.conn(), &row.id).unwrap().is_none());
    assert_eq!(stored_organization(&reopened), organization);
}

#[test]
fn duplicate_and_invalid_message_inserts_never_change_order_or_revision() {
    for outer in [false, true] {
        let (_dir, store) = fixture();
        let existing = message(&store, "existing-message", "project-c", "user", "done", 1);
        messages::insert(store.conn(), &existing).unwrap();
        change(
            &store,
            Action::ReorderThread {
                thread_id: "project-c".into(),
                before_id: None,
            },
        );
        let before = snapshot(&store).unwrap();
        let organization = stored_organization(&store);
        let duplicate = message(&store, &existing.id, "project-c", "user", "done", 2);
        let mut duplicate_sequence =
            message(&store, "duplicate-sequence", "project-c", "user", "done", 2);
        duplicate_sequence.seq = existing.seq;
        let mut invalid = message(
            &store,
            "invalid-message",
            "project-c",
            "assistant",
            "streaming",
            2,
        );
        invalid.plan_status = Some("pending".into());
        let mut orphan = message(&store, "orphan-message", "project-c", "user", "done", 2);
        orphan.thread_id = "missing-thread".into();
        for row in [duplicate, duplicate_sequence, invalid, orphan] {
            if outer {
                let tx = store.immediate_transaction().unwrap();
                assert!(messages::insert(&tx, &row).is_err());
                tx.commit().unwrap();
            } else {
                assert!(messages::insert(store.conn(), &row).is_err());
            }
            assert!(store.conn().is_autocommit());
            assert_eq!(snapshot(&store).unwrap(), before);
            assert_eq!(stored_organization(&store), organization);
            assert_eq!(
                messages::find(store.conn(), &existing.id).unwrap(),
                Some(existing.clone())
            );
            if row.id != existing.id {
                assert!(messages::find(store.conn(), &row.id).unwrap().is_none());
            }
            assert_eq!(
                messages::recent(store.conn(), "project-c", 20)
                    .unwrap()
                    .as_slice(),
                std::slice::from_ref(&existing)
            );
        }
    }
}

#[test]
fn failed_plan_instruction_and_invalid_review_leave_plan_and_order_unchanged() {
    let (_dir, store) = fixture();
    let occupied = message(&store, "occupied-id", "project-a", "user", "done", 1);
    messages::insert(store.conn(), &occupied).unwrap();
    prepare_entry(&store, Entry::PlanReview);
    let before = snapshot(&store).unwrap();
    let organization = stored_organization(&store);
    let plan = messages::find(store.conn(), "owned-plan").unwrap().unwrap();
    let thread = stored_threads::find(store.conn(), "project-c")
        .unwrap()
        .unwrap();
    assert!(matches!(
        messages::review_plan(
            store.conn(),
            messages::PlanReview {
                thread_id: "project-c",
                plan_id: "owned-plan",
                status: "approved",
                note: None,
                reviewed_at: 40,
                instruction: Some(messages::PlanInstruction {
                    id: "occupied-id",
                    content: "Execute the owned plan",
                    created_at: 1
                }),
            }
        ),
        Err(messages::PlanTransitionError::Store(_))
    ));
    for status in ["approved", "invalid-review"] {
        assert!(matches!(
            messages::review_plan(
                store.conn(),
                messages::PlanReview {
                    thread_id: "project-c",
                    plan_id: "owned-plan",
                    status,
                    note: None,
                    reviewed_at: 40,
                    instruction: None,
                }
            ),
            Err(messages::PlanTransitionError::CorruptState)
        ));
        assert_eq!(snapshot(&store).unwrap(), before);
        assert_eq!(stored_organization(&store), organization);
        assert_eq!(
            messages::find(store.conn(), "owned-plan").unwrap(),
            Some(plan.clone())
        );
        assert_eq!(
            stored_threads::find(store.conn(), "project-c").unwrap(),
            Some(thread.clone())
        );
    }
    assert_eq!(
        messages::find(store.conn(), "occupied-id").unwrap(),
        Some(occupied)
    );
    approve_plan(&store, "project-c", "owned-plan", "valid-instruction");
    assert_order(
        &store,
        Some("p1"),
        false,
        &["project-c", "project-a", "project-b"],
    );
    change(
        &store,
        Action::ReorderThread {
            thread_id: "project-c".into(),
            before_id: None,
        },
    );
    let after_drag = stored_organization(&store);
    assert_eq!(
        messages::review_plan(
            store.conn(),
            messages::PlanReview {
                thread_id: "project-c",
                plan_id: "owned-plan",
                status: "approved",
                note: None,
                reviewed_at: 50,
                instruction: Some(messages::PlanInstruction {
                    id: "replayed-instruction",
                    content: "Do not replay",
                    created_at: 1
                }),
            }
        )
        .unwrap(),
        messages::PlanReviewResult::Stale
    );
    assert!(
        messages::find(store.conn(), "replayed-instruction")
            .unwrap()
            .is_none()
    );
    assert_eq!(stored_organization(&store), after_drag);
}

#[derive(Clone, Copy)]
enum SortingFault {
    SidebarUpdate,
    MalformedPreferences,
    RevisionCap,
}

fn sorting_failure_preserves_message_and_recovers_on_next_message(fault: SortingFault) {
    for entry in ENTRIES {
        let (dir, store) = fixture();
        prepare_entry(&store, entry);
        let healthy = sql::read(store.conn()).unwrap();
        let projects = sql::projects(store.conn()).unwrap();
        let neighbor = stored_threads::find(store.conn(), "project-b").unwrap();
        let mut expected_source = stored_threads::find(store.conn(), "project-c")
            .unwrap()
            .unwrap();
        if matches!(entry, Entry::PlanReview) {
            expected_source.mode = "execute".into();
            expected_source.updated_at = 40;
        }
        let observer = Store::open(dir.path().join("owned.sqlite")).unwrap();
        match fault {
            SortingFault::SidebarUpdate => store.conn().execute_batch(
                "CREATE TRIGGER owned_sidebar_fault BEFORE UPDATE ON sidebar_organization BEGIN SELECT RAISE(ABORT,'owned sidebar update failure'); END;"
            ).unwrap(),
            SortingFault::MalformedPreferences => {
                store.conn().execute("UPDATE sidebar_organization SET preferences='{broken' WHERE singleton=1", []).unwrap();
            }
            SortingFault::RevisionCap => {
                store.conn().execute("UPDATE sidebar_organization SET revision=?1 WHERE singleton=1", [i64::MAX]).unwrap();
            }
        }
        let organization = stored_organization(&store);
        match fault {
            SortingFault::SidebarUpdate => {
                let error = apply(
                    &store,
                    healthy.revision as u64,
                    Action::ReorderThread {
                        thread_id: "project-c".into(),
                        before_id: Some("project-a".into()),
                    },
                )
                .unwrap_err();
                match error {
                    SidebarOrganizationError::Store(message) => {
                        assert_eq!(message, "owned sidebar update failure");
                    }
                    error => panic!("unexpected sidebar update preflight error: {error:?}"),
                }
            }
            SortingFault::MalformedPreferences => match snapshot(&store).unwrap_err() {
                SidebarOrganizationError::Store(message) => {
                    assert_eq!(message, "key must be a string at line 1 column 2");
                }
                error => panic!("unexpected preferences decode preflight error: {error:?}"),
            },
            SortingFault::RevisionCap => {
                assert!(matches!(
                    apply(
                        &store,
                        i64::MAX as u64,
                        Action::ReorderThread {
                            thread_id: "project-c".into(),
                            before_id: Some("project-a".into()),
                        }
                    ),
                    Err(SidebarOrganizationError::Limit(_))
                ));
            }
        }
        assert!(store.conn().is_autocommit());
        assert_eq!(stored_organization(&store), organization);
        assert_eq!(stored_organization(&observer), organization);
        let row = write_via(&store, entry, "retained-on-sort-failure");
        assert_eq!(
            messages::find(observer.conn(), &row.id).unwrap(),
            Some(row.clone())
        );
        assert_eq!(stored_organization(&observer), organization);
        assert_eq!(stored_organization(&store), organization);
        assert_eq!(sql::projects(observer.conn()).unwrap(), projects);
        assert_eq!(
            stored_threads::find(observer.conn(), "project-b").unwrap(),
            neighbor
        );
        assert_eq!(
            stored_threads::find(observer.conn(), "project-c").unwrap(),
            Some(expected_source)
        );
        if !matches!(fault, SortingFault::MalformedPreferences) {
            assert_order(
                &observer,
                Some("p1"),
                false,
                &["project-a", "project-b", "project-c"],
            );
        }
        if matches!(entry, Entry::PlanReview) {
            let plan = messages::find(observer.conn(), "owned-plan")
                .unwrap()
                .unwrap();
            assert_eq!(plan.plan_status.as_deref(), Some("approved"));
            assert_eq!(plan.plan_reviewed_at, Some(40));
            assert_eq!(
                stored_threads::find(observer.conn(), "project-c")
                    .unwrap()
                    .unwrap()
                    .mode,
                "execute"
            );
            assert_eq!(row.role, "user");
            assert_eq!(row.content, "Execute the owned plan");
        }
        match fault {
            SortingFault::SidebarUpdate => store
                .conn()
                .execute_batch("DROP TRIGGER owned_sidebar_fault")
                .unwrap(),
            SortingFault::MalformedPreferences => {
                store
                    .conn()
                    .execute(
                        "UPDATE sidebar_organization SET preferences=?1 WHERE singleton=1",
                        [&healthy.preferences],
                    )
                    .unwrap();
            }
            SortingFault::RevisionCap => {
                store
                    .conn()
                    .execute(
                        "UPDATE sidebar_organization SET revision=?1 WHERE singleton=1",
                        [healthy.revision],
                    )
                    .unwrap();
            }
        }
        let restored = stored_organization(&observer);
        assert_order(
            &observer,
            Some("p1"),
            false,
            &["project-a", "project-b", "project-c"],
        );
        assert_eq!(
            messages::finish(store.conn(), &row.id, &row.content, "done").unwrap(),
            1
        );
        assert_eq!(
            messages::update_streaming_content(store.conn(), &row.id, "late update").unwrap(),
            0
        );
        if matches!(entry, Entry::PlanReview) {
            assert_eq!(
                messages::review_plan(
                    store.conn(),
                    messages::PlanReview {
                        thread_id: "project-c",
                        plan_id: "owned-plan",
                        status: "approved",
                        note: None,
                        reviewed_at: 50,
                        instruction: Some(messages::PlanInstruction {
                            id: "replayed-after-fault",
                            content: "Do not replay",
                            created_at: 1
                        }),
                    }
                )
                .unwrap(),
                messages::PlanReviewResult::Stale
            );
            assert!(
                messages::find(store.conn(), "replayed-after-fault")
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(stored_organization(&observer), restored);
        let metadata = retained_metadata(&observer);
        let next = message(
            &store,
            "recovery-message",
            "project-b",
            "assistant",
            "streaming",
            -1,
        );
        messages::insert(store.conn(), &next).unwrap();
        assert_order(
            &observer,
            Some("p1"),
            false,
            &["project-b", "project-a", "project-c"],
        );
        assert_eq!(
            snapshot(&observer).unwrap().revision,
            healthy.revision as u64 + 1
        );
        assert_eq!(retained_metadata(&observer), metadata);
        assert_eq!(
            messages::find(observer.conn(), &row.id).unwrap(),
            Some(row.clone())
        );
        drop(observer);
        drop(store);
        let reopened = Store::open(dir.path().join("owned.sqlite")).unwrap();
        assert_order(
            &reopened,
            Some("p1"),
            false,
            &["project-b", "project-a", "project-c"],
        );
        assert_eq!(messages::find(reopened.conn(), &row.id).unwrap(), Some(row));
        assert_eq!(
            messages::find(reopened.conn(), &next.id).unwrap(),
            Some(next)
        );
    }
}

#[test]
fn sidebar_update_failure_keeps_message_and_original_order_then_next_message_recovers() {
    sorting_failure_preserves_message_and_recovers_on_next_message(SortingFault::SidebarUpdate);
}

#[test]
fn malformed_preferences_keep_message_and_original_metadata_then_next_message_recovers() {
    sorting_failure_preserves_message_and_recovers_on_next_message(
        SortingFault::MalformedPreferences,
    );
}

#[test]
fn exhausted_revision_keeps_message_and_original_order_then_next_message_recovers() {
    sorting_failure_preserves_message_and_recovers_on_next_message(SortingFault::RevisionCap);
}

#[test]
fn message_commit_failure_is_not_swallowed_as_a_sorting_failure() {
    const SQLITE_CONSTRAINT_FOREIGNKEY: i32 = 787;

    for entry in ENTRIES {
        let (dir, store) = fixture();
        prepare_entry(&store, entry);
        let observer = Store::open(dir.path().join("owned.sqlite")).unwrap();
        let before = snapshot(&observer).unwrap();
        let organization = stored_organization(&observer);
        let plan = messages::find(store.conn(), "owned-plan").unwrap();
        assert_eq!(
            store
                .conn()
                .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        store.conn().execute_batch(
            "CREATE TABLE owned_commit_guard (message_id TEXT NOT NULL, project_id TEXT NOT NULL REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED);
             CREATE TRIGGER owned_message_commit_fault AFTER INSERT ON messages WHEN NEW.id='commit-message' BEGIN INSERT INTO owned_commit_guard VALUES (NEW.id,'missing-project'); END;"
        ).unwrap();
        let tx = store.immediate_transaction().unwrap();
        assert_eq!(
            tx.execute(
                "INSERT INTO owned_commit_guard(message_id,project_id) VALUES ('owned-deferred-preflight','missing-project')",
                []
            )
            .unwrap(),
            1
        );
        assert!(!tx.is_autocommit());
        assert_eq!(
            tx.query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_check('owned_commit_guard')",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        let error = tx.commit().unwrap_err();
        assert_eq!(
            error.sqlite_error().unwrap().extended_code,
            SQLITE_CONSTRAINT_FOREIGNKEY
        );
        assert_eq!(error.to_string(), "FOREIGN KEY constraint failed");
        assert!(store.conn().is_autocommit());
        assert_eq!(
            store
                .conn()
                .query_row("SELECT COUNT(*) FROM owned_commit_guard", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        assert_eq!(stored_organization(&store), organization);
        assert_eq!(snapshot(&observer).unwrap(), before);
        let row = message(&store, "commit-message", "project-c", "user", "done", 1);
        let error = match entry {
            Entry::Autocommit => messages::insert(store.conn(), &row).unwrap_err(),
            Entry::OuterTransaction => {
                let tx = store.immediate_transaction().unwrap();
                messages::insert(&tx, &row).unwrap();
                assert_eq!(
                    tx.query_row("SELECT message_id FROM owned_commit_guard", [], |row| {
                        row.get::<_, String>(0)
                    })
                    .unwrap(),
                    row.id
                );
                tx.commit().unwrap_err()
            }
            Entry::PlanReview => match messages::review_plan(
                store.conn(),
                messages::PlanReview {
                    thread_id: "project-c",
                    plan_id: "owned-plan",
                    status: "approved",
                    note: None,
                    reviewed_at: 40,
                    instruction: Some(messages::PlanInstruction {
                        id: &row.id,
                        content: "Execute the owned plan",
                        created_at: 1,
                    }),
                },
            )
            .unwrap_err()
            {
                messages::PlanTransitionError::Store(error) => error,
                error => panic!("unexpected Plan commit error: {error:?}"),
            },
        };
        assert_eq!(
            error.sqlite_error().unwrap().extended_code,
            SQLITE_CONSTRAINT_FOREIGNKEY
        );
        assert_eq!(error.to_string(), "FOREIGN KEY constraint failed");
        assert!(store.conn().is_autocommit());
        assert_eq!(
            observer
                .conn()
                .query_row("SELECT COUNT(*) FROM owned_commit_guard", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        assert!(messages::find(store.conn(), &row.id).unwrap().is_none());
        assert!(messages::find(observer.conn(), &row.id).unwrap().is_none());
        assert_eq!(stored_organization(&store), organization);
        assert_eq!(snapshot(&observer).unwrap(), before);
        assert_eq!(messages::find(observer.conn(), "owned-plan").unwrap(), plan);
        store
            .conn()
            .execute_batch("DROP TRIGGER owned_message_commit_fault")
            .unwrap();
        write_via(&store, entry, "after-commit-failure");
        assert_order(
            &observer,
            Some("p1"),
            false,
            &["project-c", "project-a", "project-b"],
        );
    }
}

#[test]
fn message_promotion_does_not_rewrite_other_organization_metadata() {
    for entry in ENTRIES {
        let (_dir, store) = fixture();
        prepare_entry(&store, entry);
        let metadata = retained_metadata(&store);
        let before = snapshot(&store).unwrap();
        let projects = sql::projects(store.conn()).unwrap();
        for table in [
            "sidebar_groups",
            "sidebar_memberships",
            "sidebar_project_order",
        ] {
            for operation in ["INSERT", "UPDATE", "DELETE"] {
                store.conn().execute_batch(&format!(
                    "CREATE TRIGGER owned_guard_{table}_{operation} BEFORE {operation} ON {table} BEGIN SELECT RAISE(ABORT,'unrelated metadata must not be rewritten'); END;"
                )).unwrap();
            }
        }
        store.conn().execute_batch(
            "CREATE TRIGGER owned_collapsed_guard BEFORE UPDATE OF collapsed ON sidebar_organization BEGIN SELECT RAISE(ABORT,'collapsed state must not be rewritten'); END;"
        ).unwrap();
        let row = write_via(&store, entry, "metadata-safe-message");
        assert_order(
            &store,
            Some("p1"),
            false,
            &["project-c", "project-a", "project-b"],
        );
        assert_eq!(snapshot(&store).unwrap().revision, before.revision + 1);
        assert_eq!(retained_metadata(&store), metadata);
        assert_eq!(sql::projects(store.conn()).unwrap(), projects);
        assert_eq!(messages::find(store.conn(), &row.id).unwrap(), Some(row));
    }
}

#[test]
fn message_commit_invalidates_stale_drag_and_refresh_allows_retry() {
    for entry in ENTRIES {
        let (dir, store) = fixture();
        prepare_entry(&store, entry);
        let writer = Store::open(dir.path().join("owned.sqlite")).unwrap();
        let stale = snapshot(&store).unwrap();
        let row = write_via(&writer, entry, "concurrent-message");
        assert_order(
            &store,
            Some("p1"),
            false,
            &["project-c", "project-a", "project-b"],
        );
        let committed = snapshot(&store).unwrap();
        let organization = stored_organization(&store);
        assert_eq!(committed.revision, stale.revision + 1);
        let action = Action::ReorderThread {
            thread_id: "project-c".into(),
            before_id: None,
        };
        assert!(
            matches!(apply(&store, stale.revision, action.clone()), Err(SidebarOrganizationError::Conflict { expected, actual }) if expected == stale.revision && actual == committed.revision)
        );
        assert_eq!(stored_organization(&writer), organization);
        assert_eq!(messages::find(store.conn(), &row.id).unwrap(), Some(row));
        let retried = apply(&store, snapshot(&store).unwrap().revision, action)
            .unwrap()
            .snapshot;
        assert_order(
            &writer,
            Some("p1"),
            false,
            &["project-a", "project-b", "project-c"],
        );
        assert_eq!(retried.revision, committed.revision + 1);
        assert_eq!(snapshot(&writer).unwrap(), retried);
    }
}

#[test]
fn drag_commit_precedes_later_message_promotion_on_another_connection() {
    for entry in ENTRIES {
        let (dir, store) = fixture();
        prepare_entry(&store, entry);
        let writer = Store::open(dir.path().join("owned.sqlite")).unwrap();
        let stale_writer_snapshot = snapshot(&writer).unwrap();
        let dragged = apply(
            &store,
            stale_writer_snapshot.revision,
            Action::ReorderThread {
                thread_id: "project-b".into(),
                before_id: Some("project-a".into()),
            },
        )
        .unwrap()
        .snapshot;
        assert_order(
            &writer,
            Some("p1"),
            false,
            &["project-b", "project-a", "project-c"],
        );
        let metadata = retained_metadata(&store);
        let row = write_via(&writer, entry, "after-drag-message");
        assert_order(
            &store,
            Some("p1"),
            false,
            &["project-c", "project-b", "project-a"],
        );
        assert_eq!(snapshot(&store).unwrap().revision, dragged.revision + 1);
        assert_eq!(retained_metadata(&store), metadata);
        assert_eq!(messages::find(store.conn(), &row.id).unwrap(), Some(row));
    }
}
