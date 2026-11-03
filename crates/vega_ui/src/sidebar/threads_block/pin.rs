use super::*;

#[cfg(test)]
mod tests;

impl ThreadsBlock {
    pub(super) fn toggle_pin(&mut self, thread_id: &str, pinned: bool, cx: &mut Context<Self>) {
        if task_mutation_busy(cx) || self.pin_pending.is_some() {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.a_task_is_being_saved_try_again_shortly",
            ));
            cx.notify();
            return;
        }
        let database = with_store(cx, |store| {
            store.database_path().map(Path::to_path_buf).ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new(
                    "ui.organization_storage_requires_a_file_backed_database",
                )
            })
        });
        let database = match database {
            Ok(database) => database,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let thread_id = thread_id.to_string();
        let worker_id = thread_id.clone();
        let worker_database = database.clone();
        self.observe_visit_route(cx);
        let generation = self.visit_navigation_generation;
        let route = Self::current_visit_route(cx);
        self.pin_pending = Some(thread_id);
        self.error = None;
        crate::navigation::begin_task_mutation(cx);
        let worker = cx.background_executor().spawn(async move {
            conversation::persist_thread_pinned(&worker_database, &worker_id, !pinned)
                .map_err(crate::localized_copy::LocalizedCopy::from)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            cx.update(crate::navigation::finish_task_mutation);
            let _ = this.update(cx, |this, cx| {
                this.pin_pending = None;
                let current_database =
                    with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                        .ok()
                        .flatten();
                if current_database.as_ref() != Some(&database) {
                    cx.notify();
                    return;
                }
                this.observe_visit_route(cx);
                let current_route = generation == this.visit_navigation_generation
                    && route == Self::current_visit_route(cx);
                match result {
                    Ok(thread) => {
                        for row in this.threads.iter_mut().chain(this.archived.iter_mut()) {
                            if row.id == thread.id {
                                row.pinned = thread.pinned;
                            }
                        }
                        if let Some(org) = this.organization.as_mut()
                            && let Some(snapshot) = org.snapshot.as_mut()
                        {
                            for row in &mut snapshot.threads {
                                if row.id == thread.id {
                                    row.pinned = thread.pinned;
                                }
                            }
                        }
                        if current_route {
                            this.hovered = None;
                            this.hover_suppressed_until_move = true;
                            this.error = None;
                        }
                    }
                    Err(error) if current_route => {
                        this.error = Some(error);
                    }
                    Err(_) => {}
                }
                if this.organization.is_some() {
                    this.refresh_organization(cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
