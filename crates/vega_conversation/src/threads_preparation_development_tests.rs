use super::*;
use std::path::PathBuf;

struct Fixture {
    store: Store,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-preparation-{}", new_thread_id()));
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("test.db")).unwrap();
        store.migrate().unwrap();
        Self { store, root }
    }

    fn standalone(&self) -> Thread {
        create_standalone_thread(&self.store, "synthetic", "confirm").unwrap()
    }

    fn fork_source(&self, bytes: &[u8]) -> Thread {
        let project = store_projects::create(
            self.store.conn(),
            self.root.to_str().unwrap(),
            "synthetic",
            None,
        )
        .unwrap();
        let thread = create_thread(&self.store, &project.id, "synthetic", "confirm").unwrap();
        self.store.conn().execute(
            "INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('user',?1,1,'user','question','done',1),('final',?1,2,'assistant','answer','done',2)",
            [&thread.id],
        ).unwrap();
        self.store.conn().execute(
            "INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('final','response',0,6,'final')",
            [],
        ).unwrap();
        vega_store::image_attachments::insert(self.store.conn(), "user", 0, bytes).unwrap();
        thread
    }

    fn thread_count(&self) -> i64 {
        self.store
            .conn()
            .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn png() -> Vec<u8> {
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([1, 2, 3, 255]),
    ));
    let mut output = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

#[test]
fn fence_reads_under_another_writer_without_recreating_scratch() {
    let fixture = Fixture::new();
    let thread = fixture.standalone();
    let root = task_workspace_root(&fixture.store, &thread.id).unwrap();
    let other = Store::open_existing(fixture.root.join("test.db")).unwrap();
    let writer = other.immediate_transaction().unwrap();
    let reader = fixture.store.conn().unchecked_transaction().unwrap();
    let row = store::find(&reader, &thread.id).unwrap().unwrap();
    let fence = crate::permission_contracts::capture_fence(&fixture.store, &row).unwrap();
    assert_eq!(fence.workspace, root.to_str().unwrap());
    reader.commit().unwrap();
    std::fs::remove_dir(&root).unwrap();
    assert!(crate::permission_contracts::capture_fence(&fixture.store, &row).is_err());
    assert!(!root.exists());
    writer.rollback().unwrap();
}

#[test]
fn pending_authority_prepares_missing_legacy_workspace_before_capture() {
    let fixture = Fixture::new();
    let thread = fixture.standalone();
    let root = task_workspace_root(&fixture.store, &thread.id).unwrap();
    std::fs::remove_dir(&root).unwrap();
    let fence = crate::pending_messages::capture_authority(&fixture.store, &thread).unwrap();
    assert!(root.is_dir());
    assert_eq!(fence.workspace, root.to_str().unwrap());
    assert!(fixture.store.conn().is_autocommit());
}

#[test]
fn automation_claim_prepares_missing_legacy_workspace_before_writer() {
    let fixture = Fixture::new();
    let thread = fixture.standalone();
    crate::automation::command(
        &fixture.store,
        &thread.id,
        &crate::types::AutomationCommand::Create {
            kind: crate::types::AutomationKind::Loop,
            instruction: "Inspect synthetic state".into(),
            token_budget: None,
            interval_seconds: Some(60),
        },
        100,
    )
    .unwrap();
    let root = task_workspace_root(&fixture.store, &thread.id).unwrap();
    std::fs::remove_dir(&root).unwrap();
    let claim = crate::automation::claim(&fixture.store, &thread.id, None, 100)
        .unwrap()
        .unwrap();
    assert!(root.is_dir());
    assert_eq!(
        claim.permission_fence.unwrap().workspace,
        root.to_str().unwrap()
    );
    assert!(fixture.store.conn().is_autocommit());
}

#[test]
fn plan_approval_prepares_missing_legacy_workspace() {
    let fixture = Fixture::new();
    let thread = fixture.standalone();
    fixture
        .store
        .conn()
        .execute("UPDATE threads SET mode='plan' WHERE id=?1", [&thread.id])
        .unwrap();
    fixture.store.conn().execute(
        "INSERT INTO messages(id,thread_id,seq,role,content,status,kind,plan_status,created_at) VALUES('plan',?1,1,'assistant','synthetic plan','done','plan','pending',1)",
        [&thread.id],
    ).unwrap();
    let root = task_workspace_root(&fixture.store, &thread.id).unwrap();
    std::fs::remove_dir(&root).unwrap();
    let result = crate::plans::review_plan(
        &fixture.store,
        &thread.id,
        "plan",
        crate::types::PlanReviewAction::Approve,
    )
    .unwrap();
    assert!(matches!(
        result,
        crate::types::PlanReviewOutcome::Applied {
            instruction_message_id: Some(_)
        }
    ));
    assert!(root.is_dir());
    assert!(fixture.store.conn().is_autocommit());
}

#[test]
fn automation_permission_prepares_missing_legacy_workspace() {
    let fixture = Fixture::new();
    let thread = create_standalone_thread(&fixture.store, "synthetic", "full_access").unwrap();
    let task = crate::automation::command(
        &fixture.store,
        &thread.id,
        &crate::types::AutomationCommand::Create {
            kind: crate::types::AutomationKind::Loop,
            instruction: "Inspect synthetic state".into(),
            token_budget: None,
            interval_seconds: Some(60),
        },
        100,
    )
    .unwrap()
    .remove(0);
    let root = task_workspace_root(&fixture.store, &thread.id).unwrap();
    std::fs::remove_dir(&root).unwrap();
    let service = crate::automation::AutomationPermissionService::new(fixture.root.join("test.db"));
    assert!(service.prepare(&thread, &task, false).is_ok());
    assert!(root.is_dir());
    assert!(fixture.store.conn().is_autocommit());
}

#[test]
fn workspace_creation_is_rejected_inside_writer() {
    let fixture = Fixture::new();
    let thread = fixture.standalone();
    let root = task_workspace_root(&fixture.store, &thread.id).unwrap();
    std::fs::remove_dir(&root).unwrap();
    let writer = fixture.store.immediate_transaction().unwrap();
    assert!(task_workspace_root(&fixture.store, &thread.id).is_err());
    assert!(!root.exists());
    writer.rollback().unwrap();
}

#[test]
fn fence_rechecks_workspace_owner_and_permission_revision() {
    let fixture = Fixture::new();
    let thread = fixture.standalone();
    let row = store::find(fixture.store.conn(), &thread.id)
        .unwrap()
        .unwrap();
    let accepted = crate::permission_contracts::capture_fence(&fixture.store, &row).unwrap();
    let owner = fixture.standalone();
    let root = task_workspace_root(&fixture.store, &owner.id).unwrap();
    let writer = fixture.store.immediate_transaction().unwrap();
    writer.execute(
        "UPDATE threads SET workspace_owner_thread_id=?2,permission_revision=permission_revision+1 WHERE id=?1",
        [&thread.id, &owner.id],
    ).unwrap();
    let current = store::find(&writer, &thread.id).unwrap().unwrap();
    let fence = crate::permission_contracts::capture_fence(&fixture.store, &current).unwrap();
    assert_ne!(fence, accepted);
    assert_eq!(fence.workspace, root.to_str().unwrap());
    assert_eq!(fence.permission_revision, accepted.permission_revision + 1);
    writer.rollback().unwrap();
}

#[test]
fn fork_preparation_reads_under_writer_and_clones_exact_validated_bytes() {
    let fixture = Fixture::new();
    let bytes = png();
    let thread = fixture.fork_source(&bytes);
    let other = Store::open_existing(fixture.root.join("test.db")).unwrap();
    let writer = other.immediate_transaction().unwrap();
    let prepared = prepare_fork(&fixture.store, &thread.id, Some("final")).unwrap();
    assert_eq!(prepared.images[0].encoded, bytes);
    writer.rollback().unwrap();
    let fork = commit_fork(&fixture.store, &thread.id, false, prepared).unwrap();
    let source = context_compaction::load_source(fixture.store.conn(), &fork.id).unwrap();
    assert_eq!(source.messages.len(), 2);
    assert_eq!(source.images.len(), 1);
    assert_eq!(source.images[0].encoded, bytes);
    assert_eq!(
        fork.forked_from_thread_id.as_deref(),
        Some(thread.id.as_str())
    );
    assert_eq!(fixture.thread_count(), 2);
}

#[test]
fn stale_fork_image_bytes_and_same_sequence_content_never_commit() {
    for image_change in [true, false] {
        let fixture = Fixture::new();
        let thread = fixture.fork_source(&png());
        let prepared = prepare_fork(&fixture.store, &thread.id, Some("final")).unwrap();
        if image_change {
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE image_attachments SET encoded=X'00' WHERE message_id='user'",
                    [],
                )
                .unwrap();
        } else {
            fixture
                .store
                .conn()
                .execute("UPDATE messages SET content='changed' WHERE id='user'", [])
                .unwrap();
        }
        assert!(commit_fork(&fixture.store, &thread.id, false, prepared).is_err());
        assert_eq!(fixture.thread_count(), 1);
    }
}

#[test]
fn corrupt_decode_and_per_message_image_budget_leave_no_fork() {
    for invalid_decode in [true, false] {
        let fixture = Fixture::new();
        let mut bytes = png();
        if invalid_decode {
            bytes.truncate(33);
        }
        let thread = fixture.fork_source(&bytes);
        if invalid_decode {
            assert!(fork_thread(&fixture.store, &thread.id, "final").is_err());
        } else {
            let images = (0..5)
                .map(|ordinal| vega_store::image_attachments::ImageRow {
                    message_id: "user".into(),
                    ordinal,
                    artifact_call_id: None,
                    encoded: bytes.clone(),
                })
                .collect::<Vec<_>>();
            assert!(validate_fork_images(&images).is_err());
        }
        assert_eq!(fixture.thread_count(), 1);
    }
}

#[test]
fn latest_fork_rejects_a_new_completed_tail_without_partial_clone() {
    let fixture = Fixture::new();
    let thread = fixture.fork_source(&png());
    let prepared = prepare_fork(&fixture.store, &thread.id, None).unwrap();
    fixture.store.conn().execute(
        "INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('next',?1,3,'assistant','new','done',3)",
        [&thread.id],
    ).unwrap();
    fixture.store.conn().execute(
        "INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('next','next-response',0,3,'final')",
        [],
    ).unwrap();
    assert!(commit_fork(&fixture.store, &thread.id, true, prepared).is_err());
    assert_eq!(fixture.thread_count(), 1);
}

#[test]
fn final_fork_validation_rolls_back_after_clone_on_invalid_history() {
    let fixture = Fixture::new();
    let thread = fixture.fork_source(&png());
    fixture
        .store
        .conn()
        .execute("UPDATE messages SET role='system' WHERE id='user'", [])
        .unwrap();
    assert!(fork_thread(&fixture.store, &thread.id, "final").is_err());
    assert_eq!(fixture.thread_count(), 1);
}

#[test]
fn recovery_rejects_busy_owner_before_writer_and_revalidates_replacement() {
    let fixture = Fixture::new();
    let parent = fixture.standalone();
    let child = fixture.standalone();
    fixture
        .store
        .conn()
        .execute(
            "UPDATE threads SET parent_thread_id=?2 WHERE id=?1",
            [&child.id, &parent.id],
        )
        .unwrap();
    fixture.store.conn().execute(
        "INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('stream',?1,1,'assistant','','streaming',1)",
        [&child.id],
    ).unwrap();
    let database = fixture.root.join("test.db");
    let owner =
        vega_store::delegated_ownership::DelegatedOwnership::acquire(&database, &child.id).unwrap();
    let other = Store::open_existing(&database).unwrap();
    let writer = other.immediate_transaction().unwrap();
    let error =
        vega_store::recovery::recover_thread(fixture.store.conn(), &child.id, 3).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("delegated ownership unavailable")
    );
    writer.rollback().unwrap();
    let path = fixture
        .root
        .join("delegated-owner-locks")
        .join(format!("{}.lock", child.id));
    std::fs::rename(&path, path.with_extension("old")).unwrap();
    let replacement =
        vega_store::delegated_ownership::DelegatedOwnership::acquire(&database, &child.id).unwrap();
    let writer = fixture.store.immediate_transaction().unwrap();
    assert!(
        vega_store::recovery::recover_thread_with_owner(&writer, &child.id, 3, Some(&owner),)
            .is_err()
    );
    writer.rollback().unwrap();
    assert_eq!(
        store_messages::find(fixture.store.conn(), "stream")
            .unwrap()
            .unwrap()
            .status,
        "streaming"
    );
    drop(replacement);
    let counts = vega_store::recovery::recover_thread(fixture.store.conn(), &child.id, 4).unwrap();
    assert_eq!(counts.messages_interrupted, 1);
}
