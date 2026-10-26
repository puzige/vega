use super::{AutomationReason, AutomationStatus, AutomationTask, decode, save, storage};
use vega_store::Connection;

pub(crate) fn revoke_in_transaction(conn: &Connection, now: i64) -> Result<(), String> {
    let mut query = conn
        .prepare("SELECT thread_id,kind,identity,revision,data FROM automation_tasks ORDER BY thread_id,kind")
        .map_err(storage)?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)?;
    drop(query);
    for (thread, kind, identity, revision, data) in rows {
        let mut task: AutomationTask = decode(&data)?;
        if task.thread_id != thread
            || task.kind.key() != kind
            || task.identity != identity
            || i64::try_from(task.revision).ok() != Some(revision)
        {
            return Err(vega_i18n::t("domain.automation.error.changed").into());
        }
        if task.status != AutomationStatus::Active && task.permission_authorization.is_none() {
            continue;
        }
        task.revision = task
            .revision
            .checked_add(1)
            .filter(|value| i64::try_from(*value).is_ok())
            .ok_or(vega_i18n::t("domain.automation.error.revision_limit"))?;
        task.permission_authorization = None;
        if task.status == AutomationStatus::Active {
            task.status = AutomationStatus::Paused;
            task.system_reason = Some(AutomationReason::PermissionReset);
        }
        task.updated_at_ms = now;
        save(conn, &task)?;
    }
    Ok(())
}
