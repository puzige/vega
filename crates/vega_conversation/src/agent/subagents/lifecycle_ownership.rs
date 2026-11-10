use super::*;

pub(super) struct LifecycleOwnership {
    database: PathBuf,
    snapshot: SubagentSnapshot,
    previous: Option<String>,
    owner: Option<Arc<DelegatedOwnership>>,
    admission: vega_store::thread_admission::ThreadAdmission,
    registry: Registry,
    cancel: CancellationToken,
    armed: bool,
    necessary_terminal: Option<crate::database_writer::NecessaryLease>,
    database_identity: vega_store::DatabaseIdentity,
}

impl LifecycleOwnership {
    pub(super) fn new(
        database: PathBuf,
        reserved: &mut ReservedExecution,
        registry: Registry,
    ) -> Self {
        Self {
            database,
            snapshot: reserved.snapshot.clone(),
            previous: reserved
                .previous
                .as_ref()
                .map(|previous| previous.message_id.clone()),
            owner: Some(reserved.owner.clone()),
            admission: reserved.admission.clone(),
            registry,
            cancel: reserved.cancel.clone(),
            armed: false,
            necessary_terminal: reserved.necessary_terminal.take(),
            database_identity: reserved.database_identity,
        }
    }

    pub(super) fn arm(&mut self) {
        self.armed = true;
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    pub(super) fn disarm_unstarted(&mut self) -> bool {
        let absent = Store::open_existing(&self.database)
            .ok()
            .is_some_and(|store| {
                if store
                    .verify_database_identity(self.database_identity)
                    .is_err()
                {
                    return false;
                }
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

    fn quarantine_runtime(&mut self, reason: &str) {
        static QUARANTINED: std::sync::OnceLock<
            Mutex<Vec<vega_store::thread_admission::ThreadAdmission>>,
        > = std::sync::OnceLock::new();
        let admission = self.admission.clone();
        match QUARANTINED.get_or_init(Default::default).lock() {
            Ok(mut admissions) => admissions.push(admission),
            Err(_) => std::mem::forget(admission),
        }
        self.cancel.cancel();
        if let Some(owner) = &self.owner {
            owner.clone().quarantine();
        }
        self.registry.quarantine(self.snapshot.clone(), reason);
        self.armed = false;
    }

    pub(super) fn quarantine(&mut self, reason: &str) {
        self.quarantine_runtime(reason);
        let submitted = match (self.necessary_terminal.take(), self.owner.take()) {
            (Some(lease), Some(owner)) => lease
                .submit(
                    self.database_identity,
                    QuarantineChild {
                        database: self.database.clone(),
                        child: self.snapshot.agent_id.clone(),
                        message: self.snapshot.message_id.clone(),
                        previous: self.previous.clone(),
                        owner,
                        admission: self.admission.clone(),
                    },
                )
                .map(drop),
            _ => Err("necessary terminal reservation unavailable".into()),
        };
        if let Err(error) = submitted {
            tracing::error!(outcome = "necessary_terminal_not_confirmed", %error);
        }
    }
}

impl Drop for LifecycleOwnership {
    fn drop(&mut self) {
        if self.armed {
            self.quarantine("Subagent lifecycle ended without confirmed drain and release; ownership remains occupied");
        }
    }
}

struct QuarantineChild {
    database: PathBuf,
    child: String,
    message: String,
    previous: Option<String>,
    owner: Arc<DelegatedOwnership>,
    admission: vega_store::thread_admission::ThreadAdmission,
}

impl crate::database_writer::Sealed for QuarantineChild {}
impl crate::database_writer::DatabaseCommand for QuarantineChild {
    type Output = Result<(), String>;

    fn apply(self, store: &Store) -> Self::Output {
        if !self.owner.verifies(&self.database, &self.child) {
            return Err("native writer identity changed".into());
        }
        crate::thread_access::verify(store, &self.child, &self.admission)
            .map_err(|_| "child admission identity changed".to_string())?;
        let tx = store
            .immediate_transaction()
            .map_err(|error| error.to_string())?;
        let message = if vega_store::subagents::execution(&tx, &self.child, &self.message)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            Some(self.message.as_str())
        } else {
            self.previous.as_deref()
        };
        let message = message.ok_or("quarantine execution unavailable")?;
        vega_store::delegated_stages::set_release(&tx, &self.child, message, "quarantined")
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())
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
