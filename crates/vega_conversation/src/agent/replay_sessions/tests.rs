use std::panic::{AssertUnwindSafe, catch_unwind};

use vega_runtime::{ChatMessage, ChatRole, ReasoningReplayPolicy, ReplayFrame};

use super::{ReplayCheckpointFence, ReplaySessionLease, ReplaySessionRegistry};
use crate::types::PermissionResetError;

fn begin(registry: &ReplaySessionRegistry, thread: &str) -> ReplaySessionLease {
    registry.begin(
        thread,
        1,
        "source",
        ReplayCheckpointFence::from_checkpoint(None),
    )
}

fn seal(registry: &ReplaySessionRegistry, lease: ReplaySessionLease) -> bool {
    let mut frame = ReplayFrame::new(1, ReasoningReplayPolicy::Unknown);
    frame.bind_environment([7; 32]);
    let mut system = ChatMessage::new(ChatRole::System, "retained system");
    system.attach_replay_frame(frame);
    registry.seal(
        lease,
        1,
        "source".into(),
        ReplayCheckpointFence::from_checkpoint(None),
        "owner".into(),
        vec![
            system,
            ChatMessage::new(ChatRole::Assistant, "retained answer"),
        ],
    )
}

#[test]
fn acknowledged_invalidation_removes_idle_replay_and_rejects_old_lease_resealing() {
    let registry = ReplaySessionRegistry::new();
    assert!(seal(&registry, begin(&registry, "idle")));
    let reusable = begin(&registry, "idle");
    assert!(reusable.candidate().is_some());
    assert!(seal(&registry, reusable));
    let old = begin(&registry, "active");
    registry
        .invalidate_all_acknowledged()
        .expect("acknowledged reset");
    assert!(begin(&registry, "idle").candidate().is_none());
    assert!(!seal(&registry, old));
    assert!(begin(&registry, "active").candidate().is_none());
    assert!(seal(&registry, begin(&registry, "fresh")));
    assert!(begin(&registry, "fresh").candidate().is_some());
    registry
        .invalidate_all_acknowledged()
        .expect("repeat invalidation");
}

#[test]
fn poisoned_replay_registry_cannot_acknowledge_invalidation_or_reuse_sessions() {
    let registry = ReplaySessionRegistry::new();
    assert!(seal(&registry, begin(&registry, "thread")));
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _guard = registry.inner.lock().expect("fixture lock");
            panic!("isolated poison");
        }))
        .is_err()
    );
    assert_eq!(
        registry.invalidate_all_acknowledged(),
        Err(PermissionResetError::Unavailable)
    );
    let lease = begin(&registry, "thread");
    assert!(lease.candidate().is_none());
    assert!(!seal(&registry, lease));
}

#[test]
fn reset_keeps_pending_and_source_configuration_when_replay_invalidation_is_unavailable() {
    use crate::permission_reset::{
        pause_admission, prepare_reset, recover_pending_with_pause, reset_quiescent,
    };
    use vega_store::{Store, config, permission_reset};

    let root = std::env::temp_dir().join(format!("vega-reset-replay-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).expect("isolated root");
    let config_path = root.join("config.toml");
    let mut value = config::AppConfig::default();
    value.defaults.permission_mode = "full_access".into();
    value.defaults.permission_revision = 7;
    value.defaults.full_access_grant = Some("synthetic metadata to revoke".into());
    value.save_to(&config_path).expect("isolated configuration");
    let store = Store::open(":memory:").expect("isolated database");
    store.migrate().expect("fixture migrations");
    let intent = prepare_reset(&store, &config_path).expect("reset intent");
    let before = std::fs::read(&config_path).expect("source bytes");
    let registry = ReplaySessionRegistry::new();
    assert!(seal(&registry, begin(&registry, "thread")));
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _guard = registry.inner.lock().expect("isolated replay lock");
            panic!("isolated replay poison");
        }))
        .is_err()
    );
    let pause = pause_admission(&store).expect("pause");
    assert_eq!(
        reset_quiescent(&store, &config_path, &intent, &pause, &registry),
        Err(PermissionResetError::Unavailable)
    );
    let pending = permission_reset::load(store.conn())
        .expect("pending journal")
        .expect("journal retained");
    assert!(!pending.completed);
    assert_ne!(pending.authority_epoch, intent.authority_epoch);
    assert!(
        vega_store::permission_contracts::load_state(store.conn())
            .expect("state")
            .reset_pending
    );
    assert_eq!(
        std::fs::read(&config_path).expect("source not published"),
        before
    );
    assert_eq!(
        recover_pending_with_pause(&store, &config_path, &pause, &registry),
        Err(PermissionResetError::Unavailable)
    );
    assert_eq!(
        permission_reset::load(store.conn()).expect("same pending journal"),
        Some(pending)
    );
    assert_eq!(
        std::fs::read(&config_path).expect("source retained on retry"),
        before
    );
    assert!(
        pause
            .verifies(store.database_identity())
            .expect("pause held")
    );
    std::fs::remove_dir_all(root).expect("remove isolated fixture");
}

#[test]
fn exhausted_replay_generations_never_reuse_an_invalidated_lease_identity() {
    let registry = ReplaySessionRegistry::new();
    registry
        .inner
        .lock()
        .expect("fixture generation")
        .next_generation = u64::MAX - 1;
    let old = begin(&registry, "thread");
    registry
        .invalidate_all_acknowledged()
        .expect("invalidate final generation");
    let exhausted = begin(&registry, "thread");
    assert!(exhausted.candidate().is_none());
    assert!(!seal(&registry, old));
    assert!(!seal(&registry, exhausted));
    assert!(begin(&registry, "thread").candidate().is_none());
}
