use std::sync::{Mutex, MutexGuard};

use vega_store::Store;

use crate::types::{
    ConversationError, DefaultPermissionChange, DefaultPermissionError, DefaultPermissionSnapshot,
    DefaultPermissionValues, PermissionMode, TaskBackend, Thread, ThreadStatus,
};

mod intents;

static COORDINATION: Mutex<()> = Mutex::new(());
static DEFAULT_OPERATION: Mutex<()> = Mutex::new(());

pub(crate) fn coordinate() -> Result<MutexGuard<'static, ()>, ConversationError> {
    COORDINATION
        .lock()
        .map_err(|_| ConversationError::InvalidTaskIdentity)
}

pub(crate) fn require_admission_open(store: &Store) -> Result<(), ConversationError> {
    if vega_store::thread_admission::is_database_paused(store.database_identity()).map_err(
        |_| {
            ConversationError::Runtime(std::sync::Arc::new(
                vega_runtime::VegaError::RuntimeFailure(vega_runtime::RuntimeFailureDetail::new(
                    vega_runtime::RuntimeFailureReason::Admission,
                )),
            ))
        },
    )? {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    Ok(())
}

pub fn default_permission_snapshot(
    config_path: Option<&std::path::Path>,
    defaults: &vega_store::config::Defaults,
) -> DefaultPermissionSnapshot {
    DefaultPermissionSnapshot {
        config_path: config_path.map(std::path::Path::to_path_buf),
        configuration_id: config_path.and_then(|path| configuration_identity(path).ok()),
        permission_mode: defaults.permission_mode.clone(),
        permission_revision: defaults.permission_revision,
        full_access_grant: None,
    }
}

fn configuration_identity(path: &std::path::Path) -> Result<String, ConversationError> {
    let path = path.canonicalize().map_err(storage_error)?;
    if !path.is_file() {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(ConversationError::InvalidTaskIdentity)
}

pub(crate) struct DraftCreationGuard {
    draft: Thread,
    operation: String,
    epoch: String,
    generation: String,
    source: Option<String>,
}

impl DraftCreationGuard {
    pub(crate) fn begin(store: &Store, draft: &Thread) -> Result<Self, ConversationError> {
        Self::under_coordination(store, draft, coordinate()?)
    }

    fn under_coordination(
        store: &Store,
        draft: &Thread,
        coordination: MutexGuard<'static, ()>,
    ) -> Result<Self, ConversationError> {
        require_admission_open(store)?;
        if draft.backend != TaskBackend::Native
            || draft.status != ThreadStatus::Active
            || draft.parent_thread_id.is_some()
            || draft.forked_from_thread_id.is_some()
            || draft.forked_from_message_id.is_some()
            || !(0..i64::MAX).contains(&draft.permission_revision)
            || ulid::Ulid::from_string(&draft.id).is_err()
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let config = if let Some(snapshot) = &draft.default_permission {
            if draft.permission_revision != 0 {
                return Err(ConversationError::InvalidTaskIdentity);
            }
            let path = snapshot
                .config_path
                .as_deref()
                .ok_or(ConversationError::InvalidTaskIdentity)?;
            let edit = vega_store::config::begin_edit(path).map_err(|error| match error {
                vega_store::config::ConfigError::Io(error) => storage_error(error),
                _ => ConversationError::CorruptRow("permission configuration invalid".into()),
            })?;
            if snapshot.configuration_id.as_deref() != Some(configuration_identity(path)?.as_str())
                || snapshot.permission_revision < 0
                || snapshot.permission_mode != draft.permission_mode.as_str()
                || edit.config.defaults.permission_mode != snapshot.permission_mode
                || edit.config.defaults.permission_revision != snapshot.permission_revision
            {
                return Err(ConversationError::InvalidTaskIdentity);
            }
            Some(edit)
        } else {
            None
        };
        let source = draft
            .default_permission
            .as_ref()
            .map(intents::source_key)
            .transpose()?;
        drop(config);
        drop(coordination);
        let operation = ulid::Ulid::generate().to_string();
        let path = store
            .database_path()
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        let (epoch, generation) = crate::database_writer::commit(
            path,
            store.database_identity(),
            intents::PrepareDraft {
                draft: draft.clone(),
                operation: operation.clone(),
                source: source.clone(),
            },
        )
        .map_err(ConversationError::Store)??;
        if let Some(snapshot) = &draft.default_permission {
            let path = snapshot
                .config_path
                .as_deref()
                .ok_or(ConversationError::InvalidTaskIdentity)?;
            let current = vega_store::config::read_from(path)
                .map_err(|_| ConversationError::InvalidTaskIdentity)?;
            if default_permission_snapshot(Some(path), &current.defaults) != *snapshot {
                return Err(ConversationError::InvalidTaskIdentity);
            }
        }
        Ok(Self {
            draft: draft.clone(),
            operation,
            epoch,
            generation,
            source,
        })
    }

    pub(crate) fn materialize(
        &self,
        transaction: &vega_store::Transaction<'_>,
    ) -> Result<Thread, ConversationError> {
        let state =
            vega_store::permission_contracts::load_state(transaction).map_err(storage_error)?;
        let matches: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM draft_creation_intents d JOIN default_permission_intent c ON c.singleton=1 WHERE d.operation_id=?1 AND d.thread_id=?2 AND d.authority_epoch=?3 AND d.configuration_generation=?4 AND d.source_json IS ?5 AND d.state='prepared' AND c.generation=d.configuration_generation AND c.pending_id IS NULL)",
            (&self.operation, &self.draft.id, &self.epoch, &self.generation, &self.source), |row| row.get(0)).map_err(storage_error)?;
        if state.reset_pending || state.authority_epoch != self.epoch || !matches {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let mut thread = self.draft.clone();
        thread.draft_authority_epoch = None;
        thread.full_access_grant = None;
        thread.default_permission = None;
        let thread = crate::threads::materialize_draft_in_transaction(transaction, &thread)?;
        transaction.execute("UPDATE draft_creation_intents SET state='committed' WHERE operation_id=?1 AND state='prepared'", [&self.operation]).map_err(storage_error)?;
        Ok(thread)
    }
}

pub fn change_thread_permission(
    store: &Store,
    thread_id: &str,
    expected_revision: i64,
    mode: PermissionMode,
    issued_at: i64,
) -> Result<Thread, ConversationError> {
    crate::database_writer::commit(
        store
            .database_path()
            .ok_or(ConversationError::InvalidTaskIdentity)?,
        store.database_identity(),
        ChangeThreadPermission {
            thread_id: thread_id.to_owned(),
            expected_revision,
            mode,
            issued_at,
        },
    )
    .map_err(|_| ConversationError::InvalidTaskIdentity)?
}

struct ChangeThreadPermission {
    thread_id: String,
    expected_revision: i64,
    mode: PermissionMode,
    issued_at: i64,
}

impl crate::database_writer::Sealed for ChangeThreadPermission {}

impl crate::database_writer::DatabaseCommand for ChangeThreadPermission {
    type Output = Result<Thread, ConversationError>;

    fn apply(self, store: &Store) -> Self::Output {
        let thread_id = self.thread_id.as_str();
        let expected_revision = self.expected_revision;
        let mode = self.mode;
        let issued_at = self.issued_at;
        let _coordination = coordinate()?;
        require_admission_open(store)?;
        let _admission = crate::thread_access::acquire(store, thread_id)?;
        let transaction = store.immediate_transaction().map_err(storage_error)?;
        let state =
            vega_store::permission_contracts::load_state(&transaction).map_err(storage_error)?;
        if state.reset_pending {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        if !vega_store::permission_contracts::compare_exchange_thread(
            &transaction,
            thread_id,
            expected_revision,
            &state.authority_epoch,
            mode.as_str(),
            None,
            issued_at,
        )
        .map_err(storage_error)?
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        transaction.commit().map_err(storage_error)?;
        crate::threads::get_thread(store, thread_id)
    }
}

#[derive(Clone)]
pub struct DefaultPermissionService {
    database_path: std::path::PathBuf,
    config_path: std::path::PathBuf,
}

impl DefaultPermissionService {
    pub fn new(database_path: std::path::PathBuf, config_path: std::path::PathBuf) -> Self {
        Self {
            database_path,
            config_path,
        }
    }

    pub fn read(&self) -> Result<DefaultPermissionValues, DefaultPermissionError> {
        let change = self.capture(PermissionMode::Confirm)?;
        Ok(DefaultPermissionValues {
            permission_mode: change.source.permission_mode,
            permission_revision: change.source.permission_revision,
            full_access_grant: change.source.full_access_grant,
        })
    }

    pub fn prepare(
        &self,
        expected: &DefaultPermissionValues,
        mode: PermissionMode,
    ) -> Result<DefaultPermissionChange, DefaultPermissionError> {
        let change = self.capture(mode)?;
        if change.source.permission_mode != expected.permission_mode
            || change.source.permission_revision != expected.permission_revision
        {
            return Err(DefaultPermissionError::Conflict);
        }
        Ok(change)
    }

    fn capture(
        &self,
        mode: PermissionMode,
    ) -> Result<DefaultPermissionChange, DefaultPermissionError> {
        let _operation = DEFAULT_OPERATION
            .lock()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        intents::reconcile_default(&self.database_path, &self.config_path)?;
        let _coordination = coordinate().map_err(|_| DefaultPermissionError::Unavailable)?;
        let identity = configuration_identity(&self.config_path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let path = std::path::Path::new(&identity);
        let store = Store::open_read_only(&self.database_path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        if vega_store::thread_admission::is_database_paused(store.database_identity())
            .map_err(|_| DefaultPermissionError::Unavailable)?
        {
            return Err(DefaultPermissionError::ResetPending);
        }
        let edit = vega_store::config::begin_edit(path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let transaction = store
            .conn()
            .unchecked_transaction()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let state = vega_store::permission_contracts::load_state(&transaction)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        if state.reset_pending {
            return Err(DefaultPermissionError::ResetPending);
        }
        if !(0..i64::MAX).contains(&edit.config.defaults.permission_revision) {
            return Err(DefaultPermissionError::Unavailable);
        }
        let source = default_permission_snapshot(Some(path), &edit.config.defaults);
        if source.configuration_id.as_deref() != Some(identity.as_str())
            || configuration_identity(&self.config_path).ok().as_deref() != Some(identity.as_str())
        {
            return Err(DefaultPermissionError::Conflict);
        }
        transaction
            .commit()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        Ok(DefaultPermissionChange {
            source,
            database_identity: store.database_identity().permission_fence_key(),
            authority_epoch: state.authority_epoch,
            mode,
        })
    }

    pub fn save(
        &self,
        change: &DefaultPermissionChange,
    ) -> Result<DefaultPermissionValues, DefaultPermissionError> {
        let _operation = DEFAULT_OPERATION
            .lock()
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        intents::reconcile_default(&self.database_path, &self.config_path)?;
        let _coordination = coordinate().map_err(|_| DefaultPermissionError::Unavailable)?;
        let identity = configuration_identity(&self.config_path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let path = std::path::Path::new(&identity);
        let store = Store::open_read_only(&self.database_path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        if vega_store::thread_admission::is_database_paused(store.database_identity())
            .map_err(|_| DefaultPermissionError::Unavailable)?
        {
            return Err(DefaultPermissionError::ResetPending);
        }
        if change.database_identity != store.database_identity().permission_fence_key()
            || !(0..i64::MAX).contains(&change.source.permission_revision)
        {
            return Err(DefaultPermissionError::Conflict);
        }
        let mut target = change.source.clone();
        target.permission_mode = change.mode.as_str().to_owned();
        target.permission_revision += 1;
        target.full_access_grant = None;
        let source_json =
            intents::source_key(&change.source).map_err(|_| DefaultPermissionError::Unavailable)?;
        let target_json =
            intents::source_key(&target).map_err(|_| DefaultPermissionError::Unavailable)?;
        drop(_coordination);
        let operation = intents::default_commit(
            &store,
            intents::DefaultIntent::Begin {
                operation: ulid::Ulid::generate().to_string(),
                source: source_json.clone(),
                target: target_json.clone(),
                epoch: change.authority_epoch.clone(),
            },
        )?;
        let coordination = coordinate().map_err(|_| DefaultPermissionError::Unavailable)?;
        let mut edit = vega_store::config::begin_edit(path)
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        let current = default_permission_snapshot(Some(path), &edit.config.defaults);
        let state = vega_store::permission_contracts::load_state(store.conn())
            .map_err(|_| DefaultPermissionError::Unavailable)?;
        if state.reset_pending
            || state.authority_epoch != change.authority_epoch
            || (current != change.source && current != target)
            || configuration_identity(&self.config_path).ok().as_deref() != Some(identity.as_str())
        {
            drop(edit);
            drop(coordination);
            let _ = intents::default_commit(
                &store,
                intents::DefaultIntent::Cancel {
                    operation,
                    source: source_json,
                    target: target_json,
                    epoch: change.authority_epoch.clone(),
                },
            );
            return Err(DefaultPermissionError::Conflict);
        }
        if current != target {
            edit.config.defaults.permission_mode = target.permission_mode.clone();
            edit.config.defaults.permission_revision = target.permission_revision;
            edit.config.defaults.full_access_grant = None;
            edit.save()
                .map_err(|_| DefaultPermissionError::OutcomeUnknown)?;
        }
        let saved = vega_store::config::read_from(path)
            .map_err(|_| DefaultPermissionError::OutcomeUnknown)?;
        if default_permission_snapshot(Some(path), &saved.defaults) != target {
            return Err(DefaultPermissionError::OutcomeUnknown);
        }
        let values = DefaultPermissionValues::from(&saved.defaults);
        drop(edit);
        drop(coordination);
        intents::default_commit(
            &store,
            intents::DefaultIntent::Finish {
                operation,
                source: source_json,
                target: target_json,
                epoch: change.authority_epoch.clone(),
            },
        )?;
        Ok(values)
    }
}

pub fn prepare_submission_permission(
    store: &Store,
    submission: &crate::types::OrdinarySubmission,
    worker: &crate::types::OrdinaryWorker,
    expected: &Thread,
    draft: Option<&Thread>,
) -> Result<Thread, ConversationError> {
    require_admission_open(store)?;
    if worker.attempt_id().submission() != submission.id()
        || !worker
            .admission()
            .verifies(store.database_identity(), &expected.id)
        || expected.id != submission.snapshot().thread_id
        || draft.is_some_and(|draft| draft != expected)
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let accepted = submission
        .accepted_permission_fence()
        .map_err(|_| ConversationError::InvalidTaskIdentity)?;
    let creation = match (draft, &accepted) {
        (Some(draft), None) => Some(DraftCreationGuard::begin(store, draft)?),
        _ => None,
    };
    let prepared_workspace = if creation.is_some() {
        crate::threads::prepare_draft_workspace(store, expected)?
    } else {
        Some(crate::threads::task_workspace_root(store, &expected.id)?)
    };
    let command = PrepareSubmissionPermission {
        submission: submission.clone(),
        worker: worker
            .permission_commit_worker()
            .map_err(|_| ConversationError::InvalidTaskIdentity)?,
        expected: expected.clone(),
        accepted,
        creation,
        prepared_workspace,
        fallback: crate::automatic_titles::fallback_title(submission.raw().text()),
    };
    let path = store
        .database_path()
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    crate::database_writer::commit(path, store.database_identity(), command)
        .map_err(ConversationError::Store)?
}

struct PrepareSubmissionPermission {
    submission: crate::types::OrdinarySubmission,
    worker: crate::ordinary_submissions::PermissionCommitWorker,
    expected: Thread,
    accepted: Option<crate::types::PermissionFence>,
    creation: Option<DraftCreationGuard>,
    prepared_workspace: Option<std::path::PathBuf>,
    fallback: String,
}
impl crate::database_writer::Sealed for PrepareSubmissionPermission {}
impl crate::database_writer::DatabaseCommand for PrepareSubmissionPermission {
    type Output = Result<Thread, ConversationError>;
    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            submission,
            worker,
            expected,
            accepted,
            creation,
            prepared_workspace,
            fallback,
        } = self;
        let _coordination = coordinate()?;
        require_admission_open(store)?;
        if !worker
            .admission()
            .verifies(store.database_identity(), &expected.id)
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let transaction = store.immediate_transaction().map_err(storage_error)?;
        let exists = vega_store::threads::find(&transaction, &expected.id)
            .map_err(storage_error)?
            .is_some();
        let created = match (exists, &creation, &accepted) {
            (false, Some(creation), None) => Some(creation.materialize(&transaction)?),
            (true, None, _) => None,
            _ => return Err(ConversationError::InvalidTaskIdentity),
        };
        if created.is_none() && accepted.is_none() {
            if let Some(pending) = vega_store::threads::pending_model(&transaction, &expected.id)
                .map_err(storage_error)?
            {
                if pending != submission.raw().model()
                    || vega_store::threads::promote_pending_model(
                        &transaction,
                        &expected.id,
                        &pending,
                        crate::threads::now_ms(),
                    )
                    .map_err(storage_error)?
                        != 1
                {
                    return Err(ConversationError::InvalidTaskIdentity);
                }
            }
        }
        let row = vega_store::threads::open_thread_in_transaction(
            &transaction,
            &expected.id,
            crate::threads::now_ms(),
        )
        .map_err(storage_error)?
        .ok_or(ConversationError::InvalidTaskIdentity)?;
        let mut thread = crate::threads::thread_from_row(&row)?;
        let fence = capture_fence_for_commit(store, &row)?;
        let expected_permission = created.as_ref().unwrap_or(&expected);
        if prepared_workspace
            .as_ref()
            .is_some_and(|workspace| workspace.to_str() != Some(fence.workspace.as_str()))
            || thread.model != submission.raw().model()
            || thread.backend != TaskBackend::Native
            || thread.status != ThreadStatus::Active
            || thread.project_id != expected.project_id
            || thread.mode != expected.mode
            || thread.permission_mode != expected.permission_mode
            || match &accepted {
                Some(accepted) => accepted != &fence,
                None => fence.permission_revision != expected_permission.permission_revision,
            }
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        if created.is_some() && thread.title.is_empty() {
            if !fallback.is_empty()
                && vega_store::threads::seed_auto_title(
                    &transaction,
                    &thread.id,
                    submission.user_message_id(),
                    &fallback,
                )
                .map_err(storage_error)?
            {
                thread.title = fallback;
            }
        }
        let pending = worker
            .stage_permission_fence(fence)
            .map_err(|_| ConversationError::InvalidTaskIdentity)?;
        match transaction.commit_with_rollback() {
            Ok(()) => pending.publish(),
            Err(vega_store::write_coordination::CommitFailure::RolledBack(error)) => {
                if created.is_some()
                    && !matches!(
                        vega_store::threads::find(store.conn(), &expected.id),
                        Ok(None)
                    )
                {
                    pending.mark_unknown();
                }
                return Err(storage_error(error));
            }
            Err(vega_store::write_coordination::CommitFailure::Unknown(error)) => {
                pending.mark_unknown();
                return Err(storage_error(error));
            }
        }
        Ok(thread)
    }
}

pub(crate) fn capture_fence(
    store: &Store,
    thread: &vega_store::threads::ThreadRow,
) -> Result<crate::types::PermissionFence, ConversationError> {
    let workspace = crate::threads::existing_task_workspace_root(store, &thread.id)?;
    capture_fence_at_workspace(store, thread, workspace)
}

pub(crate) fn capture_fence_for_commit(
    store: &Store,
    thread: &vega_store::threads::ThreadRow,
) -> Result<crate::types::PermissionFence, ConversationError> {
    let parent;
    let workspace_thread = if let Some(parent_id) = &thread.parent_thread_id {
        parent = vega_store::threads::find(store.conn(), parent_id)
            .map_err(storage_error)?
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        if parent.parent_thread_id.is_some() {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        &parent
    } else {
        thread
    };
    let workspace = if workspace_thread.project_id.is_empty() {
        let owner = workspace_thread
            .workspace_owner_thread_id
            .as_deref()
            .unwrap_or(&workspace_thread.id);
        let mut components = std::path::Path::new(owner).components();
        if !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        store
            .database_path()
            .and_then(std::path::Path::parent)
            .ok_or(ConversationError::InvalidTaskIdentity)?
            .join("scratch")
            .join(owner)
    } else {
        let project = vega_store::projects::find(store.conn(), &workspace_thread.project_id)
            .map_err(|_| ConversationError::InvalidTaskIdentity)?
            .ok_or(ConversationError::NoProject)?;
        let path = std::path::PathBuf::from(project.path);
        if path.as_os_str().is_empty() || !path.is_absolute() {
            return Err(ConversationError::NoProject);
        }
        path
    };
    capture_fence_at_workspace(store, thread, workspace)
}

fn capture_fence_at_workspace(
    store: &Store,
    thread: &vega_store::threads::ThreadRow,
    workspace: std::path::PathBuf,
) -> Result<crate::types::PermissionFence, ConversationError> {
    require_admission_open(store)?;
    let state =
        vega_store::permission_contracts::load_state(store.conn()).map_err(storage_error)?;
    if state.reset_pending {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let fence = crate::types::PermissionFence {
        schema_version: 1,
        execution_policy_version: 2,
        authority_epoch: state.authority_epoch,
        database_identity: store.database_identity().permission_fence_key(),
        thread_id: thread.id.clone(),
        project_id: thread.project_id.clone(),
        workspace: workspace
            .to_str()
            .ok_or(ConversationError::InvalidTaskIdentity)?
            .to_owned(),
        run_mode: thread.mode.clone(),
        permission_mode: thread.permission_mode.clone(),
        permission_revision: thread.permission_revision,
        delegation: None,
        exact_rules: if thread.project_id.is_empty() {
            Vec::new()
        } else {
            let mut rules: Vec<_> =
                vega_store::permissions::list_exact(store.conn(), &thread.project_id)
                    .map_err(|error| match error {
                        vega_store::permissions::PermissionsError::Store(error) => {
                            storage_error(error)
                        }
                        _ => ConversationError::InvalidTaskIdentity,
                    })?
                    .into_iter()
                    .map(|rule| (rule.tool, rule.pattern))
                    .collect();
            rules.sort();
            rules
        },
        full_access_grant: None,
    };
    if !fence.is_valid() {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    Ok(fence)
}

pub(crate) fn interrupted_authority_matches(
    conn: &vega_store::Connection,
    expected: &crate::types::InterruptedAssistant,
    current: &crate::types::PermissionFence,
) -> Result<bool, ConversationError> {
    if !current.is_valid()
        || current.delegation.is_some()
        || !vega_store::permission_reset::replay_allowed(conn, &current.thread_id, expected.seq)
            .map_err(storage_error)?
    {
        return Ok(false);
    }
    let Some(encoded) = vega_store::permission_contracts::load_interrupted_run_fence(
        conn,
        &expected.message_id,
        &current.thread_id,
        expected.seq,
    )
    .map_err(storage_error)?
    else {
        return Ok(false);
    };
    Ok(
        serde_json::from_str::<crate::types::PermissionFence>(&encoded)
            .is_ok_and(|saved| saved == *current),
    )
}

pub(crate) fn capture_delegated_fence(
    store: &Store,
    thread: &vega_store::threads::ThreadRow,
    parent_thread: &str,
    parent_message: &str,
    child_message: &str,
    accepted: &crate::types::PermissionFence,
) -> Result<crate::types::PermissionFence, ConversationError> {
    capture_delegated_source(
        store,
        thread,
        parent_thread,
        parent_message,
        child_message,
        accepted,
        false,
    )
}

pub(crate) fn capture_delegated_fence_for_commit(
    store: &Store,
    thread: &vega_store::threads::ThreadRow,
    parent_thread: &str,
    parent_message: &str,
    child_message: &str,
    accepted: &crate::types::PermissionFence,
) -> Result<crate::types::PermissionFence, ConversationError> {
    capture_delegated_source(
        store,
        thread,
        parent_thread,
        parent_message,
        child_message,
        accepted,
        true,
    )
}

fn capture_delegated_source(
    store: &Store,
    thread: &vega_store::threads::ThreadRow,
    parent_thread: &str,
    parent_message: &str,
    child_message: &str,
    accepted: &crate::types::PermissionFence,
    committing: bool,
) -> Result<crate::types::PermissionFence, ConversationError> {
    require_admission_open(store)?;
    let state =
        vega_store::permission_contracts::load_state(store.conn()).map_err(storage_error)?;
    let encoded = vega_store::permission_contracts::load_delegating_run_fence(
        store.conn(),
        parent_message,
        parent_thread,
    )
    .map_err(storage_error)?
    .ok_or(ConversationError::InvalidTaskIdentity)?;
    let saved: crate::types::PermissionFence =
        serde_json::from_str(&encoded).map_err(|_| ConversationError::InvalidTaskIdentity)?;
    let parent = vega_store::threads::find(store.conn(), parent_thread)
        .map_err(storage_error)?
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    let parent_fence = if committing {
        capture_fence_for_commit(store, &parent)?
    } else {
        capture_fence(store, &parent)?
    };
    let child_workspace = if committing {
        capture_fence_for_commit(store, thread)?.workspace
    } else {
        crate::threads::existing_task_workspace_root(store, &thread.id)?
            .to_str()
            .ok_or(ConversationError::InvalidTaskIdentity)?
            .to_owned()
    };
    if state.reset_pending
        || parent.backend != "native"
        || parent.status != "active"
        || parent.parent_thread_id.is_some()
        || parent_fence != *accepted
        || &saved != accepted
        || !accepted.is_valid()
        || accepted.delegation.is_some()
        || accepted.thread_id != parent_thread
        || accepted.authority_epoch != state.authority_epoch
        || accepted.database_identity != store.database_identity().permission_fence_key()
        || thread.parent_thread_id.as_deref() != Some(parent_thread)
        || thread.project_id != accepted.project_id
        || child_workspace != accepted.workspace
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    if let Some((previous_message, encoded)) =
        vega_store::permission_contracts::previous_delegated_fence(
            store.conn(),
            &thread.id,
            child_message,
            parent_thread,
            parent_message,
        )
        .map_err(storage_error)?
    {
        let previous: crate::types::PermissionFence = serde_json::from_str(
            encoded
                .as_deref()
                .ok_or(ConversationError::InvalidTaskIdentity)?,
        )
        .map_err(|_| ConversationError::InvalidTaskIdentity)?;
        let source = previous
            .delegation
            .as_ref()
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        if !previous.is_valid()
            || previous.thread_id != thread.id
            || source.child_message_id != previous_message
            || source.parent_fence.as_ref() != accepted
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
    }
    let mut fence = accepted.clone();
    fence.thread_id = thread.id.clone();
    fence.permission_revision = thread.permission_revision;
    fence.full_access_grant = None;
    fence.delegation = Some(crate::types::DelegatedPermissionSource {
        parent_message_id: parent_message.to_owned(),
        child_message_id: child_message.to_owned(),
        parent_fence: Box::new(accepted.clone()),
    });
    if !fence.is_valid() {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    Ok(fence)
}

fn storage_error(error: impl Into<vega_runtime::VegaError>) -> ConversationError {
    ConversationError::Runtime(std::sync::Arc::new(error.into()))
}
