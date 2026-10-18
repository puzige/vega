use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, PartialEq, Eq)]
pub struct DelegationRow {
    pub child_thread_id: String,
    pub parent_thread_id: String,
    pub parent_message_id: String,
    pub spawn_call_id: String,
    pub child_message_id: String,
    pub name: String,
    pub task: String,
    pub state: String,
    pub result: String,
    pub truncated: bool,
    pub created_at: i64,
}

const COLUMNS: &str = "child_thread_id,parent_thread_id,parent_message_id,spawn_call_id,child_message_id,name,task,state,result,truncated,created_at";

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DelegationRow> {
    Ok(DelegationRow {
        child_thread_id: row.get(0)?,
        parent_thread_id: row.get(1)?,
        parent_message_id: row.get(2)?,
        spawn_call_id: row.get(3)?,
        child_message_id: row.get(4)?,
        name: row.get(5)?,
        task: row.get(6)?,
        state: row.get(7)?,
        result: row.get(8)?,
        truncated: row.get(9)?,
        created_at: row.get(10)?,
    })
}

pub fn list(
    conn: &Connection,
    parent: &str,
    run: Option<&str>,
) -> rusqlite::Result<Vec<DelegationRow>> {
    let mut query = conn.prepare(&format!("SELECT {COLUMNS} FROM delegated_runs WHERE parent_thread_id=?1 AND (?2 IS NULL OR parent_message_id=?2) ORDER BY created_at,child_thread_id"))?;
    query.query_map(params![parent, run], row)?.collect()
}

pub fn find(conn: &Connection, child: &str) -> rusqlite::Result<Option<DelegationRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM delegated_runs WHERE child_thread_id=?1"),
        [child],
        row,
    )
    .optional()
}

pub(crate) fn privacy_scope(
    conn: &Connection,
    thread: &str,
) -> rusqlite::Result<Vec<DelegationRow>> {
    let mut query = conn.prepare(&format!("SELECT {COLUMNS} FROM delegated_runs WHERE parent_thread_id=?1 OR child_thread_id=?1 ORDER BY child_thread_id"))?;
    query.query_map([thread], row)?.collect()
}

pub fn active_count(conn: &Connection, parent: &str, run: &str) -> rusqlite::Result<usize> {
    let count:i64 = conn.query_row("SELECT COUNT(*) FROM delegated_runs WHERE parent_thread_id=?1 AND parent_message_id=?2 AND state IN ('running','cancelling')", params![parent,run], |row| row.get(0))?;
    usize::try_from(count).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, count))
}

pub fn insert(conn: &Connection, record: &DelegationRow) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO delegated_runs(child_thread_id,parent_thread_id,parent_message_id,spawn_call_id,child_message_id,name,task,state,result,truncated,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![record.child_thread_id,record.parent_thread_id,record.parent_message_id,record.spawn_call_id,record.child_message_id,record.name,record.task,record.state,record.result,record.truncated,record.created_at])?;
    conn.execute(
        "UPDATE threads SET parent_thread_id=?2,workspace_owner_thread_id=(SELECT COALESCE(workspace_owner_thread_id,id) FROM threads WHERE id=?2 AND project_id IS NULL) WHERE id=?1 AND backend='native' AND id<>?2",
        params![record.child_thread_id, record.parent_thread_id],
    )?;
    Ok(())
}

pub fn request_cancel(conn: &Connection, child: &str, run: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute("UPDATE delegated_runs SET state='cancelling' WHERE child_thread_id=?1 AND child_message_id=?2 AND state='running'", params![child,run])? == 1)
}

pub fn finish(
    conn: &Connection,
    child: &str,
    run: &str,
    state: &str,
    result: &str,
    truncated: bool,
) -> rusqlite::Result<bool> {
    Ok(conn.execute("UPDATE delegated_runs SET state=?3,result=?4,truncated=?5 WHERE child_thread_id=?1 AND child_message_id=?2 AND state IN ('running','cancelling')", params![child,run,state,result,truncated])? == 1)
}

pub fn recover(conn: &Connection, child: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE delegated_runs SET state='interrupted',result='Interrupted before completion',truncated=0 WHERE child_thread_id=?1 AND state IN ('running','cancelling')", [child])?;
    Ok(())
}

pub fn attribute_usage(
    conn: &Connection,
    id: i64,
    child: &str,
    parent: &str,
    run: &str,
    cache_known: bool,
) -> rusqlite::Result<()> {
    conn.execute("UPDATE token_usage SET delegated_parent_thread_id=?3,delegated_parent_message_id=?4,delegated_cache_known=?5 WHERE id=?1 AND thread_id=?2 AND delegated_parent_thread_id IS NULL",params![id,child,parent,run,cache_known])?;
    Ok(())
}

pub fn mark_usage_complete(
    conn: &Connection,
    child: &str,
    run: &str,
    complete: bool,
) -> rusqlite::Result<()> {
    conn.execute("UPDATE delegated_runs SET usage_complete=?3 WHERE child_thread_id=?1 AND child_message_id=?2",params![child,run,complete])?;
    Ok(())
}

pub fn has_unknown_usage(conn: &Connection, parent: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM delegated_runs WHERE parent_thread_id=?1 AND usage_complete=0)",[parent],|row| row.get(0))
}

pub fn run_usage_complete(conn: &Connection, parent: &str, run: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT NOT EXISTS(SELECT 1 FROM delegated_runs WHERE parent_thread_id=?1 AND parent_message_id=?2 AND usage_complete=0) AND NOT EXISTS(SELECT 1 FROM token_usage WHERE delegated_parent_thread_id=?1 AND delegated_parent_message_id=?2 AND delegated_cache_known IS NOT 1)",params![parent,run],|row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO threads(id,model,created_at,updated_at) VALUES(?1,'mock',1,1)",
            [id],
        )
        .unwrap();
    }

    #[test]
    fn m15_m16_relationship_and_original_result_survive_reopen_and_replay() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("store.sqlite");
        let store = crate::Store::open(&path).unwrap();
        store.migrate().unwrap();
        thread(store.conn(), "parent");
        thread(store.conn(), "child");
        let record = DelegationRow {
            child_thread_id: "child".into(),
            parent_thread_id: "parent".into(),
            parent_message_id: "parent-run".into(),
            spawn_call_id: "spawn".into(),
            child_message_id: "child-run".into(),
            name: "Research".into(),
            task: "Find evidence".into(),
            state: "running".into(),
            result: String::new(),
            truncated: false,
            created_at: 1,
        };
        insert(store.conn(), &record).unwrap();
        assert!(insert(store.conn(), &record).is_err());
        assert_eq!(
            crate::threads::find(store.conn(), "child")
                .unwrap()
                .unwrap()
                .parent_thread_id
                .as_deref(),
            Some("parent")
        );
        finish(
            store.conn(),
            "child",
            "child-run",
            "completed",
            "original",
            false,
        )
        .unwrap();
        assert!(
            !finish(
                store.conn(),
                "child",
                "new-user-run",
                "completed",
                "later",
                false
            )
            .unwrap()
        );
        drop(store);
        let reopened = crate::Store::open(path).unwrap();
        let rows = list(reopened.conn(), "parent", Some("parent-run")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].result, "original");
        assert!(list(reopened.conn(), "unrelated", None).unwrap().is_empty());
    }

    #[test]
    fn m03_m04_capacity_and_names_are_bound_to_parent_generation() {
        let store = crate::Store::open(":memory:").unwrap();
        store.migrate().unwrap();
        thread(store.conn(), "parent");
        for index in 0..4 {
            let id = format!("child-{index}");
            thread(store.conn(), &id);
            insert(
                store.conn(),
                &DelegationRow {
                    child_thread_id: id.clone(),
                    parent_thread_id: "parent".into(),
                    parent_message_id: "run".into(),
                    spawn_call_id: id.clone(),
                    child_message_id: id,
                    name: format!("Task {index}"),
                    task: "task".into(),
                    state: "running".into(),
                    result: String::new(),
                    truncated: false,
                    created_at: 1,
                },
            )
            .unwrap();
        }
        assert_eq!(active_count(store.conn(), "parent", "run").unwrap(), 4);
        finish(
            store.conn(),
            "child-0",
            "child-0",
            "failed",
            "failure",
            false,
        )
        .unwrap();
        assert_eq!(active_count(store.conn(), "parent", "run").unwrap(), 3);
        assert_eq!(
            active_count(store.conn(), "parent", "other-run").unwrap(),
            0
        );
    }
}
