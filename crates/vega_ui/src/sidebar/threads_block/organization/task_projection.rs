use super::*;

pub(in crate::sidebar::threads_block) struct TaskProjection {
    pub(super) pinned: Vec<Thread>,
    pub(super) recents: Vec<Thread>,
    pub(super) projects: HashMap<String, Vec<Thread>>,
}

impl ThreadsBlock {
    pub(super) fn task_projection(&self) -> std::rc::Rc<TaskProjection> {
        self.organization
            .as_ref()
            .and_then(|org| org.task_projection.clone())
            .unwrap_or_else(|| {
                std::rc::Rc::new(TaskProjection {
                    pinned: Vec::new(),
                    recents: Vec::new(),
                    projects: HashMap::new(),
                })
            })
    }

    pub(super) fn prepare_task_projection(&mut self, snapshot: &SidebarOrganizationSnapshot) {
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        if org.task_projection.is_some() {
            return;
        }
        let threads: Vec<_> = self
            .threads
            .iter()
            .chain(self.archived.iter().filter(|_| org.archive))
            .cloned()
            .collect();
        let ordered = service::sorted_threads(&threads, &snapshot.preferences);
        let mut projection = TaskProjection {
            pinned: Vec::new(),
            recents: Vec::new(),
            projects: HashMap::new(),
        };
        for thread in ordered {
            if thread.pinned {
                projection.pinned.push(thread);
                continue;
            }
            if !snapshot.preferences.hide_project_threads_from_recents || thread.is_standalone() {
                projection.recents.push(thread.clone());
            }
            projection
                .projects
                .entry(thread.project_id.clone())
                .or_default()
                .push(thread);
        }
        let totals = snapshot
            .projects
            .iter()
            .map(|project| {
                (
                    project.id.clone(),
                    projection.projects.get(&project.id).map_or(0, Vec::len),
                )
            })
            .collect();
        org.sync_project_task_totals(totals);
        org.task_projection = Some(std::rc::Rc::new(projection));
    }
}
