//! S7-T39 (A10-02/A10-05) headless production journey: the bounded
//! provisional estimate, in-place usage calibration, run/thread fencing, and
//! restart recovery of the Composer counter, all driven by the real
//! provider → runtime → conversation event chain against an owned temp data
//! root with the frozen run-start pricing capability.
//!
//! Zero real keys, zero network, zero cost: the provider is the scripted
//! `MockProvider` and the store lives in a `tempfile` directory.

use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{RejectPermissionHook, run_thread_task_with_pricing};
use vega_conversation::threads::thread_usage_seed;
use vega_conversation::types::{
    ConversationEvent, ConversationMeter, METER_PROVISIONAL_CHAR_CAP, MeterSnapshot, Microcents,
    RestoredUsage, RunUsageEstimator,
};
use vega_runtime::{ChatRole, MockProvider, ProviderEvent, ScriptStep, StopReason, VegaError};
use vega_store::{Store, projects, threads, token_usage};
use vega_token::{PricingCatalog, PricingCatalog as Catalog, UsageCounts};

const THREAD_ID: &str = "estimate-e2e-thread";
const MODEL: &str = "priced-model";
const UNPRICED_MODEL: &str = "unlisted-model";

/// $1/1M input, $2/1M output, $0.1/1M cache-read, no schedule.
fn catalog() -> PricingCatalog {
    Catalog::from_specs(vec![vega_token::ModelPricingSpec {
        model: MODEL.to_string(),
        rates: vega_token::RateSpec {
            input_usd_per_million: "1".to_string(),
            output_usd_per_million: "2".to_string(),
            cache_read_usd_per_million: "0.1".to_string(),
            cache_write_usd_per_million: "0".to_string(),
        },
        max_standard_input_tokens: None,
        schedule: None,
    }])
    .unwrap()
}

fn usage(input: u64, output: u64) -> ProviderEvent {
    ProviderEvent::Usage {
        input,
        output,
        cache_read: 0,
        cache_write: 0,
    }
}

/// `ceil(chars/4)` exactly as the meter computes it.
fn estimate_tokens(text: &str) -> u64 {
    text.chars().count().div_ceil(4) as u64
}

/// Output-only provisional quote at the frozen selection (no schedule, so any
/// timestamp yields the base profile).
fn output_quote(catalog: &PricingCatalog, output_tokens: u64) -> i64 {
    catalog
        .quote(
            MODEL,
            UsageCounts {
                input: 0,
                output: output_tokens,
                cache_read: 0,
                cache_write: 0,
            },
            1_700_000_000,
        )
        .unwrap()
        .cost_microcents
}

fn open_fixture(workspace: &std::path::Path) -> Result<Store, Box<dyn Error>> {
    let repo = workspace.join("repo");
    std::fs::create_dir(&repo)?;
    std::fs::write(repo.join("lib.rs"), "fn main() {}\n")?;
    let data_root = workspace.join("data");
    std::fs::create_dir(&data_root)?;
    let store = Store::open(data_root.join("vega.db"))?;
    store.migrate()?;
    let project = projects::create(
        store.conn(),
        &repo.to_string_lossy(),
        "estimate-fixture",
        Some("master"),
    )?;
    threads::create(
        store.conn(),
        threads::NewThread {
            id: THREAD_ID,
            project_id: &project.id,
            title: "Estimate journey",
            mode: "execute",
            permission_mode: "confirm",
            model: MODEL,
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )?;
    Ok(store)
}

type CapturedEvents = Arc<Mutex<Vec<ConversationEvent>>>;

fn capturing_sink(
    events: CapturedEvents,
) -> impl FnMut(&ConversationEvent) -> Result<(), VegaError> {
    move |event: &ConversationEvent| {
        events.lock().unwrap().push(event.clone());
        Ok(())
    }
}

async fn run_journey(
    store: &Store,
    provider: &MockProvider,
    pricing_catalog: Option<PricingCatalog>,
    events: CapturedEvents,
) -> Result<(), Box<dyn Error>> {
    let repo = tempdir()?;
    let tools = vega_tools::Tools::new(repo.path())?;
    run_thread_task_with_pricing(
        store,
        provider,
        &tools,
        THREAD_ID,
        "Summarize the repository.",
        "Use tools before answering.",
        CancellationToken::new(),
        &RejectPermissionHook,
        capturing_sink(events),
        Default::default(),
        None,
        pricing_catalog,
    )
    .await?;
    Ok(())
}

/// Feeds one captured production event at a time through the meter and records
/// the snapshot after each event.
fn projected_timeline(
    events: &[ConversationEvent],
    meter: &mut ConversationMeter,
) -> Vec<MeterSnapshot> {
    events
        .iter()
        .map(|event| {
            meter.apply(event);
            meter.snapshot()
        })
        .collect()
}

fn assert_reading(
    snapshot: MeterSnapshot,
    tokens: u64,
    cost: Option<i64>,
    provisional: bool,
    context: &str,
) {
    assert_eq!(snapshot.tokens, tokens, "{context}: tokens");
    assert_eq!(snapshot.cost, cost.map(Microcents), "{context}: cost");
    assert_eq!(snapshot.provisional, provisional, "{context}: provisional");
    assert!(snapshot.available, "{context}: available");
}

// ─── estimate kernel boundaries (A10-02) ──────────────────────────────────

#[test]
fn ascii_cjk_and_emoji_estimate_by_unicode_scalars() {
    // "中文" is 6 UTF-8 bytes and "🦀" is 4 bytes, but each contributes one
    // Unicode scalar: 3 + 2 + 1 = 6 scalars → ceil(6/4) = 2 tokens. A byte
    // count would have produced 13 bytes → 4 tokens, so this also pins the
    // scalar (not byte) semantics.
    let mut meter = ConversationMeter::default();
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m1".into(),
        seq: 1,
    });
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: "abc".into(),
    });
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: "中文".into(),
    });
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: "🦀".into(),
    });
    assert_reading(meter.snapshot(), 2, None, true, "6 scalars");

    // The cap keeps the estimate bounded by construction.
    assert_eq!(METER_PROVISIONAL_CHAR_CAP, 1 << 32);
    assert_eq!(METER_PROVISIONAL_CHAR_CAP.div_ceil(4), 1 << 30);
}

#[test]
fn estimate_rounding_covers_exact_plus_one_and_minus_one() {
    for (text, expected) in [
        ("", 0u64),
        ("1234567", 2),
        ("12345678", 2),
        ("123456789", 3),
    ] {
        let mut meter = ConversationMeter::default();
        meter.apply(&ConversationEvent::MessageStarted {
            message_id: "m1".into(),
            seq: 1,
        });
        if !text.is_empty() {
            meter.apply(&ConversationEvent::TextDelta {
                message_id: "m1".into(),
                delta: text.into(),
            });
        }
        assert_reading(meter.snapshot(), expected, None, !text.is_empty(), text);
    }
}

#[test]
fn empty_delta_produces_no_noise_and_no_provisional_flag() {
    let mut meter = ConversationMeter::default();
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m1".into(),
        seq: 1,
    });
    // An empty delta must not flip the counter into `≈` mode.
    assert!(!meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: String::new(),
    }));
    let snapshot = meter.snapshot();
    assert_eq!(snapshot.tokens, 0);
    assert!(!snapshot.provisional, "empty delta is silent");
    assert_eq!(snapshot.display(), "0 tok · —");

    // Real content after the empty delta still estimates.
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: "abcd".into(),
    });
    assert_reading(meter.snapshot(), 1, None, true, "after empty delta");
}

#[test]
fn thinking_and_tool_payloads_never_enter_the_estimate() {
    let mut meter = ConversationMeter::default();
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m1".into(),
        seq: 1,
    });
    // Reasoning is not visible output (C3).
    assert!(!meter.apply(&ConversationEvent::ThinkingDelta {
        message_id: "m1".into(),
        delta: "very long reasoning".into(),
    }));
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: "abcd".into(),
    });
    // Tool progress cannot move the estimate either.
    assert!(!meter.apply(&ConversationEvent::ToolCallOutput {
        call_id: "call-1".into(),
        chunk: vega_conversation::types::ToolOutputChunk("large tool json".into()),
    }));
    assert_reading(meter.snapshot(), 1, None, true, "only visible text counted");
}

#[test]
fn unpriced_model_shows_dash_cost_while_streaming() {
    // No estimator installed (the app installs `None` for unpriced models):
    // tokens still estimate, cost stays explicitly unavailable.
    let mut meter = ConversationMeter::default();
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m1".into(),
        seq: 1,
    });
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: "abcd".repeat(100),
    });
    let snapshot = meter.snapshot();
    assert_eq!(snapshot.tokens, 100);
    assert_eq!(snapshot.cost, None);
    assert!(snapshot.provisional);
    assert_eq!(snapshot.display(), "≈100 tok · —");
}

#[test]
fn checked_overflow_latches_the_whole_counter_to_dash() {
    // Token overflow through restore + estimate.
    let mut meter = ConversationMeter::default();
    meter.restore(RestoredUsage {
        tokens: u64::MAX,
        cost: Some(Microcents(1)),
    });
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m1".into(),
        seq: 1,
    });
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: "abcd".into(),
    });
    let snapshot = meter.snapshot();
    assert!(!snapshot.available, "estimate overflow fails closed");
    assert_eq!(snapshot.display(), "—");

    // Usage overflow through calibration.
    let mut meter = ConversationMeter::default();
    meter.restore(RestoredUsage {
        tokens: u64::MAX,
        cost: None,
    });
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m1".into(),
        seq: 1,
    });
    assert!(meter.apply(&ConversationEvent::UsageUpdated {
        message_id: "m1".into(),
        usage: vega_conversation::types::TokenUsage {
            input: 1,
            output: 0,
            cache_read: 0,
            cache_write: 0,
        },
        cost: Microcents(0),
        pricing: None,
    }));
    assert!(!meter.snapshot().available, "usage overflow fails closed");

    // Cost overflow through calibration.
    let mut meter = ConversationMeter::default();
    meter.restore(RestoredUsage {
        tokens: 0,
        cost: Some(Microcents(i64::MAX)),
    });
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m1".into(),
        seq: 1,
    });
    assert!(meter.apply(&ConversationEvent::UsageUpdated {
        message_id: "m1".into(),
        usage: vega_conversation::types::TokenUsage {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
        },
        cost: Microcents(1),
        pricing: Some(vega_conversation::types::UsagePricing {
            version: "pricing_v1".into(),
            profile: "base".into(),
            call_started_at: 1_700_000_000,
        }),
    }));
    assert!(!meter.snapshot().available, "cost overflow fails closed");
    assert_eq!(meter.snapshot().display(), "—");
}

#[test]
fn late_events_after_terminal_cannot_move_the_counter() {
    let mut meter = ConversationMeter::default();
    meter.install_run_estimator(RunUsageEstimator::new(MODEL, catalog()));
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m1".into(),
        seq: 1,
    });
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: "abcd".into(),
    });
    meter.apply(&ConversationEvent::UsageUpdated {
        message_id: "m1".into(),
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
    });
    meter.apply(&ConversationEvent::MessageFinished {
        message_id: "m1".into(),
        stop_reason: vega_conversation::types::ConversationStopReason::End,
    });
    let settled = meter.snapshot();
    assert_reading(settled, 110, Some(120), false, "settled");

    // Late/duplicate updates after the terminal event are ignored: usage after
    // terminal, stray text, spurious interrupt and error all leave the
    // calibrated reading untouched.
    assert!(!meter.apply(&ConversationEvent::UsageUpdated {
        message_id: "m1".into(),
        usage: vega_conversation::types::TokenUsage {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 0,
            cache_write: 0,
        },
        cost: Microcents(9_999_999),
        pricing: Some(vega_conversation::types::UsagePricing {
            version: "pricing_v1".into(),
            profile: "base".into(),
            call_started_at: 1_700_000_001,
        }),
    }));
    assert!(!meter.apply(&ConversationEvent::TextDelta {
        message_id: "m1".into(),
        delta: "late arrival".into(),
    }));
    assert!(!meter.apply(&ConversationEvent::Interrupted {
        message_id: "m1".into(),
    }));
    assert!(!meter.apply(&ConversationEvent::Error {
        message_id: Some("m1".into()),
        error: Arc::new(VegaError::Io(std::io::Error::other("late error"))),
    }));
    assert_reading(meter.snapshot(), 110, Some(120), false, "after late events");

    // A new run (MessageStarted) starts estimating from zero again; the
    // calibrated baseline carries over.
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m2".into(),
        seq: 2,
    });
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m2".into(),
        delta: "abcd".into(),
    });
    assert_reading(
        meter.snapshot(),
        111,
        Some(120 + output_quote(&catalog(), 1)),
        true,
        "next round",
    );
}

// ─── display formatting (C4) ──────────────────────────────────────────────

#[test]
fn counter_display_covers_compact_tokens_and_microcent_precision() {
    let format = |tokens: u64, cost: Option<i64>, provisional: bool| {
        MeterSnapshot {
            tokens,
            cost: cost.map(Microcents),
            provisional,
            available: true,
        }
        .display()
    };
    // k/M compact token formatting.
    assert_eq!(format(0, None, false), "0 tok · —");
    assert_eq!(format(999, Some(120_000), false), "999 tok · US$0.12");
    assert_eq!(format(1_234, Some(120_000), false), "1.2k tok · US$0.12");
    assert_eq!(
        format(12_400_000, Some(1_000_000), false),
        "12.4M tok · US$1"
    );
    // Non-zero microcents stay distinguishable; trailing zeros trimmed.
    assert_eq!(format(8, Some(16), true), "≈8 tok · ≈US$0.000016");
    assert_eq!(
        format(1_500_000, Some(1_500_500), false),
        "1.5M tok · US$1.5005"
    );
    // Priced zero is a real value (never rewritten to `—`).
    assert_eq!(format(0, Some(0), false), "0 tok · US$0");
    // Unavailable counter fails closed.
    assert_eq!(
        MeterSnapshot {
            tokens: 0,
            cost: None,
            provisional: false,
            available: false,
        }
        .display(),
        "—"
    );
}

// ─── production journeys (event chain → meter) ────────────────────────────

#[tokio::test]
async fn two_round_journey_calibrates_in_place_without_double_counting()
-> Result<(), Box<dyn Error>> {
    let workspace = tempdir()?;
    let store = open_fixture(workspace.path())?;
    let provider = MockProvider::new_rounds(vec![
        // Round 1: visible text, a real tool round, then usage.
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Checking the repository first.".into()),
            ProviderEvent::ToolUse {
                id: "read-1".to_string(),
                name: "read".to_string(),
                input_json: r#"{"path":"lib.rs"}"#.to_string(),
            },
            usage(100_000, 10_000),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        // Round 2: final answer with usage.
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("The repository contains one file.".into()),
            usage(50_000, 5_000),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let events: CapturedEvents = Arc::default();
    run_journey(&store, &provider, Some(catalog()), events.clone()).await?;

    let captured = events.lock().unwrap().clone();
    assert!(
        captured
            .iter()
            .any(|event| matches!(event, ConversationEvent::ToolCallProposed { .. })),
        "journey includes a real tool boundary"
    );

    // The same immutable run-start selection backs the estimator.
    let mut meter = ConversationMeter::default();
    meter.install_run_estimator(RunUsageEstimator::new(MODEL, catalog()));
    let timeline = projected_timeline(&captured, &mut meter);

    let first_text = "Checking the repository first.";
    let second_text = "The repository contains one file.";
    let first_estimate = estimate_tokens(first_text);
    let second_estimate = estimate_tokens(second_text);
    assert_eq!(first_estimate, 8);
    assert_eq!(second_estimate, 9);

    let after = |timeline: &[MeterSnapshot],
                 predicate: &dyn Fn(&ConversationEvent) -> bool,
                 occurrence: usize| {
        timeline[captured
            .iter()
            .enumerate()
            .filter(|(_, event)| predicate(event))
            .nth(occurrence)
            .expect("checkpoint exists")
            .0]
    };
    let is_text = |needle: &'static str| move |event: &ConversationEvent| matches!(event, ConversationEvent::TextDelta { delta, .. } if delta == needle);
    let is_usage =
        |event: &ConversationEvent| matches!(event, ConversationEvent::UsageUpdated { .. });

    // While round 1 streams: provisional ≈8 tok / ≈US$0.000016.
    assert_reading(
        after(&timeline, &is_text(first_text), 0),
        first_estimate,
        Some(output_quote(&catalog(), first_estimate)),
        true,
        "round-1 streaming",
    );
    // Round-1 usage arrives: provisional is replaced in place — exactly
    // 110_000 tokens, no residual estimate.
    assert_reading(
        after(&timeline, &is_usage, 0),
        110_000,
        Some(120_000),
        false,
        "round-1 calibrated",
    );
    // While round 2 streams: calibrated base + fresh provisional output
    // estimate (no double counting of round 1's estimate).
    assert_reading(
        after(&timeline, &is_text(second_text), 0),
        110_000 + second_estimate,
        Some(120_000 + output_quote(&catalog(), second_estimate)),
        true,
        "round-2 streaming",
    );
    // Round-2 usage arrives: authoritative totals only.
    assert_reading(
        after(&timeline, &is_usage, 1),
        165_000,
        Some(180_000),
        false,
        "round-2 calibrated",
    );
    // After the terminal event the reading is unchanged and un-provisional.
    let last = timeline.last().unwrap();
    assert_reading(*last, 165_000, Some(180_000), false, "terminal");
    assert_eq!(last.display(), "165.0k tok · US$0.18");
    Ok(())
}

#[tokio::test]
async fn rounds_without_usage_clear_provisional_on_finish_error_and_interrupt()
-> Result<(), Box<dyn Error>> {
    let workspace = tempdir()?;
    let repo = workspace.path().join("repo");
    std::fs::create_dir(&repo)?;
    std::fs::write(repo.join("lib.rs"), "fn main() {}\n")?;
    let tools = vega_tools::Tools::new(&repo)?;

    // Finish without usage.
    let base_a = workspace.path().join("a");
    std::fs::create_dir(&base_a)?;
    let store = open_fixture(base_a.as_path())?;
    let provider = MockProvider::new(vec![
        ScriptStep::text("No usage for this round."),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]);
    let events: CapturedEvents = Arc::default();
    run_journey(&store, &provider, Some(catalog()), events.clone()).await?;
    let mut meter = ConversationMeter::default();
    meter.install_run_estimator(RunUsageEstimator::new(MODEL, catalog()));
    let timeline = projected_timeline(&events.lock().unwrap(), &mut meter);
    let streaming = timeline[events
        .lock()
        .unwrap()
        .iter()
        .position(|event| matches!(event, ConversationEvent::TextDelta { .. }))
        .unwrap()];
    assert_reading(
        streaming,
        6,
        Some(output_quote(&catalog(), 6)),
        true,
        "streaming",
    );
    let finished = timeline.last().unwrap();
    assert_reading(*finished, 0, None, false, "finished without usage");
    assert_eq!(
        finished.display(),
        "0 tok · —",
        "usage unavailable, never $0"
    );
    drop(store);

    // Provider error after visible text.
    let base_b = workspace.path().join("b");
    std::fs::create_dir(&base_b)?;
    let store = open_fixture(base_b.as_path())?;
    let provider = MockProvider::new(vec![
        ScriptStep::text("Streaming before the failure."),
        ScriptStep::Error {
            status: Some(503),
            message: "provider failure".into(),
            retryable: false,
        },
    ]);
    let events: CapturedEvents = Arc::default();
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        THREAD_ID,
        "Summarize the repository.",
        "",
        CancellationToken::new(),
        &RejectPermissionHook,
        capturing_sink(events.clone()),
        Default::default(),
        None,
        Some(catalog()),
    )
    .await?;
    assert!(run.failed);
    let mut meter = ConversationMeter::default();
    meter.install_run_estimator(RunUsageEstimator::new(MODEL, catalog()));
    let timeline = projected_timeline(&events.lock().unwrap(), &mut meter);
    let finished = timeline.last().unwrap();
    assert_reading(*finished, 0, None, false, "error cleared provisional");
    drop(store);

    // Cancellation surfaces as Interrupted and clears the provisional value.
    let base_c = workspace.path().join("c");
    std::fs::create_dir(&base_c)?;
    let store = open_fixture(base_c.as_path())?;
    let provider = MockProvider::new(vec![
        ScriptStep::text("Streaming before the cancel."),
        ScriptStep::Cancelled,
    ]);
    let events: CapturedEvents = Arc::default();
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        THREAD_ID,
        "Summarize the repository.",
        "",
        CancellationToken::new(),
        &RejectPermissionHook,
        capturing_sink(events.clone()),
        Default::default(),
        None,
        Some(catalog()),
    )
    .await?;
    assert!(run.interrupted);
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, ConversationEvent::Interrupted { .. }))
    );
    let mut meter = ConversationMeter::default();
    meter.install_run_estimator(RunUsageEstimator::new(MODEL, catalog()));
    let timeline = projected_timeline(&events.lock().unwrap(), &mut meter);
    let finished = timeline.last().unwrap();
    assert_reading(*finished, 0, None, false, "interrupt cleared provisional");
    Ok(())
}

#[tokio::test]
async fn unpriced_model_journey_keeps_tokens_but_fails_cost_closed() -> Result<(), Box<dyn Error>> {
    let workspace = tempdir()?;
    let store = open_fixture(workspace.path())?;
    // The run-start selection is exact: this catalog does not contain the
    // thread's model, so the run proceeds unpriced (C3).
    let other = Catalog::from_specs(vec![vega_token::ModelPricingSpec {
        model: "other-model".to_string(),
        rates: vega_token::RateSpec {
            input_usd_per_million: "1".to_string(),
            output_usd_per_million: "1".to_string(),
            cache_read_usd_per_million: "0".to_string(),
            cache_write_usd_per_million: "0".to_string(),
        },
        max_standard_input_tokens: None,
        schedule: None,
    }])?;
    let provider = MockProvider::new(vec![
        ScriptStep::text("Unpriced round."),
        ScriptStep::events(vec![
            usage(10, 5),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ]),
    ]);
    let events: CapturedEvents = Arc::default();
    run_journey(&store, &provider, Some(other.clone()), events.clone()).await?;
    let captured = events.lock().unwrap().clone();
    assert!(
        captured
            .iter()
            .any(|event| matches!(event, ConversationEvent::UsageUpdated { pricing: None, .. }))
    );
    // No estimator can be frozen for an unpriced model.
    assert!(RunUsageEstimator::new(UNPRICED_MODEL, catalog()).is_none());
    let mut meter = ConversationMeter::default();
    meter.install_run_estimator(RunUsageEstimator::new(MODEL, other.clone()));
    let timeline = projected_timeline(&captured, &mut meter);
    let finished = timeline.last().unwrap();
    assert_reading(*finished, 15, None, false, "unpriced calibrated");
    assert_eq!(finished.display(), "15 tok · —");

    // Restart seed: unpriced rows aggregate to tokens with unavailable cost.
    let seed = thread_usage_seed(&store, THREAD_ID)?;
    assert_eq!(seed.tokens, 15);
    assert_eq!(seed.cost, None);
    Ok(())
}

#[tokio::test]
async fn restart_restores_calibrated_baseline_from_checked_aggregate() -> Result<(), Box<dyn Error>>
{
    let workspace = tempdir()?;
    let data_root = workspace.path().join("data");
    let store = open_fixture(workspace.path())?;
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Checking the repository first.".into()),
            ProviderEvent::ToolUse {
                id: "read-1".to_string(),
                name: "read".to_string(),
                input_json: r#"{"path":"lib.rs"}"#.to_string(),
            },
            usage(100_000, 10_000),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("The repository contains one file.".into()),
            usage(50_000, 5_000),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let events: CapturedEvents = Arc::default();
    run_journey(&store, &provider, Some(catalog()), events.clone()).await?;
    drop(store);

    // Reopen: the conversation aggregate query reproduces the exact totals.
    let reopened = Store::open(data_root.join("vega.db"))?;
    let seed = thread_usage_seed(&reopened, THREAD_ID)?;
    assert_eq!(seed.tokens, 165_000);
    assert_eq!(seed.cost, Some(Microcents(180_000)));

    // A fresh meter restores the baseline and continues estimating on top.
    let mut meter = ConversationMeter::default();
    meter.restore(seed);
    assert_reading(meter.snapshot(), 165_000, Some(180_000), false, "restored");
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m3".into(),
        seq: 3,
    });
    meter.apply(&ConversationEvent::TextDelta {
        message_id: "m3".into(),
        delta: "abcd".into(),
    });
    meter.install_run_estimator(RunUsageEstimator::new(MODEL, catalog()));
    assert_reading(
        meter.snapshot(),
        165_001,
        Some(180_000 + output_quote(&catalog(), 1)),
        true,
        "restored+estimate",
    );

    // Legacy/unpriced history fails the cost segment closed but keeps tokens.
    token_usage::insert(
        reopened.conn(),
        token_usage::NewTokenUsage {
            thread_id: THREAD_ID,
            message_id: Some("legacy-message"),
            model: MODEL,
            input_tokens: 7,
            output_tokens: 3,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_microcents: 0,
            created_at: 1,
            pricing_version: None,
            pricing_profile: None,
            call_started_at: None,
        },
    )?;
    let mixed = thread_usage_seed(&reopened, THREAD_ID)?;
    assert_eq!(mixed.tokens, 165_010, "tokens still aggregate");
    assert_eq!(mixed.cost, None, "mixed legacy rows fail closed");
    let mut meter = ConversationMeter::default();
    meter.restore(mixed);
    assert_eq!(meter.snapshot().display(), "165.0k tok · —");
    // Priced usage after an unpriced baseline cannot resurrect a cost total.
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m4".into(),
        seq: 4,
    });
    meter.apply(&ConversationEvent::UsageUpdated {
        message_id: "m4".into(),
        usage: vega_conversation::types::TokenUsage {
            input: 10,
            output: 10,
            cache_read: 0,
            cache_write: 0,
        },
        cost: Microcents(30),
        pricing: Some(vega_conversation::types::UsagePricing {
            version: "pricing_v1".into(),
            profile: "base".into(),
            call_started_at: 1_700_000_000,
        }),
    });
    assert_reading(
        meter.snapshot(),
        165_030,
        None,
        false,
        "mixed stays unpriced",
    );
    Ok(())
}

#[tokio::test]
async fn counter_updates_absorb_1_000_deltas_per_second_without_io() -> Result<(), Box<dyn Error>> {
    // The meter is a pure checked-arithmetic projection: updating it per delta
    // (exactly what the Composer counter does at 1,000 delta/s) must complete
    // in a small fraction of the second. Any per-delta IO would exceed this
    // bound by orders of magnitude. P2 (<16ms to first pixel) itself stays
    // with `cargo xtask bench` and real-window walkthrough (human pending).
    let mut meter = ConversationMeter::default();
    meter.install_run_estimator(RunUsageEstimator::new(MODEL, catalog()));
    meter.restore(RestoredUsage {
        tokens: 165_000,
        cost: Some(Microcents(180_000)),
    });
    meter.apply(&ConversationEvent::MessageStarted {
        message_id: "m1".into(),
        seq: 1,
    });
    let started = Instant::now();
    let deltas = 1_000u64;
    for index in 0..deltas {
        assert!(meter.apply(&ConversationEvent::TextDelta {
            message_id: "m1".into(),
            delta: format!("delta {index} "),
        }));
        // The render path reads exactly this snapshot per repaint.
        let _ = meter.snapshot();
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "1,000 delta+snapshot cycles must stay far below one second, took {elapsed:?}"
    );
    let total_chars = (0..deltas)
        .map(|index| format!("delta {index} ").chars().count() as u64)
        .sum::<u64>();
    let expected = 165_000 + total_chars.div_ceil(4);
    let provisional_cost = 180_000 + output_quote(&catalog(), total_chars.div_ceil(4));
    assert_reading(
        meter.snapshot(),
        expected,
        Some(provisional_cost),
        true,
        "throughput",
    );
    Ok(())
}

#[tokio::test]
async fn journey_events_carry_durable_thread_model_for_frozen_selection()
-> Result<(), Box<dyn Error>> {
    // Guard for the C3 preflight: every request of the journey uses the exact
    // durable thread model, so the frozen selection and the estimator always
    // price the same model.
    let workspace = tempdir()?;
    let store = open_fixture(workspace.path())?;
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Round one.".into()),
            ProviderEvent::ToolUse {
                id: "read-1".to_string(),
                name: "read".to_string(),
                input_json: r#"{"path":"lib.rs"}"#.to_string(),
            },
            usage(10, 2),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Round two.".into()),
            usage(10, 2),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let events: CapturedEvents = Arc::default();
    run_journey(&store, &provider, Some(catalog()), events.clone()).await?;
    assert_eq!(provider.requests().len(), 2);
    assert!(
        provider
            .requests()
            .iter()
            .all(|request| request.model == MODEL && request.messages[0].role == ChatRole::System)
    );
    // Two independent priced rows, one per provider call: each call costs
    // 10µ¢ input + 4µ¢ output = 14µ¢, so 28µ¢ across the two calls.
    let aggregate = token_usage::aggregate_by_thread(store.conn(), THREAD_ID)?;
    assert_eq!(aggregate.row_count, 2);
    assert_eq!(aggregate.cost, token_usage::AggregateCost::Priced(28));
    Ok(())
}
