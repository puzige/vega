use crate::app_agent::AgentUpdate;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, mpsc},
};
use tokio::sync::oneshot;
use vega_conversation::types::{
    ConversationEvent, SubagentAdmission, SubagentHost, SubagentSnapshot,
};
use vega_runtime::VegaError;

pub(crate) enum NativeSubagentUpdate {
    Admit {
        admission: Box<SubagentAdmission>,
        reply: oneshot::Sender<Result<(), VegaError>>,
    },
    Event {
        thread_id: String,
        message_id: String,
        event: ConversationEvent,
    },
    Updated(SubagentSnapshot),
    Finished {
        snapshot: SubagentSnapshot,
        usage_complete: bool,
    },
}

pub(crate) struct NativeSubagentHost {
    sender: mpsc::SyncSender<AgentUpdate>,
}

impl NativeSubagentHost {
    pub(crate) fn new(sender: mpsc::SyncSender<AgentUpdate>) -> Arc<Self> {
        Arc::new(Self { sender })
    }
    fn send(&self, update: NativeSubagentUpdate) -> Result<(), VegaError> {
        self.sender
            .send(AgentUpdate::Subagent(update))
            .map_err(|_| unavailable())
    }
}

fn unavailable() -> VegaError {
    VegaError::Tool {
        tool: "subagent".into(),
        message: "conversation owner unavailable".into(),
    }
}

impl SubagentHost for NativeSubagentHost {
    fn admit(
        &self,
        admission: SubagentAdmission,
    ) -> Pin<Box<dyn Future<Output = Result<(), VegaError>> + Send + 'static>> {
        let (reply, receiver) = oneshot::channel();
        let cancel = admission.cancel.clone();
        let sent = self.send(NativeSubagentUpdate::Admit {
            admission: Box::new(admission),
            reply,
        });
        Box::pin(async move {
            sent?;
            tokio::select! {biased; _ = cancel.cancelled() => Err(VegaError::Cancelled), result = receiver => result.map_err(|_| unavailable())?}
        })
    }
    fn event(
        &self,
        thread_id: &str,
        message_id: &str,
        event: &ConversationEvent,
    ) -> Result<(), VegaError> {
        self.send(NativeSubagentUpdate::Event {
            thread_id: thread_id.into(),
            message_id: message_id.into(),
            event: event.clone(),
        })
    }
    fn updated(&self, snapshot: SubagentSnapshot) -> Result<(), VegaError> {
        self.send(NativeSubagentUpdate::Updated(snapshot))
    }
    fn finished(&self, snapshot: SubagentSnapshot, usage_complete: bool) -> Result<(), VegaError> {
        self.send(NativeSubagentUpdate::Finished {
            snapshot,
            usage_complete,
        })
    }
}
