use super::*;
use vega_conversation::types::SidebarCollapseUpdate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CollapseIntent {
    target: SidebarCollapseTarget,
    collapsed: bool,
    sequence: u64,
}

#[derive(Default)]
pub(super) struct CollapseState {
    persisted: Vec<SidebarCollapseTarget>,
    revision: u64,
    sequence: u64,
    pub(super) intents: Vec<CollapseIntent>,
    pub(super) in_flight: bool,
    pub(super) errors: Vec<(SidebarCollapseTarget, crate::localized_copy::LocalizedCopy)>,
    cancellation: tokio_util::sync::CancellationToken,
}

impl Drop for CollapseState {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl CollapseState {
    pub(super) fn toggle(&mut self, target: SidebarCollapseTarget) {
        let collapsed = !self.effective().contains(&target);
        self.sequence = self.sequence.wrapping_add(1);
        self.intents.retain(|intent| intent.target != target);
        self.intents.push(CollapseIntent {
            target,
            collapsed,
            sequence: self.sequence,
        });
    }

    pub(super) fn effective(&self) -> Vec<SidebarCollapseTarget> {
        let mut collapsed = self.persisted.clone();
        for intent in &self.intents {
            collapsed.retain(|target| target != &intent.target);
            if intent.collapsed {
                collapsed.push(intent.target.clone());
            }
        }
        collapsed
    }

    pub(super) fn accept(&mut self, update: SidebarCollapseUpdate) {
        if update.revision >= self.revision {
            self.revision = update.revision;
            self.persisted = update.collapsed;
        }
    }

    pub(super) fn retain_targets(&mut self, snapshot: &SidebarOrganizationSnapshot) {
        let exists = |target: &SidebarCollapseTarget| match target {
            SidebarCollapseTarget::Project(id) => {
                snapshot.projects.iter().any(|project| &project.id == id)
            }
            SidebarCollapseTarget::Group(id) => snapshot.groups.iter().any(|group| &group.id == id),
            _ => true,
        };
        self.intents.retain(|intent| exists(&intent.target));
        self.errors.retain(|(target, _)| exists(target));
        self.persisted.retain(exists);
    }

    fn finish(
        &mut self,
        batch: &[CollapseIntent],
        result: Result<SidebarCollapseUpdate, crate::localized_copy::LocalizedCopy>,
    ) {
        self.in_flight = false;
        match result {
            Ok(update) => {
                self.accept(update);
                self.errors
                    .retain(|(target, _)| !batch.iter().any(|intent| &intent.target == target));
            }
            Err(error) => {
                for intent in batch {
                    if self.intents.contains(intent) {
                        self.errors.retain(|(target, _)| target != &intent.target);
                        self.errors.push((intent.target.clone(), error.clone()));
                    }
                }
            }
        }
        self.intents.retain(|intent| !batch.contains(intent));
    }
}

impl ThreadsBlock {
    pub(super) fn apply_local_collapse(&mut self) {
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        let collapsed = org.collapse.effective();
        if let Some(snapshot) = org.snapshot.as_mut() {
            let snapshot = std::rc::Rc::make_mut(snapshot);
            snapshot.collapsed = collapsed;
            for target in &snapshot.collapsed {
                if let SidebarCollapseTarget::Project(id) = target {
                    org.project_threads_visible
                        .insert(id.clone(), PROJECT_TASKS_PAGE);
                    org.project_growth_paused.insert(id.clone());
                }
            }
        }
    }

    pub(super) fn start_collapse_save(&mut self, cx: &mut Context<Self>) {
        let database = with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
            .ok()
            .flatten();
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        if org.database != database {
            self.refresh_organization(cx);
            return;
        }
        if org.pending || org.collapse.in_flight || org.collapse.intents.is_empty() {
            return;
        }
        let Some(database) = database else {
            return;
        };
        let generation = org.generation;
        let batch = org.collapse.intents.clone();
        let changes: Vec<_> = batch
            .iter()
            .map(|intent| (intent.target.clone(), intent.collapsed))
            .collect();
        org.collapse.in_flight = true;
        let cancellation = org.collapse.cancellation.clone();
        let worker_database = database.clone();
        let worker = cx.background_executor().spawn(async move {
            let store = Store::open_existing(worker_database)
                .map_err(crate::localized_copy::LocalizedCopy::storage)?;
            service::save_collapsed_with_cancel(&store, &changes, &cancellation)
                .map_err(crate::localized_copy::LocalizedCopy::from)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                let current_database =
                    with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                        .ok()
                        .flatten();
                let Some(org) = this.organization.as_mut() else {
                    return;
                };
                if current_database.as_ref() != Some(&database)
                    || org.database.as_ref() != Some(&database)
                    || org.generation != generation
                {
                    return;
                }
                let result = result.map(|(base_revision, update)| {
                    if let Some(snapshot) = org.snapshot.as_mut() {
                        if snapshot.revision == base_revision {
                            std::rc::Rc::make_mut(snapshot).revision = update.revision;
                        } else {
                            org.refresh_queued = true;
                        }
                    }
                    update
                });
                org.collapse.finish(&batch, result);
                this.apply_local_collapse();
                this.start_collapse_save(cx);
                if this
                    .organization
                    .as_ref()
                    .is_some_and(|org| !org.collapse.in_flight && org.refresh_queued)
                {
                    this.refresh_organization(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}
