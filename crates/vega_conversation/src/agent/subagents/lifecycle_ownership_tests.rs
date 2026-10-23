use super::*;

#[tokio::test]
async fn dropping_provider_or_release_future_quarantines_without_source_normalization() {
    for release_pending in [false, true] {
        let fixture = Fixture::new();
        let provider = if release_pending {
            ScriptedProvider::with(vec![final_response(
                "Completed content awaiting host release.",
            )])
        } else {
            ScriptedProvider::default()
        };
        let host = TestHost::new(false);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let (sent, received) = oneshot::channel();
        let mut running = Box::pin(
            supervisor.run(
                async {
                    let child = spawn(
                        &hook,
                        &observer,
                        "Keep writer ownership until confirmed release.",
                    )
                    .await;
                    sent.send(child).expect("child receipt");
                    futures::future::pending::<()>().await;
                }
                .boxed_local(),
                fixture.launch(&provider, host.clone()),
            ),
        );
        bounded(async {
            tokio::select! {
                _ = &mut running => panic!("parent must remain pending"),
                _ = async { provider.request_count(1).await; if release_pending { host.await_release().await; } } => {},
            }
        }).await;
        let child = received.await.expect("admitted child");
        let before =
            vega_store::context_compaction::load_source(fixture.store.conn(), &child.agent_id)
                .expect("source before drop");
        drop(running);
        assert!(
            host.cancel().is_cancelled(),
            "abnormal drop must signal the original owner"
        );
        assert_eq!(
            vega_store::delegated_stages::release_state(
                fixture.store.conn(),
                &child.agent_id,
                &child.message_id
            )
            .expect("release state"),
            "quarantined"
        );
        assert!(
            DelegatedOwnership::acquire(&fixture.database, &child.agent_id).is_err(),
            "actual native ownership proof must outlive dropped future"
        );
        let _ = crate::history::restart_history_page(&fixture.store, "parent", 20);
        let _ = crate::history::restart_history_page(&fixture.store, &child.agent_id, 20);
        let prepare = super::super::super::pipeline::prepare_run_with_images_and_reasoning(
            fixture.database.clone(),
            child.agent_id.clone(),
            "must not replace writer".into(),
            "system".into(),
            "drop-user".into(),
            "drop-run".into(),
            PersistenceActorConfig::default(),
            false,
            None,
            None,
            Vec::new(),
        );
        assert!(prepare.is_err());
        new_parent_run(&fixture, "replacement-parent");
        let replacement = ScriptedProvider::with(vec![final_response("must never execute")]);
        let replacement_host = TestHost::new(true);
        let mut restored = Supervisor::new();
        let restored_hook = restored.hook();
        let mut launch = fixture.launch(&replacement, replacement_host);
        launch.parent_message = "replacement-parent".into();
        let admission = restored
            .run(
                async {
                    bounded(restored_hook.call(
                        "replacement".into(),
                        "spawn_agent".into(),
                        json!({"name":"replacement","task":"must stay blocked"}),
                        CancellationToken::new(),
                    ))
                    .await
                }
                .boxed_local(),
                launch,
            )
            .await;
        assert!(
            admission.is_err(),
            "new Supervisor must retain the quarantine"
        );
        assert_eq!(replacement.count(), 0);
        let after =
            vega_store::context_compaction::load_source(fixture.store.conn(), &child.agent_id)
                .expect("source after recovery attempts");
        assert_eq!(before.source_version, after.source_version);
        assert_eq!(before.fingerprint, after.fingerprint);
        assert_eq!(
            vega_store::delegated_stages::release_state(
                fixture.store.conn(),
                &child.agent_id,
                &child.message_id
            )
            .expect("durable quarantine"),
            "quarantined"
        );
    }
}
