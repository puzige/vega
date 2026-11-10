use std::sync::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TrustedActionKind {
    BranchSwitch,
    ConversationFork,
    ArtifactOpen,
    Commit,
    ModelSelection,
    PermissionSelection,
    ThreadSettings,
    PlanReview,
    PermissionReset,
    DraftAutomationCreation,
    AgentPreflight,
    SkillSelection,
    /// Conversation context settings and manual summary operations own the
    /// same single-flight gate as sends and model changes until workers drain.
    ContextSettings,
    ContextCompaction,
    ShellRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrustedActionToken {
    pub(crate) generation: u64,
    pub(crate) kind: TrustedActionKind,
    pub(crate) owner_epoch: u64,
    pub(crate) request_sequence: u64,
}

#[derive(Default)]
pub(crate) struct TrustedActionState {
    pub(crate) next_generation: u64,
    pub(crate) active: Option<TrustedActionToken>,
}

#[derive(Clone, Default)]
pub(crate) struct TrustedActionCoordinator {
    pub(crate) state: Arc<Mutex<TrustedActionState>>,
}

impl TrustedActionCoordinator {
    pub(crate) fn acquire(
        &self,
        kind: TrustedActionKind,
        owner_epoch: u64,
        request_sequence: u64,
    ) -> Option<TrustedActionToken> {
        if crate::updater::installing() {
            return None;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active.is_some() {
            return None;
        }
        let generation = state.next_generation.checked_add(1)?;
        state.next_generation = generation;
        let token = TrustedActionToken {
            generation,
            kind,
            owner_epoch,
            request_sequence,
        };
        state.active = Some(token);
        Some(token)
    }

    pub(crate) fn release(&self, token: TrustedActionToken) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active != Some(token) {
            return false;
        }
        state.active = None;
        true
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active
            .is_some()
    }

    pub(crate) fn is_kind_active(&self, kind: TrustedActionKind) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active
            .is_some_and(|token| token.kind == kind)
    }

    pub(crate) fn cancel_agent_preparation(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active.is_some_and(|token| {
            matches!(
                token.kind,
                TrustedActionKind::AgentPreflight | TrustedActionKind::SkillSelection
            )
        }) {
            state.active = None;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_completion_cannot_release_a_new_action() {
        let coordinator = TrustedActionCoordinator::default();
        let first = coordinator
            .acquire(TrustedActionKind::ThreadSettings, 1, 1)
            .unwrap();
        assert!(coordinator.release(first));
        let second = coordinator
            .acquire(TrustedActionKind::PlanReview, 2, 2)
            .unwrap();
        assert!(!coordinator.release(first));
        assert!(coordinator.is_kind_active(TrustedActionKind::PlanReview));
        assert!(coordinator.release(second));
        assert!(!coordinator.is_busy());
    }

    #[test]
    fn preparation_cancellation_does_not_release_a_pending_commit() {
        let coordinator = TrustedActionCoordinator::default();
        let preflight = coordinator
            .acquire(TrustedActionKind::AgentPreflight, 1, 1)
            .unwrap();
        assert!(coordinator.cancel_agent_preparation());
        let commit = coordinator
            .acquire(TrustedActionKind::ThreadSettings, 1, 2)
            .unwrap();
        assert!(!coordinator.cancel_agent_preparation());
        assert!(!coordinator.release(preflight));
        assert!(coordinator.is_busy());
        assert!(
            coordinator
                .acquire(TrustedActionKind::PlanReview, 1, 3)
                .is_none()
        );
        assert!(coordinator.release(commit));
    }
}
