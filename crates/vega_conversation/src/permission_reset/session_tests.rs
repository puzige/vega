use super::initial_tests::Fixture;
use super::{PermissionResetSession, prepare_reset};
use crate::agent::ReplaySessionRegistry;
use crate::types::{AutomationKind, AutomationStatus, PermissionMode, PermissionResetError};
use std::time::Duration;
use vega_store::{config, thread_admission};

#[test]
fn reset_session_keeps_one_pause_through_drain_completion_and_projection_retry() {
    let fixture = Fixture::new();
    fixture.task(AutomationKind::Loop);
    let database = fixture.root.join("vega.db");
    let intent = prepare_reset(&fixture.store, &fixture.config).expect("read-only intent");
    let epoch = vega_store::permission_contracts::load_state(fixture.store.conn())
        .expect("epoch")
        .authority_epoch;
    let owner =
        thread_admission::try_acquire(fixture.store.database_identity(), &fixture.thread.id)
            .expect("admission registry")
            .expect("existing owner");
    let mut session = PermissionResetSession::begin(database, fixture.config.clone(), intent)
        .expect("confirmed pause");
    let replay = ReplaySessionRegistry::new();
    assert_eq!(
        session.complete(&replay, Duration::ZERO),
        Err(PermissionResetError::Busy)
    );
    assert!(session.can_cancel());
    assert!(!session.completed());
    let tasks = crate::automation::load(&fixture.store, &fixture.thread.id).expect("paused tasks");
    assert_eq!(tasks[0].status, AutomationStatus::Paused);
    assert!(
        tasks[0].permission_authorization.is_some(),
        "stopping has not forged revocation"
    );
    assert_eq!(
        vega_store::permission_contracts::load_state(fixture.store.conn())
            .expect("unchanged authority")
            .authority_epoch,
        epoch
    );
    assert!(
        thread_admission::try_acquire(fixture.store.database_identity(), "new-owner")
            .expect("paused admission")
            .is_none()
    );
    drop(owner);
    let receipt = session
        .complete(&replay, Duration::ZERO)
        .expect("original owner released normally");
    assert!(session.completed());
    assert!(!session.can_cancel());
    assert_ne!(receipt.authority_epoch, epoch);
    assert_eq!(
        session
            .complete(&replay, Duration::ZERO)
            .expect("same receipt retry"),
        receipt
    );
    let projection = session
        .refresh(std::slice::from_ref(&fixture.thread.id))
        .expect("refresh under held pause");
    assert_eq!(
        projection.threads[0].thread.permission_mode,
        PermissionMode::Confirm
    );
    assert!(projection.threads[0].thread.full_access_grant.is_none());
    assert!(
        projection.threads[0].automation.tasks[0]
            .permission_authorization
            .is_none()
    );
    assert_eq!(
        projection.config.defaults.permission_revision,
        receipt.default_permission_revision
    );
    assert!(
        thread_admission::is_database_paused(fixture.store.database_identity())
            .expect("pause spans projection")
    );
    drop(session);
    assert!(
        !thread_admission::is_database_paused(fixture.store.database_identity())
            .expect("only owner releases pause")
    );
}

#[test]
fn session_refuses_unknown_external_cleanup_and_never_rewrites_its_history() {
    let fixture = Fixture::new();
    let original = fixture.tool("bash", false);
    let config = config::read_from(&fixture.config).expect("original configuration");
    let intent = prepare_reset(&fixture.store, &fixture.config).expect("intent");
    let mut session =
        PermissionResetSession::begin(fixture.root.join("vega.db"), fixture.config.clone(), intent)
            .expect("confirmed pause");
    let replay = ReplaySessionRegistry::new();
    for _ in 0..2 {
        assert_eq!(
            session.complete(&replay, Duration::ZERO),
            Err(PermissionResetError::UnsettledWork)
        );
        assert_eq!(
            session.blocked_thread().as_deref(),
            Some(fixture.thread.id.as_str())
        );
        assert!(
            session.can_cancel(),
            "explicit cancellation is allowed only before persistent reset"
        );
        assert!(!session.completed());
    }
    let current = vega_store::tool_calls::find_state(fixture.store.conn(), "call")
        .expect("history lookup")
        .expect("retained evidence");
    assert_eq!(current.status, original.status);
    assert_eq!(current.output_text, original.output_text);
    assert_eq!(
        config::read_from(&fixture.config)
            .expect("no false default save")
            .defaults
            .full_access_grant,
        config.defaults.full_access_grant
    );
    assert!(
        vega_store::permission_reset::load(fixture.store.conn())
            .expect("no false journal")
            .is_none()
    );
    assert!(
        thread_admission::is_database_paused(fixture.store.database_identity())
            .expect("blocked pause retained")
    );
}

#[test]
fn session_does_not_normalize_conflicting_automation_indexes_or_rebind_configuration() {
    let fixture = Fixture::new();
    fixture.task(AutomationKind::Loop);
    fixture
        .store
        .conn()
        .execute(
            "UPDATE automation_tasks SET revision=revision+1 WHERE kind='loop'",
            [],
        )
        .expect("synthetic mismatched index");
    let original: (i64, String) = fixture
        .store
        .conn()
        .query_row(
            "SELECT revision,data FROM automation_tasks WHERE kind='loop'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("original evidence");
    let intent = prepare_reset(&fixture.store, &fixture.config).expect("intent");
    let mut session =
        PermissionResetSession::begin(fixture.root.join("vega.db"), fixture.config.clone(), intent)
            .expect("confirmed pause");
    assert_eq!(
        session.complete(&ReplaySessionRegistry::new(), Duration::ZERO),
        Err(PermissionResetError::Unavailable)
    );
    assert!(
        !session.can_cancel(),
        "scheduling has not been safely paused"
    );
    let after: (i64, String) = fixture
        .store
        .conn()
        .query_row(
            "SELECT revision,data FROM automation_tasks WHERE kind='loop'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("unchanged corrupt evidence");
    assert_eq!(after, original);
    assert!(
        vega_store::permission_reset::load(fixture.store.conn())
            .expect("no journal")
            .is_none()
    );
    drop(session);
    let intent = prepare_reset(&fixture.store, &fixture.config).expect("fresh source read");
    config::update_from(&fixture.config, |config| {
        config.defaults.permission_revision += 1
    })
    .expect("competing permission change");
    assert!(matches!(
        PermissionResetSession::begin(fixture.root.join("vega.db"), fixture.config.clone(), intent),
        Err(PermissionResetError::ConfigurationConflict)
    ));
    assert!(
        !thread_admission::is_database_paused(fixture.store.database_identity())
            .expect("failed constructor releases only its own pause")
    );
}
