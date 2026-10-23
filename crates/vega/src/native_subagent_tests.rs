use super::*;
use std::task::{Context, Poll, Waker};
use tokio_util::sync::CancellationToken;
use vega_conversation::types::SubagentState;

fn admission() -> SubagentAdmission {
    let thread = vega_conversation::threads::draft_thread(None, "fixture-model", "confirm");
    let snapshot = SubagentSnapshot {
        agent_id: thread.id.clone(),
        parent_thread_id: "parent".into(),
        parent_message_id: "parent-run".into(),
        message_id: "child-run".into(),
        name: "child".into(),
        state: SubagentState::Running,
        result: String::new(),
        truncated: false,
        checkpoint: None,
        release_state: "held".into(),
        ready_for_review: false,
    };
    SubagentAdmission {
        thread,
        snapshot,
        cancel: CancellationToken::new(),
        permission_queue: vega_conversation::agent::PermissionQueue::new(),
        user_questions: vega_conversation::agent::UserQuestionQueue::new(),
        task: "keep ownership through acknowledgement".into(),
        queue_generation: "queue".into(),
    }
}

#[test]
fn native_accepted_admission_cancellation_waits_for_ack_then_terminal_release() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let (sender, receiver) = mpsc::sync_channel(8);
        let host = NativeSubagentHost::new(sender);
        let request = admission();
        let cancel = request.cancel.clone();
        let mut terminal = request.snapshot.clone();
        let mut pending = host.admit_with_ownership(request);
        let AgentUpdate::Subagent(NativeSubagentUpdate::Admit {
            admission: accepted,
            reply,
        }) = receiver.try_recv().expect("native UI admission delivery")
        else {
            panic!("expected actual Admit update");
        };
        assert_eq!(accepted.snapshot.message_id, terminal.message_id);
        cancel.cancel();
        let before_ack = pending
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()));
        assert!(
            matches!(before_ack, Poll::Pending),
            "cancel after delivery cannot manufacture a rejection before UI acknowledgement"
        );
        reply.send(Ok(())).expect("UI accepted acknowledgement");
        assert!(matches!(
            tokio::time::timeout(std::time::Duration::from_secs(3), pending)
                .await
                .expect("ack"),
            SubagentAdmissionOutcome::Accepted
        ));
        assert!(
            host.finish_and_release(terminal.clone(), false)
                .await
                .is_err(),
            "Running is not a release result"
        );
        assert!(host.finished(terminal.clone(), false).is_err());
        assert!(receiver.try_recv().is_err());
        terminal.state = SubagentState::Interrupted;
        let mut release = host.finish_and_release(terminal, false);
        let AgentUpdate::Subagent(NativeSubagentUpdate::FinishedAndRelease {
            snapshot, reply, ..
        }) = receiver.try_recv().expect("native release delivery")
        else {
            panic!("expected terminal cleanup");
        };
        assert_eq!(snapshot.state, SubagentState::Interrupted);
        assert!(
            release
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        reply.send(Ok(())).expect("UI cleanup acknowledgement");
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(3), release)
                .await
                .expect("cleanup")
                .is_ok()
        );
    });
}

#[test]
fn native_admission_distinguishes_confirmed_rejection_from_missing_acknowledgement() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        for rejected in [true, false] {
            let (sender, receiver) = mpsc::sync_channel(8);
            let host = NativeSubagentHost::new(sender);
            let pending = host.admit_with_ownership(admission());
            let AgentUpdate::Subagent(NativeSubagentUpdate::Admit { reply, .. }) =
                receiver.try_recv().expect("native admission")
            else {
                panic!("expected Admit");
            };
            if rejected {
                reply.send(Err(VegaError::Cancelled)).expect("UI rejection");
            } else {
                drop(reply);
            }
            let result = tokio::time::timeout(std::time::Duration::from_secs(3), pending)
                .await
                .expect("result");
            assert!(matches!(
                (rejected, result),
                (true, SubagentAdmissionOutcome::Rejected(_))
                    | (false, SubagentAdmissionOutcome::Unknown(_))
            ));
        }
    });
}
