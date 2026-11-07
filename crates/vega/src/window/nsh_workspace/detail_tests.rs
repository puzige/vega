use super::{AgentIdentity, PanelReadOwner, SubagentDetail};

fn owner(identity: Option<AgentIdentity>) -> PanelReadOwner {
    PanelReadOwner {
        parent: "parent".into(),
        epoch: 3,
        selection: 5,
        child: "child".into(),
        identity,
    }
}

fn identity(message: &str) -> AgentIdentity {
    AgentIdentity {
        child: "child".into(),
        parent: "parent".into(),
        parent_message: "parent-message".into(),
        message: message.into(),
        generation: Some(2),
        operation: Some("operation".into()),
    }
}

fn pending() -> SubagentDetail {
    SubagentDetail {
        id: "child".into(),
        loading: true,
        error: false,
        model: None,
        stream: None,
    }
}

#[test]
fn panel_population_before_detail_completion_settles_into_retry() {
    let requested = owner(None);
    let populated = owner(Some(identity("message")));
    let mut detail = pending();
    assert!(!detail.accept_completion(&requested, Some(&populated)));
    assert!(!detail.loading);
    assert!(detail.error);
    assert!(detail.stream.is_none());
    assert!(detail.model.is_none());
    let mut retry = populated.clone();
    retry.selection += 1;
    detail = pending();
    assert!(!detail.accept_completion(&requested, Some(&retry)));
    assert!(detail.loading);
    assert!(!detail.error);
    assert!(detail.accept_completion(&retry, Some(&retry)));
}

#[test]
fn message_or_operation_replacement_during_read_rejects_content_and_recovers() {
    let requested = owner(Some(identity("original")));
    for replacement in [
        identity("replacement"),
        AgentIdentity {
            operation: Some("replacement-operation".into()),
            ..identity("original")
        },
    ] {
        let current = owner(Some(replacement));
        let mut detail = pending();
        assert!(!detail.accept_completion(&requested, Some(&current)));
        assert!(!detail.loading);
        assert!(detail.error);
        assert!(detail.stream.is_none());
        assert!(detail.model.is_none());
    }
}

#[test]
fn obsolete_completion_cannot_settle_replacement_or_hidden_route() {
    let requested = owner(Some(identity("message")));
    for replacement in [
        PanelReadOwner {
            selection: requested.selection + 1,
            ..requested.clone()
        },
        PanelReadOwner {
            epoch: requested.epoch + 1,
            ..requested.clone()
        },
        PanelReadOwner {
            parent: "replacement-parent".into(),
            ..requested.clone()
        },
        PanelReadOwner {
            child: "replacement-child".into(),
            ..requested.clone()
        },
    ] {
        let mut detail = pending();
        assert!(!detail.accept_completion(&requested, Some(&replacement)));
        assert!(detail.loading);
        assert!(!detail.error);
    }
    let mut detail = pending();
    assert!(!detail.accept_completion(&requested, None));
    assert!(detail.loading);
    assert!(!detail.error);
}

#[test]
fn unchanged_pending_identity_is_accepted_before_panel_population() {
    let requested = owner(None);
    let mut detail = pending();
    assert!(detail.accept_completion(&requested, Some(&requested)));
    assert!(!detail.error);
}
