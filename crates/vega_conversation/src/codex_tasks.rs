use vega_store::Store;
use vega_store::codex_tasks as store_codex_tasks;
use vega_store::projects as store_projects;
use vega_store::threads as store;

use crate::types::{
    CodexAdapterArgument, CodexAdapterKind, CodexApprovalPolicy, CodexExecutionSnapshot,
    CodexPromptBinding, CodexReasoningEffort, CodexRunSettings, CodexSandboxMode,
    CodexSessionCreationState, CodexSessionFailureCode, CodexSessionIntentId,
    CodexSessionUncertaintyCode, CodexTaskIdentity, CodexWorkspaceSnapshot, ConversationError,
    TaskBackend, Thread,
};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn store_error<E: std::fmt::Display>(error: E) -> ConversationError {
    ConversationError::Store(error.to_string())
}

fn codex_identifier(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn codex_setting_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-'
        })
}

fn codex_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || byte == b'.'
                || byte == b'+'
                || byte == b'-'
                || byte == b'_'
        })
}

fn canonical_directory(value: &str, require_existing: bool) -> bool {
    let path = std::path::Path::new(value);
    if value.is_empty()
        || value.len() > 4096
        || value.chars().any(char::is_control)
        || !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return false;
    }
    if !require_existing {
        return true;
    }
    std::fs::canonicalize(path)
        .ok()
        .is_some_and(|canonical| canonical == path && canonical.is_dir())
}

fn validate_codex_snapshot(
    snapshot: &CodexExecutionSnapshot,
    require_existing_directories: bool,
) -> Result<(), ConversationError> {
    if !codex_identifier(&snapshot.profile.id, 128)
        || snapshot.profile.display_name.trim().is_empty()
        || snapshot.profile.display_name.len() > 128
        || snapshot.profile.display_name.chars().any(char::is_control)
        || snapshot.adapter != CodexAdapterKind::CodexAcp
        || snapshot.executable.trim().is_empty()
        || snapshot.executable.len() > 4096
        || snapshot.executable.chars().any(char::is_control)
        || !std::path::Path::new(&snapshot.executable).is_absolute()
        || snapshot.arguments.len() > 32
        || !codex_version(&snapshot.adapter_version)
        || !codex_version(&snapshot.codex_version)
        || snapshot
            .settings
            .model
            .as_deref()
            .is_some_and(|value| !codex_setting_identifier(value))
        || snapshot
            .settings
            .model_provider
            .as_deref()
            .is_some_and(|value| !codex_setting_identifier(value))
        || !canonical_directory(
            &snapshot.workspace.canonical_working_directory,
            require_existing_directories,
        )
        || snapshot.workspace.additional_directories.len() > 32
        || snapshot
            .workspace
            .project_id
            .as_deref()
            .is_some_and(|value| !codex_identifier(value, 128))
        || snapshot
            .workspace
            .worktree_id
            .as_deref()
            .is_some_and(|value| !codex_identifier(value, 128))
        || (snapshot.workspace.project_id.is_none() && snapshot.workspace.worktree_id.is_none())
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    for argument in &snapshot.arguments {
        CodexAdapterArgument::from_persisted(argument.persisted_tokens())?;
    }
    let mut directories = std::collections::HashSet::new();
    directories.insert(snapshot.workspace.canonical_working_directory.as_str());
    for directory in &snapshot.workspace.additional_directories {
        if !canonical_directory(directory, require_existing_directories)
            || !directories.insert(directory.as_str())
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
    }
    Ok(())
}

fn codex_session_intent(value: Option<String>) -> Result<CodexSessionIntentId, ConversationError> {
    value
        .as_deref()
        .and_then(CodexSessionIntentId::parse)
        .ok_or_else(|| ConversationError::CorruptRow("session intent".to_string()))
}

fn codex_snapshot_from_row(
    row: &store_codex_tasks::CodexTaskSnapshotRow,
) -> Result<CodexExecutionSnapshot, ConversationError> {
    let arguments = serde_json::from_str::<Vec<Vec<String>>>(&row.arguments_json)
        .map_err(|_| ConversationError::CorruptRow("Codex arguments".to_string()))?
        .into_iter()
        .map(CodexAdapterArgument::from_persisted)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ConversationError::CorruptRow("Codex arguments".to_string()))?;
    let additional_directories =
        serde_json::from_str::<Vec<String>>(&row.additional_directories_json)
            .map_err(|_| ConversationError::CorruptRow("Codex directories".to_string()))?;
    let snapshot = CodexExecutionSnapshot {
        profile: crate::types::CodexProfileReference {
            id: row.profile_id.clone(),
            display_name: row.profile_display_name.clone(),
        },
        adapter: CodexAdapterKind::parse(&row.adapter_kind)
            .ok_or_else(|| ConversationError::CorruptRow("adapter kind".to_string()))?,
        executable: row.executable.clone(),
        arguments,
        adapter_version: row.adapter_version.clone(),
        codex_version: row.codex_version.clone(),
        settings: CodexRunSettings {
            model: row.model.clone(),
            model_provider: row.model_provider.clone(),
            reasoning_effort: match row.reasoning_effort.as_deref() {
                Some(value) => Some(CodexReasoningEffort::parse(value).ok_or_else(|| {
                    ConversationError::CorruptRow("reasoning effort".to_string())
                })?),
                None => None,
            },
            sandbox_mode: CodexSandboxMode::parse(&row.sandbox_mode)
                .ok_or_else(|| ConversationError::CorruptRow("sandbox mode".to_string()))?,
            approval_policy: CodexApprovalPolicy::parse(&row.approval_policy)
                .ok_or_else(|| ConversationError::CorruptRow("approval policy".to_string()))?,
        },
        workspace: CodexWorkspaceSnapshot {
            project_id: row.selected_project_id.clone(),
            worktree_id: row.worktree_id.clone(),
            canonical_working_directory: row.canonical_working_directory.clone(),
            additional_directories,
        },
    };
    validate_codex_snapshot(&snapshot, false)?;
    Ok(snapshot)
}

fn new_store_snapshot(
    snapshot: &CodexExecutionSnapshot,
) -> store_codex_tasks::NewCodexTaskSnapshot {
    store_codex_tasks::NewCodexTaskSnapshot {
        profile_id: snapshot.profile.id.clone(),
        profile_display_name: snapshot.profile.display_name.clone(),
        adapter_kind: snapshot.adapter.as_str().to_string(),
        executable: snapshot.executable.clone(),
        arguments: snapshot
            .arguments
            .iter()
            .map(CodexAdapterArgument::persisted_tokens)
            .collect(),
        adapter_version: snapshot.adapter_version.clone(),
        codex_version: snapshot.codex_version.clone(),
        model: snapshot.settings.model.clone(),
        model_provider: snapshot.settings.model_provider.clone(),
        reasoning_effort: snapshot
            .settings
            .reasoning_effort
            .map(CodexReasoningEffort::as_str)
            .map(str::to_string),
        sandbox_mode: snapshot.settings.sandbox_mode.as_str().to_string(),
        approval_policy: snapshot.settings.approval_policy.as_str().to_string(),
        selected_project_id: snapshot.workspace.project_id.clone(),
        worktree_id: snapshot.workspace.worktree_id.clone(),
        canonical_working_directory: snapshot.workspace.canonical_working_directory.clone(),
        additional_directories: snapshot.workspace.additional_directories.clone(),
    }
}

fn codex_session_state_from_row(
    row: &store_codex_tasks::CodexSessionCreationRow,
) -> Result<CodexSessionCreationState, ConversationError> {
    let intent_id = || codex_session_intent(row.intent_id.clone());
    match row.state.as_str() {
        "absent"
            if row.intent_id.is_none()
                && row.session_id.is_none()
                && row.uncertainty_code.is_none()
                && row.failure_code.is_none() =>
        {
            Ok(CodexSessionCreationState::Absent)
        }
        "intent"
            if row.session_id.is_none()
                && row.uncertainty_code.is_none()
                && row.failure_code.is_none() =>
        {
            Ok(CodexSessionCreationState::Intent {
                intent_id: intent_id()?,
            })
        }
        "confirmed"
            if row.session_id.as_deref().is_some_and(|value| {
                !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
            }) && row.uncertainty_code.is_none()
                && row.failure_code.is_none() =>
        {
            Ok(CodexSessionCreationState::Confirmed {
                intent_id: intent_id()?,
                session_id: row
                    .session_id
                    .clone()
                    .ok_or_else(|| ConversationError::CorruptRow("session id".to_string()))?,
            })
        }
        "uncertain" if row.session_id.is_none() && row.failure_code.is_none() => {
            let code = row
                .uncertainty_code
                .as_deref()
                .and_then(CodexSessionUncertaintyCode::parse)
                .ok_or_else(|| ConversationError::CorruptRow("session uncertainty".to_string()))?;
            Ok(CodexSessionCreationState::Uncertain {
                intent_id: intent_id()?,
                code,
            })
        }
        "definitively_failed" if row.session_id.is_none() && row.uncertainty_code.is_none() => {
            let code = row
                .failure_code
                .as_deref()
                .and_then(CodexSessionFailureCode::parse)
                .ok_or_else(|| ConversationError::CorruptRow("session failure".to_string()))?;
            Ok(CodexSessionCreationState::DefinitivelyFailed {
                intent_id: intent_id()?,
                code,
            })
        }
        _ => Err(ConversationError::CorruptRow(
            "session creation state".to_string(),
        )),
    }
}

pub fn codex_task_identity(
    store: &Store,
    thread_id: &str,
) -> Result<Option<CodexTaskIdentity>, ConversationError> {
    let thread = store::find(store.conn(), thread_id)
        .map_err(store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.to_string()))?;
    let backend = TaskBackend::parse(&thread.backend)
        .ok_or_else(|| ConversationError::CorruptRow("backend".to_string()))?;
    let snapshot =
        store_codex_tasks::find_task_snapshot(store.conn(), thread_id).map_err(store_error)?;
    if backend == TaskBackend::Native {
        if snapshot.is_some() {
            return Err(ConversationError::CorruptRow(
                "native task has Codex snapshot".to_string(),
            ));
        }
        return Ok(None);
    }
    let snapshot = snapshot.ok_or_else(|| {
        ConversationError::CorruptRow("Codex task is missing its snapshot".to_string())
    })?;
    let snapshot = codex_snapshot_from_row(&snapshot)?;
    if (!thread.project_id.is_empty()).then_some(thread.project_id.as_str())
        != snapshot.workspace.project_id.as_deref()
    {
        return Err(ConversationError::CorruptRow(
            "Codex project binding".to_string(),
        ));
    }
    let session_creation = store_codex_tasks::find_session_creation(store.conn(), thread_id)
        .map_err(store_error)?
        .ok_or_else(|| ConversationError::CorruptRow("session creation row".to_string()))?;
    Ok(Some(CodexTaskIdentity {
        thread_id: thread_id.to_string(),
        backend,
        snapshot,
        session_creation: codex_session_state_from_row(&session_creation)?,
    }))
}

pub fn materialize_codex_draft(
    store: &Store,
    draft: &Thread,
    snapshot: &CodexExecutionSnapshot,
) -> Result<Thread, ConversationError> {
    validate_codex_snapshot(snapshot, true)?;
    let project_id = draft.project_binding();
    if draft.backend != TaskBackend::Codex || project_id != snapshot.workspace.project_id.as_deref()
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    if let Some(project_id) = project_id {
        let exists =
            store_projects::project_exists(store.conn(), project_id).map_err(store_error)?;
        if !exists {
            return Err(ConversationError::NoProject);
        }
    }
    let now = now_ms();
    let materialized = Thread {
        created_at: now,
        updated_at: now,
        ..draft.clone()
    };
    let values = store::NewThread {
        id: &materialized.id,
        project_id: &materialized.project_id,
        title: &materialized.title,
        mode: materialized.mode.as_str(),
        permission_mode: materialized.permission_mode.as_str(),
        model: &materialized.model,
        status: materialized.status.as_str(),
        pinned: materialized.pinned,
        unread: materialized.unread,
        created_at: materialized.created_at,
        updated_at: materialized.updated_at,
    };
    let snapshot = new_store_snapshot(snapshot);
    let created = store_codex_tasks::materialize_codex_thread(
        store.conn(),
        project_id,
        values,
        &snapshot,
        now,
    )
    .map_err(store_error)?;
    if !created {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    Ok(materialized)
}

pub fn bind_codex_task(
    store: &Store,
    thread_id: &str,
    snapshot: &CodexExecutionSnapshot,
) -> Result<CodexTaskIdentity, ConversationError> {
    validate_codex_snapshot(snapshot, true)?;
    let thread = store::find(store.conn(), thread_id)
        .map_err(store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.to_string()))?;
    if TaskBackend::parse(&thread.backend) != Some(TaskBackend::Native)
        || (!thread.project_id.is_empty()).then_some(thread.project_id.as_str())
            != snapshot.workspace.project_id.as_deref()
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let record = new_store_snapshot(snapshot);
    let created =
        store_codex_tasks::bind_to_materialized_thread(store.conn(), thread_id, &record, now_ms())
            .map_err(store_error)?;
    if !created {
        return Err(ConversationError::InvalidCodexSessionTransition);
    }
    codex_task_identity(store, thread_id)?
        .ok_or_else(|| ConversationError::CorruptRow("Codex task identity".to_string()))
}

pub fn begin_codex_session_creation(
    store: &Store,
    thread_id: &str,
) -> Result<CodexSessionCreationState, ConversationError> {
    let identity =
        codex_task_identity(store, thread_id)?.ok_or(ConversationError::InvalidTaskIdentity)?;
    if identity.session_creation != CodexSessionCreationState::Absent {
        return Err(ConversationError::InvalidCodexSessionTransition);
    }
    let intent_id = CodexSessionIntentId::generate();
    let changed = store_codex_tasks::begin_session_creation(
        store.conn(),
        thread_id,
        intent_id.as_str(),
        now_ms(),
    )
    .map_err(store_error)?;
    if !changed {
        return Err(ConversationError::InvalidCodexSessionTransition);
    }
    Ok(CodexSessionCreationState::Intent { intent_id })
}

pub fn confirm_codex_session_creation(
    store: &Store,
    thread_id: &str,
    intent_id: &CodexSessionIntentId,
    session_id: &str,
) -> Result<(), ConversationError> {
    if session_id.is_empty() || session_id.len() > 512 || session_id.chars().any(char::is_control) {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    let changed = store_codex_tasks::confirm_session_creation(
        store.conn(),
        thread_id,
        intent_id.as_str(),
        session_id,
        now_ms(),
    )
    .map_err(store_error)?;
    if !changed {
        return Err(ConversationError::InvalidCodexSessionTransition);
    }
    Ok(())
}

pub fn mark_codex_session_uncertain(
    store: &Store,
    thread_id: &str,
    intent_id: &CodexSessionIntentId,
    code: CodexSessionUncertaintyCode,
) -> Result<(), ConversationError> {
    let changed = store_codex_tasks::mark_session_uncertain(
        store.conn(),
        thread_id,
        intent_id.as_str(),
        code.as_str(),
        now_ms(),
    )
    .map_err(store_error)?;
    if !changed {
        return Err(ConversationError::InvalidCodexSessionTransition);
    }
    Ok(())
}

pub fn mark_codex_session_definitively_failed(
    store: &Store,
    thread_id: &str,
    intent_id: &CodexSessionIntentId,
    code: CodexSessionFailureCode,
) -> Result<(), ConversationError> {
    let changed = store_codex_tasks::mark_session_definitively_failed(
        store.conn(),
        thread_id,
        intent_id.as_str(),
        code.as_str(),
        now_ms(),
    )
    .map_err(store_error)?;
    if !changed {
        return Err(ConversationError::InvalidCodexSessionTransition);
    }
    Ok(())
}

pub fn codex_prompt_binding(
    store: &Store,
    thread_id: &str,
) -> Result<Option<CodexPromptBinding>, ConversationError> {
    let identity = codex_task_identity(store, thread_id)?;
    let Some(identity) = identity else {
        return Ok(None);
    };
    let Some(session_id) = store_codex_tasks::find_prompt_eligible_session(store.conn(), thread_id)
        .map_err(store_error)?
    else {
        return Ok(None);
    };
    Ok(Some(CodexPromptBinding {
        thread_id: identity.thread_id,
        session_id,
        snapshot: identity.snapshot,
    }))
}
