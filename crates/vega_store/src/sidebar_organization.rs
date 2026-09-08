//! Primitive sidebar metadata SQL. Callers own validation and transaction boundaries.
use crate::{projects, threads};
use rusqlite::{Connection, params};

/// Primitive group row ordered by position and stable ID.
pub type GroupRow = (String, String, String);
/// Primitive membership row ordered within its group.
pub type MembershipRow = (String, String);
/// Organization metadata read inside the caller's transaction.
pub struct OrganizationRows {
    pub revision: i64,
    pub preferences: String,
    pub collapsed: String,
    pub groups: Vec<GroupRow>,
    pub memberships: Vec<MembershipRow>,
    pub project_order: Vec<String>,
}
/// Returns exact task/group counts before any bounded snapshot load.
pub fn counts(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    conn.query_row(
        "SELECT (SELECT COUNT(*) FROM threads), (SELECT COUNT(*) FROM sidebar_groups)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}
/// Reads all organization metadata without message content.
pub fn read(conn: &Connection) -> rusqlite::Result<OrganizationRows> {
    let (revision, preferences, collapsed) = conn.query_row(
        "SELECT revision,preferences,collapsed FROM sidebar_organization WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let groups = conn
        .prepare("SELECT id,name,color FROM sidebar_groups ORDER BY position,id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let memberships = conn.prepare("SELECT thread_id,group_id FROM sidebar_memberships ORDER BY group_id,position,thread_id")?.query_map([], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let project_order = conn
        .prepare("SELECT project_id FROM sidebar_project_order ORDER BY position,project_id")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(OrganizationRows {
        revision,
        preferences,
        collapsed,
        groups,
        memberships,
        project_order,
    })
}
/// Reads registered projects with a stable recent-open fallback order.
pub fn projects(conn: &Connection) -> Result<Vec<projects::Project>, projects::ProjectsError> {
    let mut rows = projects::list(conn, projects::ProjectSort::RecentlyOpened)?;
    rows.sort_by(|a, b| {
        b.last_opened_at
            .cmp(&a.last_opened_at)
            .then(a.id.cmp(&b.id))
    });
    Ok(rows)
}
/// Reads bounded task metadata using the existing row decoder.
pub fn threads(
    conn: &Connection,
    project_ids: &[String],
) -> rusqlite::Result<Vec<threads::ThreadRow>> {
    let mut result = Vec::new();
    for id in project_ids {
        result.extend(threads::list_by_project(conn, id, None)?);
    }
    result.extend(threads::list_standalone(conn, None)?);
    Ok(result)
}
/// Persists validated metadata. Must be called within the caller's write transaction.
/// Existing groups are updated in place; only dissolved groups are deleted.
pub fn write(conn: &Connection, rows: &OrganizationRows) -> rusqlite::Result<()> {
    let existing = read(conn)?;
    for (id, _, _) in existing.groups {
        if !rows.groups.iter().any(|g| g.0 == id) {
            conn.execute("DELETE FROM sidebar_groups WHERE id=?1", [id])?;
        }
    }
    for (position, (id, name, color)) in rows.groups.iter().enumerate() {
        conn.execute("INSERT INTO sidebar_groups(id,name,color,position) VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET name=excluded.name,color=excluded.color,position=excluded.position",params![id,name,color,position as i64])?;
    }
    conn.execute("DELETE FROM sidebar_memberships", [])?;
    for (position, (thread_id, group_id)) in rows.memberships.iter().enumerate() {
        conn.execute(
            "INSERT INTO sidebar_memberships(thread_id,group_id,position) VALUES (?1,?2,?3)",
            params![thread_id, group_id, position as i64],
        )?;
    }
    conn.execute("DELETE FROM sidebar_project_order", [])?;
    for (position, id) in rows.project_order.iter().enumerate() {
        conn.execute(
            "INSERT INTO sidebar_project_order(project_id,position) VALUES (?1,?2)",
            params![id, position as i64],
        )?;
    }
    conn.execute(
        "UPDATE sidebar_organization SET revision=?1,preferences=?2,collapsed=?3 WHERE singleton=1",
        params![rows.revision, rows.preferences, rows.collapsed],
    )?;
    Ok(())
}
