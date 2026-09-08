//! Background-only route validation against the actual registered database.
use crate::types::{NavigationError, NavigationRoute, Thread};
use std::path::{Path, PathBuf};
use vega_store::Store;

/// File-scoped capability for validating a navigation destination.
pub struct NavigationService {
    database: PathBuf,
}
impl NavigationService {
    /// Construct from the application's owned database path.
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    /// Resolve a route without any timestamp or unread mutation.
    pub fn resolve(&self, route: &NavigationRoute) -> Result<Option<Thread>, NavigationError> {
        let project = match route {
            NavigationRoute::Settings | NavigationRoute::Project(None) => return Ok(None),
            NavigationRoute::Project(Some(project)) => Some(project),
            NavigationRoute::Task { project, .. } => project.as_ref(),
        };
        let store =
            Store::open_read_only(&self.database).map_err(|_| NavigationError::Unavailable)?;
        if let Some(project) = project {
            let registered = vega_store::projects::find(store.conn(), project)
                .map_err(|_| NavigationError::Unavailable)?
                .ok_or(NavigationError::InvalidRoute)?;
            if !Path::new(&registered.path).is_dir() {
                return Err(NavigationError::InvalidRoute);
            }
        }
        let NavigationRoute::Task { task, .. } = route else {
            return Ok(None);
        };
        let row = vega_store::threads::find(store.conn(), task)
            .map_err(|_| NavigationError::Unavailable)?
            .ok_or(NavigationError::InvalidRoute)?;
        let row_project = (!row.project_id.is_empty()).then_some(row.project_id.as_str());
        if row_project != project.map(String::as_str) || row.status != "active" {
            return Err(NavigationError::InvalidRoute);
        }
        crate::threads::thread_from_row(&row)
            .map(Some)
            .map_err(|_| NavigationError::Unavailable)
    }
}
