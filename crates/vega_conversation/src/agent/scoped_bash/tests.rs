use super::*;
use crate::command_scope::tests::Fixture;

#[tokio::test]
async fn scoped_runtime_requires_durable_consumption_before_any_launch_attempt() {
    use crate::agent::{PermissionHook, RejectPermissionHook};
    use futures::FutureExt;
    struct ExplicitScope;
    impl PermissionHook for ExplicitScope {
        fn request(
            &self,
            _: crate::types::PermissionRequest,
            _: CancellationToken,
        ) -> futures::future::BoxFuture<
            'static,
            Result<crate::types::PermissionDecision, vega_runtime::VegaError>,
        > {
            async { Ok(crate::types::PermissionDecision::Once) }.boxed()
        }
        fn request_scope(
            &self,
            _: CommandScopeRequest,
            _: CancellationToken,
        ) -> futures::future::BoxFuture<
            'static,
            Result<CommandScopeDecision, vega_runtime::VegaError>,
        > {
            async { Ok(CommandScopeDecision::AllowOnce) }.boxed()
        }
    }
    let fixture = Fixture::new("execute", "confirm");
    let protected = fixture.root.join("protected");
    std::fs::create_dir(&protected).expect("protected fixture");
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = reads.clone();
    let tools = fixture.tools.clone().with_credential_policy(
        vega_tools::ToolCredentialPolicy::new(
            protected,
            Vec::new(),
            Arc::new(move |_| {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(vega_credentials::CredentialError::Unavailable)
            }),
        )
        .expect("policy")
        .with_bindings_reader(Arc::new(|| Ok(Vec::new()))),
    );
    let authority = ScopedBashAuthority {
        database_path: fixture
            .store
            .database_path()
            .expect("fixture database path")
            .to_path_buf(),
        fence: fixture.accepted.permission_fence.clone(),
        run_id: fixture.accepted.assistant_message_id.clone(),
    };
    let prepared = fixture.call("no-scope-listener", "printf fixture");
    let result = authority
        .execute(
            &RejectPermissionHook,
            "no-scope-listener".into(),
            tools.clone(),
            prepared,
            vega_tools::BashLaunchPolicy::WorkspaceWrite,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        result.expect_err("no listener cannot launch").code(),
        vega_tools::BashErrorCode::ScopeDenied
    );
    assert_eq!(fixture.state("no-scope-listener"), "timeout");
    let prepared = fixture.call("failed-consumption", "printf fixture");
    fixture.store.conn().execute_batch("CREATE TRIGGER fail_runtime_consume BEFORE UPDATE ON command_scope_audits WHEN NEW.state='consumed' BEGIN SELECT RAISE(ABORT,'synthetic dispatch fault'); END;").expect("consumption fault");
    let result = authority
        .execute(
            &ExplicitScope,
            "failed-consumption".into(),
            tools,
            prepared,
            vega_tools::BashLaunchPolicy::WorkspaceWrite,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        result.expect_err("no consumption cannot launch").code(),
        vega_tools::BashErrorCode::ScopeMismatch
    );
    assert_eq!(fixture.state("failed-consumption"), "failed");
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 0);
}
