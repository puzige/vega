//! Read-only bounded palette service; call from an application worker.
use crate::types::*;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use vega_store::Store;

/// Database-scoped search/read capability; paths come only from registered projects.
#[derive(Clone)]
pub struct PaletteService {
    database: PathBuf,
}
impl PaletteService {
    /// Construct with the app-owned database path.
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }
    fn store(&self) -> Result<Store, PaletteError> {
        Store::open_read_only(&self.database).map_err(|_| PaletteError::Unavailable)
    }
    fn root(&self, project: &str) -> Result<PathBuf, PaletteError> {
        let store = self.store()?;
        vega_store::projects::find(store.conn(), project)
            .map_err(|_| PaletteError::Unavailable)?
            .map(|p| PathBuf::from(p.path))
            .filter(|p| p.is_dir())
            .ok_or(PaletteError::ProjectUnavailable)
    }
    /// Search accessible persisted tasks and the current project's bounded file index.
    pub fn search(
        &self,
        project: Option<&str>,
        query: &str,
        cancel: &CancellationToken,
    ) -> Result<PaletteSearch, PaletteError> {
        if cancel.is_cancelled() {
            return Err(PaletteError::Cancelled);
        }
        let query: String = query.chars().take(256).collect();
        let store = self.store()?;
        let projection = crate::privacy::DisplayProjection::load(&store, "")
            .map_err(|_| PaletteError::Unavailable)?;
        let tasks = vega_store::palette::search(store.conn(), project, &query)
            .map_err(|_| PaletteError::Unavailable)?
            .into_iter()
            .filter(|row| std::path::Path::new(&row.project_path).is_dir())
            .take(30)
            .map(|row| PaletteTask {
                id: row.id,
                project_id: row.project_id,
                title: projection.text(&row.title),
                project_name: projection.text(&row.project_name),
            })
            .collect();
        let mut result = PaletteSearch {
            tasks,
            ..Default::default()
        };
        if let Some(project) = project {
            let files = self.root(project).and_then(|root| {
                vega_tools::reference::bounded_file_search(&root, &query, 30, || {
                    cancel.is_cancelled()
                })
                .map_err(|_| PaletteError::Unavailable)
            });
            match files {
                Ok(files) => {
                    let needle = query.to_lowercase();
                    result.files = files
                        .into_iter()
                        .filter(|p| p.to_lowercase().contains(&needle))
                        .take(30)
                        .collect();
                }
                Err(_) => result.files_unavailable = true,
            }
        }
        if cancel.is_cancelled() {
            return Err(PaletteError::Cancelled);
        }
        Ok(result)
    }
    /// Resolve a selected task without writes; the app records a visit after acceptance.
    pub fn open_task(&self, task: &PaletteTask) -> Result<Thread, PaletteError> {
        self.root(&task.project_id)?;
        let store = self.store()?;
        let row = vega_store::threads::find(store.conn(), &task.id)
            .map_err(|_| PaletteError::Unavailable)?
            .ok_or(PaletteError::Unavailable)?;
        if row.project_id != task.project_id || row.status != "active" {
            return Err(PaletteError::Unavailable);
        }
        crate::threads::thread_from_row(&row).map_err(|_| PaletteError::Unavailable)
    }
    /// Read a bounded regular text file, retaining only its relative identity.
    pub fn preview(&self, project: &str, path: &str) -> Result<PaletteFilePreview, PaletteError> {
        let root = self.root(project)?;
        let content =
            vega_tools::reference::preview_text_file(&root, path, 128 * 1024).map_err(|error| {
                match error {
                    vega_tools::ToolError::PathEscape(_) => PaletteError::UnsafePath,
                    vega_tools::ToolError::BinaryFile(_) => PaletteError::Binary,
                    vega_tools::ToolError::TooManyResults { .. } => PaletteError::TooLarge,
                    _ => PaletteError::Unavailable,
                }
            })?;
        Ok(PaletteFilePreview {
            relative_path: path.into(),
            content,
        })
    }
    /// Validate a preview identity again before revealing it in the OS file manager.
    pub fn reveal_path(&self, project: &str, path: &str) -> Result<PathBuf, PaletteError> {
        self.preview(project, path)?;
        Ok(self.root(project)?.join(path))
    }
}
