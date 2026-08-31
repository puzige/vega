//! S7-T40 (A10-06) headless production journey: per-task cost summaries
//! projected from the durable `token_usage`/`tool_calls` audits of real
//! mock-provider runs, including typed unavailable semantics, terminal
//! interrupted/error outcomes, restart recovery, and deletion-safe usage
//! audit queries.

use std::error::Error;

use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{RejectPermissionHook, run_thread_task_with_pricing};
use vega_conversation::summary::task_cost_summary;
use vega_conversation::types::{Microcents, SummaryCost, TaskSummaryOutcome, TokenUsage};
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason};
use vega_store::{Store, projects, threads, token_usage};
use vega_token::{PricingCatalog, PricingCatalog as Catalog};

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

fn usage(input: u64, output: u64, cache_read: u64) -> ProviderEvent {
    ProviderEvent::Usage {
        input,
        output,
        cache_read,
        cache_write: 0,
    }
}

fn done(stop: StopReason) -> ProviderEvent {
    ProviderEvent::Done { stop_reason: stop }
}

struct Fixture {
    workspace: tempfile::TempDir,
    data_root: std::path::PathBuf,
    store: Store,
}

fn fixture() -> Result<Fixture, Box<dyn Error>> {
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
        "summary-fixture",
        Some("master"),
    )?;
    threads::create(
        store.conn(),
        threads::NewThread {
            id: "summary-e2e-thread",
            project_id: &project.id,
            title: "Summary journey",
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
    Ok(Fixture {
        workspace,
        data_root,
        store,
    })
}

async fn run_task(
    fixture: &Fixture,
    provider: &MockProvider,
) -> Result<vega_conversation::agent::ConversationRun, Box<dyn Error>> {
    let repo = fixture.workspace.path().join("repo");
    let tools = vega_tools::Tools::new(&repo)?;
    let run = run_thread_task_with_pricing(
        &fixture.store,
        provider,
        &tools,
        "summary-e2e-thread",
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
    Ok(run)
}

fn assistant_message_id(run: &vega_conversation::agent::ConversationRun) -> &str {
    &run.assistant_message_id
}

#[tokio::test]
async fn one_provider_call_without_tools_projects_priced_summary() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Direct answer.".into()),
        usage(80_000, 8_000, 0),
        done(StopReason::End),
    ])]);
    let run = run_task(&fixture, &provider).await?;
    assert!(!run.interrupted && !run.failed);
    assert_eq!(provider.requests().len(), 1, "exactly one provider call");

    let summary = task_cost_summary(
        &fixture.store,
        "summary-e2e-thread",
        assistant_message_id(&run),
        Some(900),
    )?;
    assert_eq!(summary.outcome, TaskSummaryOutcome::Completed);
    assert_eq!(
        summary.usage,
        Some(TokenUsage {
            input: 80_000,
            output: 8_000,
            cache_read: 0,
            cache_write: 0,
        })
    );
    // $1/1M * 80k + $2/1M * 8k = 96_000 µ¢.
    assert_eq!(summary.cost, SummaryCost::Priced(Microcents(96_000)));
    assert_eq!(summary.tool_count, 0, "no tool call audit rows");
    assert_eq!(summary.cache_hit_percent, Some(0));
    assert_eq!(summary.duration_ms, Some(900));
    Ok(())
}

#[tokio::test]
async fn two_provider_calls_with_two_tools_project_exact_cache_ratio() -> Result<(), Box<dyn Error>>
{
    let fixture = fixture()?;
    let provider = MockProvider::new_rounds(vec![
        // Round 1: two tool calls, then usage and a tool-use stop.
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "read-1".to_string(),
                name: "read".to_string(),
                input_json: r#"{"path":"lib.rs"}"#.to_string(),
            },
            ProviderEvent::ToolUse {
                id: "read-2".to_string(),
                name: "read".to_string(),
                input_json: r#"{"path":"lib.rs"}"#.to_string(),
            },
            usage(100_000, 10_000, 40_000),
            done(StopReason::ToolUse),
        ])],
        // Round 2: final answer with cached prompt, natural end.
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("The repository contains one file.".into()),
            usage(50_000, 5_000, 10_000),
            done(StopReason::End),
        ])],
    ]);
    let run = run_task(&fixture, &provider).await?;
    assert!(!run.interrupted && !run.failed);
    assert_eq!(provider.requests().len(), 2, "two provider calls");

    let summary = task_cost_summary(
        &fixture.store,
        "summary-e2e-thread",
        assistant_message_id(&run),
        None,
    )?;
    assert_eq!(summary.outcome, TaskSummaryOutcome::Completed);
    assert_eq!(
        summary.usage,
        Some(TokenUsage {
            input: 150_000,
            output: 15_000,
            cache_read: 50_000,
            cache_write: 0,
        })
    );
    // Round 1: uncached 60_000×$1 + 10_000×$2 + 40_000×$0.1 (per 1M)
    // = 84_000 µ¢; round 2: uncached 40_000×$1 + 5_000×$2 + 10_000×$0.1
    // = 51_000 µ¢ (C2: the input rate applies to uncached input only).
    assert_eq!(summary.cost, SummaryCost::Priced(Microcents(135_000)));
    assert_eq!(summary.tool_count, 2, "both audited tool calls count");
    // 50_000/150_000 = 33.33% → half-up 33%.
    assert_eq!(summary.cache_hit_percent, Some(33));
    assert_eq!(summary.duration_ms, None, "no wall-clock in this scope");
    Ok(())
}

#[tokio::test]
async fn provider_error_keeps_typed_unavailable_not_zero() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let provider = MockProvider::new(vec![ScriptStep::Error {
        status: None,
        message: "provider unavailable".to_string(),
        retryable: false,
    }]);
    let run = run_task(&fixture, &provider).await?;
    assert!(run.failed, "a non-retryable provider error fails the run");

    let summary = task_cost_summary(
        &fixture.store,
        "summary-e2e-thread",
        assistant_message_id(&run),
        None,
    )?;
    assert_eq!(summary.outcome, TaskSummaryOutcome::Failed);
    assert_eq!(
        summary.usage, None,
        "a failed call writes no usage row (C3), so tokens stay typed None"
    );
    assert_eq!(summary.cost, SummaryCost::Unavailable);
    assert_eq!(summary.cache_hit_percent, None);
    assert_eq!(summary.tool_count, 0);
    Ok(())
}

#[tokio::test]
async fn provider_cancellation_is_terminal_interrupted_summary() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let provider = MockProvider::new(vec![ScriptStep::Cancelled]);
    let run = run_task(&fixture, &provider).await?;
    assert!(run.interrupted, "scripted cancellation interrupts the run");

    let summary = task_cost_summary(
        &fixture.store,
        "summary-e2e-thread",
        assistant_message_id(&run),
        None,
    )?;
    assert_eq!(
        summary.outcome,
        TaskSummaryOutcome::Interrupted,
        "interrupted is a durable terminal outcome, never a running state"
    );
    assert_eq!(summary.usage, None);
    assert_eq!(summary.cost, SummaryCost::Unavailable);
    Ok(())
}

#[tokio::test]
async fn restart_recovers_persisted_fields_and_degrades_duration() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "read-1".to_string(),
                name: "read".to_string(),
                input_json: r#"{"path":"lib.rs"}"#.to_string(),
            },
            usage(100_000, 10_000, 40_000),
            done(StopReason::ToolUse),
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Done.".into()),
            usage(50_000, 5_000, 10_000),
            done(StopReason::End),
        ])],
    ]);
    let run = run_task(&fixture, &provider).await?;
    let message_id = assistant_message_id(&run).to_string();
    let live = task_cost_summary(
        &fixture.store,
        "summary-e2e-thread",
        &message_id,
        Some(1_700),
    )?;

    // Restart: close the store connection and reopen the same database file
    // (the workspace tempdir keeps the file alive), then re-project.
    let Fixture {
        workspace,
        data_root,
        store,
    } = fixture;
    let _workspace_guard = workspace;
    drop(store);
    let reopened = Store::open(data_root.join("vega.db"))?;
    let restarted = task_cost_summary(&reopened, "summary-e2e-thread", &message_id, None)?;

    assert_eq!(restarted.message_id, message_id);
    assert_eq!(restarted.outcome, live.outcome);
    assert_eq!(restarted.usage, live.usage, "token four-item recovers");
    assert_eq!(restarted.cost, live.cost, "priced cost recovers exactly");
    assert_eq!(restarted.tool_count, live.tool_count, "tool count recovers");
    assert_eq!(
        restarted.cache_hit_percent, live.cache_hit_percent,
        "cache ratio recovers"
    );
    assert_eq!(live.duration_ms, Some(1_700));
    assert_eq!(
        restarted.duration_ms, None,
        "restart degrades wall-clock duration to `—`"
    );
    Ok(())
}

#[tokio::test]
async fn thread_deletion_keeps_usage_audit_queryable() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Answer.".into()),
        usage(80_000, 8_000, 0),
        done(StopReason::End),
    ])]);
    let run = run_task(&fixture, &provider).await?;
    let message_id = assistant_message_id(&run).to_string();

    threads::delete_thread(fixture.store.conn(), "summary-e2e-thread")?;
    // The audit rows carry no thread foreign key, so the per-message usage
    // audit stays queryable after the thread (and its message rows) is gone.
    let aggregate =
        token_usage::aggregate_by_message(fixture.store.conn(), "summary-e2e-thread", &message_id)?;
    assert_eq!(aggregate.row_count, 1);
    assert_eq!(aggregate.input_tokens, 80_000);
    assert_eq!(aggregate.cost, token_usage::AggregateCost::Priced(96_000));

    // The full summary projection fails closed: the message row itself was
    // deleted with the thread, so no card could render (no fabricated data).
    assert!(matches!(
        task_cost_summary(&fixture.store, "summary-e2e-thread", &message_id, None),
        Err(vega_conversation::types::ConversationError::NotFound(_))
    ));
    Ok(())
}

#[tokio::test]
async fn unpriced_legacy_rows_show_tokens_but_unavailable_cost() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    // No catalog: the run proceeds unpriced (legacy zero cost, NULL
    // provenance) exactly like an S4/S5 historical row.
    let repo = fixture.workspace.path().join("repo");
    let tools = vega_tools::Tools::new(&repo)?;
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Answer.".into()),
        usage(80_000, 8_000, 0),
        done(StopReason::End),
    ])]);
    let run = run_thread_task_with_pricing(
        &fixture.store,
        &provider,
        &tools,
        "summary-e2e-thread",
        "Summarize the repository.",
        "Use tools before answering.",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        Default::default(),
        None,
        None,
    )
    .await?;
    assert!(!run.failed);

    let summary = task_cost_summary(
        &fixture.store,
        "summary-e2e-thread",
        assistant_message_id(&run),
        None,
    )?;
    assert_eq!(
        summary.usage,
        Some(TokenUsage {
            input: 80_000,
            output: 8_000,
            cache_read: 0,
            cache_write: 0,
        }),
        "tokens stay visible"
    );
    assert_eq!(
        summary.cost,
        SummaryCost::Unavailable,
        "legacy/unpriced rows never masquerade as free"
    );
    assert_eq!(summary.cache_hit_percent, Some(0));
    Ok(())
}
