//! Active-composer read-only HEAD projection; independent of switch authority.
use super::*;
use crate::sidebar::{OpenedThread, VegaStore};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use vega_conversation::{
    ProjectBranchService,
    types::{ProjectBranchState, ProjectBranchTarget},
};

pub(super) struct CurrentHead {
    service: Option<ProjectBranchService>,
    database: Option<PathBuf>,
    target: Option<ProjectBranchTarget>,
    generation: u64,
    resolving: bool,
    pending: Option<Instant>,
    next_refresh: Instant,
    pub(super) state: Option<ProjectBranchState>,
}
impl CurrentHead {
    pub(super) fn new() -> Self {
        Self {
            service: ProjectBranchService::new().ok(),
            database: None,
            target: None,
            generation: 0,
            resolving: false,
            pending: None,
            next_refresh: Instant::now(),
            state: None,
        }
    }
    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(service) = &self.service {
            service.invalidate(self.generation);
        }
        self.pending = None;
        self.next_refresh = Instant::now();
    }
}
fn database(cx: &App) -> Option<PathBuf> {
    cx.try_global::<VegaStore>()?
        .0
        .as_ref()
        .ok()?
        .database_path()
        .map(PathBuf::from)
}
impl BranchSelector {
    fn active_head_owner(&self, cx: &App) -> bool {
        cx.try_global::<OpenedThread>()
            .and_then(|v| v.0.as_ref())
            .is_some_and(|thread| {
                thread.id == self.thread_id && thread.project_id == self.project_id
            })
    }
    pub(super) fn poll_current_head(&mut self, cx: &mut Context<Self>) {
        let owner = database(cx);
        if !self.active_head_owner(cx) || owner != self.current_head.database {
            self.current_head.invalidate();
            self.current_head.target = None;
            self.current_head.state = None;
            self.current_head.database = owner;
            if !self.active_head_owner(cx) {
                return;
            }
        }
        if self.model.is_pending() {
            return;
        }
        if self.current_head.target.is_none() {
            if self.current_head.resolving || Instant::now() < self.current_head.next_refresh {
                return;
            }
            let Some(owner) = self.current_head.database.clone() else {
                return;
            };
            self.current_head.resolving = true;
            let generation = self.current_head.generation;
            let project_id = self.project_id.clone();
            let worker_owner = owner.clone();
            let worker = cx.background_executor().spawn(async move {
                let store = vega_store::Store::open(worker_owner).ok()?;
                let project = vega_store::projects::find(store.conn(), &project_id).ok()??;
                Some(ProjectBranchTarget {
                    project_id,
                    registered_path: project.path,
                })
            });
            cx.spawn(async move |this, cx| {
                let target = worker.await;
                this.update(cx, |this, cx| {
                    this.current_head.resolving = false;
                    if this.current_head.generation != generation
                        || database(cx).as_ref() != Some(&owner)
                        || !this.active_head_owner(cx)
                    {
                        return;
                    }
                    this.current_head.target = target;
                    if this.current_head.target.is_none() {
                        this.current_head.state = Some(ProjectBranchState::Unknown);
                        this.current_head.next_refresh = Instant::now() + Duration::from_secs(2);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        if let Some(completion) = self
            .current_head
            .service
            .as_ref()
            .and_then(|s| s.take_completion())
            && completion.generation == self.current_head.generation
        {
            self.current_head.pending = None;
            if let Some(row) = completion
                .rows
                .into_iter()
                .find(|row| Some(&row.target) == self.current_head.target.as_ref())
            {
                self.current_head.state = Some(row.state);
                cx.notify();
            }
        }
        if self
            .current_head
            .pending
            .is_some_and(|start| start.elapsed() >= Duration::from_secs(1))
        {
            self.current_head.invalidate();
            self.current_head.state = Some(ProjectBranchState::Unknown);
            self.current_head.next_refresh = Instant::now() + Duration::from_secs(2);
            cx.notify();
        }
        if self.current_head.pending.is_none() && Instant::now() >= self.current_head.next_refresh {
            self.current_head.generation = self.current_head.generation.wrapping_add(1);
            if let (Some(service), Some(target)) =
                (&self.current_head.service, &self.current_head.target)
            {
                service.request(self.current_head.generation, vec![target.clone()]);
                self.current_head.pending = Some(Instant::now());
                self.current_head.next_refresh = Instant::now() + Duration::from_secs(2);
            } else {
                self.current_head.state = Some(ProjectBranchState::Unknown);
                self.current_head.next_refresh = Instant::now() + Duration::from_secs(2);
                cx.notify();
            }
        }
    }
    pub(super) fn current_head_label(&self) -> &str {
        match &self.current_head.state {
            Some(ProjectBranchState::Branch(label)) => label,
            Some(ProjectBranchState::Detached) => "detached",
            Some(ProjectBranchState::NonGit) => "非 Git 文件夹",
            Some(ProjectBranchState::Unknown) => "分支暂不可用",
            None => "读取分支…",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    async fn r14_head_resolution_rejects_replaced_database_owner(cx: &mut gpui::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let owned = dir.path().canonicalize().unwrap();
        let mut stores = Vec::new();
        for branch in ["old-owner", "new-owner"] {
            let root = owned.join(branch);
            std::fs::create_dir(&root).unwrap();
            assert!(
                std::process::Command::new("/usr/bin/git")
                    .arg("-C")
                    .arg(&root)
                    .args(["init", "-b", branch])
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
            let store = vega_store::Store::open(owned.join(format!("{branch}.sqlite"))).unwrap();
            store.migrate().unwrap();
            store.conn().execute("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('same-project',?1,'owned',0,0)", [root.to_str().unwrap()]).unwrap();
            stores.push(store);
        }
        let old = stores.remove(0);
        let thread =
            vega_conversation::threads::create_thread(&old, "same-project", "mock", "confirm")
                .unwrap();
        cx.update(|cx| {
            cx.set_global(VegaStore(Ok(old)));
            cx.set_global(OpenedThread(Some(thread.clone())));
        });
        let selector = cx.new(|cx| BranchSelector::new(thread.id, thread.project_id, cx));
        selector.update(cx, |selector, cx| selector.poll_current_head(cx));
        cx.update(|cx| cx.set_global(VegaStore(Ok(stores.remove(0)))));
        selector.update(cx, |selector, cx| selector.poll_current_head(cx));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            cx.run_until_parked();
            selector.update(cx, |selector, cx| selector.poll_current_head(cx));
            if selector.read_with(cx, |selector, _| {
                selector.current_head_label() == "new-owner"
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "new exact owner did not resolve");
            std::thread::sleep(Duration::from_millis(5));
        }
        selector.read_with(cx, |selector, _| {
            assert!(!selector.model.is_open());
            assert_eq!(
                selector
                    .current_head
                    .target
                    .as_ref()
                    .unwrap()
                    .registered_path,
                owned.join("new-owner").to_str().unwrap()
            );
        });
        assert_eq!(
            std::fs::read_to_string(owned.join("old-owner/.git/HEAD")).unwrap(),
            "ref: refs/heads/old-owner\n"
        );
        assert_eq!(
            std::fs::read_to_string(owned.join("new-owner/.git/HEAD")).unwrap(),
            "ref: refs/heads/new-owner\n"
        );
    }
}
