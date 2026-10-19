//! Primitive sidebar metadata SQL. Callers own validation and transaction boundaries.
use crate::{projects, threads};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

pub fn next_revision(revision: u64) -> Option<u64> {
    revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
}

fn metadata_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn invalid_metadata(message: &str) -> rusqlite::Error {
    metadata_error(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    ))
}

pub fn promote_thread(conn: &Connection, thread_id: &str) -> rusqlite::Result<()> {
    let source: Option<(Option<String>, bool)> = conn
        .query_row(
            "SELECT project_id,pinned FROM threads WHERE id=?1 AND (parent_thread_id IS NULL OR pinned=1)",
            [thread_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((project_id, pinned)) = source else {
        return Ok(());
    };
    let (current_revision, preferences): (i64, String) = conn.query_row(
        "SELECT revision,preferences FROM sidebar_organization WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let revision = u64::try_from(current_revision)
        .ok()
        .and_then(next_revision)
        .ok_or_else(|| invalid_metadata("revision exhausted"))?;
    let mut preferences: Map<String, Value> =
        serde_json::from_str(&preferences).map_err(metadata_error)?;
    let saved: Vec<String> = preferences
        .get("manual_thread_order")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(metadata_error)?
        .unwrap_or_default();
    let threads = conn
        .prepare(
            "SELECT id,project_id,pinned FROM threads WHERE parent_thread_id IS NULL OR pinned=1 ORDER BY created_at DESC,id LIMIT 10001",
        )?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, bool>(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if threads.len() > 10_000 {
        return Err(invalid_metadata("maximum 10000 tasks"));
    }
    let regions: HashMap<_, _> = threads
        .iter()
        .map(|(id, project, pinned)| (id.as_str(), (project.as_deref(), *pinned)))
        .collect();
    let mut seen = HashSet::new();
    let mut saved: Vec<_> = saved
        .into_iter()
        .rev()
        .filter(|id| regions.contains_key(id.as_str()) && seen.insert(id.clone()))
        .collect();
    saved.reverse();
    let mut order: Vec<_> = threads
        .iter()
        .filter(|(id, _, _)| !seen.contains(id))
        .map(|(id, _, _)| id.clone())
        .chain(saved)
        .filter(|id| id != thread_id)
        .collect();
    let first = order.iter().position(|id| {
        regions
            .get(id.as_str())
            .is_some_and(|(project, candidate_pinned)| {
                *candidate_pinned == pinned && (pinned || *project == project_id.as_deref())
            })
    });
    order.insert(first.unwrap_or(order.len()), thread_id.to_owned());
    preferences.insert("sort".into(), Value::String("Manual".into()));
    preferences.insert(
        "manual_thread_order".into(),
        serde_json::to_value(order).map_err(metadata_error)?,
    );
    let preferences = serde_json::to_string(&preferences).map_err(metadata_error)?;
    let updated = conn.execute(
        "UPDATE sidebar_organization SET preferences=?1,revision=?2 WHERE singleton=1 AND revision=?3",
        params![preferences, revision as i64, current_revision],
    )?;
    if updated != 1 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(())
}

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
    let mut preferences: Map<String, Value> =
        serde_json::from_str(&existing.preferences).map_err(metadata_error)?;
    let updated: Map<String, Value> =
        serde_json::from_str(&rows.preferences).map_err(metadata_error)?;
    preferences.extend(updated);
    let preferences = serde_json::to_string(&preferences).map_err(metadata_error)?;
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
        params![rows.revision, preferences, rows.collapsed],
    )?;
    Ok(())
}
