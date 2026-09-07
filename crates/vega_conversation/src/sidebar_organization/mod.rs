//! Revision-checked sidebar metadata service. Run on a blocking/background executor.
use crate::types::*;
use vega_store::{Store, sidebar_organization as sql};

/// Maximum tasks (including archived) admitted by a complete snapshot.
pub const MAX_SIDEBAR_THREADS: usize = 10_000;
/// Maximum persisted task groups.
pub const MAX_SIDEBAR_GROUPS: usize = 128;
fn storage(error: impl std::fmt::Display) -> SidebarOrganizationError {
    SidebarOrganizationError::Store(error.to_string())
}
/// Reads a consistent metadata-only snapshot, without navigation or task mutations.
pub fn snapshot(store: &Store) -> Result<SidebarOrganizationSnapshot, SidebarOrganizationError> {
    let tx = store.conn().unchecked_transaction().map_err(storage)?;
    let result = read_snapshot(store)?;
    tx.commit().map_err(storage)?;
    Ok(result)
}
fn read_snapshot(store: &Store) -> Result<SidebarOrganizationSnapshot, SidebarOrganizationError> {
    let conn = store.conn();
    let (tasks, groups) = sql::counts(conn).map_err(storage)?;
    if tasks > MAX_SIDEBAR_THREADS as i64 || groups > MAX_SIDEBAR_GROUPS as i64 {
        return Err(SidebarOrganizationError::Limit(
            "maximum 10000 tasks and 128 groups".into(),
        ));
    }
    let rows = sql::read(conn).map_err(storage)?;
    let projects = sql::projects(conn)
        .map_err(storage)?
        .into_iter()
        .map(|p| SidebarProject {
            id: p.id,
            name: p.name,
            path: p.path,
            git_default_branch: p.git_default_branch,
            created_at: p.created_at,
            last_opened_at: p.last_opened_at,
        })
        .collect::<Vec<_>>();
    let threads = sql::threads(
        conn,
        &projects.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
    )
    .map_err(storage)?
    .iter()
    .map(crate::threads::thread_from_row)
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage)?;
    let groups = rows
        .groups
        .into_iter()
        .map(|(id, name, color)| {
            Ok(SidebarGroup {
                id,
                name,
                color: serde_json::from_str(&color).map_err(storage)?,
            })
        })
        .collect::<Result<Vec<_>, SidebarOrganizationError>>()?;
    Ok(SidebarOrganizationSnapshot {
        revision: u64::try_from(rows.revision).map_err(storage)?,
        projects,
        threads,
        groups,
        memberships: rows
            .memberships
            .into_iter()
            .map(|(thread_id, group_id)| SidebarMembership {
                thread_id,
                group_id,
            })
            .collect(),
        project_order: rows.project_order,
        preferences: serde_json::from_str(&rows.preferences).map_err(storage)?,
        collapsed: serde_json::from_str(&rows.collapsed).map_err(storage)?,
    })
}

mod mutations;
pub use mutations::apply;
mod calendar;
pub use calendar::local_calendar_bucket;

#[cfg(test)]
mod tests;

/// Register or reuse a folder and reveal its project in one revision-checked transaction.
/// Call on a background executor; folder validation and canonicalization perform filesystem IO.
pub fn register_and_reveal_project(
    store: &Store,
    path: &std::path::Path,
    branch: Option<&str>,
) -> Result<(String, SidebarOrganizationSnapshot), SidebarOrganizationError> {
    let canonical = path.canonicalize().map_err(storage)?;
    if !canonical.is_dir() {
        return Err(SidebarOrganizationError::Invalid(
            "folder is not a directory".into(),
        ));
    }
    let tx = store.immediate_transaction().map_err(storage)?;
    let revision = read_snapshot(store)?.revision;
    let original = path.to_string_lossy();
    let normalized = canonical.to_string_lossy();
    let existing = vega_store::projects::find_by_path(store.conn(), &original)
        .map_err(storage)?
        .or(vega_store::projects::find_by_path(store.conn(), &normalized).map_err(storage)?);
    let project = match existing {
        Some(project) => project,
        None => {
            let name = canonical
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or(&normalized);
            vega_store::projects::create(store.conn(), &normalized, name, branch)
                .map_err(storage)?
        }
    };
    vega_store::projects::touch_last_opened(store.conn(), &project.id).map_err(storage)?;
    let result = mutations::apply_in_transaction(
        store,
        revision,
        SidebarOrganizationAction::RevealProject {
            project_id: project.id.clone(),
        },
    )?;
    tx.commit().map_err(storage)?;
    Ok((project.id, result.snapshot))
}
