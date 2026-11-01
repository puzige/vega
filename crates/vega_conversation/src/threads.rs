use vega_store::Store;
use vega_store::context_compaction;
use vega_store::messages as store_messages;
use vega_store::projects as store_projects;
use vega_store::threads as store;
use vega_store::token_usage;

use crate::types::{
    ConversationError, CurrentProject, Microcents, PermissionMode, RestoredUsage, TaskBackend,
    Thread, ThreadAdmission, ThreadMode, ThreadStatus, ThreadUpdate,
};

#[derive(Debug, thiserror::Error)]
pub enum ForkThreadError {
    #[error(transparent)]
    Refused(#[from] vega_store::thread_fork::ForkError),
    #[error("fork history validation failed: {0}")]
    History(ConversationError),
}

pub fn load_conversation(
    database: &std::path::Path,
    thread_id: &str,
    reasoning_path: &std::path::Path,
) -> Result<crate::types::ConversationLoad, ConversationError> {
    let store = Store::open_existing(database).map_err(store_error)?;
    let reasoning_preferences = match get_thread(&store, thread_id)?.reasoning_preferences {
        Some(preferences) => preferences,
        None => {
            let (defaults, _) =
                vega_store::reasoning::read_authority_from(reasoning_path).map_err(store_error)?;
            crate::session_reasoning::initialize(&store, thread_id, &defaults.profiles)?
        }
    };
    let history = crate::display_history::restart_history_page(
        &store,
        thread_id,
        vega_store::messages::PAGE_LIMIT,
    )?;
    Ok(crate::types::ConversationLoad {
        reasoning_preferences,
        history,
        plans: crate::plans::list_plans(&store, thread_id)?,
        composer_history: composer_history(&store, thread_id)?,
        approved_not_started: crate::plans::recoverable_approved_instruction(&store, thread_id)?
            .is_some(),
        usage: thread_usage_seed(&store, thread_id)?,
        summary: crate::summary::latest_task_summary(&store, thread_id, None)?,
        pending_model: vega_store::threads::pending_model(store.conn(), thread_id)
            .map_err(store_error)?,
    })
}

pub fn fork_thread(
    store: &Store,
    source_thread_id: &str,
    through_message_id: &str,
) -> Result<Thread, ForkThreadError> {
    use vega_store::thread_fork::ForkError;
    let tx = store.immediate_transaction().map_err(ForkError::from)?;
    let row =
        vega_store::thread_fork::clone_prefix(&tx, source_thread_id, through_message_id, now_ms())?;
    let source =
        context_compaction::load_source_in_transaction(&tx, &row.id).map_err(ForkError::from)?;
    crate::agent::history_from_context_source(&source, "", None)
        .map_err(|error| ForkError::CorruptHistory(error.to_string()))?;
    let images = vega_store::image_attachments::for_messages(&tx, &row.id, &source.messages)
        .map_err(ForkError::from)?;
    for message in &source.messages {
        let attachments = images
            .iter()
            .filter(|image| image.message_id == message.id)
            .map(|image| crate::types::ImageAttachment::from_bytes(image.encoded.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| ForkError::CorruptHistory(error.to_string()))?;
        crate::attachments::validate_images(&attachments)
            .map_err(|error| ForkError::CorruptHistory(error.to_string()))?;
    }
    let thread = thread_from_row(&row).map_err(ForkThreadError::History)?;
    tx.commit().map_err(ForkError::from)?;
    Ok(thread)
}

pub fn new_thread_id() -> String {
    // ulid 3.x：构造函数为 generate()（自带当前时间戳 + 随机数）。
    ulid::Ulid::generate().to_string()
}

/// Unix-milliseconds timestamp for `created_at`/`updated_at`.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// Wraps any store-layer failure display into [`ConversationError::Store`].
///
/// Generic over the error type so this crate stays free of a direct
/// `rusqlite` dependency (the SQL types stop at `vega_store`).
fn store_error<E: std::fmt::Display>(error: E) -> ConversationError {
    ConversationError::Store(error.to_string())
}

pub fn thread_usage_seed(
    store: &Store,
    thread_id: &str,
) -> Result<RestoredUsage, ConversationError> {
    let aggregate =
        token_usage::aggregate_task_by_thread(store.conn(), thread_id).map_err(store_error)?;
    let tokens = aggregate
        .input_tokens
        .checked_add(aggregate.output_tokens)
        .ok_or_else(|| ConversationError::Store("token usage aggregate overflow".to_string()))?;
    let unknown_summary_usage = store::find(store.conn(), thread_id)
        .map_err(store_error)?
        .filter(|thread| !thread.model.trim().is_empty())
        .map(|_| context_compaction::has_unknown_usage_for_thread(store.conn(), thread_id))
        .transpose()
        .map_err(store_error)?
        .unwrap_or(false);
    let unknown_delegated_usage =
        vega_store::subagents::has_unknown_usage(store.conn(), thread_id).map_err(store_error)?;
    Ok(RestoredUsage {
        tokens,
        cost: if unknown_summary_usage || unknown_delegated_usage {
            None
        } else {
            match aggregate.cost {
                token_usage::AggregateCost::Priced(cost) => Some(Microcents(cost)),
                token_usage::AggregateCost::Unavailable => None,
            }
        },
    })
}

/// Returns durable user submissions for Composer Up-history in sequence
/// order. The synthetic approval instruction is a controller capability, not
/// text the user typed, so it is deliberately excluded.
pub fn composer_history(store: &Store, thread_id: &str) -> Result<Vec<String>, ConversationError> {
    let projection = crate::privacy::DisplayProjection::load(store, thread_id)?;
    const HISTORY_WINDOW: usize = 200;
    let rows =
        store_messages::recent(store.conn(), thread_id, HISTORY_WINDOW).map_err(store_error)?;
    Ok(rows
        .into_iter()
        .filter(|row| row.role == "user" && row.kind == "text" && row.status == "done")
        .filter(|row| row.content != crate::plans::APPROVAL_INSTRUCTION)
        .map(|row| projection.text(&row.content))
        .collect())
}

pub fn current_project(store: &Store) -> Result<Option<CurrentProject>, ConversationError> {
    let row = store_projects::latest_project(store.conn()).map_err(store_error)?;
    Ok(row.map(|row| CurrentProject {
        id: row.id,
        name: row.name,
    }))
}

pub fn create_thread(
    store: &Store,
    project_id: &str,
    model: &str,
    permission_mode: &str,
) -> Result<Thread, ConversationError> {
    // 显式守卫：无项目时给出类型化错误，而不是裸外键失败。
    let exists = store_projects::project_exists(store.conn(), project_id).map_err(store_error)?;
    if !exists {
        return Err(ConversationError::NoProject);
    }
    create_thread_with_binding(store, Some(project_id), model, permission_mode)
}

/// Creates a persistent task without a project binding.
///
/// The row uses a real SQL `NULL`, so standalone sessions never become a
/// visible or shared synthetic project. Their workspace is resolved lazily by
/// [`task_workspace_root`] when a run starts.
pub fn create_standalone_thread(
    store: &Store,
    model: &str,
    permission_mode: &str,
) -> Result<Thread, ConversationError> {
    create_thread_with_binding(store, None, model, permission_mode)
}

fn create_thread_with_binding(
    store: &Store,
    project_id: Option<&str>,
    model: &str,
    permission_mode: &str,
) -> Result<Thread, ConversationError> {
    let thread = assemble_thread(project_id, model, parse_permission_mode(permission_mode)?);
    materialize_draft(store, &thread)
}

/// Strict config-string → [`PermissionMode`] bridge for the durable
/// constructor: an empty value is the DDL default `confirm`, anything outside
/// the vocabulary fails closed.
fn parse_permission_mode(value: &str) -> Result<PermissionMode, ConversationError> {
    if value.is_empty() {
        // DDL 默认（config 缺失模板同值：confirm）。
        return Ok(PermissionMode::Confirm);
    }
    PermissionMode::parse(value)
        .ok_or_else(|| ConversationError::CorruptRow(format!("permission_mode: {value}")))
}

fn assemble_thread(
    project_id: Option<&str>,
    model: &str,
    permission_mode: PermissionMode,
) -> Thread {
    let now = now_ms();
    Thread {
        reasoning_preferences: None,
        parent_thread_id: None,
        forked_from_thread_id: None,
        forked_from_message_id: None,
        id: new_thread_id(),
        backend: TaskBackend::Native,
        project_id: project_id.unwrap_or_default().to_string(),
        title: String::new(),
        mode: ThreadMode::Execute,
        permission_mode,
        permission_revision: 0,
        draft_authority_epoch: None,
        full_access_grant: None,
        default_permission: None,
        model: model.to_string(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: now,
        updated_at: now,
    }
}

fn insert_thread(
    transaction: &vega_store::Transaction<'_>,
    project_id: Option<&str>,
    thread: &Thread,
) -> Result<(), ConversationError> {
    if let Some(preferences) = &thread.reasoning_preferences {
        for preference in preferences {
            preference.validate().map_err(store_error)?;
        }
    }
    let grant = thread
        .full_access_grant
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(store_error)?;
    let reasoning = thread
        .reasoning_preferences
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(store_error)?;
    let values = store::NewThread {
        reasoning_preferences: reasoning.as_deref(),
        id: &thread.id,
        project_id: &thread.project_id,
        title: &thread.title,
        mode: thread.mode.as_str(),
        permission_mode: thread.permission_mode.as_str(),
        permission_revision: thread.permission_revision,
        full_access_grant: grant.as_deref(),
        model: &thread.model,
        status: thread.status.as_str(),
        pinned: thread.pinned,
        unread: thread.unread,
        created_at: thread.created_at,
        updated_at: thread.updated_at,
    };
    if project_id.is_some() {
        store::create(transaction, values).map_err(store_error)?;
    } else {
        store::create_standalone(transaction, values).map_err(store_error)?;
    }
    Ok(())
}

pub fn draft_thread(project_id: Option<&str>, model: &str, permission_mode: &str) -> Thread {
    let permission_mode = PermissionMode::parse(permission_mode).unwrap_or(PermissionMode::Confirm);
    assemble_thread(project_id, model, permission_mode)
}

pub fn draft_thread_from_defaults(
    project_id: Option<&str>,
    model: &str,
    permission: crate::types::DefaultPermissionSnapshot,
) -> Thread {
    let mut thread = draft_thread(project_id, model, &permission.permission_mode);
    thread.default_permission = Some(permission);
    thread
}

pub fn materialize_draft(store: &Store, thread: &Thread) -> Result<Thread, ConversationError> {
    let creation = crate::permission_contracts::DraftCreationGuard::begin(store, thread)?;
    let transaction = store.immediate_transaction().map_err(store_error)?;
    let thread = creation.materialize(&transaction)?;
    transaction.commit().map_err(store_error)?;
    Ok(thread)
}

pub(crate) fn materialize_draft_in_transaction(
    transaction: &vega_store::Transaction<'_>,
    thread: &Thread,
) -> Result<Thread, ConversationError> {
    let state = vega_store::permission_contracts::load_state(transaction).map_err(store_error)?;
    if state.reset_pending || thread.default_permission.is_some() {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let project_id = thread.project_binding();
    if let Some(project_id) = project_id {
        let exists =
            store_projects::project_exists(transaction, project_id).map_err(store_error)?;
        if !exists {
            return Err(ConversationError::NoProject);
        }
    }
    let now = now_ms();
    let materialized = Thread {
        created_at: now,
        updated_at: now,
        ..thread.clone()
    };
    insert_thread(transaction, project_id, &materialized)?;
    Ok(materialized)
}

pub fn list_threads(
    store: &Store,
    project_id: &str,
    status: Option<ThreadStatus>,
) -> Result<Vec<Thread>, ConversationError> {
    let rows = store::list_by_project(store.conn(), project_id, status.map(ThreadStatus::as_str))
        .map_err(store_error)?;
    rows.iter().map(thread_from_row).collect()
}

/// Lists standalone tasks without introducing a fake project scope.
pub fn list_standalone_threads(
    store: &Store,
    status: Option<ThreadStatus>,
) -> Result<Vec<Thread>, ConversationError> {
    let rows = store::list_standalone(store.conn(), status.map(ThreadStatus::as_str))
        .map_err(store_error)?;
    rows.iter().map(thread_from_row).collect()
}

/// Loads one thread by id; reports [`ConversationError::NotFound`] when the
/// row is missing.
pub fn get_thread(store: &Store, thread_id: &str) -> Result<Thread, ConversationError> {
    let row = store::find(store.conn(), thread_id)
        .map_err(crate::agent::runtime_store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.to_string()))?;
    thread_from_row(&row)
}

pub fn rename_thread(
    store: &Store,
    thread_id: &str,
    title: &str,
) -> Result<Thread, ConversationError> {
    let updated = store::rename(store.conn(), thread_id, title, now_ms()).map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    get_thread(store, thread_id)
}

pub fn set_thread_status(
    store: &Store,
    thread_id: &str,
    status: ThreadStatus,
) -> Result<(), ConversationError> {
    let _admission = crate::thread_access::acquire(store, thread_id)?;
    let tx = store.immediate_transaction().map_err(store_error)?;
    let updated = store::set_status(&tx, thread_id, status.as_str()).map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    if status == ThreadStatus::Archived {
        crate::automation::pause_in_transaction(&tx, thread_id, "thread_archived")
            .map_err(ConversationError::CorruptRow)?;
    }
    tx.commit().map_err(store_error)?;
    Ok(())
}

pub fn set_thread_pinned(
    store: &Store,
    thread_id: &str,
    pinned: bool,
) -> Result<(), ConversationError> {
    let updated = store::set_pinned(store.conn(), thread_id, pinned).map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    Ok(())
}

/// Persists a typed Ask/Plan/Execute selection. Execute is rejected while a
/// pending Plan exists; approval owns that transition.
pub fn set_thread_mode(
    store: &Store,
    thread_id: &str,
    mode: ThreadMode,
) -> Result<Thread, ConversationError> {
    let _admission = crate::thread_access::acquire(store, thread_id)?;
    let updated =
        store::set_mode(store.conn(), thread_id, mode.as_str(), now_ms()).map_err(store_error)?;
    if updated == 0 {
        let current = get_thread(store, thread_id)?;
        if mode == ThreadMode::Execute {
            let plans =
                store_messages::plans_for_thread(store.conn(), thread_id).map_err(store_error)?;
            if plans
                .iter()
                .any(|plan| plan.plan_status.as_deref() == Some("pending"))
            {
                return Err(ConversationError::PendingPlan);
            }
            if current.mode != ThreadMode::Execute {
                return Err(ConversationError::PendingPlan);
            }
        }
    }
    get_thread(store, thread_id)
}

/// Persists the typed permission mode independently from run mode.
pub fn set_thread_permission_mode(
    store: &Store,
    thread_id: &str,
    mode: PermissionMode,
) -> Result<Thread, ConversationError> {
    let thread = get_thread(store, thread_id)?;
    crate::permission_contracts::change_thread_permission(
        store,
        thread_id,
        thread.permission_revision,
        mode,
        now_ms(),
    )
}

pub fn cancel_context_precompute(
    store: &Store,
    thread_id: &str,
) -> Result<crate::types::ContextPrecomputeDrain, ConversationError> {
    crate::types::ContextPrecomputeRegistry::global()
        .invalidate(store.database_identity(), thread_id)
        .map_err(|error| ConversationError::Runtime(std::sync::Arc::new(error)))
}

fn pause_context_precompute(
    store: &Store,
    thread_id: &str,
) -> Result<crate::types::ContextPrecomputePause, ConversationError> {
    crate::types::ContextPrecomputeRegistry::global()
        .pause_thread(store.database_identity(), thread_id)
        .map_err(|error| ConversationError::Runtime(std::sync::Arc::new(error)))
}

pub fn set_thread_model(
    store: &Store,
    thread_id: &str,
    model: &str,
) -> Result<Thread, ConversationError> {
    let _admission = crate::thread_access::acquire(store, thread_id)?;
    if model.is_empty() {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    let _precompute = pause_context_precompute(store, thread_id)?;
    let updated =
        store::set_model(store.conn(), thread_id, model, now_ms()).map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    get_thread(store, thread_id)
}

pub fn stage_thread_model(
    store: &Store,
    thread_id: &str,
    model: &str,
) -> Result<Thread, ConversationError> {
    let _precompute = pause_context_precompute(store, thread_id)?;
    let updated =
        store::stage_model(store.conn(), thread_id, model, now_ms()).map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    let mut thread = get_thread(store, thread_id)?;
    thread.model = model.to_owned();
    Ok(thread)
}

pub fn promote_thread_model(
    store: &Store,
    thread_id: &str,
    model: &str,
    admission: &ThreadAdmission,
) -> Result<Thread, ConversationError> {
    crate::thread_access::verify(store, thread_id, admission)?;
    let _precompute = pause_context_precompute(store, thread_id)?;
    let updated = store::promote_pending_model(store.conn(), thread_id, model, now_ms())
        .map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    get_thread(store, thread_id)
}

pub fn delete_thread(store: &Store, thread_id: &str) -> Result<(), ConversationError> {
    let _admission = crate::thread_access::acquire(store, thread_id)?;
    let is_child = store::find(store.conn(), thread_id)
        .map_err(store_error)?
        .is_some_and(|thread| thread.parent_thread_id.is_some());
    let children =
        vega_store::subagents::list(store.conn(), thread_id, None).map_err(store_error)?;
    let _child_admissions = children
        .iter()
        .map(|child| crate::thread_access::acquire(store, &child.child_thread_id))
        .collect::<Result<Vec<_>, _>>()?;
    if (is_child && crate::subagents::is_live(store, thread_id))
        || children
            .iter()
            .any(|child| crate::subagents::is_live(store, &child.child_thread_id))
    {
        return Err(ConversationError::CorruptRow(
            "Stop the task and its running agents before deleting".into(),
        ));
    }

    let precompute = std::iter::once(thread_id)
        .chain(children.iter().map(|child| child.child_thread_id.as_str()))
        .map(|thread| pause_context_precompute(store, thread))
        .collect::<Result<Vec<_>, _>>()?;
    for pause in &precompute {
        if !pause
            .drain()
            .is_finished()
            .map_err(|error| ConversationError::Runtime(std::sync::Arc::new(error)))?
        {
            return Err(ConversationError::ContextCleanupPending);
        }
    }
    let deleted = store::delete_thread(store.conn(), thread_id).map_err(store_error)?;
    if deleted == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    Ok(())
}

/// Applies a partial update (title/status/pinned/unread) to one thread.
///
/// An all-`None` update is a no-op; updating a thread that does not exist
/// reports [`ConversationError::NotFound`].
pub fn update_thread(
    store: &Store,
    thread_id: &str,
    update: &ThreadUpdate,
) -> Result<(), ConversationError> {
    if update.is_empty() {
        return Ok(());
    }
    let _admission = update
        .status
        .map(|_| crate::thread_access::acquire(store, thread_id))
        .transpose()?;
    let tx = store.immediate_transaction().map_err(store_error)?;
    let updated = store::update(
        &tx,
        thread_id,
        update.title.as_deref(),
        update.status.map(ThreadStatus::as_str),
        update.pinned,
        update.unread,
    )
    .map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    if update.status == Some(ThreadStatus::Archived) {
        crate::automation::pause_in_transaction(&tx, thread_id, "thread_archived")
            .map_err(ConversationError::CorruptRow)?;
    }
    tx.commit().map_err(store_error)?;
    Ok(())
}

pub fn read_thread_title(
    store: &Store,
    thread_id: &str,
) -> Result<Option<String>, ConversationError> {
    Ok(store::find(store.conn(), thread_id)
        .map_err(store_error)?
        .map(|row| row.title))
}

/// Opens a thread: bumps task/project activity timestamps and returns it.
pub fn open_thread(store: &Store, thread_id: &str) -> Result<Thread, ConversationError> {
    let row = store::open_thread(store.conn(), thread_id, now_ms())
        .map_err(store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.to_string()))?;
    thread_from_row(&row)
}

/// Records a genuine user visit: touches task/project timestamps and clears unread
/// atomically. Metadata refreshes must continue to use `open_thread`.
pub fn visit_thread(store: &Store, thread_id: &str) -> Result<Thread, ConversationError> {
    let row = store::visit_thread(store.conn(), thread_id, now_ms())
        .map_err(store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.to_string()))?;
    thread_from_row(&row)
}

/// Resolves only an existing task's registered, existing project directory.
/// Call from a worker because directory validation performs filesystem IO.
pub fn task_project_path(
    store: &Store,
    thread_id: &str,
) -> Result<std::path::PathBuf, ConversationError> {
    let thread = get_thread(store, thread_id)?;
    let project = store_projects::find(store.conn(), &thread.project_id)
        .map_err(|error| match error {
            store_projects::ProjectsError::Sql(error) => crate::agent::runtime_store_error(error),
            store_projects::ProjectsError::PathAlreadyRegistered(_) => {
                ConversationError::InvalidTaskIdentity
            }
        })?
        .ok_or(ConversationError::NoProject)?;
    let path = std::path::PathBuf::from(project.path);
    if path.as_os_str().is_empty() || !path.is_absolute() || !path.is_dir() {
        return Err(ConversationError::NoProject);
    }
    Ok(path)
}

/// Resolves the workspace root for a task. Project tasks use the registered
/// folder; standalone tasks receive an isolated scratch directory scoped by
/// their thread id.
pub fn task_workspace_root(
    store: &Store,
    thread_id: &str,
) -> Result<std::path::PathBuf, ConversationError> {
    // A file-backed store already lives below Application Support/ai.vega in
    // production and below an owned temp root in tests. Deriving scratch from
    // it prevents a temp database test from ever writing the user's real data
    // directory. In-memory stores retain the platform fallback for callers
    // that cannot provide a durable database root.
    let data_root = store
        .database_path()
        .and_then(std::path::Path::parent)
        .map(std::path::Path::to_path_buf)
        .or_else(vega_store::paths::data_dir)
        .ok_or(ConversationError::NoProject)?;
    task_workspace_root_at(store, thread_id, &data_root)
}

/// Resolves a task workspace against an explicit app-data root. Production
/// callers use [`task_workspace_root`]; the explicit root keeps migration and
/// restart tests isolated from the user's real Application Support tree.
fn task_workspace_root_at(
    store: &Store,
    thread_id: &str,
    data_root: &std::path::Path,
) -> Result<std::path::PathBuf, ConversationError> {
    let thread = get_thread(store, thread_id)?;
    if let Some(parent) = &thread.parent_thread_id {
        let parent = get_thread(store, parent)?;
        if parent.parent_thread_id.is_some() {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        return task_workspace_root_at(store, &parent.id, data_root);
    }
    if thread.project_id.is_empty() {
        let row = store::find(store.conn(), thread_id)
            .map_err(crate::agent::runtime_store_error)?
            .ok_or(ConversationError::InvalidTaskIdentity)?;
        let owner = row
            .workspace_owner_thread_id
            .as_deref()
            .unwrap_or(&thread.id);
        let mut components = std::path::Path::new(owner).components();
        if !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        let root = data_root.join("scratch").join(owner);
        std::fs::create_dir_all(&root).map_err(crate::agent::runtime_store_error)?;
        return Ok(root);
    }
    task_project_path(store, thread_id)
}

/// Converts a raw store row into the shared [`Thread`], validating the
/// `mode`/`status` vocabulary.
pub(crate) fn thread_from_row(row: &store::ThreadRow) -> Result<Thread, ConversationError> {
    let mode = ThreadMode::parse(&row.mode)
        .ok_or_else(|| ConversationError::CorruptRow(format!("mode: {}", row.mode)))?;
    let status = ThreadStatus::parse(&row.status)
        .ok_or_else(|| ConversationError::CorruptRow(format!("status: {}", row.status)))?;
    let permission_mode = PermissionMode::parse(&row.permission_mode).ok_or_else(|| {
        ConversationError::CorruptRow(format!("permission_mode: {}", row.permission_mode))
    })?;
    Ok(Thread {
        reasoning_preferences: row
            .reasoning_preferences
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(store_error)?,
        parent_thread_id: row.parent_thread_id.clone(),
        forked_from_thread_id: row.forked_from_thread_id.clone(),
        forked_from_message_id: row.forked_from_message_id.clone(),
        id: row.id.clone(),
        backend: TaskBackend::parse(&row.backend)
            .ok_or_else(|| ConversationError::CorruptRow("backend".to_string()))?,
        project_id: row.project_id.clone(),
        title: row.title.clone(),
        mode,
        permission_mode,
        permission_revision: row.permission_revision,
        draft_authority_epoch: None,
        full_access_grant: None,
        default_permission: None,
        model: row.model.clone(),
        status,
        pinned: row.pinned,
        unread: row.unread,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}
