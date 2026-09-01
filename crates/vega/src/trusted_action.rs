use std::sync::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Commit is the frozen T34 seam sharing this coordinator.
pub(crate) enum TrustedActionKind {
    BranchSwitch,
    ArtifactOpen,
    Commit,
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

    #[cfg(test)]
    pub(crate) fn active_token(&self) -> Option<TrustedActionToken> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active
    }
}
