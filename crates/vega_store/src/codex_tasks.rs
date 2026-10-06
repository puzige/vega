use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexTaskSnapshotRow {
    pub thread_id: String,
    pub profile_id: String,
    pub profile_display_name: String,
    pub adapter_kind: String,
    pub executable: String,
    pub arguments_json: String,
    pub adapter_version: String,
    pub codex_version: String,
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub reasoning_effort: Option<String>,
    pub sandbox_mode: String,
    pub approval_policy: String,
    pub selected_project_id: Option<String>,
    pub worktree_id: Option<String>,
    pub canonical_working_directory: String,
    pub additional_directories_json: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCodexTaskSnapshot {
    pub profile_id: String,
    pub profile_display_name: String,
    pub adapter_kind: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub adapter_version: String,
    pub codex_version: String,
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub reasoning_effort: Option<String>,
    pub sandbox_mode: String,
    pub approval_policy: String,
    pub selected_project_id: Option<String>,
    pub worktree_id: Option<String>,
    pub canonical_working_directory: String,
    pub additional_directories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexSessionCreationRow {
    pub state: String,
    pub intent_id: Option<String>,
    pub session_id: Option<String>,
    pub uncertainty_code: Option<String>,
    pub failure_code: Option<String>,
    pub updated_at: i64,
}

const SNAPSHOT_COLUMNS: &str = "thread_id, profile_id, profile_display_name, adapter_kind, executable, \
                                arguments_json, adapter_version, codex_version, model, model_provider, \
                                reasoning_effort, sandbox_mode, approval_policy, selected_project_id, \
                                worktree_id, canonical_working_directory, additional_directories_json, created_at";

pub fn bind_to_materialized_thread(
    conn: &Connection,
    thread_id: &str,
    snapshot: &NewCodexTaskSnapshot,
    created_at: i64,
) -> Result<bool, rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    let existing: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT backend, project_id FROM threads WHERE id = ?1",
            [thread_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((backend, project_id)) = existing else {
        return Ok(false);
    };
    if backend != "native" || project_id != snapshot.selected_project_id {
        return Ok(false);
    }
    if tx.execute(
        "UPDATE threads SET backend = 'codex' WHERE id = ?1 AND backend = 'native'",
        [thread_id],
    )? != 1
    {
        return Ok(false);
    }
    let arguments_json = serde_json::to_string(&snapshot.arguments)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    let additional_directories_json = serde_json::to_string(&snapshot.additional_directories)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    tx.execute(
        "INSERT INTO codex_task_snapshots \
         (thread_id, profile_id, profile_display_name, adapter_kind, executable, arguments_json, \
          adapter_version, codex_version, model, model_provider, reasoning_effort, sandbox_mode, \
          approval_policy, selected_project_id, worktree_id, canonical_working_directory, \
          additional_directories_json, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            thread_id,
            snapshot.profile_id,
            snapshot.profile_display_name,
            snapshot.adapter_kind,
            snapshot.executable,
            arguments_json,
            snapshot.adapter_version,
            snapshot.codex_version,
            snapshot.model,
            snapshot.model_provider,
            snapshot.reasoning_effort,
            snapshot.sandbox_mode,
            snapshot.approval_policy,
            snapshot.selected_project_id,
            snapshot.worktree_id,
            snapshot.canonical_working_directory,
            additional_directories_json,
            created_at,
        ],
    )?;
    tx.commit()?;
    Ok(true)
}

pub fn find_task_snapshot(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<CodexTaskSnapshotRow>, rusqlite::Error> {
    conn.query_row(
        &format!("SELECT {SNAPSHOT_COLUMNS} FROM codex_task_snapshots WHERE thread_id = ?1"),
        [thread_id],
        snapshot_from_row,
    )
    .optional()
}

pub fn find_session_creation(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<CodexSessionCreationRow>, rusqlite::Error> {
    conn.query_row(
        "SELECT state, intent_id, session_id, uncertainty_code, failure_code, updated_at \
         FROM codex_session_creations WHERE thread_id = ?1",
        [thread_id],
        |row| {
            Ok(CodexSessionCreationRow {
                state: row.get(0)?,
                intent_id: row.get(1)?,
                session_id: row.get(2)?,
                uncertainty_code: row.get(3)?,
                failure_code: row.get(4)?,
                updated_at: row.get(5)?,
            })
        },
    )
    .optional()
}

pub fn begin_session_creation(
    conn: &Connection,
    thread_id: &str,
    intent_id: &str,
    updated_at: i64,
) -> Result<bool, rusqlite::Error> {
    Ok(conn.execute(
        "UPDATE codex_session_creations SET state = 'intent', intent_id = ?1, updated_at = ?2 \
         WHERE thread_id = ?3 AND state = 'absent'",
        params![intent_id, updated_at, thread_id],
    )? == 1)
}

pub fn confirm_session_creation(
    conn: &Connection,
    thread_id: &str,
    intent_id: &str,
    session_id: &str,
    updated_at: i64,
) -> Result<bool, rusqlite::Error> {
    Ok(conn.execute(
        "UPDATE codex_session_creations SET state = 'confirmed', session_id = ?1, updated_at = ?2 \
         WHERE thread_id = ?3 AND state = 'intent' AND intent_id = ?4",
        params![session_id, updated_at, thread_id, intent_id],
    )? == 1)
}

pub fn mark_session_uncertain(
    conn: &Connection,
    thread_id: &str,
    intent_id: &str,
    code: &str,
    updated_at: i64,
) -> Result<bool, rusqlite::Error> {
    Ok(conn.execute(
        "UPDATE codex_session_creations SET state = 'uncertain', uncertainty_code = ?1, updated_at = ?2 \
         WHERE thread_id = ?3 AND state = 'intent' AND intent_id = ?4",
        params![code, updated_at, thread_id, intent_id],
    )? == 1)
}

pub fn mark_session_definitively_failed(
    conn: &Connection,
    thread_id: &str,
    intent_id: &str,
    code: &str,
    updated_at: i64,
) -> Result<bool, rusqlite::Error> {
    Ok(conn.execute(
        "UPDATE codex_session_creations SET state = 'definitively_failed', failure_code = ?1, updated_at = ?2 \
         WHERE thread_id = ?3 AND state = 'intent' AND intent_id = ?4",
        params![code, updated_at, thread_id, intent_id],
    )? == 1)
}

pub fn find_prompt_eligible_session(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT session_id FROM codex_session_creations \
         WHERE thread_id = ?1 AND state = 'confirmed' AND session_id IS NOT NULL AND length(session_id) > 0",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
}

fn snapshot_from_row(row: &rusqlite::Row) -> Result<CodexTaskSnapshotRow, rusqlite::Error> {
    Ok(CodexTaskSnapshotRow {
        thread_id: row.get(0)?,
        profile_id: row.get(1)?,
        profile_display_name: row.get(2)?,
        adapter_kind: row.get(3)?,
        executable: row.get(4)?,
        arguments_json: row.get(5)?,
        adapter_version: row.get(6)?,
        codex_version: row.get(7)?,
        model: row.get(8)?,
        model_provider: row.get(9)?,
        reasoning_effort: row.get(10)?,
        sandbox_mode: row.get(11)?,
        approval_policy: row.get(12)?,
        selected_project_id: row.get(13)?,
        worktree_id: row.get(14)?,
        canonical_working_directory: row.get(15)?,
        additional_directories_json: row.get(16)?,
        created_at: row.get(17)?,
    })
}
