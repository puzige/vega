use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{OrdinaryRun, finish_ordinary_worker};
use vega_conversation::types::{
    AttemptId, SkillSelectionIntent, SubmissionFailure, SubmissionId, Thread,
};

#[derive(Clone)]
pub(crate) struct OrdinaryContext {
    pub(crate) thread: Thread,
    pub(crate) draft: Option<Thread>,
    pub(crate) skill: Option<SkillSelectionIntent>,
    pub(crate) config: Option<PathBuf>,
    cancellation: Option<(AttemptId, CancellationToken)>,
}

impl OrdinaryContext {
    pub(crate) fn new(
        thread: Thread,
        draft: Option<Thread>,
        skill: Option<SkillSelectionIntent>,
        config: Option<PathBuf>,
    ) -> Self {
        Self {
            thread,
            draft,
            skill,
            config,
            cancellation: None,
        }
    }
}

pub(crate) fn set_cancel(
    id: SubmissionId,
    attempt: AttemptId,
    cancel: CancellationToken,
) -> Result<(), ()> {
    let mut contexts = CONTEXTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| ())?;
    let context = contexts.get_mut(&id).ok_or(())?;
    context.cancellation = Some((attempt, cancel));
    Ok(())
}

pub(crate) fn cancel(id: SubmissionId, attempt: AttemptId) {
    if let Ok(contexts) = CONTEXTS.get_or_init(Default::default).lock()
        && let Some((current, cancel)) = contexts
            .get(&id)
            .and_then(|context| context.cancellation.as_ref())
        && *current == attempt
    {
        cancel.cancel();
    }
}

static CONTEXTS: OnceLock<Mutex<HashMap<SubmissionId, OrdinaryContext>>> = OnceLock::new();

pub(crate) fn retain_context(id: SubmissionId, context: OrdinaryContext) -> Result<(), ()> {
    CONTEXTS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| ())?
        .entry(id)
        .or_insert(context);
    Ok(())
}

pub(crate) fn context(id: SubmissionId) -> Option<OrdinaryContext> {
    CONTEXTS
        .get_or_init(Default::default)
        .lock()
        .ok()?
        .get(&id)
        .cloned()
}

pub(crate) fn retire_context(id: SubmissionId) {
    if let Ok(mut contexts) = CONTEXTS.get_or_init(Default::default).lock() {
        contexts.remove(&id);
    }
}

pub(crate) struct OrdinaryLaunch {
    run: Option<OrdinaryRun>,
    pub(crate) database: PathBuf,
    pub(crate) workspace: PathBuf,
    pub(crate) thread: Thread,
    pub(crate) cancel: CancellationToken,
}

impl OrdinaryLaunch {
    pub(crate) fn new(
        run: OrdinaryRun,
        database: PathBuf,
        workspace: PathBuf,
        thread: Thread,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            run: Some(run.with_thread_snapshot(thread.clone())),
            database,
            workspace,
            thread,
            cancel,
        }
    }

    pub(crate) fn run(&self) -> Option<&OrdinaryRun> {
        self.run.as_ref()
    }

    pub(crate) fn take_run(&mut self) -> Option<OrdinaryRun> {
        self.run.take()
    }

    pub(crate) fn finish(&mut self, result: Result<(), SubmissionFailure>) {
        if let Some(run) = self.run.take() {
            finish_root(run, result);
        }
    }
}

impl Drop for OrdinaryLaunch {
    fn drop(&mut self) {
        if self.run.is_some() {
            self.cancel.cancel();
            self.finish(Err(SubmissionFailure::LaunchFailed));
        }
    }
}

pub(crate) fn finish_root(run: OrdinaryRun, result: Result<(), SubmissionFailure>) {
    let attempt = run.worker.attempt_id();
    finish_ordinary_worker(&run.submission, attempt, run.worker, result);
}
