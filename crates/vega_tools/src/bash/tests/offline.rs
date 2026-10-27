use super::*;

#[tokio::test]
async fn offline_restricted_bindings_stay_strict_without_proxy_artifacts() {
    let project = TempRoot::create().expect("project fixture");
    let temporary = TempRoot::create().expect("command temp fixture");
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    let binding = CredentialBinding {
        key_ref: "fixture-ref".into(),
        env: "FIXTURE_TOKEN".into(),
        origin: "https://fixture.invalid".into(),
        header: "Authorization".into(),
        prefix: "Bearer ".into(),
        path: "/api".into(),
    };
    let policy = crate::ToolCredentialPolicy::new(
        project.path().into(),
        vec![binding.clone()],
        Arc::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Err(CredentialError::Unavailable)
        }),
    )
    .expect("policy");
    let tools = Tools::new(project.path())
        .expect("tools")
        .with_credential_policy(policy);
    let prepared = tools
        .prepare_bash_json(r#"{"command":"pwd","timeout":null}"#)
        .expect("prepared");
    for launch_policy in [BashLaunchPolicy::ReadOnly, BashLaunchPolicy::WorkspaceWrite] {
        let failure = match tools
            .prepare_bash_launch(
                &prepared,
                launch_policy,
                &CancellationToken::new(),
                &temporary,
            )
            .await
        {
            Err(failure) => failure,
            Ok(_) => panic!("unavailable binding accepted"),
        };
        assert_eq!(failure.error.code(), BashErrorCode::SandboxUnavailable);
        assert!(failure.cleanup_safe);
    }
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    let valid = crate::ToolCredentialPolicy::new(
        project.path().into(),
        vec![binding.clone()],
        Arc::new(|_| Ok("synthetic-fixture".into())),
    )
    .expect("valid policy");
    valid
        .validate_bindings()
        .expect("strict read without Broker");
    let duplicate = crate::ToolCredentialPolicy::new(
        project.path().into(),
        vec![binding.clone(), binding],
        Arc::new(|_| Ok("synthetic-fixture".into())),
    )
    .expect("syntactically valid bindings");
    assert!(duplicate.validate_bindings().is_err());
    assert_eq!(
        std::fs::read_dir(temporary.path())
            .expect("temp entries")
            .count(),
        0
    );
    temporary.cleanup().expect("temp cleanup");
    project.cleanup().expect("project cleanup");
}
