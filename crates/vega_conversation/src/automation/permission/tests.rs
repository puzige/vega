use super::*;
use crate::permission_contracts::change_thread_permission;

struct Fixture {
    root: PathBuf,
    store: Store,
    thread: Thread,
    service: AutomationPermissionService,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "vega-automation-authorization-{}",
            ulid::Ulid::generate()
        ));
        std::fs::create_dir(&root).expect("isolated fixture");
        let database = root.join("vega.db");
        let store = Store::open(&database).expect("fixture database");
        store.migrate().expect("fixture schema");
        let thread = crate::threads::materialize_draft(
            &store,
            &crate::threads::draft_thread(None, "synthetic", "confirm"),
        )
        .expect("fixture thread");
        let thread =
            change_thread_permission(&store, &thread.id, 0, PermissionMode::FullAccess, true, 1)
                .expect("separate thread grant");
        Self {
            root,
            store,
            thread,
            service: AutomationPermissionService::new(database),
        }
    }
    fn task(&self) -> AutomationTask {
        command(
            &self.store,
            &self.thread.id,
            &AutomationCommand::Create {
                kind: AutomationKind::Loop,
                instruction: "synthetic recurring work".into(),
                token_budget: Some(1000),
                interval_seconds: Some(60),
            },
            now_ms(),
        )
        .expect("task definition")
        .remove(0)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn automation_consent_is_independent_and_resume_authorizes_the_new_revision_atomically() {
    let fixture = Fixture::new();
    let active = fixture.task();
    assert!(
        active.permission_authorization.is_none(),
        "thread consent never authorizes automation"
    );
    assert!(
        claim(
            &fixture.store,
            &fixture.thread.id,
            Some(AutomationKind::Loop),
            now_ms()
        )
        .is_err()
    );
    let paused = command(
        &fixture.store,
        &fixture.thread.id,
        &AutomationCommand::Control(AutomationKind::Loop, AutomationControl::Pause),
        now_ms(),
    )
    .expect("paused target")
    .remove(0);
    let change = fixture
        .service
        .prepare(&fixture.thread, &paused, true)
        .expect("exact task confirmation");
    assert_eq!(
        load(&fixture.store, &fixture.thread.id).expect("no prepare writes"),
        vec![paused.clone()]
    );
    assert_eq!(
        fixture.service.save(&change, false),
        Err(Error::ConfirmationRequired)
    );
    fixture.store.conn().execute_batch("CREATE TRIGGER refuse_automation_resume BEFORE UPDATE ON automation_tasks BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").expect("injected save failure");
    assert!(fixture.service.save(&change, true).is_err());
    assert_eq!(
        load(&fixture.store, &fixture.thread.id).expect("no partial resume/grant"),
        vec![paused.clone()]
    );
    fixture
        .store
        .conn()
        .execute_batch("DROP TRIGGER refuse_automation_resume")
        .expect("remove fault");
    let saved = fixture
        .service
        .save(&change, true)
        .expect("explicit task consent")
        .remove(0);
    assert_eq!(saved.status, AutomationStatus::Active);
    assert_eq!(saved.revision, paused.revision + 1);
    let authorization = saved
        .permission_authorization
        .as_ref()
        .expect("task-specific authorization");
    assert_eq!(authorization.revision, saved.revision);
    assert_eq!(authorization.identity, saved.identity);
    assert_eq!(authorization.kind, saved.kind);
    let run = claim(
        &fixture.store,
        &fixture.thread.id,
        Some(AutomationKind::Loop),
        now_ms(),
    )
    .expect("authorized claim")
    .expect("ready automation");
    assert_eq!(
        run.tasks[0].permission_authorization,
        saved.permission_authorization
    );
    release_claim(&fixture.store, &run).expect("fixture releases claim normally");
    assert_eq!(
        fixture.service.save(&change, true),
        Err(Error::Conflict),
        "old resume confirmation cannot replay"
    );
}

#[test]
fn stale_task_edits_thread_reauthorization_and_reset_pause_reject_old_consent() {
    let fixture = Fixture::new();
    let task = fixture.task();
    let change = fixture
        .service
        .prepare(&fixture.thread, &task, false)
        .expect("original task intent");
    let edited = command(
        &fixture.store,
        &fixture.thread.id,
        &AutomationCommand::Edit {
            kind: task.kind,
            identity: task.identity,
            revision: task.revision,
            instruction: "different task".into(),
        },
        now_ms(),
    )
    .expect("edit target")
    .remove(0);
    assert_eq!(fixture.service.save(&change, true), Err(Error::Conflict));
    let change = fixture
        .service
        .prepare(&fixture.thread, &edited, false)
        .expect("new exact task intent");
    let current = change_thread_permission(
        &fixture.store,
        &fixture.thread.id,
        fixture.thread.permission_revision,
        PermissionMode::FullAccess,
        true,
        2,
    )
    .expect("new independent thread grant");
    assert_eq!(fixture.service.save(&change, true), Err(Error::Conflict));
    let change = fixture
        .service
        .prepare(&current, &edited, false)
        .expect("new task and thread intent");
    let pause = crate::permission_reset::pause_admission(&fixture.store).expect("reset pause");
    assert_eq!(
        fixture.service.save(&change, true),
        Err(Error::ResetPending)
    );
    drop(pause);
    assert!(
        load(&fixture.store, &current.id).expect("no inherited task grant")[0]
            .permission_authorization
            .is_none()
    );
    assert_eq!(
        fixture
            .service
            .save(&change, true)
            .expect("fresh explicit independent consent")[0]
            .revision,
        edited.revision
    );
}

#[test]
fn automation_consent_rejects_conflicting_indexes_and_missing_storage_without_creation() {
    let fixture = Fixture::new();
    let task = fixture.task();
    fixture
        .store
        .conn()
        .execute("UPDATE automation_tasks SET revision=revision+1", [])
        .expect("synthetic index conflict");
    assert_eq!(
        fixture.service.prepare(&fixture.thread, &task, false),
        Err(Error::Conflict)
    );
    let missing = fixture.root.join("absent.db");
    let service = AutomationPermissionService::new(missing.clone());
    assert!(matches!(
        service.read(&fixture.thread.id),
        Err(Error::Unavailable)
    ));
    assert!(!missing.exists());
    assert_eq!(
        service.prepare(&fixture.thread, &task, false),
        Err(Error::Unavailable)
    );
    assert!(!missing.exists());
}
