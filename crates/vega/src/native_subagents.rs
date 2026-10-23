use crate::app_agent::AgentUpdate;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, mpsc},
};
use tokio::sync::oneshot;
use vega_conversation::types::{
    ConversationEvent, SubagentAdmission, SubagentAdmissionOutcome, SubagentHost, SubagentSnapshot,
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
    FinishedAndRelease {
        snapshot: SubagentSnapshot,
        usage_complete: bool,
        reply: oneshot::Sender<Result<(), VegaError>>,
    },
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
        let pending = self.admit_with_ownership(admission);
        Box::pin(async move {
            match pending.await {
                SubagentAdmissionOutcome::Accepted => Ok(()),
                SubagentAdmissionOutcome::Rejected(error)
                | SubagentAdmissionOutcome::Unknown(error) => Err(error),
            }
        })
    }
    fn admit_with_ownership(
        &self,
        admission: SubagentAdmission,
    ) -> Pin<Box<dyn Future<Output = SubagentAdmissionOutcome> + Send + 'static>> {
        let (reply, receiver) = oneshot::channel();
        let sent = self.send(NativeSubagentUpdate::Admit {
            admission: Box::new(admission),
            reply,
        });
        Box::pin(async move {
            if let Err(error) = sent {
                return SubagentAdmissionOutcome::Rejected(error);
            }
            match receiver.await {
                Ok(Ok(())) => SubagentAdmissionOutcome::Accepted,
                Ok(Err(error)) => SubagentAdmissionOutcome::Rejected(error),
                Err(_) => SubagentAdmissionOutcome::Unknown(unavailable()),
            }
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
    fn finish_and_release(
        &self,
        snapshot: SubagentSnapshot,
        usage_complete: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), VegaError>> + Send + 'static>> {
        if !snapshot.state.is_terminal() {
            return Box::pin(async { Err(unavailable()) });
        }
        let (reply, receiver) = oneshot::channel();
        let sent = self.send(NativeSubagentUpdate::FinishedAndRelease {
            snapshot,
            usage_complete,
            reply,
        });
        Box::pin(async move {
            sent?;
            receiver.await.map_err(|_| unavailable())?
        })
    }
    fn finished(&self, snapshot: SubagentSnapshot, usage_complete: bool) -> Result<(), VegaError> {
        if !snapshot.state.is_terminal() {
            return Err(unavailable());
        }
        self.send(NativeSubagentUpdate::Finished {
            snapshot,
            usage_complete,
        })
    }
}

#[cfg(test)]
#[path = "native_subagent_tests.rs"]
mod tests;
