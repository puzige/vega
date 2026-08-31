use super::*;

#[tokio::test]
async fn priced_usage_carries_exact_quote_provenance() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn main() {}\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("answer".into()),
        ProviderEvent::Usage {
            input: 200_000,
            output: 100_000,
            cache_read: 20_000,
            cache_write: 0,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]]);
    let mut req = request(Vec::new());
    req.model = "quote-model".to_string();
    req.pricing_catalog = Some(priced_catalog());
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed);
    // $1/1M input, $2/1M output, $0.1/1M cache-read. Rate unit is
    // micro-cents per 1M tokens ("1" ⇒ 1_000_000 µ¢/1M). Integer engine:
    // numerator = 180_000*1_000_000 + 100_000*2_000_000 + 20_000*100_000
    // = 382e9, half-up /1M ⇒ 382_000 µ¢ = $0.382.
    let expected = 382_000;
    let matched = outcome.events.iter().any(|event| {
        matches!(
            event,
            RuntimeEvent::UsageUpdated {
                usage: RuntimeTokenUsage {
                    input: 200_000,
                    output: 100_000,
                    cache_read: 20_000,
                    cache_write: 0,
                },
                cost_microcents,
                pricing: Some(RuntimeUsagePricing { version, profile, .. }),
            } if *cost_microcents == expected
                && version == "pricing_v1"
                && profile == "base"
        )
    });
    assert!(matched, "priced usage event with exact quote missing");
}

#[tokio::test]
async fn duplicate_usage_fails_closed() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn main() {}\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![
        ProviderEvent::Usage {
            input: 10,
            output: 2,
            cache_read: 0,
            cache_write: 0,
        },
        ProviderEvent::Usage {
            input: 10,
            output: 2,
            cache_read: 0,
            cache_write: 0,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(outcome.failed);
    assert!(
        outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Error(_)))
    );
}

#[tokio::test]
async fn usage_after_terminal_fails_closed() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn main() {}\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
        ProviderEvent::Usage {
            input: 10,
            output: 2,
            cache_read: 0,
            cache_write: 0,
        },
    ])]]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(outcome.failed);
}

#[tokio::test]
async fn unpriced_model_keeps_zero_cost_legacy_row() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn main() {}\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![
        ProviderEvent::Usage {
            input: 40,
            output: 8,
            cache_read: 0,
            cache_write: 0,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]]);
    let mut req = request(Vec::new());
    req.pricing_catalog = Some(priced_catalog()); // "mock" is not listed
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed);
    assert!(outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::UsageUpdated {
            cost_microcents: 0,
            pricing: None,
            ..
        }
    )));
}

#[tokio::test]
async fn over_input_limit_fails_closed_without_pricing() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn main() {}\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![
        ProviderEvent::Usage {
            input: 500,
            output: 2,
            cache_read: 0,
            cache_write: 0,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]]);
    let small_cap_catalog = PricingCatalog::from_specs(vec![vega_token::ModelPricingSpec {
        model: "quote-model".to_string(),
        rates: vega_token::RateSpec {
            input_usd_per_million: "1".to_string(),
            output_usd_per_million: "2".to_string(),
            cache_read_usd_per_million: "0.1".to_string(),
            cache_write_usd_per_million: "0".to_string(),
        },
        max_standard_input_tokens: Some(200),
        schedule: None,
    }])
    .unwrap();
    let mut req = request(Vec::new());
    req.model = "quote-model".to_string(); // small-cap catalog caps at 200
    req.pricing_catalog = Some(small_cap_catalog);
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(outcome.failed);
}
