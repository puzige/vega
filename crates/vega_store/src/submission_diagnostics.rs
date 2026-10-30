use rusqlite::{Connection, params};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationStage {
    Database,
    Admission,
    Permission,
    Workspace,
    Skills,
    Conversation,
    Preflight,
    Spawn,
    Commit,
    Launch,
    Worker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationReason {
    Database,
    Io,
    InvalidIdentity,
    MissingThread,
    MissingWorkspace,
    CorruptState,
    CleanupPending,
    PendingPlan,
    ProviderUnavailable,
    CredentialUnavailable,
    Capability,
    Reasoning,
    CredentialSafety,
    Context,
    References,
    SkillChanged,
    InvalidSkill,
    TooManyCandidates,
    PreviewRequired,
    SelectionLimit,
    Admission,
    State,
    Worker,
    Pipeline,
    ApprovalAudit,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PreparationFailure {
    pub stage: PreparationStage,
    pub reason: PreparationReason,
    pub sqlite_code: Option<i32>,
    pub sqlite_extended_code: Option<i32>,
    pub io_kind: Option<&'static str>,
}

impl PreparationFailure {
    pub fn new(stage: PreparationStage, reason: PreparationReason) -> Self {
        Self {
            stage,
            reason,
            sqlite_code: None,
            sqlite_extended_code: None,
            io_kind: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationState {
    Started,
    Prepared,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PreparationEvent {
    pub submission_id: String,
    pub attempt: u64,
    pub state: PreparationState,
    pub occurred_at: u64,
    pub failure: Option<PreparationFailure>,
}

pub fn append(conn: &Connection, event: &PreparationEvent) -> rusqlite::Result<()> {
    if ulid::Ulid::from_string(&event.submission_id).is_err()
        || event.attempt == 0
        || event.attempt > i64::MAX as u64
        || event.occurred_at > i64::MAX as u64
        || matches!(
            event.state,
            PreparationState::Started | PreparationState::Prepared
        ) == event.failure.is_some()
        || event.failure.is_some_and(|failure| {
            failure.io_kind.is_some_and(|kind| {
                !matches!(
                    kind,
                    "not_found"
                        | "permission_denied"
                        | "already_exists"
                        | "invalid_input"
                        | "invalid_data"
                        | "timed_out"
                        | "interrupted"
                        | "unsupported"
                        | "out_of_memory"
                        | "storage_full"
                        | "other"
                )
            })
        })
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "submission diagnostic".into(),
        ));
    }
    let payload = serde_json::to_string(event).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let terminal = event.state != PreparationState::Started;
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Diagnostics,
    )?;
    conn.execute(
        "INSERT INTO submission_diagnostic_events (submission_id, attempt, terminal, payload) VALUES (?1, ?2, ?3, ?4)",
        params![event.submission_id, event.attempt as i64, terminal, payload],
    )?;
    Ok(())
}

pub fn read(conn: &Connection, submission_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT payload FROM submission_diagnostic_events WHERE submission_id = ?1 ORDER BY attempt, terminal",
    )?;
    statement
        .query_map([submission_id], |row| row.get(0))?
        .collect()
}
