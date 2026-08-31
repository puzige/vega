//! S7-T38 (A10-01/A10-04) headless production journey: two provider calls
//! plus one real tool round run against an owned temp data root, with the
//! frozen run-start pricing capability producing exact quotes and durable,
//! restart-consistent usage rows.

use std::error::Error;

use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{RejectPermissionHook, run_thread_task_with_pricing};
use vega_runtime::{ChatRole, MockProvider, ProviderEvent, ScriptStep, StopReason};
use vega_store::{Store, projects, threads, token_usage};
use vega_token::{PricingCatalog, PricingCatalog as Catalog};

const THREAD_ID: &str = "usage-e2e-thread";
const MODEL: &str = "priced-model";

fn catalog() -> PricingCatalog {
    // $1/1M input, $2/1M output, $0.1/1M cache-read, no input cap.
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

#[tokio::test]
async fn two_provider_calls_persist_priced_rows_with_restart_consistency()
-> Result<(), Box<dyn Error>> {
    let workspace = tempdir()?;
    let repo = workspace.path().join("repo");
    std::fs::create_dir(&repo)?;
    std::fs::write(repo.join("lib.rs"), "fn main() {}\n")?;
    let data_root = workspace.path().join("data");
    std::fs::create_dir(&data_root)?;

    let store = Store::open(data_root.join("vega.db"))?;
    store.migrate()?;
    let project = projects::create(
        store.conn(),
        &repo.to_string_lossy(),
        "usage-fixture",
        Some("master"),
    )?;
    threads::create(
        store.conn(),
        threads::NewThread {
            id: THREAD_ID,
            project_id: &project.id,
            title: "Usage journey",
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
        // Round 1: one real tool call, then usage and a tool-use stop.
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
        // Round 2: final answer, usage, natural end.
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("The repository contains one file.".into()),
            usage(50_000, 5_000),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);

    let run = run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        THREAD_ID,
        "Summarize the repository.",
        "Use tools before answering.",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        Default::default(),
        None,
        Some(catalog()),
    )
    .await?;

    assert!(!run.interrupted);
    assert!(!run.failed);

    // Two logical provider calls, each carrying the durable thread model.
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.model == MODEL && request.messages[0].role == ChatRole::System)
    );

    // ─── persisted rows ───
    let rows = {
        let mut statement = store.conn().prepare(
            "SELECT model, input_tokens, output_tokens, cache_read_tokens, cost_microcents, \
                    pricing_version, pricing_profile, call_started_at \
             FROM token_usage WHERE thread_id = ?1 ORDER BY id",
        )?;
        statement
            .query_map([THREAD_ID], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    assert_eq!(rows.len(), 2, "one priced row per provider call");
    // Round 1 has cache_read = 0: $1/1M * 100k input + $2/1M * 10k output
    // = 100_000 + 20_000 = 120_000 µ¢ = $0.12.
    assert_eq!(rows[0].1, 100_000);
    assert_eq!(rows[0].4, 120_000, "round-1 exact priced cost");
    // $1/1M * 50k + $2/1M * 5k = 50_000 + 10_000 = 60_000 µ¢ = $0.06
    assert_eq!(rows[1].1, 50_000);
    assert_eq!(rows[1].4, 60_000, "round-2 exact priced cost");
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.0, MODEL);
        assert_eq!(row.5.as_deref(), Some("pricing_v1"), "row {index} version");
        assert_eq!(row.6.as_deref(), Some("base"), "row {index} profile");
        let call_started_at = row.7.expect("priced rows keep the frozen call start");
        assert!(call_started_at > 1_700_000_000, "unix UTC seconds");
    }
    // Per-call capture (monotonic; same-second runs are legitimate for fast
    // local mock providers — provider-internal retry reuses the frozen call
    // start by construction: the timestamp is captured once per logical call).
    assert!(
        rows[0].7.unwrap() <= rows[1].7.unwrap(),
        "call starts are monotonic"
    );

    // ─── checked aggregate (pre-restart) ───
    let aggregate = token_usage::aggregate_by_thread(store.conn(), THREAD_ID)?;
    assert_eq!(aggregate.row_count, 2);
    assert_eq!(aggregate.input_tokens, 150_000);
    assert_eq!(aggregate.output_tokens, 15_000);
    assert_eq!(aggregate.cost, token_usage::AggregateCost::Priced(180_000));

    // ─── restart consistency: reopen the store, aggregate again ───
    drop(store);
    let reopened = Store::open(data_root.join("vega.db"))?;
    let restarted = token_usage::aggregate_by_thread(reopened.conn(), THREAD_ID)?;
    assert_eq!(restarted.row_count, 2);
    assert_eq!(
        restarted.cost,
        token_usage::AggregateCost::Priced(180_000),
        "restart must reproduce the checked cost exactly"
    );
    Ok(())
}
