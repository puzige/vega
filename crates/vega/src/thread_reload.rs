use gpui::*;
use vega_conversation::history::HistoryPage;
use vega_conversation::types::*;
use vega_store::Store;
use vega_ui::conversation_stream::*;
use vega_ui::plan_card::PlanReviewRequested;

pub(crate) struct PlanReviewRefresh {
    pub(crate) thread: Thread,
    pub(crate) plans: Vec<Plan>,
    pub(crate) approved_instruction_id: Option<String>,
}

pub(crate) struct ThreadStateRefresh {
    pub(crate) thread: Thread,
    pub(crate) plans: Vec<Plan>,
    pub(crate) history: Vec<String>,
    pub(crate) recoverable_approved_instruction: Option<String>,
}

/// Persists the first-wins review. Only the committed approval winner returns
/// a durable instruction capability for the controller runner boundary.
pub(crate) fn persist_review(
    store: &Store,
    request: &PlanReviewRequested,
) -> Result<PlanReviewRefresh, String> {
    let outcome = vega_conversation::plans::review_plan(
        store,
        &request.thread_id,
        &request.plan_id,
        request.action.clone(),
    )
    .map_err(|error| error.to_string())?;
    let approved_instruction_id = match outcome {
        PlanReviewOutcome::Applied {
            instruction_message_id: Some(instruction_message_id),
        } => Some(instruction_message_id),
        PlanReviewOutcome::Applied {
            instruction_message_id: None,
        }
        | PlanReviewOutcome::Stale => None,
    };
    let thread = vega_conversation::threads::open_thread(store, &request.thread_id)
        .map_err(|error| error.to_string())?;
    let plans = vega_conversation::plans::list_plans(store, &request.thread_id)
        .map_err(|error| error.to_string())?;
    Ok(PlanReviewRefresh {
        thread,
        plans,
        approved_instruction_id,
    })
}

pub(crate) fn reload_thread_and_plans(
    store: &Store,
    thread_id: &str,
) -> Result<(Thread, Vec<Plan>), String> {
    let thread = vega_conversation::threads::open_thread(store, thread_id)
        .map_err(|error| error.to_string())?;
    let plans = vega_conversation::plans::list_plans(store, thread_id)
        .map_err(|error| error.to_string())?;
    Ok((thread, plans))
}

pub(crate) fn reload_thread_state(
    store: &Store,
    thread_id: &str,
) -> Result<ThreadStateRefresh, String> {
    let (thread, plans) = reload_thread_and_plans(store, thread_id)?;
    let history = vega_conversation::threads::composer_history(store, thread_id)
        .map_err(|error| error.to_string())?;
    let recoverable = vega_conversation::plans::recoverable_approved_instruction(store, thread_id)
        .map_err(|error| error.to_string())?;
    Ok(ThreadStateRefresh {
        thread,
        plans,
        history,
        recoverable_approved_instruction: recoverable,
    })
}

pub(crate) fn current_cache_matches(
    opened_thread_id: Option<&str>,
    cached_thread_id: Option<&str>,
    finished_thread_id: &str,
) -> bool {
    opened_thread_id == Some(finished_thread_id) && cached_thread_id == Some(finished_thread_id)
}

/// Outcome of one off-thread hydration page read (S8-T45/C7).
pub(crate) type HistoryPageOutcome = Result<HistoryPage, HistoryPageFailure>;

/// Typed hydration failure: the read failed closed with a store/IO reason.
/// Reaching the UI as a bare string keeps the stream free of SQLite types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryPageFailure {
    Store(String),
}

impl std::fmt::Display for HistoryPageFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HistoryPageFailure::Store(reason) => write!(formatter, "store error: {reason}"),
        }
    }
}

impl From<vega_conversation::types::ConversationError> for HistoryPageFailure {
    fn from(error: vega_conversation::types::ConversationError) -> Self {
        HistoryPageFailure::Store(error.to_string())
    }
}

/// Scroll-up hydration worker (S8-T45/C7): reads one keyset page below
/// `request.before` off the UI thread. The database connection is owned by
/// the store global on the main thread, so each request opens a short-lived
/// read connection to the same file; the store crate owns all SQLite and the
/// UI stays on typed projections only.
pub(crate) fn run_history_page_worker(
    database_path: std::path::PathBuf,
    request: HistoryPageRequested,
    sender: std::sync::mpsc::SyncSender<(HistoryPageRequested, HistoryPageOutcome)>,
) {
    let outcome = (|| {
        let store = Store::open(&database_path)
            .map_err(|error| HistoryPageFailure::Store(error.to_string()))?;
        vega_conversation::history::history_page_before(
            &store,
            &request.thread_id,
            vega_store::messages::PageCursor::Before(request.before),
            vega_store::messages::PAGE_LIMIT,
        )
        .map_err(HistoryPageFailure::from)
    })();
    let _ = sender.send((request, outcome));
}
