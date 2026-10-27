use super::*;
use crate::types::ThreadPermissionError as Error;
use std::path::PathBuf;

#[derive(Clone)]
pub struct AutomationPermissionService {
    database_path: PathBuf,
}

impl AutomationPermissionService {
    pub fn new(database_path: PathBuf) -> Self {
        Self { database_path }
    }

    pub fn read(&self, id: &str) -> Result<(Thread, Vec<AutomationTask>), Error> {
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let store = Store::open_read_only(&self.database_path).map_err(unavailable)?;
        require_open(&store)?;
        let thread = crate::threads::get_thread(&store, id).map_err(unavailable)?;
        let tasks = load_from(store.conn(), id).map_err(unavailable)?;
        for task in &tasks {
            require_task(store.conn(), task)?;
        }
        Ok((thread, tasks))
    }

    pub fn prepare(
        &self,
        source: &Thread,
        task: &AutomationTask,
        resume: bool,
    ) -> Result<AutomationPermissionChange, Error> {
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let store = Store::open_read_only(&self.database_path).map_err(unavailable)?;
        require_open(&store)?;
        let tx = store.conn().unchecked_transaction().map_err(unavailable)?;
        let row = vega_store::threads::find(&tx, &source.id)
            .map_err(unavailable)?
            .ok_or(Error::Conflict)?;
        let current = crate::threads::thread_from_row(&row).map_err(unavailable)?;
        if current.id != task.thread_id
            || current.project_id != source.project_id
            || current.permission_mode != PermissionMode::FullAccess
            || current.permission_revision != source.permission_revision
            || current.full_access_grant != source.full_access_grant
            || current.mode != ThreadMode::Execute
            || current.backend != TaskBackend::Native
            || current.status != ThreadStatus::Active
            || current.mode != source.mode
            || current.backend != source.backend
            || current.status != source.status
            || task.status.replaceable()
            || (resume && task.status == AutomationStatus::Active)
        {
            return Err(Error::Conflict);
        }
        require_task(&tx, task)?;
        let fence = crate::permission_contracts::capture_fence(&store, &row)
            .map_err(|_| Error::Conflict)?;
        tx.commit().map_err(unavailable)?;
        Ok(AutomationPermissionChange {
            database: store.database_identity(),
            authorization: AutomationPermissionAuthorization {
                kind: task.kind,
                identity: task.identity.clone(),
                revision: task.revision,
                fence,
            },
            task: task.clone(),
            resume,
        })
    }

    pub fn save(
        &self,
        change: &AutomationPermissionChange,
        confirmed: bool,
    ) -> Result<Vec<AutomationTask>, Error> {
        if !confirmed {
            return Err(Error::ConfirmationRequired);
        }
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        require_open(&store)?;
        if store.database_identity() != change.database {
            return Err(Error::Conflict);
        }
        let tx = store.immediate_transaction().map_err(unavailable)?;
        let row = vega_store::threads::find(&tx, &change.task.thread_id)
            .map_err(unavailable)?
            .ok_or(Error::Conflict)?;
        if row.status != "active"
            || row.mode != "execute"
            || row.backend != "native"
            || crate::permission_contracts::capture_fence(&store, &row)
                .map_err(|_| Error::Conflict)?
                != change.authorization.fence
        {
            return Err(Error::Conflict);
        }
        require_task(&tx, &change.task)?;
        let now = now_ms();
        let mut tasks = if change.resume {
            command_in_transaction(
                &tx,
                &change.task.thread_id,
                &AutomationCommand::Control(change.task.kind, AutomationControl::Resume),
                now,
            )
            .map_err(|_| Error::Conflict)?
        } else {
            load_from(&tx, &change.task.thread_id).map_err(unavailable)?
        };
        let task = tasks
            .iter_mut()
            .find(|task| task.kind == change.task.kind && task.identity == change.task.identity)
            .ok_or(Error::Conflict)?;
        let mut authorization = change.authorization.clone();
        authorization.revision = task.revision;
        task.permission_authorization = Some(authorization);
        task.updated_at_ms = now;
        save(&tx, task).map_err(unavailable)?;
        tx.commit_with_rollback()
            .map_err(|_| Error::OutcomeUnknown)?;
        Ok(tasks)
    }
}

fn require_open(store: &Store) -> Result<(), Error> {
    if vega_store::thread_admission::is_database_paused(store.database_identity())
        .map_err(unavailable)?
        || vega_store::permission_contracts::load_state(store.conn())
            .map_err(unavailable)?
            .reset_pending
    {
        return Err(Error::ResetPending);
    }
    Ok(())
}

fn require_task(conn: &Connection, expected: &AutomationTask) -> Result<(), Error> {
    let mut query = conn
        .prepare(
            "SELECT identity,revision,data FROM automation_tasks WHERE thread_id=?1 AND kind=?2",
        )
        .map_err(unavailable)?;
    let mut rows = query
        .query([expected.thread_id.as_str(), expected.kind.key()])
        .map_err(unavailable)?;
    let row = rows.next().map_err(unavailable)?.ok_or(Error::Conflict)?;
    let identity: String = row.get(0).map_err(unavailable)?;
    let revision: i64 = row.get(1).map_err(unavailable)?;
    let data: String = row.get(2).map_err(unavailable)?;
    let current: AutomationTask = decode(&data).map_err(unavailable)?;
    if &current != expected
        || identity != expected.identity
        || u64::try_from(revision).ok() != Some(expected.revision)
    {
        return Err(Error::Conflict);
    }
    Ok(())
}

fn unavailable(_: impl std::fmt::Display) -> Error {
    Error::Unavailable
}
