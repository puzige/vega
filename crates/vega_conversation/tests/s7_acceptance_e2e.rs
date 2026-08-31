//! S7-T41 (A10-01~06) deterministic mock acceptance journey.
//!
//! One cross-functional headless E2E over the real production chain —
//! `run_thread_task_with_pricing` → provider/runtime events → conversation
//! meter/summary projections → durable `token_usage`/`tool_calls`/`messages`
//! rows — with the frozen run-start catalog loaded from a real strict
//! `pricing_v1` file in an owned temp data root.
//!
//! The fixture computes a synthetic deterministic invoice from handwritten
//! constants and asserts the production DB aggregate matches it with exactly
//! zero error. THIS IS NOT REAL PROVIDER BILLING EVIDENCE: the provider is the
//! scripted `MockProvider` and no key, network or real fee is involved.
//!
//! KNOWN SEMANTIC NOTE (documented in docs/vega-s7-report.md, observation
//! carried to S8): the calibrated counter and the restart seed both sum all
//! four usage fields (`input+output+cache_read+cache_write`; see
//! `ConversationMeter::apply`'s `UsageUpdated` branch and
//! `thread_usage_seed`), so live and restored counters agree exactly. Under
//! the frozen C2 OpenAI-compatible semantics the `input` field already
//! contains `cache_read`, so the DISPLAYED token total counts cached tokens
//! twice (this fixture: 425 displayed vs 310 input+output); the T39 preflight
//! had proposed `input+output` instead. Cost is unaffected (priced via
//! `uncached = input - cache_read`, asserted exactly below). The assertions
//! pin the current production values so any S8 scope decision must
//! consciously update them.

use std::error::Error;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{RejectPermissionHook, run_thread_task_with_pricing};
use vega_conversation::summary::task_cost_summary;
use vega_conversation::threads::thread_usage_seed;
use vega_conversation::types::{
    ConversationEvent, ConversationMeter, MeterSnapshot, Microcents, RestoredUsage,
    RunUsageEstimator, SummaryCost, TaskSummaryOutcome,
};
use vega_runtime::{ChatRole, MockProvider, ProviderEvent, ScriptStep, StopReason};
use vega_store::{Store, projects, threads, token_usage};
use vega_token::{PricingCatalog, UsageCounts, load_catalog};

const THREAD_ID: &str = "s7-acceptance-thread";
const MODEL: &str = "mock-s7-acceptance";
/// Exact custom model rates, unrelated to any built-in official model id:
/// $1.00 input / $2.00 output / $0.50 cache-read / $3.00 cache-write per
/// million tokens, no schedule, no input cap.
const PRICING_JSON: &str = r#"{
  "schema_version": "pricing_v1",
  "currency": "USD",
  "models": [
    {
      "model": "mock-s7-acceptance",
      "input_usd_per_million": "1.00",
      "output_usd_per_million": "2.00",
      "cache_read_usd_per_million": "0.50",
      "cache_write_usd_per_million": "3.00"
    }
  ]
}"#;
const DOUBLED_PRICING_JSON: &str = r#"{
  "schema_version": "pricing_v1",
  "currency": "USD",
  "models": [
    {
      "model": "mock-s7-acceptance",
      "input_usd_per_million": "2.00",
      "output_usd_per_million": "4.00",
      "cache_read_usd_per_million": "1.00",
      "cache_write_usd_per_million": "6.00"
    }
  ]
}"#;
const FIXTURE_CONTENT: &str = "s7 acceptance fixture line\n";

// ─── independent invoice oracle (handwritten constants) ───────────────────
//
// Synthetic deterministic invoice — NOT REAL PROVIDER BILLING EVIDENCE.
// numerator = uncached_input*1_000_000 + output*2_000_000
//           + cache_read*500_000 + cache_write*3_000_000   (µ¢ per MTok rates)
// cost = single half-up divide by 1_000_000.
const ORACLE_ROUND_1_MICROCENTS: i64 = 150;
const ORACLE_ROUND_2_MICROCENTS: i64 = 205;
const ORACLE_TOTAL_MICROCENTS: i64 = 355;

fn oracle_round_cost(
    uncached_input: u128,
    output: u128,
    cache_read: u128,
    cache_write: u128,
) -> u128 {
    let numerator = uncached_input * 1_000_000
        + output * 2_000_000
        + cache_read * 500_000
        + cache_write * 3_000_000;
    (numerator + 500_000) / 1_000_000
}

// ─── fixture ───────────────────────────────────────────────────────────────

type CapturedEvents = Arc<Mutex<Vec<ConversationEvent>>>;

/// The sink error type is the runtime `VegaError`.
use vega_runtime::VegaError;

fn capturing_sink(
    events: CapturedEvents,
) -> impl FnMut(&ConversationEvent) -> Result<(), VegaError> {
    move |event: &ConversationEvent| {
        events.lock().unwrap().push(event.clone());
        Ok(())
    }
}

fn catalog_from(data_root: &Path, bytes: &str) -> Result<PricingCatalog, Box<dyn Error>> {
    let path = data_root.join("pricing.json");
    std::fs::write(&path, bytes)?;
    Ok(load_catalog(&path)?)
}

fn usage(input: u64, output: u64, cache_read: u64, cache_write: u64) -> ProviderEvent {
    ProviderEvent::Usage {
        input,
        output,
        cache_read,
        cache_write,
    }
}

struct UsageRow {
    model: String,
    message_id: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    cost_microcents: i64,
    pricing_version: Option<String>,
    pricing_profile: Option<String>,
    call_started_at: Option<i64>,
}

fn usage_rows(store: &Store) -> Result<Vec<UsageRow>, Box<dyn Error>> {
    let mut statement = store.conn().prepare(
        "SELECT model, message_id, input_tokens, output_tokens, cache_read_tokens, \
                cache_write_tokens, cost_microcents, pricing_version, pricing_profile, \
                call_started_at \
         FROM token_usage WHERE thread_id = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([THREAD_ID], |row| {
            Ok(UsageRow {
                model: row.get(0)?,
                message_id: row.get(1)?,
                input_tokens: row.get(2)?,
                output_tokens: row.get(3)?,
                cache_read_tokens: row.get(4)?,
                cache_write_tokens: row.get(5)?,
                cost_microcents: row.get(6)?,
                pricing_version: row.get(7)?,
                pricing_profile: row.get(8)?,
                call_started_at: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn snapshot_after(
    events: &[ConversationEvent],
    timeline: &[MeterSnapshot],
    predicate: &dyn Fn(&ConversationEvent) -> bool,
    occurrence: usize,
) -> MeterSnapshot {
    let index = events
        .iter()
        .enumerate()
        .filter(|(_, event)| predicate(event))
        .nth(occurrence)
        .unwrap_or_else(|| panic!("checkpoint {occurrence} exists"))
        .0;
    timeline[index]
}

fn assert_reading(
    snapshot: MeterSnapshot,
    tokens: u64,
    cost: i64,
    provisional: bool,
    context: &str,
) {
    assert_eq!(snapshot.tokens, tokens, "{context}: tokens");
    assert_eq!(snapshot.cost, Some(Microcents(cost)), "{context}: cost");
    assert_eq!(snapshot.provisional, provisional, "{context}: provisional");
    assert!(snapshot.available, "{context}: available");
}

fn is_text(needle: &'static str) -> impl Fn(&ConversationEvent) -> bool {
    move |event: &ConversationEvent| matches!(event, ConversationEvent::TextDelta { delta, .. } if delta == needle)
}

fn is_usage(event: &ConversationEvent) -> bool {
    matches!(event, ConversationEvent::UsageUpdated { .. })
}

/// Output-only provisional quote at the frozen selection (no schedule, so any
/// timestamp yields the base profile).
fn provisional_output_quote(catalog: &PricingCatalog, output_tokens: u64) -> i64 {
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

// ─── the one acceptance journey ────────────────────────────────────────────

#[tokio::test]
async fn two_call_tool_journey_matches_synthetic_invoice_with_zero_error()
-> Result<(), Box<dyn Error>> {
    let workspace = tempdir()?;
    let repo = workspace.path().join("repo");
    std::fs::create_dir(&repo)?;
    std::fs::write(repo.join("fixture.txt"), FIXTURE_CONTENT)?;
    let data_root = workspace.path().join("data");
    std::fs::create_dir(&data_root)?;

    // The catalog is loaded through the real strict `pricing_v1` file codec
    // from the owned temp data root (no built-in ids involved).
    let catalog = catalog_from(&data_root, PRICING_JSON)?;

    let store = Store::open(data_root.join("vega.db"))?;
    store.migrate()?;
    let project = projects::create(
        store.conn(),
        &repo.to_string_lossy(),
        "s7-acceptance-fixture",
        Some("master"),
    )?;
    threads::create(
        store.conn(),
        threads::NewThread {
            id: THREAD_ID,
            project_id: &project.id,
            title: "S7 acceptance journey",
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

    let tools = vega_tools::Tools::new(&repo)?;
    let provider = MockProvider::new_rounds(vec![
        // Provider call 1: visible text (4 Unicode scalars → provisional 1),
        // a real read tool proposal, authoritative usage, tool-use stop.
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("中文🙂A".into()),
            ProviderEvent::ToolUse {
                id: "read-1".to_string(),
                name: "read".to_string(),
                input_json: r#"{"path":"fixture.txt"}"#.to_string(),
            },
            usage(100, 20, 40, 10),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        // Provider call 2: two visible deltas (round scalar total 2 → 1, then
        // 7 → 2, i.e. ceil of the round total, never 1+2), usage, natural end.
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("完成".into()),
            ProviderEvent::TextDelta("✅abcd".into()),
            usage(160, 30, 60, 5),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);

    let events: CapturedEvents = Arc::default();
    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        THREAD_ID,
        "Summarize the fixture.",
        "Read the fixture before answering.",
        CancellationToken::new(),
        &RejectPermissionHook,
        capturing_sink(events.clone()),
        Default::default(),
        None,
        Some(catalog.clone()),
    )
    .await?;
    assert!(!run.interrupted && !run.failed, "journey completes cleanly");

    // ─── exactly two logical provider calls, one real read execution ───
    let requests = provider.requests();
    assert_eq!(requests.len(), 2, "exactly two MockProvider calls");
    assert!(
        requests
            .iter()
            .all(|request| request.model == MODEL && request.messages[0].role == ChatRole::System)
    );
    // Call 2 must observe the real round-1 tool result (observe/continue, not
    // a prerecorded final response).
    assert!(
        requests[1].messages.iter().any(|message| {
            message.role == ChatRole::Tool && message.content.contains("s7 acceptance fixture line")
        }),
        "round-2 request carries the real read result"
    );

    // ─── provisional counter → two in-place calibrations (C3/A10-02) ───
    let captured = events.lock().unwrap().clone();
    let mut meter = ConversationMeter::default();
    meter.install_run_estimator(RunUsageEstimator::new(MODEL, catalog.clone()));
    let timeline: Vec<MeterSnapshot> = captured
        .iter()
        .map(|event| {
            meter.apply(event);
            meter.snapshot()
        })
        .collect();

    // While round 1 streams: "中文🙂A" is 4 Unicode scalars → 1 provisional
    // output token quoted at the output rate ($2.00/MTok → 2 µ¢).
    assert_reading(
        snapshot_after(&captured, &timeline, &is_text("中文🙂A"), 0),
        1,
        provisional_output_quote(&catalog, 1),
        true,
        "round-1 streaming",
    );
    // Round-1 usage calibrates in place: cost is exact; the token sum pins
    // the current production meter scope (all four fields — see the module
    // note: 100+20+40+10 = 170).
    assert_reading(
        snapshot_after(&captured, &timeline, &is_usage, 0),
        170,
        ORACLE_ROUND_1_MICROCENTS,
        false,
        "round-1 calibrated",
    );
    // While round 2 streams: provisional is added only on top of the
    // calibrated call-1 base ("完成" = 2 scalars → 1 token).
    assert_reading(
        snapshot_after(&captured, &timeline, &is_text("完成"), 0),
        171,
        ORACLE_ROUND_1_MICROCENTS + provisional_output_quote(&catalog, 1),
        true,
        "round-2 first delta",
    );
    // "✅abcd" lifts the round scalar total to 7 → ceil(7/4) = 2, replacing —
    // not adding to — the earlier round-2 estimate.
    assert_reading(
        snapshot_after(&captured, &timeline, &is_text("✅abcd"), 0),
        172,
        ORACLE_ROUND_1_MICROCENTS + provisional_output_quote(&catalog, 2),
        true,
        "round-2 round-total estimate",
    );
    // Round-2 usage arrives: authoritative totals only, provisional gone
    // (current production token scope: 170 + 160+30+60+5 = 425).
    assert_reading(
        snapshot_after(&captured, &timeline, &is_usage, 1),
        425,
        ORACLE_TOTAL_MICROCENTS,
        false,
        "round-2 calibrated",
    );
    let final_snapshot = *timeline.last().unwrap();
    assert_reading(
        final_snapshot,
        425,
        ORACLE_TOTAL_MICROCENTS,
        false,
        "terminal",
    );
    assert_eq!(final_snapshot.display(), "425 tok · US$0.000355");

    // ─── exact durable rows (A10-01) ───
    let rows = usage_rows(&store)?;
    assert_eq!(rows.len(), 2, "exactly two durable usage rows");
    for (index, (row, (input, output, read, write, cost))) in rows
        .iter()
        .zip([
            (100, 20, 40, 10, ORACLE_ROUND_1_MICROCENTS),
            (160, 30, 60, 5, ORACLE_ROUND_2_MICROCENTS),
        ])
        .enumerate()
    {
        assert_eq!(row.model, MODEL, "row {index} model");
        assert_eq!(
            row.message_id.as_deref(),
            Some(run.assistant_message_id.as_str()),
            "row {index} message ownership"
        );
        assert_eq!(row.input_tokens, input, "row {index} input");
        assert_eq!(row.output_tokens, output, "row {index} output");
        assert_eq!(row.cache_read_tokens, read, "row {index} cache read");
        assert_eq!(row.cache_write_tokens, write, "row {index} cache write");
        assert_eq!(row.cost_microcents, cost, "row {index} exact cost");
        assert_eq!(row.pricing_version.as_deref(), Some("pricing_v1"));
        assert_eq!(row.pricing_profile.as_deref(), Some("base"));
        let call_started_at = row
            .call_started_at
            .unwrap_or_else(|| panic!("row {index} keeps the frozen call start"));
        assert!(call_started_at > 1_700_000_000, "unix UTC seconds");
    }
    assert!(
        rows[0].call_started_at.unwrap() <= rows[1].call_started_at.unwrap(),
        "call starts are monotonic"
    );

    // No provisional residue: the durable rows carry only the exact
    // authoritative values, and the assistant message content carries only
    // the real visible text.
    let assistant = vega_store::messages::find(store.conn(), &run.assistant_message_id)?
        .expect("assistant message exists");
    assert_eq!(assistant.role, "assistant");
    assert!(
        assistant.content.contains("中文🙂A"),
        "round-1 visible text"
    );
    assert!(assistant.content.contains("✅abcd"), "round-2 visible text");

    // ─── checked aggregates agree with the meter (A10-04) ───
    let by_message =
        token_usage::aggregate_by_message(store.conn(), THREAD_ID, &run.assistant_message_id)?;
    assert_eq!(by_message.row_count, 2);
    assert_eq!(by_message.input_tokens, 260);
    assert_eq!(by_message.output_tokens, 50);
    assert_eq!(by_message.cache_read_tokens, 100);
    assert_eq!(by_message.cache_write_tokens, 15);
    assert_eq!(
        by_message.cost,
        token_usage::AggregateCost::Priced(ORACLE_TOTAL_MICROCENTS)
    );
    let by_thread = token_usage::aggregate_by_thread(store.conn(), THREAD_ID)?;
    assert_eq!(by_thread.cost, by_message.cost, "thread aggregate agrees");

    // ─── synthetic invoice error is exactly zero ───
    // Handwritten oracle (synthetic deterministic invoice — NOT REAL PROVIDER
    // BILLING EVIDENCE):
    assert_eq!(oracle_round_cost(60, 20, 40, 10), 150);
    assert_eq!(oracle_round_cost(100, 30, 60, 5), 205);
    assert_ne!(ORACLE_TOTAL_MICROCENTS, 0, "percentage accuracy defined");
    let db_total = match by_thread.cost {
        token_usage::AggregateCost::Priced(value) => value,
        other => panic!("aggregate must be priced, got {other:?}"),
    };
    let absolute_error = db_total.abs_diff(ORACLE_TOTAL_MICROCENTS);
    assert_eq!(absolute_error, 0, "absolute error is 0 microcents");
    let percent_error = absolute_error as f64 / ORACLE_TOTAL_MICROCENTS as f64 * 100.0;
    assert_eq!(percent_error, 0.0, "percentage error is 0.00%");

    // ─── tool audit: exactly one terminal success row ───
    let tool_count: i64 = store.conn().query_row(
        "SELECT COUNT(*) FROM tool_calls WHERE thread_id = ?1 AND message_id = ?2",
        [THREAD_ID, run.assistant_message_id.as_str()],
        |row| row.get(0),
    )?;
    assert_eq!(tool_count, 1, "exactly one real read execution");
    let (tool, status): (String, String) = store.conn().query_row(
        "SELECT tool, status FROM tool_calls WHERE thread_id = ?1 AND message_id = ?2",
        [THREAD_ID, run.assistant_message_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!((tool.as_str(), status.as_str()), ("read", "success"));

    // ─── task summary card fields (A10-06) ───
    // The wall-clock duration is owned by the live caller through this seam;
    // the headless scope passes a bounded constant instead of sleep/wall/tool
    // durations.
    let live = task_cost_summary(&store, THREAD_ID, &run.assistant_message_id, Some(1_700))?;
    assert_eq!(live.outcome, TaskSummaryOutcome::Completed);
    assert_eq!(live.message_id, run.assistant_message_id);
    assert_eq!(
        live.usage,
        Some(vega_conversation::types::TokenUsage {
            input: 260,
            output: 50,
            cache_read: 100,
            cache_write: 15,
        })
    );
    assert_eq!(
        live.cost,
        SummaryCost::Priced(Microcents(ORACLE_TOTAL_MICROCENTS))
    );
    assert_eq!(live.tool_count, 1);
    // Aggregate-first cache hit: 100/260 = 38.46% → half-up 38%.
    assert_eq!(live.cache_hit_percent, Some(38));
    assert_eq!(live.duration_ms, Some(1_700));

    // ─── post-run projection adds zero provider/tool/usage mutations ───
    assert_eq!(provider.requests().len(), 2, "no provider call after run");
    assert_eq!(usage_rows(&store)?.len(), 2, "no usage rows after run");

    // ─── catalog edits after completion do not reprice stored rows ───
    let _reloaded_doubled = catalog_from(&data_root, DOUBLED_PRICING_JSON)?;
    let repriced_rows = usage_rows(&store)?;
    assert_eq!(repriced_rows[0].cost_microcents, ORACLE_ROUND_1_MICROCENTS);
    assert_eq!(repriced_rows[1].cost_microcents, ORACLE_ROUND_2_MICROCENTS);

    // ─── restart recovery: reopen the same file store/root ───
    let pre_restart = live;
    drop(store);
    let reopened = Store::open(data_root.join("vega.db"))?;

    let seed = thread_usage_seed(&reopened, THREAD_ID)?;
    // The restart seed uses the same four-field token scope as the live
    // meter, so the restored baseline matches the pre-restart counter exactly
    // (425 here; see the module note for the display-scope observation — the
    // cost segment is exact and consistent).
    assert_eq!(
        seed,
        RestoredUsage {
            tokens: 425,
            cost: Some(Microcents(ORACLE_TOTAL_MICROCENTS)),
        },
        "restart restores the exact pre-restart baseline"
    );
    // A fresh meter restores it and renders identically.
    let mut restored_meter = ConversationMeter::default();
    restored_meter.restore(seed);
    assert_eq!(restored_meter.snapshot().display(), "425 tok · US$0.000355");

    let restarted = task_cost_summary(&reopened, THREAD_ID, &run.assistant_message_id, None)?;
    assert_eq!(restarted.outcome, pre_restart.outcome);
    assert_eq!(
        restarted.usage, pre_restart.usage,
        "token four-item recovers"
    );
    assert_eq!(restarted.cost, pre_restart.cost, "priced cost recovers");
    assert_eq!(
        restarted.tool_count, pre_restart.tool_count,
        "tool count recovers"
    );
    assert_eq!(
        restarted.cache_hit_percent, pre_restart.cache_hit_percent,
        "cache ratio recovers"
    );
    assert_eq!(
        restarted.duration_ms, None,
        "restart degrades wall-clock duration to `—`"
    );

    // Ordinary history restores: the assistant message and the real tool card
    // survive the reopen.
    let restored_assistant =
        vega_store::messages::find(reopened.conn(), &run.assistant_message_id)?
            .expect("assistant message restores");
    assert_eq!(restored_assistant.content, assistant.content);
    let restored_tools = vega_store::tool_calls::count_by_message(
        reopened.conn(),
        THREAD_ID,
        &run.assistant_message_id,
    )?;
    assert_eq!(restored_tools, 1, "real tool card restores");

    // ─── schema stays exactly six tables at user_version 3 ───
    let user_version: i64 = reopened
        .conn()
        .query_row("PRAGMA user_version", [], |row| row.get(0))?;
    assert_eq!(user_version, 3, "exactly the three authorized migrations");
    let mut statement = reopened.conn().prepare(
        "SELECT name FROM sqlite_master \
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let tables: Vec<String> = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    assert_eq!(
        tables,
        vec![
            "messages",
            "permissions",
            "projects",
            "threads",
            "token_usage",
            "tool_calls",
        ],
        "exactly the six authorized tables"
    );
    Ok(())
}
