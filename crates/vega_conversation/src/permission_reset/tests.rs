use std::path::PathBuf;

use vega_store::{Store, config::AppConfig, thread_admission};

use super::pause_admission;
use crate::permission_contracts::{
    DefaultPermissionService, capture_fence, change_thread_permission, default_permission_snapshot,
};
use crate::types::{
    AutomationCommand, AutomationControl, AutomationKind,
    AutomationPermissionAuthorization as AutomationGrant, AutomationStatus, ConversationError,
    DefaultPermissionError, DefaultPermissionValues, PermissionMode, PermissionResetError,
    StartupError, Thread,
};
use crate::{automation, threads};

struct Fixture {
    root: PathBuf,
    database: PathBuf,
    config: PathBuf,
    store: Store,
    thread: Thread,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-reset-barrier-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("isolated directory");
        let root = root.canonicalize().expect("canonical directory");
        let database = root.join("vega.db");
        let config = root.join("config.toml");
        AppConfig::default()
            .save_to(&config)
            .expect("isolated configuration");
        let store = Store::open(&database).expect("isolated database");
        store.migrate().expect("migrate fixture");
        let thread = threads::materialize_draft(
            &store,
            &threads::draft_thread(None, "synthetic", "confirm"),
        )
        .expect("fixture thread");
        Self {
            root,
            database,
            config,
            store,
            thread,
        }
    }

    fn service(&self) -> DefaultPermissionService {
        DefaultPermissionService::new(self.database.clone(), self.config.clone())
    }

    fn values(&self) -> DefaultPermissionValues {
        let config = vega_store::config::read_from(&self.config).expect("fixture config");
        DefaultPermissionValues::from(&config.defaults)
    }

    fn create_task(&self, kind: AutomationKind) {
        automation::command(
            &self.store,
            &self.thread.id,
            &AutomationCommand::Create {
                kind,
                instruction: "retained instruction".into(),
                token_budget: None,
                interval_seconds: (kind == AutomationKind::Loop).then_some(60),
            },
            1,
        )
        .expect("create automation");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn pause_blocks_authority_and_draft_changes_without_mutating_source_or_other_databases() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let service = fixture.service();
    let first = service
        .prepare(&fixture.values(), PermissionMode::FullAccess)
        .expect("default confirmation");
    let values = service.save(&first, true).expect("authorized default");
    let prepared = service
        .prepare(&values, PermissionMode::FullAccess)
        .expect("reauthorization intent");
    let config = vega_store::config::read_from(&fixture.config).expect("source defaults");
    let draft = threads::draft_thread_from_defaults(
        None,
        "synthetic",
        default_permission_snapshot(Some(&fixture.config), &config.defaults),
    );
    let before = std::fs::read(&fixture.config).expect("original bytes");
    let authority =
        vega_store::permission_contracts::load_state(fixture.store.conn()).expect("original epoch");
    let owner =
        thread_admission::try_acquire(fixture.store.database_identity(), &fixture.thread.id)
            .expect("admission registry")
            .expect("existing owner");
    let pause = pause_admission(&fixture.store).expect("pause before durable pending");
    assert!(matches!(
        pause_admission(&fixture.store),
        Err(PermissionResetError::Busy)
    ));
    assert!(!pause.owners_released().expect("existing owner retained"));
    let reopened = Store::open(&fixture.database).expect("same database reopened");
    assert!(
        thread_admission::is_database_paused(reopened.database_identity())
            .expect("same identity paused")
    );
    assert_eq!(service.read(), Err(DefaultPermissionError::ResetPending));
    assert!(matches!(
        service.prepare(&values, PermissionMode::Confirm),
        Err(DefaultPermissionError::ResetPending)
    ));
    assert_eq!(
        service.save(&prepared, true),
        Err(DefaultPermissionError::ResetPending)
    );
    assert!(matches!(
        threads::materialize_draft(&fixture.store, &draft),
        Err(ConversationError::InvalidTaskIdentity)
    ));
    assert!(
        change_thread_permission(
            &fixture.store,
            &fixture.thread.id,
            0,
            PermissionMode::FullAccess,
            true,
            2
        )
        .is_err()
    );
    assert!(matches!(
        crate::startup::prepare_workspace(&fixture.database, &fixture.config),
        Err(StartupError::PermissionReset(PermissionResetError::Busy))
    ));
    assert_eq!(
        std::fs::read(&fixture.config).expect("unchanged bytes"),
        before
    );
    assert!(
        vega_store::threads::find(fixture.store.conn(), &draft.id)
            .expect("no materialization")
            .is_none()
    );
    assert_eq!(
        threads::get_thread(&fixture.store, &fixture.thread.id).expect("unchanged thread"),
        fixture.thread
    );
    let after = vega_store::permission_contracts::load_state(fixture.store.conn())
        .expect("unchanged epoch");
    assert_eq!(after.authority_epoch, authority.authority_epoch);
    assert!(!after.reset_pending);
    assert!(
        vega_store::permission_reset::load(fixture.store.conn())
            .expect("no reset journal")
            .is_none()
    );
    let other_change = other
        .service()
        .prepare(&other.values(), PermissionMode::FullAccess)
        .expect("other database intent");
    other
        .service()
        .save(&other_change, true)
        .expect("other database still admits changes");
    drop(owner);
    assert!(pause.owners_released().expect("owner drained"));
    assert!(matches!(
        change_thread_permission(
            &fixture.store,
            &fixture.thread.id,
            0,
            PermissionMode::FullAccess,
            true,
            2
        ),
        Err(ConversationError::InvalidTaskIdentity)
    ));
    drop(pause);
    let created = threads::materialize_draft(&fixture.store, &draft)
        .expect("same draft after barrier release");
    assert!(created.full_access_grant.is_some());
    change_thread_permission(
        &fixture.store,
        &fixture.thread.id,
        0,
        PermissionMode::FullAccess,
        true,
        2,
    )
    .expect("thread authorization resumes after barrier release");
    service
        .save(&prepared, true)
        .expect("unchanged-source intent works after release");
    assert!(
        !thread_admission::is_database_paused(fixture.store.database_identity())
            .expect("barrier released")
    );
}

#[test]
fn pause_blocks_automation_authority_and_claims_but_allows_evidence_preserving_release() {
    let fixture = Fixture::new();
    change_thread_permission(
        &fixture.store,
        &fixture.thread.id,
        0,
        PermissionMode::FullAccess,
        true,
        1,
    )
    .expect("thread confirmation");
    fixture.create_task(AutomationKind::Goal);
    let task = automation::load(&fixture.store, &fixture.thread.id)
        .expect("task")
        .remove(0);
    let row = vega_store::threads::find(fixture.store.conn(), &fixture.thread.id)
        .expect("thread")
        .expect("thread exists");
    let authorization = AutomationGrant {
        kind: task.kind,
        identity: task.identity.clone(),
        revision: task.revision,
        fence: capture_fence(&fixture.store, &row).expect("authorized thread fence"),
    };
    automation::authorize_full_access(&fixture.store, &authorization, true, 2)
        .expect("automation confirmation");
    let claim = automation::claim(
        &fixture.store,
        &fixture.thread.id,
        Some(AutomationKind::Goal),
        3,
    )
    .expect("claim result")
    .expect("unstarted claim");
    assert!(!claim.started);
    let before_tasks =
        automation::load(&fixture.store, &fixture.thread.id).expect("authorized task evidence");
    let before_run =
        vega_store::automation::run(fixture.store.conn(), &fixture.thread.id, &claim.id)
            .expect("claim evidence");
    let pause = pause_admission(&fixture.store).expect("pause");
    assert!(automation::authorize_full_access(&fixture.store, &authorization, true, 4).is_err());
    assert!(automation::claim(&fixture.store, &fixture.thread.id, None, 4).is_err());
    assert!(automation::activate_claim(&fixture.store, &claim).is_err());
    assert!(
        automation::command(
            &fixture.store,
            &fixture.thread.id,
            &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Resume),
            4
        )
        .is_err()
    );
    assert!(automation::recover_startup(&fixture.store).is_err());
    assert_eq!(
        automation::load(&fixture.store, &fixture.thread.id).expect("retained tasks"),
        before_tasks
    );
    assert_eq!(
        vega_store::automation::run(fixture.store.conn(), &fixture.thread.id, &claim.id)
            .expect("retained claim"),
        before_run
    );
    automation::release_claim(&fixture.store, &claim).expect("unstarted claim can release");
    assert!(
        vega_store::automation::run(fixture.store.conn(), &fixture.thread.id, &claim.id)
            .expect("released claim")
            .expect("record retained")
            .0
    );
    automation::pause_all(&fixture.store, &fixture.thread.id, "user_paused")
        .expect("pause-only cleanup remains usable");
    assert_eq!(
        automation::load(&fixture.store, &fixture.thread.id).expect("paused task")[0].status,
        AutomationStatus::Paused
    );
    drop(pause);
    let tasks = automation::command(
        &fixture.store,
        &fixture.thread.id,
        &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Resume),
        5,
    )
    .expect("resume after barrier release");
    let task = &tasks[0];
    let authorization = AutomationGrant {
        revision: task.revision,
        ..authorization
    };
    automation::authorize_full_access(&fixture.store, &authorization, true, 6)
        .expect("fresh automation confirmation");
    assert!(
        automation::claim(
            &fixture.store,
            &fixture.thread.id,
            Some(AutomationKind::Goal),
            7
        )
        .expect("fresh claim")
        .is_some()
    );
}

#[test]
fn pause_does_not_block_completion_of_already_started_automation() {
    let fixture = Fixture::new();
    fixture.create_task(AutomationKind::Goal);
    let claim = automation::claim(&fixture.store, &fixture.thread.id, None, 2)
        .expect("claim result")
        .expect("started claim");
    assert!(claim.started);
    let pause = pause_admission(&fixture.store).expect("pause");
    automation::finish(&fixture.store, &claim, true, true, 0, 3)
        .expect("completion remains available");
    assert!(
        vega_store::automation::run(fixture.store.conn(), &fixture.thread.id, &claim.id)
            .expect("completed claim")
            .expect("retained claim")
            .0
    );
    assert!(pause.owners_released().expect("no owners"));
}
