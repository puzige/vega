use super::*;

#[test]
fn requested_scope_is_strict_and_preserves_exact_command_and_timeout() {
    let project = TempRoot::create().expect("project");
    let tools = Tools::new(project.path()).expect("tools");
    let exact = "  printf '%s' 'fixture'\n";
    let input = serde_json::json!({"cmd":exact,"timeout_ms":1234,"requested_scope":{"network":true,"reason":"Fetch requested content"}}).to_string();
    let prepared = tools.prepare_bash_json(&input).expect("valid request");
    assert_eq!(prepared.command(), exact);
    assert_eq!(prepared.timeout_ms(), 1234);
    assert!(prepared.requested_scope().expect("scope").network());
    assert!(bash_inputs_semantically_equal(
        &input,
        &prepared.input_json()
    ));
    assert!(!bash_inputs_semantically_equal(
        &input,
        &serde_json::json!({"cmd":exact,"timeout_ms":1234}).to_string()
    ));
    assert!(!bash_inputs_semantically_equal(&input, &serde_json::json!({"cmd":exact,"timeout_ms":1234,"requested_scope":{"network":true,"reason":"Different reason"}}).to_string()));
    for scope in [
        serde_json::json!({}),
        serde_json::json!(null),
        serde_json::json!({"network":true}),
        serde_json::json!({"network":true,"reason":""}),
        serde_json::json!({"network":true,"reason":"line\nbreak"}),
        serde_json::json!({"network":true,"reason":"ok","approved":true}),
        serde_json::json!({"write_roots":["relative"],"reason":"ok"}),
        serde_json::json!({"write_roots":["/tmp/./fixture"],"reason":"ok"}),
        serde_json::json!({"write_roots":["/tmp/../fixture"],"reason":"ok"}),
    ] {
        assert_eq!(
            bash_input_error(
                &serde_json::json!({"command":"pwd","requested_scope":scope}).to_string()
            ),
            Some(BashInputError::InvalidScope)
        );
    }
    assert!(tools.prepare_bash_json(r#"{"command":"pwd","requested_scope":{"network":true,"network":false,"reason":"duplicate"}}"#).is_err());
    project.cleanup().expect("cleanup");
}

#[tokio::test]
async fn a_model_scope_request_cannot_use_the_unapproved_execution_entry() {
    let project = TempRoot::create().expect("project");
    let tools = Tools::new(project.path()).expect("tools");
    for policy in [BashLaunchPolicy::ReadOnly, BashLaunchPolicy::WorkspaceWrite] {
        let prepared = tools.prepare_bash_json(r#"{"command":"printf fixture","requested_scope":{"network":true,"reason":"request only"}}"#).expect("request");
        assert_eq!(
            tools
                .execute_bash(prepared, policy, CancellationToken::new())
                .await
                .expect_err("no scope authorization")
                .code(),
            BashErrorCode::ScopeMismatch
        );
    }
    assert_eq!(
        std::fs::read_dir(project.path())
            .expect("unchanged fixture")
            .count(),
        0
    );
    project.cleanup().expect("cleanup");
}

#[test]
fn directory_scope_rejects_readonly_protected_and_replaced_roots() {
    let project = TempRoot::create().expect("project");
    let extra = TempRoot::create().expect("extra");
    let protected = TempRoot::create().expect("protected");
    let tools = Tools::new(project.path())
        .expect("tools")
        .with_credential_policy(
            crate::ToolCredentialPolicy::new(
                protected.path().into(),
                Vec::new(),
                Arc::new(|_| Err(CredentialError::Unavailable)),
            )
            .expect("policy"),
        );
    let prepare = |root: &std::path::Path| {
        tools.prepare_bash_json(&serde_json::json!({"command":"pwd","requested_scope":{"write_roots":[root],"reason":"write output"}}).to_string()).expect("request")
    };
    let prepared = prepare(extra.path());
    let request = prepared.requested_scope().expect("scope").clone();
    assert!(
        tools
            .prepare_bash_scope(
                &prepared,
                BashLaunchPolicy::ReadOnly,
                request.clone(),
                Vec::new()
            )
            .is_err()
    );
    assert!(
        tools
            .prepare_bash_scope(
                &prepared,
                BashLaunchPolicy::FullAccess,
                request.clone(),
                Vec::new()
            )
            .is_err()
    );
    let scope = tools
        .prepare_bash_scope(
            &prepared,
            BashLaunchPolicy::WorkspaceWrite,
            request,
            Vec::new(),
        )
        .expect("explicit narrow directory");
    assert_eq!(scope.directories()[0].path(), extra.path());
    for root in [
        std::path::Path::new("/"),
        protected.path(),
        protected.path().parent().expect("parent"),
        &project.path().join(".git"),
    ] {
        let prepared = prepare(root);
        assert!(
            tools
                .prepare_bash_scope(
                    &prepared,
                    BashLaunchPolicy::WorkspaceWrite,
                    prepared.requested_scope().expect("scope").clone(),
                    Vec::new()
                )
                .is_err()
        );
    }
    let nested = extra.path().join("target");
    std::fs::create_dir(&nested).expect("nested");
    let prepared = prepare(&nested);
    let scope = tools
        .prepare_bash_scope(
            &prepared,
            BashLaunchPolicy::WorkspaceWrite,
            prepared.requested_scope().expect("scope").clone(),
            Vec::new(),
        )
        .expect("identity");
    std::fs::rename(&nested, extra.path().join("original")).expect("replace");
    std::fs::create_dir(&nested).expect("replacement");
    assert!(
        scope
            .validate(&prepared, BashLaunchPolicy::WorkspaceWrite)
            .is_err()
    );
    extra.cleanup().expect("extra cleanup");
    protected.cleanup().expect("protected cleanup");
    project.cleanup().expect("project cleanup");
}
