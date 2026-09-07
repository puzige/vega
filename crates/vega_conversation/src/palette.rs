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
        let tasks = vega_store::palette::search(store.conn(), project, &query)
            .map_err(|_| PaletteError::Unavailable)?
            .into_iter()
            .filter(|row| std::path::Path::new(&row.project_path).is_dir())
            .take(30)
            .map(|row| PaletteTask {
                id: row.id,
                project_id: row.project_id,
                title: row.title,
                project_name: row.project_name,
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_migrated_store_project_search_task_open_and_safe_preview() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("AGENTS.md"), "# Owned\n\nRead-only preview.").unwrap();
        std::fs::write(root.join("binary.dat"), [0, 1, 2]).unwrap();
        std::fs::write(root.join("large.txt"), vec![b'a'; 128 * 1024 + 1]).unwrap();
        let path = dir.path().join("vega.db");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let project =
            vega_store::projects::create(store.conn(), root.to_str().unwrap(), "owned", None)
                .unwrap();
        let thread =
            crate::threads::create_thread(&store, &project.id, "model", "confirm").unwrap();
        vega_store::threads::rename(store.conn(), &thread.id, "Owned search task", 42).unwrap();
        let service = PaletteService::new(path);
        let cancel = CancellationToken::new();
        let found = service
            .search(Some(&project.id), "search", &cancel)
            .unwrap();
        assert_eq!(found.tasks.len(), 1);
        let opened = service.open_task(&found.tasks[0]).unwrap();
        assert_eq!(opened.id, thread.id);
        for index in 0..600 {
            std::fs::write(root.join(format!("a-{index:04}.txt")), "x").unwrap();
        }
        std::fs::write(root.join("zz-late-target.md"), "late").unwrap();
        assert_eq!(
            service
                .search(Some(&project.id), "zz-late-target", &cancel)
                .unwrap()
                .files,
            vec!["zz-late-target.md"]
        );
        std::fs::write(dir.path().join("outside-only.txt"), "outside").unwrap();
        assert!(
            service
                .search(Some(&project.id), "outside-only", &cancel)
                .unwrap()
                .files
                .is_empty()
        );
        let files = service
            .search(Some(&project.id), "AGENTS.md", &cancel)
            .unwrap();
        assert_eq!(files.files, vec!["AGENTS.md"]);
        let preview = service.preview(&project.id, "AGENTS.md").unwrap();
        assert!(preview.content.starts_with("# Owned"));
        assert_eq!(
            service.reveal_path(&project.id, "AGENTS.md").unwrap(),
            root.join("AGENTS.md")
        );
        assert_eq!(
            service.preview(&project.id, "../vega.db"),
            Err(PaletteError::UnsafePath)
        );
        assert_eq!(
            service.preview(&project.id, "binary.dat"),
            Err(PaletteError::Binary)
        );
        assert_eq!(
            service.preview(&project.id, "large.txt"),
            Err(PaletteError::TooLarge)
        );
        std::os::unix::fs::symlink(dir.path().join("vega.db"), root.join("escape")).unwrap();
        assert_eq!(
            service.preview(&project.id, "escape"),
            Err(PaletteError::UnsafePath)
        );
        std::os::unix::fs::symlink(root.join("AGENTS.md"), root.join("alias")).unwrap();
        assert_eq!(
            service.preview(&project.id, "alias"),
            Err(PaletteError::UnsafePath)
        );
        cancel.cancel();
        assert_eq!(
            service.search(Some(&project.id), "", &cancel),
            Err(PaletteError::Cancelled)
        );
        assert_eq!(
            std::fs::read_to_string(root.join("AGENTS.md")).unwrap(),
            "# Owned\n\nRead-only preview."
        );
    }
    #[test]
    fn search_is_bounded_and_missing_database_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vega.db");
        let service = PaletteService::new(path.clone());
        assert_eq!(
            service.search(None, "", &CancellationToken::new()),
            Err(PaletteError::Unavailable)
        );
        assert!(!path.exists());
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let project =
            vega_store::projects::create(store.conn(), dir.path().to_str().unwrap(), "owned", None)
                .unwrap();
        for _ in 0..35 {
            crate::threads::create_thread(&store, &project.id, "model", "confirm").unwrap();
        }
        assert_eq!(
            service
                .search(None, "", &CancellationToken::new())
                .unwrap()
                .tasks
                .len(),
            30
        );
    }
}
