use vega_store::Store;
use vega_store::messages::PageCursor;

use crate::history::{self, HistoryEntry, HistoryPage};
use crate::types::{
    ConversationError, HistoricalDelegationResult, HistoricalHistoryEntry, HistoricalHistoryPage,
    ToolCardInputProjection, ToolCardResultProjection,
};

fn has_archive(store: &Store, thread: &str) -> Result<bool, ConversationError> {
    vega_store::thread_fork::has_archived_delegation(store.conn(), thread)
        .map_err(|error| ConversationError::Store(error.to_string()))
}

pub fn restart_history_page(
    store: &Store,
    thread: &str,
    limit: usize,
) -> Result<HistoryPage, ConversationError> {
    if !has_archive(store, thread)? {
        return history::restart_history_page(store, thread, limit);
    }
    history::with_restart_recovery(store, thread, || latest_history_page(store, thread, limit))
}

pub fn latest_history_page(
    store: &Store,
    thread: &str,
    limit: usize,
) -> Result<HistoryPage, ConversationError> {
    if !has_archive(store, thread)? {
        return history::latest_history_page(store, thread, limit);
    }
    project(
        store,
        thread,
        history::latest_historical_history_page(store, thread, limit)?,
    )
}

pub fn history_page_before(
    store: &Store,
    thread: &str,
    cursor: PageCursor,
    limit: usize,
) -> Result<HistoryPage, ConversationError> {
    if !has_archive(store, thread)? {
        return history::history_page_before(store, thread, cursor, limit);
    }
    project(
        store,
        thread,
        history::historical_history_page_before(store, thread, cursor, limit)?,
    )
}

pub fn history_page_after(
    store: &Store,
    thread: &str,
    cursor: i64,
    limit: usize,
) -> Result<HistoryPage, ConversationError> {
    if !has_archive(store, thread)? {
        return history::history_page_after(store, thread, cursor, limit);
    }
    project(
        store,
        thread,
        history::historical_history_page_after(store, thread, cursor, limit)?,
    )
}

pub fn history_page_containing_message(
    store: &Store,
    thread: &str,
    message: &str,
    limit: usize,
) -> Result<Option<HistoryPage>, ConversationError> {
    if !has_archive(store, thread)? {
        return history::history_page_containing_message(store, thread, message, limit);
    }
    history::historical_history_page_containing_message(store, thread, message, limit)?
        .map(|page| project(store, thread, page))
        .transpose()
}

fn project(
    store: &Store,
    thread: &str,
    page: HistoricalHistoryPage,
) -> Result<HistoryPage, ConversationError> {
    let entries: Vec<_> = page.entries.into_iter().map(project_entry).collect();
    let subagents = crate::subagents::history(store, thread)?
        .into_iter()
        .filter(|agent| {
            entries.iter().any(|entry| match entry {
                HistoryEntry::AssistantText { message_id, .. }
                | HistoryEntry::Tool { message_id, .. }
                | HistoryEntry::AssistantResponseMetadata { message_id, .. } => {
                    *message_id == agent.parent_message_id
                }
                _ => false,
            })
        })
        .collect();
    Ok(HistoryPage {
        interrupted_tail: history::interrupted_tail(store, thread)?,
        subagents,
        entries,
        older_cursor: page.older_cursor,
        newer_cursor: page.newer_cursor,
        newest_seq: page.newest_seq,
    })
}

fn project_entry(entry: HistoricalHistoryEntry) -> HistoryEntry {
    match entry {
        HistoricalHistoryEntry::Ordinary(entry) => entry,
        HistoricalHistoryEntry::Delegation {
            seq,
            message_id,
            call_id,
            tool,
            input_json,
            status,
            approval,
            result,
        } => HistoryEntry::Tool {
            seq,
            message_id,
            call_id,
            description: None,
            status,
            approval,
            input: Some(ToolCardInputProjection::Subagent {
                tool: "historical_delegation".into(),
                title: vega_i18n::t_args("ui.fork_historical_delegation", &[("tool", &tool)]),
            }),
            result: Some(ToolCardResultProjection::Subagent {
                status,
                agent: None,
                output: historical_details(&input_json, result),
                reused: false,
            }),
        },
    }
}

fn historical_details(input: &str, result: HistoricalDelegationResult) -> String {
    let agents: Vec<_> = result
        .agents
        .into_iter()
        .map(|agent| {
            serde_json::json!({
                "historical_agent_id": agent.historical_agent_id,
                "historical_message_id": agent.historical_message_id,
                "name": agent.name,
                "state": agent.state,
                "result": agent.result,
                "checkpoint_json": agent.checkpoint_json,
                "release_state": agent.release_state,
            })
        })
        .collect();
    serde_json::json!({
        "input_json": input,
        "receipt": result.receipt,
        "agents": agents,
        "references": result.references,
    })
    .to_string()
}
