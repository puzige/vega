use crate::trusted_action::*;
use std::path::PathBuf;
use std::sync::*;

use gpui::*;
use vega_conversation::types::*;
use vega_conversation::*;
use vega_ui::branch_selector::*;
use vega_ui::conversation_stream::*;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct BranchRouteIdentity {
    pub(crate) epoch: u64,
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
    pub(crate) stream: Entity<ConversationStream>,
    pub(crate) selector: Entity<BranchSelector>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct BranchListFence {
    pub(crate) route: BranchRouteIdentity,
    pub(crate) sequence: u64,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct BranchSwitchFence {
    pub(crate) route: BranchRouteIdentity,
    pub(crate) sequence: u64,
    pub(crate) snapshot_generation: u64,
    pub(crate) branch_id: BranchId,
    pub(crate) operation_id: BranchOperationId,
    pub(crate) lease: TrustedActionToken,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct BranchPrepareFence {
    pub(crate) route: BranchRouteIdentity,
    pub(crate) sequence: u64,
    pub(crate) snapshot_generation: u64,
    pub(crate) branch_id: BranchId,
    pub(crate) operation_id: BranchOperationId,
}

pub(crate) struct ActiveBranchRoute {
    pub(crate) identity: BranchRouteIdentity,
    pub(crate) service: Arc<BranchWorkspaceService>,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
    pub(crate) list_sequence: u64,
    pub(crate) list_fence: Option<BranchListFence>,
    pub(crate) list_cancel: Option<tokio_util::sync::CancellationToken>,
    pub(crate) switch_sequence: u64,
    pub(crate) prepare_fence: Option<BranchPrepareFence>,
    pub(crate) switch_fence: Option<BranchSwitchFence>,
    pub(crate) switch_cancel: Option<tokio_util::sync::CancellationToken>,
}

#[derive(Default)]
pub(crate) struct BranchController {
    pub(crate) next_epoch: u64,
    pub(crate) active: Option<ActiveBranchRoute>,
    pub(crate) terminal_fence: Option<BranchSwitchFence>,
    pub(crate) cancelled_prepare: Option<BranchPrepareFence>,
}

impl BranchController {
    pub(crate) fn begin(
        &mut self,
        thread: &Thread,
        stream: Entity<ConversationStream>,
        selector: Entity<BranchSelector>,
        root: PathBuf,
    ) -> Result<BranchRouteIdentity, GitWorkspaceErrorCode> {
        self.close();
        let epoch = self
            .next_epoch
            .checked_add(1)
            .ok_or(GitWorkspaceErrorCode::OutputTooLarge)?;
        let service =
            Arc::new(BranchWorkspaceService::new(root).map_err(|failure| failure.code())?);
        self.next_epoch = epoch;
        let identity = BranchRouteIdentity {
            epoch,
            thread_id: thread.id.clone(),
            project_id: thread.project_id.clone(),
            stream,
            selector,
        };
        self.active = Some(ActiveBranchRoute {
            identity: identity.clone(),
            service,
            cancel: tokio_util::sync::CancellationToken::new(),
            list_sequence: 0,
            list_fence: None,
            list_cancel: None,
            switch_sequence: 0,
            prepare_fence: None,
            switch_fence: None,
            switch_cancel: None,
        });
        Ok(identity)
    }

    pub(crate) fn close(&mut self) -> Option<ActiveBranchRoute> {
        let mut active = self.active.take();
        if let Some(active) = &active {
            active.cancel.cancel();
            if let Some(cancel) = &active.list_cancel {
                cancel.cancel();
            }
            if let Some(cancel) = &active.switch_cancel {
                cancel.cancel();
            }
        }
        if let Some(fence) = active
            .as_mut()
            .and_then(|active| active.switch_fence.take())
            && self.terminal_fence.is_none()
        {
            self.terminal_fence = Some(fence);
        }
        if let Some(fence) = active
            .as_mut()
            .and_then(|active| active.prepare_fence.take())
            && self.cancelled_prepare.is_none()
        {
            self.cancelled_prepare = Some(fence);
        }
        active
    }

    pub(crate) fn claim_prepare(&mut self, fence: &BranchPrepareFence) -> bool {
        if let Some(active) = self.active.as_mut()
            && active.prepare_fence.as_ref() == Some(fence)
        {
            active.prepare_fence = None;
            return true;
        }
        if self.cancelled_prepare.as_ref() == Some(fence) {
            self.cancelled_prepare = None;
            return true;
        }
        false
    }

    pub(crate) fn claim_terminal(&mut self, fence: &BranchSwitchFence) -> bool {
        if let Some(active) = self.active.as_mut()
            && active.switch_fence.as_ref() == Some(fence)
        {
            active.switch_fence = None;
            active.switch_cancel = None;
            return true;
        }
        if self.terminal_fence.as_ref() == Some(fence) {
            self.terminal_fence = None;
            return true;
        }
        false
    }
}

pub(crate) fn run_branch_list_worker(
    service: Arc<BranchWorkspaceService>,
    fence: BranchListFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(
        BranchListFence,
        Result<BranchSnapshot, GitWorkspaceErrorCode>,
    )>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime
                .block_on(service.refresh(cancel))
                .map_err(|failure| failure.code())
        });
    let _ = sender.send((fence, result));
}

pub(crate) fn run_branch_prepare_worker(
    service: Arc<BranchWorkspaceService>,
    fence: BranchPrepareFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(
        BranchPrepareFence,
        Result<BranchSwitchPermit, GitWorkspaceErrorCode>,
    )>,
) {
    let result = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime
            .block_on(service.prepare_switch(fence.branch_id, cancel))
            .map_err(|failure| failure.code()),
        Err(_) => Err(GitWorkspaceErrorCode::SpawnFailed),
    };
    let _ = sender.send((fence, result));
}

pub(crate) fn run_branch_switch_worker(
    service: Arc<BranchWorkspaceService>,
    permit: BranchSwitchPermit,
    fence: BranchSwitchFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(BranchSwitchFence, BranchSwitchCompletion)>,
) {
    let completion = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(service.execute_switch(permit, cancel)),
        Err(_) => BranchSwitchCompletion {
            outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::SpawnFailed),
            snapshot: None,
        },
    };
    let _ = sender.send((fence, completion));
}
