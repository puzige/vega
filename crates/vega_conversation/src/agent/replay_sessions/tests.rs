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
