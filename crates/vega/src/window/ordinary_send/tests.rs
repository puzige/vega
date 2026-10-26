use super::{OrdinaryPreparationOutcome, OrdinaryPreparationRequest, take_preparation_owner};
use crate::ordinary_submission::OrdinaryPreparationOwner;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use vega_conversation::SkillSettingsService;
use vega_conversation::agent::OrdinaryRun;
use vega_conversation::permission_contracts::{
    DefaultPermissionService, default_permission_snapshot,
};
use vega_conversation::threads::{draft_thread_from_defaults, get_thread};
use vega_conversation::types::{
    DefaultPermissionValues, OrdinarySubmission, PermissionMode, PrimaryFact, RawSubmission,
    SkillSelectionIntent, SubmissionFailure, Thread,
};
use vega_store::Store;
use vega_store::config::{self, AppConfig, ProviderConfig};

#[gpui_kit::test]
fn navigation_return_reuses_preparing_entity_and_committed_authority(
    cx: &mut gpui_kit::TestAppContext,
) {
    navigation_receipt_order(cx, false);
}

#[gpui_kit::test]
fn navigation_receipt_while_away_preserves_route_and_reloads_authority(
    cx: &mut gpui_kit::TestAppContext,
) {
    navigation_receipt_order(cx, true);
}

fn navigation_receipt_order(cx: &mut gpui_kit::TestAppContext, receipt_while_away: bool) {
    use crate::window::VegaWindow;
    use gpui_kit::{AppContext, Render};
    use vega_ui::conversation_stream::ConversationStream;
    use vega_ui::settings::SettingsOpen;
    use vega_ui::sidebar::{
        NewTaskDefaults, OpenedThread, PendingDeleteConfirm, ProjectsCollapsed, SelectedProject,
        SessionsCollapsed, SidebarCollapsed, SidebarWidth, VegaStore,
    };

    let fixture = Fixture::new(true);
    let source = fixture.draft();
    let request = fixture.request(&source, Some(source.clone()));
    let attempt = request.run.worker.attempt_id();
    let committed = request.materialize().expect("commit draft");
    request.cancel.cancel();
    let mut outcome = request.finish_preparation(Ok(committed));
    let receipt = outcome.materialized.take().expect("committed receipt");
    let durable = receipt.thread.clone();
    cx.update(|cx| {
        gpui_kit::init(cx);
        vega_ui::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(VegaStore(Ok(fixture.store())));
        cx.set_global(SelectedProject(None));
        cx.set_global(OpenedThread(Some(source.clone())));
        cx.set_global(SettingsOpen(false));
        cx.set_global(SidebarCollapsed(true));
        cx.set_global(SidebarWidth(240.0));
        cx.set_global(ProjectsCollapsed(false));
        cx.set_global(SessionsCollapsed(false));
        cx.set_global(PendingDeleteConfirm(None));
        cx.set_global(NewTaskDefaults::default());
    });
    let window = cx.add_window(|_, cx| {
        let mut owner = VegaWindow::new_with_services(cx, false);
        owner.fixture_config_path = Some(fixture.config.clone());
        owner.configured_models = Some(Vec::new());
        owner.draft = Some(source.clone());
        owner
    });
    let released_streams = window
        .update(cx, |owner, window, cx| {
            let original = cx.new(|cx| ConversationStream::new(source.clone(), cx));
            let mut released_streams = vec![original.downgrade()];
            owner.stream_view = Some((source.id.clone(), original.clone()));
            owner.agent_controller.preparation_stream = Some(original.clone());
            owner.agent_controller.ordinary_preparing.insert(
                source.id.clone(),
                OrdinaryPreparationOwner {
                    attempt,
                    cancel: CancellationToken::new(),
                },
            );
            let other_source = fixture.draft();
            let other_outcome = fixture
                .request(&other_source, Some(other_source.clone()))
                .prepare();
            let other = other_outcome
                .materialized
                .as_ref()
                .expect("persisted destination")
                .thread
                .clone();
            drop(other_outcome);
            cx.set_global(OpenedThread(Some(other.clone())));
            drop(owner.render(window, cx));
            assert_ne!(owner.stream_view.as_ref().expect("other view").1, original);
            released_streams.push(
                owner
                    .stream_view
                    .as_ref()
                    .expect("other view")
                    .1
                    .downgrade(),
            );
            if !receipt_while_away {
                cx.set_global(OpenedThread(Some(source.clone())));
                drop(owner.render(window, cx));
                assert_eq!(
                    owner.stream_view.as_ref().expect("returned view").1,
                    original
                );
                assert!(owner.owns_stream_request(&original, &source.id, cx));
            }
            assert!(
                take_preparation_owner(
                    &mut owner.agent_controller.ordinary_preparing,
                    &source.id,
                    attempt
                )
                .is_some()
            );
            owner.agent_controller.preparation_stream = None;
            owner.apply_ordinary_materialization(&original, attempt, receipt, cx);
            assert!(owner.draft.is_none());
            assert_eq!(original.read(cx).thread_snapshot(), durable);
            if receipt_while_away {
                assert_eq!(cx.global::<OpenedThread>().0.as_ref(), Some(&other));
                let reopened =
                    get_thread(&fixture.store(), &source.id).expect("resolve durable route");
                cx.set_global(OpenedThread(Some(reopened)));
                drop(owner.render(window, cx));
                let visible = &owner.stream_view.as_ref().expect("durable view").1;
                assert_eq!(visible.read(cx).thread_snapshot(), durable);
                released_streams.push(visible.downgrade());
            }
            assert_eq!(cx.global::<OpenedThread>().0.as_ref(), Some(&durable));
            assert!(durable.full_access_grant.is_some());
            released_streams
        })
        .expect("navigate real window");
    drop(outcome);
    let fresh = fixture.request(&durable, None).prepare();
    assert!(fresh.launch.is_ok());
    assert!(fresh.materialized.is_none());
    drop(fresh);
    window
        .update(cx, |_, window, _| window.remove_window())
        .expect("close fixture window");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        cx.run_until_parked();
        if released_streams
            .iter()
            .all(|stream| stream.upgrade().is_none())
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture streams must be released before temporary storage"
        );
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(10));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

struct Fixture {
    root: tempfile::TempDir,
    database: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new(full_access: bool) -> Self {
        let root = tempfile::tempdir().expect("create isolated fixture");
        let fixture = Self {
            database: root.path().join("state.sqlite"),
            config: root.path().join("config").join("config.toml"),
            root,
        };
        let store = fixture.store();
        store.migrate().expect("migrate fixture database");
        vega_store::keystore::set_key(
            fixture.config.parent().expect("fixture config root"),
            "fixture-provider",
            "synthetic-provider-value",
        )
        .expect("create synthetic credential");
        let config = AppConfig {
            providers: vec![ProviderConfig {
                name: "fixture-provider".into(),
                base_url: "https://provider.invalid/v1".into(),
                models: vec!["model".into()],
                key_ref: "fixture-provider".into(),
                ..ProviderConfig::default()
            }],
            ..AppConfig::default()
        };
        config
            .save_to(&fixture.config)
            .expect("save fixture config");
        if full_access {
            let service =
                DefaultPermissionService::new(fixture.database.clone(), fixture.config.clone());
            let change = service
                .prepare(
                    &DefaultPermissionValues::from(&config.defaults),
                    PermissionMode::FullAccess,
                )
                .expect("prepare default authorization");
            service
                .save(&change, true)
                .expect("authorize fixture default");
        }
        fixture
    }

    fn store(&self) -> Store {
        Store::open(&self.database).expect("open fixture database")
    }

    fn draft(&self) -> Thread {
        let config = config::read_from(&self.config).expect("read fixture default");
        draft_thread_from_defaults(
            None,
            "model",
            default_permission_snapshot(Some(&self.config), &config.defaults),
        )
    }

    fn request(&self, expected: &Thread, draft: Option<Thread>) -> OrdinaryPreparationRequest {
        let store = self.store();
        let admission =
            vega_store::thread_admission::try_acquire(store.database_identity(), &expected.id)
                .expect("acquire fixture admission")
                .expect("fixture admission is free");
        let (submission, worker) = OrdinarySubmission::begin(
            store.database_identity(),
            expected.id.clone(),
            RawSubmission::new("hello".into(), Vec::new(), 0, "model".into(), None, None),
            admission,
        )
        .expect("begin fixture submission");
        let registered = worker
            .register_worker()
            .expect("register preparation worker");
        OrdinaryPreparationRequest {
            registered,
            run: OrdinaryRun::new(submission, Arc::new(worker)),
            database: self.database.clone(),
            config: Some(self.config.clone()),
            expected: expected.clone(),
            draft,
            skills: None,
            cancel: CancellationToken::new(),
        }
    }

    fn unavailable_skill(&self) -> (SkillSettingsService, SkillSelectionIntent) {
        (
            SkillSettingsService::new(
                self.database.clone(),
                self.config
                    .parent()
                    .expect("fixture config root")
                    .to_path_buf(),
                None,
            ),
            SkillSelectionIntent {
                source_id: ulid::Ulid::generate().to_string(),
                name: "removed-skill".into(),
                content_sha256: "a".repeat(64),
                expected_consent_generation: 0,
            },
        )
    }

    fn admission_is_held(&self, thread_id: &str) -> bool {
        vega_store::thread_admission::try_acquire(self.store().database_identity(), thread_id)
            .expect("probe fixture admission")
            .is_none()
    }
}

fn assert_finished(submission: &OrdinarySubmission, failure: SubmissionFailure) {
    let snapshot = submission.snapshot();
    let attempt = snapshot.attempts.last().expect("finished attempt");
    assert!(attempt.sealed);
    assert!(!attempt.abnormal);
    assert_eq!(attempt.workers, 0);
    assert_eq!(attempt.primary, PrimaryFact::NotStarted);
    assert_eq!(attempt.failure, Some(failure));
}

fn assert_receipt_allows_fresh_send(
    fixture: &Fixture,
    source: &Thread,
    submission: &OrdinarySubmission,
    outcome: OrdinaryPreparationOutcome,
) {
    let receipt = outcome.materialized.as_ref().expect("committed receipt");
    assert_eq!(
        get_thread(&fixture.store(), &source.id).expect("durable thread"),
        receipt.thread
    );
    let accepted = submission.permission_fence().expect("accepted authority");
    assert_eq!(accepted.full_access_grant, receipt.thread.full_access_grant);
    assert!(fixture.admission_is_held(&source.id));
    let attempt = submission
        .snapshot()
        .attempts
        .last()
        .expect("original attempt")
        .id;
    let mut draft = Some(source.clone());
    let projection = receipt
        .reconcile(attempt, &mut draft, source, Some(source))
        .expect("matching receipt");
    assert!(draft.is_none());
    assert!(projection.stream);
    assert!(projection.route);
    let durable = receipt.thread.clone();
    drop(outcome);
    assert!(!fixture.admission_is_held(&source.id));

    let request = fixture.request(&durable, draft);
    let fresh = request.run.submission.clone();
    assert_ne!(fresh.id(), submission.id());
    let outcome = request.prepare();
    assert!(outcome.materialized.is_none());
    let launch = outcome
        .launch
        .as_ref()
        .expect("fresh send prepares successfully");
    assert!(launch.workspace.starts_with(fixture.root.path()));
    assert_eq!(fresh.permission_fence().expect("fresh authority"), accepted);
    drop(outcome);
    assert_finished(&fresh, SubmissionFailure::LaunchFailed);
    assert!(!fixture.admission_is_held(&source.id));
}

#[test]
fn cancellation_after_commit_keeps_receipt_and_allows_fresh_send() {
    for full_access in [false, true] {
        let fixture = Fixture::new(full_access);
        let source = fixture.draft();
        let request = fixture.request(&source, Some(source.clone()));
        let submission = request.run.submission.clone();
        let committed = request.materialize().expect("commit before cancellation");
        assert_eq!(
            get_thread(&fixture.store(), &source.id).expect("committed row"),
            committed.1
        );
        request.cancel.cancel();
        let outcome = request.finish_preparation(Ok(committed));
        assert!(matches!(outcome.launch, Err(SubmissionFailure::Cancelled)));
        assert_finished(&submission, SubmissionFailure::Cancelled);
        assert_receipt_allows_fresh_send(&fixture, &source, &submission, outcome);
    }
}

#[test]
fn skill_pin_failure_after_commit_keeps_receipt_and_allows_fresh_send() {
    let fixture = Fixture::new(true);
    let source = fixture.draft();
    let mut request = fixture.request(&source, Some(source.clone()));
    request.skills = Some(fixture.unavailable_skill());
    let submission = request.run.submission.clone();
    let outcome = request.prepare();
    assert!(matches!(
        outcome.launch,
        Err(SubmissionFailure::PreparationFailed)
    ));
    assert_finished(&submission, SubmissionFailure::PreparationFailed);
    assert!(
        vega_store::skills::list_thread_pins(fixture.store().conn(), &source.id)
            .expect("read failed pin state")
            .is_empty()
    );
    assert_receipt_allows_fresh_send(&fixture, &source, &submission, outcome);
}

#[test]
fn cancellation_before_commit_never_materializes_or_accepts_authority() {
    let fixture = Fixture::new(true);
    let source = fixture.draft();
    let request = fixture.request(&source, Some(source.clone()));
    let submission = request.run.submission.clone();
    request.cancel.cancel();
    let outcome = request.prepare();
    assert!(matches!(outcome.launch, Err(SubmissionFailure::Cancelled)));
    assert!(outcome.materialized.is_none());
    assert!(submission.permission_fence().is_err());
    assert!(
        vega_store::threads::find(fixture.store().conn(), &source.id)
            .expect("read cancelled draft")
            .is_none()
    );
    assert_finished(&submission, SubmissionFailure::Cancelled);
    drop(outcome);
    assert!(!fixture.admission_is_held(&source.id));
}

#[test]
fn late_receipt_cannot_release_retry_owner_or_reconcile_its_draft() {
    let fixture = Fixture::new(true);
    let source = fixture.draft();
    let mut request = fixture.request(&source, Some(source.clone()));
    request.skills = Some(fixture.unavailable_skill());
    let submission = request.run.submission.clone();
    let attempt = request.run.worker.attempt_id();
    let mut outcome = request.prepare();
    let receipt = outcome.materialized.take().expect("old committed receipt");
    drop(outcome);
    let retry = submission.try_retry(attempt).expect("acquire retry");
    let retry_attempt = retry.attempt_id();
    assert_ne!(retry_attempt, attempt);
    let cancel = CancellationToken::new();
    let mut owners = HashMap::from([(
        source.id.clone(),
        OrdinaryPreparationOwner {
            attempt: retry_attempt,
            cancel: cancel.clone(),
        },
    )]);
    assert!(take_preparation_owner(&mut owners, &source.id, attempt).is_none());
    assert_eq!(
        owners.get(&source.id).expect("retry owner remains").attempt,
        retry_attempt
    );
    assert!(!cancel.is_cancelled());
    let mut draft = Some(source.clone());
    assert!(
        receipt
            .reconcile(retry_attempt, &mut draft, &source, Some(&source))
            .is_none()
    );
    assert_eq!(draft, Some(source.clone()));
    assert!(fixture.admission_is_held(&source.id));
    assert!(take_preparation_owner(&mut owners, &source.id, retry_attempt).is_some());
    assert!(owners.is_empty());
    retry
        .complete(Err(SubmissionFailure::Cancelled))
        .expect("finish retry");
    submission.seal(retry_attempt).expect("seal retry");
    assert!(!fixture.admission_is_held(&source.id));
}

#[test]
fn late_receipt_preserves_changed_source_and_other_route_but_updates_original_stream() {
    let fixture = Fixture::new(true);
    let source = fixture.draft();
    let mut request = fixture.request(&source, Some(source.clone()));
    request.skills = Some(fixture.unavailable_skill());
    let attempt = request.run.worker.attempt_id();
    let outcome = request.prepare();
    let receipt = outcome.materialized.as_ref().expect("committed receipt");
    let mut changed = source.clone();
    changed.model = "new-model".into();
    let mut draft = Some(changed.clone());
    let projection = receipt
        .reconcile(attempt, &mut draft, &changed, Some(&changed))
        .expect("same owner");
    assert_eq!(draft, Some(changed));
    assert!(!projection.stream);
    assert!(!projection.route);

    let other = fixture.draft();
    let mut draft = Some(other.clone());
    let projection = receipt
        .reconcile(attempt, &mut draft, &source, Some(&other))
        .expect("receipt for background stream");
    assert_eq!(draft, Some(other));
    assert!(projection.stream);
    assert!(!projection.route);

    let mut draft = Some(source.clone());
    let projection = receipt
        .reconcile(attempt, &mut draft, &source, None)
        .expect("receipt after leaving route");
    assert!(draft.is_none());
    assert!(projection.stream);
    assert!(!projection.route);
}

#[test]
fn undeliverable_receipt_releases_admission_without_rolling_back_durable_thread() {
    let fixture = Fixture::new(true);
    let source = fixture.draft();
    let mut request = fixture.request(&source, Some(source.clone()));
    request.skills = Some(fixture.unavailable_skill());
    let submission = request.run.submission.clone();
    let outcome = request.prepare();
    let durable = outcome
        .materialized
        .as_ref()
        .expect("committed receipt")
        .thread
        .clone();
    assert!(fixture.admission_is_held(&source.id));
    let (sender, receiver) = std::sync::mpsc::channel();
    drop(receiver);
    let Err(rejected) = sender.send(outcome) else {
        panic!("closed receipt channel accepted an outcome");
    };
    assert!(fixture.admission_is_held(&source.id));
    drop(rejected);
    assert!(!fixture.admission_is_held(&source.id));
    assert_finished(&submission, SubmissionFailure::PreparationFailed);
    assert_eq!(
        get_thread(&fixture.store(), &source.id).expect("reopen durable row"),
        durable
    );
    let outcome = fixture.request(&durable, None).prepare();
    assert!(outcome.materialized.is_none());
    assert!(outcome.launch.is_ok());
}
