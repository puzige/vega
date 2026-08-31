//! S8-T45 (A11-03/C7) hydration E2E: the production history projection
//! (`vega_conversation::history`) over a real temp store, including one real
//! MockProvider run with tool use and pricing so the durable transcript the
//! UI hydrates after a restart contains tools, costs, a summary reference,
//! interrupted/failed rows, and redacted tool inputs.

use std::error::Error;

use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::RejectPermissionHook;
use vega_conversation::history::{self, AssistantStatus, HistoryEntry, restart_history_page};
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason};
use vega_store::messages::PageCursor;
use vega_store::{Store, messages as store_messages, projects, threads};
use vega_token::{ModelPricingSpec, PricingCatalog, RateSpec};

const THREAD: &str = "hydration-e2e-thread";
const MODEL: &str = "priced-model";
const SECRET_BODY: &str = "TOP-SECRET-FILE-BODY-9f31c";

/// The durable database file this fixture opens (`open_store` seeds it under
/// `<workspace>/data`); restart tests must reopen the exact same file.
fn database_path(workspace: &tempfile::TempDir) -> std::path::PathBuf {
    workspace.path().join("data").join("vega.db")
}

fn catalog() -> PricingCatalog {
    PricingCatalog::from_specs(vec![ModelPricingSpec {
        model: MODEL.to_string(),
        rates: RateSpec {
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

fn open_store() -> Result<(Store, tempfile::TempDir), Box<dyn Error>> {
    let workspace = tempdir()?;
    let repo = workspace.path().join("repo");
    std::fs::create_dir(&repo)?;
    std::fs::write(repo.join("lib.rs"), "fn main() {}\n")?;
    let data_root = workspace.path().join("data");
    std::fs::create_dir(&data_root)?;
    let store = Store::open(database_path(&workspace))?;
    store.migrate()?;
    let project = projects::create(
        store.conn(),
        &repo.to_string_lossy(),
        "hydration-fixture",
        Some("master"),
    )?;
    threads::create(
        store.conn(),
        threads::NewThread {
            id: THREAD,
            project_id: &project.id,
            title: "Hydration journey",
            mode: "execute",
            permission_mode: "auto",
            model: MODEL,
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )?;
    Ok((store, workspace))
}

/// Seeds `count` user/assistant exchange pairs (2 rows per step, seqs
/// 1..=2*count) with position-marked durable content.
fn seed_exchanges(store: &Store, count: usize, first_seq: usize) -> Result<(), Box<dyn Error>> {
    let conn = store.conn();
    for step in 0..count {
        let position = first_seq + step;
        store_messages::insert(
            conn,
            &store_messages::MessageRow {
                id: format!("seed-user-{position}"),
                thread_id: THREAD.to_string(),
                seq: (2 * position - 1) as i64,
                role: "user".into(),
                kind: "text".into(),
                content: format!("问 {position}"),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
        store_messages::insert(
            conn,
            &store_messages::MessageRow {
                id: format!("seed-assistant-{position}"),
                thread_id: THREAD.to_string(),
                seq: (2 * position) as i64,
                role: "assistant".into(),
                kind: "text".into(),
                content: format!("答 {position}"),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
    }
    Ok(())
}

fn user_contents(page: &history::HistoryPage) -> Vec<&str> {
    page.entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::UserText { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect()
}

/// Durable row entries only — the summary reference re-projects an existing
/// assistant row and is not a separate durable row.
fn durable_entries(page: &history::HistoryPage) -> Vec<&HistoryEntry> {
    page.entries
        .iter()
        .filter(|entry| !matches!(entry, HistoryEntry::Summary { .. }))
        .collect()
}
#[tokio::test]
async fn empty_thread_hydrates_to_an_empty_exhausted_page() -> Result<(), Box<dyn Error>> {
    let (store, _dir) = open_store()?;
    let page = history::latest_history_page(&store, THREAD, 200)?;
    assert!(page.entries.is_empty());
    assert_eq!(page.older_cursor, None);
    assert_eq!(page.newest_seq, None);
    Ok(())
}

#[tokio::test]
async fn exactly_one_page_reports_no_older_history() -> Result<(), Box<dyn Error>> {
    let (store, _dir) = open_store()?;
    seed_exchanges(&store, 100, 1)?; // 200 rows = exactly one full page
    let page = history::latest_history_page(&store, THREAD, 200)?;
    // 200 durable rows plus the S7 summary reference attached after the
    // terminal assistant row (C7 内容完整性).
    assert_eq!(durable_entries(&page).len(), 200);
    assert_eq!(page.entries.len(), 201);
    assert_eq!(page.newest_seq, Some(200));
    // A full page alone must not imply more history: one follow-up read
    // proves exhaustion instead of guessing.
    assert_eq!(page.older_cursor, Some(1));
    let older = history::history_page_before(
        &store,
        THREAD,
        PageCursor::Before(page.older_cursor.unwrap()),
        200,
    )?;
    assert!(older.entries.is_empty());
    assert_eq!(older.older_cursor, None);
    Ok(())
}

#[tokio::test]
async fn ten_thousand_rows_walk_without_gap_or_duplicate() -> Result<(), Box<dyn Error>> {
    let (store, _dir) = open_store()?;
    seed_exchanges(&store, 5_000, 1)?; // 10k rows
    let mut seen: Vec<i64> = Vec::new();
    let mut loads = 0usize;
    let mut cursor = PageCursor::Head;
    loop {
        let page = history::history_page_before(&store, THREAD, cursor, 200)?;
        loads += 1;
        for entry in &page.entries {
            let seq = match entry {
                HistoryEntry::UserText { seq, .. }
                | HistoryEntry::AssistantText { seq, .. }
                | HistoryEntry::Plan { seq, .. }
                | HistoryEntry::Summary { seq, .. }
                | HistoryEntry::Tool { seq, .. } => *seq,
            };
            seen.push(seq);
        }
        match page.older_cursor {
            Some(seq) => cursor = PageCursor::Before(seq),
            None => break,
        }
    }
    // One projection call per page — hydration is O(pages), never O(rows)
    // (per-page statement constancy is asserted at the store layer).
    // 10k rows fill exactly 50 full pages; a full page alone must not imply
    // exhaustion, so walk 51 reads where the 51st proves it (empty, cursor
    // None).
    assert_eq!(loads, 51, "50 full pages plus one exhaustion-proof read");
    assert_eq!(seen.len(), 10_000, "no duplicates and no misses");
    seen.sort_unstable();
    assert!(
        seen.iter().copied().eq(1..=10_000),
        "the hydrated union must cover every durable seq exactly once"
    );
    Ok(())
}

#[tokio::test]
async fn page_cap_200_and_one_over_are_refused() -> Result<(), Box<dyn Error>> {
    let (store, _dir) = open_store()?;
    seed_exchanges(&store, 3, 1)?;
    for rejected in [0usize, 201, 5_000] {
        let error = history::latest_history_page(&store, THREAD, rejected).unwrap_err();
        assert!(
            error.to_string().contains("page contract"),
            "page size {rejected} must be refused, got {error}"
        );
    }
    let bounded = history::latest_history_page(&store, THREAD, 200)?;
    // 6 durable rows + the S7 summary reference on the newest page.
    assert_eq!(durable_entries(&bounded).len(), 6);
    assert_eq!(bounded.entries.len(), 7);
    // In-contract sizes 1 and 199 must be accepted (C7 边界 1/199/200).
    let one = history::latest_history_page(&store, THREAD, 1)?;
    assert_eq!(durable_entries(&one).len(), 1);
    let almost = history::latest_history_page(&store, THREAD, 199)?;
    assert_eq!(durable_entries(&almost).len(), 6);
    Ok(())
}

#[tokio::test]
async fn mid_run_inserts_stay_monotonic_above_every_loaded_cursor() -> Result<(), Box<dyn Error>> {
    let (store, _dir) = open_store()?;
    seed_exchanges(&store, 250, 1)?; // 500 rows
    let head = history::latest_history_page(&store, THREAD, 200)?;
    assert_eq!(
        head.entries.first().map(|entry| match entry {
            HistoryEntry::UserText { seq, .. } => *seq,
            _ => 0,
        }),
        Some(301)
    );
    // New durable work lands strictly above every loaded seq (MAX+1), so the
    // in-flight older cursor is unaffected and cannot duplicate rows.
    seed_exchanges(&store, 2, 251)?; // seqs 501..504
    let older = history::history_page_before(
        &store,
        THREAD,
        PageCursor::Before(head.older_cursor.unwrap()),
        200,
    )?;
    let older_seqs: Vec<i64> = older
        .entries
        .iter()
        .map(|entry| match entry {
            HistoryEntry::UserText { seq, .. } | HistoryEntry::AssistantText { seq, .. } => *seq,
            _ => 0,
        })
        .collect();
    assert_eq!(older_seqs.first().copied(), Some(101));
    assert_eq!(older_seqs.last().copied(), Some(300));
    // A fresh head load sees the new rows on top, without disturbing history.
    let fresh = history::latest_history_page(&store, THREAD, 200)?;
    assert_eq!(fresh.newest_seq, Some(504));
    assert!(user_contents(&fresh).contains(&"问 251"));
    Ok(())
}

#[tokio::test]
async fn reopen_resets_cursor_to_the_newest_page() -> Result<(), Box<dyn Error>> {
    let (store, dir) = open_store()?;
    seed_exchanges(&store, 250, 1)?;
    let before_restart = history::latest_history_page(&store, THREAD, 200)?;
    drop(store);
    // Restart: controller rebuilt — repair first, then project (C7), and the
    // cursor starts from the newest page again; the old in-memory cursor was
    // view state.
    let reopened = Store::open(database_path(&dir))?;
    let page = restart_history_page(&reopened, THREAD, 200)?;
    assert_eq!(page, before_restart);
    assert_eq!(page.older_cursor, Some(301));
    Ok(())
}

#[tokio::test]
async fn restart_repairs_incomplete_rows_before_projecting() -> Result<(), Box<dyn Error>> {
    let (store, dir) = open_store()?;
    seed_exchanges(&store, 2, 1)?;
    // A killed process leaves a streaming assistant row behind.
    store_messages::insert(
        store.conn(),
        &store_messages::MessageRow {
            id: "killed-stream".into(),
            thread_id: THREAD.into(),
            seq: 5,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )?;
    drop(store);
    let reopened = Store::open(database_path(&dir))?;
    let page = restart_history_page(&reopened, THREAD, 200)?;
    // Repair normalized the row before the projection ran, so the hydrated
    // transcript keeps it visible as interrupted instead of dropping it.
    let statuses: Vec<AssistantStatus> = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::AssistantText { status, .. } => Some(*status),
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            AssistantStatus::Done,
            AssistantStatus::Done,
            AssistantStatus::Interrupted
        ]
    );
    Ok(())
}

#[tokio::test]
async fn real_run_hydrates_tools_costs_summary_and_redacts_inputs() -> Result<(), Box<dyn Error>> {
    let (store, workspace) = open_store()?;
    let repo = workspace.path().join("repo");
    let tools = vega_tools::Tools::new(&repo)?;
    let provider = MockProvider::new_rounds(vec![
        // Round 1: a read tool call, then usage and a tool-use stop.
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Checking the repository first.".into()),
            ProviderEvent::ToolUse {
                id: "read-1".to_string(),
                name: "read".to_string(),
                input_json: r#"{"path":"lib.rs"}"#.to_string(),
            },
            ProviderEvent::Usage {
                input: 100_000,
                output: 10_000,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        // Round 2: a write proposal whose raw body must never hydrate, then
        // the final answer and end.
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".to_string(),
                name: "write".to_string(),
                input_json: format!(r#"{{"path":"lib.rs","content":"{SECRET_BODY}"}}"#),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        // Round 3: final answer after the write was rejected by the hook.
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("The repository contains one file.".into()),
            ProviderEvent::Usage {
                input: 50_000,
                output: 5_000,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let run = vega_conversation::agent::run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        THREAD,
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
    assert!(!run.interrupted && !run.failed);

    // Hydrate through the production restart entry.
    let page = restart_history_page(&store, THREAD, 200)?;
    let mut tool_inputs = Vec::new();
    let mut saw_summary = false;
    let mut assistant_status = None;
    for entry in &page.entries {
        match entry {
            HistoryEntry::UserText { content, .. } => {
                assert_eq!(content, "Summarize the repository.");
            }
            HistoryEntry::AssistantText {
                status, content, ..
            } => {
                assistant_status = Some(*status);
                assert!(content.contains("repository") || content.contains("Checking"));
            }
            HistoryEntry::Tool { input, result, .. } => {
                tool_inputs.push((input.clone(), result.clone()));
            }
            HistoryEntry::Summary { summary, .. } => {
                saw_summary = true;
                assert!(
                    matches!(
                        summary.cost,
                        vega_conversation::types::SummaryCost::Priced(_)
                    ),
                    "priced rows must hydrate the exact durable cost"
                );
            }
            HistoryEntry::Plan { .. } => unreachable!("no plans in this run"),
        }
    }
    assert_eq!(assistant_status, Some(AssistantStatus::Done));
    assert!(saw_summary, "the terminal task keeps its summary reference");
    // The read call hydrates as a read-only card; the rejected write stays
    // content-free: path/badge only, never the raw body or audit JSON.
    assert_eq!(tool_inputs.len(), 2, "one card per durable tool call");
    assert!(
        matches!(&tool_inputs[0].0, Some(vega_conversation::types::ToolCardInputProjection::ReadOnly { tool }) if *tool == vega_conversation::types::ReadOnlyToolKind::Read)
    );
    let rendered = format!("{:?}{:?}", tool_inputs[0], tool_inputs[1]);
    assert!(
        !rendered.contains(SECRET_BODY) && !rendered.contains("input_json"),
        "raw write body must not survive the projection boundary: {rendered}"
    );
    assert!(
        matches!(
            &tool_inputs[1].0,
            Some(vega_conversation::types::ToolCardInputProjection::Write { path, .. })
                if path == "lib.rs"
        ),
        "the safe audit summary keeps the path and drops the body"
    );
    Ok(())
}

#[tokio::test]
async fn corrupt_rows_fail_closed_instead_of_dropping_content() -> Result<(), Box<dyn Error>> {
    let (store, _dir) = open_store()?;
    seed_exchanges(&store, 1, 1)?; // seqs 1..2
    // A user row carrying a non-text kind is outside the typed vocabulary.
    // It passes the DDL constraints, so the projection itself must refuse
    // the page — hydration never silently drops durable content.
    store
        .conn()
        .execute_batch("UPDATE messages SET kind = 'summary' WHERE seq = 1")?;
    let error = history::latest_history_page(&store, THREAD, 200).unwrap_err();
    assert!(
        error.to_string().contains("corrupt"),
        "non-text user row must fail closed, got {error}"
    );
    // A plan row missing its status column is equally corrupt once it exists;
    // it also poisons the page rather than being skipped.
    let (store2, _dir2) = open_store()?;
    seed_exchanges(&store2, 1, 1)?;
    store2.conn().execute_batch(
        "UPDATE messages SET kind = 'plan', plan_status = NULL, \
             plan_reviewed_at = NULL WHERE seq = 2",
    )?;
    let error = history::latest_history_page(&store2, THREAD, 200).unwrap_err();
    assert!(
        error.to_string().contains("corrupt"),
        "status-less plan must fail closed, got {error}"
    );
    Ok(())
}

#[tokio::test]
async fn failed_rows_stay_visible_and_older_pages_carry_no_summary() -> Result<(), Box<dyn Error>> {
    let (store, _dir) = open_store()?;
    seed_exchanges(&store, 2, 1)?;
    store_messages::insert(
        store.conn(),
        &store_messages::MessageRow {
            id: "failed-task".into(),
            thread_id: THREAD.into(),
            seq: 5,
            role: "assistant".into(),
            kind: "text".into(),
            content: "partial answer before failure".into(),
            status: "failed".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )?;
    let page = restart_history_page(&store, THREAD, 2)?;
    let statuses: Vec<AssistantStatus> = durable_entries(&page)
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::AssistantText { status, .. } => Some(*status),
            _ => None,
        })
        .collect();
    // The newest page of 2 carries the last seed assistant (Done) and the
    // failed row — interrupted/failed content is never dropped (C7).
    assert_eq!(
        statuses,
        vec![AssistantStatus::Done, AssistantStatus::Failed]
    );
    // The page reaches the thread's terminal tail (the failed row IS the
    // latest terminal assistant), so the S7 summary reference re-attaches
    // exactly there.
    assert!(
        page.entries
            .iter()
            .any(|entry| matches!(entry, HistoryEntry::Summary { .. }))
    );
    // A pure-read older page below the cursor never carries a summary
    // reference — it belongs to the newest page where its message lives.
    let older = history::history_page_before(
        &store,
        THREAD,
        PageCursor::Before(page.older_cursor.unwrap()),
        2,
    )?;
    assert!(
        !older
            .entries
            .iter()
            .any(|entry| matches!(entry, HistoryEntry::Summary { .. }))
    );
    Ok(())
}
