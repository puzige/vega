use super::*;
use crate::agent::{PersistenceActorConfig, prepare_run_with_images_and_reasoning};

struct Fixture {
    root: PathBuf,
    store: Store,
    service: ThreadPermissionService,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-permission-editor-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("isolated fixture");
        let path = root.join("vega.db");
        let store = Store::open(&path).expect("fixture store");
        store.migrate().expect("fixture schema");
        Self {
            root,
            store,
            service: ThreadPermissionService::new(path),
        }
    }

    fn draft(&self) -> Thread {
        crate::threads::draft_thread(None, "synthetic", "confirm")
    }

    fn authorized(&self) -> Thread {
        let change = self
            .service
            .prepare(&self.draft(), PermissionMode::FullAccess)
            .expect("specific draft intent");
        self.service.save(&change, true).expect("confirmed thread")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn explicit_full_access_confirmation_and_draft_materialization_commit_together() {
    let fixture = Fixture::new();
    let source = fixture.draft();
    let change = fixture
        .service
        .prepare(&source, PermissionMode::FullAccess)
        .expect("intent only");
    assert!(change.is_draft());
    assert!(
        threads::find(fixture.store.conn(), &source.id)
            .expect("no preparation write")
            .is_none()
    );
    assert_eq!(
        fixture.service.save(&change, false),
        Err(ThreadPermissionError::ConfirmationRequired)
    );
    fixture.store.conn().execute_batch("CREATE TRIGGER refuse_new_thread BEFORE INSERT ON threads BEGIN SELECT RAISE(ABORT,'synthetic save failure'); END;").expect("synthetic failure");
    assert!(fixture.service.save(&change, true).is_err());
    assert!(
        threads::find(fixture.store.conn(), &source.id)
            .expect("no partial authorization")
            .is_none()
    );
    fixture
        .store
        .conn()
        .execute_batch("DROP TRIGGER refuse_new_thread")
        .expect("remove fixture fault");
    let saved = fixture
        .service
        .save(&change, true)
        .expect("explicit confirmed draft");
    assert_eq!(saved.id, source.id);
    assert_eq!(saved.project_id, source.project_id);
    assert_eq!(saved.model, source.model);
    assert_eq!(saved.permission_mode, PermissionMode::FullAccess);
    assert_eq!(saved.permission_revision, 1);
    assert!(saved.default_permission.is_none());
    assert!(
        saved
            .full_access_grant
            .as_ref()
            .expect("new grant")
            .verifies(
                &change.authority_epoch,
                &FullAccessScope::Thread {
                    thread_id: source.id
                }
            )
    );
    assert_eq!(
        fixture.service.save(&change, true),
        Err(ThreadPermissionError::Conflict)
    );
}

#[test]
fn same_mode_reauthorization_rotates_the_grant_and_stale_confirmations_fail_closed() {
    let fixture = Fixture::new();
    let first = fixture.authorized();
    let change = fixture
        .service
        .prepare(&first, PermissionMode::FullAccess)
        .expect("same-mode confirmation");
    let second = fixture.service.save(&change, true).expect("fresh grant");
    assert_eq!(second.permission_revision, first.permission_revision + 1);
    assert_ne!(first.full_access_grant, second.full_access_grant);
    assert_eq!(
        fixture.service.save(&change, true),
        Err(ThreadPermissionError::Conflict)
    );
    let pending = fixture
        .service
        .prepare(&second, PermissionMode::FullAccess)
        .expect("pending confirmation");
    fixture
        .store
        .conn()
        .execute(
            "UPDATE permission_contract_state SET authority_epoch=?1",
            [ulid::Ulid::generate().to_string()],
        )
        .expect("changed authority");
    assert_eq!(
        fixture.service.save(&pending, true),
        Err(ThreadPermissionError::Conflict)
    );
    assert_eq!(
        fixture
            .service
            .read(&second.id)
            .expect("unchanged permission"),
        second
    );
}

#[test]
fn future_permission_edit_preserves_the_live_runs_accepted_authority() {
    let fixture = Fixture::new();
    let first = fixture.authorized();
    let run = prepare_run_with_images_and_reasoning(
        fixture.root.join("vega.db"),
        first.id.clone(),
        "synthetic request".into(),
        "synthetic system".into(),
        ulid::Ulid::generate().to_string(),
        ulid::Ulid::generate().to_string(),
        PersistenceActorConfig::default(),
        false,
        None,
        None,
        Vec::new(),
        None,
    )
    .expect("accepted run retains admission");
    let accepted = run.permission_fence.clone();
    let change = fixture
        .service
        .prepare(&first, PermissionMode::Confirm)
        .expect("next-run choice");
    let next = fixture
        .service
        .save(&change, false)
        .expect("save future choice during active run");
    assert_eq!(next.permission_mode, PermissionMode::Confirm);
    assert!(next.full_access_grant.is_none());
    let saved = vega_store::permission_contracts::load_run_fence(
        fixture.store.conn(),
        &run.assistant_message_id,
        &first.id,
    )
    .expect("original fence")
    .expect("durable original fence");
    assert_eq!(
        serde_json::from_str::<crate::types::PermissionFence>(&saved).expect("typed fence"),
        accepted
    );
    assert_eq!(accepted.permission_mode, "full_access");
    assert_eq!(accepted.full_access_grant, first.full_access_grant);
    let pause = crate::permission_reset::pause_admission(&fixture.store).expect("reset pause");
    assert_eq!(
        fixture.service.prepare(&next, PermissionMode::Auto),
        Err(ThreadPermissionError::ResetPending)
    );
    assert!(
        !pause
            .owners_released()
            .expect("live run still owns admission")
    );
    drop(run);
    assert!(
        pause
            .owners_released()
            .expect("normal owner release remains possible")
    );
}

#[test]
fn a_missing_or_replaced_database_never_receives_an_old_permission_confirmation() {
    for replaced in [false, true] {
        let mut fixture = Fixture::new();
        let change = fixture
            .service
            .prepare(&fixture.draft(), PermissionMode::FullAccess)
            .expect("original source");
        let path = fixture.root.join("vega.db");
        let original = std::mem::replace(
            &mut fixture.store,
            Store::open(":memory:").expect("placeholder connection"),
        );
        drop(original);
        std::fs::rename(&path, fixture.root.join("original.db")).expect("replace fixture database");
        if replaced {
            let other = Store::open(&path).expect("different database");
            other.migrate().expect("different schema source");
            assert_eq!(
                fixture.service.save(&change, true),
                Err(ThreadPermissionError::Conflict)
            );
        } else {
            assert_eq!(
                fixture.service.save(&change, true),
                Err(ThreadPermissionError::Unavailable)
            );
            assert!(!path.exists());
        }
    }
}
