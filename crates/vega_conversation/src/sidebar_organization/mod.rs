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
pub fn record_project_visit(
    database: &std::path::Path,
    project_id: &str,
) -> Result<(), SidebarOrganizationError> {
    let store = Store::open_existing(database).map_err(storage)?;
    crate::database_writer::commit(
        database,
        store.database_identity(),
        ProjectVisit(project_id.to_owned()),
    )
    .map_err(storage)?
}
struct ProjectVisit(String);
impl crate::database_writer::Sealed for ProjectVisit {}
impl crate::database_writer::DatabaseCommand for ProjectVisit {
    type Output = Result<(), SidebarOrganizationError>;
    fn apply(self, store: &Store) -> Self::Output {
        vega_store::projects::touch_last_opened(store.conn(), &self.0).map_err(storage)
    }
}

pub fn remove_project(
    database: &std::path::Path,
    project_id: &str,
) -> Result<bool, SidebarOrganizationError> {
    let store = Store::open_existing(database).map_err(storage)?;
    crate::database_writer::commit(
        database,
        store.database_identity(),
        RemoveProject(project_id.to_owned()),
    )
    .map_err(storage)?
}
struct RemoveProject(String);
impl crate::database_writer::Sealed for RemoveProject {}
impl crate::database_writer::DatabaseCommand for RemoveProject {
    type Output = Result<bool, SidebarOrganizationError>;
    fn apply(self, store: &Store) -> Self::Output {
        vega_store::projects::remove(store.conn(), &self.0).map_err(storage)
    }
}

fn read_snapshot(store: &Store) -> Result<SidebarOrganizationSnapshot, SidebarOrganizationError> {
    let conn = store.conn();
    let (tasks, groups) = sql::counts(conn).map_err(storage)?;
    if tasks > MAX_SIDEBAR_THREADS as i64 || groups > MAX_SIDEBAR_GROUPS as i64 {
        return Err(SidebarOrganizationError::LimitKey(
            "domain.organization.validation.total_limit",
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

mod collapse;
pub use collapse::{save_collapsed, save_collapsed_with_cancel};
mod ordering;
pub use ordering::sorted_threads;
mod mutations;
pub use mutations::apply;
mod calendar;
pub use calendar::local_calendar_bucket;

/// Register or reuse a folder and reveal its project in one revision-checked transaction.
/// Call on a background executor; folder validation and canonicalization perform filesystem IO.
pub fn register_and_reveal_project(
    store: &Store,
    path: &std::path::Path,
    branch: Option<&str>,
) -> Result<(String, SidebarOrganizationSnapshot), SidebarOrganizationError> {
    let canonical = path.canonicalize().map_err(storage)?;
    if !canonical.is_dir() {
        return Err(SidebarOrganizationError::InvalidKey(
            "domain.organization.validation.not_directory",
        ));
    }
    let original = path.to_string_lossy().into_owned();
    let normalized = canonical.to_string_lossy().into_owned();
    let name = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&normalized)
        .to_owned();
    let path = store
        .database_path()
        .ok_or_else(|| storage("database path unavailable"))?;
    crate::database_writer::commit(
        path,
        store.database_identity(),
        RegisterProject {
            original,
            normalized,
            name,
            branch: branch.map(str::to_owned),
        },
    )
    .map_err(storage)?
}
struct RegisterProject {
    original: String,
    normalized: String,
    name: String,
    branch: Option<String>,
}
impl crate::database_writer::Sealed for RegisterProject {}
impl crate::database_writer::DatabaseCommand for RegisterProject {
    type Output = Result<(String, SidebarOrganizationSnapshot), SidebarOrganizationError>;
    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            original,
            normalized,
            name,
            branch,
        } = self;
        let tx = store.immediate_transaction().map_err(storage)?;
        let revision = read_snapshot(store)?.revision;
        let existing = vega_store::projects::find_by_path(store.conn(), &original)
            .map_err(storage)?
            .or(vega_store::projects::find_by_path(store.conn(), &normalized).map_err(storage)?);
        let project = match existing {
            Some(project) => project,
            None => {
                vega_store::projects::create(store.conn(), &normalized, &name, branch.as_deref())
                    .map_err(storage)?
            }
        };
        vega_store::projects::touch_last_opened(store.conn(), &project.id).map_err(storage)?;
        let result = mutations::apply_in_transaction(
            store,
            &tx,
            revision,
            SidebarOrganizationAction::RevealProject {
                project_id: project.id.clone(),
            },
            None,
        )?;
        tx.commit().map_err(storage)?;
        Ok((project.id, result.snapshot))
    }
}
