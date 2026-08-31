use super::*;

#[tokio::test]
async fn permission_queue_notifies_after_enqueue_without_holding_its_mutex() {
    let queue = PermissionQueue::new();
    let mut listener = queue.subscribe();

    let future = queue.request(
        permission_request("write", "src/lib.rs"),
        CancellationToken::new(),
    );
    assert!(
        queue.has_pending(),
        "controller guard sees queued authority"
    );
    assert!(listener.changed().await);
    let pending = queue.take_pending().unwrap();
    let (_, mut lease) = pending.into_parts().unwrap();
    assert!(lease.respond(PermissionDecision::Once));
    assert_eq!(future.await.unwrap(), PermissionDecision::Once);
    assert!(!queue.has_pending(), "terminal decision clears the guard");
    drop(listener);
}

#[tokio::test]
async fn permission_queue_fails_closed_without_a_live_notifier() {
    let queue = PermissionQueue::new();
    let decision = queue
        .request(
            permission_request("bash", "printf ok"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(decision, PermissionDecision::Timeout);
    assert!(queue.take_pending().is_none());

    let listener = queue.subscribe();
    drop(listener);
    let decision = queue
        .request(
            permission_request("edit", "src/lib.rs"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(decision, PermissionDecision::Timeout);
    assert!(queue.take_pending().is_none());
}

#[tokio::test]
async fn permission_queue_drop_paths_and_replacement_are_first_wins() {
    let queue = PermissionQueue::new();
    let listener = queue.subscribe();

    let first = queue.request(
        permission_request("write", "first.txt"),
        CancellationToken::new(),
    );
    let second = queue.request(
        permission_request("write", "second.txt"),
        CancellationToken::new(),
    );
    assert_eq!(first.await.unwrap(), PermissionDecision::Timeout);
    let (_, mut lease) = queue.take_pending().unwrap().into_parts().unwrap();
    assert!(lease.respond(PermissionDecision::Always));
    assert!(!lease.respond(PermissionDecision::Once));
    assert_eq!(second.await.unwrap(), PermissionDecision::Always);

    let dropped_pending = queue.request(
        permission_request("edit", "src/lib.rs"),
        CancellationToken::new(),
    );
    drop(queue.take_pending().unwrap());
    assert_eq!(dropped_pending.await.unwrap(), PermissionDecision::Timeout);

    let dropped_lease = queue.request(
        permission_request("bash", "printf ok"),
        CancellationToken::new(),
    );
    drop(queue.take_pending().unwrap().into_parts().unwrap().1);
    assert_eq!(dropped_lease.await.unwrap(), PermissionDecision::Timeout);
    drop(listener);
}

#[tokio::test]
async fn replacing_subscription_rejects_an_already_taken_old_card() {
    let queue = PermissionQueue::new();
    let old_listener = queue.subscribe();
    let waiting = queue.request(
        permission_request("write", "src/lib.rs"),
        CancellationToken::new(),
    );
    let (_, mut old_lease) = queue.take_pending().unwrap().into_parts().unwrap();

    let new_listener = queue.subscribe();
    assert_eq!(waiting.await.unwrap(), PermissionDecision::Timeout);
    assert!(old_lease.is_resolved());
    assert!(!old_lease.respond(PermissionDecision::Always));
    assert!(queue.take_pending().is_none());

    let new_waiting = queue.request(
        permission_request("edit", "src/lib.rs"),
        CancellationToken::new(),
    );
    let (_, mut new_lease) = queue.take_pending().unwrap().into_parts().unwrap();
    drop(old_listener);
    assert!(!new_lease.is_resolved());
    assert!(new_lease.respond(PermissionDecision::Once));
    assert_eq!(new_waiting.await.unwrap(), PermissionDecision::Once);
    drop(new_listener);
}

#[tokio::test]
async fn permission_queue_future_drop_and_cancellation_clear_stale_cards() {
    let queue = PermissionQueue::new();
    let listener = queue.subscribe();

    let never_polled = queue.request(
        permission_request("write", "src/lib.rs"),
        CancellationToken::new(),
    );
    let (_, mut lease) = queue.take_pending().unwrap().into_parts().unwrap();
    drop(never_polled);
    assert!(lease.is_resolved());
    assert!(!lease.respond(PermissionDecision::Once));
    assert!(queue.take_pending().is_none());

    let mut polled = queue.request(
        permission_request("write", "src/main.rs"),
        CancellationToken::new(),
    );
    assert!(futures::poll!(&mut polled).is_pending());
    let (_, mut polled_lease) = queue.take_pending().unwrap().into_parts().unwrap();
    drop(polled);
    assert!(polled_lease.is_resolved());
    assert!(!polled_lease.respond(PermissionDecision::Always));

    let cancel = CancellationToken::new();
    let cancelled = queue.request(permission_request("bash", "printf ok"), cancel.clone());
    cancel.cancel();
    assert_eq!(cancelled.await.unwrap(), PermissionDecision::Timeout);
    assert!(queue.take_pending().is_none());

    let unresolved = queue.request(
        permission_request("edit", "src/lib.rs"),
        CancellationToken::new(),
    );
    drop(listener);
    assert_eq!(unresolved.await.unwrap(), PermissionDecision::Timeout);
    assert!(queue.take_pending().is_none());
}

#[tokio::test]
async fn permission_queue_rejects_malformed_or_cross_tool_danger_requests() {
    let queue = PermissionQueue::new();
    let listener = queue.subscribe();
    let mut request = permission_request("write", "src/lib.rs");
    request.danger_rule_id = Some("danger".into());
    request.danger_reason = Some("not legal for write".into());
    assert_eq!(
        queue
            .request(request, CancellationToken::new())
            .await
            .unwrap(),
        PermissionDecision::Timeout
    );
    assert!(queue.take_pending().is_none());
    drop(listener);
}
