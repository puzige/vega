#[allow(unused_imports)]
use super::*;

#[test]
fn pricing_precommit_failure_keeps_persistent_notice_and_exact_draft() {
    let data = tempfile::tempdir().expect("pricing state root");
    let service = PricingSettingsService::new(data.path().join("pricing.json"));
    let authority = service.load_or_seed().expect("pricing authority").authority;
    let plan = service
        .prepare_save(
            &authority,
            vega_conversation::types::PricingMutation::AddCustom {
                model: "custom/retry".into(),
                rates: vega_conversation::types::PricingRateInputs {
                    input_usd_per_million: "1".into(),
                    output_usd_per_million: "1".into(),
                    cache_read_usd_per_million: "1".into(),
                    cache_write_usd_per_million: "1".into(),
                },
            },
        )
        .expect("pricing plan");
    let mut state = pricing_retry_ready(
        authority,
        9,
        Some(PricingNotice::DurabilityUnknownReconciled),
        plan,
        PricingSettingsErrorCode::Io,
    );
    assert!(matches!(
        &state,
        PricingControllerState::Ready {
            generation: 9,
            notice: Some(PricingNotice::DurabilityUnknownReconciled),
            draft: Some(_),
            draft_reason: Some(PricingDraftReason::RetryPending),
            error: Some(PricingSettingsErrorCode::Io),
            ..
        }
    ));
    assert!(discard_pricing_draft(&mut state, 9));
    assert!(matches!(
        state,
        PricingControllerState::Ready {
            draft: None,
            draft_reason: None,
            error: None,
            ..
        }
    ));
}

#[test]
fn pricing_controller_operation_claim_is_single_flight_and_stale_safe() {
    let mut controller = PricingController::new(None);
    let first = controller
        .begin_operation()
        .expect("first pricing operation");
    assert!(controller.begin_operation().is_none());
    assert!(!controller.claim_completion(first + 1));
    assert_eq!(controller.active_operation, Some(first));
    assert!(controller.claim_completion(first));
    assert!(controller.active_operation.is_none());
}

pub(crate) struct CommitPanelHarness {
    pub(crate) panel: Entity<CommitPanel>,
}

impl Render for CommitPanelHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.panel.clone())
    }
}

pub(crate) struct PricingWindowHarness {
    pub(crate) root: Entity<VegaWindow>,
}

impl Render for PricingWindowHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.root.clone()
    }
}

#[gpui::test]
async fn pricing_settings_and_agent_preflight_production_e2e(cx: &mut gpui::TestAppContext) {
    let repo = diff_controller_repo();
    let data = tempfile::tempdir().expect("pricing data root");
    let store = Store::open(data.path().join("vega.db")).expect("pricing file store");
    store.migrate().expect("pricing migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 pricing repo"),
        "pricing-e2e",
        None,
    )
    .expect("pricing project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "custom/gated",
        PermissionMode::Confirm.as_str(),
    )
    .expect("pricing thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("ok".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.agent_provider_override = Some(provider.clone());
    });
    let window_root = root.clone();
    let _window: gpui::WindowHandle<PricingWindowHarness> = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| PricingWindowHarness { root: window_root })
            })
        })
        .expect("pricing window");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            matches!(
                root.pricing_controller.state,
                PricingControllerState::Ready { .. }
            )
        })
    });

    root.update(cx, |root, _| {
        let _ = root.artifact_controller.close();
    });

    let starts = AGENT_WORKER_STARTS.load(Ordering::SeqCst);
    let (agent_generation, artifact_epoch, artifact_active) = root.read_with(cx, |root, _| {
        (
            root.agent_controller.next_generation,
            root.artifact_controller.next_route_epoch,
            root.artifact_controller.active.is_some(),
        )
    });
    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("blocked before pricing".into()),
            cx,
        );
    });
    assert_eq!(AGENT_WORKER_STARTS.load(Ordering::SeqCst), starts);
    assert!(provider.requests().is_empty());
    root.read_with(cx, |root, _| {
        assert!(root.agent_controller.active.is_none());
        assert_eq!(root.agent_controller.next_generation, agent_generation);
        assert_eq!(root.artifact_controller.next_route_epoch, artifact_epoch);
        assert_eq!(root.artifact_controller.active.is_some(), artifact_active);
    });
    assert!(cx.update(|cx| cx.global::<SettingsOpen>().0));
    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::ApprovedPlan("not-started-without-pricing".into()),
            cx,
        );
    });
    assert_eq!(AGENT_WORKER_STARTS.load(Ordering::SeqCst), starts);
    assert!(provider.requests().is_empty());
    root.read_with(cx, |root, _| {
        assert!(root.agent_controller.active.is_none());
        assert_eq!(root.agent_controller.next_generation, agent_generation);
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.settings_view.is_some())
    });
    let settings = root
        .read_with(cx, |root, _| root.settings_view.clone())
        .expect("production settings entity");
    let generation = root.read_with(cx, |root, _| match &root.pricing_controller.state {
        PricingControllerState::Ready { generation, .. } => *generation,
        _ => 0,
    });
    let gate = Arc::new(std::sync::Barrier::new(2));
    root.update(cx, |root, cx| {
        root.pricing_drop_next_worker_result = true;
        root.pricing_next_worker_gate = Some(gate.clone());
        root.request_pricing_mutation(
            settings.clone(),
            &PricingMutationRequested {
                generation,
                mutation: Ok(vega_conversation::types::PricingMutation::AddCustom {
                    model: "custom/gated".into(),
                    rates: vega_conversation::types::PricingRateInputs {
                        input_usd_per_million: "1".into(),
                        output_usd_per_million: "1".into(),
                        cache_read_usd_per_million: "1".into(),
                        cache_write_usd_per_million: "1".into(),
                    },
                }),
            },
            cx,
        );
        assert!(matches!(
            root.pricing_controller.state,
            PricingControllerState::Saving { .. }
        ));
        cx.set_global(SettingsOpen(false));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    root.read_with(cx, |root, _| {
        assert!(root.settings_view.is_none());
        assert!(matches!(
            root.pricing_controller.state,
            PricingControllerState::Saving { .. }
        ));
    });
    gate.wait();
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            matches!(
                &root.pricing_controller.state,
                PricingControllerState::Ready {
                    authority,
                    draft: None,
                    notice: Some(PricingNotice::DurabilityUnknownReconciled),
                    ..
                } if authority.contains_exact_model("custom/gated")
            )
        })
    });
    assert!(data.path().join("pricing.json").is_file());

    cx.update(|cx| {
        cx.set_global(SettingsOpen(true));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.settings_view
                .as_ref()
                .is_some_and(|view| view != &settings)
        })
    });
    root.read_with(cx, |root, _| {
        assert!(matches!(
            &root.pricing_controller.state,
            PricingControllerState::Ready {
                authority,
                notice: Some(PricingNotice::DurabilityUnknownReconciled),
                ..
            } if authority.contains_exact_model("custom/gated")
        ));
    });
    cx.update(|cx| {
        cx.set_global(SettingsOpen(false));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.settings_view.is_none())
    });

    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("priced run".into()),
            cx,
        );
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.agent_controller.active.is_none())
            && provider.requests().len() == 1
    });
    assert_eq!(AGENT_WORKER_STARTS.load(Ordering::SeqCst), starts + 1);
    assert_eq!(provider.requests().len(), 1);
}
