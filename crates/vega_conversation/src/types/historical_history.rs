use super::{Approval, ToolCallStatus};

pub use vega_store::thread_fork::HistoricalReference;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalAgentSummary {
    pub historical_agent_id: Option<String>,
    pub historical_message_id: Option<String>,
    pub name: Option<String>,
    pub state: Option<String>,
    pub result: Option<String>,
    pub checkpoint_json: Option<String>,
    pub release_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalDelegationResult {
    pub receipt: Option<String>,
    pub agents: Vec<HistoricalAgentSummary>,
    pub references: Vec<HistoricalReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoricalHistoryEntry {
    Ordinary(crate::history::HistoryEntry),
    Delegation {
        seq: i64,
        message_id: String,
        call_id: String,
        tool: String,
        input_json: String,
        status: ToolCallStatus,
        approval: Option<Approval>,
        result: HistoricalDelegationResult,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalHistoryPage {
    pub entries: Vec<HistoricalHistoryEntry>,
    pub older_cursor: Option<i64>,
    pub newer_cursor: Option<i64>,
    pub newest_seq: Option<i64>,
}
