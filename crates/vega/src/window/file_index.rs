use super::*;

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use vega_ui::conversation_stream::{
    FileIndexCancelled, FileIndexFailureCode, FileIndexRequested, FileIndexRetryRequested,
};

/// GPUI polling cadence for the bounded file-index worker.
pub(crate) const FILE_INDEX_POLL: Duration = Duration::from_millis(4);

/// Exact route identity carried from the stream event into the worker and
/// back. Every field participates in the late-result fence.
#[derive(Clone)]
pub(crate) struct FileIndexOwner {
    pub(crate) stream: Entity<ConversationStream>,
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
    pub(crate) generation: u64,
}

impl FileIndexOwner {
    fn same_as(&self, other: &Self) -> bool {
        self.stream == other.stream
            && self.thread_id == other.thread_id
            && self.project_id == other.project_id
            && self.generation == other.generation
    }
}

pub(crate) struct ActiveFileIndex {
    pub(crate) owner: FileIndexOwner,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
}

#[derive(Default)]
pub(crate) struct FileIndexController {
    pub(crate) active: Option<ActiveFileIndex>,
}

impl FileIndexController {
    pub(crate) fn cancel(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancel.cancel();
        }
    }
}

struct FileIndexOutcome {
    owner: FileIndexOwner,
    result: Result<vega_conversation::types::FileIndexSnapshot, FileIndexFailureCode>,
}

fn map_file_index_error(code: vega_tools::reference::FileIndexErrorCode) -> FileIndexFailureCode {
    use vega_tools::reference::FileIndexErrorCode as Source;
    match code {
        Source::ProjectUnavailable => FileIndexFailureCode::ProjectUnavailable,
        Source::Cancelled => FileIndexFailureCode::Cancelled,
        Source::DeadlineExceeded => FileIndexFailureCode::DeadlineExceeded,
        Source::VisitedLimitExceeded => FileIndexFailureCode::VisitedLimitExceeded,
        Source::RetainedBytesExceeded => FileIndexFailureCode::RetainedBytesExceeded,
        Source::Traversal => FileIndexFailureCode::Traversal,
        Source::InvalidPath => FileIndexFailureCode::InvalidPath,
    }
}

impl VegaWindow {
    fn file_index_route_is_current(&self, owner: &FileIndexOwner, cx: &App) -> bool {
        !owner.project_id.is_empty()
            && !cx.global::<SettingsOpen>().0
            && self.owns_stream_request(&owner.stream, &owner.thread_id, cx)
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.project_id == owner.project_id)
            && cx.global::<SelectedProject>().0.as_deref() == Some(owner.project_id.as_str())
    }

    pub(crate) fn request_file_index(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &FileIndexRequested,
        cx: &mut Context<Self>,
    ) {
        self.start_file_index(
            stream,
            &request.thread_id,
            &request.project_id,
            request.generation,
            cx,
        );
    }

    pub(crate) fn retry_file_index(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &FileIndexRetryRequested,
        cx: &mut Context<Self>,
    ) {
        self.start_file_index(
            stream,
            &request.thread_id,
            &request.project_id,
            request.generation,
            cx,
        );
    }

    fn start_file_index(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: &str,
        project_id: &str,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        // File references are project scoped. A standalone task has a private
        // scratch root for tools, but must not silently turn that root into a
        // registered project or enter the project index route.
        if project_id.is_empty() {
            return;
        }
        let owner = FileIndexOwner {
            stream: stream.clone(),
            thread_id: thread_id.to_owned(),
            project_id: project_id.to_owned(),
            generation,
        };
        if !self.file_index_route_is_current(&owner, cx) {
            return;
        }
        if self
            .file_index_controller
            .active
            .as_ref()
            .is_some_and(|active| active.owner.same_as(&owner))
        {
            return;
        }
        // A retry or a new generation replaces the old logical job before its
        // worker can publish a result. The active-owner comparison below drops
        // any late message from that detached worker.
        self.file_index_controller.cancel();

        let project_path = match &cx.global::<VegaStore>().0 {
            Ok(store) => vega_store::projects::find(store.conn(), project_id)
                .ok()
                .flatten()
                .map(|project| PathBuf::from(project.path)),
            Err(_) => None,
        };
        let Some(project_path) = project_path else {
            stream.update(cx, |stream, cx| {
                stream.apply_file_index_result(
                    generation,
                    Err(FileIndexFailureCode::ProjectUnavailable),
                    cx,
                );
            });
            return;
        };

        let cancel = tokio_util::sync::CancellationToken::new();
        let worker_cancel = cancel.clone();
        let worker_owner = owner.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-file-index".into())
            .spawn(move || {
                let result = vega_tools::reference::bounded_file_index_with_budget(
                    &project_path,
                    vega_tools::reference::REFERENCE_INDEX_LIMIT,
                    move || worker_cancel.is_cancelled(),
                )
                .map(|entries| vega_conversation::types::FileIndexSnapshot { entries })
                .map_err(|error| map_file_index_error(error.code()));
                let _ = sender.send(FileIndexOutcome {
                    owner: worker_owner,
                    result,
                });
            });
        if worker.is_err() {
            stream.update(cx, |stream, cx| {
                stream.apply_file_index_result(
                    generation,
                    Err(FileIndexFailureCode::Traversal),
                    cx,
                );
            });
            return;
        }
        let poll_owner = owner.clone();
        let poll_cancel = cancel.clone();
        self.file_index_controller.active = Some(ActiveFileIndex { owner, cancel });
        cx.spawn(async move |this, cx| {
            loop {
                if poll_cancel.is_cancelled() {
                    break;
                }
                let owner = poll_owner.clone();
                let cancel = poll_cancel.clone();
                let current = this
                    .update(cx, |this, cx| {
                        if cancel.is_cancelled() {
                            return false;
                        }
                        let owns_active = this
                            .file_index_controller
                            .active
                            .as_ref()
                            .is_some_and(|active| active.owner.same_as(&owner));
                        if !owns_active {
                            return false;
                        }
                        if !this.file_index_route_is_current(&owner, cx) {
                            this.cancel_file_index_if_route_stale(cx);
                            return false;
                        }
                        true
                    })
                    .unwrap_or(false);
                if !current || poll_cancel.is_cancelled() {
                    break;
                }
                cx.background_executor().timer(FILE_INDEX_POLL).await;
                if poll_cancel.is_cancelled() {
                    break;
                }
                let outcome = match receiver.try_recv() {
                    Ok(outcome) => outcome,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if poll_cancel.is_cancelled() {
                            break;
                        }
                        let owner = poll_owner.clone();
                        let _ = this.update(cx, |this, cx| {
                            this.finish_file_index(
                                FileIndexOutcome {
                                    owner,
                                    result: Err(FileIndexFailureCode::Traversal),
                                },
                                cx,
                            )
                        });
                        break;
                    }
                };
                if poll_cancel.is_cancelled() {
                    break;
                }
                let _ = this.update(cx, |this, cx| this.finish_file_index(outcome, cx));
                break;
            }
        })
        .detach();
    }

    fn finish_file_index(&mut self, outcome: FileIndexOutcome, cx: &mut Context<Self>) {
        let active_matches = self
            .file_index_controller
            .active
            .as_ref()
            .is_some_and(|active| active.owner.same_as(&outcome.owner));
        if !active_matches {
            return;
        }
        self.file_index_controller.active.take();
        let FileIndexOutcome { owner, result } = outcome;
        if !self.file_index_route_is_current(&owner, cx) {
            owner
                .stream
                .update(cx, ConversationStream::invalidate_file_index);
            return;
        }
        let generation = owner.generation;
        owner.stream.update(cx, |stream, cx| {
            stream.apply_file_index_result(generation, result, cx);
        });
    }

    pub(crate) fn cancel_file_index(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &FileIndexCancelled,
        _: &mut Context<Self>,
    ) {
        let matches = self
            .file_index_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.owner.stream == stream
                    && active.owner.thread_id == request.thread_id
                    && active.owner.project_id == request.project_id
                    && active.owner.generation == request.generation
            });
        if matches {
            self.file_index_controller.cancel();
        }
    }

    pub(crate) fn cancel_file_index_if_route_stale(&mut self, cx: &mut App) {
        let stale = self
            .file_index_controller
            .active
            .as_ref()
            .is_some_and(|active| !self.file_index_route_is_current(&active.owner, cx));
        if !stale {
            return;
        }
        let Some(active) = self.file_index_controller.active.take() else {
            return;
        };
        active.cancel.cancel();
        active
            .owner
            .stream
            .update(cx, ConversationStream::invalidate_file_index);
    }
}
