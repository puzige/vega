use super::*;

#[gpui::test]
async fn composer_counter_projects_estimate_calibration_and_fences(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "meter-thread");
    // Unpriced start: the counter is visible (not noise) and shows `—`.
    let initial = stream.read_with(cx, |stream, _| stream.meter_snapshot());
    assert_eq!(initial.display(), "0 tok · —");

    stream.update(cx, |stream, cx| {
        stream.install_meter_estimator(
            // `RunUsageEstimator::new` is already `Option`: an unpriced
            // model yields `None` and the counter shows `—`.
            RunUsageEstimator::new(
                "meter-model",
                vega_conversation::PricingCatalog::from_specs(vec![
                    vega_conversation::ModelPricingSpec {
                        model: "meter-model".into(),
                        rates: vega_conversation::RateSpec {
                            input_usd_per_million: "1".into(),
                            output_usd_per_million: "2".into(),
                            cache_read_usd_per_million: "0.1".into(),
                            cache_write_usd_per_million: "0".into(),
                        },
                        max_standard_input_tokens: None,
                        schedule: None,
                    },
                ])
                .expect("catalog"),
            ),
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "assistant".into(),
                delta: "中文🦀".into(),
            },
            cx,
        );
    });
    let streaming = stream.read_with(cx, |stream, _| stream.meter_snapshot());
    assert_eq!(streaming.tokens, 1, "3 unicode scalars ceil-divided by 4");
    assert!(streaming.provisional);
    assert_eq!(streaming.display(), "≈1 tok · ≈US$0.000002");

    // Calibration replaces the estimate in place; late duplicate usage on
    // the finished message cannot re-add.
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::UsageUpdated {
                message_id: "assistant".into(),
                usage: vega_conversation::types::TokenUsage {
                    input: 100,
                    output: 10,
                    cache_read: 0,
                    cache_write: 0,
                },
                cost: Microcents(120),
                pricing: Some(vega_conversation::types::UsagePricing {
                    version: "pricing_v1".into(),
                    profile: "base".into(),
                    call_started_at: 1_700_000_000,
                }),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "assistant".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    let calibrated = stream.read_with(cx, |stream, _| stream.meter_snapshot());
    assert_eq!(calibrated.display(), "110 tok · US$0.00012");

    // Route fence: a late text delta for the finished message must not
    // resurrect the provisional counter.
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "assistant".into(),
                delta: "late arrival".into(),
            },
            cx,
        );
    });
    let fenced = stream.read_with(cx, |stream, _| stream.meter_snapshot());
    assert_eq!(fenced.display(), "110 tok · US$0.00012");

    // Restart recovery: the restored aggregate becomes the new baseline.
    stream.update(cx, |stream, cx| {
        stream.restore_meter(
            RestoredUsage {
                tokens: 1_234_567,
                cost: Some(Microcents(180_000)),
            },
            cx,
        );
    });
    let restored = stream.read_with(cx, |stream, _| stream.meter_snapshot());
    assert_eq!(restored.display(), "1.2M tok · US$0.18");
}

#[gpui::test]
async fn composer_counter_error_path_clears_provisional(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "meter-error-thread");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "assistant".into(),
                delta: "abcd".into(),
            },
            cx,
        );
    });
    assert!(stream.read_with(cx, |stream, _| stream.meter_snapshot().provisional));
    // Controller failure (spawn error etc.) clears run-scoped state.
    stream.update(cx, ConversationStream::apply_agent_error);
    let cleared = stream.read_with(cx, |stream, _| stream.meter_snapshot());
    assert!(!cleared.provisional);
    assert_eq!(cleared.display(), "0 tok · —");
}
