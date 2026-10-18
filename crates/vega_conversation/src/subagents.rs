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
        if let Ok(mut live) = LIVE.get_or_init(Default::default).lock() {
            if let Some(count) = live.get_mut(&self.0) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    live.remove(&self.0);
                }
            }
        }
    }
}

pub fn children(store: &Store, parent: &str) -> Result<Vec<SubagentSnapshot>, ConversationError> {
    let projection = crate::privacy::DisplayProjection::load(store, parent)?;
    vega_store::subagents::list(store.conn(), parent, None)
        .map_err(|error| ConversationError::Store(error.to_string()))?
        .into_iter()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Store, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("state.db")).unwrap();
        store.migrate().unwrap();
        store.conn().execute_batch("INSERT INTO threads(id,model,created_at,updated_at) VALUES('p','m',1,1),('a','m',1,1),('b','m',1,1); INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('old','p',1,'assistant','old','done',1),('new','p',2,'assistant','new','done',2),('child-a','a',1,'assistant','partial','streaming',1),('child-b','b',1,'assistant','partial','streaming',1);").unwrap();
        for (id, parent_run) in [("a", "old"), ("b", "new")] {
            vega_store::subagents::insert(
                store.conn(),
                &vega_store::subagents::DelegationRow {
                    child_thread_id: id.into(),
                    parent_thread_id: "p".into(),
                    parent_message_id: parent_run.into(),
                    spawn_call_id: id.into(),
                    child_message_id: format!("child-{id}"),
                    name: id.into(),
                    task: "task".into(),
                    state: "running".into(),
                    result: String::new(),
                    truncated: false,
                    created_at: 1,
                },
            )
            .unwrap();
        }
        (store, temp)
    }

    #[test]
    fn m15_m18_m19_restart_recovers_dead_children_and_history_groups_follow_owner_pages() {
        let (store, _temp) = fixture();
        let live = LiveDelegation::new(store.database_path().unwrap(), "b").unwrap();
        let newest = crate::history::restart_history_page(&store, "p", 1).unwrap();
        assert_eq!(newest.subagents.len(), 1);
        assert_eq!(newest.subagents[0].parent_message_id, "new");
        assert_eq!(newest.subagents[0].state, SubagentState::Running);
        assert_eq!(
            vega_store::subagents::find(store.conn(), "a")
                .unwrap()
                .unwrap()
                .state,
            "interrupted"
        );
        assert_eq!(
            vega_store::messages::find(store.conn(), "child-b")
                .unwrap()
                .unwrap()
                .status,
            "streaming"
        );
        let older = crate::history::history_page_before(
            &store,
            "p",
            vega_store::messages::PageCursor::Before(newest.older_cursor.unwrap()),
            1,
        )
        .unwrap();
        assert_eq!(older.subagents.len(), 1);
        assert_eq!(older.subagents[0].parent_message_id, "old");
        drop(live);
        let reopened = Store::open(store.database_path().unwrap()).unwrap();
        crate::history::restart_history_page(&reopened, "p", 20).unwrap();
        assert!(
            children(&reopened, "p")
                .unwrap()
                .iter()
                .all(|agent| agent.state == SubagentState::Interrupted)
        );
    }

    #[test]
    fn m14_m15_deleting_parent_preserves_children_and_guard_covers_later_child_run() {
        let (store, _temp) = fixture();
        let live = LiveDelegation::new(store.database_path().unwrap(), "a").unwrap();
        assert!(crate::threads::delete_thread(&store, "p").is_err());
        drop(live);
        crate::threads::delete_thread(&store, "p").unwrap();
        assert!(
            vega_store::threads::find(store.conn(), "p")
                .unwrap()
                .is_none()
        );
        assert!(
            vega_store::threads::find(store.conn(), "a")
                .unwrap()
                .unwrap()
                .parent_thread_id
                .is_none()
        );
        assert!(
            vega_store::messages::find(store.conn(), "child-a")
                .unwrap()
                .is_some()
        );
        assert!(
            crate::threads::list_standalone_threads(&store, None)
                .unwrap()
                .iter()
                .any(|thread| thread.id == "a")
        );
    }

    #[test]
    fn m15_standalone_child_inherits_parent_actual_scratch_root() {
        let (store, _temp) = fixture();
        let root = crate::threads::task_workspace_root(&store, "p").unwrap();
        std::fs::write(root.join("shared.txt"), "preserved workspace").unwrap();
        assert_eq!(
            root,
            crate::threads::task_workspace_root(&store, "a").unwrap()
        );
        crate::threads::delete_thread(&store, "p").unwrap();
        let reopened = Store::open(store.database_path().unwrap()).unwrap();
        assert_eq!(
            root,
            crate::threads::task_workspace_root(&reopened, "a").unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(root.join("shared.txt")).unwrap(),
            "preserved workspace"
        );
    }
}
