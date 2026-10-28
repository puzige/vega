use super::*;
use vega_conversation::types::ContextPrecomputeLease;
use vega_ui::conversation_stream::ComposerStopRequested;

fn bind(fixture: &Fixture, cx: &mut TestAppContext) -> ContextPrecomputeLease {
    fixture.window.update(cx, |window, cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        let admission =
            vega_store::thread_admission::try_acquire(store.database_identity(), &thread().id)
                .unwrap()
                .unwrap();
        window
            .context_precompute_owner
            .bind(store.database_identity(), &thread().id, &admission)
            .unwrap()
            .unwrap()
    })
}

#[gpui_kit::test]
fn explicit_stop_revokes_background_lease_before_all_preparation_early_returns(
    cx: &mut TestAppContext,
) {
    for phase in ["background", "title", "ordinary", "active", "trusted"] {
        let fixture = Fixture::new(cx);
        let lease = bind(&fixture, cx);
        let cancelled = CancellationToken::new();
        let mut worker = None;
        fixture.window.update(cx, |window, cx| {
            match phase {
                "title" => {
                    window.agent_controller.queue_drain_waits.insert(
                        thread().id,
                        crate::app_agent::QueueDrainWait {
                            generation: 1,
                            stream: fixture.stream.clone(),
                            message_id: "pending".into(),
                            message_generation: "generation".into(),
                            cancel: cancelled.clone(),
                        },
                    );
                }
                "ordinary" => {
                    let store = cx.global::<VegaStore>().0.as_ref().unwrap();
                    let admission = vega_store::thread_admission::try_acquire(
                        store.database_identity(),
                        &thread().id,
                    )
                    .unwrap()
                    .unwrap();
                    let raw = vega_conversation::ordinary_submissions::RawSubmission::new(
                        "Synthetic".into(),
                        Vec::new(),
                        Vec::new(),
                        1,
                        thread().model,
                        None,
                        None,
                    );
                    let (_, ordinary) =
                        vega_conversation::ordinary_submissions::OrdinarySubmission::begin(
                            store.database_identity(),
                            thread().id,
                            raw,
                            admission,
                        )
                        .unwrap();
                    window.agent_controller.ordinary_preparing.insert(
                        thread().id,
                        crate::ordinary_submission::OrdinaryPreparationOwner {
                            attempt: ordinary.attempt_id(),
                            cancel: cancelled.clone(),
                        },
                    );
                    worker = Some(ordinary);
                }
                "active" => {
                    window
                        .agent_controller
                        .begin(thread().id, fixture.stream.clone(), None, None)
                        .unwrap();
                }
                "trusted" => {
                    window.agent_controller.preparation_stream = Some(fixture.stream.clone());
                    assert!(
                        window
                            .trusted_actions
                            .acquire(TrustedActionKind::AgentPreflight, 0, 1)
                            .is_some()
                    );
                }
                _ => {}
            }
            window.stop_composer(
                fixture.stream.clone(),
                &ComposerStopRequested {
                    thread_id: thread().id,
                },
                cx,
            );
            if phase == "trusted" {
                assert!(window.agent_controller.preparation_stream.is_none());
                assert!(!window.trusted_actions.is_busy());
            }
            if phase == "active" {
                assert!(
                    window
                        .agent_controller
                        .active
                        .get(&thread().id)
                        .unwrap()
                        .cancel
                        .is_cancelled()
                );
            }
        });
        assert!(!lease.is_current().unwrap(), "{phase}");
        if matches!(phase, "title" | "ordinary") {
            assert!(cancelled.is_cancelled(), "{phase}");
        }
        drop(worker);
    }
}

#[gpui_kit::test]
fn stale_stream_stop_and_route_changes_do_not_revoke_background_but_window_close_does(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx);
    let lease = bind(&fixture, cx);
    let other_stream = cx.new(|cx| ConversationStream::new(thread(), cx));
    fixture.window.update(cx, |window, cx| {
        window.stop_composer(
            other_stream.clone(),
            &ComposerStopRequested {
                thread_id: thread().id,
            },
            cx,
        );
    });
    assert!(lease.is_current().unwrap());
    let mut other_thread = thread();
    other_thread.id = "another-thread".into();
    fixture.navigate(cx, other_thread, &other_stream);
    assert!(lease.is_current().unwrap());
    fixture
        .window
        .update(cx, |window, _| window.window_terminal_cleanup());
    assert!(!lease.is_current().unwrap());
}
