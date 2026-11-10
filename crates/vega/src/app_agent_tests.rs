use super::{AGENT_EVENT_BATCH, AgentUpdate, drain_agent_updates};
use std::sync::mpsc;
use vega_conversation::types::ConversationEvent;

fn failure() -> ConversationEvent {
    ConversationEvent::Error {
        message_id: Some("failed-message".into()),
        error: vega_runtime::VegaError::RuntimeFailure(vega_runtime::RuntimeFailureDetail::new(
            vega_runtime::RuntimeFailureReason::ApprovalAudit,
        ))
        .into(),
        execution_duration_ms: None,
    }
}

#[test]
fn error_does_not_release_worker_until_cleanup_finishes() {
    let (sender, receiver) = mpsc::channel();
    sender.send(AgentUpdate::Event(failure())).unwrap();
    let batch = drain_agent_updates(&receiver);
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.finished, None);
    sender
        .send(AgentUpdate::Finished {
            success: false,
            reference_failure: None,
            credential_failure: false,
        })
        .unwrap();
    assert_eq!(drain_agent_updates(&receiver).finished, Some(false));
}

#[test]
fn disconnected_failed_worker_produces_release_terminal() {
    let (sender, receiver) = mpsc::channel();
    sender.send(AgentUpdate::Event(failure())).unwrap();
    drop(sender);
    let batch = drain_agent_updates(&receiver);
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.finished, Some(false));
}

#[test]
fn full_batch_does_not_lose_failed_terminal() {
    let (sender, receiver) = mpsc::channel();
    for _ in 0..AGENT_EVENT_BATCH {
        sender.send(AgentUpdate::CodexModeConfirmed).unwrap();
    }
    sender.send(AgentUpdate::Event(failure())).unwrap();
    sender
        .send(AgentUpdate::Finished {
            success: false,
            reference_failure: None,
            credential_failure: false,
        })
        .unwrap();
    assert_eq!(drain_agent_updates(&receiver).finished, None);
    let terminal = drain_agent_updates(&receiver);
    assert_eq!(terminal.events.len(), 1);
    assert_eq!(terminal.finished, Some(false));
}
