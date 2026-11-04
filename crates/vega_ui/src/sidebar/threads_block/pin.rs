use super::*;

impl ThreadsBlock {
    pub(super) fn toggle_pin(&mut self, thread_id: &str, pinned: bool, cx: &mut Context<Self>) {
        self.persist_row_update(
            thread_id,
            ThreadUpdate {
                pinned: Some(!pinned),
                ..Default::default()
            },
            cx,
        );
    }

    pub(super) fn persist_row_update(
        &mut self,
        thread_id: &str,
        update: ThreadUpdate,
        cx: &mut Context<Self>,
    ) {
        if task_mutation_busy(cx) || self.row_pending.is_some() {
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
        let committed = update.clone();
        self.observe_visit_route(cx);
        let generation = self.visit_navigation_generation;
        let route = Self::current_visit_route(cx);
        self.row_pending = Some(thread_id);
        self.error = None;
        crate::navigation::begin_task_mutation(cx);
        let worker = cx.background_executor().spawn(async move {
            conversation::persist_thread_update(&worker_database, &worker_id, &update)
                .map_err(crate::localized_copy::LocalizedCopy::from)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            cx.update(crate::navigation::finish_task_mutation);
            let _ = this.update(cx, |this, cx| {
                this.row_pending = None;
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
                                merge_update(row, &committed);
                                if committed.unread.is_some() {
                                    row.last_response_failed = thread.last_response_failed;
                                }
                            }
                        }
                        if committed.status.is_some() {
                            let row = this
                                .threads
                                .iter()
                                .chain(this.archived.iter())
                                .find(|row| row.id == thread.id)
                                .cloned();
                            this.threads.retain(|row| row.id != thread.id);
                            this.archived.retain(|row| row.id != thread.id);
                            if let Some(row) = row {
                                if row.status == ThreadStatus::Archived {
                                    this.archived.push(row);
                                } else {
                                    this.threads.push(row);
                                }
                            }
                        }
                        for rows in [&mut this.threads, &mut this.archived] {
                            rows.sort_by_key(|row| {
                                (
                                    std::cmp::Reverse(row.pinned),
                                    std::cmp::Reverse(row.updated_at),
                                )
                            });
                        }
                        if let Some(mut opened) = cx.global::<OpenedThread>().0.clone()
                            && opened.id == thread.id
                        {
                            merge_update(&mut opened, &committed);
                            if committed.unread.is_some() {
                                opened.last_response_failed = thread.last_response_failed;
                            }
                            cx.set_global(OpenedThread(Some(opened)));
                        }
                        if let Some(org) = this.organization.as_mut()
                            && let Some(snapshot) = org.snapshot.as_mut()
                        {
                            for row in &mut snapshot.threads {
                                if row.id == thread.id {
                                    merge_update(row, &committed);
                                    if committed.unread.is_some() {
                                        row.last_response_failed = thread.last_response_failed;
                                    }
                                }
                            }
                        }
                        if this.organization.is_some() {
                            this.refresh_organization(cx);
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
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
