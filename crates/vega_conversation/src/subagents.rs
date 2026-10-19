use crate::types::{ConversationError, SubagentSnapshot, SubagentState};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use vega_store::Store;

type LiveIdentity = (std::path::PathBuf, String);
type LiveRuns = Mutex<HashMap<LiveIdentity, usize>>;
static LIVE: OnceLock<LiveRuns> = OnceLock::new();

pub(crate) struct LiveDelegation(LiveIdentity);

impl LiveDelegation {
    pub(crate) fn new(database: &std::path::Path, id: &str) -> Result<Self, ConversationError> {
        let mut live = LIVE
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| ConversationError::InvalidTaskIdentity)?;
        let key = (database.to_path_buf(), id.into());
        *live.entry(key.clone()).or_default() += 1;
        Ok(Self(key))
    }
}

impl Drop for LiveDelegation {
    fn drop(&mut self) {
        if let Ok(mut live) = LIVE.get_or_init(Default::default).lock()
            && let Some(count) = live.get_mut(&self.0)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                live.remove(&self.0);
            }
        }
    }
}

pub fn children(store: &Store, parent: &str) -> Result<Vec<SubagentSnapshot>, ConversationError> {
    project_children(
        store,
        parent,
        vega_store::subagents::list(store.conn(), parent, None)
            .map_err(|error| ConversationError::Store(error.to_string()))?,
    )
}

pub fn is_latest_execution(
    store: &Store,
    snapshot: &SubagentSnapshot,
) -> Result<bool, ConversationError> {
    Ok(
        vega_store::subagents::find(store.conn(), &snapshot.agent_id)
            .map_err(|error| ConversationError::Store(error.to_string()))?
            .is_some_and(|row| {
                row.child_message_id == snapshot.message_id
                    && row.parent_thread_id == snapshot.parent_thread_id
                    && row.parent_message_id == snapshot.parent_message_id
            }),
    )
}

pub(crate) fn history(
    store: &Store,
    parent: &str,
) -> Result<Vec<SubagentSnapshot>, ConversationError> {
    project_children(
        store,
        parent,
        vega_store::subagents::history(store.conn(), parent)
            .map_err(|error| ConversationError::Store(error.to_string()))?,
    )
}

fn project_children(
    store: &Store,
    parent: &str,
    rows: Vec<vega_store::subagents::DelegationRow>,
) -> Result<Vec<SubagentSnapshot>, ConversationError> {
    let projection = crate::privacy::DisplayProjection::load(store, parent)?;
    rows.into_iter()
        .map(|row| {
            Ok(SubagentSnapshot {
                agent_id: row.child_thread_id,
                parent_thread_id: row.parent_thread_id,
                parent_message_id: row.parent_message_id,
                message_id: row.child_message_id,
                name: projection.text(&row.name),
                state: SubagentState::parse(&row.state)
                    .ok_or(ConversationError::InvalidTaskIdentity)?,
                result: projection.text(&row.result),
                truncated: row.truncated,
            })
        })
        .collect()
}

pub(crate) fn recover_children(
    store: &Store,
    parent: &str,
    now: i64,
) -> Result<(), ConversationError> {
    let live = LIVE
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| ConversationError::InvalidTaskIdentity)?;
    for child in vega_store::subagents::list(store.conn(), parent, None)
        .map_err(|error| ConversationError::Store(error.to_string()))?
    {
        if matches!(child.state.as_str(), "running" | "cancelling")
            && !live.contains_key(&(
                store
                    .database_path()
                    .unwrap_or_else(|| std::path::Path::new(":memory:"))
                    .to_path_buf(),
                child.child_thread_id.clone(),
            ))
        {
            vega_store::recovery::recover_thread(store.conn(), &child.child_thread_id, now)
                .map_err(|error| ConversationError::Store(error.to_string()))?;
        }
    }
    Ok(())
}

pub fn related_thread(
    store: &Store,
    source: &str,
    target: &str,
) -> Result<crate::types::Thread, ConversationError> {
    let source = vega_store::threads::find(store.conn(), source)
        .map_err(|error| ConversationError::Store(error.to_string()))?
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    let target = vega_store::threads::find(store.conn(), target)
        .map_err(|error| ConversationError::Store(error.to_string()))?
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    if source.parent_thread_id.as_deref() != Some(&target.id)
        && target.parent_thread_id.as_deref() != Some(&source.id)
    {
        return Err(ConversationError::InvalidTaskIdentity);
    }
    crate::threads::thread_from_row(&target)
}

pub(crate) fn is_live(store: &Store, thread: &str) -> bool {
    LIVE.get_or_init(Default::default).lock().is_ok_and(|live| {
        live.contains_key(&(
            store
                .database_path()
                .unwrap_or_else(|| std::path::Path::new(":memory:"))
                .to_path_buf(),
            thread.into(),
        ))
    })
}
