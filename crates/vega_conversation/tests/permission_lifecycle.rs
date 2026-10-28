use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{PermissionHook, PermissionQueue};
use vega_conversation::types::{PermissionDecision, PermissionRequest};

fn request() -> PermissionRequest {
    PermissionRequest {
        call_id: "call".into(),
        tool: "bash".into(),
        display_target: "echo hello".into(),
        danger_rule_id: None,
        danger_reason: None,
        external: None,
        acp_options: None,
    }
}

#[tokio::test]
async fn lost_listener_and_missing_listener_are_unavailable_not_timeout() {
    let queue = PermissionQueue::new();
    assert_eq!(
        queue
            .request(request(), CancellationToken::new())
            .await
            .unwrap(),
        PermissionDecision::Unavailable
    );
    let listener = queue.subscribe();
    let response = queue.request(request(), CancellationToken::new());
    drop(listener);
    assert_eq!(response.await.unwrap(), PermissionDecision::Unavailable);
    assert!(!queue.has_pending());
}

#[tokio::test]
async fn cancellation_resolves_pending_request_without_authorizing() {
    let queue = PermissionQueue::new();
    let _listener = queue.subscribe();
    let cancel = CancellationToken::new();
    let response = queue.request(request(), cancel.clone());
    cancel.cancel();
    assert_eq!(response.await.unwrap(), PermissionDecision::Cancelled);
    assert!(!queue.has_pending());
}

#[tokio::test]
async fn first_response_survives_later_cancellation_and_listener_drop() {
    for decision in [
        PermissionDecision::Timeout,
        PermissionDecision::Unavailable,
        PermissionDecision::Deny { note: None },
    ] {
        let queue = PermissionQueue::new();
        let listener = queue.subscribe();
        let cancel = CancellationToken::new();
        let response = queue.request(request(), cancel.clone());
        let (_, mut lease) = queue.take_pending().unwrap().into_parts().unwrap();
        assert!(lease.respond(decision.clone()));
        assert!(!lease.respond(PermissionDecision::Once));
        cancel.cancel();
        drop(listener);
        assert_eq!(response.await.unwrap(), decision);
        assert!(!queue.has_pending());
    }
}
