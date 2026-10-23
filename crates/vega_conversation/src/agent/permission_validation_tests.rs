use super::*;
use sha2::{Digest, Sha256};

fn request(tool: &str, target: &str) -> PermissionRequest {
    PermissionRequest {
        call_id: "artifact-validation-looking-id".into(),
        tool: tool.into(),
        display_target: target.into(),
        danger_rule_id: None,
        danger_reason: None,
        external: None,
        acp_options: None,
    }
}

#[tokio::test]
async fn ordinary_permission_requires_exact_tool_card_before_human_decision() {
    let queue = PermissionQueue::new();
    let _listener = queue.subscribe();
    let waiting = PermissionHook::request(
        &queue,
        request("read", "/tmp/example"),
        CancellationToken::new(),
    );
    let pending = queue.take_pending().expect("pending normal Read");
    assert_eq!(
        pending.presentation(None),
        PermissionPresentation::WaitingForTool
    );
    assert_eq!(
        pending.presentation(Some(("write", "/tmp/example"))),
        PermissionPresentation::Mismatch
    );
    assert_eq!(
        pending.presentation(Some(("read", "/tmp/other"))),
        PermissionPresentation::Mismatch
    );
    assert_eq!(
        pending.presentation(Some(("read", "/tmp/example"))),
        PermissionPresentation::Ready
    );
    assert!(pending.allows_persistent_permission());
    let (_, mut lease) = pending.into_parts().expect("permission lease");
    assert!(lease.respond(PermissionDecision::Always));
    assert_eq!(waiting.await.expect("decision"), PermissionDecision::Always);
    assert!(!queue.has_pending());
}

#[tokio::test]
async fn runtime_artifact_read_uses_one_shot_presenter_and_real_permission_decision() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace directory");
    let artifact = directory.path().join("external.txt");
    let content = b"verified raw bytes\r\n";
    std::fs::write(&artifact, content).expect("artifact");
    let artifact = artifact.canonicalize().expect("canonical artifact");
    let tools = vega_tools::Tools::new(&workspace).expect("tools");
    let path = artifact.to_str().expect("path");
    let config = RuntimeToolConfig::new(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::Confirm,
        "project".into(),
        "child".into(),
        directory.path().join("checkpoints"),
        Vec::new(),
    );
    for decision in [
        Some(PermissionDecision::Once),
        Some(PermissionDecision::Deny { note: None }),
        Some(PermissionDecision::Always),
        None,
    ] {
        let queue = PermissionQueue::new();
        let _listener = queue.subscribe();
        let cancel = CancellationToken::new();
        let adapter = ArtifactReadPermissionAdapter { shared: &queue };
        let future =
            vega_runtime::read_identity_with_permission(&tools, &config, path, &adapter, &cancel);
        tokio::pin!(future);
        assert!(futures::poll!(future.as_mut()).is_pending());
        let pending = queue.take_pending().expect("runtime validation permission");
        assert_eq!(pending.presentation(None), PermissionPresentation::Ready);
        assert!(!pending.allows_persistent_permission());
        let (prompt, mut lease) = pending.into_parts().expect("validation lease");
        assert_eq!(prompt.tool, "read");
        assert_eq!(prompt.display_target, path);
        let approved = decision == Some(PermissionDecision::Once);
        if let Some(decision) = decision {
            assert!(lease.respond(decision));
        } else {
            cancel.cancel();
        }
        let result = tokio::time::timeout(Duration::from_secs(3), future)
            .await
            .expect("validation must resolve");
        if approved {
            let identity = result.expect("approved Read");
            assert!(identity.complete);
            assert_eq!(identity.canonical_path, path);
            assert_eq!(identity.sha256, format!("{:x}", Sha256::digest(content)));
        } else {
            assert!(
                result.is_err(),
                "denied, cancelled and Always validation cannot authorize Read"
            );
        }
        assert!(!queue.has_pending());
    }
}

#[tokio::test]
async fn runtime_validation_cannot_grant_tool_or_relative_path_presentation_exemption() {
    for prompt in [
        request("write", "/tmp/example"),
        request("read", "relative.txt"),
        PermissionRequest {
            danger_rule_id: Some("danger".into()),
            danger_reason: Some("reason".into()),
            ..request("read", "/tmp/example")
        },
    ] {
        let queue = PermissionQueue::new();
        let _listener = queue.subscribe();
        let decision = queue
            .request_artifact_validation(prompt, CancellationToken::new())
            .await
            .expect("rejected validation");
        assert_eq!(decision, PermissionDecision::Timeout);
        assert!(queue.take_pending().is_none());
        assert!(!queue.has_pending());
    }
}
