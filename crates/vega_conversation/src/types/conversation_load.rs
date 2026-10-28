pub struct ConversationLoad {
    pub history: crate::history::HistoryPage,
    pub plans: Vec<super::Plan>,
    pub composer_history: Vec<String>,
    pub approved_not_started: bool,
    pub usage: super::RestoredUsage,
    pub summary: Option<super::TaskCostSummary>,
    pub pending_model: Option<String>,
}
