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
