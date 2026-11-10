use super::*;
use crate::codex_persistence::{CodexPersistence, CodexSource};
use crate::database_writer::{DatabaseCommand, NecessaryLease, Sealed};
use crate::permission_contracts::DraftCreationGuard;
use std::path::{Path, PathBuf};
use vega_store::thread_admission::ThreadAdmission;

pub struct CodexStartup {
    pub thread: Thread,
    pub snapshot: CodexExecutionSnapshot,
    pub intent: CodexSessionCreationState,
    pub persistence: CodexPersistence,
}

struct PreparedStartup {
    creation: DraftCreationGuard,
    owner: ThreadAdmission,
    snapshot: CodexExecutionSnapshot,
    thread_id: String,
    intent_id: CodexSessionIntentId,
    prepared_snapshot: store_codex_tasks::PreparedCodexSnapshot,
}

pub fn prepare_codex_startup(
    path: &Path,
    draft: Thread,
    snapshot: CodexExecutionSnapshot,
) -> Result<CodexStartup, String> {
    let store = Store::open_read_only(path).map_err(|error| error.to_string())?;
    let prepared = prepare(&store, draft, snapshot)?;
    install(path.to_path_buf(), &store, prepared)
}

fn prepare(
    store: &Store,
    mut draft: Thread,
    snapshot: CodexExecutionSnapshot,
) -> Result<PreparedStartup, String> {
    validate_codex_snapshot(&snapshot, true).map_err(|error| error.to_string())?;
    if draft.backend != TaskBackend::Codex
        || draft.project_binding() != snapshot.workspace.project_id.as_deref()
    {
        return Err("Codex draft source changed".into());
    }
    let owner =
        crate::thread_access::acquire(store, &draft.id).map_err(|error| error.to_string())?;
    let thread_id = draft.id.clone();
    draft.backend = TaskBackend::Native;
    let creation = DraftCreationGuard::begin(store, &draft).map_err(|error| error.to_string())?;
    let record = new_store_snapshot(&snapshot);
    let prepared_snapshot =
        store_codex_tasks::prepare_snapshot(&record).map_err(|error| error.to_string())?;
    Ok(PreparedStartup {
        creation,
        owner,
        snapshot,
        thread_id,
        intent_id: CodexSessionIntentId::generate(),
        prepared_snapshot,
    })
}

fn install(
    path: PathBuf,
    store: &Store,
    prepared: PreparedStartup,
) -> Result<CodexStartup, String> {
    let database = store.database_identity();
    let abandonment = NecessaryLease::reserve(&path)?;
    let fallback = AbandonedStartup {
        thread_id: prepared.thread_id.clone(),
        snapshot: prepared.snapshot.clone(),
        intent_id: prepared.intent_id.clone(),
        owner: prepared.owner.clone(),
        external_started: false,
    };
    let result = crate::database_writer::commit(&path, database, prepared);
    match result {
        Ok(Ok((thread, source))) => {
            let snapshot = source.task.snapshot.clone();
            let intent = source.task.session_creation.clone();
            Ok(CodexStartup {
                thread,
                snapshot,
                intent,
                persistence: CodexPersistence::from_startup(path, database, source, abandonment),
            })
        }
        result => {
            if let Err(error) = abandonment.submit(database, fallback) {
                tracing::error!(error = %error, "Codex startup reconciliation not queued");
            }
            match result {
                Ok(Err(error)) | Err(error) => Err(error),
                Ok(Ok(_)) => Err("Codex startup receipt invalid".into()),
            }
        }
    }
}

impl Sealed for PreparedStartup {}
impl DatabaseCommand for PreparedStartup {
    type Output = Result<(Thread, CodexSource), String>;
    fn apply(self, store: &Store) -> Self::Output {
        let _permission =
            crate::permission_contracts::coordinate().map_err(|error| error.to_string())?;
        crate::thread_access::verify(store, &self.thread_id, &self.owner)
            .map_err(|error| error.to_string())?;
        crate::permission_contracts::require_admission_open(store)
            .map_err(|error| error.to_string())?;
        validate_codex_snapshot(&self.snapshot, false).map_err(|error| error.to_string())?;
        let tx = store
            .immediate_transaction()
            .map_err(|error| error.to_string())?;
        let mut thread = self
            .creation
            .materialize(&tx)
            .map_err(|error| error.to_string())?;
        if thread.id != self.thread_id
            || thread.project_binding() != self.snapshot.workspace.project_id.as_deref()
        {
            return Err("Codex draft identity changed before install".into());
        }
        if !store_codex_tasks::bind_prepared_in_transaction(
            &tx,
            &thread.id,
            &self.prepared_snapshot,
            now_ms(),
        )
        .map_err(|error| error.to_string())?
        {
            return Err("Codex backend transition rejected".into());
        }
        if !store_codex_tasks::begin_session_creation(
            &tx,
            &thread.id,
            self.intent_id.as_str(),
            now_ms(),
        )
        .map_err(|error| error.to_string())?
        {
            return Err("Codex initial session source changed".into());
        }
        thread.backend = TaskBackend::Codex;
        let task = codex_task_identity(store, &thread.id)
            .map_err(|error| error.to_string())?
            .ok_or("Codex task missing after install")?;
        let authority =
            vega_store::permission_contracts::load_state(&tx).map_err(|error| error.to_string())?;
        let permission = vega_store::permission_contracts::load_thread(&tx, &thread.id)
            .map_err(|error| error.to_string())?
            .ok_or("Codex permission source missing")?;
        let source = CodexSource {
            task,
            owner: self.owner,
            authority,
            permission,
            message_id: None,
        };
        tx.commit().map_err(|error| {
            format!("Codex startup commit outcome requires reconciliation: {error}")
        })?;
        Ok((thread, source))
    }
}

pub(crate) struct AbandonedStartup {
    pub(crate) thread_id: String,
    pub(crate) snapshot: CodexExecutionSnapshot,
    pub(crate) intent_id: CodexSessionIntentId,
    pub(crate) owner: ThreadAdmission,
    pub(crate) external_started: bool,
}
impl Sealed for AbandonedStartup {}
impl DatabaseCommand for AbandonedStartup {
    type Output = Result<(), String>;
    fn apply(self, store: &Store) -> Self::Output {
        let _permission =
            crate::permission_contracts::coordinate().map_err(|error| error.to_string())?;
        crate::thread_access::verify(store, &self.thread_id, &self.owner)
            .map_err(|error| error.to_string())?;
        let tx = store
            .immediate_transaction()
            .map_err(|error| error.to_string())?;
        if vega_store::threads::find(&tx, &self.thread_id)
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Ok(());
        }
        let task = codex_task_identity(store, &self.thread_id)
            .map_err(|error| error.to_string())?
            .ok_or("Codex abandoned startup source missing")?;
        if task.snapshot != self.snapshot {
            return Err("Codex abandoned startup source changed".into());
        }
        match task.session_creation {
            CodexSessionCreationState::Intent { intent_id } if intent_id == self.intent_id => {
                let changed = if self.external_started {
                    store_codex_tasks::mark_session_uncertain(
                        &tx,
                        &self.thread_id,
                        intent_id.as_str(),
                        CodexSessionUncertaintyCode::OutcomeUnknown.as_str(),
                        now_ms(),
                    )
                } else {
                    store_codex_tasks::mark_session_definitively_failed(
                        &tx,
                        &self.thread_id,
                        intent_id.as_str(),
                        CodexSessionFailureCode::ProcessStartFailed.as_str(),
                        now_ms(),
                    )
                }
                .map_err(|error| error.to_string())?;
                if !changed {
                    return Err("Codex abandoned startup not settled".into());
                }
            }
            CodexSessionCreationState::Confirmed { intent_id, .. }
            | CodexSessionCreationState::Uncertain { intent_id, .. }
            | CodexSessionCreationState::DefinitivelyFailed { intent_id, .. }
                if intent_id == self.intent_id => {}
            _ => return Err("Codex abandoned startup intent changed".into()),
        }
        tx.commit().map_err(|error| error.to_string())
    }
}
