use super::*;

impl ThreadsBlock {
    pub(in crate::sidebar) fn mutation_route(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (u64, (Option<String>, Option<String>, bool)) {
        self.observe_visit_route(cx);
        (
            self.visit_navigation_generation,
            Self::current_visit_route(cx),
        )
    }

    pub(in crate::sidebar) fn remove_deleted_row(&mut self, id: &str, cx: &mut Context<Self>) {
        self.threads.retain(|row| row.id != id);
        self.archived.retain(|row| row.id != id);
        if let Some(snapshot) = self
            .organization
            .as_mut()
            .and_then(|org| org.snapshot.as_mut())
        {
            std::rc::Rc::make_mut(snapshot)
                .threads
                .retain(|row| row.id != id);
        }
        if let Some(org) = self.organization.as_mut() {
            org.task_projection = None;
            org.focus_dirty = true;
        }
        if self
            .editing
            .as_ref()
            .is_some_and(|session| session.thread_id == id)
        {
            self.editing = None;
        }
        self.close_actions();
        self.reload_background(cx);
    }

    pub(in crate::sidebar) fn reload_background(&mut self, cx: &mut Context<Self>) {
        if self.organization.is_some() {
            self.refresh_organization(cx);
            return;
        }
        let selected = cx.global::<SelectedProject>().0.clone();
        if self.loaded_project != selected {
            self.close_actions();
        }
        self.loaded_project = selected.clone();
        self.project_generation = self.project_generation.wrapping_add(1);
        let generation = self.project_generation;
        let database = with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
            .ok()
            .flatten();
        let worker_database = database.clone();
        let worker_project = selected.clone();
        let worker = cx.background_executor().spawn(async move {
            let Some(project) = worker_project else {
                return Ok((Vec::new(), Vec::new()));
            };
            let database = worker_database.ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new(
                    "ui.task_storage_requires_a_file_backed_database",
                )
            })?;
            let store = Store::open_existing(database)
                .map_err(crate::localized_copy::LocalizedCopy::storage)?;
            let active = conversation::list_threads(&store, &project, Some(ThreadStatus::Active))
                .map_err(crate::localized_copy::LocalizedCopy::from)?;
            let archived =
                conversation::list_threads(&store, &project, Some(ThreadStatus::Archived))
                    .map_err(crate::localized_copy::LocalizedCopy::from)?;
            Ok::<_, crate::localized_copy::LocalizedCopy>((active, archived))
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                let current_database =
                    with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                        .ok()
                        .flatten();
                if this.project_generation != generation
                    || current_database != database
                    || cx.global::<SelectedProject>().0 != selected
                {
                    return;
                }
                match result {
                    Ok((threads, archived)) => {
                        this.threads = threads;
                        this.archived = archived;
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
