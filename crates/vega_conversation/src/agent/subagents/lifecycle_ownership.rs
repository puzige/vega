use super::*;

pub(super) struct LifecycleOwnership {
    database: PathBuf,
    snapshot: SubagentSnapshot,
    previous: Option<String>,
    owner: Option<Arc<DelegatedOwnership>>,
    registry: Registry,
    cancel: CancellationToken,
    armed: bool,
}

impl LifecycleOwnership {
    pub(super) fn new(
        database: PathBuf,
        snapshot: SubagentSnapshot,
        previous: Option<String>,
        owner: Arc<DelegatedOwnership>,
        registry: Registry,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            database,
            snapshot,
            previous,
            owner: Some(owner),
            registry,
            cancel,
            armed: false,
        }
    }

    pub(super) fn arm(&mut self) {
        self.armed = true;
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    pub(super) fn disarm_unstarted(&mut self) -> bool {
        let absent = Store::open(&self.database).ok().is_some_and(|store| {
            vega_store::subagents::execution(
                store.conn(),
                &self.snapshot.agent_id,
                &self.snapshot.message_id,
            )
            .is_ok_and(|row| row.is_none())
        });
        if absent {
            self.disarm();
        }
        absent
    }

    pub(super) fn quarantine(&mut self, reason: &str) {
        self.cancel.cancel();
        if let Some(owner) = self.owner.take() {
            owner.quarantine();
        }
        if let Ok(store) = Store::open(&self.database) {
            let _ = store.conn().busy_timeout(Duration::ZERO);
            let message = vega_store::subagents::execution(
                store.conn(),
                &self.snapshot.agent_id,
                &self.snapshot.message_id,
            )
            .ok()
            .flatten()
            .map(|_| self.snapshot.message_id.as_str())
            .or(self.previous.as_deref());
            if let Some(message) = message {
                let _ = vega_store::delegated_stages::set_release(
                    store.conn(),
                    &self.snapshot.agent_id,
                    message,
                    "quarantined",
                );
            }
        }
        self.registry.quarantine(self.snapshot.clone(), reason);
        self.armed = false;
    }
}

impl Drop for LifecycleOwnership {
    fn drop(&mut self) {
        if self.armed {
            self.quarantine("Subagent lifecycle ended without confirmed drain and release; ownership remains occupied");
        }
    }
}

pub(super) fn unstarted_terminal(
    snapshot: &SubagentSnapshot,
    cancel: &CancellationToken,
) -> SubagentSnapshot {
    let mut terminal = snapshot.clone();
    terminal.state = if cancel.is_cancelled() {
        SubagentState::Interrupted
    } else {
        SubagentState::Failed
    };
    terminal.result = "Child execution did not start".into();
    terminal.truncated = false;
    terminal.ready_for_review = false;
    terminal
}
